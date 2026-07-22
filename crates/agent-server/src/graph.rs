//! Міст до графа: інтерактивний run вузла (спека runtime.md,
//! «Інтерактивна сесія = run вузла»; git.md — claim CAS, run ref, fenced
//! publish).
//!
//! Контракт графа НЕ реімплементується: всі операції — виклики `mt-core`
//! (та сама реалізація, яку `@7n/mt` використовує через napi). Життєвий
//! цикл: attach (CAS claim + detached worktree + run ref) → комміти ходів
//! із `session.jsonl` → `done` (fenced publish) або release (пауза).
//! `.nitra/` живе лише в run ref і прибирається перед publish — інваріант
//! git.md: у `main` він не потрапляє ніколи.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Duration, Utc};
use mt_core::claims::{
    acquire_claim, discover_repo_root, node_hash, release_claim, renew_or_takeover_claim,
    tasks_root_relative, ClaimFields, RUN_REF_PREFIX,
};
use mt_core::publish::{fenced_publish, PublishOutcome, PublishRequest};
use mt_core::worktree::{create_run_worktree, push_run_ref, remove_run_worktree};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Конфігурація моста.
#[derive(Debug, Clone)]
pub struct GraphConfig {
    /// tasks-директорія проєкту (напр. `<repo>/mt`).
    pub tasks_dir: PathBuf,
    /// Lease інтерактивного claim (спека: коротший за автономний;
    /// дефолт 0.3.0 — `interactive_claim_lease_sec: 900`).
    pub lease_sec: i64,
    /// Актор claim-а (інтерактивну сесію веде людина).
    pub actor: String,
}

impl GraphConfig {
    pub fn new(tasks_dir: PathBuf) -> Self {
        Self {
            tasks_dir,
            lease_sec: 900,
            actor: "human".into(),
        }
    }
}

