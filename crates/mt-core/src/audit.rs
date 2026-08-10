//! Аудит-цикл (graph.md, «Аудит (async черга)») — закриття гейта, який
//! відкриває `mt audit`.
//!
//! ```text
//! fact_NNN → mt audit → pending-audit_NNN → аудитор:
//!   success       → audit-result_NNN (success) → resolved
//!   failed        → audit-result_NNN (failed)  → waiting (rework, run N+1)
//!   clarification → amended_NNN → повторний аудит → фінальний вердикт
//! ```
//!
//! Уточнення — **не вердикт**: воно не закриває цикл, і його можна
//! попросити лише один раз. Прострочене уточнення без `amended_NNN.md`
//! матеріалізується як `failed` — але з явною позначкою, що це дія
//! політики, а не судження аудитора.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{max_nnn, validate_name, write_atomic};

/// Відкритий аудит-цикл вузла: NNN pending-audit без вердикту.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAudit {
    pub nnn: u64,
    pub pending_file: String,
    /// Уточнення вже запитано (повторно не можна — спека).
    pub clarification_file: Option<String>,
    /// Відповідь агента на уточнення.
    pub amended_file: Option<String>,
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn node_dir(tasks_dir: &str, node_path: &str) -> Result<PathBuf, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    Ok(dir)
}

fn existing(dir: &Path, name: String) -> Option<String> {
    dir.join(&name).is_file().then_some(name)
}

/// Відкритий цикл вузла або `None`. Відкритий = є `pending-audit_NNN.md`
/// з максимальним NNN і немає відповідного `audit-result_NNN.md`.
pub fn open_audit(dir: &Path) -> Option<OpenAudit> {
    let nnn = max_nnn(dir, "pending-audit_", ".md");
    if nnn == 0 || dir.join(format!("audit-result_{nnn:03}.md")).is_file() {
        return None;
    }
    Some(OpenAudit {
        nnn,
        pending_file: format!("pending-audit_{nnn:03}.md"),
        clarification_file: existing(dir, format!("clarification_{nnn:03}.md")),
        amended_file: existing(dir, format!("amended_{nnn:03}.md")),
    })
}

