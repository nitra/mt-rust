//! napi-біндінги до `mt-core` для `@7n/mt`.
//!
//! Тонкий шар: конвертація типів JS ⇄ Rust і мапінг помилок у `napi::Error`.
//! Уся доменна логіка живе в `mt-core`. JS-обгортки — `npm/lib/core/native.mjs`.

use std::path::PathBuf;

use napi::bindgen_prelude::*;
use napi_derive::napi;

fn to_napi_err(e: String) -> Error {
    Error::from_reason(e)
}

/// Сканує tasks-директорію і повертає дерево вузлів (JSON-контракт як у CLI `scan`).
/// `worktrees: None` → discovery через `git worktree list` від `tasks_dir`.
#[napi]
pub fn scan_tasks(tasks_dir: String, worktrees: Option<Vec<String>>) -> Result<serde_json::Value> {
    let wt = worktrees.unwrap_or_else(|| mt_core::discover_worktrees(&PathBuf::from(&tasks_dir)));
    let nodes = mt_core::scan_tasks(tasks_dir, wt).map_err(to_napi_err)?;
    serde_json::to_value(nodes).map_err(|e| to_napi_err(e.to_string()))
}

/// Створює вузол задачі (JSON-контракт як у CLI `create`: поле `created: bool`).
#[napi]
pub fn create_task(
    tasks_dir: String,
    name: String,
    opts: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let opts: mt_core::CreateOpts = match opts {
        Some(v) => serde_json::from_value(v).map_err(|e| to_napi_err(e.to_string()))?,
        None => mt_core::CreateOpts::default(),
    };
    let outcome = mt_core::create_task(tasks_dir, name, opts).map_err(to_napi_err)?;
    Ok(outcome.to_cli_json())
}

/// Виявляє workspace-и (mt/-директорії) від заданих коренів або від cwd.
#[napi]
pub fn find_workspaces(dirs: Option<Vec<String>>) -> Result<serde_json::Value> {
    let workspaces = match dirs {
        Some(ds) if !ds.is_empty() => ds
            .iter()
            .flat_map(|d| mt_core::find_all_tasks_dirs_from(&PathBuf::from(d)))
            .collect(),
        _ => mt_core::find_all_tasks_dirs().map_err(to_napi_err)?,
    };
    serde_json::to_value(workspaces).map_err(|e| to_napi_err(e.to_string()))
}

/// Імена активних git-worktree (останній компонент шляху) від `start_dir`.
#[napi]
pub fn discover_worktrees(start_dir: String) -> Vec<String> {
    mt_core::discover_worktrees(&PathBuf::from(&start_dir))
}

// ── nnn (npm/lib/core/nnn.mjs) ────────────────────────────────────────────────

/// Форматує число як NNN-рядок ('001', '002', …).
#[napi]
pub fn pad_nnn(n: u32) -> String {
    mt_core::nnn::pad_nnn(u64::from(n))
}

/// Наступний NNN для `run_NNN.md`: count(run_*.md) + 1.
#[napi]
pub fn next_run_nnn(files: Vec<String>) -> String {
    mt_core::nnn::next_run_nnn(&files)
}

/// Наступний NNN для `plan_NNN.md`: max(plan_*.md) + 1.
#[napi]
pub fn next_plan_nnn(files: Vec<String>) -> String {
    mt_core::nnn::next_plan_nnn(&files)
}

/// Найвищий NNN серед `fact_NNN.md`, або null.
#[napi]
pub fn latest_fact_nnn(files: Vec<String>) -> Option<String> {
    mt_core::nnn::latest_fact_nnn(&files)
}

/// Найвищий NNN серед `pending-audit_NNN.md`, або null.
#[napi]
pub fn latest_pending_audit_nnn(files: Vec<String>) -> Option<String> {
    mt_core::nnn::latest_pending_audit_nnn(&files)
}

/// Найвищий NNN серед `audit-result_NNN.md`, або null.
#[napi]
pub fn latest_audit_result_nnn(files: Vec<String>) -> Option<String> {
    mt_core::nnn::latest_audit_result_nnn(&files)
}

// ── frontmatter (npm/lib/core/frontmatter.mjs) ────────────────────────────────

/// Парсить YAML front-matter з markdown-тексту (без fm → `{}`).
#[napi]
pub fn parse_front_matter(text: String) -> serde_json::Value {
    mt_core::frontmatter::parse_front_matter(&text)
}

/// Тіло документа без front-matter.
#[napi]
pub fn get_body(text: String) -> String {
    mt_core::frontmatter::get_body(&text)
}

/// Серіалізує об'єкт у YAML-рядок (байт-у-байт як JS `serializeYaml`).
#[napi]
pub fn serialize_yaml(obj: serde_json::Value, indent_level: Option<u32>) -> String {
    mt_core::frontmatter::serialize_yaml(&obj, indent_level.unwrap_or(0) as usize)
}

