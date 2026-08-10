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
    acquire_claim, discover_main_worktree_root, discover_repo_root, node_hash, tasks_root_relative,
    ClaimFields,
};
use crate::config::{
    agent_cli_env_from_process, merge_config, normalize_model_tier, resolve_model_for_cli,
    AgentCliEnv,
};
use crate::frontmatter::parse_front_matter;
use crate::git::GitRepository;
use crate::nnn::pad_nnn;
use crate::publish::{fenced_publish, publish_failure_run, PublishOutcome, PublishRequest};
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

/// Frontmatter прапора виконавця `a.md` (graph.md §a.md). Немає файлу → None.
///
/// Старий markdown-секційний формат («## Model tier») відхиляється **явно**:
/// мовчазне читання як порожнього frontmatter підмінило б `model_tier` і
/// `agent_cli` дефолтами — вузол тихо виконався б не тим виконавцем.
pub(crate) fn read_executor_flag(dir: &Path) -> Result<Option<serde_json::Value>, String> {
    let path = dir.join("a.md");
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    if !content.trim_start().starts_with("---") {
        let hint = if content.contains("## ") {
            "markdown-секційний формат («## Model tier») більше не підтримується"
        } else {
            "файл без YAML-frontmatter"
        };
        return Err(format!(
            "{}: {hint} — очікується `---`-frontmatter із ключами \
             model_tier/skills/agent_cli (graph.md §a.md)",
            path.display()
        ));
    }
    let fm = crate::frontmatter::parse_front_matter(&content);
    crate::frontmatter::check_schema_version(&fm, &path)?;
    Ok(Some(fm))
}

