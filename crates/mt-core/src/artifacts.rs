//! Version-chain артефакти вузла (§4 файловий контракт mt.md).
//!
//! Read-модель для GUI-timeline і CLI: перелік файлів `task/plan/run/fact/…`
//! з ключовими полями frontmatter, у детермінованому порядку chain:
//! `task.md` → NNN-групи (plan → run → fact → аудит-цикл) → термінальні маркери.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::frontmatter::parse_front_matter;
use crate::validate_name;

/// Тип артефакта у директорії вузла. Serde-імена збігаються з файловими
/// префіксами (kebab-case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Task,
    Plan,
    PlanApproved,
    PlanRejected,
    Run,
    Fact,
    PendingAudit,
    AuditResult,
    Clarification,
    Amended,
    Unresolvable,
    RunSummary,
}

/// Один артефакт вузла з витягом frontmatter-полів, потрібних для timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeArtifact {
    pub file: String,
    pub kind: ArtifactKind,
    /// NNN version chain; немає у `task`/`unresolvable`/`run-summary`.
    pub nnn: Option<u64>,
    pub created_at: Option<String>,
    pub actor: Option<String>,
    /// run: success|failed|progress-timeout|…; audit-result: success|failed.
    pub result: Option<String>,
    /// plan: atomic|composite.
    pub decision: Option<String>,
    pub wall_sec: Option<u64>,
    pub cost_usd: Option<f64>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
}

/// `(prefix, kind, rank)` для файлів форми `<prefix><NNN>.md`; rank —
/// порядок усередині однієї NNN-групи (план → виконання → аудит-цикл).
const NNN_KINDS: [(&str, ArtifactKind, u8); 9] = [
    ("plan_", ArtifactKind::Plan, 1),
    ("plan-rejected_", ArtifactKind::PlanRejected, 2),
    ("plan-approved_", ArtifactKind::PlanApproved, 3),
    ("run_", ArtifactKind::Run, 4),
    ("fact_", ArtifactKind::Fact, 5),
    ("pending-audit_", ArtifactKind::PendingAudit, 6),
    ("clarification_", ArtifactKind::Clarification, 7),
    ("amended_", ArtifactKind::Amended, 8),
    ("audit-result_", ArtifactKind::AuditResult, 9),
];

/// Класифікує ім'я файлу як артефакт вузла: `(kind, nnn, rank)`.
/// `None` — не артефакт (прапори `a.md`/`h.md`, чернетки, довільні файли).
fn classify(file: &str) -> Option<(ArtifactKind, Option<u64>, u8)> {
    match file {
        "task.md" => return Some((ArtifactKind::Task, None, 0)),
        "unresolvable.md" => return Some((ArtifactKind::Unresolvable, None, 10)),
        "run-summary.md" => return Some((ArtifactKind::RunSummary, None, 11)),
        _ => {}
    }
    for (prefix, kind, rank) in NNN_KINDS {
        let Some(rest) = file.strip_prefix(prefix) else {
            continue;
        };
        let Some(digits) = rest.strip_suffix(".md") else {
            continue;
        };
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return Some((kind, digits.parse().ok(), rank));
        }
    }
    None
}

fn fm_str(fm: &Value, key: &str) -> Option<String> {
    fm.get(key).and_then(Value::as_str).map(String::from)
}

/// Перелік артефактів вузла `node_path` (відносно `tasks_dir`), відсортований
/// у порядку chain: `task.md` → за NNN (у групі — за rank) → термінальні.
pub fn list_node_artifacts(tasks_dir: &str, node_path: &str) -> Result<Vec<NodeArtifact>, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    let entries = fs::read_dir(&dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;

    let mut out: Vec<(u64, u8, NodeArtifact)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((kind, nnn, rank)) = classify(&name) else {
            continue;
        };
        let fm = fs::read_to_string(entry.path())
            .map(|c| parse_front_matter(&c))
            .unwrap_or(Value::Null);
        // Групувальний ключ: task — перед chain, термінальні маркери — після.
        let group = match kind {
            ArtifactKind::Task => 0,
            ArtifactKind::Unresolvable | ArtifactKind::RunSummary => u64::MAX,
            _ => nnn.unwrap_or(0),
        };
        out.push((
            group,
            rank,
            NodeArtifact {
                file: name,
                kind,
                nnn,
                created_at: fm_str(&fm, "created_at"),
                actor: fm_str(&fm, "actor"),
                result: fm_str(&fm, "result"),
                decision: fm_str(&fm, "decision"),
                wall_sec: fm.get("wall_sec").and_then(Value::as_u64),
                cost_usd: fm.get("cost_usd").and_then(Value::as_f64),
                tokens_in: fm.get("tokens_in").and_then(Value::as_u64),
                tokens_out: fm.get("tokens_out").and_then(Value::as_u64),
            },
        ));
    }
    out.sort_by(|a, b| (a.0, a.1, &a.2.file).cmp(&(b.0, b.1, &b.2.file)));
    Ok(out.into_iter().map(|(_, _, a)| a).collect())
}

