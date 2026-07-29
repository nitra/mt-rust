//! Fenced publish protocol (спека mt.md, «Fenced publish protocol») —
//! atomic multi-ref push результату worktree у `main`, з рефетчем/rebase і
//! retry через exponential backoff+jitter. Використовується агентом і
//! аудитором однаково; тут — генерична реалізація над готовим worktree.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    claims::{CLAIM_REF_PREFIX, RUN_REF_PREFIX},
    git::{compat, GitRepository},
};

/// Вхід одного fenced-publish запиту.
pub struct PublishRequest<'a> {
    /// Detached worktree з готовим результатом (агент/аудитор уже закомітив).
    pub worktree: &'a Path,
    pub node_hash: &'a str,
    /// Exact claim SHA, яким цей runner володіє — fencing-перевірка.
    pub claim_sha: &'a str,
    pub token: &'a str,
    /// Очікуваний SHA run ref перед publish (зазвичай той, що запушено при
    /// створенні worktree).
    pub run_ref_sha_before: &'a str,
}

/// Підсумок публікації. `published: false` без `Err` — fencing/conflict
/// (claim втрачено або конкурентний publish виграв гонку), не системна помилка.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublishOutcome {
    pub published: bool,
    /// `true` — рейс програно чи claim втрачено (retry марний, публікація
    /// зупиняється); `false` — вичерпано `retry_max` спроб при звичайних
    /// race-відхиленнях (можна повторити пізніше).
    pub fenced: bool,
    pub result_sha: Option<String>,
    pub attempts: u32,
}

/// Псевдо-джиттер без зовнішньої залежності `rand`: молодші біти системного
/// часу в наносекундах — достатньо для розсіювання конкурентних retry.
fn jitter_ms(spread_ms: u64) -> u64 {
    if spread_ms == 0 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    u64::from(nanos) % spread_ms
}

/// Fenced publish (спека, кроки 1–3): fetch main + claim ref → rebase
/// worktree на `origin/main` → перевірка fencing (claim ref усе ще exact
/// SHA) → atomic multi-ref push (main + CAS-видалення claim/run ref).
/// Retry з exponential backoff+jitter при race-відхиленні push-у.
pub fn fenced_publish(
    repo_root: &Path,
    req: &PublishRequest,
    retry_max: u32,
    base_ms: u64,
) -> Result<PublishOutcome, String> {
    let claim_ref = format!("{CLAIM_REF_PREFIX}/{}", req.node_hash);
    let run_ref = format!("{RUN_REF_PREFIX}/{}/{}", req.node_hash, req.token);
    let repository = GitRepository::open(repo_root).map_err(|error| error.to_string())?;

    for attempt in 0..retry_max.max(1) {
        repository
            .fetch_refspec("+refs/heads/main:refs/remotes/origin/main")
            .map_err(|error| error.to_string())?;
        // Custom ref — явний fetch (спека: стандартний refspec його не покриває).
        if repository
            .fetch_refspec(&format!("+{claim_ref}:{claim_ref}"))
            .is_err()
        {
            // Claim ref зник (звільнено/протух і прибрано) — без claim publish
            // неможливий: fencing failed, не системна помилка.
            return Ok(PublishOutcome {
                published: false,
                fenced: true,
                result_sha: None,
                attempts: attempt + 1,
            });
        }

        // Fencing: claim ref усе ще на exact SHA цього runner-а.
        let current_claim_sha = match repository.resolve_ref(&claim_ref) {
            Ok(sha) => sha,
            Err(_) => {
                return Ok(PublishOutcome {
                    published: false,
                    fenced: true,
                    result_sha: None,
                    attempts: attempt + 1,
                })
            }
        };
        if current_claim_sha != req.claim_sha {
            return Ok(PublishOutcome {
                published: false,
                fenced: true,
                result_sha: None,
                attempts: attempt + 1,
            });
        }

        let main_sha_before = repository
            .resolve_ref("refs/remotes/origin/main")
            .map_err(|error| error.to_string())?;

        // Rebase worktree на origin/main. Конфлікт → merge-conflict (термінально
        // для цієї спроби; викликач фіксує result: merge-conflict, без retry тут).
        compat::rebase_onto(req.worktree, "origin/main")
            .map_err(|error| format!("rebase conflict on publish: {error}"))?;

        let result_sha = GitRepository::open(req.worktree)
            .and_then(|worktree| worktree.resolve_ref("HEAD"))
            .map_err(|error| error.to_string())?;

        let lease_main = format!("--force-with-lease=refs/heads/main:{main_sha_before}");
        let lease_claim = format!("--force-with-lease={claim_ref}:{}", req.claim_sha);
        let lease_run = format!("--force-with-lease={run_ref}:{}", req.run_ref_sha_before);
        let main_refspec = format!("{result_sha}:refs/heads/main");
        let claim_refspec = format!(":{claim_ref}");
        let run_refspec = format!(":{run_ref}");
        if compat::push_atomic(
            req.worktree,
            &[
                "push",
                "--atomic",
                &lease_main,
                &lease_claim,
                &lease_run,
                "origin",
                &main_refspec,
                &claim_refspec,
                &run_refspec,
            ],
        )
        .map_err(|error| error.to_string())?
        {
            // Best-effort: fast-forward локальний main, якщо саме на ньому і
            // це чистий ff (щоб GUI live-скан одразу бачив опублікований
            // результат, а не чекав ручного pull — не критично при невдачі).
            let _ = sync_local_main(repo_root, &result_sha);
            return Ok(PublishOutcome {
                published: true,
                fenced: false,
                result_sha: Some(result_sha),
                attempts: attempt + 1,
            });
        }

        // Race програно — backoff+jitter, наступна ітерація перечитує стан.
        let backoff = base_ms.saturating_mul(1u64 << attempt.min(16));
        std::thread::sleep(Duration::from_millis(backoff + jitter_ms(base_ms)));
    }

    Ok(PublishOutcome {
        published: false,
        fenced: false,
        result_sha: None,
        attempts: retry_max,
    })
}

