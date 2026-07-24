//! `mt doctor` — діагностика `.mt.json`/`mt/`/git-стану проєкту.

use clap::Args;
use serde::Serialize;

use crate::context::resolve_tasks_dir;
use crate::output::json;

#[derive(Serialize)]
struct Check {
    name: String,
    ok: bool,
    detail: String,
}

#[derive(Args)]
pub struct DoctorArgs {}

pub fn run(_args: DoctorArgs, as_json: bool) -> Result<(), String> {
    let mut checks = Vec::new();

    let tasks_dir = resolve_tasks_dir(false);
    checks.push(Check {
        name: "tasks_dir".to_string(),
        ok: tasks_dir.is_ok(),
        detail: tasks_dir
            .clone()
            .unwrap_or_else(|e| format!("не знайдено — {e} (запусти `mt init <name>`)")),
    });

    if let Ok(tasks_dir) = &tasks_dir {
        match mt_core::claims::discover_repo_root(std::path::Path::new(tasks_dir)) {
            Ok(root) => {
                checks.push(Check {
                    name: "git_repo".to_string(),
                    ok: true,
                    detail: root.display().to_string(),
                });
                let origin = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(["remote", "get-url", "origin"])
                    .output();
                let has_origin = origin.map(|o| o.status.success()).unwrap_or(false);
                checks.push(Check {
                    name: "git_origin".to_string(),
                    ok: has_origin,
                    detail: if has_origin {
                        "origin налаштовано".to_string()
                    } else {
                        "немає origin — claim/publish/run недоступні".to_string()
                    },
                });
            }
            Err(e) => checks.push(Check {
                name: "git_repo".to_string(),
                ok: false,
                detail: e,
            }),
        }
    }

    let all_ok = checks.iter().all(|c| c.ok);
    if as_json {
        json(&serde_json::json!({ "checks": checks, "ok": all_ok }));
    } else {
        for c in &checks {
            println!("{} {}: {}", if c.ok { "✓" } else { "✗" }, c.name, c.detail);
        }
    }
    std::process::exit(if all_ok { 0 } else { 1 });
}
