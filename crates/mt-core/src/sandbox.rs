//! Sandbox-профілі скілів (спека `operations.md`, security model):
//! «skill → профіль у `skill_profiles`: allowlist команд, network (off за
//! замовчуванням), fs-scope (worktree). Команда поза allowlist → відмова».
//!
//! **Головне рішення модуля — де саме проходить межа сумісності.** Deny-by-
//! default усередині налаштованих профілів — так; deny-by-default для
//! проєкту, який `skill_profiles` не налаштував, — ні. Друге зламало б
//! кожен наявний вузол мовчазною відмовою, і «безпека» звелася б до того,
//! що її вимикають назад. Тому:
//!
//! - секції немає → політика **не enforcing**, поведінка як до її появи;
//! - секція є → у її межах allowlist жорсткий, а `network` вимкнено, доки
//!   його не ввімкнули явно.
//!
//! Це видно з коду ([`Policy::is_enforcing`]), а не лише з коментаря.
//!
//! **Межа виконання, названа чесно.** Автономний хід виконує підписочний
//! CLI власним процесом — MT не перехоплює кожен його syscall і не вдає, що
//! перехоплює. Політика тут: (а) жорстко застосовується там, де агент
//! **питає** дозволу (ACP `session/request_permission`), і (б) експортується
//! у ENV виконавця. Пісочниця рівня ядра ОС — окрема задача, і її
//! відсутність не сховано за словом «sandbox».

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ENV із дозволеними програмами (через кому) для виконавця.
pub const ENV_ALLOW: &str = "MT_SKILL_ALLOW";

/// ENV із дозволом на мережу (`1`/`0`).
pub const ENV_NETWORK: &str = "MT_SKILL_NETWORK";

/// Куди скіл має право писати.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsScope {
    /// Лише worktree run-а — дефолт зі спеки.
    #[default]
    Worktree,
    /// Без обмеження (свідоме послаблення в конфігу).
    Unrestricted,
}

/// Профіль одного скіла.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillProfile {
    /// Дозволені програми (перше слово команди).
    pub allow: Vec<String>,
    /// Чи має скіл доступ до мережі. `false` за замовчуванням — вимога спеки.
    pub network: bool,
    /// Межа файлових операцій.
    pub fs_scope: FsScope,
}

impl Default for SkillProfile {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            network: false,
            fs_scope: FsScope::Worktree,
        }
    }
}

/// Чому дію відхилено.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// Програма не в allowlist жодного зі скілів вузла.
    Command { program: String },
    /// Шлях виходить за межі worktree.
    OutsideWorktree { path: PathBuf },
    /// Мережа вимкнена для всіх скілів вузла.
    Network,
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command { program } => write!(
                f,
                "sandbox: команда `{program}` поза allowlist скілів вузла — відмова"
            ),
            Self::OutsideWorktree { path } => write!(
                f,
                "sandbox: шлях {} за межами worktree — відмова",
                path.display()
            ),
            Self::Network => write!(f, "sandbox: мережа вимкнена для скілів вузла — відмова"),
        }
    }
}

/// Ефективна політика вузла — обʼєднання профілів його скілів.
///
/// Саме обʼєднання, а не перетин: `skills: [bash, web-search]` означає, що
/// вузлу потрібні обидва набори можливостей. Звуження робить не цей рівень,
/// а перелік скілів у `a.md` — він і є стеля вузла.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    allow: BTreeSet<String>,
    network: bool,
    fs_scope: FsScope,
    enforcing: bool,
}

impl Policy {
    /// Політика, що нічого не перевіряє — для проєктів без `skill_profiles`.
    pub fn permissive() -> Self {
        Self {
            allow: BTreeSet::new(),
            network: true,
            fs_scope: FsScope::Unrestricted,
            enforcing: false,
        }
    }

    /// Чи політика взагалі щось забороняє.
    pub fn is_enforcing(&self) -> bool {
        self.enforcing
    }

    /// Чи дозволена мережа.
    pub fn network(&self) -> bool {
        self.network
    }

    /// Дозволені програми — вміст [`ENV_ALLOW`].
    pub fn allowed_programs(&self) -> Vec<String> {
        self.allow.iter().cloned().collect()
    }

