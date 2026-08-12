//! #7493 — the `testing` cargo feature must never reach a production build.
//!
//! `perry_codegen::testing::NativeRootsPin` lets a test declare which root
//! lowering it asserts on. Its whole safety argument is that with the feature
//! off, the pin, its thread-local, and the branch it adds to the top of
//! `rs4gc_enabled()` are `#[cfg]`-ed out of the artifact — and that the only
//! manifest edge which turns the feature on is a `[dev-dependencies]` one,
//! which cargo resolves for test/bench targets only.
//!
//! The second half of that argument is a claim about manifests, and claims
//! about manifests rot. This is the gate for it. It lives in `src/`, so unlike
//! the integration suites it runs in the per-PR `cargo-test` job — which is
//! the tier #7493 exists because these suites are *not* in.
//!
//! Built so it can fail (CLAUDE.md, "four ways a gate can be unable to fail"):
//! the scan asserts its own subject was live. It must find at least one
//! manifest, must find the crate's own manifest among them, and must find the
//! dev-dependency edge that legitimately enables the feature — so "0 offenders
//! over 0 manifests" and "0 offenders over 40 manifests" cannot report the same
//! verdict. `feature_scan_flags_a_planted_production_edge` then plants the
//! exact offending shape in an in-memory manifest and asserts the scanner names
//! it.
//!
//! This file also holds the **root-lowering default tripwire**. #7370 flipped
//! the default from shadow stack to native roots, and the five integration
//! suites that assert on the losing lowering went red without anything in the
//! per-PR tier noticing — they run nightly/at-tag only. A default flip is a
//! deliberate act, so the fix is not to detect it after the fact but to make it
//! impossible to land silently: `host_target_lowering_default_is_native_roots`
//! fails in `cargo-test` (a REQUIRED context) the moment the default changes,
//! and its message names the suites that then need re-pinning.

use std::path::{Path, PathBuf};

/// Sections whose entries cargo builds for NON-test targets. An edge here that
/// enabled `testing` would put the pin in shipped binaries.
const PRODUCTION_SECTIONS: [&str; 2] = ["dependencies", "build-dependencies"];

/// Workspace root, from this file's location.
fn workspace_root() -> PathBuf {
    // <root>/crates/perry-codegen/src/codegen/testing_feature_gate_tests.rs
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<pkg> should have two ancestors")
        .to_path_buf()
}

/// Every `Cargo.toml` in the workspace (root + one per crate).
fn workspace_manifests() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut out = vec![root.join("Cargo.toml")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        let mut crate_manifests: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path().join("Cargo.toml"))
            .filter(|p| p.is_file())
            .collect();
        crate_manifests.sort();
        out.extend(crate_manifests);
    }
    out
}

/// A TOML section header line, e.g. `[dependencies.perry-codegen]` -> the
/// section path segments. Returns `None` for non-header lines.
fn section_path(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    // `[[bin]]` and friends: strip the extra bracket pair.
    let inner = inner.strip_prefix('[').unwrap_or(inner);
    let inner = inner.strip_suffix(']').unwrap_or(inner);
    Some(
        inner
            .split('.')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect(),
    )
}

/// Is this section one cargo builds for non-test targets?
///
/// Handles both `[dependencies]` and target-specific
/// `[target.'cfg(unix)'.dependencies]`, and rejects every `dev-dependencies`
/// spelling of the same.
fn is_production_dependency_section(path: &[String]) -> bool {
    // The section name that decides is the last one that names a dependency
    // table — `[dependencies.foo]` and `[target.X.dependencies.foo]` both carry
    // it, as does `[workspace.dependencies]`.
    path.iter()
        .any(|seg| PRODUCTION_SECTIONS.contains(&seg.as_str()))
        && !path.iter().any(|seg| seg == "dev-dependencies")
}

