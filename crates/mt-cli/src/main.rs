use std::path::PathBuf;
use std::process;

fn usage() -> ! {
    eprintln!("Usage:");
    eprintln!("  mt-scanner scan <tasks_dir> [--worktrees a,b,c]  — scan tasks, output JSON array");
    eprintln!("      --worktrees: comma-list of active worktree names (overrides git discovery)");
    eprintln!("  mt-scanner workspaces [<dir>]                    — discover workspaces, output JSON array");
    eprintln!(
        "  mt-scanner create <tasks_dir> <name> [flags]     — create a task node, output JSON"
    );
    eprintln!("      [--mode agent|human] [--model-tier MIN|AVG|MAX] [--budget-sec N] [--hint <t>] [--dep <id>]...");
    process::exit(1);
}

/// Parses `create` flags after `<tasks_dir> <name>`. Unknown flags are ignored.
fn parse_create_opts(args: &[String]) -> mt_core::CreateOpts {
    let mut opts = mt_core::CreateOpts::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                opts.mode = match args.get(i + 1).map(String::as_str) {
                    Some("agent") => Some(mt_core::Mode::Agent),
                    Some("human") => Some(mt_core::Mode::Human),
                    _ => {
                        eprintln!("Error: --mode must be agent|human");
                        process::exit(2);
                    }
                };
                i += 1;
            }
            "--model-tier" => {
                opts.model_tier = args.get(i + 1).cloned();
                i += 1;
            }
            "--budget-sec" => {
                opts.budget_sec = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 1;
            }
            "--hint" => {
                opts.hint = args.get(i + 1).cloned();
                i += 1;
            }
            "--dep" => {
                if let Some(v) = args.get(i + 1) {
                    opts.deps.push(v.clone());
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Parses an optional `--worktrees a,b,c` flag. Returns None if the flag is absent,
/// Some(vec) (possibly empty) when present.
fn parse_worktrees_arg(args: &[String]) -> Option<Vec<String>> {
    let pos = args.iter().position(|a| a == "--worktrees")?;
    let raw = args.get(pos + 1).map(String::as_str).unwrap_or("");
    Some(
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }

    match args[1].as_str() {
        "scan" => {
            if args.len() < 3 {
                usage();
            }
            let tasks_dir = args[2].clone();
            // --worktrees overrides discovery; otherwise discover via git from tasks_dir.
            let worktrees = parse_worktrees_arg(&args)
                .unwrap_or_else(|| mt_core::discover_worktrees(&PathBuf::from(&tasks_dir)));
            match mt_core::scan_tasks(tasks_dir, worktrees) {
                Ok(nodes) => println!("{}", serde_json::to_string_pretty(&nodes).unwrap()),
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(2);
                }
            }
        }
        "create" => {
            if args.len() < 4 {
                usage();
            }
            let tasks_dir = args[2].clone();
            let name = args[3].clone();
            let opts = parse_create_opts(&args[4..]);
            match mt_core::create_task(tasks_dir, name, opts) {
                Ok(outcome) => println!(
                    "{}",
                    serde_json::to_string_pretty(&outcome.to_cli_json()).unwrap()
                ),
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(2);
                }
            }
        }
        "workspaces" => {
            // Accept multiple roots: `workspaces <dir...>` scans each and merges.
            // No dir → discover from cwd (back-compat).
            let workspaces = if args.len() >= 3 {
                args[2..]
                    .iter()
                    .flat_map(|d| mt_core::find_all_tasks_dirs_from(&PathBuf::from(d)))
                    .collect()
            } else {
                match mt_core::find_all_tasks_dirs() {
                    Ok(ws) => ws,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        process::exit(2);
                    }
                }
            };
            println!("{}", serde_json::to_string_pretty(&workspaces).unwrap());
        }
        _ => usage(),
    }
}
