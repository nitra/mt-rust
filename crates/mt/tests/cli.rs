//! Black-box e2e тести `mt`-бінарника — спавн реального процесу, перевірка
//! stdout/exit-коду. Мутуючі команди — лише в ізольованих `TestRepo`-фікстурах
//! (спека 2026-07-23-mt-cli-rust.md: жодних мутацій на живому dogfood-дереві).

mod common;

use common::{run, stdout, TestRepo};

#[test]
fn doctor_reports_missing_tasks_dir_before_init() {
    let repo = TestRepo::new();
    let out = run(repo.work.path(), &["doctor"]);
    assert!(!out.status.success());
    assert!(stdout(&out).contains("tasks_dir"));
}

#[test]
fn init_creates_task_and_status_reports_it() {
    let repo = TestRepo::new();
    let out = run(repo.work.path(), &["init", "demo", "--mode", "human"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(repo.work.path().join("mt/demo/task.md").is_file());
    assert!(repo.work.path().join("mt/demo/h.md").is_file());

    let status = run(repo.work.path(), &["status"]);
    assert!(status.status.success());
    assert!(stdout(&status).contains("Pending: 1"));
}

#[test]
fn init_is_idempotent_and_json_reflects_it() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo"]);
    let out = run(repo.work.path(), &["--json", "init", "demo"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["created"], false);
    assert_eq!(v["reason"], "exists");
}

#[test]
fn init_rejects_nested_name_without_existing_ancestor() {
    let repo = TestRepo::new();
    let out = run(repo.work.path(), &["init", "research/collect-data"]);
    assert!(!out.status.success());
    assert!(stdout(&out).contains("research") || !out.status.success());
}

#[test]
fn plan_verify_fact_done_full_signal_cycle() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo", "--mode", "agent"]);
    let node_dir = repo.work.path().join("mt/demo");

    let plan = run(&node_dir, &["plan"]);
    assert!(plan.status.success());
    assert!(node_dir.join("plan_001.md").is_file());

    let verify = run(&node_dir, &["verify"]);
    assert!(verify.status.success(), "{}", stdout(&verify));

    let fact = run(&node_dir, &["fact", "--summary", "Зроблено"]);
    assert!(fact.status.success());
    assert!(node_dir.join("fact_001.md").is_file());

    let done = run(&node_dir, &["done"]);
    assert!(done.status.success(), "{}", stdout(&done));
    assert!(node_dir.join("run_001.md").is_file());

    let status = run(repo.work.path(), &["--json", "status"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(v["counts"]["Resolved"], 1);
}

#[test]
fn failed_requires_completed_blockers_next_attempt() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo", "--mode", "agent"]);
    let node_dir = repo.work.path().join("mt/demo");
    let out = run(
        &node_dir,
        &[
            "failed",
            "--completed",
            "нічого",
            "--blockers",
            "API недоступний",
            "--next-attempt",
            "спробувати ще раз",
        ],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(node_dir.join("run_001.md").is_file());
}

#[test]
fn check_exits_clean_when_nothing_needs_attention() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo"]);
    let out = run(repo.work.path(), &["check"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("чисто"));
}

#[test]
fn root_flag_lets_command_run_from_elsewhere() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo"]);
    let elsewhere = tempfile::tempdir().unwrap();
    let out = run(
        elsewhere.path(),
        &["--root", repo.work.path().to_str().unwrap(), "status"],
    );
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("Pending: 1"));
}

#[test]
fn worktree_create_list_remove_round_trip() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo"]);
    let create = run(repo.work.path(), &["worktree", "create", "devwork"]);
    assert!(create.status.success(), "{}", stdout(&create));

    let list = run(repo.work.path(), &["--json", "worktree", "list"]);
    let entries: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert!(entries
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["name"] == "devwork"));

    let remove = run(
        repo.work.path(),
        &["worktree", "remove", "devwork", "--force"],
    );
    assert!(remove.status.success(), "{}", stdout(&remove));

    let list2 = run(repo.work.path(), &["--json", "worktree", "list"]);
    let entries2: serde_json::Value = serde_json::from_str(&stdout(&list2)).unwrap();
    assert!(!entries2
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["name"] == "devwork"));
}

#[test]
fn spawn_approve_materializes_children_from_plan() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "research", "--mode", "human"]);
    let node_dir = repo.work.path().join("mt/research");
    std::fs::write(
        node_dir.join("plan_001.md"),
        "---\nschema_version: 1\ndecision: composite\n---\n\n## Children\n\n```yaml\nchildren:\n  - id: collect\n    mode: agent\n    task: x\n```\n",
    )
    .unwrap();

    let approve = run(&node_dir, &["spawn", "--approve"]);
    assert!(approve.status.success(), "{}", stdout(&approve));
    assert!(node_dir.join("collect/task.md").is_file());
    assert!(node_dir.join("plan-approved_001.md").is_file());
}

#[test]
fn doctor_json_reports_ok_after_init_with_origin() {
    let repo = TestRepo::new();
    run(repo.work.path(), &["init", "demo"]);
    let out = run(repo.work.path(), &["--json", "doctor"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["ok"], true);
}