/// Scan one manifest's text. Returns `(offending lines, saw a dev edge that
/// enables the feature)`.
fn scan_manifest(label: &str, text: &str) -> (Vec<String>, bool) {
    let mut offenders = Vec::new();
    let mut saw_dev_edge = false;
    let mut section: Vec<String> = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        if let Some(path) = section_path(line) {
            section = path;
            continue;
        }
        if !line.contains("testing") {
            continue;
        }
        // Only dependency-table lines matter, and only ones naming this crate.
        let names_this_crate =
            line.contains("perry-codegen") || section.iter().any(|s| s == "perry-codegen");
        if !names_this_crate {
            continue;
        }
        // `features = [..., "testing", ...]` — quoted, so a `testing = []`
        // feature DEFINITION in `[features]` is not mistaken for an edge.
        if !line.contains("\"testing\"") {
            continue;
        }
        if is_production_dependency_section(&section) {
            offenders.push(format!(
                "{label}:{}: [{}] enables perry-codegen's `testing` feature: {}",
                lineno + 1,
                section.join("."),
                line.trim()
            ));
        } else if section.iter().any(|seg| seg == "dev-dependencies") {
            saw_dev_edge = true;
        }
    }
    (offenders, saw_dev_edge)
}

#[test]
fn testing_feature_is_never_enabled_by_a_production_dependency_edge() {
    let manifests = workspace_manifests();
    assert!(
        manifests.len() > 10,
        "the manifest scan found only {} files — the glob is broken and this \
         gate would be vacuously green",
        manifests.len()
    );

    let mut offenders = Vec::new();
    let mut saw_dev_edge = false;
    let mut saw_own_manifest = false;
    for path in &manifests {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let label = path
            .strip_prefix(workspace_root())
            .unwrap_or(path)
            .display()
            .to_string();
        if label.contains("perry-codegen") {
            saw_own_manifest = true;
        }
        let (mut found, dev) = scan_manifest(&label, &text);
        offenders.append(&mut found);
        saw_dev_edge |= dev;
    }

    assert!(
        saw_own_manifest,
        "the scan never reached crates/perry-codegen/Cargo.toml"
    );
    assert!(
        saw_dev_edge,
        "no [dev-dependencies] edge enables perry-codegen's `testing` feature — \
         either the mechanism was removed (in which case delete this gate and \
         the feature) or the scanner stopped recognising it, which would make \
         its clean verdict meaningless"
    );
    assert!(
        offenders.is_empty(),
        "the `testing` feature is test-support only and must not be reachable \
         from a production build:\n  {}",
        offenders.join("\n  ")
    );
}

