//! Ядро MT-графа (канон, який `@7n/mt` споживає через napi): скан дерева
//! `mt/` у [`TaskNode`]-граф із виведенням станів ([`TaskState`]), прапори
//! виконавця `a.md`/`h.md`, створення вузлів і нормалізація імен; доменні
//! підсистеми (claims, runner, publish, orchestrate, …) — в окремих модулях.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Version-chain артефакти вузла: read-модель переліку `task/plan/run/fact/…` для GUI-timeline і CLI.
pub mod artifacts;
/// Аудит-цикл: закриття гейта, який відкриває `mt audit` (вердикт, уточнення, amend).
pub mod audit;
/// Remote execution claims: read-модель ownership вузлів через git refs `refs/mt/claims/<node-hash>`.
pub mod claims;
/// Конфігурація `.mt.json`: дефолти + merge рівнів у ефективний конфіг вузла.
pub mod config;

/// Розвилки: `decision-request`, стан `awaiting-decision`, відповідь власника.
pub mod decision;
/// Мапінг handle → PII з git-ignored `.mt/directory.json` (у git-файлах лишаються лише handles).
pub mod directory;
/// Парсинг і байт-точна серіалізація YAML-frontmatter task-файлів.
pub mod frontmatter;

/// Єдина межа взаємодії mt-core з Git.
pub mod git;
/// Файловий шар i18n: схема перекладу, staleness, contract-aware сегментація.
pub mod i18n;
/// Cost/time ledger: агрегація `wall_sec`/`tokens_*`/`cost_usd` з усіх `run_NNN.md` графу.
pub mod ledger;

/// MCP-сервери surface: декларація, резолв секретів, payload для ACP.
pub mod mcp;
/// Lifecycle-мутації вузла: `mt invalidate` і `mt kill` (архівація version chain у `history/`).
pub mod lifecycle;
/// NNN-нумерація артефактів (`run_NNN.md`, `fact_NNN.md`, …) — чисті функції над іменами файлів.
pub mod nnn;
/// Оркестратор `run --auto`: одноразовий прохід по `waiting` агентських вузлах чергами по `agent_concurrency`.
pub mod orchestrate;
/// Fenced publish protocol: atomic multi-ref push результату worktree у `main` з rebase і retry.
pub mod publish;

/// Мета-цикл ретроспективи: датасет run-історії й детерміновані пропозиції.
pub mod retro;
/// Run-wrapper: CAS claim → detached worktree → spawn виконавця → watchdog → підсумок стану.
pub mod runner;

/// Sandbox-профілі скілів: allowlist команд, мережа, fs-scope.
pub mod sandbox;

/// Secrets-брокер: сховища, інжекція в ENV, маскування у виводах.
pub mod secrets;
/// Обгортка завершення виконавця: запис фактів, результатів і аудит-циклу вузла.
pub mod signal;
/// Протокол spawn: валідація `## Children` плану й матеріалізація дітей після plan-review.
pub mod spawn;

/// Surface-профілі: резолюція режиму ходу, стеля скілів, context kinds.
pub mod surfaces;
/// Тестові фікстури: версійні git-фіксації (bare `origin` + клон) для ізольованих тестів.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod test_support;
/// Іменування, матчінг і provisioning worktree для задач.
pub mod worktree;

// ── Types ─────────────────────────────────────────────────────────────────────

/// Стан вузла графа, виведений сканом файлового контракту (graph.md) —
/// від `Unassigned` до термінальних `Resolved`/`Failed`/`Unresolvable`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Unassigned,
    Pending,      // h.md exists
    Waiting,      // a.md exists, deps resolved
    Blocked,      // a.md exists, deps not resolved
    PlanReview,   // composite plan awaiting human approval
    Spawned,      // children materialized, not all resolved
    Running,      // running_<pid>_until_<ts> sentinel present
    Stalled,      // remote claim lease expired (needs claim refs — not derived by local scan)
    PendingAudit, // open audit cycle
    /// Відкритий `decision-request` без відповіді власника мандата
    /// (mandates.md). Блокує залежні так само, як `pending-audit`.
    AwaitingDecision,
    Resolved,     // accepted fact exists
    Failed,       // failed_streak >= agent_retry_max
    Unresolvable, // unresolvable.md exists (terminal)
}

/// Вузол MT-графа: ідентичність, стан, залежності, бюджети й діти —
/// одиниця виводу `scan_tasks` для CLI/GUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub path: String,
    pub state: TaskState,
    pub deps: Vec<String>,
    pub mode: String,
    pub budget_sec: Option<u64>,
    pub budget_hard_sec: Option<u64>,
    pub deadline: Option<String>,
    pub hint: Option<String>,
    pub created_at: Option<String>,
    pub children: Vec<TaskNode>,
    pub is_composite: bool,
    /// Порушення контракту, що не міняють стан вузла, але видимі в `mt status`
    /// (graph.md: порушення схеми, далі — `orphan-node`,
    /// `blocked-invalid-dep`). Порожній вектор не серіалізується — наявні
    /// JSON-споживачі не бачать нового поля.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Знайдений воркспейс із `mt/`-деревом: людська мітка + шлях до tasks-директорії.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub label: String,
    pub path: String,
}

/// `schema_version` прапорів виконавця (graph.md §a.md/§h.md).
pub const FLAG_SCHEMA_VERSION: u64 = 1;

/// Виконавець вузла. Істина — прапор-файл `a.md`/`h.md`, не поле frontmatter.
/// Пише прапор виконавця (`a.md` або `h.md`) і видаляє протилежний —
/// інваріант «рівно один прапор» (§4.3). Повертає ім'я записаного файлу.
pub fn write_executor_flag(
    task_dir: &Path,
    mode: Mode,
    model_tier: &str,
    skills: &[String],
    qualification: Option<&str>,
) -> Result<&'static str, String> {
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    match mode {
        Mode::Agent => {
            let fm = serde_json::json!({
                "schema_version": FLAG_SCHEMA_VERSION,
                "created_at": created_at,
                "model_tier": model_tier,
                "skills": skills,
                "interactive": false,
            });
            write_atomic(
                &task_dir.join("a.md"),
                &frontmatter::build_markdown(&fm, ""),
            )?;
            let _ = fs::remove_file(task_dir.join("h.md"));
            Ok("a.md")
        }
        Mode::Human => {
            let fm = serde_json::json!({
                "schema_version": FLAG_SCHEMA_VERSION,
                "created_at": created_at,
                "notify": true,
                "qualification": qualification.unwrap_or(""),
            });
            let body = if qualification.is_some() {
                ""
            } else {
                "<!-- Опишіть необхідну кваліфікацію виконавця у полі qualification -->\n"
            };
            write_atomic(
                &task_dir.join("h.md"),
                &frontmatter::build_markdown(&fm, body),
            )?;
            let _ = fs::remove_file(task_dir.join("a.md"));
            Ok("h.md")
        }
    }
}

/// Виконавець вузла: `Agent` (прапор `a.md`) чи `Human` (прапор `h.md`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Agent,
    Human,
}

/// Опції створення вузла. `None`-поля резолвляться з `.mt.json` (default_*).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateOpts {
    #[serde(default)]
    pub mode: Option<Mode>,
    #[serde(default)]
    pub model_tier: Option<String>,
    #[serde(default)]
    pub budget_sec: Option<u64>,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// Текст місії дитини (спека: `## Children` → `task:`); без нього — шаблон.
    #[serde(default)]
    pub task: Option<String>,
    /// Для mode: human — кваліфікація виконавця (`## Children` → `qualification:`).
    #[serde(default)]
    pub qualification: Option<String>,
}

/// Результат `create_task`. CLI серіалізує через [`CreateOutcome::to_cli_json`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CreateOutcome {
    Created {
        name: String,
        task_path: String,
        flag: String,
        deps: Vec<String>,
    },
    Exists {
        name: String,
        task_path: String,
    },
}

impl CreateOutcome {
    /// Плаский JSON-контракт CLI (§3.2 spec): поле `created: bool`.
    pub fn to_cli_json(&self) -> serde_json::Value {
        match self {
            CreateOutcome::Created {
                name,
                task_path,
                flag,
                deps,
            } => serde_json::json!({
                "created": true,
                "name": name,
                "task_path": task_path,
                "flag": flag,
                "deps": deps,
            }),
            CreateOutcome::Exists { name, task_path } => serde_json::json!({
                "created": false,
                "reason": "exists",
                "name": name,
                "task_path": task_path,
            }),
        }
    }
}

// ── Frontmatter ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct Frontmatter {
    created_at: Option<String>,
    budget_sec: Option<u64>,
    budget_hard_sec: Option<u64>,
    deadline: Option<String>,
    hint: Option<String>,
}

fn parse_frontmatter(content: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return fm;
    }
    let end = lines[1..]
        .iter()
        .position(|l| l.trim() == "---")
        .map(|i| i + 1)
        .unwrap_or(lines.len());
    for line in &lines[1..end] {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(pos) = t.find(':') {
            let key = t[..pos].trim();
            let val = t[pos + 1..].trim();
            match key {
                "created_at" => fm.created_at = Some(val.to_string()),
                "budget_sec" => fm.budget_sec = val.parse().ok(),
                "budget_hard_sec" => fm.budget_hard_sec = val.parse().ok(),
                "deadline" => fm.deadline = Some(val.to_string()),
                "hint" => fm.hint = Some(val.to_string()),
                _ => {}
            }
        }
    }
    fm
}

// ── NNN helpers ───────────────────────────────────────────────────────────────

pub(crate) fn max_nnn(dir: &Path, prefix: &str, suffix: &str) -> u64 {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            if s.starts_with(prefix) && s.ends_with(suffix) {
                s[prefix.len()..s.len() - suffix.len()].parse::<u64>().ok()
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0)
}

