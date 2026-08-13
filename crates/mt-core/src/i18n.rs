//! Файловий шар i18n (спека `i18n.md`) — контрактне ядро.
//!
//! Глава сама називає, що в ній контрактне, а що референсне: «контрактне —
//! існування base-мови, `source_hash` у схемі перекладу і триступенева
//! схема "що перекладається"». Саме це тут і реалізовано, плюс
//! **contract-aware сегментація**, без якої fail-closed лишався б обіцянкою.
//!
//! Три принципи, які тримають решту:
//!
//! 1. **Один канон.** Переклади ніколи не впливають на стан графа: scanner,
//!    `## Check` і hash факту читають лише base. Тут немає жодного API, яким
//!    переклад міг би потрапити в derived-стан.
//! 2. **Переклади — derived.** Кожен несе `source_hash` base-версії; hash
//!    розійшовся → переклад stale.
//! 3. **Graceful degradation.** Немає перекладу, він застарів або схема
//!    невідома → показується base. Система коректна без жодного перекладу,
//!    тож усі помилки тут ведуть до base, а не до відмови.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::frontmatter::parse_front_matter;

/// Простір ref-ів перекладів (`i18n.md`, «Сховище»).
pub const I18N_REF_PREFIX: &str = "refs/mt/i18n";

/// Секції, чиє **тіло** читає машина, тож воно лишається base завжди.
///
/// Саме через них fail-closed має сенс: креативний переклад shell-рядка з
/// `## Check` або dep-id з `## Inputs` ламав би не текст, а виконання.
pub const MACHINE_SECTIONS: [&str; 5] = ["Check", "Children", "Inputs", "Approvals", "Ref"];

/// Секції, чиє тіло — людський текст і саме заради нього існує крос-мовність.
///
/// **Неоднозначність спеки, вирішена свідомо:** глава перелічує `## Task` і
/// `## Done when` разом із машинними у списку «контрактні секції, які
/// парсить скрипт», але поруч каже «перекладається лише людський текст між
/// ними» — а `## Task`/`## Done when` і є той людський текст; без них
/// переклад вузла втрачає сенс. Розв'язок: **заголовки** всіх секцій
/// лишаються base завжди (вони і є те, що парсить скрипт), тіла машинних
/// секцій — теж, а тіла цих двох перекладаються.
pub const PROSE_SECTIONS: [&str; 2] = ["Task", "Done when"];

/// Конфіг i18n (`i18n` у `.mt.json`).
///
/// Per-field дефолти: секція майже завжди часткова, і без них
/// `{"base_lang": "uk"}` не розібрався б цілком.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct I18nConfig {
    /// Base-мова проєкту — єдине джерело істини.
    pub base_lang: String,
    /// Що перекладати (globs); дефолт зі спеки.
    pub include: Vec<String>,
    /// Звуження поверх `include`.
    pub exclude: Vec<String>,
    /// Профіль моделі для системної черги регенерації.
    pub model_tier: String,
    /// GC перекладів мов без активних учасників.
    pub ttl_days: u64,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self {
            base_lang: "en".to_string(),
            include: vec!["**/*.md".to_string()],
            exclude: Vec::new(),
            model_tier: "MIN".to_string(),
            ttl_days: 90,
        }
    }
}

/// Читає секцію `i18n` з конфігу проєкту.
pub fn i18n_config(config: &Value) -> I18nConfig {
    config
        .get("i18n")
        .and_then(|section| serde_json::from_value(section.clone()).ok())
        .unwrap_or_default()
}

/// Ref сховища перекладів мови.
pub fn i18n_ref(lang: &str) -> String {
    format!("{I18N_REF_PREFIX}/{lang}")
}

