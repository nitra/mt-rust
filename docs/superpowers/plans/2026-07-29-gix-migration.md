# Gix Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Перевести всі підтримувані Git-взаємодії Rust-проєкту на `gix`, зберігши CAS claims, recovery refs, worktree isolation і fenced publish.

**Architecture:** `mt-core::git` стане єдиною facade. Native `gix` реалізує discovery, refs, objects, commits, status, fetch/push і worktree inspection; `git::compat` лишається єдиним місцем shell-out для linked-worktree lifecycle, rebase та atomic multi-ref push. `mt`, `mt-napi` і `agent-server` споживають facade, а не `Command::new("git")`.

**Tech Stack:** Rust 2021, `gix` 0.86 with `blocking-network-client`, existing `serde`, `tempfile`, Cargo integration tests.

## Global Constraints

- Pinned dependency: `gix = "0.86"`; enabled features must include blocking network transport, index/status and worktree inspection.
- Keep exact ref schema: `refs/mt/claims/<node-hash>` and `refs/mt/runs/<node-hash>/<token>`.
- Remote race is `Rejected(LeaseMismatch)`, never a silently ignored error.
- Outside `crates/mt-core/src/git/compat.rs` production code must not contain `Command::new("git")`.
- `compat` may expose only `create_linked_worktree`, `remove_linked_worktree`, `prune_linked_worktrees`, `rebase_onto`, and `push_atomic`.
- Every changed Rust file receives current Ukrainian `docs/<stem>.md` documentation.
- Run the changelog gate after every committed unit; do not add generated `mt/lint-*` task drafts to a feature commit.

---

### Task 1: Add the gix facade and static boundary

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/mt-core/Cargo.toml`
- Create: `crates/mt-core/src/git/mod.rs`
- Create: `crates/mt-core/src/git/error.rs`
- Create: `crates/mt-core/src/git/refs.rs`
- Modify: `crates/mt-core/src/lib.rs`
- Create: `crates/mt-core/src/git/docs/mod.md`
- Create: `crates/mt-core/src/git/docs/error.md`
- Create: `crates/mt-core/src/git/docs/refs.md`
- Test: `crates/mt-core/tests/git_boundary.rs`

**Interfaces:**
- Produces: `GitRepository::open(path: &Path) -> Result<GitRepository, GitError>`.
- Produces: `ClaimRef`, `RunRef`, `RemoteUpdate`, `RemoteUpdateResult` and `GitError`.
- Consumed by every later task.

- [x] **Step 1: Write failing boundary tests**

```rust
#[test]
fn claim_and_run_ref_names_are_validated() {
    assert_eq!(ClaimRef::new("abc123").unwrap().as_str(), "refs/mt/claims/abc123");
    assert!(RunRef::new("abc123", "bad/token").is_err());
}
```

- [x] **Step 2: Run the new test and confirm failure**

Run: `cargo test -p mt-core --test git_boundary`

Expected: FAIL because the facade types do not exist.

- [x] **Step 3: Add dependency and minimal typed facade**

```toml
# Cargo.toml [workspace.dependencies]
gix = { version = "0.86", default-features = false, features = ["basic", "blocking-network-client", "index", "status", "worktree-mutation"] }
```

```rust
pub enum RemoteUpdateResult { Applied, Rejected(LeaseMismatch) }
pub struct ClaimRef(gix::refs::FullName);
pub struct RunRef(gix::refs::FullName);
pub struct GitRepository { repo: gix::Repository }
```

Make `lib.rs` export only `pub mod git;`; preserve old callers until their individual migration tasks.

- [x] **Step 4: Run tests, format and clippy**

Run: `cargo test -p mt-core --test git_boundary && cargo fmt --all -- --check && cargo clippy -p mt-core --all-targets --all-features -- -D warnings`

Expected: PASS.

- [x] **Step 5: Generate Rust file docs and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit:

```bash
git add Cargo.toml Cargo.lock crates/mt-core
git commit -m "feat(git): add gix facade boundary"
```

### Task 2: Replace discovery, origin and local-ref reads

**Files:**
- Modify: `crates/mt-core/src/git/mod.rs`
- Modify: `crates/mt-core/src/claims.rs:40-47,336-365`
- Modify: `crates/mt/src/commands/doctor.rs:31-53`
- Modify: `crates/mt-core/src/lib.rs:775-790`
- Test: `crates/mt-core/tests/git_repository.rs`
- Test: `crates/mt/tests/cli.rs`

**Interfaces:**
- Consumes: `GitRepository` and typed refs from Task 1.
- Produces: `repo_root()`, `origin_url()`, `resolve_ref()`, `read_blob_at_commit()`, `linked_worktrees()`.

- [ ] **Step 1: Write failing repository tests**

```rust
#[test]
fn opens_from_nested_linked_worktree_and_finds_main_repo() { /* fixture */ }
#[test]
fn origin_is_none_without_remote_and_a_url_when_configured() { /* fixture */ }
#[test]
fn resolve_ref_and_read_blob_do_not_shell_out() { /* fixture */ }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test -p mt-core --test git_repository`

Expected: FAIL because the facade read methods do not exist.

- [ ] **Step 3: Implement native gix reads**

Use `gix::discover()` to open from any task/worktree path, `main_repo()` for the shared repository, `find_remote("origin")` for doctor, `try_find_reference()` plus object lookup for SHA/blob reads, and `Repository::worktrees()` for inventory. Replace porcelain parsing and `git rev-parse`, `git show`, `git remote get-url`, and `git worktree list` readers.

- [ ] **Step 4: Verify CLI behavior**

Run: `cargo test -p mt-core --test git_repository && cargo test -p mt --test cli doctor worktree -- --nocapture`

Expected: PASS; missing origin remains a doctor failure with the existing user-facing text.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates/mt-core crates/mt && git commit -m "feat(git): use gix for repository discovery"`