// Категорії `result` у run_NNN.md (graph.md, таблиця «Категорія | Values»):
// execution-failure інкрементує failed_streak; lifecycle-результати
// (decomposed | claim-lost | handoff) лічильник не змінюють.
fn is_execution_failure(result: &str) -> bool {
    matches!(
        result,
        "failed" | "progress-timeout" | "budget-exceeded" | "merge-conflict"
    )
}

// Run без розпізнаного result рахується як failure: недоведений провал
// коштує зайвого ретраю, недорахований — нескінченної драбини.
fn run_is_failure(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return true;
    };
    match frontmatter::parse_front_matter(&content).get("result") {
        Some(v) => v.as_str().is_none_or(|s| is_execution_failure(s.trim())),
        None => true,
    }
}

// «Прийнятий fact» (graph.md): актуальний fact_N (max NNN) без pending-audit_N
// або з audit-result_N: success. Відхилений аудитом fact прийнятим НЕ є, тому
// межу не рухає — інакше цикл «провал → провал → сирий fact → аудит відхилив»
// обнуляв би лічильник вічно, і драбина ніколи не дійшла б до engineer/unresolvable.
pub(crate) fn accepted_fact_nnn(dir: &Path) -> u64 {
    match accepted_fact_state(dir) {
        FactState::Resolved => max_nnn(dir, "fact_", ".md"),
        FactState::PendingAudit | FactState::None => 0,
    }
}

// count(run із result ∈ execution-failure і NNN > останнього прийнятого fact) — graph.md.
fn failed_streak(dir: &Path) -> u64 {
    let since = accepted_fact_nnn(dir);
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            let Some(nnn) = s
                .strip_prefix("run_")
                .and_then(|rest| rest.strip_suffix(".md"))
                .and_then(|n| n.parse::<u64>().ok())
            else {
                return false;
            };
            nnn > since && run_is_failure(&e.path())
        })
        .count() as u64
}

/// Сума `wall_sec` усіх run-ів вузла — вхід третього тригера `unresolvable`
/// (chain-ліміт часу, graph.md).
fn total_wall_sec(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with("run_") && s.ends_with(".md")
        })
        .filter_map(|e| {
            let content = fs::read_to_string(e.path()).ok()?;
            frontmatter::parse_front_matter(&content)
                .get("wall_sec")
                .and_then(serde_json::Value::as_u64)
        })
        .sum()
}

fn count_files(dir: &Path, prefix: &str, suffix: &str) -> u64 {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with(prefix) && s.ends_with(suffix)
        })
        .count() as u64
}

/// Чому вузол став термінально нерозв'язним (graph.md, три тригери
/// `unresolvable`) — або `None`, якщо жоден не спрацював.
///
/// Тригери навмисно різнорідні: вичерпана драбина виконання, вичерпана
/// драбина *планування* і вичерпаний сумарний бюджет часу. Кожен окремо
/// означає «автомат більше не має ходів», і вихід із стану лише людський —
/// `mt invalidate` з правкою `task.md`, `mt kill` або engineer на предку.
///
/// `budget_total_sec` опційний (спека): не заданий — тригер неактивний.
pub fn unresolvable_reason(dir: &Path, config: &serde_json::Value) -> Option<String> {
    let num = |key: &str, default: u64| {
        config
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default)
    };

    let exhausted = num("agent_retry_max", 3) + num("engineer_retry_max", 1);
    let streak = failed_streak(dir);
    if streak >= exhausted {
        return Some(format!(
            "вичерпано драбину виконання: {streak} провалів поспіль ≥ agent_retry_max + engineer_retry_max ({exhausted})"
        ));
    }

    let rejects = count_files(dir, "plan-rejected_", ".md");
    let reject_max = num("plan_reject_max", 3);
    if rejects >= reject_max {
        return Some(format!(
            "вичерпано драбину планування: {rejects} відхилених планів ≥ plan_reject_max ({reject_max})"
        ));
    }

    if let Some(limit) = config
        .get("budget_total_sec")
        .and_then(serde_json::Value::as_u64)
    {
        let spent = total_wall_sec(dir);
        if spent > limit {
            return Some(format!(
                "вичерпано сумарний бюджет: {spent}s > budget_total_sec ({limit}s)"
            ));
        }
    }

    None
}

/// Пише термінальний маркер `unresolvable.md`. Ідемпотентно: маркер
/// immutable, тож наявний не перезаписується.
pub fn write_unresolvable(dir: &Path, reason: &str) -> Result<bool, String> {
    let path = dir.join("unresolvable.md");
    if path.exists() {
        return Ok(false);
    }
    let content = format!(
        "---\nschema_version: 1\ncreated_at: {}\n---\n\n## Reason\n\n{reason}\n\n\
         ## Next\n\nВихід лише людський: `mt invalidate <вузол>` із правкою `task.md`, \
         `mt kill <вузол>`, або `mt run <предок> --actor engineer`.\n",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    write_atomic(&path, &content)?;
    Ok(true)
}

// ── State detection ───────────────────────────────────────────────────────────

// Reads result: field from audit-result frontmatter (only exception to name-based state rule).
fn audit_result_success(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return false;
    }
    let end = lines[1..]
        .iter()
        .position(|l| l.trim() == "---")
        .map(|i| i + 1)
        .unwrap_or(lines.len());
    for line in &lines[1..end] {
        if let Some(val) = line.trim().strip_prefix("result:") {
            return val.trim() == "success";
        }
    }
    false
}

#[derive(PartialEq)]
enum FactState {
    None,
    PendingAudit,
    Resolved,
}

// Accepted-fact state from the LATEST fact NNN only (mirrors JS getAcceptedFactState).
// A non-latest open audit cycle does not block resolution.
fn accepted_fact_state(dir: &Path) -> FactState {
    let nnn = max_nnn(dir, "fact_", ".md");
    if nnn == 0 {
        return FactState::None;
    }
    let nnn_s = format!("{nnn:03}");
    if !dir.join(format!("pending-audit_{nnn_s}.md")).exists() {
        return FactState::Resolved;
    }
    let result_path = dir.join(format!("audit-result_{nnn_s}.md"));
    if !result_path.exists() {
        return FactState::PendingAudit;
    }
    // Audit completed: success → resolved; failed → fall through (None).
    if audit_result_success(&result_path) {
        FactState::Resolved
    } else {
        FactState::None
    }
}

// Local runtime marker running_<pid>_until_<ts> (mirrors JS RUNNING_MARKER_RE /^running_\d+_until_/).
fn has_running_marker(dir: &Path) -> bool {
    fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && is_running_marker(&e.file_name().to_string_lossy())
        })
    })
}

fn is_running_marker(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("running_") else {
        return false;
    };
    let Some(idx) = rest.find("_until_") else {
        return false;
    };
    idx > 0 && rest[..idx].bytes().all(|b| b.is_ascii_digit())
}

/// Санітизує ім'я задачі для порівняння worktree: `[^A-Za-z0-9_-]` → `'-'`.
/// ⚠️ Логіку синхронізовано з JS `sanitizeTaskName` у `npm/lib/core/state.mjs` (спільні тест-вектори).
pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Нормалізує ім'я гілки до безпечного імені директорії у .worktrees/.
/// Правило: не-alphanum/не-_/не-- → '-', consecutive '-' → '-', trim leading/trailing '-'.
/// ⚠️ Логіку синхронізовано з JS `sanitizeBranch` у `@7n/mt` (npm/lib/commands/worktree.mjs).
pub fn sanitize_branch(branch: &str) -> String {
    let s: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = re_collapse_dashes(&s);
    s.trim_matches('-').to_string()
}

fn re_collapse_dashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_dash = false;
    for c in s.chars() {
        if c == '-' {
            if !last_dash {
                result.push('-');
            }
            last_dash = true;
        } else {
            result.push(c);
            last_dash = false;
        }
    }
    result
}

// running if an active worktree name starts with the sanitized node path.
fn worktree_matches(path: &str, worktrees: &[String]) -> bool {
    let prefix = sanitize(path);
    !prefix.is_empty() && worktrees.iter().any(|wt| wt.starts_with(&prefix))
}

fn plan_decision(dir: &Path, nnn: u64) -> Option<String> {
    let content = fs::read_to_string(dir.join(format!("plan_{nnn:03}.md"))).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.first()?.trim() != "---" {
        return None;
    }
    let end = lines[1..]
        .iter()
        .position(|l| l.trim() == "---")
        .map(|i| i + 1)
        .unwrap_or(lines.len());
    for line in &lines[1..end] {
        if let Some(val) = line.trim().strip_prefix("decision:") {
            return Some(val.trim().to_string());
        }
    }
    None
}

// plan-review / spawned for composite plans (mirrors JS getCompositePlanState).
fn composite_plan_state(dir: &Path, children: &[TaskNode]) -> Option<TaskState> {
    let nnn = max_nnn(dir, "plan_", ".md");
    if nnn == 0 {
        return None;
    }
    if plan_decision(dir, nnn).as_deref() != Some("composite") {
        return None;
    }
    let nnn_s = format!("{nnn:03}");
    let approved = dir.join(format!("plan-approved_{nnn_s}.md")).exists();
    let rejected = dir.join(format!("plan-rejected_{nnn_s}.md")).exists();
    if !approved && !rejected {
        return Some(TaskState::PlanReview);
    }
    if approved && !children.is_empty() {
        return Some(TaskState::Spawned);
    }
    None
}