fn flag_str(flag: Option<&serde_json::Value>, key: &str) -> Option<String> {
    flag?
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// `retry_ladder` зі спеки — список щаблів: рядок (`- diagnose-first`) або
/// мапа (`- {strategy: …, model_tier_delta: …}`). Щабель
/// "alternative-approach" несе `model_tier_delta: 1` за замовчуванням (graph.md).
fn parse_retry_ladder(value: &serde_json::Value) -> Option<Vec<LadderStep>> {
    let steps: Vec<LadderStep> = value
        .as_array()?
        .iter()
        .filter_map(|item| {
            let strategy = match item {
                serde_json::Value::String(s) => s.trim().to_lowercase(),
                serde_json::Value::Object(_) => item
                    .get("strategy")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("base")
                    .trim()
                    .to_lowercase(),
                _ => return None,
            };
            if strategy.is_empty() {
                return None;
            }
            let delta = item
                .get("model_tier_delta")
                .and_then(serde_json::Value::as_u64)
                .map_or(usize::from(strategy == "alternative-approach"), |d| {
                    d as usize
                });
            Some(LadderStep {
                model_tier_delta: delta,
                strategy,
            })
        })
        .collect();
    (!steps.is_empty()).then_some(steps)
}

/// Чи підсумок публікації є **термінальним конфліктом**, тобто спробою, яку
/// треба записати як `result: merge-conflict` (graph.md — execution failure,
/// +1 до `failed_streak`).
///
/// Два тригери (git.md): конфлікт rebase і вичерпані publish-retry. Втрата
/// claim (`fenced`) сюди НЕ входить — це `claim-lost`, lifecycle-категорія,
/// яка лічильник не рухає: роботу забрав інший runner, а не вона провалилась.
fn terminal_conflict_reason(publish: &Result<PublishOutcome, String>) -> Option<String> {
    match publish {
        Err(error) if error.contains("rebase conflict") => Some(error.clone()),
        Ok(outcome) if !outcome.published && !outcome.fenced => {
            Some("вичерпано publish retry — конкурентний publish виграв гонку".to_string())
        }
        _ => None,
    }
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
/// Тіло markdown-файлу без frontmatter.
fn file_body(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let body = crate::frontmatter::get_body(&content);
    let body = if body.trim().is_empty() {
        content
    } else {
        body
    };
    let trimmed = body.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Найбільший NNN серед файлів `<prefix>NNN.md` разом із тілом.
fn latest_artifact_body(dir: &Path, prefix: &str) -> Option<String> {
    let nnn = crate::max_nnn(dir, prefix, ".md");
    (nnn > 0).then(|| file_body(&dir.join(format!("{prefix}{nnn:03}.md"))))?
}

/// Резюме прийнятих fact-ів залежностей — те, заради чого вузол чекав на
/// deps (graph.md, блок `[deps/]`). Порожній dep-файл — лише ребро; зміст
/// живе у fact-і залежності, тож інлайниться саме він.
fn dep_facts(tasks_root: &Path, node_dir: &Path) -> Vec<String> {
    crate::read_deps_dir(node_dir)
        .into_iter()
        .filter_map(|dep_id| {
            let dep_dir = tasks_root.join(&dep_id);
            let nnn = crate::accepted_fact_nnn(&dep_dir);
            let body = file_body(&dep_dir.join(format!("fact_{nnn:03}.md")))?;
            Some(format!("### {dep_id}\n\n{body}"))
        })
        .collect()
}

/// Перший шар стискання невдач (graph.md, «Prior attempts резюме»): із
/// кожного failure-рану після останнього прийнятого fact беруться лише
/// Completed/Blockers/Next Attempt — сирі run-файли в промпт не йдуть.
fn prior_attempts(node_dir: &Path) -> Vec<String> {
    let since = crate::accepted_fact_nnn(node_dir);
    let latest = crate::max_nnn(node_dir, "run_", ".md");
    (since + 1..=latest)
        .filter_map(|nnn| {
            let path = node_dir.join(format!("run_{nnn:03}.md"));
            let content = fs::read_to_string(&path).ok()?;
            let sections: Vec<String> = ["Completed", "Blockers", "Next Attempt"]
                .into_iter()
                .filter_map(|name| {
                    md_section(&content, name).map(|body| format!("- **{name}:** {body}"))
                })
                .collect();
            (!sections.is_empty())
                .then(|| format!("### Спроба {nnn:03}\n\n{}", sections.join("\n")))
        })
        .collect()
}

/// Один виклик виконавця в директорії вузла — без claim, worktree й
/// watchdog-політик вузла. Для акторів, що не виконують задачу, а лише
/// читають її й пишуть один артефакт (аудитор).
///
/// Повертає CLI, який спрацював, або `None`, якщо весь каскад упоровся в
/// ліміти підписки.
pub fn run_single_phase(
    dir: &Path,
    cli_env: &AgentCliEnv,
    model_tier: &str,
    prompt: &str,
) -> Result<Option<String>, String> {
    for cli in cascade_order(&cli_env.agent_cli, &cli_env.cloud_agent_clis) {
        let model = resolve_model_for_cli(cli_env, &cli, model_tier);
        let Some((prog, args)) = build_agent_cli_argv(&cli, model.as_deref(), prompt) else {
            continue;
        };
        let out = Command::new(prog)
            .args(args)
            .current_dir(dir)
            .env("MT_MODEL_TIER", model_tier)
            .env("MT_AGENT_CLI", &cli)
            .output()
            .map_err(|e| format!("{cli}: {e}"))?;
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !is_rate_limited(out.status.success(), &combined) {
            return Ok(Some(cli));
        }
    }
    Ok(None)
}

/// Хто виконує run (graph.md, `actor:` у `run_NNN.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    /// Звичайний виконавець вузла.
    Agent,
    /// Ремонтник графа — вмикається, коли драбина агента вичерпана.
    Engineer,
}

impl Actor {
    fn as_str(self) -> &'static str {
        match self {
            Actor::Agent => "agent",
            Actor::Engineer => "engineer",
        }
    }
}

/// Сирий run-history вузла після останнього прийнятого fact — **без**
/// стискання, на відміну від контексту агента.
///
/// Інженеру потрібні саме подробиці: він шукає причину в тому, що агент
/// відкинув як шум (хвіст виводу виконавця, точні повідомлення про помилки),
/// тож перший шар стискання тут був би втратою доказів.
fn full_run_history(node_dir: &Path) -> Vec<String> {
    let since = crate::accepted_fact_nnn(node_dir);
    let latest = crate::max_nnn(node_dir, "run_", ".md");
    (since + 1..=latest)
        .filter_map(|nnn| {
            let body = file_body(&node_dir.join(format!("run_{nnn:03}.md")))?;
            Some(format!("### run_{nnn:03}.md\n\n{body}"))
        })
        .collect()
}

/// Контекст EngineerAgent (graph.md): task + deps + **повний** run-history +
/// `.mt/engineer-prompt.md`.
///
/// Інженер не виконує задачу — він лагодить граф, тому промпт закінчується
/// не вимогою fact-у, а переліком дозволених втручань.
fn build_engineer_prompt(
    task_path: &str,
    node_dir: &Path,
    tasks_root: &Path,
    engineer_prompt: Option<&Path>,
    nnn: &str,
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    if let Some(body) = engineer_prompt.and_then(file_body) {
        blocks.push(format!("## Протокол інженера\n\n{body}"));
    }
    blocks.push(format!(
        "## Ремонт вузла: {task_path}\n\nРобоча директорія: {}\nRun NNN: {nnn}\n\n\
         Драбина ретраїв вичерпана — звичайний виконавець уже не дає результату. \
         Твоя робота не «спробувати ще раз тим самим способом», а знайти **причину** \
         і полагодити граф.",
        node_dir.display()
    ));
    if let Some(body) = file_body(&node_dir.join("task.md")) {
        blocks.push(format!("## task.md\n\n{body}"));
    }
    let deps = dep_facts(tasks_root, node_dir);
    if !deps.is_empty() {
        blocks.push(format!(
            "## Результати залежностей\n\n{}",
            deps.join("\n\n")
        ));
    }
    let history = full_run_history(node_dir);
    if !history.is_empty() {
        blocks.push(format!(
            "## Повна історія спроб\n\n{}",
            history.join("\n\n")
        ));
    }
    if let Some(body) = latest_artifact_body(node_dir, "audit-result_") {
        blocks.push(format!("## Вердикт аудиту\n\n{body}"));
    }
    blocks.push(
        "## Дозволені втручання\n\n\
         - `mt invalidate <вузол>` — скинути version chain і дати вузлу чистий старт \
           (за потреби спершу виправивши `task.md`);\n\
         - `mt kill <вузол>` — прибрати помилково створене піддерево;\n\
         - правка `task.md` вузла, якщо контракт сформульовано так, що його неможливо виконати;\n\
         - правка `a.md` — інший `model_tier`, інший `agent_cli`, інші `skills`.\n\n\
         Якщо причину усунуто і вузол готовий до звичайного виконання — опиши це \
         в `run-draft.md` секціями `## Completed` / `## Blockers` / `## Next Attempt`. \
         Якщо ти сам довів задачу до результату — напиши `fact_" .to_string()
            + nnn
            + ".md` з `## Summary`.",
    );
    blocks.join("\n\n")
}

/// `decision:` актуального `plan_NNN.md` — atomic | composite.
fn latest_plan_decision(dir: &Path) -> Option<String> {
    let nnn = crate::max_nnn(dir, "plan_", ".md");
    let content = fs::read_to_string(dir.join(format!("plan_{nnn:03}.md"))).ok()?;
    crate::frontmatter::parse_front_matter(&content)
        .get("decision")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.trim().to_lowercase())
}

/// Чи потрібен Етап 1 (graph.md, «Два етапи виконання»).
///
/// Пропускаємо там, де рішення atomic/composite вже ухвалене: людський
/// `hint: atomic` у `task.md` або будь-який наявний `plan_NNN.md` (зокрема
/// від явного `mt plan`). Інакше кожна спроба платила б зайвим викликом
/// моделі за рішення, яке вже прийнято.
fn needs_planning(dir: &Path, task_fm: &serde_json::Value) -> bool {
    let hint = task_fm
        .get("hint")
        .and_then(serde_json::Value::as_str)
        .map(str::trim);
    hint != Some("atomic") && crate::max_nnn(dir, "plan_", ".md") == 0
}

