use mt_core::git::{ClaimRef, RunRef};

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
