//! Сигнали виконавця `mt done | audit | failed` — файловий рівень wrapper-а
//! (спека mt.md, «Два етапи виконання вузла» і «Composite fact»).
//!
//! Потік: виконавець пише `fact_NNN.md` (NNN наступної спроби) → `done`/`audit`
//! проганяє `## Check` з task.md → wrapper пише `run_NNN.md`; `audit` додатково
//! відкриває аудит-цикл (`pending-audit_NNN.md`). `failed` пише run без fact
//! («дірка» в нумерації). Після успішного done — composite-агрегація вгору:
//! всі діти resolved → синтетична пара run/fact батька (actor: wrapper).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::frontmatter::{get_body, parse_front_matter};
use crate::lifecycle::child_nodes;
use crate::nnn::pad_nnn;
use crate::{accepted_fact_state, validate_name, write_atomic, FactState};

/// Результат однієї команди `## Check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub command: String,
    pub exit_code: i32,
    pub output: String,
}

/// Результат сигналу done/audit: записані файли + пропагація вгору.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalOutcome {
    pub run_file: String,
    pub fact_file: String,
    /// Для audit — файл відкритого аудит-циклу.
    pub pending_audit_file: Option<String>,
    /// Батьки, що отримали синтетичну пару run/fact (composite-агрегація).
    pub propagated: Vec<String>,
}

fn node_dir(tasks_dir: &str, node_path: &str) -> Result<PathBuf, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    Ok(dir)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// NNN наступної спроби: `count(run_*.md) + 1` (спека, «NNN source»).
pub fn next_run_nnn(dir: &Path) -> u64 {
    let count = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.strip_prefix("run_")
                        .and_then(|r| r.strip_suffix(".md"))
                        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                })
                .count()
        })
        .unwrap_or(0);
    count as u64 + 1
}

/// Витягує команди секції `## Check` task.md: кожен непорожній рядок —
/// shell-команда, `#` — коментар.
pub fn check_commands(task_md: &str) -> Vec<String> {
    let body = get_body(task_md);
    let mut commands = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        if line.trim() == "## Check" {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with("## ") {
                break;
            }
            let t = line.trim();
            if !t.is_empty() && !t.starts_with('#') && !t.starts_with("<!--") {
                commands.push(t.to_string());
            }
        }
    }
    commands
}

/// Проганяє `## Check` (cwd = project root — батько tasks_dir). Будь-який
/// ненульовий exit → `Err` з виводом команд; сигнал відхиляється.
pub fn run_check(tasks_dir: &str, node_path: &str) -> Result<Vec<CheckResult>, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let task_md = fs::read_to_string(dir.join("task.md")).map_err(|e| e.to_string())?;
    // Контракт graph.md: `## Check` ганяється у директорії вузла (артефакти
    // вузла — поряд із task.md); командам, яким потрібен корінь репо,
    // додається власний cwd-еквівалент у самому рядку Check.
    let cwd = dir.as_path();
    let mut results = Vec::new();
    for command in check_commands(&task_md) {
        let out = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("## Check `{command}`: {e}"))?;
        let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&out.stderr));
        let exit_code = out.status.code().unwrap_or(-1);
        results.push(CheckResult {
            command: command.clone(),
            exit_code,
            output: output.trim().to_string(),
        });
        if exit_code != 0 {
            let last = results.last().unwrap();
            return Err(format!(
                "## Check failed: `{}` → exit {}\n{}",
                last.command, last.exit_code, last.output
            ));
        }
    }
    Ok(results)
}

/// Пише `fact_NNN.md` (NNN наступної спроби) з обов'язковим `## Summary`.
pub fn write_fact(
    tasks_dir: &str,
    node_path: &str,
    summary: &str,
    extra_body: Option<&str>,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    if summary.trim().is_empty() {
        return Err("## Summary обов'язковий для fact".to_string());
    }
    let nnn = pad_nnn(next_run_nnn(&dir));
    let fact_file = format!("fact_{nnn}.md");
    let mut content = format!(
        "---\nschema_version: 1\ncreated_at: {}\n---\n\n## Summary\n\n{}\n",
        now_iso(),
        summary.trim()
    );
    if let Some(extra) = extra_body {
        if !extra.trim().is_empty() {
            content.push('\n');
            content.push_str(extra.trim());
            content.push('\n');
        }
    }
    write_atomic(&dir.join(&fact_file), &content)?;
    Ok(fact_file)
}