/// Промпт Етапу 1: вимагає рішення atomic/composite у `plan_NNN.md`.
fn build_plan_prompt(
    task_path: &str,
    node_dir: &Path,
    tasks_root: &Path,
    system_prompt: Option<&Path>,
    plan_nnn: u64,
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    if let Some(body) = system_prompt.and_then(file_body) {
        blocks.push(format!("## Протокол виконання\n\n{body}"));
    }
    blocks.push(format!(
        "## Етап 1 — планування: {task_path}\n\nРобоча директорія: {}",
        node_dir.display()
    ));
    if let Some(body) = file_body(&node_dir.join("task.md")) {
        blocks.push(format!("## task.md\n\n{body}"));
    }
    let deps = dep_facts(tasks_root, node_dir);
    if !deps.is_empty() {
        blocks.push(format!(
            "## Результати залежностей\n\n{}",
            deps.join("\n\n")
        ));
    }
    blocks.push(format!(
        "## Що зробити\n\nВиріши, чи задача **атомарна** (виконується одним прогоном), \
         чи **складена** (розкладається на підзадачі), і створи файл `plan_{plan_nnn:03}.md`:\n\n\
         ```markdown\n---\nschema_version: 1\ndecision: atomic\n---\n\n## Context\n\n<з чого виходиш>\n\n\
         ## Approach\n\n<як робитимеш>\n```\n\n\
         Якщо задача складена — `decision: composite` і секція `## Children` \
         зі списком підзадач:\n\n\
         ```markdown\n## Children\n\n```yaml\nchildren:\n  - id: collect-data\n    mode: agent\n    \
         task: |\n      Що саме зробити\n  - id: analyze\n    mode: agent\n    deps: [collect-data]\n    \
         task: Перевірити зібране\n```\n```\n\n\
         `mode` обов'язковий для кожної дитини (`agent` або `human`). \
         **Нічого більше на цьому етапі не роби** — саме виконання буде наступною фазою."
    ));
    blocks.join("\n\n")
}

