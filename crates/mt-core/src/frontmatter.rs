//! YAML front-matter parser/serializer для mt task-файлів.
//!
//! Порт `npm/lib/core/frontmatter.mjs` 1:1 — включно з «дивними» кутовими
//! випадками парсера, бо JS-обгортка тепер делегує сюди, а вихід
//! `serialize_yaml` має лишатися **байт-у-байт** ідентичним JS-версії.
//! Ключі зберігають порядок вставки (`serde_json` із `preserve_order`).

use serde_json::{Map, Value};

/// Спецсимволи YAML, що вимагають лапок (JS `YAML_SPECIAL_RE`).
const YAML_SPECIAL: &[char] = &[':', '#', '[', ']', '{', '}', ',', '\n'];

/// Розбиває текст на `(кінець збігу front-matter, внутрішній блок)`.
/// Еквівалент JS `/^---\r?\n([\s\S]*?)\r?\n---/`.
fn split_frontmatter(text: &str) -> Option<(usize, &str)> {
    let rest = text.strip_prefix("---")?;
    let nl_len = if rest.starts_with("\r\n") {
        2
    } else if rest.starts_with('\n') {
        1
    } else {
        return None;
    };
    let after_open = &rest[nl_len..];
    let idx = after_open.find("\n---")?;
    let inner_end = if after_open[..idx].ends_with('\r') {
        idx - 1
    } else {
        idx
    };
    let match_end = 3 + nl_len + idx + 4;
    Some((match_end, &after_open[..inner_end]))
}

/// Парсить YAML front-matter з markdown-тексту. Без front-matter → порожній об'єкт.
pub fn parse_front_matter(text: &str) -> Value {
    match split_frontmatter(text) {
        Some((_, inner)) => Value::Object(parse_yaml_block(inner)),
        None => Value::Object(Map::new()),
    }
}

/// Парсить чистий YAML-блок (без `---`-маркерів) — напр. `.mt-claim.yml`.
pub fn parse_yaml(text: &str) -> Value {
    Value::Object(parse_yaml_block(text))
}

/// Повертає тіло документа (без front-matter, з обрізаним лівим whitespace).
pub fn get_body(text: &str) -> String {
    match split_frontmatter(text) {
        Some((end, _)) => text[end..].trim_start().to_string(),
        None => text.to_string(),
    }
}

/// Кількість пробілів на початку рядка.
fn get_indent(line: &str) -> usize {
    line.bytes().take_while(|b| *b == b' ').count()
}

/// JS `line.slice(n)` по символах (для нормалізації відступу вкладених блоків).
fn slice_chars(line: &str, n: usize) -> String {
    line.chars().skip(n).collect()
}

fn parse_yaml_block(block: &str) -> Map<String, Value> {
    let lines: Vec<&str> = block
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut result = Map::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            i += 1;
            continue;
        }

        if get_indent(line) > 0 {
            // Верхній рівень — пропускаємо «бродячі» дочірні рядки.
            i += 1;
            continue;
        }

        let Some(colon_idx) = line.find(':') else {
            i += 1;
            continue;
        };

        let key = line[..colon_idx].trim().to_string();
        let raw_val = line[colon_idx + 1..].trim();

        if !raw_val.is_empty() {
            result.insert(key, parse_scalar(raw_val));
            i += 1;
            continue;
        }

        // Значення відсутнє після ':' — дивимось наступні рядки.
        i += 1;
        if i >= lines.len() {
            result.insert(key, Value::Null);
            continue;
        }

        let next_line = lines[i];
        if next_line.trim().is_empty() {
            result.insert(key, Value::Null);
            continue;
        }

        let next_indent = get_indent(next_line);
        if next_indent == 0 {
            result.insert(key, Value::Null);
            continue;
        }

        if next_line.trim_start().starts_with("- ") {
            // Список.
            let mut arr = vec![];
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() {
                    i += 1;
                    continue;
                }
                if get_indent(l) == 0 {
                    break;
                }
                let t = l.trim_start();
                if let Some(item) = t.strip_prefix("- ") {
                    arr.push(parse_scalar(item.trim()));
                }
                i += 1;
            }
            result.insert(key, Value::Array(arr));
        } else {
            // Вкладений об'єкт: нормалізуємо відступ (видаляємо перший рівень).
            let mut child_lines = vec![];
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() {
                    i += 1;
                    continue;
                }
                if get_indent(l) == 0 {
                    break;
                }
                child_lines.push(slice_chars(l, next_indent));
                i += 1;
            }
            result.insert(
                key,
                Value::Object(parse_yaml_block(&child_lines.join("\n"))),
            );
        }
    }

    result
}

/// JS `Number(s)` для обрізаного непорожнього рядка → serde-число.
/// `Infinity`/`NaN` не представні в JSON → `None` (значення лишиться рядком).
fn js_number(s: &str) -> Option<Value> {
    let parse_radix = |digits: &str, radix: u32| -> Option<f64> {
        if digits.is_empty() {
            return None;
        }
        u128::from_str_radix(digits, radix).ok().map(|v| v as f64)
    };
    let lower = s.get(..2).map(str::to_ascii_lowercase);
    let n: f64 = match lower.as_deref() {
        Some("0x") => parse_radix(&s[2..], 16)?,
        Some("0o") => parse_radix(&s[2..], 8)?,
        Some("0b") => parse_radix(&s[2..], 2)?,
        _ => {
            // Rust приймає "inf"/"nan" — JS Number ні (лише "Infinity", який пропускаємо).
            if s.chars()
                .any(|c| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E'))
            {
                return None;
            }
            s.parse().ok()?
        }
    };
    if !n.is_finite() {
        return None;
    }
    // Цілі в безпечному діапазоні зберігаємо як int — серіалізація як у JS String(n).
    if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0 {
        return Some(Value::from(n as i64));
    }
    serde_json::Number::from_f64(n).map(Value::Number)
}

/// Парсить скалярне значення: булеве, null, число, лапки, або рядок.
fn parse_scalar(s: &str) -> Value {
    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "~" => return Value::Null,
        _ => {}
    }
    if let Some(n) = js_number(s) {
        return n;
    }
    // Знімаємо лапки.
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= 2 {
        let (first, last) = (chars[0], chars[chars.len() - 1]);
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return Value::String(chars[1..chars.len() - 1].iter().collect());
        }
    }
    Value::String(s.to_string())
}

