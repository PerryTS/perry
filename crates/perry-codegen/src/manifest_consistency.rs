//! In-crate coverage for the #463/#512 manifest-drift check that
//! `crates/perry-codegen/tests/manifest_consistency.rs` used to own alone.
//!
//! # Why this exists (a gate that could not fail on the change that broke it)
//!
//! `every_dispatch_entry_has_manifest_counterpart` compares two tables:
//! `NATIVE_MODULE_TABLE` (this crate's dispatch table, walked through
//! [`crate::iter_native_method_signatures`]) against
//! `perry_api_manifest::API_MANIFEST`. By construction it can be tripped by
//! an edit to EITHER table — but as an integration test under
//! `crates/perry-codegen/tests/`, CI's `e2e-scoped` only runs an integration
//! suite per-PR when the diff names it (CLAUDE.md: "Integration suites under
//! `crates/*/tests/*.rs` run per-PR only when the diff names them"). The one
//! file a drifting PR will almost never touch is `manifest_consistency.rs`
//! itself — a PR that adds `NATIVE_MODULE_TABLE` rows has no reason to edit
//! the test file that checks them. #10668 (node:http client response
//! surface + net.Socket surface cluster) landed 15 such rows, and nothing
//! caught the drift until it happened to surface on a later, unrelated PR
//! that ran in the same CI job.
//!
//! This is a fifth way a gate can be unable to fail, distinct from the four
//! CLAUDE.md already tracks under "Four ways a gate can be unable to fail":
//! there the gate itself is broken (`continue-on-error`, not required,
//! cancelled, or its subject never runs). Here the gate is fine — it runs,
//! it can go red, it's required — and the change class it exists to guard
//! simply never triggers it, because trigger condition and subject are
//! disjoint by construction.
//!
//! Moving the assertion into a `#[cfg(test)]` unit test inside this crate
//! puts it on the `cargo-test`-visible per-PR gate unconditionally
//! (CLAUDE.md again: "Prefer putting acceptance coverage in
//! `cargo-test`-visible unit tests (#5960)"). `perry-codegen` already
//! depends on `perry-api-manifest` as an ordinary (non-dev) dependency — see
//! this crate's `Cargo.toml` — so reaching `API_MANIFEST` from here adds no
//! new dependency edge.
//!
//! The integration test's copy of this same check was removed rather than
//! kept as a duplicate: its trigger condition is a strict subset of this
//! module's (this module runs on every PR; the integration test ran only on
//! PRs that touched its own file), so a passing integration-test copy could
//! never catch anything this module doesn't already catch first. The
//! integration file's other checks — `manifest_param_counts_match_dispatch_table`,
//! the reverse-direction module/binding checks — are unaffected and stay
//! where they are; see that file's header for why.

use perry_api_manifest::{ApiKind, API_MANIFEST};

#[test]
fn every_dispatch_entry_has_manifest_counterpart() {
    let mut missing: Vec<String> = Vec::new();

    for sig in crate::iter_native_method_signatures() {
        // Look for a manifest entry on the same (module, name) where
        // the kind is Method with matching has_receiver. class_filter
        // mismatches across rows of the same (module, method) pair are
        // expected — the dispatch table specializes by class, the
        // manifest does not.
        let hit = API_MANIFEST.iter().any(|e| {
            e.module == sig.module
                && e.name == sig.method
                && matches!(
                    e.kind,
                    ApiKind::Method { has_receiver, .. } if has_receiver == sig.has_receiver
                )
        });
        if !hit {
            let cls = sig.class_filter.unwrap_or("-");
            missing.push(format!(
                "{}::{} (has_receiver={}, class_filter={})",
                sig.module, sig.method, sig.has_receiver, cls
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "API_MANIFEST is missing {} entry/entries that exist in NATIVE_MODULE_TABLE:\n  {}\n\n\
         Add the missing rows to crates/perry-api-manifest/src/entries.rs — \
         drift here would make the unimplemented-API check (#463) error on real implementations.",
        missing.len(),
        missing.join("\n  ")
    );
}
