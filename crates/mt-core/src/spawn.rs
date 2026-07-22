//! Протокол spawn — plan-review рішення (спека mt.md, «Протокол spawn»).
//!
//! `## Children` актуального `plan_NNN.md` — єдине джерело структури підграфу.
//! `spawn_approve` валідує специфікацію і матеріалізує дітей + `plan-approved_NNN.md`;
//! `spawn_reject` пише `plan-rejected_NNN.md` із причиною. Без approve —
//! жодних дочірніх вузлів (правило легітимності).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::nnn::pad_nnn;
use crate::{create_task, validate_name, write_atomic, CreateOpts, Mode};

/// Специфікація однієї дитини з `## Children` (спека: mode — обов'язковий).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChildSpec {
    pub id: String,
    pub mode: Option<String>,
    pub model_tier: Option<String>,
    pub skills: Vec<String>,
    pub qualification: Option<String>,
    pub budget_sec: Option<u64>,
    /// `export: false` → дитина не потрапляє у `## children` fact батька.
    pub export: bool,
    /// Сусіди — голий id; cross-level — шлях відносно tasks root.
    pub deps: Vec<String>,
    pub task: Option<String>,
}

/// Read-модель plan-review для GUI: актуальний план і його `## Children`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanReview {
    pub plan_file: String,
    pub nnn: u64,
    pub decision: Option<String>,
    pub decided: bool,
    pub children: Vec<ChildSpec>,
}

/// Результат `spawn_approve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnOutcome {
    pub approved_file: String,
    pub children: Vec<String>,
}

fn node_dir(tasks_dir: &str, node_path: &str) -> Result<PathBuf, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    Ok(dir)
}

/// Актуальний план: `plan_NNN.md` з max NNN.
fn latest_plan(dir: &Path) -> Option<(u64, String, String)> {
    let mut best: Option<(u64, String)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(digits) = name
            .strip_prefix("plan_")
            .and_then(|r| r.strip_suffix(".md"))
        else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let n: u64 = digits.parse().ok()?;
        if best.as_ref().is_none_or(|(b, _)| n > *b) {
            best = Some((n, name));
        }
    }
    let (nnn, file) = best?;
    let content = fs::read_to_string(dir.join(&file)).ok()?;
    Some((nnn, file, content))
}

/// Вирізає тіло секції `## Children` (до наступного `## `-заголовка).
fn children_section(plan: &str) -> Option<String> {
    let mut lines = plan.lines();
    let mut section = Vec::new();
    let mut inside = false;
    for line in lines.by_ref() {
        if line.trim() == "## Children" {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with("## ") {
                break;
            }
            section.push(line);
        }
    }
    if section.is_empty() {
        return None;
    }
    // Fenced-блок усередині секції → беремо його вміст; інакше секцію цілком.
    let text = section.join("\n");
    if let Some(start) = text.find("```") {
        let after = &text[start..];
        let body_start = after.find('\n')? + start + 1;
        let body = &text[body_start..];
        let end = body.find("```").unwrap_or(body.len());
        return Some(body[..end].to_string());
    }
    Some(text)
}

/// Інлайн-масив `[a, b]` → елементи; порожній `[]` → порожньо.
fn parse_inline_list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn indent_of(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// Парсер підмножини YAML для `children:` — список об'єктів зі скалярами,
/// інлайн-масивами та блоковими скалярами `|` (частковий парсер frontmatter
/// списки об'єктів не підтримує).
pub fn parse_children(yaml: &str) -> Result<Vec<ChildSpec>, String> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut children: Vec<ChildSpec> = Vec::new();
    let mut i = 0;

    // Пропускаємо все до ключа children:
    while i < lines.len() && lines[i].trim() != "children:" {
        i += 1;
    }
    if i >= lines.len() {
        return Err("## Children: ключ `children:` не знайдено".to_string());
    }
    i += 1;

    let mut item_indent = None;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        let indent = indent_of(line);
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("- ") {
            // Новий елемент списку.
            item_indent = Some(indent);
            children.push(ChildSpec {
                id: String::new(),
                mode: None,
                model_tier: None,
                skills: Vec::new(),
                qualification: None,
                budget_sec: None,
                export: true,
                deps: Vec::new(),
                task: None,
            });
            apply_field(
                children.last_mut().unwrap(),
                rest,
                &lines,
                &mut i,
                indent + 2,
            )?;
        } else if item_indent.is_some_and(|ii| indent > ii) {
            let Some(child) = children.last_mut() else {
                return Err("## Children: поле поза елементом списку".to_string());
            };
            apply_field(child, trimmed, &lines, &mut i, indent)?;
        } else {
            break; // вихід із блоку children
        }
        i += 1;
    }
    Ok(children)
}