pub(crate) fn write_run(
    dir: &Path,
    nnn: &str,
    actor: &str,
    result: &str,
    sections: &str,
) -> Result<String, String> {
    write_run_fm(dir, nnn, actor, result, sections, "")
}

/// Як [`write_run`], але з додатковими frontmatter-рядками (wall_sec тощо).
pub fn write_run_fm(
    dir: &Path,
    nnn: &str,
    actor: &str,
    result: &str,
    sections: &str,
    extra_fm: &str,
) -> Result<String, String> {
    let run_file = format!("run_{nnn}.md");
    let content = format!(
        "---\nschema_version: 1\ncreated_at: {}\nactor: {actor}\nresult: {result}\n{extra_fm}---\n{sections}",
        now_iso()
    );
    write_atomic(&dir.join(&run_file), &content)?;
    Ok(run_file)
}

/// Політика аудиту вузла: frontmatter `audit:` task.md (required|optional|off).
fn audit_policy(dir: &Path) -> String {
    fs::read_to_string(dir.join("task.md"))
        .ok()
        .map(|c| parse_front_matter(&c))
        .and_then(|fm| {
            fm.get("audit")
                .and_then(serde_json::Value::as_str)
                .map(String::from)
        })
        .unwrap_or_else(|| "optional".to_string())
}

fn signal_success(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    with_audit: bool,
    extra_fm: &str,
) -> Result<SignalOutcome, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let policy = audit_policy(&dir);
    if !with_audit && policy == "required" {
        return Err("audit: required — вузол приймає лише сигнал audit".to_string());
    }
    if with_audit && policy == "off" {
        return Err("audit: off — аудит для цього вузла вимкнено".to_string());
    }

    let nnn_num = next_run_nnn(&dir);
    let nnn = pad_nnn(nnn_num);
    let fact_file = format!("fact_{nnn}.md");
    if !dir.join(&fact_file).is_file() {
        return Err(format!("{fact_file} відсутній — спершу запишіть fact"));
    }

    run_check(tasks_dir, node_path)?;

    let sections = format!("\n## Ref\n\nref: {fact_file}\n");
    let run_file = write_run_fm(&dir, &nnn, actor, "success", &sections, extra_fm)?;

    let pending_audit_file = if with_audit {
        let pa = format!("pending-audit_{nnn}.md");
        write_atomic(
            &dir.join(&pa),
            &format!(
                "---\nschema_version: 1\ncreated_at: {}\nactor: {actor}\n---\n",
                now_iso()
            ),
        )?;
        Some(pa)
    } else {
        None
    };

    // Аудит відкритий → вузол не resolved → пропагація вгору не запускається.
    let propagated = if with_audit {
        Vec::new()
    } else {
        propagate_composite(tasks_dir, node_path)?
    };

    Ok(SignalOutcome {
        run_file,
        fact_file,
        pending_audit_file,
        propagated,
    })
}

/// `mt done`: fact існує → `## Check` → `run_NNN (success)` → агрегація вгору.
pub fn done(tasks_dir: &str, node_path: &str, actor: &str) -> Result<SignalOutcome, String> {
    signal_success(tasks_dir, node_path, actor, false, "")
}

/// Як [`done`], але з додатковими frontmatter-рядками `run_NNN.md`
/// (runner фіксує `agent_cli`, `wall_sec` тощо).
pub fn done_fm(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    extra_fm: &str,
) -> Result<SignalOutcome, String> {
    signal_success(tasks_dir, node_path, actor, false, extra_fm)
}

/// `mt audit`: як done, але відкриває аудит-цикл (`pending-audit_NNN.md`).
pub fn audit(tasks_dir: &str, node_path: &str, actor: &str) -> Result<SignalOutcome, String> {
    signal_success(tasks_dir, node_path, actor, true, "")
}

/// Як [`audit`], але з додатковими frontmatter-рядками `run_NNN.md`.
pub fn audit_fm(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    extra_fm: &str,
) -> Result<SignalOutcome, String> {
    signal_success(tasks_dir, node_path, actor, true, extra_fm)
}