### Task 3: Move claim object creation and remote reads to gix

**Files:**
- Modify: `crates/mt-core/src/git/mod.rs`
- Modify: `crates/mt-core/src/claims.rs:138-365`
- Modify: `crates/mt-core/src/test_support.rs`
- Test: `crates/mt-core/tests/claims_gix.rs`

**Interfaces:**
- Produces: `write_commit(parent, files, message, signature)`, `list_remote_refs(pattern)`, `fetch_exact_ref(refname)`.
- Preserves: `ClaimInfo`, `ClaimFields`, `parse_ls_remote`, `acquire_claim` public behavior.

- [ ] **Step 1: Add failing object and remote-read tests**

```rust
#[test]
fn claim_commit_contains_only_claim_yaml_and_expected_parent() { /* fixture */ }
#[test]
fn remote_claim_read_ignores_non_claim_refs_and_reports_transport_failure() { /* fixture */ }
```

- [ ] **Step 2: Run the focused tests**

Run: `cargo test -p mt-core --test claims_gix`

Expected: FAIL before native writer/remote reader exists.

- [ ] **Step 3: Implement without index or checkout**

Build `.mt-claim.yml` as a blob, put it in a one-entry tree, create a commit with the specified parent, then list/fetch only `refs/mt/claims/*` through the configured `origin` remote. Decode the claim blob with gix object APIs instead of `git show`.

- [ ] **Step 4: Run claim regression tests**

Run: `cargo test -p mt-core claims -- --nocapture`