/// Застосовує один рядок `key: value` до дитини; для `task: |` збирає
/// блоковий скаляр (рядки з більшим відступом), пересуваючи курсор `i`.
fn apply_field(
    child: &mut ChildSpec,
    field: &str,
    lines: &[&str],
    i: &mut usize,
    field_indent: usize,
) -> Result<(), String> {
    let Some((key, raw)) = field.split_once(':') else {
        return Err(format!(
            "## Children: очікував `key: value`, отримав {field:?}"
        ));
    };
    let key = key.trim();
    let value = raw.split('#').next().unwrap_or("").trim().to_string();

    if value == "|" || value == "|-" {
        // Блоковий скаляр: рядки з відступом > field_indent.
        let mut block = Vec::new();
        while *i + 1 < lines.len() {
            let next = lines[*i + 1];
            if !next.trim().is_empty() && indent_of(next) <= field_indent {
                break;
            }
            block.push(next.trim().to_string());
            *i += 1;
        }
        let text = block.join("\n").trim().to_string();
        if key == "task" {
            child.task = Some(text);
        }
        return Ok(());
    }

    match key {
        "id" => child.id = value,
        "mode" => child.mode = Some(value),
        "model_tier" => child.model_tier = Some(value),
        "qualification" => child.qualification = Some(value),
        "task" => child.task = Some(value),
        "budget_sec" => {
            child.budget_sec = Some(
                value
                    .parse()
                    .map_err(|_| format!("## Children: budget_sec не число: {value:?}"))?,
            )
        }
        "export" => child.export = value != "false",
        "skills" => child.skills = parse_inline_list(&value),
        "deps" => child.deps = parse_inline_list(&value),
        "audit" => {} // приймаємо без обробки: create_task поки не пише audit
        _ => {}       // невідомі поля — толерантно ігноруємо
    }
    Ok(())
}

/// Read-модель plan-review вузла: актуальний план + розібрані `## Children`.
pub fn plan_review(tasks_dir: &str, node_path: &str) -> Result<PlanReview, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let (nnn, plan_file, content) =
        latest_plan(&dir).ok_or_else(|| format!("no plan_NNN.md in {node_path}"))?;
    let fm = crate::frontmatter::parse_front_matter(&content);
    let decision = fm
        .get("decision")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    let children = match children_section(&content) {
        Some(yaml) => parse_children(&yaml)?,
        None => Vec::new(),
    };
    let nnn_s = pad_nnn(nnn);
    let decided = dir.join(format!("plan-approved_{nnn_s}.md")).exists()
        || dir.join(format!("plan-rejected_{nnn_s}.md")).exists();
    Ok(PlanReview {
        plan_file,
        nnn,
        decision,
        decided,
        children,
    })
}