/// Серіалізує об'єкт у YAML-рядок (без `---` маркерів). Байт-у-байт як JS
/// `serializeYaml`: scalar, масиви (`  - item`), вкладені об'єкти.
pub fn serialize_yaml(obj: &Value, indent_level: usize) -> String {
    let indent = "  ".repeat(indent_level);
    let mut lines: Vec<String> = vec![];

    if let Value::Object(map) = obj {
        for (key, val) in map {
            match val {
                Value::Null => lines.push(format!("{indent}{key}:")),
                Value::Array(items) => {
                    lines.push(format!("{indent}{key}:"));
                    for item in items {
                        lines.push(format!("{indent}  - {}", serialize_scalar(item)));
                    }
                }
                Value::Object(_) => {
                    lines.push(format!("{indent}{key}:"));
                    lines.push(serialize_yaml(val, indent_level + 1));
                }
                _ => lines.push(format!("{indent}{key}: {}", serialize_scalar(val))),
            }
        }
    }

    lines.join("\n")
}

/// Серіалізує скалярне значення у рядок (JS `serializeScalar` + `String(val)`).
fn serialize_scalar(val: &Value) -> String {
    match val {
        Value::String(s) => {
            if s.contains(YAML_SPECIAL) || s.trim() != s {
                format!("\"{}\"", s.replace('"', "\\\""))
            } else {
                s.clone()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                format!("{}", n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

/// Будує markdown-файл із front-matter і тілом: `---\n<yaml>\n---\n\n<body>`.
pub fn build_markdown(fm: &Value, body: &str) -> String {
    let yaml = serialize_yaml(fm, 0);
    ["---", &yaml, "---", "", body].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_simple_frontmatter() {
        let fm = parse_front_matter("---\nschema_version: 1\nhint: atomic\n---\n\nbody");
        assert_eq!(fm, json!({"schema_version": 1, "hint": "atomic"}));
    }

    #[test]
    fn no_frontmatter_gives_empty_object() {
        assert_eq!(parse_front_matter("just text"), json!({}));
        assert_eq!(get_body("just text"), "just text");
    }

    #[test]
    fn get_body_strips_frontmatter_and_leading_ws() {
        assert_eq!(get_body("---\na: 1\n---\n\n## Body\n"), "## Body\n");
    }

    #[test]
    fn parses_lists_and_nested_objects() {
        let text =
            "---\nskills:\n  - bash\n  - write-files\nexecutor:\n  mode: agent\n  tier: MAX\n---\n";
        let fm = parse_front_matter(text);
        assert_eq!(
            fm,
            json!({
                "skills": ["bash", "write-files"],
                "executor": {"mode": "agent", "tier": "MAX"}
            })
        );
    }

    #[test]
    fn parses_scalars_like_js() {
        let fm = parse_front_matter(
            "---\nnum: 42\nfloat: 1.5\nyes: true\nno: false\nnil: null\ntilde: ~\nquoted: \"a: b\"\n---\n",
        );
        assert_eq!(
            fm,
            json!({
                "num": 42, "float": 1.5, "yes": true, "no": false,
                "nil": null, "tilde": null, "quoted": "a: b"
            })
        );
    }

    #[test]
    fn crlf_frontmatter() {
        let fm = parse_front_matter("---\r\na: 1\r\n---\r\nbody");
        assert_eq!(fm, json!({"a": 1}));
    }

    #[test]
    fn serialize_yaml_matches_js_bytes() {
        let obj = json!({
            "schema_version": 1,
            "created_at": "2026-06-14T00:00:00Z",
            "budget_sec": 1800,
            "hint": "atomic",
            "note": null,
            "skills": ["bash", "write-files"],
            "nested": {"a": 1, "b": "x y"}
        });
        // Часові мітки містять ':' → JS-версія теж бере їх у лапки.
        assert_eq!(
            serialize_yaml(&obj, 0),
            "schema_version: 1\ncreated_at: \"2026-06-14T00:00:00Z\"\nbudget_sec: 1800\nhint: atomic\nnote:\nskills:\n  - bash\n  - write-files\nnested:\n  a: 1\n  b: x y"
        );
    }

    #[test]
    fn serialize_quotes_special_chars() {
        let obj = json!({"a": "x: y", "b": " pad ", "c": "q\"q"});
        assert_eq!(
            serialize_yaml(&obj, 0),
            "a: \"x: y\"\nb: \" pad \"\nc: q\"q"
        );
    }

    #[test]
    fn build_markdown_layout() {
        let md = build_markdown(&json!({"a": 1}), "body\n");
        assert_eq!(md, "---\na: 1\n---\n\nbody\n");
    }

    #[test]
    fn roundtrip_parse_serialize() {
        let src = "schema_version: 1\ncreated_at: \"2026-06-14T00:00:00Z\"\nresult: success";
        let fm = parse_front_matter(&format!("---\n{src}\n---\n"));
        assert_eq!(serialize_yaml(&fm, 0), src);
    }
}