    /// ENV-пари для процесу виконавця.
    ///
    /// Порожньо для не-enforcing політики: змінна, що нічого не обмежує,
    /// лише вводила б виконавця в оману.
    pub fn env(&self) -> Vec<(String, String)> {
        if !self.enforcing {
            return Vec::new();
        }
        vec![
            (ENV_ALLOW.to_string(), self.allowed_programs().join(",")),
            (
                ENV_NETWORK.to_string(),
                if self.network { "1" } else { "0" }.to_string(),
            ),
        ]
    }

    /// Перевіряє команду за allowlist.
    ///
    /// Звіряється **перше слово** — програма. Перевіряти цілий рядок було б
    /// гірше за відсутність перевірки: `git status` пройшов би, а
    /// `git status --short` — ні, і allowlist перетворився б на список
    /// точних заклинань.
    ///
    /// # Errors
    /// Програма поза allowlist.
    pub fn check_command(&self, command: &str) -> Result<(), Denied> {
        if !self.enforcing {
            return Ok(());
        }
        let program = program_of(command);
        if program.is_empty() || self.allow.contains(&program) {
            return Ok(());
        }
        Err(Denied::Command { program })
    }

    /// Перевіряє шлях проти fs-scope.
    ///
    /// Порівнюються **нормалізовані** шляхи: інакше `../../etc/passwd`
    /// проходив би текстову перевірку, залишаючись виходом за межі.
    ///
    /// # Errors
    /// Шлях за межами worktree.
    pub fn check_path(&self, worktree: &Path, path: &Path) -> Result<(), Denied> {
        if !self.enforcing || self.fs_scope == FsScope::Unrestricted {
            return Ok(());
        }
        let candidate = if path.is_absolute() {
            normalize(path)
        } else {
            normalize(&worktree.join(path))
        };
        if candidate.starts_with(normalize(worktree)) {
            return Ok(());
        }
        Err(Denied::OutsideWorktree { path: candidate })
    }

    /// Перевіряє доступ до мережі.
    ///
    /// # Errors
    /// Мережа вимкнена.
    pub fn check_network(&self) -> Result<(), Denied> {
        if !self.enforcing || self.network {
            return Ok(());
        }
        Err(Denied::Network)
    }
}

/// Перше слово команди без шляху: `/usr/bin/git` → `git`.
fn program_of(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .map(|word| {
            Path::new(word).file_name().map_or_else(
                || word.to_string(),
                |name| name.to_string_lossy().to_string(),
            )
        })
        .unwrap_or_default()
}

/// Нормалізує шлях лексично (без звернення до ФС — шляху може ще не бути).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Профілі з конфігу проєкту.
pub fn skill_profiles(config: &Value) -> Option<BTreeMap<String, SkillProfile>> {
    let map = config.get("skill_profiles")?.as_object()?;
    Some(
        map.iter()
            .map(|(name, raw)| {
                (
                    name.clone(),
                    serde_json::from_value(raw.clone()).unwrap_or_default(),
                )
            })
            .collect(),
    )
}

