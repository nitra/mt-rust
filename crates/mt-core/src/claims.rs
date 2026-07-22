//! Remote execution claims (спека mt.md, «Authoritative execution claim»).
//!
//! Ownership вузла живе у GitHub custom refs `refs/mt/claims/<node-hash>`;
//! claim ref вказує на commit із `.mt-claim.yml`. Модуль дає read-модель:
//! node-hash, читання remote claims через git CLI і зіставлення з вузлами.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::frontmatter::parse_yaml;

/// Префікс claim refs (дефолт `.mt.json` → `claim_ref_prefix`).
pub const CLAIM_REF_PREFIX: &str = "refs/mt/claims";

/// `node-hash` = перші 20 hex символів SHA-256 від `<tasks-root>\0<node-path>`.
/// `tasks_root` — канонічний шлях tasks-директорії відносно git root (напр.
/// `mt` або `packages/api/mt`), `node_path` — вузол відносно tasks root.
pub fn node_hash(tasks_root: &str, node_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tasks_root.as_bytes());
    hasher.update([0u8]);
    hasher.update(node_path.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(20);
    for byte in digest.iter() {
        if hex.len() >= 20 {
            break;
        }
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(20);
    hex
}

/// Префікс run refs (спека: `refs/mt/runs/<node-hash>/<token>`).
pub const RUN_REF_PREFIX: &str = "refs/mt/runs";

/// Git top-level, що містить `tasks_dir` (`git rev-parse --show-toplevel`).
pub fn discover_repo_root(tasks_dir: &Path) -> Result<PathBuf, String> {
    let out = git(tasks_dir, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(out.trim()))
}

/// Канонічний шлях `tasks_dir` відносно `repo_root`, POSIX-нормалізований
/// (`\` → `/`) — вхід для [`node_hash`] (спека: `<tasks-root>\0<node-path>`).
pub fn tasks_root_relative(repo_root: &Path, tasks_dir: &Path) -> Result<String, String> {
    let repo_root = repo_root
        .canonicalize()
        .map_err(|e| format!("repo root {}: {e}", repo_root.display()))?;
    let tasks_dir = tasks_dir
        .canonicalize()
        .map_err(|e| format!("tasks dir {}: {e}", tasks_dir.display()))?;
    let rel = tasks_dir
        .strip_prefix(&repo_root)
        .map_err(|_| "tasks dir escapes its git repository".to_string())?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Один запис `git ls-remote origin 'refs/mt/claims/*'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteClaimRef {
    pub node_hash: String,
    pub sha: String,
}

/// Парсить вивід `git ls-remote` (рядки `<sha>\t<ref>`), лишаючи claim refs.
pub fn parse_ls_remote(output: &str, prefix: &str) -> Vec<RemoteClaimRef> {
    let mut refs = Vec::new();
    for line in output.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(hash) = name.strip_prefix(prefix).and_then(|r| r.strip_prefix('/')) else {
            continue;
        };
        if !sha.is_empty() && !hash.is_empty() && !hash.contains('/') {
            refs.push(RemoteClaimRef {
                node_hash: hash.to_string(),
                sha: sha.to_string(),
            });
        }
    }
    refs
}

/// Розібраний `.mt-claim.yml` claim-коміта.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimInfo {
    pub node_hash: String,
    /// `node:` з claim-файлу (шлях відносно tasks root; інформативний).
    pub node: Option<String>,
    pub actor: Option<String>,
    pub runner_id: Option<String>,
    pub lease_until: Option<String>,
    /// Lease прострочений (з урахуванням grace) → derived-стан `stalled`.
    pub expired: bool,
    /// Інтерактивна сесія тримає claim; відсутнє поле (старі claim-и 0.2.x)
    /// → `false` (ADR 260711-2100).
    pub interactive: bool,
}

fn yaml_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

/// Чи прострочений lease: `lease_until + grace_sec ≤ now`. Непарсибельний
/// або відсутній `lease_until` вважаємо простроченим (консервативно).
pub fn lease_expired(lease_until: Option<&str>, grace_sec: i64, now: DateTime<Utc>) -> bool {
    let Some(until) = lease_until.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) else {
        return true;
    };
    until.with_timezone(&Utc) + chrono::Duration::seconds(grace_sec) <= now
}

/// Будує [`ClaimInfo`] з YAML-вмісту `.mt-claim.yml`.
pub fn parse_claim(node_hash: &str, yaml: &str, grace_sec: i64, now: DateTime<Utc>) -> ClaimInfo {
    let v = parse_yaml(yaml);
    let lease_until = yaml_str(&v, "lease_until");
    ClaimInfo {
        node_hash: node_hash.to_string(),
        node: yaml_str(&v, "node"),
        actor: yaml_str(&v, "actor"),
        runner_id: yaml_str(&v, "runner_id"),
        expired: lease_expired(lease_until.as_deref(), grace_sec, now),
        lease_until,
        interactive: v
            .get("interactive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
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
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Поля `.mt-claim.yml`, які runner контролює при acquire/renew/takeover.
pub struct ClaimFields<'a> {
    pub node: &'a str,
    pub actor: &'a str,
    pub runner_id: &'a str,
    pub claimed_at: &'a str,
    pub lease_until: &'a str,
    pub token: &'a str,
    pub generation: u64,
    /// SHA `origin/main` на момент першого claim — фіксується назавжди,
    /// незалежно від наступних renewal/takeover (parent першого коміту).
    pub base_sha: &'a str,
    pub run_ref: &'a str,
    /// Інтерактивна сесія (attach) замість автономного run-а (0.3.0,
    /// git.md «Claim»: `token = session_id`, коротший lease; політики
    /// watchdog/бюджетів мʼякші). ADR 260711-2100.
    pub interactive: bool,
}

fn claim_yaml(f: &ClaimFields) -> String {
    format!(
        "schema_version: 1\nnode: {}\nactor: {}\nrunner_id: {}\nclaimed_at: {}\n\
         lease_until: {}\ntoken: {}\ngeneration: {}\nbase_sha: {}\nrun_ref: {}\ninteractive: {}\n",
        f.node,
        f.actor,
        f.runner_id,
        f.claimed_at,
        f.lease_until,
        f.token,
        f.generation,
        f.base_sha,
        f.run_ref,
        f.interactive
    )
}

/// Як [`git`], але пише `stdin` у дочірній процес (для `hash-object`/`mktree`).
fn git_stdin(repo: &Path, args: &[&str], stdin: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(stdin.as_bytes())
        .map_err(|e| format!("git {}: write stdin: {e}", args.join(" ")))?;
    let out = child
        .wait_with_output()
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

/// Пише claim-коміт (blob → tree → commit-tree) без checkout/індексу —
/// придатне для headless runner без робочого дерева проєкту. `parent` —
/// `base_sha` для першого claim, попередній claim-коміт для renew/takeover
/// (спека: «Новий claim commit має parent = попередній claim commit»).
fn create_claim_commit(repo: &Path, parent: &str, fields: &ClaimFields) -> Result<String, String> {
    let blob_sha = git_stdin(repo, &["hash-object", "-w", "--stdin"], &claim_yaml(fields))?;
    let tree_entry = format!("100644 blob {blob_sha}\t.mt-claim.yml\n");
    let tree_sha = git_stdin(repo, &["mktree"], &tree_entry)?;
    let message = format!("mt: claim {}", fields.node);
    let commit_sha = git(
        repo,
        &["commit-tree", &tree_sha, "-p", parent, "-m", &message],
    )?;
    Ok(commit_sha.trim().to_string())
}

/// Підсумок CAS-push claim ref. `accepted: false` — інший runner виграв
/// гонку (нормальний результат гонки, не помилка транспорту/мережі).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimPush {
    pub accepted: bool,
    pub commit_sha: String,
}

/// Розрізняє "інший runner виграв гонку" (force-with-lease rejection) від
/// системної помилки (мережа/автентифікація) за текстом stderr git push.
fn is_lease_rejection(stderr: &str) -> bool {
    stderr.contains("stale info")
        || stderr.contains("[rejected]")
        || stderr.contains("already exists")
        || stderr.contains("fetch first")
}

fn push_claim_ref(
    repo: &Path,
    node_hash: &str,
    new_sha: &str,
    expected: Option<&str>,
) -> Result<bool, String> {
    let refname = format!("{CLAIM_REF_PREFIX}/{node_hash}");
    let lease = format!("--force-with-lease={refname}:{}", expected.unwrap_or(""));
    let refspec = format!("{new_sha}:{refname}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", &lease, "origin", &refspec])
        .output()
        .map_err(|e| format!("git push claim: {e}"))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_lease_rejection(&stderr) {
        return Ok(false);
    }
    Err(format!("git push claim: {}", stderr.trim()))
}

/// Create-only CAS (спека, крок 3): приймається лише якщо `refs/mt/claims/<hash>`
/// на remote ще не існує. Лише accepted push дає право створити worktree.
pub fn acquire_claim(
    repo: &Path,
    node_hash: &str,
    fields: &ClaimFields,
) -> Result<ClaimPush, String> {
    let commit_sha = create_claim_commit(repo, fields.base_sha, fields)?;
    let accepted = push_claim_ref(repo, node_hash, &commit_sha, None)?;
    Ok(ClaimPush {
        accepted,
        commit_sha,
    })
}

/// Renewal/takeover (спека, крок 5): CAS лише з exact `old_claim_sha`. Той
/// самий виклик покриває і renewal (той самий `token`/`generation` у `fields`),
/// і takeover (новий `token`, `generation + 1`) — розрізняє лише вміст `fields`.
pub fn renew_or_takeover_claim(
    repo: &Path,
    node_hash: &str,
    old_claim_sha: &str,
    fields: &ClaimFields,
) -> Result<ClaimPush, String> {
    let commit_sha = create_claim_commit(repo, old_claim_sha, fields)?;
    let accepted = push_claim_ref(repo, node_hash, &commit_sha, Some(old_claim_sha))?;
    Ok(ClaimPush {
        accepted,
        commit_sha,
    })
}

/// CAS-видалення claim ref після fenced publish — лише якщо runner досі
/// власник exact `claim_sha`. `accepted: false` не є помилкою: означає, що
/// claim вже загублено (takeover), publish цього runner-а мав бути fenced.
pub fn release_claim(repo: &Path, node_hash: &str, claim_sha: &str) -> Result<bool, String> {
    let refname = format!("{CLAIM_REF_PREFIX}/{node_hash}");
    let lease = format!("--force-with-lease={refname}:{claim_sha}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", &lease, "origin", &format!(":{refname}")])
        .output()
        .map_err(|e| format!("git push --delete claim: {e}"))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_lease_rejection(&stderr) {
        return Ok(false);
    }
    Err(format!("git push --delete claim: {}", stderr.trim()))
}

/// Читає remote claims: `ls-remote` → fetch claim refs → `.mt-claim.yml` з
/// кожного claim-коміта. `grace_sec` — буфер перед takeover (`claim_grace_sec`).
pub fn fetch_remote_claims(repo_root: &Path, grace_sec: i64) -> Result<Vec<ClaimInfo>, String> {
    let ls = git(
        repo_root,
        &["ls-remote", "origin", &format!("{CLAIM_REF_PREFIX}/*")],
    )?;
    let refs = parse_ls_remote(&ls, CLAIM_REF_PREFIX);
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    // Custom refs не покриваються стандартним refspec — тягнемо явно (спека).
    git(
        repo_root,
        &[
            "fetch",
            "--quiet",
            "origin",
            &format!("+{CLAIM_REF_PREFIX}/*:{CLAIM_REF_PREFIX}/*"),
        ],
    )?;
    let now = Utc::now();
    let mut claims = Vec::new();
    for r in refs {
        let yaml = git(repo_root, &["show", &format!("{}:.mt-claim.yml", r.sha)])?;
        claims.push(parse_claim(&r.node_hash, &yaml, grace_sec, now));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{output, TestRepo};
    use chrono::TimeZone;

    fn fields<'a>(node: &'a str, token: &'a str, base_sha: &'a str) -> ClaimFields<'a> {
        ClaimFields {
            node,
            actor: "agent",
            runner_id: "test-runner/1",
            claimed_at: "2026-06-09T10:00:00Z",
            lease_until: "2030-01-01T00:00:00Z",
            token,
            generation: 1,
            base_sha,
            run_ref: "refs/mt/runs/deadbeef/tok",
            interactive: false,
        }
    }

    /// `interactive:` пишеться у claim YAML і читається назад; відсутність
    /// поля (старі claim-и 0.2.x) → false (ADR 260711-2100).
    #[test]
    fn interactive_field_roundtrips_and_defaults_to_false() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 7, 11, 12, 0, 0).unwrap();
        let mut f = fields("research/analyze", "t1", "abc");
        f.interactive = true;
        let yaml = claim_yaml(&f);
        assert!(yaml.contains("interactive: true"), "{yaml}");
        assert!(parse_claim("h", &yaml, 0, now).interactive);

        // Старий claim без поля — консервативний false.
        let legacy = "schema_version: 1\nnode: x\nlease_until: 2030-01-01T00:00:00Z\n";
        assert!(!parse_claim("h", legacy, 0, now).interactive);
    }

    #[test]
    fn acquire_is_create_only_second_attempt_rejected() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let hash = node_hash("mt", "research/analyze");

        let first = acquire_claim(
            repo.work.path(),
            &hash,
            &fields("research/analyze", "t1", &base),
        )
        .unwrap();
        assert!(first.accepted);

        // Другий CAS-push з тим самим create-only lease (expect empty) —
        // ref уже існує, тож приймається лише один.
        let second = acquire_claim(
            repo.work.path(),
            &hash,
            &fields("research/analyze", "t2", &base),
        )
        .unwrap();
        assert!(!second.accepted);
    }

    #[test]
    fn renew_with_correct_sha_accepted_wrong_sha_rejected() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let hash = node_hash("mt", "research/analyze");
        let first = acquire_claim(
            repo.work.path(),
            &hash,
            &fields("research/analyze", "t1", &base),
        )
        .unwrap();

        let renewed = renew_or_takeover_claim(
            repo.work.path(),
            &hash,
            &first.commit_sha,
            &fields("research/analyze", "t1", &base),
        )
        .unwrap();
        assert!(renewed.accepted);
        assert_ne!(renewed.commit_sha, first.commit_sha);

        // Ланцюг claim-комітів: parent нового = попередній claim-коміт (не main).
        let parent = output(
            repo.work.path(),
            &["rev-parse", &format!("{}^", renewed.commit_sha)],
        );
        assert_eq!(parent, first.commit_sha);

        // Застаріле знання SHA (гонка вже пройшла) → CAS відхиляє.
        let stale = renew_or_takeover_claim(
            repo.work.path(),
            &hash,
            &first.commit_sha,
            &fields("research/analyze", "t3", &base),
        )
        .unwrap();
        assert!(!stale.accepted);
    }

    #[test]
    fn release_requires_exact_sha_then_ref_gone() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let hash = node_hash("mt", "research/analyze");
        let claim = acquire_claim(
            repo.work.path(),
            &hash,
            &fields("research/analyze", "t1", &base),
        )
        .unwrap();

        // Застарілий SHA — release відхиляється, ref лишається.
        assert!(!release_claim(
            repo.work.path(),
            &hash,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        )
        .unwrap());
        let ls = git(
            repo.work.path(),
            &["ls-remote", "origin", &format!("{CLAIM_REF_PREFIX}/{hash}")],
        )
        .unwrap();
        assert!(!ls.trim().is_empty());

        assert!(release_claim(repo.work.path(), &hash, &claim.commit_sha).unwrap());
        let ls = git(
            repo.work.path(),
            &["ls-remote", "origin", &format!("{CLAIM_REF_PREFIX}/{hash}")],
        )
        .unwrap();
        assert!(ls.trim().is_empty());
    }

    #[test]
    fn discovers_repo_root_and_relative_tasks_dir() {
        let repo = TestRepo::new();
        let tasks_dir = repo.work.path().join("mt");
        std::fs::create_dir_all(&tasks_dir).unwrap();

        let root = discover_repo_root(&tasks_dir).unwrap();
        assert_eq!(
            root.canonicalize().unwrap(),
            repo.work.path().canonicalize().unwrap()
        );
        assert_eq!(tasks_root_relative(&root, &tasks_dir).unwrap(), "mt");
    }

    #[test]
    fn fetch_remote_claims_reads_back_what_acquire_wrote() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let hash = node_hash("mt", "research/analyze");
        acquire_claim(
            repo.work.path(),
            &hash,
            &fields("research/analyze", "t1", &base),
        )
        .unwrap();

        let claims = fetch_remote_claims(repo.work.path(), 60).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].node_hash, hash);
        assert_eq!(claims[0].node.as_deref(), Some("research/analyze"));
        assert_eq!(claims[0].runner_id.as_deref(), Some("test-runner/1"));
        assert!(!claims[0].expired);
    }

    #[test]
    fn node_hash_is_20_hex_and_stable() {
        let h = node_hash("mt", "research/analyze");
        assert_eq!(h.len(), 20);
        assert!(h.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(h, node_hash("mt", "research/analyze"));
        assert_ne!(h, node_hash("mt", "research"));
        // Роздільник \0 розрізняє межу root/path.
        assert_ne!(node_hash("mt/a", "b"), node_hash("mt", "a/b"));
    }

    #[test]
    fn parses_ls_remote_output() {
        let out = "abc123\trefs/mt/claims/deadbeefdeadbeefdead\n\
                   ffff00\trefs/mt/runs/x/y\n\
                   012345\trefs/heads/main\n";
        let refs = parse_ls_remote(out, CLAIM_REF_PREFIX);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].node_hash, "deadbeefdeadbeefdead");
        assert_eq!(refs[0].sha, "abc123");
    }

    #[test]
    fn claim_expiry_uses_grace() {
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 11, 0, 0).unwrap();
        assert!(!lease_expired(Some("2026-06-09T11:00:30Z"), 60, now));
        assert!(lease_expired(Some("2026-06-09T10:58:00Z"), 60, now));
        assert!(lease_expired(Some("not-a-date"), 60, now));
        assert!(lease_expired(None, 60, now));
    }

    #[test]
    fn parses_claim_yaml() {
        let yaml = "schema_version: 1\nnode: research/analyze\nactor: agent\n\
                    runner_id: server-1/4821\nclaimed_at: 2026-06-09T10:00:00Z\n\
                    lease_until: 2026-06-09T11:00:00Z\ntoken: t\ngeneration: 1\n";
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 10, 30, 0).unwrap();
        let c = parse_claim("deadbeef", yaml, 60, now);
        assert_eq!(c.node.as_deref(), Some("research/analyze"));
        assert_eq!(c.actor.as_deref(), Some("agent"));
        assert_eq!(c.runner_id.as_deref(), Some("server-1/4821"));
        assert!(!c.expired);
    }
}