/// Живий інтерактивний run: claim утримується, worktree матеріалізований.
#[derive(Debug)]
pub struct InteractiveRun {
    pub node: String,
    pub node_hash: String,
    /// = run_token сесії (ідентифікатор run ref).
    pub token: String,
    /// Поточний claim commit (renewal просуває).
    pub claim_sha: String,
    /// SHA `origin/main` на момент attach — база worktree, незмінна.
    pub base_sha: String,
    pub worktree: PathBuf,
    repo_root: PathBuf,
    tasks_root_rel: String,
    generation: u64,
    lease_sec: i64,
    actor: String,
    /// Верифіковані approvals цього run-а — матеріалізуються у
    /// `## Approvals` синтезованого `run_NNN.md` (access.md).
    approvals: Vec<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn iso(ts: chrono::DateTime<Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Тікет кооперативного handoff (runtime.md, «Міграція сесії між хостами»):
/// ідентифікує старий run ref, з якого новий хост відновлює worktree, і
/// generation, від якої продовжує лічильник claim-а (git.md: «новий хост:
/// create, generation+1» — попри те, що механічно це create-only CAS,
/// бо старий claim уже видалено). Serialize/Deserialize — тікет піде через
/// relay `HandoffRequest`-відповідь у наступній задачі.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffTicket {
    pub run_token: String,
    pub generation: u64,
}

/// Спільна реалізація attach/attach_resume: `resume_token` — `None` для
/// звичайного attach (worktree від `origin/main`), `Some(старий token)` —
/// worktree від tip старого run ref (журнал і мідфлайт-правки успадковані).
fn attach_impl(
    config: &GraphConfig,
    node: &str,
    generation: u64,
    resume_token: Option<&str>,
) -> Result<InteractiveRun, String> {
    let repo_root = discover_repo_root(&config.tasks_dir)?;
    let tasks_root_rel = tasks_root_relative(&repo_root, &config.tasks_dir)?;
    let hash = node_hash(&tasks_root_rel, node);

    git(&repo_root, &["fetch", "--quiet", "origin", "main"])?;
    let base_sha = git(&repo_root, &["rev-parse", "origin/main"])?;

    let worktree_base = match resume_token {
        None => base_sha.clone(),
        Some(old_token) => {
            let old_run_ref = format!("{RUN_REF_PREFIX}/{hash}/{old_token}");
            git(
                &repo_root,
                &[
                    "fetch",
                    "--quiet",
                    "origin",
                    &format!("+{old_run_ref}:{old_run_ref}"),
                ],
            )
            .map_err(|e| format!("attach-resume: старий run ref {old_run_ref} недоступний: {e}"))?;
            git(&repo_root, &["rev-parse", &old_run_ref])?
        }
    };

    let token = Uuid::new_v4().to_string();
    let runner_id = format!("agent-server/{}", std::process::id());
    let run_ref = format!("{RUN_REF_PREFIX}/{hash}/{token}");
    let fields = ClaimFields {
        node,
        actor: &config.actor,
        runner_id: &runner_id,
        claimed_at: &iso(Utc::now()),
        lease_until: &iso(Utc::now() + Duration::seconds(config.lease_sec)),
        token: &token,
        generation,
        base_sha: &base_sha,
        run_ref: &run_ref,
        interactive: true,
    };
    let claim = acquire_claim(&repo_root, &hash, &fields)?;
    if !claim.accepted {
        return Err(format!(
            "claim-lost: вузол {node} уже утримується іншим runner/сесією"
        ));
    }

    let worktrees_dir = repo_root.join(".worktrees");
    let worktree = create_run_worktree(&repo_root, &worktrees_dir, &hash, &token, &worktree_base)?;
    push_run_ref(&worktree, &hash, &token)?;

    Ok(InteractiveRun {
        node: node.to_string(),
        node_hash: hash,
        token,
        claim_sha: claim.commit_sha,
        base_sha,
        worktree,
        repo_root,
        tasks_root_rel,
        generation,
        lease_sec: config.lease_sec,
        actor: config.actor.clone(),
        approvals: Vec::new(),
    })
}

/// Attach вузла: CAS claim → detached worktree від `base_sha` → run ref.
/// `accepted: false` CAS-у → явна помилка claim-lost (вузол уже зайнято).
pub fn attach(config: &GraphConfig, node: &str) -> Result<InteractiveRun, String> {
    attach_impl(config, node, 1, None)
}

/// Відновлення на новому хості після кооперативного `handoff`
/// (runtime.md, кроки 2-3): CAS-create claim (generation = `ticket` + 1) →
/// worktree ЗІ СТАНУ старого run ref (не `origin/main`) — журнал і
/// мідфлайт-правки успадковані → push нового run ref. Недоступний старий
/// run ref (втрачено/типо у тікеті) → явна помилка, не паніка.
pub fn attach_resume(
    config: &GraphConfig,
    node: &str,
    ticket: &HandoffTicket,
) -> Result<InteractiveRun, String> {
    attach_impl(config, node, ticket.generation + 1, Some(&ticket.run_token))
}

impl InteractiveRun {
    /// Поточна генерація claim-а (fencing token для side effects).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Додає верифікований approval-рядок (пише ws-обробник після
    /// успішної перевірки підпису гейтом).
    pub fn add_approval(&mut self, line: String) {
        self.approvals.push(line);
    }

    /// Коміт ходу: журнал сесії (`.nitra/session.jsonl`) + правки файлів →
    /// push run ref (recovery/handoff, спека git.md: «кожен хід = коміт +
    /// негайний push run ref»). Порожній хід (нічого не змінилось) — no-op.
    pub fn commit_turn(&self, session_jsonl: &str, message: &str) -> Result<(), String> {
        let nitra_dir = self.worktree.join(".nitra");
        std::fs::create_dir_all(&nitra_dir).map_err(|e| e.to_string())?;
        std::fs::write(nitra_dir.join("session.jsonl"), session_jsonl)
            .map_err(|e| e.to_string())?;

        git(&self.worktree, &["add", "-A"])?;
        let staged = git(&self.worktree, &["status", "--porcelain"])?;
        if staged.is_empty() {
            return Ok(());
        }
        git(&self.worktree, &["commit", "-q", "-m", message])?;
        push_run_ref(&self.worktree, &self.node_hash, &self.token)
    }

