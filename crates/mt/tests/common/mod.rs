//! Мінімальна git-фікстура для black-box e2e тестів `mt`-бінарника — bare
//! "origin" + звичайний клон з одним комітом на `main`. Аналог
//! `mt_core::test_support::TestRepo`, продубльований тут навмисно: той
//! модуль — `#[cfg(test)]`-приватний усередині `mt-core` і недоступний
//! іншим крейтам навіть у тестових білдах.

pub use mt_core::test_support::TestRepo;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Шлях до скомпільованого бінарника `mt` цього тестового прогону.
pub fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mt"))
}

pub fn run(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("mt {args:?}: {e}"))
}

pub fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}
