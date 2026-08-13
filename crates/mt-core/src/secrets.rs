//! Secrets-брокер (спека `operations.md`, security model): `a.md` →
//! `secrets: [KEY]`, wrapper інжектить через ENV з OS keychain і **маскує у
//! виводах**. У файлах вузлів секретів немає.
//!
//! Три частини, і третя — не менш важлива за перші дві:
//!
//! 1. **Сховище** — звідки береться значення (`SecretStore`).
//! 2. **Інжекція** — значення потрапляє виконавцю лише через ENV процесу.
//! 3. **Маскування** — вивід виконавця чиститься ДО того, як потрапить у
//!    `run_NNN.md`. Без цього кроку перші два дають хибне відчуття безпеки:
//!    секрет усе одно опинився б у git, просто не з конфігу, а з логу.
//!
//! `secret:<key>`-посилання в конфігах (MCP-сервери surface — `surfaces.md`)
//! резолвляться тим самим брокером, тому токени не лежать у конфігу
//! відкритим текстом.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Префікс посилання на секрет у конфігах (`surfaces.md`).
pub const SECRET_REF_PREFIX: &str = "secret:";

/// Чим замінюється значення у виводах.
pub const MASK: &str = "***";

/// Сховище секретів.
///
/// Один метод: брокер лише читає. Запису тут свідомо немає — секрети
/// кладе людина штатним інструментом ОС, і MT не має API, яким їх можна
/// створити або переписати з боку агента.
pub trait SecretStore: Send + Sync {
    /// Значення секрета або `None`, якщо його немає.
    fn get(&self, key: &str) -> Option<String>;
}

/// Файлове сховище — fallback для Linux/headless (`stack.md`).
///
/// Формат: плоский JSON `{"KEY": "value"}`.
///
/// `Debug` навмисно не виводить значень — інакше `{:?}` у логах зводив би
/// нанівець сенс усього модуля.
pub struct FileSecretStore {
    values: BTreeMap<String, String>,
}

/// Помилка відкриття файлового сховища.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretStoreError {
    /// Файл читають не лише власник — fail closed.
    TooPermissive { path: PathBuf, mode: u32 },
    /// Файл не читається або не є JSON-обʼєктом.
    Unreadable { path: PathBuf, reason: String },
}

impl std::fmt::Display for SecretStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooPermissive { path, mode } => write!(
                f,
                "сховище секретів {} має права {mode:04o}: очікується 0600 — \
                 читати його поки не буду",
                path.display()
            ),
            Self::Unreadable { path, reason } => {
                write!(f, "сховище секретів {}: {reason}", path.display())
            }
        }
    }
}

impl FileSecretStore {
    /// Відкриває сховище, перевіряючи права доступу.
    ///
    /// Права перевіряються **до** читання і відмова жорстка: сховище, яке
    /// може прочитати будь-хто в системі, не є сховищем, і мовчки
    /// користуватись ним було б гірше, ніж не мати його зовсім.
    ///
    /// # Errors
    /// Занадто відкриті права або вміст не читається.
    pub fn open(path: &Path) -> Result<Self, SecretStoreError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(path).map_err(|error| SecretStoreError::Unreadable {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(SecretStoreError::TooPermissive {
                    path: path.to_path_buf(),
                    mode,
                });
            }
        }
        let text = std::fs::read_to_string(path).map_err(|error| SecretStoreError::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        let parsed: Value =
            serde_json::from_str(&text).map_err(|error| SecretStoreError::Unreadable {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
        let object = parsed
            .as_object()
            .ok_or_else(|| SecretStoreError::Unreadable {
                path: path.to_path_buf(),
                reason: "очікується JSON-обʼєкт {\"KEY\": \"value\"}".into(),
            })?;
        Ok(Self {
            values: object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_string()))
                })
                .collect(),
        })
    }
}

impl std::fmt::Debug for FileSecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSecretStore")
            .field("keys", &self.values.len())
            .finish()
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

/// Сховище в памʼяті — для тестів і для випадку «секретів немає».
#[derive(Default)]
pub struct MemorySecretStore {
    values: BTreeMap<String, String>,
}