    /// Renewal lease: той самий token/generation, CAS від поточного claim
    /// SHA. `Ok(false)` — claim втрачено (takeover-ом), сесію слід зупинити.
    pub fn renew(&mut self) -> Result<bool, String> {
        let run_ref = format!("{RUN_REF_PREFIX}/{}/{}", self.node_hash, self.token);
        let fields = ClaimFields {
            node: &self.node,
            actor: &self.actor.clone(),
            runner_id: &format!("agent-server/{}", std::process::id()),
            claimed_at: &iso(Utc::now()),
            lease_until: &iso(Utc::now() + Duration::seconds(self.lease_sec)),
            token: &self.token.clone(),
            generation: self.generation,
            base_sha: &self.base_sha.clone(),
            run_ref: &run_ref,
            interactive: true,
        };
        let push =
            renew_or_takeover_claim(&self.repo_root, &self.node_hash, &self.claim_sha, &fields)?;
        if push.accepted {
            self.claim_sha = push.commit_sha;
        }
        Ok(push.accepted)
    }

    /// Синтез контрактних артефактів спроби (graph.md): `run_NNN.md`
    /// (actor, result success, `## Approvals` за наявності) і мінімальний
    /// `fact_NNN.md`, якщо виконавець не створив власний — без fact вузол
    /// після publish не стає resolved.
    fn write_run_artifacts(&self) -> Result<(), String> {
        let dir = self.worktree.join(&self.tasks_root_rel).join(&self.node);
        let nnn = mt_core::nnn::pad_nnn(mt_core::signal::next_run_nnn(&dir));

        let fact_path = dir.join(format!("fact_{nnn}.md"));
        if !fact_path.exists() {
            let fact = format!(
                "---\nschema_version: 1\ncreated_at: {}\n---\n\n## Summary\n\n\
                 Інтерактивний run завершено (mt done); журнал сесії — у run ref.\n",
                iso(Utc::now())
            );
            std::fs::write(&fact_path, fact).map_err(|e| e.to_string())?;
        }

        let sections = if self.approvals.is_empty() {
            "\n".to_string()
        } else {
            format!("\n## Approvals\n\n{}\n", self.approvals.join("\n"))
        };
        mt_core::signal::write_run_fm(&dir, &nnn, &self.actor, "success", &sections, "")?;
        Ok(())
    }

    /// `mt done`: гейт `## Check` (контракт graph.md — fail → відмова
    /// сигналу, run лишається живим) → синтез `run_NNN.md`/`fact_NNN.md` →
    /// стрип `.nitra/` з індексу (інваріант git.md) → fenced publish
    /// (rebase на origin/main + atomic push main / видалення claim+run
    /// ref). Успіх → worktree прибирається.
    pub fn done(&self, retry_max: u32, base_ms: u64) -> Result<PublishOutcome, String> {
        // ## Check вузла ганяється у worktree (cwd = корінь worktree —
        // батько tasks-директорії, як у автономного wrapper-а).
        let wt_tasks_dir = self.worktree.join(&self.tasks_root_rel);
        mt_core::signal::run_check(&wt_tasks_dir.to_string_lossy(), &self.node)?;

        // Remote run ref стоїть на останньому запушеному ході (HEAD ДО
        // артефакт/strip-комітів) — саме його очікує force-with-lease.
        let run_ref_sha = git(&self.worktree, &["rev-parse", "HEAD"])?;

        self.write_run_artifacts()?;
        git(&self.worktree, &["add", "-A"])?;
        let staged = git(&self.worktree, &["status", "--porcelain"])?;
        if !staged.is_empty() {
            git(
                &self.worktree,
                &[
                    "commit",
                    "-q",
                    "-m",
                    &format!("mt: {} run (success)", self.node),
                ],
            )?;
        }
        let tracked = git(&self.worktree, &["ls-files", ".nitra"])?;
        if !tracked.is_empty() {
            git(&self.worktree, &["rm", "-r", "-q", "--cached", ".nitra"])?;
            git(
                &self.worktree,
                &["commit", "-q", "-m", "mt: strip session artifacts"],
            )?;
        }
        let request = PublishRequest {
            worktree: &self.worktree,
            node_hash: &self.node_hash,
            claim_sha: &self.claim_sha,
            token: &self.token,
            run_ref_sha_before: &run_ref_sha,
        };
        let outcome = fenced_publish(&self.repo_root, &request, retry_max, base_ms)?;
        if outcome.published {
            let _ = remove_run_worktree(&self.repo_root, &self.worktree);
        }
        // Не published → worktree/run ref лишаються для debug (спека,
        // «Failure-сімейство»).
        Ok(outcome)
    }