// Priority per spec / JS deriveNodeState:
// pending-audit > resolved > awaiting-decision > unresolvable > running > plan-review > spawned >
// waiting/failed > pending > unassigned. (stalled needs remote — skipped in local scan.)
fn detect_state(
    dir: &Path,
    path: &str,
    children: &[TaskNode],
    agent_retry_max: u64,
    worktrees: &[String],
) -> TaskState {
    // 1 + 2. pending-audit / resolved — accepted fact on the latest NNN.
    match accepted_fact_state(dir) {
        FactState::PendingAudit => return TaskState::PendingAudit,
        FactState::Resolved => return TaskState::Resolved,
        FactState::None => {}
    }
    // 3. awaiting-decision — відкрита розвилка. Стоїть ПЕРЕД
    // `unresolvable`, бо це не той самий фінал: `unresolvable` термінальний
    // (баг, який система не подужала), а розвилка чекає на людину і
    // розблокується її відповіддю.
    if crate::decision::open_decision(dir).is_some() {
        return TaskState::AwaitingDecision;
    }
    // 4. unresolvable — terminal marker file.
    if dir.join("unresolvable.md").exists() {
        return TaskState::Unresolvable;
    }
    // 4. running — local marker or an active worktree matching this node.
    if has_running_marker(dir) || worktree_matches(path, worktrees) {
        return TaskState::Running;
    }
    // 5 + 6. plan-review / spawned — composite plan without approve, or approved with children.
    if let Some(st) = composite_plan_state(dir, children) {
        return st;
    }
    // 7. waiting / failed — a.md = agent executor; failed once streak exhausted.
    if dir.join("a.md").exists() {
        if failed_streak(dir) >= agent_retry_max {
            return TaskState::Failed;
        }
        return TaskState::Waiting; // may be upgraded to Blocked in post-processing
    }
    // 8. pending — h.md = human executor.
    if dir.join("h.md").exists() {
        return TaskState::Pending;
    }
    // 9. unassigned — no executor.
    TaskState::Unassigned
}

// ── Blocked post-processing ───────────────────────────────────────────────────

fn build_state_map(nodes: &[TaskNode], map: &mut HashMap<String, TaskState>) {
    for node in nodes {
        map.insert(node.path.clone(), node.state.clone());
        build_state_map(&node.children, map);
    }
}

fn apply_blocked(nodes: &mut [TaskNode], state_map: &HashMap<String, TaskState>) {
    for node in nodes.iter_mut() {
        if node.state == TaskState::Waiting && !node.deps.is_empty() {
            let blocked = node.deps.iter().any(|dep_id| {
                state_map
                    .get(dep_id)
                    .is_none_or(|s| *s != TaskState::Resolved)
            });
            if blocked {
                node.state = TaskState::Blocked;
            }
        }
        if !node.children.is_empty() {
            apply_blocked(&mut node.children, state_map);
        }
    }
}

// ── Deps ──────────────────────────────────────────────────────────────────────

fn collect_deps(deps_root: &Path, current: &Path, result: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_deps(deps_root, &path, result);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(deps_root) {
                let dep_str = rel.to_string_lossy().replace('\\', "/");
                let dep_id = dep_str.strip_suffix(".md").unwrap_or(&dep_str).to_string();
                result.push(dep_id);
            }
        }
    }
}

pub(crate) fn read_deps_dir(node_dir: &Path) -> Vec<String> {
    let deps_dir = node_dir.join("deps");
    if !deps_dir.is_dir() {
        return vec![];
    }
    let mut result = vec![];
    collect_deps(&deps_dir, &deps_dir, &mut result);
    result
}

// ── Node scanner ──────────────────────────────────────────────────────────────

// Spec «Монорепо»: scan пропускає `.gitignore`d, приховані (`.`) та
// `node_modules`/`target`/`dist`/`build` директорії.
fn scan_skip_dir(name: &str, ignore_patterns: &[String]) -> bool {
    name.starts_with('.')
        || matches!(name, "node_modules" | "target" | "dist" | "build")
        || dir_is_gitignored(name, ignore_patterns)
}

fn scan_dir(
    dir: &Path,
    tasks_root: &Path,
    agent_retry_max: u64,
    worktrees: &[String],
    inherited_ignores: &[String],
) -> Option<TaskNode> {
    if !dir.join("task.md").exists() {
        return None;
    }

    let content = fs::read_to_string(dir.join("task.md")).unwrap_or_default();
    let fm = parse_frontmatter(&content);

    let mode = if dir.join("a.md").exists() {
        "agent"
    } else if dir.join("h.md").exists() {
        "human"
    } else {
        "unassigned"
    };

    let deps = read_deps_dir(dir);

    // Scan children; skip history/ and other non-node dirs (no task.md = None),
    // plus hidden/denylisted/.gitignore'd dirs per spec «Монорепо».
    let ignores = load_gitignore(dir, inherited_ignores);
    let mut children: Vec<TaskNode> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        let mut subdirs: Vec<_> = entries
            .flatten()
            .filter(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && !scan_skip_dir(&e.file_name().to_string_lossy(), &ignores)
            })
            .collect();
        subdirs.sort_by_key(|e| e.file_name());
        for sub in subdirs {
            if let Some(child) = scan_dir(
                &sub.path(),
                tasks_root,
                agent_retry_max,
                worktrees,
                &ignores,
            ) {
                children.push(child);
            }
        }
    }

    // Легітимність дітей (graph.md, «Протокол spawn»): дитина законна лише
    // якщо її id є в `## Children` approved-плану цього вузла. Інакше це
    // директорія з task.md, яку ніхто не схвалював — orphan-node.
    let approved = spawn::approved_child_ids(dir);
    for child in &mut children {
        if !approved.as_ref().is_some_and(|ids| ids.contains(&child.id)) {
            child.warnings.push(
                "orphan-node: id відсутній у `## Children` approved-плану батька (graph.md)"
                    .to_string(),
            );
        }
    }

    let is_composite = !children.is_empty();
    let id = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let path = dir
        .strip_prefix(tasks_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| id.clone());
    let state = detect_state(dir, &path, &children, agent_retry_max, worktrees);

    Some(TaskNode {
        id,
        path,
        state,
        deps,
        mode: mode.to_string(),
        budget_sec: fm.budget_sec,
        budget_hard_sec: fm.budget_hard_sec,
        deadline: fm.deadline,
        hint: fm.hint,
        created_at: fm.created_at,
        children,
        is_composite,
        warnings: schema_warnings(dir),
    })
}

/// Порушення контракту `schema_version` у файлах вузла (graph.md). Скан не
/// падає на них: він має показувати граф цілком, зокрема зламані вузли — а
/// відмова виконувати їх стоїть на гейтах `preflight`/`signal`/`spawn`.
fn schema_warnings(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for name in ["task.md", "a.md", "h.md"] {
        let path = dir.join(name);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if !content.trim_start().starts_with("---") {
            out.push(format!("{name}: немає YAML-frontmatter"));
            continue;
        }
        let fm = frontmatter::parse_front_matter(&content);
        match frontmatter::schema_version_of(&fm) {
            Err(e) => out.push(format!("{name}: {e}")),
            Ok(None) => out.push(format!(
                "{name}: немає schema_version — обовʼязкове перше поле (graph.md)"
            )),
            Ok(Some(_)) => {}
        }
    }
    out
}

// ── Workspace discovery ───────────────────────────────────────────────────────

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn workspace_label(git_root: &Path, workspace_dir: &Path) -> String {
    if workspace_dir == git_root {
        git_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("root")
            .to_string()
    } else {
        workspace_dir
            .strip_prefix(git_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| workspace_dir.to_string_lossy().into_owned())
    }
}

fn load_gitignore(dir: &Path, inherited: &[String]) -> Vec<String> {
    let mut patterns = inherited.to_vec();
    if let Ok(content) = fs::read_to_string(dir.join(".gitignore")) {
        for line in content.lines() {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with('#') && !l.starts_with('!') {
                patterns.push(l.trim_end_matches('/').trim_start_matches('/').to_string());
            }
        }
    }
    patterns
}

fn glob_match_name(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            name.starts_with(prefix)
                && name.ends_with(suffix)
                && name.len() >= prefix.len() + suffix.len()
        }
        None => name == pattern,
    }
}

fn dir_is_gitignored(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_match_name(p, name))
}

fn has_task_nodes(dir: &Path) -> bool {
    fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| {
            e.file_type().map(|t| t.is_dir()).unwrap_or(false) && e.path().join("task.md").exists()
        })
    })
}

fn scan_for_workspaces(
    current: &Path,
    git_root: &Path,
    result: &mut Vec<WorkspaceInfo>,
    depth: u8,
    inherited_ignores: &[String],
) {
    if depth > 6 {
        return;
    }

    let ignores = load_gitignore(current, inherited_ignores);

    let mt_config = current.join(".mt.json");
    if mt_config.exists() {
        let mt_dir = fs::read_to_string(&mt_config)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| {
                v.get("mt_dir")
                    .and_then(|v| v.as_str())
                    .map(|s| current.join(s))
            })
            .unwrap_or_else(|| current.join("mt"));
        if mt_dir.is_dir() && has_task_nodes(&mt_dir) {
            result.push(WorkspaceInfo {
                label: workspace_label(git_root, current),
                path: mt_dir.to_string_lossy().into_owned(),
            });
            return;
        }
    }

    for dirname in &["mt", "tasks"] {
        let candidate = current.join(dirname);
        if candidate.is_dir() && has_task_nodes(&candidate) {
            result.push(WorkspaceInfo {
                label: workspace_label(git_root, current),
                path: candidate.to_string_lossy().into_owned(),
            });
            return;
        }
    }

    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut subdirs: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let n = name.to_string_lossy();
            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && !n.starts_with('.')
                && !matches!(n.as_ref(), "node_modules" | "target" | "dist" | "build")
                && !dir_is_gitignored(&n, &ignores)
        })
        .collect();
    subdirs.sort_by_key(|e| e.file_name());
    for sub in subdirs {
        scan_for_workspaces(&sub.path(), git_root, result, depth + 1, &ignores);
    }
}