Expected: PASS, including expired-lease parsing and custom-ref filtering.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates/mt-core && git commit -m "feat(git): create and read claims with gix"`

### Task 4: Migrate CAS claim and run-ref transport

**Files:**
- Modify: `crates/mt-core/src/git/mod.rs`
- Modify: `crates/mt-core/src/claims.rs:240-334`
- Modify: `crates/mt-core/src/worktree.rs:79-111`
- Test: `crates/mt-core/tests/remote_cas.rs`

**Interfaces:**
- Produces: `push_with_expected(refname, new_target, expected_target)` and `delete_with_expected(refname, expected_target)`.
- Consumed by: runner, agent server and fenced publish.

- [ ] **Step 1: Write race tests against a local bare remote**

```rust
#[test]
fn only_one_create_only_claim_is_applied() { /* two repositories, one remote */ }
#[test]
fn stale_renewal_and_delete_return_lease_mismatch() { /* exact old oid */ }
#[test]
fn run_ref_push_and_exact_delete_survive_reopen() { /* reopen fixture */ }
```

- [ ] **Step 2: Run the tests and confirm failure**

Run: `cargo test -p mt-core --test remote_cas`

Expected: FAIL because claims and run refs still execute Git CLI.

- [ ] **Step 3: Implement typed transport mapping**

Map native transport success to `Applied`; map the protocol's expected-old-object rejection to `Rejected(LeaseMismatch)`; return all other errors as `GitError`. Do not parse stderr and do not fall back to CLI for a non-atomic single-ref update.

- [ ] **Step 4: Verify concurrency semantics**

Run: `cargo test -p mt-core --test remote_cas && cargo test -p mt-core claims worktree -- --nocapture`

Expected: PASS; exactly one concurrent acquire succeeds.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates/mt-core && git commit -m "feat(git): move claim CAS transport to gix"`

### Task 5: Migrate index, status and commits

**Files:**
- Modify: `crates/mt-core/src/git/mod.rs`
- Modify: `crates/mt-core/src/runner.rs:461-530`
- Modify: `crates/agent-server/src/graph.rs:69-370`
- Test: `crates/mt-core/tests/git_commit.rs`
- Test: `crates/agent-server/tests/graph_wiring.rs`

**Interfaces:**
- Produces: `commit_all_if_changed(message, SignaturePolicy) -> Result<Option<ObjectId>, GitError>` and `remove_from_index(path)`.
- Preserves: author/committer `mt-runner <mt-runner@localhost>` where current runner uses it.

- [ ] **Step 1: Write failing status/commit tests**

```rust
#[test]
fn clean_worktree_returns_none_without_creating_commit() { /* fixture */ }
#[test]
fn dirty_worktree_commits_all_files_with_runner_identity() { /* fixture */ }
#[test]
fn removing_nitra_from_index_keeps_worktree_journal_file() { /* fixture */ }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test -p mt-core --test git_commit`

Expected: FAIL before the facade owns index mutation.

- [ ] **Step 3: Implement native index workflow**

Use gix status/index APIs to detect changes, update the index from the worktree, write a tree and commit it with explicit signatures. For `.nitra` stripping, remove index entries only, preserving the untracked worktree journal file.

- [ ] **Step 4: Replace both runner callers and verify**

Run: `cargo test -p mt-core --test git_commit && cargo test -p agent-server graph_wiring -- --nocapture`

Expected: PASS; no direct Git command remains in `runner.rs` or `agent-server/src/graph.rs` for commit/status/index work.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates/mt-core crates/agent-server && git commit -m "feat(git): commit runner state through gix"`

### Task 6: Isolate unsupported porcelain in compat

**Files:**
- Create: `crates/mt-core/src/git/compat.rs`
- Modify: `crates/mt-core/src/git/mod.rs`
- Modify: `crates/mt-core/src/worktree.rs:52-157,264-280,320-345`
- Modify: `crates/mt-core/src/publish.rs:87-190`
- Create: `crates/mt-core/src/git/docs/compat.md`
- Test: `crates/mt-core/tests/git_compat.rs`

**Interfaces:**
- Produces: the five allow-listed `compat` methods from Global Constraints.
- Native facade remains responsible for `fetch`, SHA resolution and post-publish local inspection.

- [ ] **Step 1: Write contract tests**

```rust
#[test]
fn compat_refuses_any_operation_outside_the_allow_list() { /* compile-time/module visibility */ }
#[test]
fn linked_worktree_remove_respects_force() { /* dirty fixture */ }
#[test]
fn rebase_conflict_is_reported_and_aborted() { /* divergent fixture */ }
#[test]
fn atomic_publish_never_leaves_main_and_claim_partially_updated() { /* rejected lease fixture */ }
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test -p mt-core --test git_compat`

Expected: FAIL because compat module and contract boundaries do not exist.

- [ ] **Step 3: Move only unsupported commands**

Place `worktree add/remove/prune`, `rebase --abort`, and `push --atomic` inside private `compat`. Replace every other command in `worktree.rs` and `publish.rs` with facade calls. `push_atomic` accepts typed updates and exact expected OIDs; its error enum distinguishes lease rejection from transport failure.

- [ ] **Step 4: Verify publish and worktree behavior**

Run: `cargo test -p mt-core worktree publish -- --nocapture`

Expected: PASS; no production `Command::new("git")` exists outside `git/compat.rs`.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates/mt-core && git commit -m "refactor(git): isolate unsupported porcelain"`