    /// Пауза/відпустити: CAS-delete claim + прибрати worktree; run ref
    /// лишається (журнал сесії — база відновлення наступного attach).
    pub fn release(self) -> Result<bool, String> {
        let released = release_claim(&self.repo_root, &self.node_hash, &self.claim_sha)?;
        let _ = remove_run_worktree(&self.repo_root, &self.worktree);
        Ok(released)
    }

    /// Кооперативний handoff (git.md, claim-операція `handoff`; runtime.md,
    /// «Міграція сесії між хостами», крок 2): синтезує `run_NNN.md
    /// (result: handoff)` → коміт → push run ref БЕЗ стрипу `.nitra/` —
    /// повний журнал розмови їде разом (checkpoint-режим із дистильованим
    /// summary — окрема задача) → CAS-delete claim. Повертає тікет для
    /// `attach_resume` на новому хості.
    pub fn handoff(self) -> Result<HandoffTicket, String> {
        let dir = self.worktree.join(&self.tasks_root_rel).join(&self.node);
        let nnn = mt_core::nnn::pad_nnn(mt_core::signal::next_run_nnn(&dir));
        mt_core::signal::write_run_fm(&dir, &nnn, &self.actor, "handoff", "\n", "")?;

        git(&self.worktree, &["add", "-A"])?;
        let staged = git(&self.worktree, &["status", "--porcelain"])?;
        if !staged.is_empty() {
            git(
                &self.worktree,
                &["commit", "-q", "-m", &format!("mt: {} handoff", self.node)],
            )?;
        }
        push_run_ref(&self.worktree, &self.node_hash, &self.token)?;

        let ticket = HandoffTicket {
            run_token: self.token.clone(),
            generation: self.generation,
        };
        release_claim(&self.repo_root, &self.node_hash, &self.claim_sha)?;
        let _ = remove_run_worktree(&self.repo_root, &self.worktree);
        Ok(ticket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Герметична фікстура: bare-репо як origin + робочий клон із tasks-
    /// директорією `mt/demo` на `main` (патерн mt-core test_support).
    struct Fixture {
        #[allow(dead_code)]
        origin: tempfile::TempDir,
        work: tempfile::TempDir,
    }

    fn sh(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "t@t.local")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "t@t.local")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    impl Fixture {
        fn new() -> Self {
            let origin = tempfile::tempdir().unwrap();
            sh(origin.path(), &["init", "--bare", "-q", "-b", "main"]);
            let work = tempfile::tempdir().unwrap();
            sh(work.path(), &["init", "-q", "-b", "main"]);
            std::fs::create_dir_all(work.path().join("mt/demo")).unwrap();
            std::fs::write(work.path().join("mt/demo/task.md"), "## Task\n").unwrap();
            sh(work.path(), &["add", "."]);
            sh(work.path(), &["commit", "-q", "-m", "init"]);
            sh(
                work.path(),
                &["remote", "add", "origin", origin.path().to_str().unwrap()],
            );
            sh(work.path(), &["push", "-q", "origin", "main"]);
            Self { origin, work }
        }

        fn config(&self) -> GraphConfig {
            GraphConfig::new(self.work.path().join("mt"))
        }

        fn remote_refs(&self) -> String {
            super::git(self.work.path(), &["ls-remote", "origin"]).unwrap()
        }
    }

    /// attach: claim ref + run ref на remote, detached worktree від base_sha.
    #[test]
    fn attach_claims_and_materializes_worktree() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();

        assert!(run.worktree.exists());
        let refs = fixture.remote_refs();
        assert!(
            refs.contains(&format!("refs/mt/claims/{}", run.node_hash)),
            "{refs}"
        );
        assert!(
            refs.contains(&format!("refs/mt/runs/{}/{}", run.node_hash, run.token)),
            "{refs}"
        );
        let head = super::git(&run.worktree, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head, run.base_sha, "worktree від base_sha (origin/main)");

        // Claim позначений інтерактивним (0.3.0, ADR 260711-2100).
        let claim_yaml = super::git(
            Path::new(fixture.origin.path()),
            &[
                "show",
                &format!("refs/mt/claims/{}:.mt-claim.yml", run.node_hash),
            ],
        )
        .unwrap();
        assert!(claim_yaml.contains("interactive: true"), "{claim_yaml}");
    }

