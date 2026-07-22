//! NNN-нумерація артефактів задач (`run_NNN.md`, `fact_NNN.md`, …).
//!
//! Чисті функції над списком імен файлів — FS лишається на боці викликача
//! (JS-обгортки зберігають ін'єкцію `readdirSync`). Семантика 1:1 із
//! `npm/lib/core/nnn.mjs`: NNN — рядок із ведучими нулями до 3 цифр.

/// Форматує число як NNN-рядок: `1` → `"001"`.
pub fn pad_nnn(n: u64) -> String {
    format!("{n:03}")
}

/// Чи відповідає ім'я шаблону `<prefix><цифри><suffix>` (мінімум одна цифра).
fn nnn_of(name: &str, prefix: &str, suffix: &str) -> Option<u64> {
    let rest = name.strip_prefix(prefix)?;
    let digits = rest.strip_suffix(suffix)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Максимальний NNN серед файлів шаблону, або 0.
fn max_nnn(files: &[String], prefix: &str, suffix: &str) -> u64 {
    files
        .iter()
        .filter_map(|f| nnn_of(f, prefix, suffix))
        .max()
        .unwrap_or(0)
}

/// Наступний NNN для `run_NNN.md`: `count(run_*.md) + 1`.
pub fn next_run_nnn(files: &[String]) -> String {
    let count = files
        .iter()
        .filter(|f| nnn_of(f, "run_", ".md").is_some())
        .count() as u64;
    pad_nnn(count + 1)
}

/// Наступний NNN для `plan_NNN.md`: `max(plan_*.md) + 1`.
pub fn next_plan_nnn(files: &[String]) -> String {
    pad_nnn(max_nnn(files, "plan_", ".md") + 1)
}

/// Найвищий NNN серед `fact_NNN.md`, або `None`.
pub fn latest_fact_nnn(files: &[String]) -> Option<String> {
    match max_nnn(files, "fact_", ".md") {
        0 => None,
        m => Some(pad_nnn(m)),
    }
}

/// Найвищий NNN серед `pending-audit_NNN.md`, або `None`.
pub fn latest_pending_audit_nnn(files: &[String]) -> Option<String> {
    match max_nnn(files, "pending-audit_", ".md") {
        0 => None,
        m => Some(pad_nnn(m)),
    }
}

/// Найвищий NNN серед `audit-result_NNN.md`, або `None`.
pub fn latest_audit_result_nnn(files: &[String]) -> Option<String> {
    match max_nnn(files, "audit-result_", ".md") {
        0 => None,
        m => Some(pad_nnn(m)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn pad_nnn_vectors() {
        assert_eq!(pad_nnn(1), "001");
        assert_eq!(pad_nnn(42), "042");
        assert_eq!(pad_nnn(1000), "1000");
    }

    #[test]
    fn next_run_counts_files() {
        assert_eq!(next_run_nnn(&files(&[])), "001");
        // Рахує кількість, не max: пропуски в нумерації не заповнює.
        assert_eq!(next_run_nnn(&files(&["run_001.md", "run_005.md"])), "003");
        assert_eq!(next_run_nnn(&files(&["run_x.md", "fact_001.md"])), "001");
    }

    #[test]
    fn next_plan_uses_max() {
        assert_eq!(next_plan_nnn(&files(&[])), "001");
        assert_eq!(
            next_plan_nnn(&files(&["plan_001.md", "plan_005.md"])),
            "006"
        );
    }

    #[test]
    fn latest_helpers() {
        assert_eq!(latest_fact_nnn(&files(&[])), None);
        assert_eq!(
            latest_fact_nnn(&files(&["fact_001.md", "fact_003.md"])),
            Some("003".to_string())
        );
        assert_eq!(
            latest_pending_audit_nnn(&files(&["pending-audit_002.md"])),
            Some("002".to_string())
        );
        assert_eq!(
            latest_audit_result_nnn(&files(&["audit-result_007.md"])),
            Some("007".to_string())
        );
    }

    #[test]
    fn rejects_non_digit_middles() {
        assert_eq!(latest_fact_nnn(&files(&["fact_+1.md", "fact_.md"])), None);
    }
}
