//! Артефакт `decision-request` і стан `awaiting-decision` — вихід
//! «вичерпана драбина → розвилка» (спека `mandates.md`, «Артефакт
//! `decision-request`»; `graph.md` — derived-стани).
//!
//! **Інваріант, заради якого це існує** (mandates.md): до людини не доходить
//! «run failed» — це вже покриває retry ladder. Розвилка доповзає лише коли
//! драбину вичерпано **і** причина — не баг, а вибір поза автономією агента.
//! Тому `unresolvable` перестає бути єдиним фіналом вичерпаної драбини:
//! термінальним він лишається для багів, а для вибору вузол переходить в
//! `awaiting-decision` і чекає на власника мандата.
//!
//! **Де що лежить.** Нормативне місце самого артефакта — run branch поруч
//! із `session.jsonl` (`refs/mt/runs/{run-id}/decisions/NNNN-*.md`), і воно
//! тут не змінюється. Але derived-стан рахує `scan_tasks` по робочому
//! дереву, куди run branch не розгорнутий, тож у теці вузла лежить
//! **маркер** `awaiting-decision_NNNN.md` із вказівником на артефакт — той
//! самий патерн, що `pending-audit_NNN.md`, стан якого теж матеріалізований
//! маркером, а зміст живе окремо.
//!
//! Хто пакує артефакт — окремий агент escalation-intake (mandates.md:
//! «`decision-request` НІКОЛИ не пишеться виконавцем напряму»); цей модуль
//! дає лише формат, запис, читання і відповідь.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::frontmatter::{parse_front_matter, SCHEMA_VERSION};

/// Тека артефактів розвилки всередині run branch (mandates.md).
pub const DECISIONS_DIR: &str = "decisions";

/// Префікс маркера стану в теці вузла.
const MARKER_PREFIX: &str = "awaiting-decision_";

/// Одна спроба драбини у `retry_history` розвилки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryAttempt {
    /// Хто виконував (актор або CLI щабля драбини).
    pub agent: String,
    /// Номер спроби, з 1.
    pub attempt: u64,
    /// Чим завершилось (`result` з `run_NNN.md`).
    pub outcome: String,
}

/// Розвилка, упакована для власника мандата.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    /// `generation` карти мандатів на момент пакування — fencing: відповідь
    /// за застарілою картою перераховується (mandates.md).
    pub mandate_generation: u64,
    /// Кому адресовано (крок 1 маршрутизатора — `effective_owner`).
    pub computed_owner: String,
    /// Ланцюг ескалації від власника до кореня.
    pub escalation_chain: Vec<String>,
    /// Що вже пробували — доказ, що драбину вичерпано, а не обійдено.
    pub retry_history: Vec<RetryAttempt>,
    /// Фасети важеля (глибина квіз-гейта рахується з них).
    pub leverage_facets: serde_json::Value,
    /// Ціна зволікання — людськими словами.
    pub deadline_cost: String,
    /// Ідентифікатор агента-рекомендувальника.
    pub recommended_by: String,
    /// Тіло: контекст, варіанти, рекомендація (markdown нижче фронтматеру).
    pub body: String,
}

/// Відповідь власника на розвилку.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionAnswer {
    /// Обраний варіант (`chosen_option` з `ApprovalResponse`).
    pub chosen_option: String,
    /// Хто підписав (handle власника мандата).
    pub decided_by: String,
    /// Base64 Ed25519-підпису акта.
    pub signature: String,
    /// Час рішення (ISO8601).
    pub decided_at: String,
}

/// Наступний вільний номер розвилки в теці `decisions/`.
fn next_decision_nnnn(decisions: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(decisions) else {
        return 1;
    };
    let highest = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let digits = name.split('-').next()?;
            digits.parse::<u64>().ok()
        })
        .max();
    highest.map_or(1, |n| n + 1)
}

/// Імʼя файлу артефакта за номером.
pub fn request_file(nnnn: u64) -> String {
    format!("{nnnn:04}-decision-request.md")
}

/// Імʼя файлу відповіді за номером.
pub fn answer_file(nnnn: u64) -> String {
    format!("{nnnn:04}-decision-answer.md")
}

/// Імʼя маркера стану в теці вузла.
pub fn marker_file(nnnn: u64) -> String {
    format!("{MARKER_PREFIX}{nnnn:04}.md")
}

