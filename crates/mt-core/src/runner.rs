//! Run-wrapper (спека mt.md, «Wrapper-скрипт») — git-режим за замовчуванням:
//! CAS claim → detached worktree від `origin/main` → spawn виконавця у
//! worktree → watchdog (hard budget → SIGKILL, progress-timeout за mtime) →
//! підсумок через [`crate::signal`] (fact є і `## Check` пройдено → done/audit
//! і composite вгору, інакше failed із секціями з `run-draft.md`) → коміт
//! worktree → fenced publish.
//!
//! Виконавці — **підписочні CLI**, єдиний agent-шлях (`claude` | `codex` |
//! `cursor` | `pi`, runtime.md «Підписочні CLI-виконавці»; точку розширення
//! `node_executor` видалено — PR #48): конфіг — user-level ENV
//! ([`crate::config::AgentCliEnv`]), per-node override — `a.md` «## Agent
//! cli»; вичерпані ліміти підписки → каскад `MT_CLOUD_AGENT_CLIS`. Retry
//! ladder (`## Retry ladder` у `a.md` або дефолт
//! base/diagnose-first/alternative-approach) резолвить стратегію спроби та
//! ескалацію model_tier MIN→AVG→MAX.
//!
//! Вимагає git-репозиторій з `origin`, до якого є push-доступ (claims/publish
//! — реальні мутації спільного remote). Rejected claim / merge-conflict /
//! вичерпаний publish-retry → `Err` (нормальний "інший runner виграв", не
//! системний збій) — викликач (`orchestrate::run_auto`) додає вузол у
//! skip-set цього проходу й переходить до інших.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::claims::{
    acquire_claim, discover_repo_root, node_hash, tasks_root_relative, ClaimFields,
};
use crate::config::{
    agent_cli_env_from_process, merge_config, normalize_model_tier, resolve_model_for_cli,
    AgentCliEnv,
};
use crate::frontmatter::parse_front_matter;
use crate::nnn::pad_nnn;
use crate::publish::{fenced_publish, PublishRequest};
use crate::signal::{self, next_run_nnn, write_run_fm};
use crate::worktree::{create_run_worktree, push_run_ref, remove_run_worktree};
use crate::{accepted_fact_state, validate_name, FactState};

/// Підтримувані підписочні CLI-виконавці (порядок — лише для повідомлень).
pub const AGENT_CLIS: [&str; 4] = ["claude", "codex", "cursor", "pi"];

/// Порядок model_tier для ескалації драбиною (позиційний зсув, cap на MAX).
const MODEL_TIER_ORDER: [&str; 3] = ["MIN", "AVG", "MAX"];

/// Щабель драбини ретраїв: стратегія + зсув тиру.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LadderStep {
    pub strategy: String,
    pub model_tier_delta: usize,
}

/// Драбина ретраїв за замовчуванням (graph.md «Retry ladder»): 1 — base;
/// 2 — diagnose-first; 3 — alternative-approach (+1 model_tier).
fn default_retry_ladder() -> Vec<LadderStep> {
    ["base", "diagnose-first", "alternative-approach"]
        .into_iter()
        .map(|strategy| LadderStep {
            strategy: strategy.to_string(),
            model_tier_delta: usize::from(strategy == "alternative-approach"),
        })
        .collect()
}

/// План запуску після preflight — бюджети, NNN, щабель ретраю, виконавець.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunPlan {
    pub nnn: u64,
    pub attempt: u64,
    pub budget_sec: u64,
    pub budget_hard_sec: u64,
    pub progress_timeout_sec: u64,
    /// Ефективний тир MIN/AVG/MAX (після ескалації щаблем драбини).
    pub model_tier: String,
    /// Стратегія щабля драбини (`MT_RETRY_STRATEGY`).
    pub retry_strategy: String,
    /// Підписочний CLI вузла: `a.md` «## Agent cli» → env `MT_AGENT_CLI` → claude.
    pub agent_cli: String,
}

/// Підсумок спроби (файли вже опубліковані в `origin/main`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutcome {
    /// success | failed | progress-timeout | budget-exceeded
    pub result: String,
    pub run_file: String,
    pub fact_file: Option<String>,
    pub wall_sec: u64,
    /// Фактичний CLI після каскаду (None — всі кандидати вичерпали ліміти).
    pub agent_cli: Option<String>,
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

fn fm_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(serde_json::Value::as_u64)
}

/// Непорожні рядки секції `## <title>` прапора `a.md` — спільний
/// markdown-конвент прапорів виконавця («## Model tier», «## Retry ladder»,
/// «## Agent cli»). Немає a.md/секції/рядків → None.
fn read_flag_section(dir: &Path, title_lower: &str) -> Option<Vec<String>> {
    let content = fs::read_to_string(dir.join("a.md")).ok()?;
    let mut lines = content.lines();
    lines.find(|l| l.trim().to_lowercase() == title_lower)?;
    let values: Vec<String> = lines
        .take_while(|l| !l.trim_start().starts_with("##"))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    (!values.is_empty()).then_some(values)
}

