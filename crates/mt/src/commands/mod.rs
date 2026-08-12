//! Підкоманди `mt`: кожен модуль тут відповідає за окремий CLI entrypoint.
/// Підкоманда `mt audit`.
pub mod audit;
/// Підкоманда `mt check`.
pub mod check;
/// Підкоманда `mt decision`.
pub mod decision;
/// Підкоманда `mt doctor`.
pub mod doctor;
/// Підкоманда `mt graph`.
pub mod graph;
/// Підкоманда `mt lifecycle`.
pub mod lifecycle;
/// Підкоманда `mt plan`.
pub mod plan;
/// Підкоманда `mt run`.
pub mod run;
/// Підкоманда `mt session`.
pub mod session;
/// Підкоманда `mt signal`.
pub mod signal;
/// Підкоманда `mt task`.
pub mod task;
/// Підкоманда `mt worktree`.
pub mod worktree;