/// Публікує артефакт аудиту в `main` — вердикт має бути видимим усім, хто
/// читає граф, інакше стан вузла розходиться між машинами. Fail-open поза
/// git-репозиторієм, як і решта lifecycle-операцій.
fn publish_artifact(tasks_dir: &str, dir: &Path, file: &str, message: &str) -> Result<(), String> {
    let Ok(repo_root) = crate::claims::discover_main_worktree_root(Path::new(tasks_dir)) else {
        return Ok(());
    };
    let abs = dir.join(file);
    let Ok(rel) = abs.strip_prefix(&repo_root) else {
        return Ok(());
    };
    let content = fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    let config = crate::config::merge_config(
        fs::read_to_string(repo_root.join(".mt.json")).ok().as_deref(),
    );
    let outcome = crate::publish::publish_lifecycle(
        &repo_root,
        &crate::runner::worktrees_dir_path(&repo_root, &config),
        &[(rel.to_string_lossy().replace('\\', "/"), Some(content))],
        message,
        config
            .get("publish_retry_max")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(8) as u32,
        config
            .get("publish_retry_base_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(250),
    )?;
    if !outcome.published {
        return Err(format!(
            "audit: вичерпано publish retry — {file} лишився локальним, повторіть пізніше"
        ));
    }
    Ok(())
}

/// Вердикт аудитора: `audit-result_NNN.md` (graph.md — пишеться **виключно**
/// аудитором; `failed` повертає вузол у `waiting` на rework).
///
/// `auto_by_policy` позначає вердикт, який виніс не аудитор, а таймаут
/// уточнення: у трейлі має бути видно різницю між судженням і дією політики.
pub fn verdict(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    success: bool,
    reasoning: &str,
    auto_by_policy: bool,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let open = open_audit(&dir).ok_or_else(|| {
        format!("{node_path}: немає відкритого аудит-циклу — вердикт нема куди писати")
    })?;
    if reasoning.trim().is_empty() {
        return Err("## Reasoning обов'язковий для вердикту аудиту".to_string());
    }

    let nnn = open.nnn;
    let file = format!("audit-result_{nnn:03}.md");
    let result = if success { "success" } else { "failed" };
    let policy_line = if auto_by_policy {
        "auto_by_policy: true\n"
    } else {
        ""
    };
    write_atomic(
        &dir.join(&file),
        &format!(
            "---\nschema_version: 1\ncreated_at: {}\nactor: {actor}\nresult: {result}\n{policy_line}---\n\n## Reasoning\n\n{}\n",
            now_iso(),
            reasoning.trim()
        ),
    )?;
    publish_artifact(
        tasks_dir,
        &dir,
        &file,
        &format!("mt: audit {node_path} {nnn:03} ({result})"),
    )?;
    Ok(file)
}

/// Запит уточнення — **не вердикт**: цикл лишається відкритим. Дозволений
/// один раз (спека), інакше аудитор міг би нескінченно тримати вузол у
/// `pending-audit`, не ухвалюючи рішення.
pub fn clarification(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    question: &str,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let open = open_audit(&dir).ok_or_else(|| {
        format!("{node_path}: немає відкритого аудит-циклу — уточнення нема до чого")
    })?;
    if open.clarification_file.is_some() {
        return Err(format!(
            "{node_path}: уточнення вже запитано — спека дозволяє лише одне на цикл"
        ));
    }
    if question.trim().is_empty() {
        return Err("текст уточнення обов'язковий".to_string());
    }

    let nnn = open.nnn;
    let file = format!("clarification_{nnn:03}.md");
    write_atomic(
        &dir.join(&file),
        &format!(
            "---\nschema_version: 1\ncreated_at: {}\nactor: {actor}\n---\n\n## Question\n\n{}\n",
            now_iso(),
            question.trim()
        ),
    )?;
    publish_artifact(
        tasks_dir,
        &dir,
        &file,
        &format!("mt: clarification {node_path} {nnn:03}"),
    )?;
    Ok(file)
}

/// Відповідь виконавця на уточнення: `amended_NNN.md` (NNN — того самого
/// циклу). Після неї аудит повторюється й має завершитись вердиктом.
pub fn amend(
    tasks_dir: &str,
    node_path: &str,
    actor: &str,
    answer: &str,
) -> Result<String, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let open = open_audit(&dir)
        .ok_or_else(|| format!("{node_path}: немає відкритого аудит-циклу"))?;
    if open.clarification_file.is_none() {
        return Err(format!(
            "{node_path}: уточнення не запитували — відповідати нема на що"
        ));
    }
    if open.amended_file.is_some() {
        return Err(format!("{node_path}: відповідь на уточнення вже є"));
    }
    if answer.trim().is_empty() {
        return Err("текст відповіді обов'язковий".to_string());
    }

    let nnn = open.nnn;
    let file = format!("amended_{nnn:03}.md");
    write_atomic(
        &dir.join(&file),
        &format!(
            "---\nschema_version: 1\ncreated_at: {}\nactor: {actor}\n---\n\n## Answer\n\n{}\n",
            now_iso(),
            answer.trim()
        ),
    )?;
    publish_artifact(
        tasks_dir,
        &dir,
        &file,
        &format!("mt: amended {node_path} {nnn:03}"),
    )?;
    Ok(file)
}

/// Прострочене уточнення → авто-вердикт `failed` (graph.md: timeout
/// `clarification_timeout_sec` без `amended_NNN.md`).
///
/// Повертає ім'я записаного вердикту або `None`, якщо чекати ще є сенс:
/// цикл закритий, уточнення не запитували, відповідь надійшла або строк не
/// вийшов.
pub fn expire_clarification(
    tasks_dir: &str,
    node_path: &str,
    timeout_sec: u64,
) -> Result<Option<String>, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let Some(open) = open_audit(&dir) else {
        return Ok(None);
    };
    let (Some(clarification), None) = (&open.clarification_file, &open.amended_file) else {
        return Ok(None);
    };

    let content = fs::read_to_string(dir.join(clarification)).map_err(|e| e.to_string())?;
    let asked_at = crate::frontmatter::parse_front_matter(&content)
        .get("created_at")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .ok_or_else(|| format!("{clarification}: немає розбірного created_at"))?;
    let elapsed = chrono::Utc::now()
        .signed_duration_since(asked_at.with_timezone(&chrono::Utc))
        .num_seconds();
    if elapsed < timeout_sec as i64 {
        return Ok(None);
    }

    verdict(
        tasks_dir,
        node_path,
        "wrapper",
        false,
        &format!(
            "Уточнення `{clarification}` лишилось без відповіді понад {timeout_sec}s. \
             Вердикт винесено політикою, не аудитором."
        ),
        true,
    )
    .map(Some)
}