/// Хеш base-версії файлу — те, з чим звіряється `source_hash`.
pub fn source_hash(base_content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base_content.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(2 * digest.len() + "sha256:".len());
    hex.push_str("sha256:");
    for byte in digest.iter() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Фронтматер файлу перекладу (`i18n.md`, «Схема файлу перекладу»).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationMeta {
    /// Шлях base-файлу.
    pub source: String,
    /// Base-версія, з якої згенеровано.
    pub source_hash: String,
    /// `true` = писала людина цією мовою; фонова регенерація не перезаписує.
    pub authored: bool,
    /// Коли згенеровано (ISO8601).
    pub translated_at: String,
    /// Тир моделі, якою згенеровано.
    pub model: String,
}

/// Чи переклад свіжий для цієї base-версії.
///
/// Порівняння за hash, а не за часом: час каже «коли зробили», hash —
/// «з чого зробили», і лише друге відповідає на питання «чи актуально».
pub fn is_fresh(meta: &TranslationMeta, base_content: &str) -> bool {
    meta.source_hash == source_hash(base_content)
}

/// Розбирає файл перекладу на метадані й тіло.
///
/// `None` — не переклад або схема невідома; викликач показує base
/// (принцип graceful degradation).
pub fn parse_translation(text: &str) -> Option<(TranslationMeta, String)> {
    let front = parse_front_matter(text);
    let field = |key: &str| {
        front
            .get(key)
            .and_then(|value| value.as_str().map(str::to_string))
    };
    let meta = TranslationMeta {
        source: field("source")?,
        source_hash: field("source_hash")?,
        authored: front
            .get("authored")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        translated_at: field("translated_at").unwrap_or_default(),
        model: field("model").unwrap_or_default(),
    };
    let body = text
        .split_once("\n---\n")
        .and_then(|(_, rest)| rest.split_once("---\n").map(|(_, body)| body.to_string()))
        .or_else(|| {
            // Фронтматер на початку файлу: `---\n…\n---\n<тіло>`.
            let rest = text.strip_prefix("---\n")?;
            rest.split_once("\n---\n").map(|(_, body)| body.to_string())
        })?;
    Some((meta, body))
}

/// Серіалізує файл перекладу.
pub fn render_translation(meta: &TranslationMeta, body: &str) -> String {
    format!(
        "---\nschema_version: {}\nsource: {}\nsource_hash: {}\nauthored: {}\n\
         translated_at: {}\nmodel: {}\n---\n{body}",
        crate::frontmatter::SCHEMA_VERSION,
        meta.source,
        meta.source_hash,
        meta.authored,
        meta.translated_at,
        meta.model
    )
}

/// Триступенева схема «що перекладається» (`i18n.md`):
/// default `**/*.md` → `include`/`exclude` → per-file `i18n: off`.
///
/// Код і конфіги не перекладаються ніколи — це забезпечує вже перший
/// ступінь, бо дефолтний include покриває лише markdown.
pub fn is_translatable(config: &I18nConfig, path: &str, content: &str) -> bool {
    if !config.include.iter().any(|glob| glob_matches(glob, path)) {
        return false;
    }
    if config.exclude.iter().any(|glob| glob_matches(glob, path)) {
        return false;
    }
    // Per-file opt-out — найсильніший ступінь: автор файлу знає краще за конфіг.
    parse_front_matter(content)
        .get("i18n")
        .and_then(Value::as_str)
        != Some("off")
}

/// Мінімальний glob: `**` (будь-скільки сегментів), `*` (у межах сегмента),
/// `?` і літерали.
///
/// Свій матчер, а не крейт: у конфігу зустрічаються патерни виду `**/*.md`
/// і `docs/**`, і тягти заради них залежність у ядро графа — невиправдано.
fn glob_matches(pattern: &str, path: &str) -> bool {
    glob_match_at(pattern.as_bytes(), path.as_bytes())
}

/// Рекурсивний матчер: `**` пробує всі довжини хвоста, `*`/`?` не
/// переходять межу сегмента.
fn glob_match_at(pattern: &[u8], path: &[u8]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern.starts_with(b"**/") {
        // Нуль сегментів або будь-який префікс, що закінчується `/`.
        if glob_match_at(&pattern[3..], path) {
            return true;
        }
        for (index, byte) in path.iter().enumerate() {
            if *byte == b'/' && glob_match_at(pattern, &path[index + 1..]) {
                return true;
            }
        }
        return false;
    }
    if pattern.starts_with(b"**") {
        return (0..=path.len()).any(|split| glob_match_at(&pattern[2..], &path[split..]));
    }
    match pattern[0] {
        b'*' => (0..=path.len())
            .take_while(|split| !path[..*split].contains(&b'/'))
            .any(|split| glob_match_at(&pattern[1..], &path[split..])),
        b'?' => !path.is_empty() && path[0] != b'/' && glob_match_at(&pattern[1..], &path[1..]),
        literal => {
            !path.is_empty() && path[0] == literal && glob_match_at(&pattern[1..], &path[1..])
        }
    }
}

/// Шматок файлу для перекладача.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// Текст шматка.
    pub text: String,
    /// Чи можна його перекладати.
    pub translatable: bool,
}

/// Contract-aware сегментація: розділяє файл на те, що перекладач бачить,
/// і те, що він не має права чіпати.
///
/// Base лишається дослівним у: фронтматері (ключі й enum-значення),
/// заголовках секцій (це якорі парсера), тілах машинних секцій і code
/// fences. Перекладається лише проза між ними.
///
/// **Fail closed:** файл із фронтматером, але без відомого
/// `schema_version`, не перекладається взагалі — повертається один
/// неперекладний шматок. Креативний переклад контракту не має шансу
/// зламати scanner.
pub fn segment(content: &str) -> Vec<Segment> {
    let verbatim = |text: &str| Segment {
        text: text.to_string(),
        translatable: false,
    };
    let (front, body) = split_front_matter(content);
    if let Some(front_text) = &front {
        let schema_ok = parse_front_matter(content)
            .get("schema_version")
            .and_then(Value::as_u64)
            == Some(crate::frontmatter::SCHEMA_VERSION);
        if !schema_ok {
            return vec![verbatim(content)];
        }
        let mut out = vec![verbatim(front_text)];
        out.extend(segment_body(body));
        return out;
    }
    segment_body(body)
}