/// Контекст агента за формулою graph.md:
/// `[task.md] + [a.md|h.md] + [deps/] + [plan_*.md] + [Prior attempts] +
/// [run-summary.md] + [audit-result_*.md]`, поверх протоколу поведінки
/// (`.mt/system-prompt.md`).
///
/// Кожен блок опційний: відсутній файл просто прибирає секцію, щоб короткий
/// вузол не отримував порожніх заголовків.
fn build_agent_prompt(
    task_path: &str,
    node_dir: &Path,
    tasks_root: &Path,
    system_prompt: Option<&Path>,
    nnn: &str,
    budget_sec: u64,
) -> String {
    let mut blocks: Vec<String> = Vec::new();

    if let Some(body) = system_prompt.and_then(file_body) {
        blocks.push(format!("## Протокол виконання\n\n{body}"));
    }
    blocks.push(format!(
        "## Задача: {task_path}\n\nРобоча директорія: {}\nRun NNN: {nnn}\nБюджет: {budget_sec}s",
        node_dir.display()
    ));
    if let Some(body) = file_body(&node_dir.join("task.md")) {
        blocks.push(format!("## task.md\n\n{body}"));
    }
    for flag in ["a.md", "h.md"] {
        if let Some(body) = fs::read_to_string(node_dir.join(flag))
            .ok()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
        {
            blocks.push(format!("## Прапор виконавця ({flag})\n\n```yaml\n{body}\n```"));
        }
    }
    let deps = dep_facts(tasks_root, node_dir);
    if !deps.is_empty() {
        blocks.push(format!(
            "## Результати залежностей\n\n{}",
            deps.join("\n\n")
        ));
    }
    if let Some(body) = latest_artifact_body(node_dir, "plan_") {
        blocks.push(format!("## Актуальний план\n\n{body}"));
    }
    // Другий шар стискання: якщо є LLM-резюме, сирі спроби не дублюються.
    if let Some(body) = file_body(&node_dir.join("run-summary.md")) {
        blocks.push(format!("## Резюме попередніх спроб\n\n{body}"));
    } else {
        let attempts = prior_attempts(node_dir);
        if !attempts.is_empty() {
            blocks.push(format!(
                "## Попередні спроби (не повторюй ці помилки)\n\n{}",
                attempts.join("\n\n")
            ));
        }
    }
    if let Some(body) = latest_artifact_body(node_dir, "audit-result_") {
        blocks.push(format!("## Вердикт аудиту\n\n{body}"));
    }
    if let Some(body) = latest_artifact_body(node_dir, "clarification_") {
        blocks.push(format!("## Запит уточнення від аудитора\n\n{body}"));
    }

    blocks.push(format!(
        "## Обов'язковий фінальний крок\n\nСтвори файл `fact_{nnn}.md` у робочій директорії. \
         Без нього run вважається ПРОВАЛЕНИМ, навіть якщо решту зроблено. Приклад:\n\n\
         ```markdown\n## Summary\n\n<одне речення про результат>\n```"
    ));

    blocks.join("\n\n")
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
    // Fail closed до claim: файл із чужою схемою не виконуємо (graph.md).
    crate::frontmatter::check_schema_version_of_file(&dir.join("task.md"))?;
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
    let flag = read_executor_flag(&dir)?;
    let tier_flag = flag_str(flag.as_ref(), "model_tier");
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

    let ladder = flag
        .as_ref()
        .and_then(|f| f.get("retry_ladder"))
        .and_then(parse_retry_ladder)
        .unwrap_or_else(default_retry_ladder);
    let step = resolve_retry_step(attempt, &ladder);
    let model_tier = bump_model_tier(&base_tier, step.model_tier_delta);
    let retry_strategy = step.strategy.clone();

    let agent_cli = flag_str(flag.as_ref(), "agent_cli")
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

pub(crate) fn worktrees_dir_path(repo_root: &Path, config: &serde_json::Value) -> PathBuf {
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
    crate::git::GitRepository::open(worktree)
        .and_then(|repository| {
            repository.commit_all_if_changed(message, crate::git::SignaturePolicy::Runner)
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
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

/// [`run_node`] із явним актором — `mt run --actor engineer` (graph.md).
pub fn run_node_as(tasks_dir: &str, node_path: &str, actor: Actor) -> Result<RunOutcome, String> {
    run_node_env_as(
        tasks_dir,
        node_path,
        &agent_cli_env_from_process(),
        actor,
    )
}

/// Як [`run_node`], але з явним конфігом виконавців (ін'єкція для тестів).
pub fn run_node_env(
    tasks_dir: &str,
    node_path: &str,
    cli_env: &AgentCliEnv,
) -> Result<RunOutcome, String> {
    run_node_env_as(tasks_dir, node_path, cli_env, Actor::Agent)
}

/// [`run_node_env`] із явним актором.
pub fn run_node_env_as(
    tasks_dir: &str,
    node_path: &str,
    cli_env: &AgentCliEnv,
    actor: Actor,
) -> Result<RunOutcome, String> {
    let plan = preflight_env(tasks_dir, node_path, cli_env)?;
    // Інженер вмикається лише коли драбина агента вичерпана (graph.md:
    // `failed_streak ≥ agent_retry_max`). Інакше найдорожчий актор
    // витрачався б на вузол, який ще навіть не пробували як слід.
    if actor == Actor::Engineer {
        let dir = node_dir(tasks_dir, node_path)?;
        let config = merge_config(
            fs::read_to_string(Path::new(tasks_dir).join("../.mt.json"))
                .ok()
                .as_deref(),
        );
        let agent_retry_max = fm_u64(&config, "agent_retry_max").unwrap_or(3);
        let streak = crate::failed_streak(&dir);
        if streak < agent_retry_max {
            return Err(format!(
                "{node_path}: інженер вмикається після вичерпання драбини агента \
                 ({streak} провалів із {agent_retry_max}) — спершу штатні ретраї"
            ));
        }
    }

    let repo_root = discover_repo_root(Path::new(tasks_dir))?;
    let tasks_root_rel = tasks_root_relative(&repo_root, Path::new(tasks_dir))?;
    let hash = node_hash(&tasks_root_rel, node_path);
    // Ephemeral run-worktree — repo-wide shared placement (те саме адмін-дерево
    // `.git/worktrees/`, що й у `mt worktree create`): якщо `mt run` викликано
    // зсередини linked dev-worktree, `repo_root` вище — корінь *цього*
    // checkout-у (правильно для `tasks_root_rel`/claims), а не головного репо —
    // тож для фізичного розташування нового worktree потрібен окремий,
    // main-scoped корінь, інакше він вкладеться під `.worktrees/<цей-worktree>/`.
    let main_root = discover_main_worktree_root(&repo_root)?;

    let raw_config = fs::read_to_string(repo_root.join(".mt.json")).ok();
    let config = merge_config(raw_config.as_deref());
    let claim_lease_sec = fm_u64(&config, "claim_lease_sec").unwrap_or(3600) as i64;
    let publish_retry_max = fm_u64(&config, "publish_retry_max").unwrap_or(8) as u32;
    let publish_retry_base_ms = fm_u64(&config, "publish_retry_base_ms").unwrap_or(250);

    let repository = GitRepository::open(&repo_root).map_err(|error| error.to_string())?;
    repository
        .fetch_refspec("+refs/heads/main:refs/remotes/origin/main")
        .map_err(|error| error.to_string())?;
    let base_sha = repository
        .resolve_ref("refs/remotes/origin/main")
        .map_err(|error| error.to_string())?;

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

    let worktrees_dir = worktrees_dir_path(&main_root, &config);
    let worktree = create_run_worktree(&main_root, &worktrees_dir, &hash, &token, &base_sha)?;
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
    let wt_tasks_root = worktree.join(&tasks_root_rel);
    let system_prompt = config
        .get("system_prompt")
        .and_then(serde_json::Value::as_str)
        .map(|rel| worktree.join(rel));

    // Одна фаза = один виклик виконавця з каскадом і watchdog. Етапи 1 і 2
    // (graph.md, «Два етапи виконання») — два виклики в межах одного
    // run/claim/worktree, тому тіло спільне.
    // Повертає (підсумок, який CLI спрацював) — без мутації зовнішнього
    // стану, щоб фази можна було викликати в будь-якому порядку.
    let run_phase = |prompt: &str| -> Result<(Option<WatchedOutcome>, Option<String>), String> {
        for cli in cascade_order(&plan.agent_cli, &cli_env.cloud_agent_clis) {
            let model = resolve_model_for_cli(cli_env, &cli, &plan.model_tier);
            let Some((prog, args)) = build_agent_cli_argv(&cli, model.as_deref(), prompt) else {
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
                return Ok((Some(w), Some(cli)));
            }
        }
        Ok((None, None)) // усі CLI каскаду вичерпали ліміти підписки
    };

    // ── Етап 1: планування ──────────────────────────────────────────────
    // Inline-фаза: агент спершу вирішує, вузол атомарний чи розкладається.
    // Пропускається, коли рішення вже є — людський `hint: atomic` або
    // наявний план (зокрема від явного `mt plan`).
    let plan_nnn_before = crate::max_nnn(&dir, "plan_", ".md");
    let run_task_fm = fs::read_to_string(dir.join("task.md"))
        .map(|c| parse_front_matter(&c))
        .unwrap_or_else(|_| serde_json::json!({}));
    // Інженер не планує задачу — він лагодить граф, тож Етап 1 його не
    // стосується.
    if actor == Actor::Agent && needs_planning(&dir, &run_task_fm) {
        let planning_prompt = build_plan_prompt(
            node_path,
            &dir,
            &wt_tasks_root,
            system_prompt.as_deref(),
            plan_nnn_before + 1,
        );
        run_phase(&planning_prompt)?;
    }
    // Composite-план завершує run: далі — людський гейт plan-review, а не
    // виконання. Порожній результат Етапу 1 читається як неявний atomic —
    // план є підмогою, а не перепусткою до роботи.
    let decomposed = latest_plan_decision(&dir).as_deref() == Some("composite");

    // ── Етап 2: виконання ───────────────────────────────────────────────
    let watched: Option<WatchedOutcome> = if decomposed {
        None
    } else {
        let prompt = match actor {
            Actor::Agent => build_agent_prompt(
                node_path,
                &dir,
                &wt_tasks_root,
                system_prompt.as_deref(),
                &nnn_s,
                plan.budget_sec,
            ),
            Actor::Engineer => build_engineer_prompt(
                node_path,
                &dir,
                &wt_tasks_root,
                config
                    .get("engineer_prompt")
                    .and_then(serde_json::Value::as_str)
                    .map(|rel| worktree.join(rel))
                    .as_deref(),
                &nnn_s,
            ),
        };
        let (w, cli) = run_phase(&prompt)?;
        used_agent_cli = cli;
        w
    };
    // Динамічна декомпозиція під час Етапу 2: агент дописав новий composite-
    // план замість fact-у (graph.md, «Протокол spawn»).
    let decomposed = decomposed
        || (crate::max_nnn(&dir, "plan_", ".md") > plan_nnn_before
            && latest_plan_decision(&dir).as_deref() == Some("composite"));

    let wall_sec = started.elapsed().as_secs();
    let cli_fm = used_agent_cli
        .as_ref()
        .map(|c| format!("agent_cli: {c}\n"))
        .unwrap_or_default();
    let extra_fm = format!("{cli_fm}wall_sec: {wall_sec}\n");

    let fact_file = format!("fact_{nnn_s}.md");
    let kill_reason = watched.as_ref().and_then(|w| w.kill_reason);

    let has_fact = dir.join(&fact_file).is_file();
    let (result, run_file, out_fact_file, propagated) = if decomposed {
        // Lifecycle-результат: спроби не було, тому failed_streak не рухається
        // (graph.md, таблиця категорій). Вузол іде на людський гейт.
        let nnn_plan = crate::max_nnn(&dir, "plan_", ".md");
        let sections = format!(
            "\n## Completed\n\nЕтап 1: задачу визнано складеною, план `plan_{nnn_plan:03}.md`\n\n\
             ## Blockers\n\nнемає — потрібен людський апрув плану\n\n\
             ## Next Attempt\n\n`mt spawn --approve` матеріалізує дітей\n"
        );
        let run_file = write_run_fm(&dir, &nnn_s, actor.as_str(), "decomposed", &sections, &extra_fm)?;
        ("decomposed".to_string(), run_file, None, Vec::new())
    } else if kill_reason.is_none() && has_fact {
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
            signal::audit_fm(&wt_tasks_dir_str, node_path, actor.as_str(), &extra_fm)
        } else {
            signal::done_fm(&wt_tasks_dir_str, node_path, actor.as_str(), &extra_fm)
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
                let run_file = write_run_fm(&dir, &nnn_s, actor.as_str(), "failed", &sections, &extra_fm)?;
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
        let run_file = write_run_fm(&dir, &nnn_s, actor.as_str(), &result, &sections, &extra_fm)?;
        (result, run_file, None, Vec::new())
    };

    // Термінальний маркер пишеться ДО коміту, щоб піти тим самим fenced
    // push, що й run: інакше вузол на мить лишався б у стані «ще ретраїмо»
    // з уже вичерпаною драбиною, і наступний прохід оркестратора взяв би
    // його в роботу знову.
    // Другий шар стискання невдач (graph.md): після `run_summary_threshold`
    // failure-ранів wrapper замовляє LLM-резюме. Далі контекст агента бере
    // саме його замість переліку спроб — інакше промпт росте лінійно з
    // довжиною серії, а корисного в ньому не додається.
    let failure_runs = crate::failed_streak(&dir);
    let summary_threshold = fm_u64(&config, "run_summary_threshold").unwrap_or(3);
    if failure_runs >= summary_threshold && !dir.join("run-summary.md").exists() {
        let attempts = prior_attempts(&dir);
        if !attempts.is_empty() {
            let prompt = format!(
                "## Задача\n\nСтисни історію невдалих спроб вузла `{node_path}` в одне резюме \
                 для наступного виконавця.\n\n## Спроби\n\n{}\n\n## Що зробити\n\nЗапиши файл \
                 `run-summary.md` у поточній директорії: що вже пробували, які гіпотези \
                 відпали, і що лишається неперевіреним. Без переказу кожної спроби окремо — \
                 потрібен висновок, а не журнал.",
                attempts.join("\n\n")
            );
            // Невдача резюмування не валить run: це підмога наступній спробі,
            // а не частина результату цієї.
            let _ = run_phase(&prompt);
        }
    }

    if let Some(reason) = crate::unresolvable_reason(&dir, &config) {
        crate::write_unresolvable(&dir, &reason)?;
    }

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
    );

    if let Some(reason) = terminal_conflict_reason(&publish) {
        let sections = format!(
            "\n## Completed\n\n{result}: результат зібрано у worktree\n\n## Blockers\n\n\
             publish не вдався: {reason}\n\n## Next Attempt\n\nперечитати origin/main і повторити\n"
        );
        let conflict_run = write_run_fm(
            &dir,
            &nnn_s,
            "wrapper",
            "merge-conflict",
            &sections,
            &extra_fm,
        )?;
        // Робочий worktree і run ref лишаються для debug (спека,
        // «Failure-сімейство»); публікується лише run-файл, claim
        // звільняється тим самим atomic push.
        let node_rel = format!("{tasks_root_rel}/{node_path}/{conflict_run}");
        publish_failure_run(
            &repo_root,
            &worktrees_dir,
            &publish_req,
            &node_rel,
            &fs::read_to_string(dir.join(&conflict_run)).map_err(|e| e.to_string())?,
            publish_retry_max,
            publish_retry_base_ms,
        )?;
        return Ok(RunOutcome {
            result: "merge-conflict".to_string(),
            run_file: conflict_run,
            fact_file: None,
            wall_sec,
            agent_cli: used_agent_cli,
            propagated: Vec::new(),
        });
    }
    let publish = publish?;

    if !publish.published {
        // Worktree/run ref лишаються для debug (спека, «Failure-сімейство» /
        // «Orphan worktree») — не видаляємо. Сюди доходить лише fencing:
        // claim уже не наш, тому й не чіпаємо.
        return Err(
            "claim-lost: втрачено ownership під час виконання, publish скасовано".to_string(),
        );
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

    // `hint: atomic` — ці тести про Етап 2; рішення atomic уже ухвалене, тож
    // Етап 1 свідомо не запускається (див. `needs_planning`).
    const TASK: &str = "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\nbudget_sec: 5\nbudget_hard_sec: 2\nprogress_timeout_sec: 60\nhint: atomic\n---\n\n## Task\n\nx\n";

    const FLAG: &str = "---\nschema_version: 1\nmodel_tier: AVG\n---\n";

    /// Пише task.md/a.md на диск, без git — для тестів `preflight()`
    /// (суто файлова логіка, git-репо не потрібне).
    fn node_files_only(tmp: &Path, path: &str) {
        let dir = tmp.join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        fs::write(dir.join("a.md"), FLAG).unwrap();
    }

    /// Як [`node_files_only`], але комітить і пушить у `origin/main` —
    /// потрібно для `run_node()`: worktree чекаутиться саме з `origin/main`.
    fn node(tmp: &Path, path: &str) {
        node_files_only(tmp, path);
        crate::test_support::commit_all(tmp, &format!("add {path}"));
        crate::test_support::push_head(tmp, "refs/heads/main");
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
            "---\nschema_version: 1\nmodel_tier: AVG\nagent_cli: cursor\nskills: [bash]\n---\n",
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
            "---\nschema_version: 1\nmodel_tier: AVG\nretry_ladder:\n  - {}\n  - strategy: diagnose-first\n---\n",
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
    fn run_summary_replaces_attempts_once_generated() {
        // Другий шар стискання вмикається за порогом; сам генератор —
        // LLM-виклик, тут перевіряємо ефект на контексті наступної спроби.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        for nnn in 1..=3 {
            fs::write(
                dir.join(format!("run_{nnn:03}.md")),
                "---\nresult: failed\n---\n\n## Blockers\n\nтест X падає\n",
            )
            .unwrap();
        }
        // До резюме — перелік спроб.
        let before = build_agent_prompt("solo", &dir, tmp.path(), None, "004", 600);
        assert!(before.contains("Спроба 001"));

        fs::write(dir.join("run-summary.md"), "Три спроби впирались у конфіг.\n").unwrap();
        let after = build_agent_prompt("solo", &dir, tmp.path(), None, "004", 600);
        assert!(after.contains("Три спроби впирались"));
        assert!(!after.contains("Спроба 001"), "резюме витісняє перелік");
    }

    // ── EngineerAgent (graph.md, «Retry ladder, engineer, unresolvable») ──

    #[test]
    fn engineer_waits_until_agent_ladder_is_exhausted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        let r = root.to_string_lossy().into_owned();

        // Драбина ще не вичерпана — найдорожчий актор не витрачається.
        let err = run_node_env_as(&r, "solo", &env_default(), Actor::Engineer).unwrap_err();
        assert!(err.contains("вичерпання драбини агента"), "got: {err}");
        assert!(err.contains("0 провалів із 3"), "got: {err}");
    }

    #[test]
    fn engineer_history_is_raw_not_compacted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        let run = "---\nschema_version: 1\nresult: failed\n---\n\n## Blockers\n\nтест X падає\n\n\
                   ## Executor output tail\n\n```text\nсирий лог із деталями\n```\n";
        fs::write(dir.join("run_001.md"), run).unwrap();
        fs::write(dir.join("run_002.md"), run).unwrap();

        let p = build_engineer_prompt("solo", &dir, tmp.path(), None, "003");
        assert!(p.contains("Повна історія спроб"));
        assert!(p.contains("run_001.md") && p.contains("run_002.md"));
        // Інженеру потрібні саме подробиці, які агентський контекст стискає.
        assert!(
            p.contains("сирий лог із деталями"),
            "хвіст виводу має лишитись: {p}"
        );
        assert!(p.contains("Дозволені втручання"));
        assert!(p.contains("mt invalidate") && p.contains("mt kill"));
        // Це не промпт виконання: інженер не зобов'язаний писати fact.
        assert!(!p.contains("Обов'язковий фінальний крок"));
    }

    #[test]
    fn engineer_history_resets_after_accepted_fact() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("run_001.md"), "---\nresult: failed\n---\n\n## Blockers\n\nстаре\n")
            .unwrap();
        fs::write(dir.join("fact_002.md"), "---\n---\n\n## Summary\n\nok\n").unwrap();
        assert!(full_run_history(&dir).is_empty());
    }

    #[test]
    fn engineer_run_is_recorded_as_engineer_actor() {
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");
        let dir = root.join("solo");
        // Драбина агента вичерпана: три провали поспіль.
        for nnn in 1..=3 {
            fs::write(
                dir.join(format!("run_{nnn:03}.md")),
                "---\nschema_version: 1\nresult: failed\n---\n",
            )
            .unwrap();
        }
        crate::test_support::commit_all(repo.work.path(), "failed runs");
        crate::test_support::push_head(repo.work.path(), "refs/heads/main");

        let r = root.to_string_lossy().into_owned();
        with_path_shims(&[("claude", FAKE_CLI_WRITES_FACT)], || {
            let out = run_node_env_as(&r, "solo", &env_default(), Actor::Engineer).unwrap();
            assert_eq!(out.result, "success");
        });
        let run = fs::read_to_string(dir.join("run_004.md")).unwrap();
        assert!(run.contains("actor: engineer"), "got: {run}");
    }

    // ── Етап 1: планування (graph.md, «Два етапи виконання») ──

    #[test]
    fn planning_runs_only_when_decision_is_open() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        let no_hint = serde_json::json!({});
        assert!(needs_planning(&dir, &no_hint), "рішення ще немає");

        // Людський hint — рішення вже ухвалене, зайвий виклик моделі не потрібен.
        assert!(!needs_planning(&dir, &serde_json::json!({"hint": "atomic"})));
        // composite-hint планування не скасовує: склад підзадач ще треба вигадати.
        assert!(needs_planning(&dir, &serde_json::json!({"hint": "composite"})));

        // Наявний план (напр. від явного `mt plan`) теж закриває питання.
        fs::write(dir.join("plan_001.md"), "---\ndecision: atomic\n---\n").unwrap();
        assert!(!needs_planning(&dir, &no_hint));
    }

    #[test]
    fn latest_plan_decision_reads_highest_nnn() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        assert!(latest_plan_decision(&dir).is_none());

        fs::write(dir.join("plan_001.md"), "---\ndecision: atomic\n---\n").unwrap();
        assert_eq!(latest_plan_decision(&dir).as_deref(), Some("atomic"));
        // Динамічна декомпозиція дописує наступний NNN — читається саме він.
        fs::write(dir.join("plan_002.md"), "---\ndecision: Composite\n---\n").unwrap();
        assert_eq!(latest_plan_decision(&dir).as_deref(), Some("composite"));
    }

    #[test]
    fn plan_prompt_demands_a_decision_and_forbids_execution() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("solo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("task.md"), TASK).unwrap();
        let p = build_plan_prompt("solo", &dir, tmp.path(), None, 1);

        assert!(p.contains("Етап 1"));
        assert!(p.contains("plan_001.md"));
        assert!(p.contains("decision: atomic"));
        assert!(p.contains("composite"));
        assert!(p.contains("mode"), "mode обов'язковий для кожної дитини");
        assert!(p.contains("Нічого більше на цьому етапі не роби"));
        // Це не промпт виконання: fact на цьому етапі не просять.
        assert!(!p.contains("Обов'язковий фінальний крок"));
    }

    #[test]
    fn composite_plan_ends_run_as_decomposed() {
        // Наскрізно: виконавець на Етапі 1 пише composite-план → run
        // завершується lifecycle-результатом, вузол іде на plan-review.
        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        let dir = root.join("solo");
        fs::create_dir_all(&dir).unwrap();
        // Без hint — Етап 1 запускається.
        fs::write(
            dir.join("task.md"),
            "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\nbudget_sec: 5\nbudget_hard_sec: 2\nprogress_timeout_sec: 60\n---\n\n## Task\n\nx\n",
        )
        .unwrap();
        fs::write(dir.join("a.md"), FLAG).unwrap();
        crate::test_support::commit_all(repo.work.path(), "add solo");
        crate::test_support::push_head(repo.work.path(), "refs/heads/main");

        let r = root.to_string_lossy().into_owned();
        const WRITES_COMPOSITE_PLAN: &str =
            r#"printf -- '---\nschema_version: 1\ndecision: composite\n---\n\n## Children\n\nchildren\n' > plan_001.md"#;
        with_path_shims(&[("claude", WRITES_COMPOSITE_PLAN)], || {
            let out = run_node_env(&r, "solo", &env_default()).unwrap();
            assert_eq!(out.result, "decomposed");
            assert!(out.fact_file.is_none(), "fact на цьому шляху не пишеться");
        });

        let run = fs::read_to_string(root.join("solo/run_001.md")).unwrap();
        assert!(run.contains("result: decomposed"));
        assert!(run.contains("plan-review") || run.contains("spawn --approve"));
        // Lifecycle-результат: серія провалів не рухається (graph.md).
        assert_eq!(
            crate::scan_tasks(r.clone(), vec![])
                .unwrap()
                .first()
                .map(|n| n.state.clone()),
            Some(crate::TaskState::PlanReview)
        );
    }

    // ── контекст агента (graph.md, «Контекст агента») ──

    /// Дерево `<tmp>/mt/<node>` з файлами; повертає зібраний промпт.
    fn prompt_for(node: &str, files: &[(&str, &str)], extra: &[(&str, &str)]) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let tasks_root = tmp.path().join("mt");
        let dir = tasks_root.join(node);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        // Файли поза вузлом (залежності, system-prompt) — відносно tasks_root.
        for (rel, content) in extra {
            let path = tasks_root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let sp = tasks_root.join("system-prompt.md");
        build_agent_prompt(node, &dir, &tasks_root, Some(&sp), "003", 600)
    }

    #[test]
    fn context_has_task_body_and_final_step() {
        let p = prompt_for("solo", &[("task.md", TASK)], &[]);
        assert!(p.contains("## task.md"));
        assert!(p.contains("Обов'язковий фінальний крок"));
        assert!(p.contains("fact_003.md"));
        // Порожніх заголовків для відсутніх артефактів немає.
        assert!(!p.contains("Актуальний план"));
        assert!(!p.contains("Попередні спроби"));
        assert!(!p.contains("Результати залежностей"));
    }

    #[test]
    fn context_includes_system_prompt_and_flag() {
        let p = prompt_for(
            "solo",
            &[("task.md", TASK), ("a.md", FLAG)],
            &[("system-prompt.md", "Протокол: спершу тести.\n")],
        );
        assert!(p.contains("Протокол виконання"));
        assert!(p.contains("спершу тести"));
        assert!(p.contains("Прапор виконавця (a.md)"));
        assert!(p.contains("model_tier: AVG"));
    }

    #[test]
    fn context_inlines_dependency_facts_not_edge_files() {
        let p = prompt_for(
            "solo",
            &[("task.md", TASK), ("deps/collect.md", "")],
            &[(
                "collect/fact_001.md",
                "---\nschema_version: 1\n---\n\n## Summary\n\nЗібрано 42 рядки.\n",
            )],
        );
        assert!(p.contains("Результати залежностей"));
        assert!(p.contains("### collect"));
        assert!(p.contains("Зібрано 42 рядки"), "інлайниться fact, не ребро");
    }

    #[test]
    fn context_compacts_prior_attempts_to_sections() {
        let run = "---\nschema_version: 1\nresult: failed\n---\n\n## Completed\n\nнічого\n\n\
                   ## Blockers\n\nтест X падає\n\n## Next Attempt\n\nполагодити X\n\n\
                   ## Executor output tail\n\n```text\nдовгий сирий лог\n```\n";
        let p = prompt_for(
            "solo",
            &[("task.md", TASK), ("run_001.md", run), ("run_002.md", run)],
            &[],
        );
        assert!(p.contains("Попередні спроби"));
        assert!(p.contains("Спроба 001") && p.contains("Спроба 002"));
        assert!(p.contains("тест X падає"));
        // Перший шар стискання: сирий хвіст логу в промпт не тягнеться.
        assert!(!p.contains("довгий сирий лог"));
    }

    #[test]
    fn run_summary_replaces_raw_attempts() {
        let run = "---\nresult: failed\n---\n\n## Blockers\n\nтест X падає\n";
        let p = prompt_for(
            "solo",
            &[
                ("task.md", TASK),
                ("run_001.md", run),
                ("run-summary.md", "Три спроби впирались в конфіг.\n"),
            ],
            &[],
        );
        assert!(p.contains("Резюме попередніх спроб"));
        assert!(p.contains("Три спроби впирались"));
        // Другий шар стискання витісняє перший, а не додається до нього.
        assert!(!p.contains("Спроба 001"));
    }

    #[test]
    fn attempts_reset_after_accepted_fact() {
        let run = "---\nresult: failed\n---\n\n## Blockers\n\nстаре\n";
        let p = prompt_for(
            "solo",
            &[
                ("task.md", TASK),
                ("run_001.md", run),
                ("fact_002.md", "---\n---\n\n## Summary\n\nok\n"),
            ],
            &[],
        );
        // Прийнятий fact_002 закриває історію: run_001 більше не релевантний.
        assert!(!p.contains("Попередні спроби"), "got: {p}");
    }

    #[test]
    fn context_includes_audit_verdict_and_clarification() {
        let p = prompt_for(
            "solo",
            &[
                ("task.md", TASK),
                (
                    "audit-result_001.md",
                    "---\nresult: failed\n---\n\nНе покрито крайній випадок.\n",
                ),
                (
                    "clarification_001.md",
                    "---\n---\n\nЧому обрано саме цей алгоритм?\n",
                ),
            ],
            &[],
        );
        assert!(p.contains("Вердикт аудиту"));
        assert!(p.contains("Не покрито крайній випадок"));
        assert!(p.contains("Запит уточнення"));
    }

    // ── класифікація підсумку publish (git.md) ──

    fn outcome(published: bool, fenced: bool) -> Result<PublishOutcome, String> {
        Ok(PublishOutcome {
            published,
            fenced,
            result_sha: None,
            attempts: 1,
        })
    }

    #[test]
    fn rebase_conflict_is_terminal_merge_conflict() {
        let publish = Err("rebase conflict on publish: CONFLICT (content)".to_string());
        assert!(terminal_conflict_reason(&publish)
            .unwrap()
            .contains("rebase conflict"));
    }

    #[test]
    fn exhausted_retries_are_terminal_merge_conflict() {
        assert!(terminal_conflict_reason(&outcome(false, false))
            .unwrap()
            .contains("вичерпано publish retry"));
    }

    #[test]
    fn claim_lost_is_not_merge_conflict() {
        // fenced — це claim-lost (lifecycle), лічильник не рухає.
        assert!(terminal_conflict_reason(&outcome(false, true)).is_none());
    }

    #[test]
    fn successful_publish_is_not_conflict() {
        assert!(terminal_conflict_reason(&outcome(true, false)).is_none());
    }

    #[test]
    fn unrelated_error_is_not_merge_conflict() {
        // Системний збій не маскується під результат спроби.
        let publish = Err("git: permission denied".to_string());
        assert!(terminal_conflict_reason(&publish).is_none());
    }

    #[test]
    fn preflight_fails_closed_on_unknown_task_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(
            root.join("solo/task.md"),
            "---\nschema_version: 2\nbudget_sec: 5\n---\n\n## Task\n\nx\n",
        )
        .unwrap();
        let r = root.to_string_lossy().into_owned();

        let err = preflight_env(&r, "solo", &env_default()).unwrap_err();
        assert!(err.contains("schema_version 2"), "got: {err}");
        assert!(err.contains("task.md"), "помилка має називати файл");
    }

    #[test]
    fn preflight_fails_closed_on_unknown_flag_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(
            root.join("solo/a.md"),
            "---\nschema_version: 7\nmodel_tier: AVG\n---\n",
        )
        .unwrap();
        let r = root.to_string_lossy().into_owned();

        let err = preflight_env(&r, "solo", &env_default()).unwrap_err();
        assert!(err.contains("schema_version 7"), "got: {err}");
    }

    #[test]
    fn preflight_rejects_legacy_section_flag() {
        // Жорсткий перехід: старий a.md не читається як порожній (що підмінило
        // б model_tier/agent_cli дефолтами), а валить preflight до claim.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(
            root.join("solo/a.md"),
            "## Model tier\n\nMAX\n\n## Agent cli\n\ncursor\n",
        )
        .unwrap();
        let r = root.to_string_lossy().into_owned();

        let err = preflight_env(&r, "solo", &env_default()).unwrap_err();
        assert!(err.contains("markdown-секційний формат"), "got: {err}");
        assert!(err.contains("graph.md"), "помилка має вказувати на спеку");
    }

    #[test]
    fn preflight_rejects_flag_without_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node_files_only(&root, "solo");
        fs::write(root.join("solo/a.md"), "model_tier: MAX\n").unwrap();
        let r = root.to_string_lossy().into_owned();

        let err = preflight_env(&r, "solo", &env_default()).unwrap_err();
        assert!(err.contains("без YAML-frontmatter"), "got: {err}");
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
        assert!(!crate::test_support::remote_ref_exists(
            repo.work.path(),
            "refs/mt/claims/00000000000000000000"
        ));
        // Локальний main (той самий work-клон) підхопив публікацію.
        assert!(root.join("solo/fact_001.md").is_file());
        let run = fs::read_to_string(root.join("solo/run_001.md")).unwrap();
        assert!(run.contains("result: success"));
        assert!(run.contains("agent_cli: claude"));
    }

    #[test]
    fn run_from_inside_a_linked_worktree_places_ephemeral_worktree_under_main_root() {
        // `mt run`, викликаний з cwd усередині dev-worktree (а не головного
        // checkout-у), не повинен вкладати ephemeral run-worktree під
        // `.worktrees/` *цього* worktree — фізичне розташування має бути
        // repo-wide, під головним коренем (той самий нюанс, що й `mt
        // worktree create`, див. `discover_main_worktree_root`).
        use crate::worktree::create_dev_worktree;

        let repo = TestRepo::new();
        let root = repo.work.path().join("mt");
        node(&root, "solo");

        let worktrees_dir = tempfile::tempdir().unwrap();
        let devwork =
            create_dev_worktree(repo.work.path(), worktrees_dir.path(), "devwork", "main").unwrap();
        let devwork_tasks_dir = devwork.path.join("mt");
        assert!(devwork_tasks_dir.join("solo/task.md").is_file());

        let marker = tempfile::NamedTempFile::new().unwrap();
        let marker_path = marker.path().to_path_buf();
        std::env::set_var("MT_TEST_RUN_WORKTREE_PWD_MARKER", &marker_path);

        let r = devwork_tasks_dir.to_string_lossy().into_owned();
        with_path_shims(
            &[(
                "claude",
                r#"pwd > "$MT_TEST_RUN_WORKTREE_PWD_MARKER"
printf -- '---\nschema_version: 1\n---\n\n## Summary\n\nok\n' > "fact_${MT_RUN_NNN}.md""#,
            )],
            || {
                let out = run_node_env(&r, "solo", &env_default()).unwrap();
                assert_eq!(out.result, "success");
            },
        );
        std::env::remove_var("MT_TEST_RUN_WORKTREE_PWD_MARKER");

        let captured_cwd = fs::read_to_string(&marker_path).unwrap();
        let captured_cwd = captured_cwd.trim();
        assert!(
            !captured_cwd.contains("/.worktrees/devwork/.worktrees/"),
            "ephemeral run-worktree nested under the linked worktree instead of the main root: {captured_cwd}"
        );

        // Головний репо-корінь отримав `.worktrees/<hash>-<token>` (успішний
        // run прибирає worktree по завершенню — перевіряємо, що nested-шлях
        // під devwork так і не з'явився).
        assert!(!devwork.path.join(".worktrees").exists());
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
        fs::write(dir.join("a.md"), FLAG).unwrap();
        crate::test_support::commit_all(repo.work.path(), "add gated");
        crate::test_support::push_head(repo.work.path(), "refs/heads/main");

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
