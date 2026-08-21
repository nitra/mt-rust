//! Ізоляція виконавця рівня ОС: запис дозволений **лише у worktree run-а**
//! (спека `operations.md`, security model — fs-scope worktree).
//!
//! **Межа свідомо вужча за слово «sandbox».** Ізолюється рівно файловий
//! запис. Allowlist команд і мережа лишаються декларативними
//! ([`crate::sandbox`]): їх застосовує ACP-гейт і сам CLI, а не ядро ОС.
//! Причина — сумісність: повна ізоляція (мережа, exec) ламає підписочні CLI,
//! які ходять у свої API й запускають тулчейни, і її вимикали б назад цілком.
//! Вузька межа, яку не хочеться вимикати, захищає більше за широку, яку
//! вимкнули.
//!
//! Що це дає: агент, який зірвався, не може зіпсувати робоче дерево, чужі
//! worktree, конфіги користувача чи систему — усе, що поза worktree, для
//! нього read-only.
//!
//! **Вимкнено за замовчуванням** — той самий принцип, що в `skill_profiles`:
//! механізм вмикається явно, бо мовчазне ввімкнення зламало б наявні
//! проєкти й «безпека» звелася б до її вимикання.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Режим ізоляції з конфігу.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IsolationMode {
    /// Без ізоляції рівня ОС (дефолт).
    #[default]
    Off,
    /// Запис дозволений лише у worktree (+ явно розширені шляхи).
    Worktree,
}

/// Чому ізоляцію не вдалося застосувати.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationError {
    /// Платформа не підтримується цією реалізацією.
    Unsupported { platform: &'static str },
    /// Worktree не резолвиться у канонічний шлях.
    Unresolvable { path: PathBuf },
}

impl std::fmt::Display for IsolationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { platform } => write!(
                f,
                "ізоляція worktree не реалізована для {platform} — \
                 вимкни sandbox.isolation або запусти на підтримуваній платформі"
            ),
            Self::Unresolvable { path } => write!(
                f,
                "ізоляція: worktree {} не резолвиться у канонічний шлях",
                path.display()
            ),
        }
    }
}

/// Режим із конфігу проєкту (`sandbox.isolation`).
pub fn isolation_mode(config: &Value) -> IsolationMode {
    match config
        .get("sandbox")
        .and_then(|section| section.get("isolation"))
        .and_then(Value::as_str)
    {
        Some("worktree") => IsolationMode::Worktree,
        _ => IsolationMode::Off,
    }
}

/// Додатково дозволені на запис шляхи (`sandbox.isolation_writable`).
///
/// Потрібні, бо підписочні CLI тримають стан поза worktree (кеш npm,
/// `~/.claude`). Розширення **явне**: дефолт лишається строгим, а кожен
/// отвір видно в конфігу.
pub fn extra_writable(config: &Value) -> Vec<String> {
    config
        .get("sandbox")
        .and_then(|section| section.get("isolation_writable"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(expand_home))
                .collect()
        })
        .unwrap_or_default()
}

/// Розгортає провідний `~` у `$HOME`.
fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix("~/") else {
        return path.to_string();
    };
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/{rest}"),
        Err(_) => path.to_string(),
    }
}

/// Готова до запуску команда.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped {
    /// Програма (сама або обгортка ізоляції).
    pub program: String,
    /// Аргументи, включно з початковою програмою, якщо є обгортка.
    pub args: Vec<String>,
}

/// Канонічний шлях — **обовʼязково**, і це не косметика.
///
/// `sandbox-exec` звіряє резолвлені шляхи: профіль із логічним
/// `/var/folders/...` не покриває реальний `/private/var/folders/...`, і
/// пісочниця забороняє запис навіть у власний worktree. Симптом — «усе
/// зламалось», причина — непомітна.
fn canonical(path: &Path) -> Result<PathBuf, IsolationError> {
    path.canonicalize()
        .map_err(|_| IsolationError::Unresolvable {
            path: path.to_path_buf(),
        })
}

/// Текст профілю `sandbox-exec` (macOS).
///
/// Порядок правил значущий: пізніші перекривають ранні, тому спершу
/// глобальна заборона запису, далі точкові дозволи.
pub fn macos_profile(worktree: &Path, writable: &[PathBuf]) -> String {
    let mut profile = String::from("(version 1)\n(allow default)\n(deny file-write*)\n");
    profile.push_str(&format!(
        "(allow file-write* (subpath \"{}\"))\n",
        worktree.display()
    ));
    for path in writable {
        profile.push_str(&format!(
            "(allow file-write* (subpath \"{}\"))\n",
            path.display()
        ));
    }
    // Стандартні пристрої й ioctl: без них процес не має куди писати stdout
    // і падає ще до першого корисного рядка.
    profile.push_str(
        "(allow file-write-data (literal \"/dev/null\") (literal \"/dev/stdout\") \
         (literal \"/dev/stderr\") (literal \"/dev/tty\") (literal \"/dev/dtracehelper\"))\n\
         (allow file-ioctl)\n",
    );
    profile
}

/// Тимчасова директорія процесу — її мусить бути видно на запис, інакше
/// падає майже будь-який тулчейн.
fn process_tmp() -> Option<PathBuf> {
    let raw = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    Path::new(&raw).canonicalize().ok()
}