/// Пише розвилку в run worktree і маркер стану в теку вузла.
///
/// Два записи, бо це дві різні речі: артефакт — нормативний вміст у run
/// branch, маркер — те, з чого `scan_tasks` бачить стан, не розгортаючи
/// run branch.
///
/// # Errors
/// Помилки файлової системи.
pub fn write_decision_request(
    node_dir: &Path,
    run_worktree: &Path,
    request: &DecisionRequest,
) -> std::io::Result<u64> {
    let decisions = run_worktree.join(DECISIONS_DIR);
    fs::create_dir_all(&decisions)?;
    let nnnn = next_decision_nnnn(&decisions);

    let facets = serde_json::to_string(&request.leverage_facets).unwrap_or_else(|_| "{}".into());
    let history = request
        .retry_history
        .iter()
        .map(|attempt| {
            format!(
                "  - {{agent: {}, attempt: {}, outcome: {}}}\n",
                attempt.agent, attempt.attempt, attempt.outcome
            )
        })
        .collect::<String>();
    let front = format!(
        "---\nschema_version: {SCHEMA_VERSION}\ntype: decision-request\n\
         mandate_generation: {}\ncomputed_owner: {}\nescalation_chain: [{}]\n\
         retry_history:\n{}leverage_facets: {}\ndeadline_cost: \"{}\"\nrecommended_by: {}\n---\n\n{}",
        request.mandate_generation,
        request.computed_owner,
        request.escalation_chain.join(", "),
        history,
        facets,
        request.deadline_cost.replace('"', "'"),
        request.recommended_by,
        request.body
    );
    fs::write(decisions.join(request_file(nnnn)), front)?;

    // Маркер вузла — вказівник, не копія: дублювати вміст означало б два
    // джерела істини, які розійдуться.
    fs::write(
        node_dir.join(marker_file(nnnn)),
        format!(
            "---\nschema_version: {SCHEMA_VERSION}\ntype: awaiting-decision\n\
             decision_ref: {}/{}\ncomputed_owner: {}\n---\n",
            DECISIONS_DIR,
            request_file(nnnn),
            request.computed_owner
        ),
    )?;
    Ok(nnnn)
}

/// Номер відкритої розвилки вузла — маркер без відповіді. `None`, якщо
/// розвилок немає або всі закриті.
pub fn open_decision(node_dir: &Path) -> Option<u64> {
    let mut open: Option<u64> = None;
    let entries = fs::read_dir(node_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(MARKER_PREFIX) else {
            continue;
        };
        let Some(digits) = rest.strip_suffix(".md") else {
            continue;
        };
        let Ok(nnnn) = digits.parse::<u64>() else {
            continue;
        };
        if node_dir.join(answered_marker(nnnn)).exists() {
            continue;
        }
        open = Some(open.map_or(nnnn, |current| current.max(nnnn)));
    }
    open
}

/// Імʼя маркера відповіді в теці вузла (закриває `awaiting-decision`).
fn answered_marker(nnnn: u64) -> String {
    format!("decided_{nnnn:04}.md")
}