/// Sabotage: the scanner must actually name a production edge, not merely
/// return an empty list because it looks at nothing.
#[test]
fn feature_scan_flags_a_planted_production_edge() {
    let planted = "\
[package]
name = \"pretend\"

[dependencies]
perry-codegen = { path = \"../perry-codegen\", features = [\"testing\"] }

[dev-dependencies]
perry-codegen = { path = \"../perry-codegen\", features = [\"testing\"] }
";
    let (offenders, saw_dev_edge) = scan_manifest("planted/Cargo.toml", planted);
    assert_eq!(
        offenders.len(),
        1,
        "the production edge should be the only offender: {offenders:?}"
    );
    assert!(
        offenders[0].contains("[dependencies]"),
        "the offender should name the section it was found in: {}",
        offenders[0]
    );
    assert!(
        saw_dev_edge,
        "the dev edge should be recognised, not counted as an offender"
    );

    // …and a target-specific production table is the same offence.
    let target_scoped = "\
[target.'cfg(unix)'.dependencies]
perry-codegen = { path = \"../perry-codegen\", features = [\"testing\"] }
";
    let (offenders, _) = scan_manifest("planted2/Cargo.toml", target_scoped);
    assert_eq!(
        offenders.len(),
        1,
        "a target-specific [dependencies] table is still a production edge: \
         {offenders:?}"
    );

    // …while the feature DEFINITION itself is not an edge.
    let definition_only = "\
[package]
name = \"perry-codegen\"

[features]
testing = []
";
    let (offenders, _) = scan_manifest("planted3/Cargo.toml", definition_only);
    assert!(
        offenders.is_empty(),
        "declaring the feature is not enabling it: {offenders:?}"
    );
}

/// Tripwire (#7493): the root-lowering default on a host the runtime can walk
/// must be NATIVE ROOTS, and the pin must be able to select either lowering.
///
/// The suites listed in the failure message assert on one specific lowering and
/// say so with `perry_codegen::testing::NativeRootsPin`. WHICH pin each of them
/// needs is a function of this default. When #7370 flipped it, nothing in the
/// per-PR tier went red — those suites run nightly/at-tag only — and
/// `shadow_slot_hygiene` sat at 0/12 on `main` until someone ran the tier by
/// hand. This test lives in `src/`, so it runs in `cargo-test`, which IS a
/// required context: a future flip fails HERE, in the PR that makes it, with
/// the follow-on work named in the failure message.
///
/// It asserts its subject was live rather than merely that nothing threw: the
/// two pins must produce DIFFERENT answers, and the unsupported-target arm must
/// produce the opposite default — so an `rs4gc_enabled()` wired to a constant
/// fails this test rather than passing it.
#[test]
fn host_target_lowering_default_is_native_roots() {
    use super::helpers::{rs4gc_enabled, set_native_roots_for_target};
    use crate::testing::NativeRootsPin;

    // `PERRY_RS4GC` is an explicit, process-global override cached in a
    // `OnceLock`; under it the DEFAULT is not what is being measured.
    if std::env::var("PERRY_RS4GC").is_ok() {
        return;
    }

    for triple in [
        "aarch64-apple-darwin",
        "arm64-apple-macosx15.0.0",
        "x86_64-unknown-linux-gnu",
    ] {
        set_native_roots_for_target(triple);
        assert!(
            rs4gc_enabled(),
            "the root-lowering default for {triple} is no longer native roots. \
             If that is intentional, re-pin the suites that assert on a specific \
             lowering BEFORE landing it — crates/perry-codegen/tests/ \
             shadow_slot_hygiene.rs, scalar_replaced_slot_roots.rs, \
             temp_root_operand_temporaries.rs, native_proof_regressions.rs (and \
             its invalidation module) and native_proof_buffer_views.rs — then \
             update this test. #7493 is what happens when that step is skipped: \
             those suites run nightly/at-tag only, so nothing goes red at merge \
             time."
        );

        // The pin must be able to say BOTH things, or a suite that declares its
        // lowering is declaring nothing.
        {
            let _pin = NativeRootsPin::shadow();
            assert!(
                !rs4gc_enabled(),
                "NativeRootsPin::shadow() must select the shadow-stack lowering"
            );
        }
        assert!(
            rs4gc_enabled(),
            "the pin must restore the previous decision"
        );
        {
            let _pin = NativeRootsPin::native();
            assert!(
                rs4gc_enabled(),
                "NativeRootsPin::native() must select the native-roots lowering"
            );
        }
    }

    // A target whose frame bases the runtime cannot resolve must still fall
    // back to the shadow stack — otherwise `gc_map` refuses and the compile
    // fails outright. This arm is what makes the assertion above a statement
    // about the DEFAULT rather than about a constant.
    set_native_roots_for_target("arm64_32-apple-watchos");
    assert!(
        !rs4gc_enabled(),
        "watchOS ILP32 has no native-root map reader; the default must fall \
         back to the shadow stack there"
    );
    // …and an explicit pin still outranks it, which is what lets a test assert
    // native-roots IR while a `PERRY_RS4GC=0` sweep is in progress.
    {
        let _pin = NativeRootsPin::native();
        assert!(
            rs4gc_enabled(),
            "an explicit pin outranks the per-target default"
        );
    }
}
