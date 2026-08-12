//! Мета-цикл ретроспективи (спека `retro.md`) — MVP.
//!
//! Audit trail графа вже є готовим датасетом: `run_NNN` несуть `result`,
//! `actor`, `agent_cli`, `wall_sec`. Тут — читач цієї історії і **детерміновані**
//! закономірності, які видно без LLM: рівно ті, що спека наводить прикладами
//! («щабель N драбини ніколи не рятує», «задачі з tool Y мають гірший
//! результат, ніж із Z»).
//!
//! Чотири нормативні принципи глави тримаються тут конструкцією, а не
//! обіцянкою:
//!
//! 1. **Працює на виконавця** — цикл opt-in, `enabled` за замовчуванням
//!    `false`; без явного вмикання аналіз не запускається.
//! 2. **Не інструмент нагляду** — агрегати рахуються по `agent_cli` і
//!    щаблях драбини, тобто по **інструментах**. Персональних зрізів по
//!    людях тут немає й не передбачено API.
//! 3. **Пропозиція ≠ дія** — модуль лише формує текст; він нічого не пише
//!    в граф і не чіпає конфіги.
//! 4. **Дані не покидають периметр** — звіт лягає в приватний простір
//!    виконавця поза репозиторієм задач.
//!
//! LLM-збагачення (вільні формулювання, зв'язки між класами задач) — окремий
//! крок поверх цього датасету; детермінована частина цінна сама по собі й,
//! на відміну від LLM-кроку, перевіряється тестом.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::frontmatter::parse_front_matter;

/// Конфіг ретроспективи (`retro` у `.mt.json`).
///
/// Кожне поле має власний дефолт: секція в `.mt.json` майже завжди
/// часткова (`{"enabled": true}` і більше нічого), а без per-field
/// дефолтів така секція не розібралась би цілком — і вмикання ретро
/// мовчки не спрацювало б.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetroConfig {
    /// Opt-in per-виконавець. Дефолт `false` — контрактна вимога глави.
    pub enabled: bool,
    /// Період між прогонами, днів.
    pub schedule_days: u64,
    /// Не ганяти аналіз на порожньому періоді.
    pub min_resolved: usize,
    /// Поріг довіри для зрізів: менше спостережень — не робимо висновку.
    pub impact_min_runs: usize,
    /// Тир моделі для LLM-кроку (тут не використовується — крок окремий).
    pub model_tier: String,
}

impl Default for RetroConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule_days: 7,
            min_resolved: 10,
            impact_min_runs: 10,
            model_tier: "AVG".to_string(),
        }
    }
}

/// Читає секцію `retro` з конфігу проєкту; відсутня секція — дефолти.
pub fn retro_config(config: &Value) -> RetroConfig {
    config
        .get("retro")
        .and_then(|section| serde_json::from_value(section.clone()).ok())
        .unwrap_or_default()
}

/// Один run з історії вузла.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Шлях вузла в межах tasks-дерева.
    pub node: String,
    /// Порядковий номер run-а у вузлі — він же номер спроби драбини.
    pub attempt: u64,
    /// Актор (`agent`, `engineer`, `auditor`).
    pub actor: String,
    /// Виконавець ходу, якщо записаний.
    pub agent_cli: Option<String>,
    /// Результат (`success`, `failed`, `merge-conflict`, `handoff`, …).
    pub result: String,
    /// Тривалість, якщо записана.
    pub wall_sec: Option<u64>,
}

impl RunRecord {
    /// Посилання на файл для `evidence`.
    fn evidence(&self) -> String {
        format!("{}/run_{:03}.md", self.node, self.attempt)
    }

    /// Чи є результат успіхом виконання.
    fn is_success(&self) -> bool {
        self.result == "success"
    }

    /// Чи є результат провалом **виконання** — а не подією життєвого циклу.
    ///
    /// `handoff`, `killed`, `invalidated` тощо не є провалами агента, і
    /// рахувати їх у статистику інструментів означало б звинувачувати CLI
    /// у переїзді сесії на іншу машину.
    fn is_execution_failure(&self) -> bool {
        matches!(
            self.result.as_str(),
            "failed" | "merge-conflict" | "timeout"
        )
    }
}

/// Пропозиція ретроспективи — рівно поля зі схеми глави.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    /// Вузол або клас задач.
    pub target: String,
    /// Що саме побачив аналіз.
    pub observed: String,
    /// Що пропонується змінити.
    pub proposal: String,
    /// Файли-докази.
    pub evidence: Vec<String>,
    /// Очікуваний ефект.
    pub impact_estimate: String,
}