    /// Другий attach того самого вузла — claim-lost, не системна помилка.
    #[test]
    fn second_attach_is_claim_lost() {
        let fixture = Fixture::new();
        let _held = attach(&fixture.config(), "demo").unwrap();
        let error = attach(&fixture.config(), "demo").unwrap_err();
        assert!(error.contains("claim-lost"), "{error}");
    }

    /// commit_turn пише журнал у run ref; done стрипає .nitra/ і публікує
    /// fact у main; claim/run ref прибрані.
    #[test]
    fn turn_then_done_publishes_without_session_artifacts() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();

        // Хід: результатний файл + журнал сесії.
        std::fs::write(run.worktree.join("mt/demo/fact_001.md"), "## Summary\nok\n").unwrap();
        run.commit_turn("{\"seq\":0}\n", "mt: demo run 001 (хід 1)")
            .unwrap();

        // Журнал доїхав у run ref.
        let run_ref = format!("refs/mt/runs/{}/{}", run.node_hash, run.token);
        let journal = super::git(
            fixture.work.path(),
            &["show", &format!("{run_ref}:.nitra/session.jsonl")],
        );
        // ls-remote бачить ref, а show читає локальний — worktree пушить
        // напряму в origin; читаємо з origin.
        let origin_journal = super::git(
            Path::new(fixture.origin.path()),
            &["show", &format!("{run_ref}:.nitra/session.jsonl")],
        )
        .unwrap();
        assert_eq!(origin_journal, "{\"seq\":0}");
        drop(journal);

        let node_hash = run.node_hash.clone();
        let outcome = run.done(3, 10).unwrap();
        assert!(outcome.published, "{outcome:?}");

        // main просунувся, fact є, .nitra/ немає, claim/run ref прибрані.
        let main_files = super::git(
            Path::new(fixture.origin.path()),
            &["ls-tree", "-r", "--name-only", "main"],
        )
        .unwrap();
        assert!(main_files.contains("mt/demo/fact_001.md"), "{main_files}");
        assert!(
            !main_files.contains(".nitra"),
            ".nitra/ не мусить потрапити у main: {main_files}"
        );
        let refs = fixture.remote_refs();
        assert!(
            !refs.contains(&format!("refs/mt/claims/{node_hash}")),
            "{refs}"
        );
        assert!(!refs.contains("refs/mt/runs/"), "{refs}");
    }

    /// `## Check`-гейт: падаюча перевірка → відмова done (claim/worktree
    /// живі); після виправлення той самий run публікується.
    #[test]
    fn failing_check_blocks_done_until_fixed() {
        let fixture = Fixture::new();
        // Вузол із Check: вимагає файл ready у директорії вузла.
        std::fs::write(
            fixture.work.path().join("mt/demo/task.md"),
            "## Task\n\n## Check\n\ntest -f mt/demo/ready\n",
        )
        .unwrap();
        sh(fixture.work.path(), &["add", "."]);
        sh(fixture.work.path(), &["commit", "-q", "-m", "check"]);
        sh(fixture.work.path(), &["push", "-q", "origin", "main"]);

        let run = attach(&fixture.config(), "demo").unwrap();
        run.commit_turn("{}\n", "mt: хід").unwrap();

        let error = run.done(3, 10).unwrap_err();
        assert!(error.contains("## Check failed"), "{error}");
        assert!(
            fixture.remote_refs().contains("refs/mt/claims/"),
            "claim живий після відмови Check"
        );

        // Виправлення у worktree → done проходить.
        std::fs::write(run.worktree.join("mt/demo/ready"), "ok").unwrap();
        run.commit_turn("{}\n", "mt: виправлення").unwrap();
        let outcome = run.done(3, 10).unwrap();
        assert!(outcome.published, "{outcome:?}");
    }