/// Ефективна політика вузла зі списку його скілів.
///
/// Секції в конфігу немає → [`Policy::permissive`]: проєкт, який sandbox не
/// налаштовував, працює як раніше.
pub fn policy_for(config: &Value, skills: &[String]) -> Policy {
    let Some(profiles) = skill_profiles(config) else {
        return Policy::permissive();
    };
    let mut policy = Policy {
        allow: BTreeSet::new(),
        network: false,
        fs_scope: FsScope::Worktree,
        enforcing: true,
    };
    for skill in skills {
        let Some(profile) = profiles.get(skill) else {
            // Скіл без профілю не додає можливостей — але й не забороняє
            // те, що дали інші скіли вузла.
            continue;
        };
        policy.allow.extend(profile.allow.iter().cloned());
        policy.network |= profile.network;
        if profile.fs_scope == FsScope::Unrestricted {
            policy.fs_scope = FsScope::Unrestricted;
        }
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Value {
        serde_json::json!({
            "skill_profiles": {
                "bash": {"allow": ["git", "cargo"], "network": false},
                "web-search": {"allow": ["curl"], "network": true},
                "write-files": {"fs_scope": "worktree"}
            }
        })
    }

    fn skills(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn project_without_section_is_not_enforcing() {
        // Межа сумісності: deny-by-default для ненастроєного проєкту зламав
        // би кожен наявний вузол, і «безпека» звелася б до її вимикання.
        let policy = policy_for(&serde_json::json!({}), &skills(&["bash"]));
        assert!(!policy.is_enforcing());
        assert!(policy.check_command("rm -rf /").is_ok());
        assert!(
            policy.env().is_empty(),
            "не-enforcing політика не бреше ENV"
        );
    }

    #[test]
    fn command_outside_allowlist_is_denied() {
        let policy = policy_for(&config(), &skills(&["bash"]));
        assert!(policy.check_command("git status --short").is_ok());
        let denied = policy.check_command("curl https://x").unwrap_err();
        assert_eq!(
            denied,
            Denied::Command {
                program: "curl".into()
            }
        );
        assert!(denied.to_string().contains("curl"));
    }

    #[test]
    fn allowlist_matches_program_not_whole_line() {
        // Перевірка цілого рядка перетворила б allowlist на список точних
        // заклинань: `git status` пройшов би, `git status --short` — ні.
        let policy = policy_for(&config(), &skills(&["bash"]));
        assert!(policy
            .check_command("git commit -m 'текст із пробілами'")
            .is_ok());
        // Абсолютний шлях до дозволеної програми — теж дозволений.
        assert!(policy.check_command("/usr/bin/git log").is_ok());
    }

    #[test]
    fn skills_union_their_profiles() {
        // Вузлу з двома скілами потрібні обидва набори можливостей;
        // звужує перелік скілів у `a.md`, а не цей рівень.
        let policy = policy_for(&config(), &skills(&["bash", "web-search"]));
        assert!(policy.check_command("cargo test").is_ok());
        assert!(policy.check_command("curl https://x").is_ok());
        assert!(policy.network());
    }

    #[test]
    fn network_is_off_until_a_skill_enables_it() {
        let policy = policy_for(&config(), &skills(&["bash"]));
        assert!(!policy.network());
        assert_eq!(policy.check_network().unwrap_err(), Denied::Network);

        let with_search = policy_for(&config(), &skills(&["bash", "web-search"]));
        assert!(with_search.check_network().is_ok());
    }

    #[test]
    fn unknown_skill_adds_nothing_and_forbids_nothing() {
        let policy = policy_for(&config(), &skills(&["bash", "невідомий"]));
        assert!(policy.check_command("git status").is_ok());
        assert!(policy.check_command("curl x").is_err());
    }

    #[test]
    fn fs_scope_rejects_traversal_out_of_worktree() {
        // Нормалізація, а не текстове порівняння: інакше `../..` проходив
        // би перевірку, лишаючись виходом за межі.
        let policy = policy_for(&config(), &skills(&["write-files"]));
        let worktree = Path::new("/repo/.worktrees/run-1");

        assert!(policy
            .check_path(worktree, Path::new("src/main.rs"))
            .is_ok());
        let denied = policy
            .check_path(worktree, Path::new("../../../etc/passwd"))
            .unwrap_err();
        assert!(
            matches!(denied, Denied::OutsideWorktree { .. }),
            "{denied:?}"
        );
        assert!(policy
            .check_path(worktree, Path::new("/etc/passwd"))
            .is_err());
    }

    #[test]
    fn unrestricted_scope_is_an_explicit_opt_out() {
        let config = serde_json::json!({
            "skill_profiles": {"admin": {"allow": ["sh"], "fs_scope": "unrestricted"}}
        });
        let policy = policy_for(&config, &skills(&["admin"]));
        assert!(policy.is_enforcing(), "команди все одно за allowlist");
        assert!(policy
            .check_path(Path::new("/repo"), Path::new("/etc/hosts"))
            .is_ok());
        assert!(policy.check_command("rm -rf /").is_err());
    }

    #[test]
    fn env_exports_policy_for_the_executor() {
        let policy = policy_for(&config(), &skills(&["bash"]));
        let env = policy.env();
        assert!(
            env.contains(&(ENV_ALLOW.to_string(), "cargo,git".to_string())),
            "{env:?}"
        );
        assert!(
            env.contains(&(ENV_NETWORK.to_string(), "0".to_string())),
            "{env:?}"
        );
    }
}