/// Записує відповідь власника: артефакт у run branch + маркер закриття у
/// вузлі. Ідемпотентно — повторна відповідь на закриту розвилку відхиляється.
///
/// # Errors
/// Розвилка не відкрита або помилка файлової системи.
pub fn answer_decision(
    node_dir: &Path,
    run_worktree: &Path,
    nnnn: u64,
    answer: &DecisionAnswer,
) -> Result<(), String> {
    if !node_dir.join(marker_file(nnnn)).exists() {
        return Err(format!("розвилки {nnnn:04} у вузлі немає"));
    }
    if node_dir.join(answered_marker(nnnn)).exists() {
        return Err(format!("розвилка {nnnn:04} уже закрита"));
    }
    let decisions = run_worktree.join(DECISIONS_DIR);
    fs::create_dir_all(&decisions).map_err(|error| error.to_string())?;
    fs::write(
        decisions.join(answer_file(nnnn)),
        format!(
            "---\nschema_version: {SCHEMA_VERSION}\ntype: decision-answer\n\
             decision_ref: {}\nchosen_option: {}\ndecided_by: {}\n\
             decided_at: {}\nsignature: {}\n---\n",
            request_file(nnnn),
            answer.chosen_option,
            answer.decided_by,
            answer.decided_at,
            answer.signature
        ),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        node_dir.join(answered_marker(nnnn)),
        format!(
            "---\nschema_version: {SCHEMA_VERSION}\ntype: decided\n\
             decision_ref: {}/{}\nchosen_option: {}\ndecided_by: {}\n---\n",
            DECISIONS_DIR,
            answer_file(nnnn),
            answer.chosen_option,
            answer.decided_by
        ),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Обраний варіант закритої розвилки — з маркера вузла (щоб читач не
/// розгортав run branch).
pub fn chosen_option(node_dir: &Path, nnnn: u64) -> Option<String> {
    let text = fs::read_to_string(node_dir.join(answered_marker(nnnn))).ok()?;
    parse_front_matter(&text)
        .get("chosen_option")
        .and_then(|value| value.as_str().map(str::to_string))
}

/// Історія спроб драбини з `run_NNN.md` вузла — вміст `retry_history`
/// розвилки. Доказ «драбину вичерпано» збирається з фактів, а не зі слів
/// агента.
pub fn retry_history(node_dir: &Path) -> Vec<RetryAttempt> {
    let Ok(entries) = fs::read_dir(node_dir) else {
        return Vec::new();
    };
    let mut runs: Vec<(u64, String, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy().to_string();
            let digits = name.strip_prefix("run_")?.strip_suffix(".md")?;
            let nnn = digits.parse::<u64>().ok()?;
            let text = fs::read_to_string(entry.path()).ok()?;
            let front = parse_front_matter(&text);
            let field = |key: &str| {
                front
                    .get(key)
                    .and_then(|value| value.as_str().map(str::to_string))
            };
            Some((
                nnn,
                field("actor").unwrap_or_else(|| "executor".into()),
                field("result").unwrap_or_else(|| "unknown".into()),
            ))
        })
        .collect();
    runs.sort_by_key(|(nnn, _, _)| *nnn);
    runs.into_iter()
        .enumerate()
        .map(|(index, (_, agent, outcome))| RetryAttempt {
            agent,
            attempt: index as u64 + 1,
            outcome,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn request() -> DecisionRequest {
        DecisionRequest {
            mandate_generation: 17,
            computed_owner: "olena".into(),
            escalation_chain: vec!["olena".into(), "vitalii".into()],
            retry_history: vec![RetryAttempt {
                agent: "executor-sonnet".into(),
                attempt: 1,
                outcome: "failed-tests".into(),
            }],
            leverage_facets: serde_json::json!({"irreversible": true}),
            deadline_cost: "затримка блокує 3 залежні задачі".into(),
            recommended_by: "escalation-intake-fable-5".into(),
            body: "## Контекст\nтест\n".into(),
        }
    }

    #[test]
    fn write_creates_artifact_and_marker() {
        let dir = node();
        let nnnn = write_decision_request(dir.path(), dir.path(), &request()).unwrap();
        assert_eq!(nnnn, 1);

        let artifact =
            std::fs::read_to_string(dir.path().join(DECISIONS_DIR).join(request_file(1))).unwrap();
        assert!(artifact.contains("type: decision-request"), "{artifact}");
        assert!(artifact.contains("computed_owner: olena"), "{artifact}");
        assert!(artifact.contains("mandate_generation: 17"), "{artifact}");
        assert!(artifact.contains("attempt: 1"), "{artifact}");
        assert!(artifact.contains("## Контекст"), "{artifact}");

        // Маркер — вказівник, не копія: тіло розвилки в ньому не дублюється.
        let marker = std::fs::read_to_string(dir.path().join(marker_file(1))).unwrap();
        assert!(marker.contains("type: awaiting-decision"), "{marker}");
        assert!(marker.contains(&request_file(1)), "{marker}");
        assert!(!marker.contains("## Контекст"), "{marker}");
    }

    #[test]
    fn open_decision_tracks_answer() {
        let dir = node();
        write_decision_request(dir.path(), dir.path(), &request()).unwrap();
        assert_eq!(open_decision(dir.path()), Some(1));

        let answer = DecisionAnswer {
            chosen_option: "B".into(),
            decided_by: "olena".into(),
            signature: "AQID".into(),
            decided_at: "2026-08-12T10:00:00Z".into(),
        };
        answer_decision(dir.path(), dir.path(), 1, &answer).unwrap();
        assert_eq!(
            open_decision(dir.path()),
            None,
            "відповідь закриває розвилку"
        );
        assert_eq!(chosen_option(dir.path(), 1).as_deref(), Some("B"));
    }

    #[test]
    fn second_answer_is_rejected() {
        // Розвилка закривається один раз: інакше аудит-трейл «хто вирішив»
        // переписувався б заднім числом.
        let dir = node();
        write_decision_request(dir.path(), dir.path(), &request()).unwrap();
        let answer = DecisionAnswer {
            chosen_option: "A".into(),
            decided_by: "olena".into(),
            signature: String::new(),
            decided_at: "2026-08-12T10:00:00Z".into(),
        };
        answer_decision(dir.path(), dir.path(), 1, &answer).unwrap();
        let again = answer_decision(dir.path(), dir.path(), 1, &answer).unwrap_err();
        assert!(again.contains("уже закрита"), "{again}");
    }

    #[test]
    fn answer_without_request_is_rejected() {
        let dir = node();
        let answer = DecisionAnswer {
            chosen_option: "A".into(),
            decided_by: "olena".into(),
            signature: String::new(),
            decided_at: "2026-08-12T10:00:00Z".into(),
        };
        let error = answer_decision(dir.path(), dir.path(), 1, &answer).unwrap_err();
        assert!(error.contains("немає"), "{error}");
    }

    #[test]
    fn numbering_continues_across_requests() {
        let dir = node();
        write_decision_request(dir.path(), dir.path(), &request()).unwrap();
        let second = write_decision_request(dir.path(), dir.path(), &request()).unwrap();
        assert_eq!(second, 2);
        // Обидві відкриті — стан рахується за найновішою.
        assert_eq!(open_decision(dir.path()), Some(2));
    }

    #[test]
    fn retry_history_comes_from_run_files() {
        // Доказ «драбину вичерпано» збирається з run-файлів, а не зі слів
        // того, хто ескалює.
        let dir = node();
        std::fs::write(
            dir.path().join("run_001.md"),
            "---\nschema_version: 1\nactor: executor\nresult: failed\n---\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("run_002.md"),
            "---\nschema_version: 1\nactor: engineer\nresult: merge-conflict\n---\n",
        )
        .unwrap();

        let history = retry_history(dir.path());
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].agent, "executor");
        assert_eq!(history[0].attempt, 1);
        assert_eq!(history[1].agent, "engineer");
        assert_eq!(history[1].outcome, "merge-conflict");
    }
}
