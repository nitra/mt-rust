//! `mt escalate` / `mt decide` — розвилка замість глухого `unresolvable`
//! (спека `mandates.md`, «Артефакт `decision-request`»).
//!
//! Інваріант, який тут матеріалізується: до людини не доходить «run
//! failed» — драбина це вже покриває; доповзає лише **вибір**, коли
//! драбину вичерпано, а причина — не баг.
//!
//! Маршрутизація адресата — `mt-mandates` (`effective_owner`): не «кому
//! зручно», а кому карта мандатів дає право вирішити.

use std::path::{Path, PathBuf};

use mt_core::decision::{
    answer_decision, chosen_option, open_decision, retry_history, write_decision_request,
    DecisionAnswer, DecisionRequest,
};
use mt_mandates::{effective_owner, parse_mandates, DecisionFacets, LookupError, RiskLevel};

/// Аргументи `mt escalate`.
#[derive(Debug, clap::Args)]
pub struct EscalateArgs {
    /// Вузол (шлях у tasks-директорії).
    pub node: String,
    /// Тип рішення для маршрутизації (`architecture`, `ops`, …).
    #[arg(long, default_value = "ops")]
    pub decision_type: String,
    /// Рішення незворотне (фасет важеля).
    #[arg(long)]
    pub irreversible: bool,
    /// Оцінка бюджету рішення, EUR.
    #[arg(long)]
    pub budget_eur: Option<f64>,
    /// Рівень ризику: low | medium | high.
    #[arg(long)]
    pub risk: Option<String>,
    /// Ціна зволікання людськими словами.
    #[arg(long, default_value = "")]
    pub deadline_cost: String,
    /// Хто спакував розвилку (агент escalation-intake).
    #[arg(long, default_value = "escalation-intake")]
    pub recommended_by: String,
    /// Файл із тілом розвилки (контекст, варіанти, рекомендація).
    #[arg(long)]
    pub body_file: Option<PathBuf>,
}

/// Аргументи `mt decide`.
#[derive(Debug, clap::Args)]
pub struct DecideArgs {
    /// Вузол (шлях у tasks-директорії).
    pub node: String,
    /// Обраний варіант (`A`, `B`, …).
    #[arg(long)]
    pub option: String,
    /// Handle того, хто вирішив.
    #[arg(long)]
    pub by: String,
    /// Base64 Ed25519-підпису акта.
    #[arg(long, default_value = "")]
    pub signature: String,
}

/// Тека вузла в межах tasks-дерева.
fn node_dir(node: &str) -> PathBuf {
    Path::new("mt").join(node)
}

/// Рівень ризику з рядка CLI.
fn risk_level(raw: Option<&str>) -> Result<Option<RiskLevel>, String> {
    match raw {
        None => Ok(None),
        Some("low") => Ok(Some(RiskLevel::Low)),
        Some("medium") => Ok(Some(RiskLevel::Medium)),
        Some("high") => Ok(Some(RiskLevel::High)),
        Some(other) => Err(format!("невідомий рівень ризику: {other}")),
    }
}

/// `mt escalate <node>` — спакувати розвилку і поставити вузол в
/// `awaiting-decision`.
///
/// # Errors
/// Немає карти мандатів, немає адресата, помилки запису.
pub fn run_escalate(args: EscalateArgs, json: bool) -> Result<(), String> {
    let dir = node_dir(&args.node);
    if !dir.is_dir() {
        return Err(format!("вузол {} не знайдено", args.node));
    }
    let mandates = parse_mandates(Path::new(".mt/mandates.yaml"))
        .map_err(|error| format!("карта мандатів недоступна: {error}"))?;

    let facets = DecisionFacets {
        budget_eur: args.budget_eur,
        risk: risk_level(args.risk.as_deref())?,
        irreversible: args.irreversible,
    };
    // `refs/mt/tasks/**` — простір, у якому мандати задають scope; вузол
    // адресується так само, як у карті, інакше lookup не зійдеться.
    let node_ref = format!("refs/mt/tasks/{}", args.node);
    let owner =
        effective_owner(&mandates, &node_ref, &args.decision_type, &facets).map_err(|error| {
            match error {
                LookupError::NoMatch => format!(
                    "жоден мандат не покриває {node_ref} / {} у межах порогів — \
                 розвилку нема кому адресувати",
                    args.decision_type
                ),
                other => format!("маршрутизація не вдалась: {other:?}"),
            }
        })?;

    let body = match &args.body_file {
        Some(path) => std::fs::read_to_string(path).map_err(|error| error.to_string())?,
        None => "## Контекст\n\n## Варіанти\n\n## Рекомендація агента\n".to_string(),
    };
    let request = DecisionRequest {
        mandate_generation: mandates.generation,
        computed_owner: owner.owner.clone(),
        escalation_chain: owner.escalation_chain.clone(),
        // Історія — з run-файлів вузла, а не зі слів того, хто ескалює:
        // «драбину вичерпано» має бути доказом, а не заявою.
        retry_history: retry_history(&dir),
        leverage_facets: serde_json::json!({
            "irreversible": args.irreversible,
            "risk": args.risk,
            "budget_eur": args.budget_eur,
        }),
        deadline_cost: args.deadline_cost.clone(),
        recommended_by: args.recommended_by.clone(),
        body,
    };
    let nnnn = write_decision_request(&dir, &dir, &request).map_err(|error| error.to_string())?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "node": args.node,
                "decision": nnnn,
                "computed_owner": owner.owner,
                "escalation_chain": owner.escalation_chain,
            })
        );
    } else {
        println!(
            "розвилка {nnnn:04} у {}: адресат {} (ланцюг: {})",
            args.node,
            owner.owner,
            owner.escalation_chain.join(" → ")
        );
    }
    Ok(())
}

/// `mt decide <node> --option B` — відповідь власника, що закриває розвилку.
///
/// # Errors
/// Немає відкритої розвилки або помилки запису.
pub fn run_decide(args: DecideArgs, json: bool) -> Result<(), String> {
    let dir = node_dir(&args.node);
    let Some(nnnn) = open_decision(&dir) else {
        return Err(format!("у вузла {} немає відкритої розвилки", args.node));
    };
    let answer = DecisionAnswer {
        chosen_option: args.option.clone(),
        decided_by: args.by.clone(),
        signature: args.signature.clone(),
        decided_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    answer_decision(&dir, &dir, nnnn, &answer)?;

    let chosen = chosen_option(&dir, nnnn).unwrap_or_else(|| args.option.clone());
    if json {
        println!(
            "{}",
            serde_json::json!({ "node": args.node, "decision": nnnn, "chosen_option": chosen })
        );
    } else {
        println!("розвилка {nnnn:04} у {}: обрано {chosen}", args.node);
    }
    Ok(())
}