/// Ділить файл на фронтматер (як є, з роздільниками) і решту.
fn split_front_matter(content: &str) -> (Option<String>, &str) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content);
    };
    let Some(end) = rest.find("\n---\n") else {
        return (None, content);
    };
    let front = &content[..end + "---\n".len() + "\n---\n".len()];
    (Some(front.to_string()), &rest[end + "\n---\n".len()..])
}

/// Сегментація тіла: заголовки, машинні секції й code fences — дослівно.
fn segment_body(body: &str) -> Vec<Segment> {
    let machine: BTreeSet<&str> = MACHINE_SECTIONS.into_iter().collect();
    let mut out: Vec<Segment> = Vec::new();
    let mut current_machine = false;
    let mut in_fence = false;

    let push = |out: &mut Vec<Segment>, line: &str, translatable: bool| {
        if let Some(last) = out.last_mut() {
            if last.translatable == translatable {
                last.text.push_str(line);
                last.text.push('\n');
                return;
            }
        }
        out.push(Segment {
            text: format!("{line}\n"),
            translatable,
        });
    };

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            push(&mut out, line, false);
            continue;
        }
        if in_fence {
            push(&mut out, line, false);
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("## ") {
            // Заголовок — якір парсера, лишається base завжди.
            current_machine = machine.contains(title.trim());
            push(&mut out, line, false);
            continue;
        }
        if trimmed.starts_with('#') {
            push(&mut out, line, false);
            continue;
        }
        push(&mut out, line, !current_machine);
    }
    out
}