/// Best-effort ff-only синхронізація локального `main` після власного
/// publish — щоб живий working tree (яке бачить FS-watcher GUI) одразу
/// відобразило результат без ручного `git pull`. Мовчки ігнорує невдачу
/// (інша гілка, брудне дерево, конфлікт) — дані вже в `origin/main`,
/// це лише питання видимості локально.
fn sync_local_main(repo_root: &Path, result_sha: &str) -> Result<(), String> {
    compat::fast_forward_main(repo_root, result_sha).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::{acquire_claim, node_hash, ClaimFields};
    use crate::test_support::{commit_all, remote_ref_exists, TestRepo};
    use crate::worktree::{create_run_worktree, push_run_ref};

    fn setup(repo: &TestRepo) -> (String, crate::claims::ClaimPush, std::path::PathBuf) {
        let base = repo.main_sha();
        let hash = node_hash("mt", "research/analyze");
        let fields = ClaimFields {
            node: "research/analyze",
            actor: "agent",
            runner_id: "test/1",
            claimed_at: "2026-06-09T10:00:00Z",
            lease_until: "2030-01-01T00:00:00Z",
            token: "tok1",
            generation: 1,
            base_sha: &base,
            run_ref: "refs/mt/runs/x/tok1",
            interactive: false,
        };
        let claim = acquire_claim(repo.work.path(), &hash, &fields).unwrap();
        assert!(claim.accepted);

        let worktrees_dir = tempfile::tempdir().unwrap();
        // Тримаємо TempDir живим через leak — тест короткий, ок для fixture.
        let worktrees_dir = Box::leak(Box::new(worktrees_dir));
        let wt = create_run_worktree(repo.work.path(), worktrees_dir.path(), &hash, "tok1", &base)
            .unwrap();
        push_run_ref(&wt, &hash, "tok1").unwrap();

        (hash, claim, wt)
    }

    #[test]
    fn publishes_result_atomically_and_updates_local_main() {
        let repo = TestRepo::new();
        let (hash, claim, wt) = setup(&repo);
        let base = repo.main_sha();

        std::fs::write(wt.join("result.txt"), "done").unwrap();
        commit_all(&wt, "mt: result");

        let req = PublishRequest {
            worktree: &wt,
            node_hash: &hash,
            claim_sha: &claim.commit_sha,
            token: "tok1",
            run_ref_sha_before: &base,
        };
        let outcome = fenced_publish(repo.work.path(), &req, 8, 10).unwrap();
        assert!(outcome.published);
        assert!(!outcome.fenced);
        assert!(outcome.result_sha.is_some());

        // main на remote просунувся; claim/run ref прибрані.
        assert!(remote_ref_exists(repo.work.path(), "refs/heads/main"));
        assert!(!remote_ref_exists(
            repo.work.path(),
            &format!("refs/mt/claims/{hash}")
        ));
        assert!(!remote_ref_exists(
            repo.work.path(),
            &format!("refs/mt/runs/{hash}/tok1")
        ));

        // Локальний main (той самий work-клон, HEAD на main) синхронізовано.
        assert!(repo.work.path().join("result.txt").is_file());
    }

    #[test]
    fn fenced_when_claim_lost_to_takeover() {
        let repo = TestRepo::new();
        let (hash, claim, wt) = setup(&repo);
        let base = repo.main_sha();

        // Інший runner перехопив claim (takeover) конкурентно.
        let fields2 = ClaimFields {
            node: "research/analyze",
            actor: "agent",
            runner_id: "test/2",
            claimed_at: "2026-06-09T10:05:00Z",
            lease_until: "2030-01-01T00:00:00Z",
            token: "tok2",
            generation: 2,
            base_sha: &base,
            run_ref: "refs/mt/runs/x/tok2",
            interactive: false,
        };
        crate::claims::renew_or_takeover_claim(
            repo.work.path(),
            &hash,
            &claim.commit_sha,
            &fields2,
        )
        .unwrap();

        std::fs::write(wt.join("result.txt"), "done").unwrap();
        commit_all(&wt, "mt: result");

        let req = PublishRequest {
            worktree: &wt,
            node_hash: &hash,
            claim_sha: &claim.commit_sha, // застарілий SHA — програний claim
            token: "tok1",
            run_ref_sha_before: &base,
        };
        let outcome = fenced_publish(repo.work.path(), &req, 3, 5).unwrap();
        assert!(!outcome.published);
        assert!(outcome.fenced);
    }
}