/// Парсить рядки секції «## Retry ladder» у драбину (буліт/рядок на щабель).
/// Щабель "alternative-approach" завжди несе `model_tier_delta: 1` (graph.md).
fn parse_retry_ladder(lines: &[String]) -> Option<Vec<LadderStep>> {
    let steps: Vec<LadderStep> = lines
        .iter()
        .map(|l| l.trim_start_matches(['-', '*']).trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .map(|strategy| LadderStep {
            model_tier_delta: usize::from(strategy == "alternative-approach"),
            strategy,
        })
        .collect();
    (!steps.is_empty()).then_some(steps)
}

/// Щабель драбини для номера спроби; коротша драбина — останній щабель
/// повторюється (graph.md).
fn resolve_retry_step(attempt: u64, ladder: &[LadderStep]) -> &LadderStep {
    let idx = (attempt.max(1) - 1).min(ladder.len() as u64 - 1) as usize;
    &ladder[idx]
}

/// Підвищує model_tier на `delta` позицій MIN→AVG→MAX (cap на MAX).
/// Невідомий tier або delta=0 → без змін.
fn bump_model_tier(tier: &str, delta: usize) -> String {
    if delta == 0 {
        return tier.to_string();
    }
    match MODEL_TIER_ORDER.iter().position(|t| *t == tier) {
        Some(idx) => MODEL_TIER_ORDER[(idx + delta).min(MODEL_TIER_ORDER.len() - 1)].to_string(),
        None => tier.to_string(),
    }
}

/// argv підписочного CLI: команда + аргументи headless-запуску. Модель
/// передається лише за наявності мапінгу (`MT_AGENT_CLI_MODEL_MAP`); без неї
/// CLI резолвить модель сам. Невідомий CLI → None.
///
/// Прапори звірені живим спайком 2026-07-14 (claude 2.1.193, codex 0.142.5,
/// cursor-agent 2026.07.01, pi 0.80.3): у claude немає `--no-session`
/// (є `--no-session-persistence`), у codex exec немає `--full-auto`
/// (пісочниця — `--sandbox workspace-write`, сесія — `--ephemeral`).
fn build_agent_cli_argv(
    cli: &str,
    model: Option<&str>,
    prompt: &str,
) -> Option<(String, Vec<String>)> {
    let mut args: Vec<String> = Vec::new();
    let cmd = match cli {
        "claude" => {
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
            args.extend([
                "--no-session-persistence".into(),
                "-p".into(),
                prompt.into(),
            ]);
            "claude"
        }
        "codex" => {
            args.push("exec".into());
            if let Some(m) = model {
                args.extend(["-m".into(), m.into()]);
            }
            args.extend([
                "--sandbox".into(),
                "workspace-write".into(),
                "--ephemeral".into(),
                prompt.into(),
            ]);
            "codex"
        }
        "cursor" => {
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
            args.extend(["--print".into(), "--force".into(), prompt.into()]);
            "cursor-agent"
        }
        "pi" => {
            if let Some(m) = model {
                args.extend(["--model".into(), m.into()]);
            }
            args.extend(["--no-session".into(), "-p".into(), prompt.into()]);
            "pi"
        }
        _ => return None,
    };
    Some((cmd.to_string(), args))
}

/// Порядок каскаду: `[обраний agent_cli, ...cloud_agent_clis]` без дублів
/// (невідомі імена лишаються — спавн їх пропустить).
fn cascade_order(agent_cli: &str, cloud: &[String]) -> Vec<String> {
    let mut order = vec![agent_cli.to_string()];
    for cli in cloud {
        if !order.contains(cli) {
            order.push(cli.clone());
        }
    }
    order
}

/// Чи схожий результат CLI на вичерпані ліміти підписки: ненульовий exit і
/// rate-limit-маркер у виводі (best-effort текстова евристика — до
/// структурованих ACP-помилок, ADR 260713-2110).
fn is_rate_limited(exit_ok: bool, output: &str) -> bool {
    if exit_ok {
        return false;
    }
    let t = output.to_lowercase();
    if [
        "too many requests",
        "usage limit",
        "quota exceeded",
        "quota reached",
    ]
    .iter()
    .any(|m| t.contains(m))
    {
        return true;
    }
    // rate.?limit — до одного символу між словами.
    let squashed: String = t.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if squashed.contains("ratelimit") {
        return true;
    }
    // \b429\b — «429» без цифр по сусідству.
    let bytes = t.as_bytes();
    t.match_indices("429").any(|(i, _)| {
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
        let after_ok = i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_digit();
        before_ok && after_ok
    })
}

/// Headless-промпт agent-шляху — спільний для всіх підписочних CLI.
///
/// Місія **вкладається** у промпт (тіло `task.md` без frontmatter): непряме
/// «прочитай task.md» — заважке для слабких локальних моделей (тертя M0,
/// dogfood 2026-07-15: gemma-2B через pi виконує пряму інструкцію, але
/// губиться на meta-prompt). `plan_*.md` лишаються за посиланням — вони
/// опційні і можуть бути великими.
fn build_agent_prompt(task_path: &str, node_dir: &Path, nnn: &str, budget_sec: u64) -> String {
    let task_body = fs::read_to_string(node_dir.join("task.md"))
        .map(|content| {
            let trimmed = content.trim_start();
            match trimmed.strip_prefix("---") {
                Some(rest) => rest
                    .split_once("\n---")
                    .map(|(_, body)| body.trim_start_matches('\n').to_string())
                    .unwrap_or(content.clone()),
                None => content.clone(),
            }
        })
        .unwrap_or_default();
    format!(
        "You are executing task: {task_path}\nWorking directory: {}\nRun NNN: {nnn}\nBudget: {budget_sec}s\n\n\
         The task (from task.md):\n\n{task_body}\n\n\
         Execute the task above in the current directory (read plan_*.md if present).\n\n\
         MANDATORY FINAL STEP: create the file fact_{nnn}.md in the current directory. \
         Without fact_{nnn}.md the run counts as FAILED even if everything else is done. Example content:\n\n\
         ## Summary\n\n<one sentence describing the result>",
        node_dir.display()
    )
}

/// Preflight за спекою: a.md, deps resolved, без відкритого аудиту, вузол не
/// running; бюджети — task.md > .mt.json > дефолти; виконавець — a.md-прапори,
/// далі ENV, далі дефолти. Суто локальні перевірки (без git) — дешевий гейт
/// перед дорожчим claim acquisition.
pub fn preflight(tasks_dir: &str, node_path: &str) -> Result<RunPlan, String> {
    preflight_env(tasks_dir, node_path, &agent_cli_env_from_process())
}

/// Як [`preflight`], але з явним конфігом виконавців (ін'єкція для тестів
/// і викликачів, що вже прочитали ENV).
pub fn preflight_env(
    tasks_dir: &str,
    node_path: &str,
    cli_env: &AgentCliEnv,
) -> Result<RunPlan, String> {
    let dir = node_dir(tasks_dir, node_path)?;
    if !dir.join("a.md").is_file() {
        return Err("вузол без a.md — runner запускає лише агентські вузли".to_string());
    }
    if crate::has_running_marker(&dir) {
        return Err("вузол уже running (є running_* маркер)".to_string());
    }
    match accepted_fact_state(&dir) {
        FactState::PendingAudit => {
            return Err("відкритий аудит-цикл — retry заблоковано".to_string())
        }
        FactState::Resolved => return Err("вузол уже resolved".to_string()),
        FactState::None => {}
    }
    for dep in crate::read_deps_dir(&dir) {
        let dep_dir = Path::new(tasks_dir).join(&dep);
        if !dep_dir.join("task.md").is_file() {
            return Err(format!("blocked-invalid-dep: {dep}"));
        }
        if accepted_fact_state(&dep_dir) != FactState::Resolved {
            return Err(format!("blocked: {dep} не resolved"));
        }
    }

    let task_fm = fs::read_to_string(dir.join("task.md"))
        .map(|c| parse_front_matter(&c))
        .unwrap_or(serde_json::Value::Null);
    let project_root = Path::new(tasks_dir)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let config = fs::read_to_string(project_root.join(".mt.json"))
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or(serde_json::Value::Null);

    let budget_sec = fm_u64(&task_fm, "budget_sec")
        .or_else(|| fm_u64(&config, "default_budget_sec"))
        .unwrap_or(1800);
    let multiplier = fm_u64(&config, "budget_hard_sec_multiplier").unwrap_or(3);
    let budget_hard_sec = fm_u64(&task_fm, "budget_hard_sec")
        .or_else(|| fm_u64(&config, "default_budget_hard_sec"))
        .unwrap_or(budget_sec * multiplier);
    if budget_hard_sec == 0 {
        return Err(
            "budget_hard_sec: 0 → validation error (hard limit не вимикається)".to_string(),
        );
    }
    let progress_timeout_sec = fm_u64(&task_fm, "progress_timeout_sec")
        .or_else(|| fm_u64(&config, "progress_timeout_sec"))
        .unwrap_or(300);

    let nnn = next_run_nnn(&dir);
    let last_fact = crate::max_nnn(&dir, "fact_", ".md");
    let attempt = nnn.saturating_sub(last_fact).max(1);

    // Істина model_tier — прапор a.md; fallback: executor.model_tier у
    // frontmatter (старі вузли) → default_model_tier із .mt.json → AVG.
    let tier_flag = read_flag_section(&dir, "## model tier").map(|v| v[0].clone());
    let executor_tier = task_fm
        .get("executor")
        .and_then(|e| e.get("model_tier"))
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    let config_tier = config
        .get("default_model_tier")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    let base_tier = normalize_model_tier(
        &tier_flag
            .or(executor_tier)
            .or(config_tier)
            .unwrap_or_else(|| "AVG".to_string()),
    );

    let ladder = read_flag_section(&dir, "## retry ladder")
        .and_then(|lines| parse_retry_ladder(&lines))
        .unwrap_or_else(default_retry_ladder);
    let step = resolve_retry_step(attempt, &ladder);
    let model_tier = bump_model_tier(&base_tier, step.model_tier_delta);
    let retry_strategy = step.strategy.clone();

    let agent_cli = read_flag_section(&dir, "## agent cli")
        .map(|v| v[0].clone())
        .unwrap_or_else(|| cli_env.agent_cli.clone())
        .to_lowercase();
    // Fail-fast до claim/worktree: невідомий CLI — помилка конфігурації.
    if !AGENT_CLIS.contains(&agent_cli.as_str()) {
        return Err(format!(
            "невідомий agent_cli \"{agent_cli}\" — підтримується: {}",
            AGENT_CLIS.join(", ")
        ));
    }

    Ok(RunPlan {
        nnn,
        attempt,
        budget_sec,
        budget_hard_sec,
        progress_timeout_sec,
        model_tier,
        retry_strategy,
        agent_cli,
    })
}

/// Останній mtime у піддереві (для progress-watchdog).
fn latest_mtime(dir: &Path) -> SystemTime {
    let mut latest = SystemTime::UNIX_EPOCH;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(m) = meta.modified() {
                    if m > latest {
                        latest = m;
                    }
                }
                if meta.is_dir() {
                    stack.push(entry.path());
                }
            }
        }
    }
    latest
}

