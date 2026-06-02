//! Cross-process determinism for `perry_hir::stable_hash` (#686).
//!
//! Builds the `stable_hash_cross_process` example binary, runs it
//! twice, and asserts that both runs print byte-identical hashes.
//! This is the only test that can catch a forgotten `HashMap`-sort
//! in the hash walk, because Rust's `HashMap` randomizes iteration
//! order between processes (not within one), so an in-process loop
//! would silently pass with a bug that breaks the cache in CI.

use std::process::Command;

fn run_example() -> String {
    // `cargo run --example stable_hash_cross_process` builds (or
    // reuses) the example and prints its stdout. We use --quiet so
    // the cargo banner doesn't pollute the output we want to compare.
    let out = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "--example",
            "stable_hash_cross_process",
            "-p",
            "perry-hir",
        ])
        .output()
        .expect("failed to run stable_hash_cross_process example");
    assert!(
        out.status.success(),
        "example exited with {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("non-utf8 stdout from example")
        .trim()
        .to_string()
}

#[test]
fn hash_stable_across_processes() {
    let a = run_example();
    let b = run_example();
    assert_eq!(
        a, b,
        "perry_hir::stable_hash::hash_module must be cross-process deterministic; \
         a={a}, b={b}. A drift here usually means a HashMap (or HashSet) iteration \
         leaked into the hash walk — find it and sort entries before emit."
    );
    // Sanity: the printed hash isn't the empty/initial djb2 state.
    assert_ne!(
        a, "0000000000001515",
        "hash looks suspiciously like djb2 init"
    );
}