/// Збирає run-історію всього дерева задач.
pub fn collect_runs(tasks_dir: &Path) -> Vec<RunRecord> {
    let mut out = Vec::new();
    collect_into(tasks_dir, tasks_dir, &mut out);
    out.sort_by(|a, b| (a.node.as_str(), a.attempt).cmp(&(b.node.as_str(), b.attempt)));
    out
}

fn collect_into(root: &Path, dir: &Path, out: &mut Vec<RunRecord>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_into(root, &path, out);
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(nnn) = name
            .strip_prefix("run_")
            .and_then(|rest| rest.strip_suffix(".md"))
            .and_then(|digits| digits.parse::<u64>().ok())
        else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let front = parse_front_matter(&text);
        let field = |key: &str| {
            front
                .get(key)
                .and_then(|value| value.as_str().map(str::to_string))
        };
        let node = dir
            .strip_prefix(root)
            .unwrap_or(dir)
            .to_string_lossy()
            .to_string();
        out.push(RunRecord {
            node,
            attempt: nnn,
            actor: field("actor").unwrap_or_else(|| "agent".into()),
            agent_cli: field("agent_cli"),
            result: field("result").unwrap_or_else(|| "unknown".into()),
            wall_sec: front.get("wall_sec").and_then(Value::as_u64),
        });
    }
}

/// Скільки вузлів дійшли до успіху — вхідний поріг `min_resolved`.
pub fn resolved_nodes(runs: &[RunRecord]) -> usize {
    let mut nodes: Vec<&str> = runs
        .iter()
        .filter(|run| run.is_success())
        .map(|run| run.node.as_str())
        .collect();
    nodes.sort_unstable();
    nodes.dedup();
    nodes.len()
}

/// Детермінований аналіз історії.
///
/// Обидва правила навмисно консервативні: висновок робиться лише там, де
/// спостережень не менше `impact_min_runs`. Порада, зроблена з двох випадків,
/// гірша за відсутність поради — вона виглядає як дані.
pub fn analyze(runs: &[RunRecord], config: &RetroConfig) -> Vec<Suggestion> {
    let mut out = Vec::new();
    out.extend(hopeless_attempt(runs, config));
    out.extend(weak_agent_cli(runs, config));
    out
}

/// Щабель драбини, який жодного разу не врятував.
///
/// Спека наводить це прикладом дослівно: «щабель 2 retry ladder на вузлах
/// типу W ніколи не рятує — одразу alternative-approach».
fn hopeless_attempt(runs: &[RunRecord], config: &RetroConfig) -> Option<Suggestion> {
    // Дивимось лише на спроби, що взагалі бували не першими: перша спроба
    // не є «щаблем драбини», вона є самою задачею.
    let mut worst: Option<(u64, usize, Vec<String>)> = None;
    for attempt in 2..=6u64 {
        let at: Vec<&RunRecord> = runs
            .iter()
            .filter(|run| run.attempt == attempt && run.actor == "agent")
            .collect();
        if at.len() < config.impact_min_runs {
            continue;
        }
        if at.iter().any(|run| run.is_success()) {
            continue;
        }
        let evidence = at.iter().take(5).map(|run| run.evidence()).collect();
        worst = Some((attempt, at.len(), evidence));
        break;
    }
    let (attempt, count, evidence) = worst?;
    Some(Suggestion {
        target: "*".to_string(),
        observed: format!("щабель {attempt} драбини не дав жодного успіху (спостережень: {count})"),
        proposal: format!(
            "retry_ladder: прибрати щабель {attempt} або замінити його на alternative-approach"
        ),
        evidence,
        impact_estimate: format!("марних прогонів на періоді: {count}"),
    })
}