/// `mt failed`: `run_NNN (failed)` без fact; секції Completed/Blockers/Next
/// Attempt обов'язкові (інваріант файлу — джерело діагностики ретраїв).
pub fn failed(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    completed: &str,
    blockers: &str,
    next_attempt: &str,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    for (name, value) in [
        ("Completed", completed),
        ("Blockers", blockers),
        ("Next Attempt", next_attempt),
    ] {
        if value.trim().is_empty() {
            return Err(format!("## {name} обов'язковий при failed"));
        }
    }
    let nnn = pad_nnn(next_run_nnn(&dir));
    let sections = format!(
        "\n## Completed\n\n{}\n\n## Blockers\n\n{}\n\n## Next Attempt\n\n{}\n",
        completed.trim(),
        blockers.trim(),
        next_attempt.trim()
    );
    write_run(&dir, &nnn, actor, "failed", &sections)
}

/// Перше речення `## Summary` останнього fact вузла (для агрегації батька).
fn latest_fact_summary(dir: &Path) -> Option<(String, String)> {
    let nnn = crate::max_nnn(dir, "fact_", ".md");
    if nnn == 0 {
        return None;
    }
    let file = format!("fact_{nnn:03}.md");
    let content = fs::read_to_string(dir.join(&file)).ok()?;
    let body = get_body(&content);
    let mut inside = false;
    for line in body.lines() {
        if line.trim() == "## Summary" {
            inside = true;
            continue;
        }
        if inside {
            let t = line.trim();
            if t.starts_with("## ") {
                break;
            }
            if !t.is_empty() {
                return Some((file, t.to_string()));
            }
        }
    }
    Some((file, String::new()))
}