/// Безпечне читання одного артефакта вузла. `file` мусить класифікуватись як
/// артефакт — allowlist разом із [`validate_name`] гарантує, що шлях не
/// виходить за межі директорії вузла.
pub fn read_node_artifact(tasks_dir: &str, node_path: &str, file: &str) -> Result<String, String> {
    validate_name(node_path)?;
    if classify(file).is_none() {
        return Err(format!("not a node artifact: {file:?}"));
    }
    let path = Path::new(tasks_dir).join(node_path).join(file);
    fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let node = tmp.path().join("analyze");
        fs::create_dir_all(node.join("deps")).unwrap();
        write(
            &node,
            "task.md",
            "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\n---\n\n## Task\n",
        );
        write(&node, "a.md", "schema_version: 1\n");
        write(
            &node,
            "plan_001.md",
            "---\nschema_version: 1\ndecision: atomic\n---\n",
        );
        write(
            &node,
            "run_001.md",
            "---\nschema_version: 1\nactor: agent\nresult: failed\nwall_sec: 120\ncost_usd: 0.15\n---\n",
        );
        write(
            &node,
            "run_002.md",
            "---\nschema_version: 1\nactor: agent\nresult: success\nwall_sec: 300\n---\n",
        );
        write(
            &node,
            "fact_002.md",
            "---\nschema_version: 1\n---\n\n## Summary\n",
        );
        write(
            &node,
            "pending-audit_002.md",
            "---\nschema_version: 1\nactor: agent\n---\n",
        );
        write(
            &node,
            "audit-result_002.md",
            "---\nschema_version: 1\nactor: auditor\nresult: success\n---\n",
        );
        write(&node, "run-draft.md", "## Completed\n");
        tmp
    }

    #[test]
    fn lists_chain_in_order_and_parses_frontmatter() {
        let tmp = fixture();
        let arts = list_node_artifacts(&tmp.path().to_string_lossy(), "analyze").unwrap();
        let files: Vec<&str> = arts.iter().map(|a| a.file.as_str()).collect();
        assert_eq!(
            files,
            [
                "task.md",
                "plan_001.md",
                "run_001.md",
                "run_002.md",
                "fact_002.md",
                "pending-audit_002.md",
                "audit-result_002.md",
            ]
        );
        let run1 = &arts[2];
        assert_eq!(run1.kind, ArtifactKind::Run);
        assert_eq!(run1.nnn, Some(1));
        assert_eq!(run1.result.as_deref(), Some("failed"));
        assert_eq!(run1.wall_sec, Some(120));
        assert_eq!(run1.cost_usd, Some(0.15));
        let audit = arts.last().unwrap();
        assert_eq!(audit.kind, ArtifactKind::AuditResult);
        assert_eq!(audit.actor.as_deref(), Some("auditor"));
    }

    #[test]
    fn skips_flags_drafts_and_dirs() {
        let tmp = fixture();
        let arts = list_node_artifacts(&tmp.path().to_string_lossy(), "analyze").unwrap();
        assert!(arts
            .iter()
            .all(|a| a.file != "a.md" && a.file != "run-draft.md"));
    }

    #[test]
    fn read_artifact_allows_only_classified_files() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(read_node_artifact(&root, "analyze", "task.md").is_ok());
        assert!(read_node_artifact(&root, "analyze", "a.md").is_err());
        assert!(read_node_artifact(&root, "analyze", "../analyze/task.md").is_err());
        assert!(read_node_artifact(&root, "../x", "task.md").is_err());
    }

    #[test]
    fn terminal_markers_sort_last() {
        let tmp = fixture();
        let node = tmp.path().join("analyze");
        write(&node, "unresolvable.md", "---\nschema_version: 1\n---\n");
        write(
            &node,
            "run-summary.md",
            "---\nschema_version: 1\nactor: wrapper\n---\n",
        );
        let arts = list_node_artifacts(&tmp.path().to_string_lossy(), "analyze").unwrap();
        let tail: Vec<&str> = arts.iter().rev().take(2).map(|a| a.file.as_str()).collect();
        assert_eq!(tail, ["run-summary.md", "unresolvable.md"]);
    }
}
