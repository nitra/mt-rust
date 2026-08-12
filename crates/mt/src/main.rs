//! `mt` — Meta-task CLI: тонкий шар над `mt-core` (без napi/subprocess).

mod commands;
mod context;
mod output;

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(
    name = "mt",
    version,
    about = "mt — Meta-task CLI (задачний граф @7n/mt)"
)]
struct Cli {
    /// Виконати команду в іншому корені проєкту.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    /// Машинний вивід (JSON) замість тексту.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ініціалізувати проєкт (за потреби) і створити нову задачу.
    Init(commands::task::InitArgs),
    /// Записати нову чернетку плану (`plan_NNN.md`).
    Plan(commands::plan::PlanArgs),
    /// Прочитати/схвалити/відхилити plan-review (`## Children`).
    Spawn(commands::plan::SpawnArgs),
    /// Записати fact (`fact_NNN.md`) — обов'язковий перед done/audit.
    Fact(commands::signal::FactArgs),
    /// Структурна самоперевірка (`## Check`) з директорії задачі.
    Verify(commands::signal::VerifyArgs),
    /// Сигнал успіху: run_NNN(success) + агрегація вгору.
    Done(commands::signal::DoneArgs),
    /// Сигнал успіху з відкриттям аудит-циклу.
    Audit(commands::signal::AuditArgs),
    /// Сигнал провалу.
    Failed(commands::signal::FailedArgs),
    /// Хост-процес: WS-сесії, discovery, orchestrator-роль.
    Serve(commands::session::ServeArgs),
    /// Інтерактивна сесія з вузлом через запущений `mt serve`.
    Attach(commands::session::AttachArgs),
    /// «Перенести сюди»: забрати вузол у поточного тримача через relay.
    Handoff(commands::session::HandoffArgs),
    /// Зупинити виконання піддерева (running-маркери, від листів).
    Stop(commands::lifecycle::StopArgs),
    /// Вердикт аудитора: закриває цикл (`audit-result_NNN.md`).
    Verdict(commands::audit::VerdictArgs),
    /// Запит уточнення від аудитора — не вердикт, цикл лишається відкритим.
    Clarify(commands::audit::ClarifyArgs),
    /// Відповідь виконавця на уточнення аудитора.
    Amend(commands::audit::AmendArgs),
    /// Запустити вузол (claim → worktree → виконавець → publish).
    Run(commands::run::RunArgs),
    /// Один прохід автопілота по всіх waiting agent-вузлах.
    Auto(commands::run::AutoArgs),
    /// Вбити вузол (і каскадно нащадків).
    Kill(commands::lifecycle::KillArgs),
    /// Інвалідувати вузол (архівувати version chain).
    Invalidate(commands::lifecycle::InvalidateArgs),
    /// Статус графа задач (+ cost ledger).
    Status(commands::graph::StatusArgs),
    /// Сирий скан дерева задач.
    Scan(commands::graph::ScanArgs),
    /// One-shot attention/CI-гейт (колишній `watch`).
    Check(commands::check::CheckArgs),
    /// Керування developer git-worktree.
    Worktree(commands::worktree::WorktreeArgs),
    /// Діагностика `.mt.json`/`mt/`/git-стану.
    Doctor(commands::doctor::DoctorArgs),
    /// Згенерувати shell-completion скрипт.
    Completions { shell: Shell },
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    if let Err(e) = context::apply_root(&cli.root) {
        output::fail_result(json, e);
    }

    let result = match cli.command {
        Command::Init(args) => commands::task::run(args, json),
        Command::Plan(args) => commands::plan::run_plan(args, json),
        Command::Spawn(args) => commands::plan::run_spawn(args, json),
        Command::Fact(args) => commands::signal::run_fact(args, json),
        Command::Verify(args) => commands::signal::run_verify(args, json),
        Command::Done(args) => commands::signal::run_done(args, json),
        Command::Audit(args) => commands::signal::run_audit(args, json),
        Command::Failed(args) => commands::signal::run_failed(args, json),
        Command::Serve(args) => commands::session::run_serve_cmd(args, json),
        Command::Attach(args) => commands::session::run_attach_cmd(args, json),
        Command::Handoff(args) => commands::session::run_handoff_cmd(args, json),
        Command::Stop(args) => commands::lifecycle::run_stop(args, json),
        Command::Verdict(args) => commands::audit::run_verdict(args, json),
        Command::Clarify(args) => commands::audit::run_clarify(args, json),
        Command::Amend(args) => commands::audit::run_amend(args, json),
        Command::Run(args) => commands::run::run(args, json),
        Command::Auto(args) => commands::run::auto(args, json),
        Command::Kill(args) => commands::lifecycle::run_kill(args, json),
        Command::Invalidate(args) => commands::lifecycle::run_invalidate(args, json),
        Command::Status(args) => commands::graph::run_status(args, json),
        Command::Scan(args) => commands::graph::run_scan(args, json),
        Command::Check(args) => commands::check::run(args, json), // process::exit усередині
        Command::Worktree(args) => commands::worktree::run(args, json),
        Command::Doctor(args) => commands::doctor::run(args, json), // process::exit усередині
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "mt", &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(e) = result {
        output::fail_result(json, e);
    }
}