/// Composite-агрегація вгору (спека, «Composite fact»): якщо всі діти батька
/// resolved — wrapper пише синтетичну пару run/fact батька; рекурсивно далі.
/// Повертає шляхи батьків, що отримали синтетичну пару.
pub fn propagate_composite(tasks_dir: &str, node_path: &str) -> Result<Vec<String>, String> {
    let mut propagated = Vec::new();
    let mut current = node_path.to_string();

    while let Some((parent_path, _)) = current.rsplit_once('/') {
        let parent_dir = Path::new(tasks_dir).join(parent_path);
        if !parent_dir.join("task.md").is_file() {
            break;
        }
        // Батько вже resolved або не має дітей → зупинка.
        if accepted_fact_state(&parent_dir) == FactState::Resolved {
            break;
        }
        let children = child_nodes(&parent_dir);
        if children.is_empty() {
            break;
        }
        let all_resolved = children
            .iter()
            .all(|c| accepted_fact_state(&parent_dir.join(c)) == FactState::Resolved);
        if !all_resolved {
            break;
        }

        // export: false діти (з ## Children approved-плану) не потрапляють у ## children.
        let exported: Vec<&String> = {
            let excluded: Vec<String> = crate::spawn::plan_review(tasks_dir, parent_path)
                .map(|r| {
                    r.children
                        .iter()
                        .filter(|c| !c.export)
                        .map(|c| c.id.clone())
                        .collect()
                })
                .unwrap_or_default();
            children.iter().filter(|c| !excluded.contains(c)).collect()
        };

        let mut summaries = Vec::new();
        let mut refs = Vec::new();
        for child in &exported {
            if let Some((fact_file, summary)) = latest_fact_summary(&parent_dir.join(child)) {
                if !summary.is_empty() {
                    summaries.push(summary);
                }
                refs.push(format!("- {child}: ref: {child}/{fact_file}"));
            }
        }

        let nnn = pad_nnn(next_run_nnn(&parent_dir));
        write_run(
            &parent_dir,
            &nnn,
            "wrapper",
            "success",
            &format!("\n## Reasoning\n\nагрегація дітей\n\n## Ref\n\nref: fact_{nnn}.md\n"),
        )?;
        let fact_content = format!(
            "---\nschema_version: 1\ncreated_at: {}\n---\n\n## Summary\n\n{}\n\n## children\n\n{}\n",
            now_iso(),
            summaries.join(" "),
            refs.join("\n")
        );
        write_atomic(&parent_dir.join(format!("fact_{nnn}.md")), &fact_content)?;

        propagated.push(parent_path.to_string());
        current = parent_path.to_string();
    }
    Ok(propagated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK_WITH_CHECK: &str = "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\n---\n\n## Task\n\nx\n\n## Check\n\n# коментар\ntrue\n\n## Inputs\n";

    fn node(tmp: &Path, path: &str, task_md: &str) {
        let dir = tmp.join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), task_md).unwrap();
    }

    #[test]
    fn check_commands_skips_comments_and_stops_at_next_section() {
        let commands = check_commands(TASK_WITH_CHECK);
        assert_eq!(commands, ["true"]);
        assert!(check_commands("---\nschema_version: 1\n---\n\n## Task\n").is_empty());
    }

    #[test]
    fn done_requires_fact_then_writes_run() {
        let tmp = tempfile::tempdir().unwrap();
        node(tmp.path(), "solo", TASK_WITH_CHECK);
        let root = tmp.path().to_string_lossy().into_owned();

        assert!(done(&root, "solo", "human")
            .unwrap_err()
            .contains("fact_001"));

        write_fact(&root, "solo", "Зроблено 42 речі.", None).unwrap();
        let out = done(&root, "solo", "human").unwrap();
        assert_eq!(out.run_file, "run_001.md");
        assert_eq!(out.fact_file, "fact_001.md");
        let run = fs::read_to_string(tmp.path().join("solo/run_001.md")).unwrap();
        assert!(run.contains("actor: human"));
        assert!(run.contains("result: success"));
        assert!(run.contains("ref: fact_001.md"));
    }

    #[test]
    fn failing_check_rejects_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let task = TASK_WITH_CHECK.replace("true", "exit 3");
        node(tmp.path(), "solo", &task);
        let root = tmp.path().to_string_lossy().into_owned();
        write_fact(&root, "solo", "s", None).unwrap();
        let err = done(&root, "solo", "human").unwrap_err();
        assert!(err.contains("exit 3"));
        assert!(!tmp.path().join("solo/run_001.md").exists());
    }

    #[test]
    fn audit_opens_cycle_and_required_blocks_done() {
        let tmp = tempfile::tempdir().unwrap();
        let task = TASK_WITH_CHECK.replace("---\n\n## Task", "audit: required\n---\n\n## Task");
        node(tmp.path(), "solo", &task);
        let root = tmp.path().to_string_lossy().into_owned();
        write_fact(&root, "solo", "s", None).unwrap();

        assert!(done(&root, "solo", "human")
            .unwrap_err()
            .contains("required"));
        let out = audit(&root, "solo", "human").unwrap();
        assert_eq!(
            out.pending_audit_file.as_deref(),
            Some("pending-audit_001.md")
        );
        assert!(out.propagated.is_empty());
    }

    #[test]
    fn failed_requires_sections_and_leaves_gap() {
        let tmp = tempfile::tempdir().unwrap();
        node(tmp.path(), "solo", TASK_WITH_CHECK);
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(failed(&root, "solo", "human", "", "b", "n").is_err());
        let run = failed(
            &root,
            "solo",
            "human",
            "зроблено половину",
            "впс",
            "розбити на батчі",
        )
        .unwrap();
        assert_eq!(run, "run_001.md");
        assert!(!tmp.path().join("solo/fact_001.md").exists());
    }

    #[test]
    fn done_propagates_composite_up() {
        let tmp = tempfile::tempdir().unwrap();
        node(tmp.path(), "root", TASK_WITH_CHECK);
        node(tmp.path(), "root/a", TASK_WITH_CHECK);
        node(tmp.path(), "root/b", TASK_WITH_CHECK);
        let root = tmp.path().to_string_lossy().into_owned();

        write_fact(&root, "root/a", "A готово.", None).unwrap();
        let out_a = done(&root, "root/a", "agent").unwrap();
        assert!(out_a.propagated.is_empty()); // b ще не resolved

        write_fact(&root, "root/b", "B готово.", None).unwrap();
        let out_b = done(&root, "root/b", "agent").unwrap();
        assert_eq!(out_b.propagated, ["root"]);

        let fact = fs::read_to_string(tmp.path().join("root/fact_001.md")).unwrap();
        assert!(fact.contains("A готово. B готово.") || fact.contains("B готово. A готово."));
        assert!(fact.contains("- a: ref: a/fact_001.md"));
        assert!(fact.contains("- b: ref: b/fact_001.md"));
        let run = fs::read_to_string(tmp.path().join("root/run_001.md")).unwrap();
        assert!(run.contains("actor: wrapper"));
    }
}