    /// done синтезує контрактні артефакти: run_001.md з ## Approvals і
    /// мінімальний fact_001.md — обидва доїжджають у main.
    #[test]
    fn done_synthesizes_run_and_fact_artifacts() {
        let fixture = Fixture::new();
        let mut run = attach(&fixture.config(), "demo").unwrap();
        run.add_approval(
            "- 2026-07-12T00:00:00Z device=phone approved=true request=req-1 signature=ab".into(),
        );
        run.commit_turn("{}\n", "mt: хід").unwrap();

        let outcome = run.done(3, 10).unwrap();
        assert!(outcome.published, "{outcome:?}");

        let run_md = super::git(
            Path::new(fixture.origin.path()),
            &["show", "main:mt/demo/run_001.md"],
        )
        .unwrap();
        assert!(run_md.starts_with("---\nschema_version: 1"), "{run_md}");
        assert!(run_md.contains("actor: human"), "{run_md}");
        assert!(run_md.contains("result: success"), "{run_md}");
        assert!(run_md.contains("## Approvals"), "{run_md}");
        assert!(run_md.contains("request=req-1"), "{run_md}");

        let fact_md = super::git(
            Path::new(fixture.origin.path()),
            &["show", "main:mt/demo/fact_001.md"],
        )
        .unwrap();
        assert!(fact_md.contains("## Summary"), "{fact_md}");
    }

    /// Власний fact виконавця з тим самим NNN не перезаписується синтезом.
    #[test]
    fn executor_fact_is_preserved() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();
        std::fs::write(
            run.worktree.join("mt/demo/fact_001.md"),
            "## Summary\n\nвласний fact виконавця\n",
        )
        .unwrap();
        run.commit_turn("{}\n", "mt: fact від виконавця").unwrap();

        assert!(run.done(3, 10).unwrap().published);

