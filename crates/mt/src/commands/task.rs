//! `mt init` — бутстрап проєкту (за потреби) + створення вузла задачі.

use clap::Args;
use mt_core::{create_task, CreateOpts, Mode};

use crate::context::resolve_tasks_dir;
use crate::output::emit;

#[derive(Args)]
pub struct InitArgs {
    /// Ім'я нової задачі (шлях у графі, напр. `research/collect-data`).
    pub name: String,
    #[arg(long, value_parser = ["agent", "human"])]
    pub mode: Option<String>,
    #[arg(long = "model-tier")]
    pub model_tier: Option<String>,
    #[arg(long = "budget-sec")]
    pub budget_sec: Option<u64>,
    #[arg(long)]
    pub hint: Option<String>,
    /// Залежність (можна повторювати): `--dep upstream --dep other`.
    #[arg(long = "dep")]
    pub deps: Vec<String>,
    /// Текст місії (`## Task`); без нього — шаблон-заглушка.
    #[arg(long)]
    pub task: Option<String>,
    /// Кваліфікація виконавця (лише для `--mode human`).
    #[arg(long)]
    pub qualification: Option<String>,
}

/// `scan_tasks` вимагає `task.md` на КОЖНОМУ сегменті шляху, щоб спуститись
/// углиб (проміжна директорія без власного `task.md` — глухий кут сканера).
/// `create_task` пише `task.md` лише в листі — багатосегментне ім'я без
/// існуючих предків створює вузол, невидимий для `scan`/`status`/`check`.
/// Перевіряємо предків заздалегідь, а не мовчки створюємо сирітський вузол.
fn ensure_ancestors_exist(tasks_dir: &str, name: &str) -> Result<(), String> {
    let segments: Vec<&str> = name.split('/').collect();
    if segments.len() < 2 {
        return Ok(());
    }
    let mut prefix = String::new();
    for seg in &segments[..segments.len() - 1] {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(seg);
        if !std::path::Path::new(tasks_dir)
            .join(&prefix)
            .join("task.md")
            .is_file()
        {
            return Err(format!(
                "предок {prefix:?} не має task.md — scan/status/check його не побачать; \
                 спершу `mt init {prefix}`, або створюй дітей через `mt spawn --approve`"
            ));
        }
    }
    Ok(())
}

pub fn run(args: InitArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(true)?;
    ensure_ancestors_exist(&tasks_dir, &args.name)?;
    let mode = match args.mode.as_deref() {
        Some("agent") => Some(Mode::Agent),
        Some("human") => Some(Mode::Human),
        _ => None,
    };
    let opts = CreateOpts {
        mode,
        model_tier: args.model_tier,
        budget_sec: args.budget_sec,
        hint: args.hint,
        deps: args.deps,
        skills: None,
        task: args.task,
        qualification: args.qualification,
    };
    let outcome = create_task(tasks_dir, args.name, opts)?;
    let value = outcome.to_cli_json();
    emit(json, &value, |v| {
        if v["created"].as_bool() == Some(true) {
            println!(
                "створено: {} ({})",
                v["task_path"].as_str().unwrap_or_default(),
                v["flag"].as_str().unwrap_or_default()
            );
        } else {
            println!("вже існує: {}", v["task_path"].as_str().unwrap_or_default());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_segment_name_has_no_ancestors_to_check() {
        assert!(ensure_ancestors_exist("/nonexistent", "demo").is_ok());
    }

    #[test]
    fn rejects_missing_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_ancestors_exist(tmp.path().to_str().unwrap(), "research/collect-data")
            .unwrap_err();
        assert!(err.contains("research"));
    }

    #[test]
    fn accepts_when_ancestor_already_has_task_md() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("research")).unwrap();
        std::fs::write(tmp.path().join("research/task.md"), "x").unwrap();
        assert!(
            ensure_ancestors_exist(tmp.path().to_str().unwrap(), "research/collect-data").is_ok()
        );
    }

    #[test]
    fn checks_every_ancestor_in_deep_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/task.md"), "x").unwrap();
        // "a" has task.md, "a/b" does not → should still reject.
        let err = ensure_ancestors_exist(tmp.path().to_str().unwrap(), "a/b/c").unwrap_err();
        assert!(err.contains("a/b"));
    }
}