/// Будує markdown-файл із front-matter і тілом.
#[napi]
pub fn build_markdown(fm: serde_json::Value, body: Option<String>) -> String {
    mt_core::frontmatter::build_markdown(&fm, body.as_deref().unwrap_or(""))
}

// ── state (npm/lib/core/state.mjs) ────────────────────────────────────────────

/// Санітизує ім'я задачі для worktree: `[^A-Za-z0-9_-]` → '-'.
#[napi]
pub fn sanitize_task_name(name: String) -> String {
    mt_core::sanitize(&name)
}

/// Валідує id вузла (§8 spec). Повертає текст помилки або null якщо валідне.
#[napi]
pub fn validate_task_name(name: String) -> Option<String> {
    mt_core::validate_name(&name).err()
}

/// Нормалізує ім'я гілки до безпечного імені директорії у `.worktrees/`.
#[napi]
pub fn sanitize_branch(branch: String) -> String {
    mt_core::sanitize_branch(&branch)
}

// ── config (npm/lib/core/config.mjs) ──────────────────────────────────────────

/// Дефолтна конфігурація (JS `CONFIG_DEFAULTS`, порядок ключів збережено).
#[napi]
pub fn config_defaults() -> serde_json::Value {
    mt_core::config::config_defaults()
}

/// Зливає сирий текст `.mt.json` (або null) з дефолтами
#[napi]
pub fn merge_config(raw: Option<String>) -> serde_json::Value {
    mt_core::config::merge_config(raw.as_deref())
}

/// Ефективний конфіг вузла: plan_NNN > .mt-override.json > task.md > .mt.json.
#[napi]
pub fn effective_config(
    mt_json: Option<String>,
    task_md: Option<String>,
    mt_override_json: Option<String>,
    plan_md: Option<String>,
) -> serde_json::Value {
    mt_core::config::effective_config(
        mt_json.as_deref(),
        task_md.as_deref(),
        mt_override_json.as_deref(),
        plan_md.as_deref(),
    )
}

// ── runner (npm/lib/commands/run.mjs — тонкий клієнт) ─────────────────────────

/// Preflight вузла (бюджети, NNN/attempt, тир/драбина, agent_cli) — план
/// запуску або помилка-відмова. Конфіг виконавців — ENV процесу.
#[napi]
pub fn run_preflight(tasks_dir: String, node_path: String) -> Result<serde_json::Value> {
    let plan = mt_core::runner::preflight(&tasks_dir, &node_path).map_err(to_napi_err)?;
    serde_json::to_value(plan).map_err(|e| to_napi_err(e.to_string()))
}

/// Запускає вузол: CAS claim → worktree → виконавець (підписочний CLI з
/// каскадом або node_executor) → `## Check` → fenced publish. **Блокуючий.**
#[napi]
pub fn run_node(tasks_dir: String, node_path: String) -> Result<serde_json::Value> {
    let outcome = mt_core::runner::run_node(&tasks_dir, &node_path).map_err(to_napi_err)?;
    serde_json::to_value(outcome).map_err(|e| to_napi_err(e.to_string()))
}

/// Оркестраторний прохід `run --auto`: waiting-агентські вузли чергами по
/// `concurrency` через run_node. **Блокуючий.**
#[napi]
pub fn run_auto(tasks_dir: String, concurrency: u32) -> Result<serde_json::Value> {
    let results =
        mt_core::orchestrate::run_auto(&tasks_dir, concurrency as usize).map_err(to_napi_err)?;
    serde_json::to_value(results).map_err(|e| to_napi_err(e.to_string()))
}

/// `mt kill` (файловий рівень): піддерево без run-артефактів видаляється
/// назавжди; інакше — архів у `.history/<ts>-kill-<path>/`. Повертає
/// `deleted:<path>` або `.history/<archive>`.
#[napi]
pub fn kill_node(tasks_dir: String, node_path: String) -> Result<String> {
    mt_core::lifecycle::kill(&tasks_dir, &node_path).map_err(to_napi_err)
}

// ── worktree (npm/lib/core/worktree.mjs) ──────────────────────────────────────

/// Ім'я worktree для задачі: `<sanitized-path>-<epoch-сек>`.
#[napi]
pub fn make_worktree_name(task_path: String, epoch_sec: i64) -> String {
    mt_core::worktree::make_worktree_name(&task_path, epoch_sec.max(0) as u64)
}

/// Перший запис зі списку, що належить задачі (точний або `<prefix>-...`), або null.
#[napi]
pub fn find_worktree_match(entries: Vec<String>, task_path: String) -> Option<String> {
    mt_core::worktree::find_worktree_match(&entries, &task_path)
}