/// Виконавець із помітно гіршим результатом за решту.
///
/// Це порівняння **інструментів**, не людей (принцип «не інструмент
/// нагляду»): у датасеті немає поля виконавця-людини й не передбачено.
fn weak_agent_cli(runs: &[RunRecord], config: &RetroConfig) -> Option<Suggestion> {
    let mut stats: std::collections::BTreeMap<String, (usize, usize)> =
        std::collections::BTreeMap::new();
    for run in runs.iter().filter(|run| run.actor == "agent") {
        let Some(cli) = &run.agent_cli else { continue };
        let entry = stats.entry(cli.clone()).or_insert((0, 0));
        if run.is_success() {
            entry.0 += 1;
        } else if run.is_execution_failure() {
            entry.1 += 1;
        }
    }
    let rate = |(ok, bad): &(usize, usize)| {
        let total = ok + bad;
        if total == 0 {
            return None;
        }
        Some((*ok as f64 / total as f64, total))
    };
    let mut ranked: Vec<(String, f64, usize)> = stats
        .iter()
        .filter_map(|(cli, counts)| rate(counts).map(|(r, total)| (cli.clone(), r, total)))
        .filter(|(_, _, total)| *total >= config.impact_min_runs)
        .collect();
    if ranked.len() < 2 {
        return None;
    }
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let (weak, weak_rate, weak_total) = ranked.first()?.clone();
    let (strong, strong_rate, _) = ranked.last()?.clone();
    // Розрив має бути змістовним, інакше це шум вибірки.
    if strong_rate - weak_rate < 0.25 {
        return None;
    }
    let evidence = runs
        .iter()
        .filter(|run| run.agent_cli.as_deref() == Some(weak.as_str()) && run.is_execution_failure())
        .take(5)
        .map(RunRecord::evidence)
        .collect();
    Some(Suggestion {
        target: "*".to_string(),
        observed: format!(
            "{weak}: {:.0}% успіху на {weak_total} прогонах проти {:.0}% у {strong}",
            weak_rate * 100.0,
            strong_rate * 100.0
        ),
        proposal: format!("agent_cli: спробувати {strong} на задачах цього класу"),
        evidence,
        impact_estimate: format!(
            "+{:.0} п.п. успішності за поточною вибіркою",
            (strong_rate - weak_rate) * 100.0
        ),
    })
}

/// Markdown-звіт періоду.
pub fn report_markdown(period: &str, suggestions: &[Suggestion]) -> String {
    let mut out = format!("# Ретроспектива {period}\n\n");
    if suggestions.is_empty() {
        out.push_str("Закономірностей із достатньою кількістю спостережень не знайдено.\n");
        return out;
    }
    for (index, suggestion) in suggestions.iter().enumerate() {
        out.push_str(&format!(
            "## Пропозиція {}\n\n```yaml\nsuggestion:\n  target: '{}'\n  observed: '{}'\n  \
             proposal: '{}'\n  evidence: [{}]\n  impact_estimate: '{}'\n```\n\n",
            index + 1,
            suggestion.target,
            suggestion.observed.replace('\'', "’"),
            suggestion.proposal.replace('\'', "’"),
            suggestion.evidence.join(", "),
            suggestion.impact_estimate.replace('\'', "’")
        ));
    }
    out
}

/// Приватний простір звітів виконавця: `<home>/.nitra/retro`.
///
/// Поза репозиторієм задач — свідомо: у спільний граф пропозиції не
/// потрапляють (принцип «працює на виконавця»).
pub fn retro_dir(home: &Path) -> PathBuf {
    home.join(".nitra").join("retro")
}

/// Пише звіт періоду; повертає шлях.
///
/// # Errors
/// Помилки файлової системи.
pub fn write_report(home: &Path, period: &str, body: &str) -> std::io::Result<PathBuf> {
    let dir = retro_dir(home);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{period}.md"));
    fs::write(&path, body)?;
    Ok(path)
}