/// Кількість поспіль відхилених аудитів (graph.md, `audit_failed_streak`) —
/// рахується від найбільшого NNN вниз і уривається першим `success`.
///
/// Окремий від `failed_streak` лічильник: провал виконання і провал
/// приймання — різні осі, і вичерпання однієї не має витрачати іншу.
pub fn audit_failed_streak(dir: &Path) -> u64 {
    let mut streak = 0;
    for nnn in (1..=max_nnn(dir, "audit-result_", ".md")).rev() {
        let path = dir.join(format!("audit-result_{nnn:03}.md"));
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        match crate::frontmatter::parse_front_matter(&content)
            .get("result")
            .and_then(serde_json::Value::as_str)
        {
            Some("failed") => streak += 1,
            Some("success") => break,
            _ => break,
        }
    }
    streak
}

/// Чи вичерпано драбину приймання — час ескалювати до людини (graph.md:
/// `audit_failed_streak ≥ audit_retry_max`).
///
/// Сама доставка ескалації — не тут: це `decision-request` мандатів (M6) або
/// relay-push. Тут лише детермінований предикат, спільний для обох шляхів.
pub fn audit_escalation_due(dir: &Path, config: &serde_json::Value) -> bool {
    let max = config
        .get("audit_retry_max")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2);
    audit_failed_streak(dir) >= max
}

/// Результат прогону агента-аудитора.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRun {
    /// Що аудитор написав: `audit-result_NNN.md` або `clarification_NNN.md`.
    pub artifact: Option<String>,
    pub nnn: u64,
    pub agent_cli: Option<String>,
}

/// Прогін агента-аудитора над відкритим циклом (graph.md, `mt run --actor
/// auditor`).
///
/// Свідома відмінність від `run_node`: без claim і без ефемерного worktree.
/// Аудит нічого не виконує — він читає контракт і результат та пише один
/// артефакт, тож фенсити нема чого, а зайвий worktree лише коштував би
/// часу на кожен вузол із `audit: required`.
///
/// Модель береться з `audit_model` (`.mt.json`), інакше — звичайний тир
/// вузла: аудит зазвичай дешевший за виконання, але політика вирішує.
pub fn run_auditor(
    tasks_dir: &str,
    node_path: &str,
    cli_env: &crate::config::AgentCliEnv,
) -> Result<AuditRun, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    let open = open_audit(&dir).ok_or_else(|| {
        format!("{node_path}: немає відкритого аудит-циклу — аудитувати нема чого")
    })?;
    if open.clarification_file.is_some() && open.amended_file.is_none() {
        return Err(format!(
            "{node_path}: чекає на відповідь виконавця (`mt amend`) — аудит зупинено"
        ));
    }

    let config = crate::config::merge_config(
        fs::read_to_string(Path::new(tasks_dir).join("../.mt.json"))
            .ok()
            .as_deref(),
    );
    let tier = config
        .get("audit_model")
        .and_then(serde_json::Value::as_str)
        .map(crate::config::normalize_model_tier)
        .unwrap_or_else(|| "AVG".to_string());

    let prompt = build_auditor_prompt(node_path, &dir, open.nnn);
    let agent_cli = crate::runner::run_single_phase(&dir, cli_env, &tier, &prompt)?;

    // Вердикт/уточнення пише сам аудитор через CLI — читаємо, що вийшло.
    let after = open_audit(&dir);
    let artifact = match &after {
        None => Some(format!("audit-result_{:03}.md", open.nnn)),
        Some(state) => state.clarification_file.clone(),
    };
    Ok(AuditRun {
        artifact,
        nnn: open.nnn,
        agent_cli,
    })
}