impl MemorySecretStore {
    /// Створює сховище з пар.
    pub fn new(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            values: pairs.into_iter().collect(),
        }
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

/// Keychain macOS через `security(1)`.
///
/// Команда інʼєктується: без цього поведінку неможливо перевірити ніде,
/// крім macOS із заповненим keychain.
pub struct KeychainSecretStore {
    service: String,
    run: KeychainLookup,
}

/// Виклик до keychain: `(service, key) → значення`.
type KeychainLookup = Box<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

impl KeychainSecretStore {
    /// Сховище поверх системного `security find-generic-password`.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            run: Box::new(|service, key| {
                let output = std::process::Command::new("security")
                    .args(["find-generic-password", "-s", service, "-a", key, "-w"])
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let value = String::from_utf8(output.stdout).ok()?;
                let value = value.trim_end_matches('\n').to_string();
                (!value.is_empty()).then_some(value)
            }),
        }
    }

    /// Те саме з підміненим викликом — для тестів.
    pub fn with_runner(
        service: impl Into<String>,
        run: impl Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            service: service.into(),
            run: Box::new(run),
        }
    }
}

impl SecretStore for KeychainSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        (self.run)(&self.service, key)
    }
}

/// Ключі, які просить вузол (`a.md` → `secrets: [KEY]`).
pub fn requested_keys(a_md_front: &Value) -> Vec<String> {
    a_md_front
        .get("secrets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Підсумок резолюції: що інжектувати і чого забракло.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Пари ENV для процесу виконавця.
    pub env: Vec<(String, String)>,
    /// Ключі, яких немає у сховищі.
    pub missing: Vec<String>,
}

/// Резолвить ключі вузла у ENV.
///
/// Відсутній ключ **не** підставляється порожнім рядком: порожній секрет
/// виглядає як наявний і провалює виклик десь глибше, замість того щоб
/// сказати правду одразу. Тому він потрапляє у `missing`, а рішення
/// (відмовити чи запуститись) лишається викликачу.
pub fn resolve_keys(keys: &[String], store: &dyn SecretStore) -> Resolved {
    let mut env = Vec::new();
    let mut missing = Vec::new();
    for key in keys {
        match store.get(key) {
            Some(value) => env.push((key.clone(), value)),
            None => missing.push(key.clone()),
        }
    }
    Resolved { env, missing }
}

/// Резолвить `secret:<key>`-посилання в значенні конфігу (`surfaces.md`).
///
/// Значення без префікса повертається як є — конфіг лишається звичайним
/// конфігом, а брокер втручається лише там, де його явно покликали.
pub fn resolve_ref(value: &str, store: &dyn SecretStore) -> Option<String> {
    match value.strip_prefix(SECRET_REF_PREFIX) {
        Some(key) => store.get(key),
        None => Some(value.to_string()),
    }
}

/// Маскувальник виводів.
///
/// Тримає значення, а не ключі: у логах зустрічається саме значення.
#[derive(Debug, Clone, Default)]
pub struct Masker {
    values: Vec<String>,
}

impl Masker {
    /// Маскувальник для резолвлених секретів.
    pub fn new(env: &[(String, String)]) -> Self {
        let mut values: Vec<String> = env
            .iter()
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
            .collect();
        // Довші першими: інакше коротший секрет, що є підрядком довшого,
        // порізав би довший на шматки й лишив його хвіст у виводі.
        values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        values.dedup();
        Self { values }
    }

    /// Чи є що маскувати.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Замінює всі входження значень секретів на [`MASK`].
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for value in &self.values {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), MASK);
            }
        }
        out
    }
}

/// Змінна, що явно вказує файлове сховище.
///
/// Потрібна не лише тестам: у CI й контейнерах keychain-а немає, а
/// «здогадуватись за платформою» там означало б тихо лишитись без секретів.
pub const SECRETS_FILE_ENV: &str = "MT_SECRETS_FILE";