/// Останній звіт у приватному просторі (для `mt retro show`).
pub fn latest_report(home: &Path) -> Option<(PathBuf, String)> {
    let dir = retro_dir(home);
    let mut reports: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    reports.sort();
    let path = reports.pop()?;
    let text = fs::read_to_string(&path).ok()?;
    Some((path, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(node: &str, attempt: u64, cli: &str, result: &str) -> RunRecord {
        RunRecord {
            node: node.into(),
            attempt,
            actor: "agent".into(),
            agent_cli: Some(cli.into()),
            result: result.into(),
            wall_sec: Some(10),
        }
    }

    fn config() -> RetroConfig {
        RetroConfig {
            impact_min_runs: 3,
            ..RetroConfig::default()
        }
    }

    #[test]
    fn retro_is_opt_in_by_default() {
        // Контрактна вимога глави, не деталь реалізації.
        assert!(!RetroConfig::default().enabled);
        assert!(!retro_config(&serde_json::json!({})).enabled);
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        // Секція в конфігу майже завжди часткова; без per-field дефолтів
        // вона не розібралась би цілком — і вмикання мовчки не спрацювало б.
        let config = retro_config(&serde_json::json!({"retro": {"enabled": true}}));
        assert!(config.enabled);
        assert_eq!(config.min_resolved, RetroConfig::default().min_resolved);
        assert_eq!(config.model_tier, RetroConfig::default().model_tier);
    }

    #[test]
    fn hopeless_rung_is_reported() {
        let runs: Vec<RunRecord> = (1..=4)
            .flat_map(|n| {
                [
                    run(&format!("node-{n}"), 1, "codex", "failed"),
                    run(&format!("node-{n}"), 2, "codex", "failed"),
                ]
            })
            .collect();
        let found = analyze(&runs, &config());
        let rung = found
            .iter()
            .find(|s| s.proposal.contains("retry_ladder"))
            .expect("щабель без жодного успіху мав дати пропозицію");
        assert!(rung.observed.contains("щабель 2"), "{}", rung.observed);
        // Звіт заявлений як YAML, тож `*` мусить бути в лапках — інакше
        // це якір-аліас, і документ не парситься.
        let body = report_markdown("2026-08", &found);
        assert!(body.contains("target: '*'"), "{body}");
        assert!(!rung.evidence.is_empty());
    }

    #[test]
    fn rung_that_ever_rescues_is_not_reported() {
        // Один успіх на щаблі знімає висновок: щабель рятує, просто рідко.
        let mut runs: Vec<RunRecord> = (1..=4)
            .flat_map(|n| {
                [
                    run(&format!("node-{n}"), 1, "codex", "failed"),
                    run(&format!("node-{n}"), 2, "codex", "failed"),
                ]
            })
            .collect();
        runs.push(run("node-5", 2, "codex", "success"));
        assert!(!analyze(&runs, &config())
            .iter()
            .any(|s| s.proposal.contains("retry_ladder")));
    }

    #[test]
    fn small_sample_yields_no_advice() {
        // Порада з двох випадків гірша за відсутність поради — вона
        // виглядає як дані.
        let runs = vec![
            run("a", 2, "codex", "failed"),
            run("b", 2, "codex", "failed"),
        ];
        assert!(analyze(&runs, &config()).is_empty());
    }

    #[test]
    fn weak_tool_is_compared_against_strong_one() {
        let mut runs = Vec::new();
        for n in 0..4 {
            runs.push(run(&format!("x{n}"), 1, "codex", "failed"));
            runs.push(run(&format!("y{n}"), 1, "claude", "success"));
        }
        let found = analyze(&runs, &config());
        let tool = found
            .iter()
            .find(|s| s.proposal.contains("agent_cli"))
            .expect("розрив у результатах інструментів мав дати пропозицію");
        assert!(tool.observed.contains("codex"), "{}", tool.observed);
        assert!(tool.proposal.contains("claude"), "{}", tool.proposal);
    }

    #[test]
    fn similar_tools_are_not_ranked() {
        // Без змістовного розриву висновку немає — інакше кожен шум
        // вибірки ставав би «рекомендацією».
        let mut runs = Vec::new();
        for n in 0..4 {
            runs.push(run(&format!("x{n}"), 1, "codex", "success"));
            runs.push(run(&format!("y{n}"), 1, "claude", "success"));
        }
        assert!(!analyze(&runs, &config())
            .iter()
            .any(|s| s.proposal.contains("agent_cli")));
    }

    #[test]
    fn lifecycle_results_do_not_blame_the_tool() {
        // handoff — це переїзд сесії, а не провал CLI.
        let mut runs = Vec::new();
        for n in 0..4 {
            runs.push(run(&format!("x{n}"), 1, "codex", "handoff"));
            runs.push(run(&format!("y{n}"), 1, "claude", "success"));
        }
        assert!(!analyze(&runs, &config())
            .iter()
            .any(|s| s.proposal.contains("agent_cli")));
    }

    #[test]
    fn collect_reads_runs_from_tree() {
        let dir = tempfile::tempdir().unwrap();
        let node = dir.path().join("alpha/beta");
        std::fs::create_dir_all(&node).unwrap();
        std::fs::write(
            node.join("run_001.md"),
            "---\nschema_version: 1\nactor: agent\nagent_cli: codex\nresult: failed\nwall_sec: 12\n---\n",
        )
        .unwrap();

        let runs = collect_runs(dir.path());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].node, "alpha/beta");
        assert_eq!(runs[0].attempt, 1);
        assert_eq!(runs[0].agent_cli.as_deref(), Some("codex"));
        assert_eq!(runs[0].wall_sec, Some(12));
        assert_eq!(resolved_nodes(&runs), 0);
    }

    #[test]
    fn report_is_written_outside_the_task_repo() {
        // Дані не покидають периметр і не потрапляють у спільний граф.
        let home = tempfile::tempdir().unwrap();
        let body = report_markdown("2026-08", &[]);
        let path = write_report(home.path(), "2026-08", &body).unwrap();
        assert!(path.starts_with(home.path().join(".nitra/retro")));

        let (found, text) = latest_report(home.path()).unwrap();
        assert_eq!(found, path);
        assert!(text.contains("Ретроспектива 2026-08"));
    }
}
