//! Directory: мапінг handle → PII (`.mt/directory.json`, git-ignored).
//!
//! PII-політика (operations.md): у git-файлах вузлів живуть лише handles
//! (`assignee: vkozlov`, `owner: olena`, `from`/`to` ескалацій) — email та
//! імʼя людини лишаються поза історією, у локальному `.mt/directory.json`
//! і на relay (`accounts.email`). Цей модуль — канонічний парсер файлу;
//! читання ФС лишається на боці викликача (як у `config`).
//!
//! Формат — плоский обʼєкт: значення або рядок-email, або обʼєкт
//! `{ "email": "...", "name": "..." }`:
//!
//! ```json
//! {
//!   "vkozlov": "v.kozlov@example.com",
//!   "olena": { "email": "olena@example.com", "name": "Олена" }
//! }
//! ```

use std::collections::HashMap;

use serde_json::Value;

/// Канонічний шлях directory-файлу відносно кореня репо.
pub const DIRECTORY_PATH: &str = ".mt/directory.json";

/// PII одного handle: email — ключ мапінгу на relay-акаунт, імʼя — display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub email: String,
    pub name: Option<String>,
}

/// Розбирає сирий текст `.mt/directory.json` у мапінг handle → PII.
/// Відсутній файл (`None`), битий JSON чи не-обʼєкт → порожній мапінг
/// (нерозмічена directory — штатний стан, не помилка). Невалідні значення
/// (без email) пропускаються.
pub fn parse_directory(raw: Option<&str>) -> HashMap<String, DirectoryEntry> {
    let Some(raw) = raw else {
        return HashMap::new();
    };
    let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(raw) else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .filter_map(|(handle, value)| {
            let entry = match value {
                Value::String(email) if !email.trim().is_empty() => DirectoryEntry {
                    email: email.trim().to_string(),
                    name: None,
                },
                Value::Object(fields) => DirectoryEntry {
                    email: fields.get("email")?.as_str()?.trim().to_string(),
                    name: fields
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
                _ => return None,
            };
            (!entry.email.is_empty()).then_some((handle, entry))
        })
        .collect()
}

/// Email за handle-ом (None — handle поза directory: емітер не може
/// резолвити адресний push, події їдуть без `to_account_id`).
pub fn resolve_email<'a>(
    directory: &'a HashMap<String, DirectoryEntry>,
    handle: &str,
) -> Option<&'a str> {
    directory.get(handle).map(|entry| entry.email.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_object_entries() {
        let raw = r#"{
            "vkozlov": "v.kozlov@example.com",
            "olena": { "email": " olena@example.com ", "name": "Олена" },
            "broken": { "name": "без email" },
            "empty": "  "
        }"#;
        let directory = parse_directory(Some(raw));
        assert_eq!(directory.len(), 2);
        assert_eq!(
            resolve_email(&directory, "vkozlov"),
            Some("v.kozlov@example.com")
        );
        assert_eq!(
            directory.get("olena"),
            Some(&DirectoryEntry {
                email: "olena@example.com".into(),
                name: Some("Олена".into())
            })
        );
        assert_eq!(resolve_email(&directory, "broken"), None);
    }

    #[test]
    fn missing_or_invalid_file_is_empty_mapping() {
        assert!(parse_directory(None).is_empty());
        assert!(parse_directory(Some("не json")).is_empty());
        assert!(parse_directory(Some("[1,2]")).is_empty());
    }
}