        let fact_md = super::git(
            Path::new(fixture.origin.path()),
            &["show", "main:mt/demo/fact_001.md"],
        )
        .unwrap();
        assert!(fact_md.contains("власний fact виконавця"), "{fact_md}");
    }

    /// renew просуває claim SHA і лишає ownership за нами.
    #[test]
    fn renew_extends_lease() {
        let fixture = Fixture::new();
        let mut run = attach(&fixture.config(), "demo").unwrap();
        let before = run.claim_sha.clone();
        assert!(run.renew().unwrap());
        assert_ne!(run.claim_sha, before, "renewal — новий claim commit");
        // Після renewal вузол досі зайнятий.
        assert!(attach(&fixture.config(), "demo").is_err());
    }

    /// release: claim знято (вузол знову вільний), run ref лишається.
    #[test]
    fn release_frees_node_and_keeps_run_ref() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();
        run.commit_turn("{\"seq\":0}\n", "mt: журнал").unwrap();
        let token = run.token.clone();
        let node_hash = run.node_hash.clone();

        assert!(run.release().unwrap());

        let refs = fixture.remote_refs();
        assert!(
            !refs.contains(&format!("refs/mt/claims/{node_hash}")),
            "{refs}"
        );
        assert!(
            refs.contains(&format!("refs/mt/runs/{node_hash}/{token}")),
            "run ref — база відновлення: {refs}"
        );
        // Вузол знову можна attach-нути.
        assert!(attach(&fixture.config(), "demo").is_ok());
    }

    /// handoff: run-файл result:handoff з повним журналом (.nitra/
    /// НЕ стрипається — checkpoint-режим поза скоупом), claim знято,
    /// worktree прибрано, run ref лишається (база attach_resume).
    #[test]
    fn handoff_writes_marker_and_frees_claim() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();
        run.commit_turn("{\"seq\":0}\n", "mt: перший хід").unwrap();
        let node_hash = run.node_hash.clone();
        let old_token = run.token.clone();
        let worktree = run.worktree.clone();

        let ticket = run.handoff().unwrap();

        assert_eq!(ticket.run_token, old_token);
        assert_eq!(ticket.generation, 1);
        assert!(!worktree.exists(), "worktree прибрано після handoff");

        let refs = fixture.remote_refs();
        assert!(
            !refs.contains(&format!("refs/mt/claims/{node_hash}")),
            "claim знято: {refs}"
        );
        let run_ref = format!("refs/mt/runs/{node_hash}/{old_token}");
        assert!(refs.contains(&run_ref), "run ref лишається: {refs}");

        let run_md = super::git(
            Path::new(fixture.origin.path()),
            &["show", &format!("{run_ref}:mt/demo/run_001.md")],
        )
        .unwrap();
        assert!(run_md.contains("result: handoff"), "{run_md}");
        let journal = super::git(
            Path::new(fixture.origin.path()),
            &["show", &format!("{run_ref}:.nitra/session.jsonl")],
        )
        .unwrap();
        assert_eq!(
            journal, "{\"seq\":0}",
            "повний журнал (без checkpoint-стрипу): {journal}"
        );
    }

    /// attach_resume: claim CAS-create (generation = old+1), worktree
    /// успадковує мідфлайт-правки і журнал старого run ref, новий run ref
    /// існує.
    #[test]
    fn attach_resume_inherits_worktree_and_bumps_generation() {
        let fixture = Fixture::new();
        let run = attach(&fixture.config(), "demo").unwrap();
        std::fs::write(run.worktree.join("mt/demo/draft.md"), "мідфлайт").unwrap();
        run.commit_turn("{\"seq\":0}\n", "mt: чернетка").unwrap();
        let ticket = run.handoff().unwrap();

        let resumed = attach_resume(&fixture.config(), "demo", &ticket).unwrap();

        assert_eq!(resumed.generation(), ticket.generation + 1);
        assert_ne!(resumed.token, ticket.run_token, "новий token сесії");
        assert_eq!(
            std::fs::read_to_string(resumed.worktree.join("mt/demo/draft.md")).unwrap(),
            "мідфлайт",
            "мідфлайт-правка успадкована у новому worktree"
        );
        assert_eq!(
            std::fs::read_to_string(resumed.worktree.join(".nitra/session.jsonl")).unwrap(),
            "{\"seq\":0}\n",
            "журнал сесії успадкований"
        );
        let refs = fixture.remote_refs();
        assert!(
            refs.contains(&format!(
                "refs/mt/runs/{}/{}",
                resumed.node_hash, resumed.token
            )),
            "новий run ref: {refs}"
        );
        assert!(
            refs.contains(&format!("refs/mt/claims/{}", resumed.node_hash)),
            "новий claim: {refs}"
        );
    }

    /// attach_resume із тікетом на неіснуючий run ref → явна помилка,
    /// не паніка.
    #[test]
    fn attach_resume_missing_run_ref_is_explicit_error() {
        let fixture = Fixture::new();
        let ticket = HandoffTicket {
            run_token: "no-such-token".into(),
            generation: 1,
        };
        let error = attach_resume(&fixture.config(), "demo", &ticket).unwrap_err();
        assert!(error.contains("недоступний"), "{error}");
    }

    /// Наскрізно: attach → хід → handoff → attach_resume → done — публікує
    /// ту саму серію NNN без розривів (генерація продовжена через handoff).
    #[test]
    fn full_handoff_cycle_publishes_without_nnn_gap() {
        let fixture = Fixture::new();
        let first = attach(&fixture.config(), "demo").unwrap();
        first
            .commit_turn("{\"seq\":0}\n", "mt: перший хід")
            .unwrap();
        let ticket = first.handoff().unwrap();

        let second = attach_resume(&fixture.config(), "demo", &ticket).unwrap();
        second
            .commit_turn("{\"seq\":0}\n{\"seq\":1}\n", "mt: другий хід")
            .unwrap();
        let node_hash = second.node_hash.clone();
        let second_token = second.token.clone();
        let outcome = second.done(3, 10).unwrap();
        assert!(outcome.published, "{outcome:?}");

        // run_001.md — handoff-маркер першого хосту; run_002.md — success
        // від другого. Без розривів NNN попри зміну хоста.
        let main_files = super::git(
            Path::new(fixture.origin.path()),
            &["ls-tree", "-r", "--name-only", "main"],
        )
        .unwrap();
        assert!(main_files.contains("mt/demo/run_002.md"), "{main_files}");
        assert!(main_files.contains("mt/demo/fact_002.md"), "{main_files}");
        assert!(!main_files.contains(".nitra"), "{main_files}");

        let refs = fixture.remote_refs();
        assert!(
            !refs.contains(&format!("refs/mt/claims/{node_hash}")),
            "claim прибрано: {refs}"
        );
        assert!(
            !refs.contains(&format!("refs/mt/runs/{node_hash}/{second_token}")),
            "новий run ref прибрано fenced publish-ом: {refs}"
        );
        // Handoff-run ref першого хосту навмисно лишається (не-checkpoint
        // режим не архівує журнал; GC орфанованих run ref-ів після done —
        // окрема задача, аналог `mt cleanup`).
        assert!(
            refs.contains(&format!("refs/mt/runs/{node_hash}/{}", ticket.run_token)),
            "{refs}"
        );
    }
}