/// Дефолтне сховище платформи.
///
/// `MT_SECRETS_FILE` (якщо заданий) → файл; інакше macOS — Keychain, решта —
/// `~/.nitra/secrets.json` з правами `0600` (`stack.md`: «Linux/headless:
/// файл 0600 (fallback)»). Помилка прав доступу не ковтається: краще
/// лишитись без секретів і сказати про це, ніж мовчки читати відкритий файл.
pub fn default_store(home: &Path) -> (Box<dyn SecretStore>, Option<SecretStoreError>) {
    if let Ok(explicit) = std::env::var(SECRETS_FILE_ENV) {
        return match FileSecretStore::open(Path::new(&explicit)) {
            Ok(store) => (Box::new(store), None),
            Err(error) => (Box::new(MemorySecretStore::default()), Some(error)),
        };
    }
    if cfg!(target_os = "macos") {
        return (Box::new(KeychainSecretStore::new("mt")), None);
    }
    let path = home.join(".nitra").join("secrets.json");
    match FileSecretStore::open(&path) {
        Ok(store) => (Box::new(store), None),
        Err(SecretStoreError::Unreadable { .. }) => {
            // Файлу просто немає — штатний стан проєкту без секретів.
            (Box::new(MemorySecretStore::default()), None)
        }
        Err(error) => (Box::new(MemorySecretStore::default()), Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemorySecretStore {
        MemorySecretStore::new([
            ("STRIPE_KEY".to_string(), "sk_live_abc123".to_string()),
            ("SHORT".to_string(), "abc".to_string()),
        ])
    }

    #[test]
    fn keys_come_from_a_md() {
        let front = serde_json::json!({"secrets": ["STRIPE_KEY", "OTHER"]});
        assert_eq!(requested_keys(&front), ["STRIPE_KEY", "OTHER"]);
        assert!(requested_keys(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn missing_key_is_reported_not_blanked() {
        // Порожній секрет виглядає як наявний і провалює виклик глибше,
        // замість сказати правду одразу.
        let resolved = resolve_keys(&["STRIPE_KEY".to_string(), "ABSENT".to_string()], &store());
        assert_eq!(
            resolved.env,
            [("STRIPE_KEY".to_string(), "sk_live_abc123".to_string())]
        );
        assert_eq!(resolved.missing, ["ABSENT"]);
    }

    #[test]
    fn config_ref_resolves_only_with_prefix() {
        assert_eq!(
            resolve_ref("secret:STRIPE_KEY", &store()).as_deref(),
            Some("sk_live_abc123")
        );
        // Звичайне значення лишається звичайним значенням.
        assert_eq!(resolve_ref("npx", &store()).as_deref(), Some("npx"));
        // Посилання на невідомий ключ — не «порожньо», а відсутність.
        assert_eq!(resolve_ref("secret:ABSENT", &store()), None);
    }

    #[test]
    fn masker_removes_values_from_output() {
        let resolved = resolve_keys(&["STRIPE_KEY".to_string()], &store());
        let masker = Masker::new(&resolved.env);
        let masked = masker.apply("curl -H 'Authorization: sk_live_abc123' https://x");
        assert!(!masked.contains("sk_live_abc123"), "{masked}");
        assert!(masked.contains(MASK), "{masked}");
    }

    #[test]
    fn overlapping_secrets_are_masked_longest_first() {
        // Коротший секрет, що є підрядком довшого, інакше порізав би
        // довший і лишив його хвіст у виводі.
        let env = vec![
            ("A".to_string(), "abc".to_string()),
            ("B".to_string(), "abcdef".to_string()),
        ];
        let masked = Masker::new(&env).apply("значення abcdef тут");
        assert!(!masked.contains("def"), "{masked}");
        assert_eq!(masked, format!("значення {MASK} тут"));
    }

    #[test]
    fn empty_masker_is_identity() {
        let masker = Masker::new(&[]);
        assert!(masker.is_empty());
        assert_eq!(masker.apply("нічого не міняється"), "нічого не міняється");
    }

    #[cfg(unix)]
    #[test]
    fn permissive_file_store_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        // Сховище, яке може прочитати будь-хто в системі, не є сховищем;
        // мовчки користуватись ним гірше, ніж не мати його зовсім.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, r#"{"K": "v"}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let error = FileSecretStore::open(&path).unwrap_err();
        assert!(
            matches!(error, SecretStoreError::TooPermissive { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("0600"));
    }

    #[cfg(unix)]
    #[test]
    fn file_store_reads_when_locked_down() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        std::fs::write(&path, r#"{"K": "v"}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let store = FileSecretStore::open(&path).unwrap();
        assert_eq!(store.get("K").as_deref(), Some("v"));
        assert_eq!(store.get("MISSING"), None);
    }

    #[test]
    fn keychain_store_uses_service_and_key() {
        let store = KeychainSecretStore::with_runner("mt", |service, key| {
            (service == "mt" && key == "STRIPE_KEY").then(|| "from-keychain".to_string())
        });
        assert_eq!(store.get("STRIPE_KEY").as_deref(), Some("from-keychain"));
        assert_eq!(store.get("OTHER"), None);
    }
}
