use mt_core::git::{ClaimRef, RunRef};
use std::path::Path;

#[test]
fn claim_and_run_ref_names_are_validated() {
    assert_eq!(
        ClaimRef::new("abc123").unwrap().as_str(),
        "refs/mt/claims/abc123"
    );
    assert_eq!(
        RunRef::new("abc123", "run-1").unwrap().as_str(),
        "refs/mt/runs/abc123/run-1"
    );
    assert!(ClaimRef::new("bad/ref").is_err());
    assert!(ClaimRef::new("ABC123").is_err());
    assert!(RunRef::new("abc123", "bad/token").is_err());
}

#[test]
fn compat_is_the_only_git_cli_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap();
    let mut offenders = Vec::new();
    scan_rust_sources(&root.join("crates"), root, &mut offenders);

    assert!(
        offenders.is_empty(),
        "direct Git CLI outside compat:\n{}",
        offenders.join("\n")
    );
}

fn scan_rust_sources(dir: &Path, root: &Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_rust_sources(&path, root, offenders);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs")
            || path.ends_with("git/compat.rs")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        if source.contains("Command::new(\"git\")") {
            offenders.push(relative(&path, root));
        }
    }
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}
