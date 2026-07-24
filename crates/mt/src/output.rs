//! Уніфікований вивід команд: `--json` (serde_json pretty) або текст, і
//! єдина точка виходу з помилкою (stderr, exit 1) — без emoji-стилю
//! старого JS CLI (спека: пріоритет скриптовності над людяністю виводу).

use serde::Serialize;

pub fn json<T: Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\":{e:?}}}"))
    );
}

/// Друкує результат у форматі `--json`, або викликає `text` для людського
/// виводу. Повертає `()` — виклики завершують `main` через `?`/`process::exit`.
pub fn emit<T: Serialize>(as_json: bool, value: &T, text: impl FnOnce(&T)) {
    if as_json {
        json(value);
    } else {
        text(value);
    }
}

/// Виводить помилку і завершує з кодом 1; у `--json`-режимі — як
/// `{"error": "..."}` на stdout (скрипти парсять stdout, не stderr).
pub fn fail_result(as_json: bool, message: impl std::fmt::Display) -> ! {
    if as_json {
        json(&serde_json::json!({ "error": message.to_string() }));
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1);
}
