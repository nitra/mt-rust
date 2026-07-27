//! napi-rs біндинг `mt-core` для Node/Bun-споживачів (MVP: worktree
//! create/remove/status + scan/plan/spawn) — див.
//! `docs/specs/2026-07-27-mt-napi-binding.md`.
//!
//! Кожна `#[napi]`-функція — тонкий шар над `mt_core`/`crates/mt`-glue:
//! бізнес-логіка лишається в `mt-core`, тут лише конвертація в типізовані
//! napi-об'єкти (рішення Ж специфікації).

#![deny(clippy::all)]

mod context;
mod graph;
mod plan;
mod worktree;

pub use graph::{scan, TaskNodeJs};
pub use plan::{
    plan, spawn_approve, spawn_reject, spawn_review, ChildSpecJs, PlanResult, PlanReviewJs,
    SpawnApproveResult, SpawnRejectResult,
};
pub use worktree::{
    worktree_create, worktree_remove, worktree_status, WorktreeCreateResult, WorktreeRemoveResult,
    WorktreeStatusItem,
};
