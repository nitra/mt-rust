//! Типи мандатів `.mt/mandates.yaml` (mandates.md, «Нормативний контракт
//! (M6 фаза 0)» → «Схема `.mt/mandates.yaml`»).
//!
//! Serde-модель відповідає таблиці полів контракту один-в-один: `generation`
//! (лічильник версії файлу), `mandates[]` (записи делегування),
//! `escalates_to: null` рівно на одному кореневому записі. Структурна
//! валідність (єдиний корінь, досяжність, непорожній scope, `audacity`
//! лише для `kind: model`) — відповідальність [`crate::parse::parse_mandates`],
//! не serde: типи тут навмисно приймають ширшу множину значень, ніж дозволяє
//! контракт, щоб парсер міг повернути змістовну помилку валідації замість
//! serde-помилки десеріалізації.

use serde::{Deserialize, Serialize};

/// Корінь `.mt/mandates.yaml` — `generation` + масив мандатів.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MandatesFile {
    /// Інкрементується на 1 кожною зміною файлу; керує лише тулінг
    /// (контракт: «ручна правка відхиляється»).
    pub generation: u64,
    #[serde(default)]
    pub mandates: Vec<Mandate>,
}

/// Один запис делегування — власник, межі повноважень, ескалація.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mandate {
    /// Handle людини або ім'я моделі (`.mt/models/{model}.yaml`, без
    /// розширення).
    pub owner: String,
    #[serde(default)]
    pub kind: MandateKind,
    pub scope: Scope,
    #[serde(default)]
    pub thresholds: Thresholds,
    /// `None` лише для рівно одного кореневого мандата.
    pub escalates_to: Option<String>,
}

/// `kind: person | model` — модель як першокласний власник мандата.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandateKind {
    #[default]
    Person,
    Model,
}

/// Межі мандата за git-glob refs і типами рішень. Порожній масив =
/// порожній scope (валідатор відхиляє — «не «усе»»).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scope {
    /// Git-glob патерни в просторі `refs/mt/**`.
    #[serde(default)]
    pub refs: Vec<String>,
    /// `"*"` дозволено як «усі типи».
    #[serde(default)]
    pub decision_types: Vec<String>,
}

impl Scope {
    /// `decision_types` містить wildcard `"*"` — покриває будь-який тип.
    pub fn covers_all_decision_types(&self) -> bool {
        self.decision_types.iter().any(|t| t == "*")
    }
}

/// Пороги мандата. Відсутнє поле = без обмеження за цією віссю (найширший
/// дефолт кореневого мандата), крім `irreversible`, де відсутність = `false`
/// (консервативний дефолт контракту).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Thresholds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_eur: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    /// Відсутнє → `false` за контрактом; зберігаємо як `Option`, щоб
    /// відрізнити «не вказано» (дефолт) від явного `false` для serde
    /// round-trip, семантику дефолту застосовує [`Thresholds::irreversible_or_default`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irreversible: Option<bool>,
    /// **Лише для `kind: model`** — присутність на `kind: person` є
    /// помилкою валідації (перевіряє `parse_mandates`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audacity: Option<AudacityLevel>,
}

impl Thresholds {
    /// `irreversible` із застосованим дефолтом контракту (`false`).
    pub fn irreversible_or_default(&self) -> bool {
        self.irreversible.unwrap_or(false)
    }

    /// `audacity` із застосованим дефолтом контракту (`low`, лише для
    /// `kind: model`).
    pub fn audacity_or_default(&self) -> AudacityLevel {
        self.audacity.unwrap_or_default()
    }
}

/// Рівень ризику мандата/фасету. Порядок варіантів — зростання суворості
/// (`Ord`-похідний з `#[derive]` іде за порядком оголошення).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Бюджет зухвалості ШІ-мандата (`vision.md`, «Дельта»). `Low` — дефолт.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudacityLevel {
    #[default]
    Low,
    Medium,
    High,
}

/// Ширина ураження рішення (`decision-request`, `leverage_facets.blast_radius`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlastRadius {
    Node,
    Subtree,
    Repo,
    Company,
}

/// Чи реально розходяться варіанти рішення в наслідках
/// (`leverage_facets.divergence`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Divergence {
    #[default]
    Low,
    High,
}

/// Декларативні фасети важеля рішення — вхід маршрутизатора ескалацій і
/// мапінгу глибини квізу (mandates.md, «Крок 2 — leverage-фасети»).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeverageFacets {
    pub irreversible: bool,
    pub blast_radius: BlastRadius,
    #[serde(default)]
    pub divergence: Divergence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est_cost_eur: Option<f64>,
}

/// Глибина квіз-гейту (`NNNN-quiz.md`, поле `depth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuizDepth {
    OneTap,
    Standard,
    TeachBack,
}

/// Фактичні значення рішення, що маршрутизується — вхід
/// [`crate::lookup::effective_owner`] («facets» napi-контракту). Звіряються
/// з `thresholds` кандидата як стеля: `None` = вісь не застосовна до цього
/// рішення (не обмежує вибір).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DecisionFacets {
    pub budget_eur: Option<f64>,
    pub risk: Option<RiskLevel>,
    pub irreversible: bool,
}

/// Спільний тип вердикту для `validate_mandate_change`/`validate_approval`
/// (napi-API поверхня контракту).
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Valid,
    Invalid(String),
}

impl Verdict {
    pub fn is_valid(&self) -> bool {
        matches!(self, Verdict::Valid)
    }

    pub fn invalid(reason: impl Into<String>) -> Self {
        Verdict::Invalid(reason.into())
    }
}