/// `agent_retry_max` через штатний merge конфігу — дефолт живе в
/// [`config::config_defaults`], а не дублюється тут хардкодом.
fn read_agent_retry_max(project_root: &Path) -> u64 {
    let raw = fs::read_to_string(project_root.join(".mt.json")).ok();
    config::merge_config(raw.as_deref())
        .get("agent_retry_max")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(3)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Сканує `tasks_dir` і повертає дерево вузлів.
///
/// `worktrees` — імена активних git-worktree (останній компонент шляху). Вузол, чий
/// sanitized-шлях є префіксом імені активного worktree, отримує стан `running`.
/// Накладає remote claim-и на derived-стани (graph.md): активний claim →
/// `running`, прострочений із grace → `stalled`.
///
/// Локальний скан бачить лише `running_*`-маркер **своєї** машини, тому без
/// цього накладання вузол, який виконує інший хост, виглядав би вільним —
/// і оркестратор спробував би взяти його в роботу, щоб програти CAS.
///
/// Пріоритет станів зі спеки — `pending-audit`, `resolved` і `unresolvable`
/// стоять вище за `stalled` і `running`, тому claim їх не перекриває.
pub fn apply_remote_claims(
    nodes: &mut [TaskNode],
    tasks_root_rel: &str,
    claims: &HashMap<String, bool>,
) {
    for node in nodes.iter_mut() {
        let terminal = matches!(
            node.state,
            TaskState::PendingAudit | TaskState::Resolved | TaskState::Unresolvable
        );
        if !terminal {
            let hash = claims::node_hash(tasks_root_rel, &node.path);
            if let Some(expired) = claims.get(&hash) {
                node.state = if *expired {
                    TaskState::Stalled
                } else {
                    TaskState::Running
                };
            }
        }
        if !node.children.is_empty() {
            apply_remote_claims(&mut node.children, tasks_root_rel, claims);
        }
    }
}

/// [`scan_tasks`] + накладання remote claim-ів.
///
/// Fail-open: немає git-репозиторію чи remote — повертається локальний скан.
/// Видимість чужої роботи корисна, але не має робити `mt status` непридатним
/// офлайн.
pub fn scan_tasks_with_claims(
    tasks_dir: String,
    worktrees: Vec<String>,
) -> Result<Vec<TaskNode>, String> {
    let mut nodes = scan_tasks(tasks_dir.clone(), worktrees)?;
    let dir = PathBuf::from(&tasks_dir);
    let Ok(repo_root) = claims::discover_repo_root(&dir) else {
        return Ok(nodes);
    };
    let Ok(tasks_root_rel) = claims::tasks_root_relative(&repo_root, &dir) else {
        return Ok(nodes);
    };
    let config = config::merge_config(
        fs::read_to_string(repo_root.join(".mt.json"))
            .ok()
            .as_deref(),
    );
    let grace = config
        .get("claim_grace_sec")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(60);
    let Ok(remote) = claims::fetch_remote_claims(&repo_root, grace) else {
        return Ok(nodes);
    };
    let map: HashMap<String, bool> = remote
        .into_iter()
        .map(|c| (c.node_hash, c.expired))
        .collect();
    apply_remote_claims(&mut nodes, &tasks_root_rel, &map);
    Ok(nodes)
}

/// Сканує `tasks_dir` і будує локальний [`TaskNode`]-граф задач із вкладеними дітьми та виведеними станами.
pub fn scan_tasks(tasks_dir: String, worktrees: Vec<String>) -> Result<Vec<TaskNode>, String> {
    let dir = PathBuf::from(&tasks_dir);
    if !dir.exists() {
        return Err(format!("Directory not found: {tasks_dir}"));
    }
    let project_root = dir.parent().unwrap_or(&dir).to_path_buf();
    let agent_retry_max = read_agent_retry_max(&project_root);

    // .gitignore успадковується з project root у tasks-dir і далі вглиб дерева.
    let root_ignores = load_gitignore(&project_root, &[]);
    let ignores = load_gitignore(&dir, &root_ignores);

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| {
            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && !scan_skip_dir(&e.file_name().to_string_lossy(), &ignores)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut nodes: Vec<TaskNode> = entries
        .iter()
        .filter_map(|e| scan_dir(&e.path(), &dir, agent_retry_max, &worktrees, &ignores))
        .collect();

    // Post-processing: mark Waiting → Blocked where deps are not yet resolved.
    let mut state_map = HashMap::new();
    build_state_map(&nodes, &mut state_map);
    apply_blocked(&mut nodes, &state_map);

    Ok(nodes)
}

/// Виявляє активні git-worktree native `gix` API із `start_dir`.
/// Повертає імена (останній компонент шляху кожного worktree). Помилка → порожньо.
pub fn discover_worktrees(start: &Path) -> Vec<String> {
    git::GitRepository::open(start)
        .and_then(|repository| repository.linked_worktrees())
        .unwrap_or_default()
}

/// Знаходить усі mt/ директорії у репо, починаючи від `start_dir`.
pub fn find_all_tasks_dirs_from(start_dir: &Path) -> Vec<WorkspaceInfo> {
    let git_root = find_git_root(start_dir).unwrap_or_else(|| start_dir.to_path_buf());
    let mut result = vec![];
    scan_for_workspaces(&git_root, &git_root, &mut result, 0, &[]);
    result
}

/// Знаходить усі mt/ директорії у репо від поточного cwd.
pub fn find_all_tasks_dirs() -> Result<Vec<WorkspaceInfo>, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    Ok(find_all_tasks_dirs_from(&cwd))
}

/// Знаходить першу tasks-директорію, ідучи вгору від cwd.
pub fn find_tasks_dir() -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut dir: &Path = &cwd;
    let mut depth = 0u8;
    loop {
        let mt_config = dir.join(".mt.json");
        if mt_config.exists() {
            if let Ok(content) = fs::read_to_string(&mt_config) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(td) = v.get("mt_dir").and_then(|v| v.as_str()) {
                        let full = dir.join(td);
                        if full.is_dir() {
                            return Ok(full.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        let config_path = dir.join(".n-cursor.json");
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(td) = v.get("tasks_dir").and_then(|v| v.as_str()) {
                        let full = dir.join(td);
                        if full.is_dir() {
                            return Ok(full.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        for dirname in &["mt", "tasks"] {
            let candidate = dir.join(dirname);
            if candidate.is_dir() && has_task_nodes(&candidate) {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
        depth += 1;
        if depth >= 8 {
            break;
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
    Err("Could not auto-detect tasks directory.".to_string())
}

// ── Task creation (write-side) ─────────────────────────────────────────────────

/// Валідує id вузла (§8 spec). Дозволені сегменти `[a-z0-9-]+`, роздільник `/`.
/// Відхиляє порожні/`.`/`..` сегменти, провідний/кінцевий `/`, traversal.
/// На відміну від [`sanitize`], НЕ виправляє — повертає `Err`.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name must not be empty".to_string());
    }
    if name.starts_with('/') || name.ends_with('/') {
        return Err(format!("name must not start or end with '/': {name:?}"));
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err(format!("name has an empty segment: {name:?}"));
        }
        if seg == "." || seg == ".." {
            return Err(format!("name segment must not be '.' or '..': {name:?}"));
        }
        if !seg
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            // Текст синхронізовано з JS validateTaskName (npm/lib/core/state.mjs).
            return Err(format!(
                "name segment {seg:?} must match [a-z0-9-]: {name:?}"
            ));
        }
    }
    Ok(())
}

struct CreateDefaults {
    mode: Mode,
    model_tier: String,
    budget_sec: u64,
}

fn read_create_defaults(project_root: &Path) -> CreateDefaults {
    let v = fs::read_to_string(project_root.join(".mt.json"))
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok());
    let mode = v
        .as_ref()
        .and_then(|v| v.get("default_mode").and_then(|x| x.as_str()))
        .map(|s| {
            if s == "agent" {
                Mode::Agent
            } else {
                Mode::Human
            }
        })
        .unwrap_or(Mode::Human);
    let model_tier = v
        .as_ref()
        .and_then(|v| v.get("default_model_tier").and_then(|x| x.as_str()))
        .unwrap_or("AVG")
        .to_string();
    let budget_sec = v
        .as_ref()
        .and_then(|v| v.get("default_budget_sec").and_then(|x| x.as_u64()))
        .unwrap_or(1800);
    CreateDefaults {
        mode,
        model_tier,
        budget_sec,
    }
}

/// Найвищий неіснуючий предок `dir` (для відкату — що саме ми створимо).
fn first_missing_ancestor(dir: &Path) -> Option<PathBuf> {
    if dir.exists() {
        return None;
    }
    let mut candidate = dir.to_path_buf();
    while let Some(parent) = candidate.parent() {
        if parent.exists() {
            return Some(candidate);
        }
        candidate = parent.to_path_buf();
    }
    Some(candidate)
}

/// Атомарний запис: tmp-файл у тій самій директорії → rename (§13/§11.1).
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("path has no parent directory")?;
    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{fname}.tmp"));
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

const TASK_BODY: &str = "\n## Task\n\n<!-- Опишіть завдання тут -->\n\n## Done when\n\n<!-- Критерії успіху -->\n\n## Check\n\n<!-- кожен непорожній рядок — shell-команда (exit 0) -->\n\n## Inputs\n\n<!-- Вхідні дані / контекст для виконавця -->\n";

/// Створює вузол задачі з шаблонного контракту (§4 spec): `<name>/task.md`,
/// прапор виконавця (`a.md`/`h.md`), опційні `deps/<id>.md`.
///
/// Ідемпотентно: якщо `task.md` уже існує — повертає `Exists`, нічого не пише.
/// Атомарно: при частковій відмові прибирає щойно створену гілку директорій.
pub fn create_task(
    tasks_dir: String,
    name: String,
    opts: CreateOpts,
) -> Result<CreateOutcome, String> {
    validate_name(&name)?;

    let tasks_root = PathBuf::from(&tasks_dir);
    let project_root = tasks_root.parent().unwrap_or(&tasks_root).to_path_buf();
    let defaults = read_create_defaults(&project_root);

    let task_dir = tasks_root.join(&name);
    let task_path_rel = format!("{name}/task.md");
    let task_md = task_dir.join("task.md");

    // Ідемпотентність (§2.5): існуючий вузол не чіпаємо.
    if task_md.exists() {
        return Ok(CreateOutcome::Exists {
            name,
            task_path: task_path_rel,
        });
    }

    let mode = opts.mode.unwrap_or(defaults.mode);
    let model_tier = opts.model_tier.unwrap_or(defaults.model_tier);
    let budget_sec = opts.budget_sec.unwrap_or(defaults.budget_sec);
    let hint = opts.hint.unwrap_or_else(|| "atomic".to_string());
    let skills = opts
        .skills
        .unwrap_or_else(|| vec!["bash".to_string(), "write-files".to_string()]);

    // Гілка директорій, яку ми створимо — для відкату при частковій відмові.
    let rollback_root = first_missing_ancestor(&task_dir);

    let build = || -> Result<CreateOutcome, String> {
        fs::create_dir_all(&task_dir).map_err(|e| e.to_string())?;

        // schema_version ПЕРШИМ полем (інваріант docs/mt.md); лише нові файли (§2.8).
        let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let frontmatter = format!(
            "---\nschema_version: 1\ncreated_at: {created_at}\nbudget_sec: {budget_sec}\nhint: {hint}\n---\n"
        );
        let body = match &opts.task {
            Some(text) => format!(
                "\n## Task\n\n{}\n\n## Done when\n\n<!-- Критерії успіху -->\n\n## Check\n\n<!-- кожен непорожній рядок — shell-команда (exit 0) -->\n\n## Inputs\n\n<!-- Вхідні дані / контекст для виконавця -->\n",
                text.trim_end()
            ),
            None => TASK_BODY.to_string(),
        };
        write_atomic(&task_md, &format!("{frontmatter}{body}"))?;

        // Прапор виконавця — рівно один (§4.3).
        let flag = write_executor_flag(
            &task_dir,
            mode,
            &model_tier,
            &skills,
            opts.qualification.as_deref(),
        )?;

        // Залежності — порожні файли-ребра deps/<id>.md (§4.4).
        if !opts.deps.is_empty() {
            let deps_dir = task_dir.join("deps");
            fs::create_dir_all(&deps_dir).map_err(|e| e.to_string())?;
            for dep in &opts.deps {
                let dep_file = deps_dir.join(format!("{dep}.md"));
                if let Some(parent) = dep_file.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                write_atomic(&dep_file, "")?;
            }
        }

        Ok(CreateOutcome::Created {
            name: name.clone(),
            task_path: task_path_rel.clone(),
            flag: flag.to_string(),
            deps: opts.deps.clone(),
        })
    };

    let result = build();
    if result.is_err() {
        if let Some(root) = rollback_root {
            let _ = fs::remove_dir_all(&root);
        }
    }
    result
}

/// Пише новий `plan_NNN.md` (NNN = max+1) — чернетка для виконавця: `decision:
/// atomic` за замовчуванням (перепишеться на `composite` разом із `##
/// Children`, якщо декомпозиція потрібна), опційний `mode:` override.
/// Повертає ім'я записаного файлу.
pub fn write_plan_draft(
    tasks_dir: &str,
    node_path: &str,
    mode: Option<&str>,
) -> Result<String, String> {
    validate_name(node_path)?;
    let dir = PathBuf::from(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    let files: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let nnn = nnn::next_plan_nnn(&files);
    let plan_file = format!("plan_{nnn}.md");
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mode_line = mode.map(|m| format!("mode: {m}\n")).unwrap_or_default();
    let content = format!(
        "---\nschema_version: 1\ncreated_at: {created_at}\ndecision: atomic\n{mode_line}---\n\n## Context\n\n<!-- контекст рішення: чому atomic/composite -->\n\n## Children\n\n<!-- лише для decision: composite — див. спеку «Протокол spawn» -->\n\n## Risks\n\n<!-- ризики виконання -->\n"
    );
    write_atomic(&dir.join(&plan_file), &content)?;
    Ok(plan_file)
}

// ── Tests ───────────────────────────────────────────────────────────────────────
// Reproduce the authoritative cases from npm/lib/tests/state.test.mjs (the JS suite
// these replace), plus worktree→running and sanitize vectors.

#[cfg(test)]
mod tests {

    #[test]
    fn test_sanitize_branch() {
        assert_eq!(sanitize_branch("feat/my-feature"), "feat-my-feature");
        assert_eq!(sanitize_branch("main"), "main");
        assert_eq!(sanitize_branch("feature/fix:bug"), "feature-fix-bug");
        assert_eq!(sanitize_branch("-leading"), "leading");
        assert_eq!(sanitize_branch("trailing-"), "trailing");
        assert_eq!(sanitize_branch("double//slash"), "double-slash");
        assert_eq!(sanitize_branch("a b c"), "a-b-c");
    }

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Builds <tmp>/mt/<node>/ with `files` (name, content), scans, returns that node's state.
    /// `.mt.json` (with optional agent_retry_max) lives in the project root (parent of mt/).
    fn state_of(
        node: &str,
        files: &[(&str, &str)],
        worktrees: &[&str],
        retry: Option<u64>,
    ) -> TaskState {
        let root = tempdir().unwrap();
        if let Some(m) = retry {
            fs::write(
                root.path().join(".mt.json"),
                format!("{{\"agent_retry_max\": {m}}}"),
            )
            .unwrap();
        }
        let tasks_root = root.path().join("mt");
        let node_dir = tasks_root.join(node);
        fs::create_dir_all(&node_dir).unwrap();
        for (name, content) in files {
            fs::write(node_dir.join(name), content).unwrap();
        }
        let wt: Vec<String> = worktrees.iter().map(|s| (*s).to_string()).collect();
        let nodes = scan_tasks(tasks_root.to_string_lossy().into_owned(), wt).unwrap();
        find_node(&nodes, node)
            .expect("node not found")
            .state
            .clone()
    }

    fn find_node<'a>(nodes: &'a [TaskNode], path: &str) -> Option<&'a TaskNode> {
        for n in nodes {
            if n.path == path {
                return Some(n);
            }
            if let Some(found) = find_node(&n.children, path) {
                return Some(found);
            }
        }
        None
    }

    const COMPOSITE: &str = "---\nschema_version: 1\ndecision: composite\n---\n";
    const ATOMIC: &str = "---\nschema_version: 1\ndecision: atomic\n---\n";

    // ── unassigned / pending / waiting ──
    #[test]
    fn unassigned_when_no_executor() {
        assert_eq!(
            state_of("task", &[("task.md", "")], &[], None),
            TaskState::Unassigned
        );
    }
    #[test]
    fn pending_with_h_md() {
        assert_eq!(
            state_of("task", &[("task.md", ""), ("h.md", "")], &[], None),
            TaskState::Pending
        );
        assert_eq!(
            state_of(
                "task",
                &[("task.md", ""), ("h.md", ""), ("plan_001.md", "")],
                &[],
                None
            ),
            TaskState::Pending
        );
    }
    #[test]
    fn waiting_with_a_md() {
        assert_eq!(
            state_of("task", &[("task.md", ""), ("a.md", "")], &[], None),
            TaskState::Waiting
        );
    }
    #[test]
    fn waiting_when_streak_below_max() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", ""),
            ("run_002.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting); // streak 2 < 3
    }

    // ── warnings схеми у скані ──

    /// Як [`state_of`], але повертає попередження вузла.
    fn warnings_of(node: &str, files: &[(&str, &str)]) -> Vec<String> {
        let root = tempdir().unwrap();
        let tasks_root = root.path().join("mt");
        let node_dir = tasks_root.join(node);
        fs::create_dir_all(&node_dir).unwrap();
        for (name, content) in files {
            fs::write(node_dir.join(name), content).unwrap();
        }
        let nodes = scan_tasks(tasks_root.to_string_lossy().into_owned(), vec![]).unwrap();
        find_node(&nodes, node)
            .expect("node not found")
            .warnings
            .clone()
    }

    #[test]
    fn scan_is_silent_on_well_formed_node() {
        let files = [
            ("task.md", "---\nschema_version: 1\n---\n\n## Task\n\nx\n"),
            ("a.md", "---\nschema_version: 1\nmodel_tier: AVG\n---\n"),
        ];
        assert!(warnings_of("task", &files).is_empty());
    }

    #[test]
    fn scan_warns_on_unknown_and_missing_schema() {
        let files = [
            ("task.md", "---\nschema_version: 5\n---\n\n## Task\n\nx\n"),
            ("a.md", "---\nmodel_tier: AVG\n---\n"),
        ];
        let w = warnings_of("task", &files);
        assert_eq!(w.len(), 2, "got: {w:?}");
        assert!(
            w[0].starts_with("task.md:") && w[0].contains('5'),
            "got: {w:?}"
        );
        assert!(
            w[1].starts_with("a.md:") && w[1].contains("schema_version"),
            "got: {w:?}"
        );
    }

    #[test]
    fn scan_warns_but_does_not_change_state() {
        // Скан має показувати зламаний вузол, а не ховати його: стан
        // лишається звичайним, відмова виконувати стоїть на гейтах.
        let files = [
            ("task.md", "---\nschema_version: 5\n---\n\n## Task\n\nx\n"),
            ("a.md", "---\nschema_version: 1\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
        assert_eq!(warnings_of("task", &files).len(), 1);
    }

    // ── orphan-node (graph.md, «Протокол spawn») ──

    /// Дерево `mt/<parent>/…` з дитиною; `approved` — чи писати
    /// plan_001 + plan-approved_001 з дитиною в `## Children`.
    fn parent_with_child(approved_ids: Option<&[&str]>, child_id: &str) -> Vec<String> {
        let root = tempdir().unwrap();
        let tasks_root = root.path().join("mt");
        let parent = tasks_root.join("parent");
        let child = parent.join(child_id);
        fs::create_dir_all(&child).unwrap();
        let task = "---\nschema_version: 1\n---\n\n## Task\n\nx\n";
        fs::write(parent.join("task.md"), task).unwrap();
        fs::write(child.join("task.md"), task).unwrap();
        if let Some(ids) = approved_ids {
            let listed = ids
                .iter()
                .map(|id| format!("  - id: {id}\n    mode: agent\n"))
                .collect::<String>();
            fs::write(
                parent.join("plan_001.md"),
                format!("---\nschema_version: 1\n---\n\n## Children\n\n```yaml\nchildren:\n{listed}```\n"),
            )
            .unwrap();
            fs::write(
                parent.join("plan-approved_001.md"),
                "---\nschema_version: 1\n---\n",
            )
            .unwrap();
        }
        let nodes = scan_tasks(tasks_root.to_string_lossy().into_owned(), vec![]).unwrap();
        find_node(&nodes, &format!("parent/{child_id}"))
            .expect("child not found")
            .warnings
            .clone()
    }

    #[test]
    fn child_listed_in_approved_plan_is_legitimate() {
        assert!(parent_with_child(Some(&["collect"]), "collect").is_empty());
    }

    #[test]
    fn child_absent_from_approved_plan_is_orphan() {
        let w = parent_with_child(Some(&["collect"]), "smuggled");
        assert_eq!(w.len(), 1, "got: {w:?}");
        assert!(w[0].starts_with("orphan-node:"), "got: {w:?}");
    }

    #[test]
    fn child_without_any_approved_plan_is_orphan() {
        // Без approve дітей не існує легітимно — правило легітимності spawn.
        let w = parent_with_child(None, "collect");
        assert_eq!(w.len(), 1, "got: {w:?}");
        assert!(w[0].starts_with("orphan-node:"));
    }

    #[test]
    fn root_level_node_is_never_orphan() {
        let files = [("task.md", "---\nschema_version: 1\n---\n\n## Task\n\nx\n")];
        assert!(warnings_of("solo", &files).is_empty());
    }

    // ── awaiting-decision ──
    #[test]
    fn awaiting_decision_when_marker_open() {
        // Вичерпана драбина + розвилка: вузол чекає на людину, а не
        // «зламався». Саме це відрізняє розвилку від `unresolvable`.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", ""),
            ("run_002.md", ""),
            ("run_003.md", ""),
            (
                "awaiting-decision_0001.md",
                "---\nschema_version: 1\ntype: awaiting-decision\n---\n",
            ),
        ];
        assert_eq!(
            state_of("task", &files, &[], None),
            TaskState::AwaitingDecision
        );
    }

    #[test]
    fn awaiting_decision_wins_over_unresolvable() {
        // Обидва маркери разом означають «драбину вичерпано і розвилку
        // спаковано» — вузол чекає на відповідь, а не термінально мертвий.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("unresolvable.md", ""),
            ("awaiting-decision_0001.md", ""),
        ];
        assert_eq!(
            state_of("task", &files, &[], None),
            TaskState::AwaitingDecision
        );
    }

    #[test]
    fn answered_decision_releases_state() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("awaiting-decision_0001.md", ""),
            (
                "decided_0001.md",
                "---\nschema_version: 1\nchosen_option: B\n---\n",
            ),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }

    #[test]
    fn accepted_fact_still_wins_over_open_decision() {
        // Пріоритет зі спеки: прийнятий результат старший за розвилку —
        // інакше закритий вузол «воскресав» би через забутий маркер.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("awaiting-decision_0001.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Resolved);
    }

    // ── failed ──
    #[test]
    fn failed_when_streak_reaches_max() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", ""),
            ("run_002.md", ""),
            ("run_003.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Failed); // streak 3 >= 3
    }
    #[test]
    fn failed_with_custom_retry_max_1() {
        let files = [("task.md", ""), ("a.md", ""), ("run_001.md", "")];
        assert_eq!(state_of("task", &files, &[], Some(1)), TaskState::Failed);
    }
    #[test]
    fn not_failed_when_fact_resets_streak() {
        // fact_001 with no pending-audit → resolved (checked before failed)
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", ""),
            ("fact_001.md", ""),
            ("run_002.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Resolved);
    }
    #[test]
    fn lifecycle_results_do_not_count_toward_streak() {
        // graph.md: decomposed | claim-lost | handoff — категорія lifecycle,
        // failed_streak не змінюють. Три міграції сесії ≠ failed.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: handoff\n---\n"),
            ("run_002.md", "---\nresult: claim-lost\n---\n"),
            ("run_003.md", "---\nresult: decomposed\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }
    #[test]
    fn execution_failures_count_toward_streak() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("run_002.md", "---\nresult: progress-timeout\n---\n"),
            ("run_003.md", "---\nresult: budget-exceeded\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Failed);
    }
    #[test]
    fn handoff_between_failures_does_not_inflate_streak() {
        // Дві реальні невдачі + handoff посередині: streak 2 < 3 → ще waiting.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("run_002.md", "---\nresult: handoff\n---\n"),
            ("run_003.md", "---\nresult: merge-conflict\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }
    #[test]
    fn streak_counts_only_runs_after_accepted_fact() {
        // fact_002 прийнято (аудит success) → run_001 не рахується, streak 1 < 2.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("fact_002.md", ""),
            ("pending-audit_002.md", ""),
            ("audit-result_002.md", "---\nresult: success\n---\n"),
            ("run_003.md", "---\nresult: failed\n---\n"),
        ];
        // Прийнятий fact → resolved має пріоритет над станом лічильника.
        assert_eq!(state_of("task", &files, &[], Some(2)), TaskState::Resolved);
    }
    #[test]
    fn rejected_fact_does_not_reset_streak() {
        // fact_002 відхилено аудитом → межа 0 → рахуються run_001 і run_003:
        // streak 2 ≥ agent_retry_max 2 → failed, драбина термінує.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("fact_002.md", ""),
            ("pending-audit_002.md", ""),
            ("audit-result_002.md", "---\nresult: failed\n---\n"),
            ("run_003.md", "---\nresult: failed\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], Some(2)), TaskState::Failed);
    }
    #[test]
    fn rejected_fact_livelock_terminates() {
        // Патологія, яку закриває межа «прийнятий fact»: кожен цикл дає сирий
        // fact, аудит його відхиляє. Раніше лічильник обнулявся вічно.
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("fact_002.md", ""),
            ("pending-audit_002.md", ""),
            ("audit-result_002.md", "---\nresult: failed\n---\n"),
            ("run_003.md", "---\nresult: failed\n---\n"),
            ("fact_004.md", ""),
            ("pending-audit_004.md", ""),
            ("audit-result_004.md", "---\nresult: failed\n---\n"),
            ("run_005.md", "---\nresult: failed\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Failed);
    }

    // ── remote claims → running / stalled ──

    fn tree_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<TaskNode>) {
        let root = tempdir().unwrap();
        let tasks_root = root.path().join("mt");
        let dir = tasks_root.join("solo");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
        let nodes = scan_tasks(tasks_root.to_string_lossy().into_owned(), vec![]).unwrap();
        (root, nodes)
    }

    #[test]
    fn active_remote_claim_shows_node_as_running() {
        let (_root, mut nodes) = tree_with(&[
            ("task.md", "---\nschema_version: 1\n---\n"),
            ("a.md", "---\nschema_version: 1\n---\n"),
        ]);
        assert_eq!(nodes[0].state, TaskState::Waiting, "локально — вільний");

        let hash = claims::node_hash("mt", "solo");
        let map = HashMap::from([(hash, false)]);
        apply_remote_claims(&mut nodes, "mt", &map);
        // Вузол виконує інший хост — оркестратор не має його чіпати.
        assert_eq!(nodes[0].state, TaskState::Running);
    }

    #[test]
    fn expired_remote_claim_shows_node_as_stalled() {
        let (_root, mut nodes) = tree_with(&[
            ("task.md", "---\nschema_version: 1\n---\n"),
            ("a.md", "---\nschema_version: 1\n---\n"),
        ]);
        let map = HashMap::from([(claims::node_hash("mt", "solo"), true)]);
        apply_remote_claims(&mut nodes, "mt", &map);
        assert_eq!(nodes[0].state, TaskState::Stalled);
    }

    #[test]
    fn remote_claim_does_not_override_terminal_states() {
        // Пріоритет зі спеки: resolved / unresolvable / pending-audit вище
        // за running і stalled — інакше протухлий claim «воскрешав» би
        // завершений вузол.
        for (files, expected) in [
            (
                vec![
                    ("task.md", "---\nschema_version: 1\n---\n"),
                    ("a.md", "---\nschema_version: 1\n---\n"),
                    ("fact_001.md", ""),
                ],
                TaskState::Resolved,
            ),
            (
                vec![
                    ("task.md", "---\nschema_version: 1\n---\n"),
                    ("a.md", "---\nschema_version: 1\n---\n"),
                    ("unresolvable.md", "---\nschema_version: 1\n---\n"),
                ],
                TaskState::Unresolvable,
            ),
        ] {
            let (_root, mut nodes) = tree_with(&files);
            let map = HashMap::from([(claims::node_hash("mt", "solo"), true)]);
            apply_remote_claims(&mut nodes, "mt", &map);
            assert_eq!(nodes[0].state, expected);
        }
    }

    #[test]
    fn nodes_without_claims_keep_local_state() {
        let (_root, mut nodes) = tree_with(&[
            ("task.md", "---\nschema_version: 1\n---\n"),
            ("a.md", "---\nschema_version: 1\n---\n"),
        ]);
        apply_remote_claims(&mut nodes, "mt", &HashMap::new());
        assert_eq!(nodes[0].state, TaskState::Waiting);
    }

    // ── три тригери unresolvable (graph.md) ──

    /// Директорія вузла з набором файлів + конфіг; повертає причину.
    fn reason_for(files: &[(&str, &str)], config: serde_json::Value) -> Option<String> {
        let root = tempdir().unwrap();
        let dir = root.path().join("node");
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            fs::write(dir.join(name), content).unwrap();
        }
        unresolvable_reason(&dir, &config)
    }

    fn failed_run(n: u64) -> (String, String) {
        (
            format!("run_{n:03}.md"),
            "---\nschema_version: 1\nresult: failed\nwall_sec: 100\n---\n".to_string(),
        )
    }

    #[test]
    fn no_trigger_on_healthy_node() {
        let runs: Vec<_> = (1..=2).map(failed_run).collect();
        let files: Vec<(&str, &str)> = runs.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
        // streak 2 < 3+1 → ще ретраїмо.
        assert!(reason_for(&files, serde_json::json!({})).is_none());
    }

    #[test]
    fn trigger_execution_ladder_exhausted() {
        let runs: Vec<_> = (1..=4).map(failed_run).collect();
        let files: Vec<(&str, &str)> = runs.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
        // agent_retry_max 3 + engineer_retry_max 1 = 4.
        let reason = reason_for(&files, serde_json::json!({})).expect("має спрацювати");
        assert!(reason.contains("драбину виконання"), "got: {reason}");
    }

    #[test]
    fn trigger_planning_ladder_exhausted() {
        let files = [
            ("plan-rejected_001.md", ""),
            ("plan-rejected_002.md", ""),
            ("plan-rejected_003.md", ""),
        ];
        let reason = reason_for(&files, serde_json::json!({})).expect("має спрацювати");
        assert!(reason.contains("драбину планування"), "got: {reason}");
    }

    #[test]
    fn trigger_total_budget_exceeded() {
        let runs: Vec<_> = (1..=2).map(failed_run).collect();
        let files: Vec<(&str, &str)> = runs.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
        // 2 × 100s > 150s.
        let reason = reason_for(&files, serde_json::json!({"budget_total_sec": 150}))
            .expect("має спрацювати");
        assert!(reason.contains("сумарний бюджет"), "got: {reason}");
    }

    #[test]
    fn total_budget_trigger_is_opt_in() {
        let runs: Vec<_> = (1..=2).map(failed_run).collect();
        let files: Vec<(&str, &str)> = runs.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
        // Без budget_total_sec третій тригер неактивний (спека: опційний).
        assert!(reason_for(&files, serde_json::json!({})).is_none());
    }

    #[test]
    fn write_unresolvable_is_idempotent() {
        let root = tempdir().unwrap();
        let dir = root.path().join("node");
        fs::create_dir_all(&dir).unwrap();
        assert!(write_unresolvable(&dir, "перша причина").unwrap());
        // Маркер immutable — повторний виклик не перезаписує.
        assert!(!write_unresolvable(&dir, "інша причина").unwrap());
        let content = fs::read_to_string(dir.join("unresolvable.md")).unwrap();
        assert!(content.contains("перша причина"));
        assert!(content.contains("schema_version: 1"));
    }

    #[test]
    fn unresolvable_marker_wins_over_failed_state() {
        // Термінальний стан має пріоритет над «ще ретраїмо».
        let files = [
            ("task.md", "---\nschema_version: 1\n---\n"),
            ("a.md", "---\nschema_version: 1\n---\n"),
            ("run_001.md", "---\nresult: failed\n---\n"),
            ("run_002.md", "---\nresult: failed\n---\n"),
            ("run_003.md", "---\nresult: failed\n---\n"),
            ("unresolvable.md", "---\nschema_version: 1\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Unresolvable);
    }

    // ── unresolvable ──
    #[test]
    fn unresolvable_marker() {
        assert_eq!(
            state_of(
                "task",
                &[("task.md", ""), ("a.md", ""), ("unresolvable.md", "")],
                &[],
                None
            ),
            TaskState::Unresolvable
        );
    }

    // ── running: marker + worktree ──
    #[test]
    fn running_marker() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("running_4821_until_1234567890", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Running);
    }
    #[test]
    fn running_worktree_match() {
        let files = [("task.md", ""), ("a.md", "")];
        assert_eq!(
            state_of("my-task", &files, &["my-task-1234567890"], None),
            TaskState::Running
        );
    }
    #[test]
    fn not_running_when_worktree_mismatch() {
        let files = [("task.md", ""), ("a.md", "")];
        assert_eq!(
            state_of("my-task", &files, &["other-task-1234567890"], None),
            TaskState::Waiting
        );
    }
    #[test]
    fn running_worktree_nested_cross_level() {
        // research/analyze node, worktree "research-analyze-<ts>"
        let root = tempdir().unwrap();
        let analyze = root.path().join("mt/research/analyze");
        fs::create_dir_all(&analyze).unwrap();
        fs::write(root.path().join("mt/research/task.md"), "").unwrap();
        fs::write(analyze.join("task.md"), "").unwrap();
        fs::write(analyze.join("a.md"), "").unwrap();
        let nodes = scan_tasks(
            root.path().join("mt").to_string_lossy().into_owned(),
            vec!["research-analyze-1234567890".to_string()],
        )
        .unwrap();
        assert_eq!(
            find_node(&nodes, "research/analyze").unwrap().state,
            TaskState::Running
        );
    }

    // ── plan-review / spawned ──
    #[test]
    fn plan_review_composite_unapproved() {
        let files = [("task.md", ""), ("a.md", ""), ("plan_001.md", COMPOSITE)];
        assert_eq!(state_of("task", &files, &[], None), TaskState::PlanReview);
    }
    #[test]
    fn atomic_plan_is_waiting_not_review() {
        let files = [("task.md", ""), ("a.md", ""), ("plan_001.md", ATOMIC)];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }
    #[test]
    fn composite_approved_without_children_falls_to_waiting() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("plan_001.md", COMPOSITE),
            ("plan-approved_001.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }
    #[test]
    fn plan_review_for_human_composite() {
        let files = [("task.md", ""), ("h.md", ""), ("plan_001.md", COMPOSITE)];
        assert_eq!(state_of("task", &files, &[], None), TaskState::PlanReview);
    }
    #[test]
    fn spawned_when_composite_approved_with_children() {
        let root = tempdir().unwrap();
        let parent = root.path().join("mt/parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();
        fs::write(parent.join("task.md"), "").unwrap();
        fs::write(parent.join("a.md"), "").unwrap();
        fs::write(parent.join("plan_001.md"), COMPOSITE).unwrap();
        fs::write(parent.join("plan-approved_001.md"), "").unwrap();
        fs::write(child.join("task.md"), "").unwrap();
        let nodes = scan_tasks(
            root.path().join("mt").to_string_lossy().into_owned(),
            vec![],
        )
        .unwrap();
        assert_eq!(
            find_node(&nodes, "parent").unwrap().state,
            TaskState::Spawned
        );
    }

    // ── pending-audit / resolved (latest fact only) ──
    #[test]
    fn pending_audit_open_cycle() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("pending-audit_001.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::PendingAudit);
    }
    #[test]
    fn resolved_when_newer_fact_supersedes_audit() {
        // fact_001 has open audit, but fact_002 (latest) has none → resolved
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("pending-audit_001.md", ""),
            ("fact_002.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Resolved);
    }
    #[test]
    fn resolved_plain_fact() {
        assert_eq!(
            state_of(
                "task",
                &[("task.md", ""), ("a.md", ""), ("fact_001.md", "")],
                &[],
                None
            ),
            TaskState::Resolved
        );
    }
    #[test]
    fn resolved_audit_success() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("pending-audit_001.md", ""),
            ("audit-result_001.md", "---\nresult: success\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Resolved);
    }
    #[test]
    fn audit_failed_falls_through_to_waiting() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("pending-audit_001.md", ""),
            ("audit-result_001.md", "---\nresult: failed\n---\n"),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Waiting);
    }

    // ── priority chain ──
    #[test]
    fn resolved_over_unresolvable() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("unresolvable.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Resolved);
    }
    #[test]
    fn pending_audit_over_resolved() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("fact_001.md", ""),
            ("pending-audit_001.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::PendingAudit);
    }
    #[test]
    fn unresolvable_over_running_marker() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("running_1_until_9999999999", ""),
            ("unresolvable.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Unresolvable);
    }
    #[test]
    fn running_over_plan_review() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("plan_001.md", COMPOSITE),
            ("running_1_until_9999999999", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Running);
    }
    #[test]
    fn unresolvable_over_failed() {
        let files = [
            ("task.md", ""),
            ("a.md", ""),
            ("run_001.md", ""),
            ("run_002.md", ""),
            ("run_003.md", ""),
            ("unresolvable.md", ""),
        ];
        assert_eq!(state_of("task", &files, &[], None), TaskState::Unresolvable);
    }

    // ── blocked post-processing ──
    #[test]
    fn waiting_with_unresolved_dep_becomes_blocked() {
        let root = tempdir().unwrap();
        let mt = root.path().join("mt");
        let a = mt.join("a");
        let b = mt.join("b");
        fs::create_dir_all(a.join("deps")).unwrap();
        fs::create_dir_all(&b).unwrap();
        // a depends on b; b is unassigned (not resolved) → a blocked
        fs::write(a.join("task.md"), "").unwrap();
        fs::write(a.join("a.md"), "").unwrap();
        fs::write(a.join("deps/b.md"), "").unwrap();
        fs::write(b.join("task.md"), "").unwrap();
        let nodes = scan_tasks(mt.to_string_lossy().into_owned(), vec![]).unwrap();
        assert_eq!(find_node(&nodes, "a").unwrap().state, TaskState::Blocked);
        assert_eq!(find_node(&nodes, "a").unwrap().deps, vec!["b".to_string()]);
    }

    // ── scan skips gitignored/hidden/denylisted dirs (spec «Монорепо») ──
    #[test]
    fn scan_skips_gitignored_and_denylisted_dirs() {
        let root = tempdir().unwrap();
        let mt = root.path().join("mt");
        // .gitignore у project root: mt/scratch ігнорується
        fs::write(root.path().join(".gitignore"), "scratch\n").unwrap();
        for (dir, tracked) in [
            ("visible", true),
            ("scratch", false),      // gitignored
            ("node_modules", false), // denylist
            (".hidden", false),      // hidden
        ] {
            let d = mt.join(dir);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("task.md"), "").unwrap();
            let _ = tracked;
        }
        let nodes = scan_tasks(mt.to_string_lossy().into_owned(), vec![]).unwrap();
        let paths: Vec<_> = nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, vec!["visible"]);
    }

    #[test]
    fn scan_skips_gitignored_child_dirs() {
        let root = tempdir().unwrap();
        let parent = root.path().join("mt/parent");
        fs::create_dir_all(parent.join("tmp-cache")).unwrap();
        fs::write(parent.join("task.md"), "").unwrap();
        fs::write(parent.join(".gitignore"), "tmp-*\n").unwrap();
        fs::write(parent.join("tmp-cache/task.md"), "").unwrap();
        let nodes = scan_tasks(
            root.path().join("mt").to_string_lossy().into_owned(),
            vec![],
        )
        .unwrap();
        assert!(find_node(&nodes, "parent").is_some());
        assert!(find_node(&nodes, "parent/tmp-cache").is_none());
        assert!(!find_node(&nodes, "parent").unwrap().is_composite);
    }

    // ── sanitize vectors (must match JS sanitizeTaskName) ──
    #[test]
    fn sanitize_vectors() {
        assert_eq!(sanitize("research/collect data"), "research-collect-data");
        assert_eq!(sanitize("my-task_01"), "my-task_01");
        assert_eq!(sanitize(""), "");
    }

    // ── create_task (write-side) ──

    /// <tmp>/mt with optional .mt.json defaults in the project root; returns (root, mt_dir).
    fn create_repo(mt_json: Option<&str>) -> (tempfile::TempDir, String) {
        let root = tempdir().unwrap();
        if let Some(j) = mt_json {
            fs::write(root.path().join(".mt.json"), j).unwrap();
        }
        let mt = root.path().join("mt");
        fs::create_dir_all(&mt).unwrap();
        let mt_dir = mt.to_string_lossy().into_owned();
        (root, mt_dir)
    }

    #[test]
    fn create_writes_task_flag_and_frontmatter() {
        let (root, mt) = create_repo(None);
        let opts = CreateOpts {
            mode: Some(Mode::Human),
            ..Default::default()
        };
        let outcome = create_task(mt.clone(), "demo".to_string(), opts).unwrap();
        match outcome {
            CreateOutcome::Created {
                name,
                task_path,
                flag,
                deps,
            } => {
                assert_eq!(name, "demo");
                assert_eq!(task_path, "demo/task.md");
                assert_eq!(flag, "h.md");
                assert!(deps.is_empty());
            }
            _ => panic!("expected Created"),
        }
        let task_md = fs::read_to_string(root.path().join("mt/demo/task.md")).unwrap();
        // schema_version must be the FIRST frontmatter field.
        assert!(
            task_md.starts_with("---\nschema_version: 1\n"),
            "got: {task_md}"
        );
        assert!(task_md.contains("\nbudget_sec: 1800\n")); // default
        assert!(task_md.contains("\nhint: atomic\n"));
        // No mode/executor/deps fields in frontmatter (§2.6/§2.7).
        assert!(!task_md.contains("mode:"));
        assert!(!task_md.contains("executor"));
        assert!(!task_md.contains("\ndeps:"));
        // Секції task.md — контракт graph.md (## Task / ## Done when / ## Check / ## Inputs).
        assert!(task_md.contains("## Task"));
        assert!(task_md.contains("## Done when"));
        assert!(task_md.contains("## Check")); // машинний done/audit-гейт (signal.rs)
        assert!(task_md.contains("## Inputs"));
        assert!(!task_md.contains("## Mission")); // старий канон docs/mt.md більше не пишемо
                                                  // h.md created, a.md not.
        assert!(root.path().join("mt/demo/h.md").exists());
        assert!(!root.path().join("mt/demo/a.md").exists());
    }

    #[test]
    fn create_agent_writes_a_md_with_tier() {
        let (root, mt) = create_repo(None);
        let opts = CreateOpts {
            mode: Some(Mode::Agent),
            model_tier: Some("MAX".to_string()),
            ..Default::default()
        };
        let outcome = create_task(mt, "agentic".to_string(), opts).unwrap();
        assert!(matches!(&outcome, CreateOutcome::Created { flag, .. } if flag == "a.md"));
        let a = fs::read_to_string(root.path().join("mt/agentic/a.md")).unwrap();
        assert!(a.starts_with("---\n"), "a.md має бути frontmatter: {a}");
        let fm = frontmatter::parse_front_matter(&a);
        assert_eq!(fm["schema_version"], serde_json::json!(1));
        assert_eq!(fm["model_tier"], serde_json::json!("MAX"));
        assert_eq!(fm["skills"], serde_json::json!(["bash", "write-files"]));
        assert_eq!(fm["interactive"], serde_json::json!(false));
        assert!(fm["created_at"].is_string());
        assert!(!root.path().join("mt/agentic/h.md").exists());
    }

    #[test]
    fn create_human_writes_h_md_frontmatter() {
        let (root, mt) = create_repo(None);
        let opts = CreateOpts {
            mode: Some(Mode::Human),
            qualification: Some("senior backend engineer".to_string()),
            ..Default::default()
        };
        create_task(mt, "manual".to_string(), opts).unwrap();
        let h = fs::read_to_string(root.path().join("mt/manual/h.md")).unwrap();
        let fm = frontmatter::parse_front_matter(&h);
        assert_eq!(fm["schema_version"], serde_json::json!(1));
        assert_eq!(
            fm["qualification"],
            serde_json::json!("senior backend engineer")
        );
        assert_eq!(fm["notify"], serde_json::json!(true));
    }

    #[test]
    fn create_is_idempotent() {
        let (root, mt) = create_repo(None);
        create_task(mt.clone(), "demo".to_string(), CreateOpts::default()).unwrap();
        let before = fs::read_to_string(root.path().join("mt/demo/task.md")).unwrap();
        let again = create_task(mt, "demo".to_string(), CreateOpts::default()).unwrap();
        assert!(matches!(again, CreateOutcome::Exists { .. }));
        let after = fs::read_to_string(root.path().join("mt/demo/task.md")).unwrap();
        assert_eq!(before, after); // not rewritten
    }

    #[test]
    fn create_nested_name_recursive_mkdir() {
        let (root, mt) = create_repo(None);
        create_task(
            mt,
            "research/collect-data".to_string(),
            CreateOpts::default(),
        )
        .unwrap();
        assert!(root
            .path()
            .join("mt/research/collect-data/task.md")
            .exists());
    }

    #[test]
    fn create_dep_writes_empty_edge_file() {
        let (root, mt) = create_repo(None);
        let opts = CreateOpts {
            deps: vec!["upstream".to_string()],
            ..Default::default()
        };
        let outcome = create_task(mt, "downstream".to_string(), opts).unwrap();
        assert!(matches!(&outcome, CreateOutcome::Created { deps, .. } if deps == &["upstream"]));
        let edge = root.path().join("mt/downstream/deps/upstream.md");
        assert!(edge.exists());
        assert_eq!(fs::read_to_string(&edge).unwrap(), "");
    }

    #[test]
    fn create_resolves_defaults_from_mt_json() {
        let (root, mt) = create_repo(Some(
            "{\"default_mode\":\"agent\",\"default_model_tier\":\"MIN\",\"default_budget_sec\":42}",
        ));
        let outcome = create_task(mt, "d".to_string(), CreateOpts::default()).unwrap();
        assert!(matches!(&outcome, CreateOutcome::Created { flag, .. } if flag == "a.md"));
        let task_md = fs::read_to_string(root.path().join("mt/d/task.md")).unwrap();
        assert!(task_md.contains("\nbudget_sec: 42\n"));
        let a = fs::read_to_string(root.path().join("mt/d/a.md")).unwrap();
        assert!(a.contains("MIN"));
    }

    // ── name-validation vectors, see tests/fixtures/name-vectors.json ──
    #[test]
    fn validate_name_shared_vectors() {
        let raw = include_str!("../tests/fixtures/name-vectors.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        for name in v["valid"].as_array().unwrap() {
            let n = name.as_str().unwrap();
            assert!(validate_name(n).is_ok(), "expected VALID: {n:?}");
        }
        for name in v["invalid"].as_array().unwrap() {
            let n = name.as_str().unwrap();
            assert!(validate_name(n).is_err(), "expected INVALID: {n:?}");
        }
    }

    #[test]
    fn create_rejects_traversal_name() {
        let (_root, mt) = create_repo(None);
        assert!(create_task(mt, "../escape".to_string(), CreateOpts::default()).is_err());
    }

    // ── write_plan_draft ──

    #[test]
    fn write_plan_draft_writes_first_and_next_nnn() {
        let (root, mt) = create_repo(None);
        create_task(mt.clone(), "demo".to_string(), CreateOpts::default()).unwrap();
        let file = write_plan_draft(&mt, "demo", None).unwrap();
        assert_eq!(file, "plan_001.md");
        let content = fs::read_to_string(root.path().join("mt/demo").join(&file)).unwrap();
        assert!(content.starts_with("---\nschema_version: 1\n"));
        assert!(content.contains("decision: atomic"));
        assert!(!content.contains("mode:"));
        assert!(content.contains("## Children"));

        let file2 = write_plan_draft(&mt, "demo", Some("agent")).unwrap();
        assert_eq!(file2, "plan_002.md");
        let content2 = fs::read_to_string(root.path().join("mt/demo").join(&file2)).unwrap();
        assert!(content2.contains("mode: agent"));
    }

    #[test]
    fn write_plan_draft_missing_node_errors() {
        let (_root, mt) = create_repo(None);
        assert!(write_plan_draft(&mt, "nope", None).is_err());
    }
}