/// Матеріалізація read path: кладе свіжі переклади поверх base у worktree.
///
/// Stale-переклади свідомо **не** матеріалізуються: показати застарілий
/// текст як актуальний гірше, ніж показати base — база принаймні
/// правдива. Регенерація stale — фонова черга, поза цим викликом.
///
/// Повертає перелік перезаписаних шляхів.
///
/// # Errors
/// Помилки файлової системи.
pub fn materialize(
    worktree: &Path,
    translations: &[(String, TranslationMeta, String)],
) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (path, meta, body) in translations {
        let target = worktree.join(path);
        let Ok(base) = std::fs::read_to_string(&target) else {
            // Base зник — перекладу нема на що накладати.
            continue;
        };
        if !is_fresh(meta, &base) {
            continue;
        }
        std::fs::write(&target, body)?;
        written.push(target);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_lang_defaults_to_en_and_partial_section_merges() {
        assert_eq!(I18nConfig::default().base_lang, "en");
        let config = i18n_config(&serde_json::json!({"i18n": {"base_lang": "uk"}}));
        assert_eq!(config.base_lang, "uk");
        assert_eq!(config.include, ["**/*.md"], "решта полів — дефолти");
    }

    #[test]
    fn staleness_is_decided_by_hash_not_time() {
        let meta = TranslationMeta {
            source: "docs/vision.md".into(),
            source_hash: source_hash("канон"),
            authored: false,
            translated_at: "2026-08-13T00:00:00Z".into(),
            model: "MIN".into(),
        };
        assert!(is_fresh(&meta, "канон"));
        assert!(!is_fresh(&meta, "канон змінився"));
    }

    #[test]
    fn translation_file_roundtrips() {
        let meta = TranslationMeta {
            source: "docs/vision.md".into(),
            source_hash: source_hash("base"),
            authored: true,
            translated_at: "2026-08-13T00:00:00Z".into(),
            model: "MIN".into(),
        };
        let text = render_translation(&meta, "# Заголовок\n");
        let (parsed, body) = parse_translation(&text).unwrap();
        assert_eq!(parsed, meta);
        assert_eq!(body, "# Заголовок\n");
    }

    #[test]
    fn non_translation_file_degrades_to_none() {
        // Принцип graceful degradation: незрозумілий файл — не помилка.
        assert!(parse_translation("# просто markdown\n").is_none());
    }

    #[test]
    fn three_step_translatability() {
        let mut config = I18nConfig::default();
        // 1. default include — лише markdown; код і конфіги ніколи.
        assert!(is_translatable(&config, "docs/vision.md", ""));
        assert!(!is_translatable(&config, "src/main.rs", ""));
        assert!(!is_translatable(&config, ".mt.json", ""));

        // 2. exclude звужує.
        config.exclude = vec!["docs/**".into()];
        assert!(!is_translatable(&config, "docs/vision.md", ""));
        assert!(is_translatable(&config, "README.md", ""));

        // 3. per-file opt-out сильніший за конфіг.
        assert!(!is_translatable(
            &I18nConfig::default(),
            "README.md",
            "---\nschema_version: 1\ni18n: off\n---\n"
        ));
    }

    #[test]
    fn machine_sections_and_headings_stay_verbatim() {
        let content = "---\nschema_version: 1\nmode: agent\n---\n\
                       ## Task\n\nЗробити X.\n\n## Check\n\ncargo test\n\n## Inputs\n\nref: a/b\n";
        let segments = segment(content);
        let translatable: String = segments
            .iter()
            .filter(|s| s.translatable)
            .map(|s| s.text.as_str())
            .collect();
        let verbatim: String = segments
            .iter()
            .filter(|s| !s.translatable)
            .map(|s| s.text.as_str())
            .collect();

        // Людський текст — перекладається.
        assert!(translatable.contains("Зробити X."), "{translatable}");
        // Заголовки — якорі парсера, лишаються base.
        assert!(!translatable.contains("## Task"), "{translatable}");
        // Тіла машинних секцій — теж: креативний переклад shell-рядка
        // зламав би не текст, а виконання.
        assert!(!translatable.contains("cargo test"), "{translatable}");
        assert!(!translatable.contains("ref: a/b"), "{translatable}");
        assert!(verbatim.contains("mode: agent"), "фронтматер дослівно");
    }

    #[test]
    fn prose_section_bodies_are_translated() {
        // Прив'язує рішення щодо неоднозначності спеки до поведінки:
        // саме заради цих тіл крос-мовність і існує.
        for section in PROSE_SECTIONS {
            let content =
                format!("---\nschema_version: 1\n---\n\n## {section}\n\nЛюдський текст.\n");
            let translatable: String = segment(&content)
                .iter()
                .filter(|s| s.translatable)
                .map(|s| s.text.as_str())
                .collect();
            assert!(
                translatable.contains("Людський текст."),
                "секція {section}: {translatable}"
            );
        }
    }

    #[test]
    fn code_fences_stay_verbatim() {
        let content = "Проза.\n\n```bash\nrm -rf /\n```\n\nЩе проза.\n";
        let translatable: String = segment(content)
            .iter()
            .filter(|s| s.translatable)
            .map(|s| s.text.as_str())
            .collect();
        assert!(translatable.contains("Проза."));
        assert!(!translatable.contains("rm -rf"), "{translatable}");
    }

    #[test]
    fn unknown_schema_version_is_not_translated_at_all() {
        // Fail closed: файл із майбутньої схеми показується base цілком.
        let content = "---\nschema_version: 99\n---\n\n## Task\n\nтекст\n";
        let segments = segment(content);
        assert_eq!(segments.len(), 1);
        assert!(!segments[0].translatable);
    }

    #[test]
    fn segments_reassemble_into_the_original() {
        // Сегментація не має права загубити жодного символу — інакше
        // «переклад» тихо різав би файли.
        let content = "---\nschema_version: 1\n---\n\n## Task\n\nТекст.\n\n## Check\n\nls\n";
        let joined: String = segment(content).iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, content);
    }

    #[test]
    fn stale_translation_is_not_materialized() {
        let worktree = tempfile::tempdir().unwrap();
        std::fs::write(worktree.path().join("a.md"), "нова base").unwrap();

        let stale = TranslationMeta {
            source: "a.md".into(),
            source_hash: source_hash("стара base"),
            authored: false,
            translated_at: String::new(),
            model: "MIN".into(),
        };
        let written = materialize(
            worktree.path(),
            &[("a.md".to_string(), stale, "переклад".to_string())],
        )
        .unwrap();
        assert!(written.is_empty(), "застарілий переклад не накладається");
        assert_eq!(
            std::fs::read_to_string(worktree.path().join("a.md")).unwrap(),
            "нова base"
        );
    }

    #[test]
    fn fresh_translation_is_materialized_over_base() {
        let worktree = tempfile::tempdir().unwrap();
        std::fs::write(worktree.path().join("a.md"), "base").unwrap();
        let fresh = TranslationMeta {
            source: "a.md".into(),
            source_hash: source_hash("base"),
            authored: false,
            translated_at: String::new(),
            model: "MIN".into(),
        };
        let written = materialize(
            worktree.path(),
            &[("a.md".to_string(), fresh, "переклад".to_string())],
        )
        .unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(
            std::fs::read_to_string(worktree.path().join("a.md")).unwrap(),
            "переклад"
        );
    }

    #[test]
    fn i18n_ref_lives_outside_main() {
        assert_eq!(i18n_ref("uk"), "refs/mt/i18n/uk");
    }
}