/// Обгортає команду ізоляцією за режимом.
///
/// # Errors
/// Платформа не підтримується або worktree не резолвиться.
pub fn wrap(
    mode: IsolationMode,
    worktree: &Path,
    extra: &[String],
    program: &str,
    args: &[String],
) -> Result<Wrapped, IsolationError> {
    if mode == IsolationMode::Off {
        return Ok(Wrapped {
            program: program.to_string(),
            args: args.to_vec(),
        });
    }

    if cfg!(target_os = "macos") {
        let root = canonical(worktree)?;
        let mut writable: Vec<PathBuf> = process_tmp().into_iter().collect();
        // Нерезолвний додатковий шлях пропускаємо мовчки: він міг бути
        // задекларований наперед (кеш, якого ще немає), і це не привід
        // валити run.
        writable.extend(
            extra
                .iter()
                .filter_map(|path| Path::new(path).canonicalize().ok()),
        );
        let mut wrapped = vec![
            "-p".to_string(),
            macos_profile(&root, &writable),
            program.to_string(),
        ];
        wrapped.extend(args.iter().cloned());
        return Ok(Wrapped {
            program: "sandbox-exec".to_string(),
            args: wrapped,
        });
    }

    // Fail closed: мовчазний запуск без ізоляції там, де її попросили, —
    // найгірший результат, бо виглядає як захищений.
    Err(IsolationError::Unsupported {
        platform: std::env::consts::OS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_is_off_by_default() {
        assert_eq!(isolation_mode(&serde_json::json!({})), IsolationMode::Off);
        assert_eq!(
            isolation_mode(&serde_json::json!({"sandbox": {"isolation": "off"}})),
            IsolationMode::Off
        );
        assert_eq!(
            isolation_mode(&serde_json::json!({"sandbox": {"isolation": "worktree"}})),
            IsolationMode::Worktree
        );
    }

    #[test]
    fn off_mode_passes_command_through() {
        let wrapped = wrap(
            IsolationMode::Off,
            Path::new("/nonexistent"),
            &[],
            "claude",
            &["-p".to_string(), "текст".to_string()],
        )
        .unwrap();
        assert_eq!(wrapped.program, "claude");
        assert_eq!(wrapped.args, ["-p", "текст"]);
    }

    #[test]
    fn profile_denies_before_allowing() {
        // Порядок значущий: у sandbox-exec пізніші правила перекривають
        // ранні, тож дозвіл до заборони не дав би нічого.
        let profile = macos_profile(Path::new("/repo/wt"), &[]);
        let deny = profile.find("(deny file-write*)").expect("немає заборони");
        let allow = profile
            .find("(allow file-write* (subpath \"/repo/wt\"))")
            .unwrap();
        assert!(deny < allow, "{profile}");
    }

    #[test]
    fn extra_writable_expands_home() {
        let config = serde_json::json!({
            "sandbox": {"isolation_writable": ["~/.claude", "/opt/cache"]}
        });
        let paths = extra_writable(&config);
        assert_eq!(paths.len(), 2);
        assert!(!paths[0].starts_with('~'), "{paths:?}");
        assert_eq!(paths[1], "/opt/cache");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn worktree_path_is_canonicalized() {
        // Пастка, на якій це ламається тихо: логічний /var/folders/… не
        // покриває реальний /private/var/folders/…, і пісочниця забороняє
        // запис навіть у власний worktree.
        let dir = tempfile::tempdir().unwrap();
        let logical = dir.path().to_path_buf();
        let real = logical.canonicalize().unwrap();

        let wrapped = wrap(IsolationMode::Worktree, &logical, &[], "echo", &[]).unwrap();
        let profile = &wrapped.args[1];
        assert!(
            profile.contains(&format!("(subpath \"{}\")", real.display())),
            "у профілі має бути канонічний шлях: {profile}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn missing_worktree_is_an_error_not_a_bare_command() {
        let error = wrap(
            IsolationMode::Worktree,
            Path::new("/nonexistent-worktree"),
            &[],
            "echo",
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(error, IsolationError::Unresolvable { .. }),
            "{error:?}"
        );
    }

    /// Найважливіший тест модуля: пісочниця справді ізолює, а не лише
    /// збирає рядок профілю.
    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_actually_confines_writes() {
        let worktree = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let inside_file = worktree.path().canonicalize().unwrap().join("inside.txt");
        let outside_file = outside.path().canonicalize().unwrap().join("outside.txt");

        // Заборонений напрямок навмисно не в TMPDIR: він дозволений на
        // запис, тож перевіряти треба поза ним.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let forbidden = Path::new(&home).join("mt-isolation-probe.txt");

        let script = format!(
            "echo ok > {} && (echo bad > {} && echo ESCAPED || echo DENIED)",
            inside_file.display(),
            forbidden.display()
        );
        let wrapped = wrap(
            IsolationMode::Worktree,
            worktree.path(),
            &[],
            "/bin/sh",
            &["-c".to_string(), script],
        )
        .unwrap();

        let out = std::process::Command::new(&wrapped.program)
            .args(&wrapped.args)
            .output()
            .expect("sandbox-exec не запустився");
        let stdout = String::from_utf8_lossy(&out.stdout);

        assert!(
            inside_file.exists(),
            "запис у worktree мусив пройти: {stdout}"
        );
        assert!(
            stdout.contains("DENIED"),
            "запис поза worktree не заборонено: {stdout}"
        );
        assert!(!forbidden.exists(), "файл поза worktree створено");
        let _ = outside_file;
    }
}