/// Валідація `## Children` за спекою: id/naming, mode per-child, deps
/// існують (сусід у списку або cross-level вузол на диску), циклів немає.
fn validate_children(tasks_dir: &str, children: &[ChildSpec]) -> Result<(), String> {
    if children.is_empty() {
        return Err("## Children порожня — немає що матеріалізувати".to_string());
    }
    let ids: HashSet<&str> = children.iter().map(|c| c.id.as_str()).collect();
    if ids.len() != children.len() {
        return Err("## Children: дублікати id".to_string());
    }
    for child in children {
        validate_name(&child.id)?;
        if child.id.contains('/') {
            return Err(format!("child id must be a single segment: {:?}", child.id));
        }
        match child.mode.as_deref() {
            Some("agent") | Some("human") => {}
            Some(other) => return Err(format!("child {:?}: невалідний mode {other:?}", child.id)),
            None => return Err(format!("child {:?}: mode обов'язковий per-child", child.id)),
        }
        for dep in &child.deps {
            let sibling = ids.contains(dep.as_str());
            let cross = Path::new(tasks_dir).join(dep).join("task.md").is_file();
            if !sibling && !cross {
                return Err(format!("child {:?}: dep {dep:?} не існує", child.id));
            }
        }
    }
    // Цикли серед сусідів: DFS по sibling-ребрах.
    fn dfs<'a>(
        id: &'a str,
        children: &'a [ChildSpec],
        visiting: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(format!("## Children: цикл через {id:?}"));
        }
        if let Some(child) = children.iter().find(|c| c.id == id) {
            for dep in &child.deps {
                if children.iter().any(|c| c.id == *dep) {
                    dfs(dep, children, visiting, done)?;
                }
            }
        }
        visiting.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = HashSet::new();
    for child in children {
        dfs(&child.id, children, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

fn guard_undecided(dir: &Path, nnn: u64) -> Result<String, String> {
    let nnn_s = pad_nnn(nnn);
    if dir.join(format!("plan-approved_{nnn_s}.md")).exists() {
        return Err(format!("plan {nnn_s} вже approved"));
    }
    if dir.join(format!("plan-rejected_{nnn_s}.md")).exists() {
        return Err(format!("plan {nnn_s} вже rejected"));
    }
    Ok(nnn_s)
}

fn decision_frontmatter() -> String {
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    format!("---\nschema_version: 1\ncreated_at: {created_at}\n---\n")
}

/// `mt spawn --approve`: валідує `## Children` актуального плану,
/// матеріалізує дітей (task.md + прапор + deps/) і пише `plan-approved_NNN.md`.
pub fn spawn_approve(tasks_dir: &str, node_path: &str) -> Result<SpawnOutcome, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let review = plan_review(tasks_dir, node_path)?;
    if review.decision.as_deref() != Some("composite") {
        return Err(format!(
            "актуальний план {} не composite — spawn не застосовний",
            review.plan_file
        ));
    }
    let nnn_s = guard_undecided(&dir, review.nnn)?;
    validate_children(tasks_dir, &review.children)?;

    let mut created = Vec::new();
    for child in &review.children {
        let deps = child
            .deps
            .iter()
            .map(|dep| {
                if dep.contains('/') {
                    dep.clone() // cross-level: шлях відносно tasks root
                } else {
                    format!("{node_path}/{dep}") // сусід: повний шлях
                }
            })
            .collect();
        let mode = match child.mode.as_deref() {
            Some("human") => Mode::Human,
            _ => Mode::Agent,
        };
        create_task(
            tasks_dir.to_string(),
            format!("{node_path}/{}", child.id),
            CreateOpts {
                mode: Some(mode),
                model_tier: child.model_tier.clone(),
                budget_sec: child.budget_sec,
                hint: None,
                deps,
                skills: (!child.skills.is_empty()).then(|| child.skills.clone()),
                task: child.task.clone(),
                qualification: child.qualification.clone(),
            },
        )?;
        created.push(child.id.clone());
    }

    let approved_file = format!("plan-approved_{nnn_s}.md");
    let list = created
        .iter()
        .map(|id| format!("- {id}"))
        .collect::<Vec<_>>()
        .join("\n");
    write_atomic(
        &dir.join(&approved_file),
        &format!("{}\n## Children\n\n{list}\n", decision_frontmatter()),
    )?;
    Ok(SpawnOutcome {
        approved_file,
        children: created,
    })
}

/// `mt spawn --reject --reason`: пише `plan-rejected_NNN.md`; вузол
/// derived-повертається у `waiting`, наступний план бачить причину.
pub fn spawn_reject(tasks_dir: &str, node_path: &str, reason: &str) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let (nnn, _, _) = latest_plan(&dir).ok_or_else(|| format!("no plan_NNN.md in {node_path}"))?;
    let nnn_s = guard_undecided(&dir, nnn)?;
    let rejected_file = format!("plan-rejected_{nnn_s}.md");
    write_atomic(
        &dir.join(&rejected_file),
        &format!(
            "{}\n## Reason\n\n{}\n",
            decision_frontmatter(),
            reason.trim()
        ),
    )?;
    Ok(rejected_file)
}