### Task 7: Convert all test fixtures and CLI tests to gix

**Files:**
- Modify: `crates/mt-core/src/test_support.rs`
- Modify: `crates/mt/tests/common/mod.rs`
- Modify: `crates/mt/tests/cli.rs`
- Modify: `crates/agent-server/tests/handoff_ws.rs`
- Modify: `crates/agent-server/tests/graph_wiring.rs`
- Create or Modify: `crates/mt-core/src/docs/test_support.md`
- Test: `crates/mt-core/tests/git_boundary.rs`

**Interfaces:**
- Produces: `TestRepo::new()` creating bare remote, work repo, initial commit and origin entirely through gix.
- Removes: test-only generic `git(dir, args)` helpers.

- [ ] **Step 1: Add the global static guard after every caller has migrated**

```rust
#[test]
fn rust_tests_have_no_direct_git_command() {
    assert_no_forbidden_git_command("crates");
}
```

- [ ] **Step 2: Run and confirm current failures**

Run: `cargo test -p mt-core --test git_boundary`

Expected: FAIL, listing fixture helpers and direct test shell-outs.

- [ ] **Step 3: Replace fixture setup and assertions**

Create bare repos, initial commits, remotes, branches, object reads and ref assertions through `TestRepo` facade methods. Tests that need unsupported behavior invoke public production `compat` behavior and assert results through gix; they never execute system Git directly.

- [ ] **Step 4: Run workspace tests**

Run: `cargo test --workspace`

Expected: PASS with no Rust test invoking `Command::new("git")`.

- [ ] **Step 5: Docs, gate and commit**

Run: `npx @7n/rules lint doc-files && npx @7n/rules lint changelog`

Commit: `git add crates && git commit -m "test(git): build fixtures through gix"`

### Task 8: Enforce the end state and publish migration evidence

**Files:**
- Modify: `crates/mt-core/tests/git_boundary.rs`
- Modify: `docs/superpowers/specs/2026-07-29-gix-migration-design.md`

**Interfaces:**
- Produces: permanent capability-matrix test and updated design status with exact compat allow-list.

- [ ] **Step 1: Write final enforcement tests**

```rust
#[test]
fn compat_is_the_only_git_cli_boundary() { /* source scan */ }
#[test]
fn all_native_capabilities_have_gix_contract_coverage() { /* matrix rows */ }
```

- [ ] **Step 2: Run enforcement tests and verify initial missing coverage**

Run: `cargo test -p mt-core --test git_boundary`

Expected: PASS only after Tasks 1-7; any newly added direct CLI call fails with its source path.

- [ ] **Step 3: Update documentation**

Mark the design's capability matrix with the pinned `gix` version, native APIs used and the three retained compat groups. Add behavior documentation for every changed Rust module through `npx @7n/rules lint doc-files`.

- [ ] **Step 4: Run complete verification**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
npx @7n/rules lint
npx @7n/rules lint changelog
```

Expected: every command exits 0.

- [ ] **Step 5: Commit final migration**

```bash
git add crates docs
git commit -m "docs: record gix migration guarantees"
```

## Plan self-review

- Spec coverage: Tasks 1-8 cover native facade, typed refs, claims, run refs, status/commits, worktrees, fenced publish, all test helpers, static enforcement and documentation.
- Placeholder scan: the plan has no deferred implementation markers; every task identifies files, APIs, a failing test, a command and a commit.
- Type consistency: `GitRepository`, typed refs and `RemoteUpdateResult` originate in Task 1 and are consumed only after that task; `compat` has the same five-method allow-list in the design and in Task 6.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-29-gix-migration.md`. Two execution options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using `executing-plans`, in batches with checkpoints.

Which approach?