/// Секція `## <name>` з markdown-тексту (для run-draft.md).
fn md_section(text: &str, name: &str) -> Option<String> {
    let header = format!("## {name}");
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim() == header {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with("## ") {
                break;
            }
            out.push(line);
        }
    }
    let s = out.join("\n").trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn iso_plus(sec: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(sec))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Псевдо-унікальний токен спроби без залежності `uuid` (час + pid).
fn fresh_token() -> String {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{nanos:x}-{}", std::process::id())
}

fn worktrees_dir_path(repo_root: &Path, config: &serde_json::Value) -> PathBuf {
    let raw = config
        .get("worktrees_dir")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("./.worktrees");
    let rel = raw.strip_prefix("./").unwrap_or(raw);
    if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        repo_root.join(rel)
    }
}

/// Комітить усі зміни worktree (fact/run/plan/тощо); "нема що комітити" —
/// не помилка (виконавець теоретично міг не лишити diff).
fn commit_worktree(worktree: &Path, message: &str) -> Result<(), String> {
    git(worktree, &["add", "-A"])?;
    let status = git(worktree, &["status", "--porcelain"])?;
    if status.is_empty() {
        return Ok(());
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["commit", "-q", "-m", message])
        .env("GIT_AUTHOR_NAME", "mt-runner")
        .env("GIT_AUTHOR_EMAIL", "mt-runner@localhost")
        .env("GIT_COMMITTER_NAME", "mt-runner")
        .env("GIT_COMMITTER_EMAIL", "mt-runner@localhost")
        .output()
        .map_err(|e| format!("git commit: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Результат одного спавну під watchdog-ом.
struct WatchedOutcome {
    /// budget-exceeded | progress-timeout (None — процес завершився сам).
    kill_reason: Option<&'static str>,
    exit_ok: bool,
    /// stdout + stderr разом (для rate-limit евристики).
    combined: String,
}

/// Спавнить процес і супроводжує його watchdog-ом: hard budget → SIGKILL,
/// progress-timeout за mtime `watch_dir`. stdout/stderr — у тимчасові файли
/// (щоб не блокувати pipe і не лишати слідів у worktree). Локальний
/// running-маркер у `live_dir` — observability для сканера (НЕ lock).
fn spawn_watched(
    mut cmd: Command,
    watch_dir: &Path,
    live_dir: &Path,
    budget_hard_sec: u64,
    progress_timeout_sec: u64,
) -> Result<WatchedOutcome, String> {
    let capture_base = std::env::temp_dir().join(format!("mt-run-{}", fresh_token()));
    let stdout_path = capture_base.with_extension("out");
    let stderr_path = capture_base.with_extension("err");
    let stdout_file = fs::File::create(&stdout_path).map_err(|e| e.to_string())?;
    let stderr_file = fs::File::create(&stderr_path).map_err(|e| e.to_string())?;

    let started = Instant::now();
    let started_unix = chrono::Utc::now().timestamp();
    let mut child = cmd
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("spawn виконавця: {e}"))?;

    let marker = live_dir.join(format!(
        "running_{}_until_{}",
        child.id(),
        started_unix + budget_hard_sec as i64
    ));
    let _ = fs::write(&marker, "");

    let mut kill_reason: Option<&'static str> = None;
    let mut exit_ok = false;
    let mut baseline_mtime = latest_mtime(watch_dir);
    let mut baseline_at = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                exit_ok = status.success();
                break;
            }
            None => {
                if started.elapsed().as_secs() > budget_hard_sec {
                    let _ = child.kill();
                    kill_reason = Some("budget-exceeded");
                    let _ = child.wait();
                    break;
                }
                let m = latest_mtime(watch_dir);
                if m > baseline_mtime {
                    baseline_mtime = m;
                    baseline_at = Instant::now();
                } else if baseline_at.elapsed().as_secs() > progress_timeout_sec {
                    let _ = child.kill();
                    kill_reason = Some("progress-timeout");
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
    let _ = fs::remove_file(&marker);

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    Ok(WatchedOutcome {
        kill_reason,
        exit_ok,
        combined: format!("{stdout}\n{stderr}"),
    })
}

/// Запускає виконавця вузла, супроводжує спробу до кінця і публікує результат
/// через fenced publish. **Блокуючий** — викликач (napi/CLI) сам вирішує потік.
pub fn run_node(tasks_dir: &str, node_path: &str) -> Result<RunOutcome, String> {
    run_node_env(tasks_dir, node_path, &agent_cli_env_from_process())
}

/// Як [`run_node`], але з явним конфігом виконавців (ін'єкція для тестів).
pub fn run_node_env(
    tasks_dir: &str,
    node_path: &str,
    cli_env: &AgentCliEnv,
) -> Result<RunOutcome, String> {
    let plan = preflight_env(tasks_dir, node_path, cli_env)?;

    let repo_root = discover_repo_root(Path::new(tasks_dir))?;
    let tasks_root_rel = tasks_root_relative(&repo_root, Path::new(tasks_dir))?;
    let hash = node_hash(&tasks_root_rel, node_path);

    let raw_config = fs::read_to_string(repo_root.join(".mt.json")).ok();
    let config = merge_config(raw_config.as_deref());
    let claim_lease_sec = fm_u64(&config, "claim_lease_sec").unwrap_or(3600) as i64;
    let publish_retry_max = fm_u64(&config, "publish_retry_max").unwrap_or(8) as u32;
    let publish_retry_base_ms = fm_u64(&config, "publish_retry_base_ms").unwrap_or(250);

    git(&repo_root, &["fetch", "--quiet", "origin", "main"])?;
    let base_sha = git(&repo_root, &["rev-parse", "origin/main"])?;

    let token = fresh_token();
    let runner_id = format!("mt-runner/{}", std::process::id());
    let run_ref = format!("refs/mt/runs/{hash}/{token}");
    let claimed_at = iso_now();
    let lease_until = iso_plus(claim_lease_sec);
    let fields = ClaimFields {
        node: node_path,
        actor: "agent",
        runner_id: &runner_id,
        claimed_at: &claimed_at,
        lease_until: &lease_until,
        token: &token,
        generation: 1,
        base_sha: &base_sha,
        run_ref: &run_ref,
        interactive: false,
    };
    let claim = acquire_claim(&repo_root, &hash, &fields)?;
    if !claim.accepted {
        return Err("claim-lost: інший runner уже володіє цим вузлом".to_string());
    }

    let worktrees_dir = worktrees_dir_path(&repo_root, &config);
    let worktree = create_run_worktree(&repo_root, &worktrees_dir, &hash, &token, &base_sha)?;
    push_run_ref(&worktree, &hash, &token)?;

    let wt_tasks_dir = worktree.join(&tasks_root_rel);
    let wt_tasks_dir_str = wt_tasks_dir.to_string_lossy().into_owned();
    let dir = wt_tasks_dir.join(node_path);
    let dir_str = dir.to_string_lossy().into_owned();
    let nnn_s = pad_nnn(plan.nnn);
    let live_dir = node_dir(tasks_dir, node_path)?;

    let started = Instant::now();
    let started_iso = iso_now();
    // ENV-контракт виконавця (runtime.md «Контракт команди-екзекутора»).
    let base_envs: Vec<(String, String)> = vec![
        ("MT_RUN_NNN".into(), nnn_s.clone()),
        ("MT_ATTEMPT".into(), plan.attempt.to_string()),
        ("MT_RETRY_STRATEGY".into(), plan.retry_strategy.clone()),
        ("MT_BUDGET_SEC".into(), plan.budget_sec.to_string()),
        (
            "MT_HARD_BUDGET_SEC".into(),
            plan.budget_hard_sec.to_string(),
        ),
        ("MT_STARTED_AT".into(), started_iso.clone()),
        ("MT_TASK_PATH".into(), node_path.to_string()),
        ("MT_NODE_DIR".into(), dir_str.clone()),
        (
            "MT_WORKTREE".into(),
            worktree.to_string_lossy().into_owned(),
        ),
        ("MT_RUN_TOKEN".into(), token.clone()),
        ("MT_MODEL_TIER".into(), plan.model_tier.clone()),
        ("MT_AGENT_CLI".into(), plan.agent_cli.clone()),
        ("MT_CLAIM_TOKEN".into(), token.clone()),
        ("MT_CLAIM_GENERATION".into(), "1".into()),
    ];

    // Єдиний agent-шлях — підписочний CLI з каскадом по хмарних підписках
    // за rate-limit (node_executor видалено — PR #48).
    let mut used_agent_cli: Option<String> = None;
    let watched: Option<WatchedOutcome> = {
        let prompt = build_agent_prompt(node_path, &dir, &nnn_s, plan.budget_sec);
        let mut outcome = None;
        for cli in cascade_order(&plan.agent_cli, &cli_env.cloud_agent_clis) {
            let model = resolve_model_for_cli(cli_env, &cli, &plan.model_tier);
            let Some((prog, args)) = build_agent_cli_argv(&cli, model.as_deref(), &prompt) else {
                continue; // невідоме ім'я у каскаді — пропускаємо
            };
            let mut cmd = Command::new(prog);
            cmd.args(args).current_dir(&dir);
            cmd.envs(base_envs.iter().cloned());
            cmd.env("MT_AGENT_CLI", &cli);
            let w = spawn_watched(
                cmd,
                &dir,
                &live_dir,
                plan.budget_hard_sec,
                plan.progress_timeout_sec,
            )?;
            // Watchdog-kill — термінальний; rate-limit → наступний кандидат.
            if w.kill_reason.is_some() || !is_rate_limited(w.exit_ok, &w.combined) {
                used_agent_cli = Some(cli);
                outcome = Some(w);
                break;
            }
        }
        outcome // None — усі CLI каскаду вичерпали ліміти підписки
    };

    let wall_sec = started.elapsed().as_secs();
    let cli_fm = used_agent_cli
        .as_ref()
        .map(|c| format!("agent_cli: {c}\n"))
        .unwrap_or_default();
    let extra_fm = format!("{cli_fm}wall_sec: {wall_sec}\n");

    let fact_file = format!("fact_{nnn_s}.md");
    let kill_reason = watched.as_ref().and_then(|w| w.kill_reason);

    let has_fact = dir.join(&fact_file).is_file();
    let (result, run_file, out_fact_file, propagated) = if kill_reason.is_none() && has_fact {
        let policy_required = fs::read_to_string(dir.join("task.md"))
            .map(|c| parse_front_matter(&c))
            .ok()
            .and_then(|fm| {
                fm.get("audit")
                    .and_then(serde_json::Value::as_str)
                    .map(|s| s == "required")
            })
            .unwrap_or(false);
        let signaled = if policy_required {
            signal::audit_fm(&wt_tasks_dir_str, node_path, "agent", &extra_fm)
        } else {
            signal::done_fm(&wt_tasks_dir_str, node_path, "agent", &extra_fm)
        };
        match signaled {
            Ok(out) => (
                "success".to_string(),
                out.run_file,
                Some(out.fact_file),
                out.propagated,
            ),
            Err(check_err) => {
                // Fact без пройденого ## Check не публікується — інакше вузол
                // хибно стане resolved (accepted_fact_state рахує лише файли).
                let _ = fs::remove_file(dir.join(&fact_file));
                let sections = format!(
                    "\n## Completed\n\nfact записано, але ## Check не пройшов (fact відкликано)\n\n## Blockers\n\n{check_err}\n\n## Next Attempt\n\nвиправити і повторити done\n"
                );
                let run_file = write_run_fm(&dir, &nnn_s, "agent", "failed", &sections, &extra_fm)?;
                ("failed".to_string(), run_file, None, Vec::new())
            }
        }
    } else {
        let draft = fs::read_to_string(dir.join("run-draft.md")).unwrap_or_default();
        let result = kill_reason.unwrap_or("failed").to_string();
        let default_blockers = if watched.is_none() {
            "усі CLI каскаду вичерпали ліміти підписки".to_string()
        } else {
            format!("процес завершився без fact ({result})")
        };
        let completed =
            md_section(&draft, "Completed").unwrap_or_else(|| "невідомо (draft відсутній)".into());
        let blockers = md_section(&draft, "Blockers").unwrap_or(default_blockers);
        let next = md_section(&draft, "Next Attempt")
            .unwrap_or_else(|| "діагностувати попередній ран".into());
        // Діагностика провалу не губиться: хвіст виводу виконавця (який уже
        // читається для rate-limit-детекту) — у run-файл (тертя M0 №5).
        let output_tail = watched
            .as_ref()
            .map(|w| {
                let tail: Vec<&str> = w.combined.trim().lines().rev().take(15).collect();
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            })
            .filter(|t| !t.is_empty())
            .map(|t| format!("\n## Executor output tail\n\n```text\n{t}\n```\n"))
            .unwrap_or_default();
        let sections = format!(
            "\n## Completed\n\n{completed}\n\n## Blockers\n\n{blockers}\n\n## Next Attempt\n\n{next}\n{output_tail}"
        );
        let run_file = write_run_fm(&dir, &nnn_s, "agent", &result, &sections, &extra_fm)?;
        (result, run_file, None, Vec::new())
    };

    commit_worktree(
        &worktree,
        &format!("mt: {node_path} run {nnn_s} ({result})"),
    )?;

    let publish_req = PublishRequest {
        worktree: &worktree,
        node_hash: &hash,
        claim_sha: &claim.commit_sha,
        token: &token,
        run_ref_sha_before: &base_sha,
    };
    let publish = fenced_publish(
        &repo_root,
        &publish_req,
        publish_retry_max,
        publish_retry_base_ms,
    )?;

    if !publish.published {
        // Worktree/run ref лишаються для debug (спека, «Failure-сімейство» /
        // «Orphan worktree») — не видаляємо, наступний runner чи людина
        // розбереться. Claim теж не чіпаємо: якщо fenced — він уже не наш.
        return Err(if publish.fenced {
            "claim-lost: втрачено ownership під час виконання, publish скасовано".to_string()
        } else {
            "publish: вичерпано retry — конкурентний publish виграв гонку, спробуйте пізніше"
                .to_string()
        });
    }

    // Успішна публікація — worktree більше не потрібен.
    let _ = remove_run_worktree(&repo_root, &worktree);

    Ok(RunOutcome {
        result,
        run_file,
        fact_file: out_fact_file,
        wall_sec,
        agent_cli: used_agent_cli,
        propagated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    const TASK: &str = "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\nbudget_sec: 5\nbudget_hard_sec: 2\nprogress_timeout_sec: 60\n---\n\n## Task\n\nx\n";

    /// Пише task.md/a.md на диск, без git — для тестів `preflight()`
    /// (суто файлова логіка, git-репо не потрібне).
    fn node_files_only(tmp: &Path, path: &str) {
        let dir = tmp.join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        fs::write(dir.join("a.md"), "schema_version: 1\n").unwrap();
    }

    /// Як [`node_files_only`], але комітить і пушить у `origin/main` —
    /// потрібно для `run_node()`: worktree чекаутиться саме з `origin/main`.
    fn node(tmp: &Path, path: &str) {
        node_files_only(tmp, path);
        crate::test_support::run(tmp, &["add", "."]);
        crate::test_support::run(tmp, &["commit", "-q", "-m", &format!("add {path}")]);
        crate::test_support::run(tmp, &["push", "-q", "origin", "main"]);
    }

    /// Тіло фейкового `claude`, що пише валідний fact поточної спроби
    /// (cwd шима — директорія вузла у worktree, NNN — з env).
    const FAKE_CLI_WRITES_FACT: &str = r#"printf -- '---\nschema_version: 1\n---\n\n## Summary\n\nok\n' > "fact_${MT_RUN_NNN}.md""#;

    fn env_default() -> AgentCliEnv {
        AgentCliEnv::default()
    }

    #[test]
    fn preflight_blocks_unresolved_deps_and_running() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "a");
        node_files_only(&root, "b");
        fs::create_dir_all(root.join("b/deps")).unwrap();
        fs::write(root.join("b/deps/a.md"), "").unwrap();
        let r = root.to_string_lossy().into_owned();

        assert!(preflight_env(&r, "b", &env_default())
            .unwrap_err()
            .contains("blocked: a"));
        fs::write(root.join("a/running_1_until_9999999999"), "").unwrap();
        assert!(preflight_env(&r, "a", &env_default())
            .unwrap_err()
            .contains("running"));
    }

    #[test]
    fn preflight_resolves_executor_flags_and_ladder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(
            root.join("solo/a.md"),
            "## Model tier\n\nAVG\n\n## Agent cli\n\ncursor\n",
        )
        .unwrap();
        let r = root.to_string_lossy().into_owned();

        // attempt=1 — базовий щабель.
        let plan = preflight_env(&r, "solo", &env_default()).unwrap();
        assert_eq!(plan.agent_cli, "cursor");
        assert_eq!(plan.model_tier, "AVG");
        assert_eq!(plan.retry_strategy, "base");

        // failed_streak=2 → attempt=3 → alternative-approach ескалює AVG→MAX.
        fs::write(root.join("solo/run_001.md"), "---\nresult: failed\n---\n").unwrap();
        fs::write(root.join("solo/run_002.md"), "---\nresult: failed\n---\n").unwrap();
        let plan = preflight_env(&r, "solo", &env_default()).unwrap();
        assert_eq!(plan.attempt, 3);
        assert_eq!(plan.retry_strategy, "alternative-approach");
        assert_eq!(plan.model_tier, "MAX");
    }

    #[test]
    fn preflight_short_ladder_repeats_last_step() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(
            root.join("solo/a.md"),
            "## Model tier\n\nAVG\n\n## Retry ladder\n\n- base\n- diagnose-first\n",
        )
        .unwrap();
        fs::write(root.join("solo/run_001.md"), "---\nresult: failed\n---\n").unwrap();
        fs::write(root.join("solo/run_002.md"), "---\nresult: failed\n---\n").unwrap();
        let r = root.to_string_lossy().into_owned();

        let plan = preflight_env(&r, "solo", &env_default()).unwrap();
        assert_eq!(plan.attempt, 3);
        // Коротша драбина — останній щабель повторюється, без ескалації тиру.
        assert_eq!(plan.retry_strategy, "diagnose-first");
        assert_eq!(plan.model_tier, "AVG");
    }

    #[test]
    fn preflight_rejects_unknown_agent_cli_fail_fast() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        let r = root.to_string_lossy().into_owned();
        let cli_env = AgentCliEnv {
            agent_cli: "gemini".to_string(),
            ..AgentCliEnv::default()
        };
        let err = preflight_env(&r, "solo", &cli_env).unwrap_err();
        assert!(err.contains("невідомий agent_cli \"gemini\""));
    }

    #[test]
    fn agent_cli_argv_per_cli_with_and_without_model() {
        let (cmd, args) = build_agent_cli_argv("codex", None, "p").unwrap();
        assert_eq!(cmd, "codex");
        assert_eq!(
            args,
            ["exec", "--sandbox", "workspace-write", "--ephemeral", "p"]
        );
        let (cmd, args) = build_agent_cli_argv("codex", Some("gpt-5.6-terra"), "p").unwrap();
        assert_eq!(cmd, "codex");
        assert_eq!(
            args,
            [
                "exec",
                "-m",
                "gpt-5.6-terra",
                "--sandbox",
                "workspace-write",
                "--ephemeral",
                "p"
            ]
        );
        let (cmd, args) = build_agent_cli_argv("cursor", None, "p").unwrap();
        assert_eq!(cmd, "cursor-agent");
        assert_eq!(args, ["--print", "--force", "p"]);
        let (cmd, args) = build_agent_cli_argv("claude", Some("opus"), "p").unwrap();
        assert_eq!(cmd, "claude");
        assert_eq!(
            args,
            ["--model", "opus", "--no-session-persistence", "-p", "p"]
        );
        let (cmd, args) = build_agent_cli_argv("pi", None, "p").unwrap();
        assert_eq!(cmd, "pi");
        assert_eq!(args, ["--no-session", "-p", "p"]);
        assert!(build_agent_cli_argv("gemini", None, "p").is_none());
    }

    #[test]
    fn rate_limit_heuristic_and_cascade_order() {
        assert!(is_rate_limited(false, "Rate limit exceeded"));
        assert!(is_rate_limited(false, "usage limit reached for your plan"));
        assert!(is_rate_limited(false, "HTTP 429 Too Many Requests"));
        assert!(is_rate_limited(false, "quota exceeded"));
        // Успішний exit або не-лімітна помилка каскад не запускають.
        assert!(!is_rate_limited(true, "rate limit"));
        assert!(!is_rate_limited(false, "syntax error in generated patch"));
        assert!(!is_rate_limited(false, "id 14290 not found"));

        let cloud = vec!["codex".to_string(), "cursor".to_string()];
        assert_eq!(cascade_order("codex", &cloud), ["codex", "cursor"]);
        assert_eq!(
            cascade_order("claude", &cloud),
            ["claude", "codex", "cursor"]
        );
    }

    #[test]
    fn run_success_publishes_fact_to_origin_main() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");
        let r = root.to_string_lossy().into_owned();
        with_path_shims(&[("claude", FAKE_CLI_WRITES_FACT)], || {
            let out = run_node_env(&r, "solo", &env_default()).unwrap();
            assert_eq!(out.result, "success");
            assert_eq!(out.fact_file.as_deref(), Some("fact_001.md"));
            assert_eq!(out.agent_cli.as_deref(), Some("claude"));
        });
        assert!(!crate::has_running_marker(&root.join("solo")));

        // Опубліковано в origin/main: claim/run ref прибрані, коміт на remote.
        let claims = crate::test_support::output(
            repo.work.path(),
            &["ls-remote", "origin", "refs/mt/claims/*"],
        );
        assert!(claims.is_empty());
        // Локальний main (той самий work-клон) підхопив публікацію.
        assert!(root.join("solo/fact_001.md").is_file());
        let run = fs::read_to_string(root.join("solo/run_001.md")).unwrap();
        assert!(run.contains("result: success"));
        assert!(run.contains("agent_cli: claude"));
    }

    #[test]
    fn hard_budget_kills_and_publishes_failure_run() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "slow");
        let r = root.to_string_lossy().into_owned();
        let mut out = None;
        with_path_shims(&[("claude", "sleep 30")], || {
            out = Some(run_node_env(&r, "slow", &env_default()).unwrap());
        });
        assert_eq!(out.unwrap().result, "budget-exceeded");
        let run = fs::read_to_string(root.join("slow/run_001.md")).unwrap();
        assert!(run.contains("result: budget-exceeded"));
        assert!(run.contains("wall_sec:"));
        assert!(!root.join("slow/fact_001.md").exists());
    }

    #[test]
    fn failure_takes_sections_from_draft_and_publishes() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "fail");
        let r = root.to_string_lossy().into_owned();
        let draft_cli = r#"printf -- '## Completed\n\nполовина\n\n## Blockers\n\nнемає доступу\n\n## Next Attempt\n\nдати ключ\n' > run-draft.md; exit 1"#;
        let mut result = String::new();
        with_path_shims(&[("claude", draft_cli)], || {
            result = run_node_env(&r, "fail", &env_default()).unwrap().result;
        });
        assert_eq!(result, "failed");
        let run = fs::read_to_string(root.join("fail/run_001.md")).unwrap();
        assert!(run.contains("немає доступу"));
        assert!(run.contains("дати ключ"));
    }

    #[test]
    fn failed_check_revokes_fact_and_publishes_failed_run() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        let dir = root.join("gated");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("task.md"),
            "---\nschema_version: 1\nbudget_sec: 5\nbudget_hard_sec: 2\n---\n\n## Task\n\nx\n\n## Check\n\nfalse\n",
        )
        .unwrap();
        fs::write(dir.join("a.md"), "schema_version: 1\n").unwrap();
        crate::test_support::run(repo.work.path(), &["add", "."]);
        crate::test_support::run(repo.work.path(), &["commit", "-q", "-m", "add gated"]);
        crate::test_support::run(repo.work.path(), &["push", "-q", "origin", "main"]);

        let r = root.to_string_lossy().into_owned();
        let mut result = String::new();
        with_path_shims(&[("claude", FAKE_CLI_WRITES_FACT)], || {
            result = run_node_env(&r, "gated", &env_default()).unwrap().result;
        });
        assert_eq!(result, "failed");
        // Fact відкликано — вузол не стає хибно resolved.
        assert!(!root.join("gated/fact_001.md").exists());
        let run = fs::read_to_string(root.join("gated/run_001.md")).unwrap();
        assert!(run.contains("result: failed"));
        assert!(run.contains("## Check"));
    }

    #[test]
    fn rejected_claim_when_node_already_claimed() {
        // Claim відхиляється ДО спавну виконавця — фейковий CLI не потрібен.
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");
        let r = root.to_string_lossy().into_owned();

        let hash = node_hash("mt", "solo");
        let base = repo.main_sha();
        let fields = ClaimFields {
            node: "solo",
            actor: "agent",
            runner_id: "other/1",
            claimed_at: &iso_now(),
            lease_until: &iso_plus(3600),
            token: "already-there",
            generation: 1,
            base_sha: &base,
            run_ref: "refs/mt/runs/x/already-there",
            interactive: false,
        };
        acquire_claim(repo.work.path(), &hash, &fields).unwrap();

        let err = run_node_env(&r, "solo", &env_default()).unwrap_err();
        assert!(err.contains("claim-lost"));
    }

    /// Каскадні тести спавнять фейкові CLI через PATH-шими — серіалізуємо
    /// мутацію PATH процесу.
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Тимчасовий bin-каталог із фейковими CLI, prepended до PATH.
    fn with_path_shims(shims: &[(&str, &str)], f: impl FnOnce()) {
        let _guard = PATH_LOCK.lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        for (name, body) in shims {
            let p = bin.path().join(name);
            fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{orig}", bin.path().display()));
        f();
        std::env::set_var("PATH", orig);
    }

    #[test]
    fn cascade_falls_over_to_next_cloud_cli_on_rate_limit() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");
        let r = root.to_string_lossy().into_owned();
        let cli_env = AgentCliEnv {
            agent_cli: "codex".to_string(),
            cloud_agent_clis: vec!["codex".to_string(), "cursor".to_string()],
            ..AgentCliEnv::default()
        };
        with_path_shims(
            &[
                (
                    "codex",
                    "echo 'Rate limit exceeded, try again later' >&2; exit 1",
                ),
                (
                    "cursor-agent",
                    r#"printf -- '---\nschema_version: 1\n---\n\n## Summary\n\nok\n' > "fact_${MT_RUN_NNN}.md""#,
                ),
            ],
            || {
                let out = run_node_env(&r, "solo", &cli_env).unwrap();
                assert_eq!(out.result, "success");
                assert_eq!(out.agent_cli.as_deref(), Some("cursor"));
            },
        );
        let run = fs::read_to_string(root.join("solo/run_001.md")).unwrap();
        assert!(run.contains("agent_cli: cursor"));
        assert!(run.contains("result: success"));
    }

    #[test]
    fn cascade_exhausted_or_plain_error_paths() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");
        let r = root.to_string_lossy().into_owned();

        // Усі кандидати rate-limited → failed без fact.
        let cli_env = AgentCliEnv {
            agent_cli: "codex".to_string(),
            cloud_agent_clis: vec!["cursor".to_string()],
            ..AgentCliEnv::default()
        };
        with_path_shims(
            &[
                ("codex", "echo 'usage limit reached for your plan'; exit 1"),
                ("cursor-agent", "echo 'quota exceeded'; exit 1"),
            ],
            || {
                let out = run_node_env(&r, "solo", &cli_env).unwrap();
                assert_eq!(out.result, "failed");
                assert!(out.agent_cli.is_none());
            },
        );
        let run = fs::read_to_string(root.join("solo/run_001.md")).unwrap();
        assert!(run.contains("вичерпали ліміти підписки"));

        // Не-лімітна помилка НЕ каскадує: перший кандидат фіксується як
        // фактичний CLI, наступний не викликається.
        let marker = repo.work.path().join("cursor-called");
        let marker_cmd = format!("touch {}", marker.display());
        with_path_shims(
            &[
                ("codex", "echo 'syntax error in generated patch'; exit 1"),
                ("cursor-agent", marker_cmd.as_str()),
            ],
            || {
                let out = run_node_env(&r, "solo", &cli_env).unwrap();
                assert_eq!(out.result, "failed");
                assert_eq!(out.agent_cli.as_deref(), Some("codex"));
                assert!(!marker.exists());
            },
        );
    }
}