/// Промпт аудитора (graph.md: аудитором може бути агент за `audit_model`).
///
/// Аудитор бачить контракт і результат, але **не** історію спроб: його
/// питання — «чи відповідає fact тому, що обіцяв `## Done when`», а не
/// «чи важко було це зробити». Знання про муки виконавця тут лише зсуває
/// планку.
pub fn build_auditor_prompt(node_path: &str, dir: &Path, nnn: u64) -> String {
    let body = |name: &str| -> Option<String> {
        let content = fs::read_to_string(dir.join(name)).ok()?;
        let body = crate::frontmatter::get_body(&content);
        let text = if body.trim().is_empty() { content } else { body };
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    };

    let mut blocks = vec![format!(
        "## Аудит вузла: {node_path}\n\nРобоча директорія: {}\nЦикл: {nnn:03}",
        dir.display()
    )];
    if let Some(task) = body("task.md") {
        blocks.push(format!("## Контракт (task.md)\n\n{task}"));
    }
    if let Some(fact) = body(&format!("fact_{nnn:03}.md")) {
        blocks.push(format!("## Результат (fact_{nnn:03}.md)\n\n{fact}"));
    }
    if let Some(amended) = body(&format!("amended_{nnn:03}.md")) {
        blocks.push(format!("## Відповідь на твоє уточнення\n\n{amended}"));
    }
    blocks.push(format!(
        "## Що зробити\n\nЗвір результат із секцією `## Done when` контракту й ухвали рішення \
         однією з команд у цій директорії:\n\n\
         - `mt verdict {node_path} --success --reason \"<чому приймаєш>\"`\n\
         - `mt verdict {node_path} --reason \"<чого бракує>\"` — відхилити на доопрацювання\n\
         - `mt clarify {node_path} --question \"<що незрозуміло>\"` — лише якщо без відповіді \
           рішення ухвалити неможливо; дозволено один раз за цикл\n\n\
         Оцінюй відповідність контракту, а не складність шляху до неї."
    ));
    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\n---\n\n## Task\n\nx\n";

    /// Вузол із відкритим аудит-циклом на NNN=001.
    fn node_with_open_audit() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        fs::write(dir.join("fact_001.md"), "---\n---\n\n## Summary\n\nok\n").unwrap();
        fs::write(dir.join("pending-audit_001.md"), "---\nschema_version: 1\n---\n").unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        (tmp, root)
    }

    #[test]
    fn auditor_prompt_judges_contract_not_effort() {
        let (tmp, _root) = node_with_open_audit();
        let dir = tmp.path().join("solo");
        fs::write(
            dir.join("task.md"),
            "---\nschema_version: 1\n---\n\n## Task\n\nx\n\n## Done when\n\nтести зелені\n",
        )
        .unwrap();
        fs::write(dir.join("run_001.md"), "---\nresult: failed\n---\n\n## Blockers\n\nмучився довго\n").unwrap();

        let p = build_auditor_prompt("solo", &dir, 1);
        assert!(p.contains("Контракт (task.md)") && p.contains("тести зелені"));
        assert!(p.contains("Результат (fact_001.md)"));
        assert!(p.contains("mt verdict") && p.contains("mt clarify"));
        // Історія спроб аудитору не показується — вона зсуває планку.
        assert!(!p.contains("мучився довго"), "got: {p}");
    }

    #[test]
    fn auditor_refuses_without_open_cycle_or_pending_answer() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        let env = crate::config::AgentCliEnv::default();
        assert!(run_auditor(&root, "solo", &env)
            .unwrap_err()
            .contains("немає відкритого аудит-циклу"));

        // Уточнення без відповіді — аудит зупинено, м'яч на боці виконавця.
        let (_t, root2) = node_with_open_audit();
        clarification(&root2, "solo", "auditor", "Чому?").unwrap();
        assert!(run_auditor(&root2, "solo", &env)
            .unwrap_err()
            .contains("чекає на відповідь виконавця"));
    }

    #[test]
    fn verdict_closes_the_cycle() {
        let (tmp, root) = node_with_open_audit();
        let file = verdict(&root, "solo", "auditor", true, "Покриття достатнє.", false).unwrap();
        assert_eq!(file, "audit-result_001.md");

        let content = fs::read_to_string(tmp.path().join("solo").join(&file)).unwrap();
        assert!(content.contains("result: success"));
        assert!(content.contains("actor: auditor"));
        assert!(content.contains("Покриття достатнє"));
        assert!(!content.contains("auto_by_policy"));
        // Цикл закрито — другий вердикт нема куди писати.
        assert!(verdict(&root, "solo", "auditor", true, "ще раз", false).is_err());
    }

    #[test]
    fn verdict_requires_open_cycle_and_reasoning() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(verdict(&root, "solo", "auditor", true, "ok", false)
            .unwrap_err()
            .contains("немає відкритого аудит-циклу"));

        let (_t, root2) = node_with_open_audit();
        assert!(verdict(&root2, "solo", "auditor", true, "   ", false)
            .unwrap_err()
            .contains("Reasoning"));
    }

    #[test]
    fn clarification_is_not_a_verdict_and_only_once() {
        let (tmp, root) = node_with_open_audit();
        clarification(&root, "solo", "auditor", "Чому цей алгоритм?").unwrap();
        // Цикл лишається відкритим.
        assert!(open_audit(&tmp.path().join("solo")).is_some());
        // Друге уточнення заборонене — інакше вузол можна тримати вічно.
        assert!(clarification(&root, "solo", "auditor", "А ще чому?")
            .unwrap_err()
            .contains("лише одне"));
    }

    #[test]
    fn amend_requires_a_question_first() {
        let (_tmp, root) = node_with_open_audit();
        assert!(amend(&root, "solo", "agent", "бо так")
            .unwrap_err()
            .contains("не запитували"));

        clarification(&root, "solo", "auditor", "Чому?").unwrap();
        assert_eq!(amend(&root, "solo", "agent", "бо так").unwrap(), "amended_001.md");
        assert!(amend(&root, "solo", "agent", "і ще").is_err());
    }

    #[test]
    fn expired_clarification_fails_by_policy() {
        let (tmp, root) = node_with_open_audit();
        let dir = tmp.path().join("solo");
        fs::write(
            dir.join("clarification_001.md"),
            "---\nschema_version: 1\ncreated_at: 2020-01-01T00:00:00Z\n---\n\n## Question\n\nЧому?\n",
        )
        .unwrap();

        let file = expire_clarification(&root, "solo", 3600).unwrap().unwrap();
        let content = fs::read_to_string(dir.join(&file)).unwrap();
        assert!(content.contains("result: failed"));
        // Видно, що це дія політики, а не судження аудитора.
        assert!(content.contains("auto_by_policy: true"));
        assert!(content.contains("actor: wrapper"));
    }

    #[test]
    fn fresh_clarification_is_not_expired() {
        let (_tmp, root) = node_with_open_audit();
        clarification(&root, "solo", "auditor", "Чому?").unwrap();
        assert!(expire_clarification(&root, "solo", 3600).unwrap().is_none());
    }

    #[test]
    fn answered_clarification_is_never_expired() {
        let (tmp, root) = node_with_open_audit();
        let dir = tmp.path().join("solo");
        fs::write(
            dir.join("clarification_001.md"),
            "---\ncreated_at: 2020-01-01T00:00:00Z\n---\n\n## Question\n\nЧому?\n",
        )
        .unwrap();
        fs::write(dir.join("amended_001.md"), "---\n---\n\n## Answer\n\nбо так\n").unwrap();
        assert!(expire_clarification(&root, "solo", 1).unwrap().is_none());
    }

    #[test]
    fn audit_streak_counts_consecutive_failures_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        let write = |nnn: u64, result: &str| {
            fs::write(
                dir.join(format!("audit-result_{nnn:03}.md")),
                format!("---\nresult: {result}\n---\n"),
            )
            .unwrap();
        };
        write(1, "failed");
        write(2, "success");
        write(3, "failed");
        write(4, "failed");
        // Рахунок від верху вниз уривається на success: 003 і 001 не в серії.
        assert_eq!(audit_failed_streak(&dir), 2);
        assert!(audit_escalation_due(&dir, &serde_json::json!({})));
        assert!(!audit_escalation_due(
            &dir,
            &serde_json::json!({"audit_retry_max": 5})
        ));
    }
}