/// Перемикає виконавця вузла: пише `a.md`/`h.md`, видаляє протилежний прапор.
pub fn set_executor(
    tasks_dir: &str,
    node_path: &str,
    mode: Mode,
    model_tier: Option<&str>,
    skills: Option<&[String]>,
    qualification: Option<&str>,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let default_skills = ["bash".to_string(), "write-files".to_string()];
    let flag = crate::write_executor_flag(
        &dir,
        mode,
        model_tier.unwrap_or("AVG"),
        skills.unwrap_or(&default_skills),
        qualification,
    )?;
    Ok(flag.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\ndecision: composite\n---\n\n## Context\n\nx\n\n## Children\n\n```yaml\nchildren:\n  - id: collect-data\n    mode: agent\n    model_tier: AVG\n    skills: [bash, web-search]\n    budget_sec: 1800\n    deps: []\n    task: |\n      Зібрати дані з API за Q4\n  - id: analyze\n    mode: human\n    qualification: senior analyst\n    export: false\n    deps: [collect-data]\n    task: Перевірити аномалії\n```\n\n## Risks\n\ny\n";

    fn fixture() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        let node = tmp.path().join("research");
        fs::create_dir_all(&node).unwrap();
        fs::write(
            node.join("task.md"),
            "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\nbudget_sec: 600\n---\n\n## Task\n",
        )
        .unwrap();
        fs::write(node.join("plan_001.md"), PLAN).unwrap();
        (tmp, "research".to_string())
    }

    #[test]
    fn parses_children_specs() {
        let review_yaml = children_section(PLAN).unwrap();
        let children = parse_children(&review_yaml).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].id, "collect-data");
        assert_eq!(children[0].skills, ["bash", "web-search"]);
        assert_eq!(children[0].budget_sec, Some(1800));
        assert_eq!(
            children[0].task.as_deref(),
            Some("Зібрати дані з API за Q4")
        );
        assert!(children[0].export);
        assert_eq!(children[1].mode.as_deref(), Some("human"));
        assert!(!children[1].export);
        assert_eq!(children[1].deps, ["collect-data"]);
    }

    #[test]
    fn approve_materializes_children_and_writes_sentinel() {
        let (tmp, node) = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let out = spawn_approve(&root, &node).unwrap();
        assert_eq!(out.children, ["collect-data", "analyze"]);

        let collect = tmp.path().join("research/collect-data");
        assert!(collect.join("task.md").is_file());
        assert!(collect.join("a.md").is_file());
        let task = fs::read_to_string(collect.join("task.md")).unwrap();
        assert!(task.contains("Зібрати дані з API за Q4"));

        let analyze = tmp.path().join("research/analyze");
        assert!(analyze.join("h.md").is_file());
        // Сусідній dep матеріалізовано повним шляхом від tasks root.
        assert!(analyze.join("deps/research/collect-data.md").is_file());

        assert!(tmp.path().join("research/plan-approved_001.md").is_file());
        // Повторний approve → відмова.
        assert!(spawn_approve(&root, &node).is_err());
    }

    #[test]
    fn reject_writes_reason_and_blocks_second_decision() {
        let (tmp, node) = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let file = spawn_reject(&root, &node, "занадто дрібна декомпозиція").unwrap();
        assert_eq!(file, "plan-rejected_001.md");
        let content = fs::read_to_string(tmp.path().join("research").join(file)).unwrap();
        assert!(content.contains("занадто дрібна декомпозиція"));
        assert!(spawn_approve(&root, &node).is_err());
    }

    #[test]
    fn validation_rejects_missing_mode_and_cycles() {
        let no_mode = parse_children("children:\n  - id: a\n").unwrap();
        assert!(validate_children("/nonexistent", &no_mode).is_err());

        let cyclic = parse_children(
            "children:\n  - id: a\n    mode: agent\n    deps: [b]\n  - id: b\n    mode: agent\n    deps: [a]\n",
        )
        .unwrap();
        let err = validate_children("/nonexistent", &cyclic).unwrap_err();
        assert!(err.contains("цикл"));
    }

    #[test]
    fn set_executor_switches_flags() {
        let (tmp, node) = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        assert_eq!(
            set_executor(&root, &node, Mode::Agent, Some("MAX"), None, None).unwrap(),
            "a.md"
        );
        assert!(tmp.path().join("research/a.md").is_file());
        assert_eq!(
            set_executor(&root, &node, Mode::Human, None, None, Some("senior")).unwrap(),
            "h.md"
        );
        assert!(tmp.path().join("research/h.md").is_file());
        assert!(!tmp.path().join("research/a.md").exists());
    }
}
