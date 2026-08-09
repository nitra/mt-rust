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

        if next_line.trim_start().starts_with("- ") || next_line.trim_start() == "-" {
            // Список. Елемент — скаляр (`- bash`), inline-мапа (`- {}`) або
            // блокова мапа (`- strategy: x` + рядки продовження глибше).
            let item_indent = get_indent(next_line);
            let mut arr = vec![];
            let mut item: Option<Vec<String>> = None;
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
                if get_indent(l) <= item_indent && (t == "-" || t.starts_with("- ")) {
                    if let Some(prev) = item.take() {
                        arr.push(finish_list_item(prev));
                    }
                    item = Some(vec![t.strip_prefix('-').unwrap_or(t).trim().to_string()]);
                } else if let Some(cur) = item.as_mut() {
                    // Рядок продовження блокової мапи цього елемента.
                    cur.push(t.to_string());
                }
                i += 1;
            }
            if let Some(prev) = item.take() {
                arr.push(finish_list_item(prev));
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
/// Елемент списку: рядки `["strategy: x", "skills_add: [y]"]` → мапа;
/// один рядок без `key:` → скаляр. Порожній елемент (`-` або `- {}`) → `{}`.
fn finish_list_item(lines: Vec<String>) -> Value {
    let first = lines.first().map(String::as_str).unwrap_or("");
    if lines.len() == 1 {
        if first.is_empty() {
            return Value::Object(Map::new());
        }
        if !looks_like_mapping(first) {
            return parse_scalar(first);
        }
    }
    Value::Object(parse_yaml_block(&lines.join("\n")))
}

/// Чи рядок є `key: value`/`key:` — ключ до першої двокрапки без пробілів
/// і лапок (щоб `http://x` чи `note: a: b` не ламали визначення).
fn looks_like_mapping(s: &str) -> bool {
    let Some(idx) = s.find(':') else {
        return false;
    };
    let key = &s[..idx];
    let rest = &s[idx + 1..];
    !key.is_empty()
        && !key.contains(' ')
        && !key.contains('"')
        && !key.contains('\'')
        && (rest.is_empty() || rest.starts_with(' '))
}

/// Inline flow-масив `[a, b]` — split по комах верхнього рівня (вкладені
/// дужки й лапки не розрізаються).
fn parse_flow_seq(inner: &str) -> Vec<Value> {
    let mut items: Vec<Value> = vec![];
    let (mut depth, mut quote, mut start) = (0usize, None::<char>, 0usize);
    let bytes: Vec<(usize, char)> = inner.char_indices().collect();
    for (pos, ch) in bytes {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                items.push(parse_scalar(inner[start..pos].trim()));
                start = pos + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        items.push(parse_scalar(tail));
    }
    items
}

fn parse_scalar(s: &str) -> Value {
    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" | "~" => return Value::Null,
        _ => {}
    }
    // Inline flow: `[a, b]` і `{}` / `{k: v}`.
    if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Value::Array(parse_flow_seq(inner));
    }
    if let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        let block: Vec<String> = parse_flow_seq(inner)
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|p| looks_like_mapping(p))
            .collect();
        return Value::Object(parse_yaml_block(&block.join("\n")));
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

    // ── inline flow і списки мап (форма a.md зі спеки graph.md) ──

    #[test]
    fn inline_flow_sequence() {
        let v = parse_yaml("skills: [bash, write-files]\nsecrets: [STRIPE_KEY]");
        assert_eq!(v["skills"], json!(["bash", "write-files"]));
        assert_eq!(v["secrets"], json!(["STRIPE_KEY"]));
    }

    #[test]
    fn inline_flow_sequence_quoted_and_empty() {
        let v = parse_yaml("a: []\nb: ['x, y', z]");
        assert_eq!(v["a"], json!([]));
        // Кома всередині лапок не розрізає елемент.
        assert_eq!(v["b"], json!(["x, y", "z"]));
    }

    #[test]
    fn block_sequence_of_scalars_unchanged() {
        let v = parse_yaml("skills:\n  - bash\n  - write-files");
        assert_eq!(v["skills"], json!(["bash", "write-files"]));
    }

    #[test]
    fn sequence_of_mappings() {
        let v = parse_yaml("retry_ladder:\n  - {}\n  - strategy: diagnose-first\n");
        assert_eq!(v["retry_ladder"], json!([{}, {"strategy": "diagnose-first"}]));
    }

    #[test]
    fn sequence_of_multiline_mappings() {
        let src = "retry_ladder:\n  - strategy: alternative-approach\n    model_tier_delta: 1\n  - strategy: diagnose-first\n";
        let v = parse_yaml(src);
        assert_eq!(
            v["retry_ladder"],
            json!([
                {"strategy": "alternative-approach", "model_tier_delta": 1},
                {"strategy": "diagnose-first"}
            ])
        );
    }

    #[test]
    fn colon_in_value_is_not_a_mapping_key() {
        // «- note: a: b» — ключ note, решта значення; «- http://x» — скаляр.
        let v = parse_yaml("items:\n  - http://x\n  - note: a: b\n");
        assert_eq!(v["items"][0], json!("http://x"));
        assert_eq!(v["items"][1], json!({"note": "a: b"}));
    }

    #[test]
    fn a_md_frontmatter_full_shape() {
        let src = concat!(
            "---\n",
            "schema_version: 1\n",
            "created_at: 2026-08-09T10:00:00Z\n",
            "model_tier: AVG\n",
            "agent_cli: codex\n",
            "skills: [bash, write-files]\n",
            "secrets: [STRIPE_KEY]\n",
            "retry_ladder:\n",
            "  - {}\n",
            "  - strategy: diagnose-first\n",
            "interactive: false\n",
            "---\n\nПрозовий коментар до вибору виконавця.\n"
        );
        let fm = parse_front_matter(src);
        assert_eq!(fm["schema_version"], json!(1));
        assert_eq!(fm["model_tier"], json!("AVG"));
        assert_eq!(fm["agent_cli"], json!("codex"));
        assert_eq!(fm["skills"], json!(["bash", "write-files"]));
        assert_eq!(fm["interactive"], json!(false));
        assert_eq!(fm["retry_ladder"][1]["strategy"], json!("diagnose-first"));
        assert_eq!(get_body(src), "Прозовий коментар до вибору виконавця.\n");
    }
}
