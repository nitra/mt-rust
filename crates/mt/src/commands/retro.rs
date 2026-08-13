//! `mt retro` / `mt retro show` — мета-цикл ретроспективи (спека `retro.md`).
//!
//! Гейт opt-in тут не косметичний: цикл працює **на виконавця**, а не на
//! нагляд, тож без явного `retro.enabled` він не запускається взагалі —
//! і команда каже, що саме треба ввімкнути, а не мовчить.

use std::path::Path;

use mt_core::retro::{
    analyze, collect_runs, latest_report, report_markdown, resolved_nodes, retro_config,
    write_report,
};

use crate::context::{project_config, resolve_tasks_dir};

/// Аргументи `mt retro`.
#[derive(Debug, clap::Args)]
pub struct RetroArgs {
    /// Показати останній звіт замість нового прогону.
    #[arg(long)]
    pub show: bool,
    /// Мітка періоду для імені звіту (дефолт — поточний місяць).
    #[arg(long)]
    pub period: Option<String>,
    /// Прогнати попри поріг `min_resolved` (ручний запуск зі спеки).
    #[arg(long)]
    pub force: bool,
}

/// Домашня тека виконавця — приватний простір звітів.
fn home() -> Result<std::path::PathBuf, String> {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| "HOME не визначено — нема куди класти приватний звіт".to_string())
}

/// Поточний період як `YYYY-MM`.
fn current_period() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

/// Запускає команду.
///
/// # Errors
/// Ретро вимкнено, немає tasks-дерева або помилки запису.
pub fn run(args: RetroArgs, json: bool) -> Result<(), String> {
    if args.show {
        return show(json);
    }

    let tasks_dir = resolve_tasks_dir(false)?;
    let config = retro_config(&project_config(&tasks_dir));
    if !config.enabled {
        return Err(
            "ретро вимкнено: цикл opt-in per-виконавець — увімкни `retro.enabled` у .mt.json"
                .to_string(),
        );
    }

    let runs = collect_runs(Path::new(&tasks_dir));
    let resolved = resolved_nodes(&runs);
    if !args.force && resolved < config.min_resolved {
        // Не помилка: порожній період — штатний стан, а не збій.
        if json {
            println!(
                "{}",
                serde_json::json!({ "skipped": true, "resolved": resolved, "min_resolved": config.min_resolved })
            );
        } else {
            println!(
                "пропускаю: закритих вузлів {resolved} < min_resolved ({}); --force щоб прогнати",
                config.min_resolved
            );
        }
        return Ok(());
    }

    let suggestions = analyze(&runs, &config);
    let period = args.period.unwrap_or_else(current_period);
    let body = report_markdown(&period, &suggestions);
    let path = write_report(&home()?, &period, &body).map_err(|error| error.to_string())?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "period": period,
                "report": path.to_string_lossy(),
                "runs": runs.len(),
                "resolved": resolved,
                "suggestions": suggestions,
            })
        );
    } else {
        println!(
            "ретро {period}: прогонів {}, закритих вузлів {resolved}, пропозицій {}",
            runs.len(),
            suggestions.len()
        );
        println!("звіт: {}", path.display());
    }
    Ok(())
}

/// `mt retro --show` — останній звіт із приватного простору.
fn show(json: bool) -> Result<(), String> {
    let Some((path, text)) = latest_report(&home()?) else {
        return Err("звітів ретро ще немає — прогони `mt retro`".to_string());
    };
    if json {
        println!(
            "{}",
            serde_json::json!({ "report": path.to_string_lossy(), "body": text })
        );
    } else {
        println!("{text}");
    }
    Ok(())
}
