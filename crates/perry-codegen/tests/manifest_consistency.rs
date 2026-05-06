//! Drift guard for `perry-api-manifest::API_MANIFEST` (#463 / #512).
//!
//! Every row of `NATIVE_MODULE_TABLE` (the static dispatch table in
//! `lower_call.rs`) must have a counterpart entry in `API_MANIFEST`,
//! otherwise the unimplemented-API check would error on a real
//! implementation. This file covers two drifts:
//!
//! 1. `every_dispatch_entry_has_manifest_counterpart` — by name only;
//!    catches new dispatch rows that nobody added to the manifest.
//! 2. `manifest_param_counts_match_dispatch_table` (#512) — for
//!    auto-derivable rows (`has_receiver: false`, no class filter) the
//!    manifest's `params.len()` must match the dispatch table's args
//!    arity, so the generated `.d.ts` doesn't claim a different shape
//!    than what codegen actually accepts.
//!
//! Class-filtered duplicates collapse to one manifest entry — the
//! manifest tracks "is this method known on this module?", not the
//! per-class signature variants.

use perry_api_manifest::{ApiKind, ParamSpec, TypeSpec, API_MANIFEST};
use perry_codegen::iter_native_method_signatures;

#[test]
fn every_dispatch_entry_has_manifest_counterpart() {
    let mut missing: Vec<String> = Vec::new();

    for sig in iter_native_method_signatures() {
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

/// #512: for every auto-derivable dispatch row (no receiver, no class
/// filter) the manifest's `params` length must match the dispatch
/// table's args length, AND each `NA_STR` in the dispatch table must
/// land on a `TypeSpec::String` slot in the manifest. The manifest
/// can NARROW (a row that says `NA_F64` is fine to declare as
/// `String` — codegen NaN-boxes whatever the user passes), but the
/// reverse is a real drift: a manifest claiming `String` where the
/// dispatch table accepts `NA_F64` would let `tsc` accept a wrong call.
///
/// Rows whose manifest entry has `params: &[]` AND `returns: Any` are
/// skipped — that's the "no signature data" fallback the emitter
/// preserves as `(...args: any[]): any` to avoid regressions.
#[test]
fn manifest_param_counts_match_dispatch_table() {
    let mut mismatches: Vec<String> = Vec::new();

    for sig in iter_native_method_signatures() {
        // Only the auto-derivable shape — instance methods and
        // class-filtered rows are deliberately loose for now.
        if sig.has_receiver || sig.class_filter.is_some() {
            continue;
        }

        // Find the matching manifest entry — exact (module, name,
        // has_receiver: false, class_filter: None) tuple. There may be
        // duplicates (e.g. some modules register both a `default` and
        // a named export pointing at the same impl); we check all of
        // them since the .d.ts dedupes by name later.
        let candidates: Vec<&'static perry_api_manifest::ApiEntry> = API_MANIFEST
            .iter()
            .filter(|e| {
                e.module == sig.module
                    && e.name == sig.method
                    && matches!(
                        e.kind,
                        ApiKind::Method {
                            has_receiver: false,
                            class_filter: None,
                        }
                    )
            })
            .collect();
        if candidates.is_empty() {
            // every_dispatch_entry_has_manifest_counterpart catches this.
            continue;
        }

        for entry in candidates {
            // Skip "no signature data" entries — the emitter
            // intentionally falls back to `(...args: any[]): any`.
            if entry.params.is_empty() && entry.returns == TypeSpec::Any {
                continue;
            }

            // Count manifest params (treating Rest as one slot for
            // arity purposes — the dispatch table represents varargs
            // as a single NA_VARARGS slot too).
            let manifest_arity = entry.params.len();
            let dispatch_arity = sig.arg_kinds.len();
            if manifest_arity != dispatch_arity {
                mismatches.push(format!(
                    "{}::{} arity drift: manifest declares {} param(s), dispatch table has {} ({:?})",
                    sig.module, sig.method, manifest_arity, dispatch_arity, sig.arg_kinds
                ));
                continue;
            }

            // Per-slot check — manifest is allowed to NARROW
            // (Any → String/Number/...), but it can't WIDEN onto a
            // type the dispatch path won't accept.
            for (idx, (param, dkind)) in entry.params.iter().zip(sig.arg_kinds.iter()).enumerate() {
                let m_ty = match param {
                    ParamSpec::Named { ty, .. } => *ty,
                    ParamSpec::Rest { ty, .. } => *ty,
                };
                // Allowed combinations:
                //   - dispatch NA_STR : manifest String/Any (narrowing is fine)
                //   - dispatch NA_F64/PTR/JSV : manifest anything
                //     (these slots accept NaN-boxed values of any TS
                //     shape, including string)
                //   - dispatch NA_VARARGS : manifest Rest of any type
                let ok = match (*dkind, m_ty) {
                    ("NA_STR", TypeSpec::String) | ("NA_STR", TypeSpec::Any) => true,
                    // NA_STR coerces strictly — manifest claiming
                    // Number/Bool/etc. would cause `tsc` to accept a
                    // call that codegen would then mis-coerce.
                    ("NA_STR", _) => false,
                    // Other dispatch kinds (NaN-boxed JSValue paths)
                    // accept arbitrary types at the FFI boundary.
                    (_, _) => true,
                };
                if !ok {
                    mismatches.push(format!(
                        "{}::{} param {}: manifest {:?} can't narrow dispatch {} (codegen would mis-coerce)",
                        sig.module, sig.method, idx, m_ty, dkind
                    ));
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "API_MANIFEST has {} param-shape drift(s) vs NATIVE_MODULE_TABLE:\n  {}\n\n\
         The manifest is allowed to declare a tighter type than the dispatch \
         table accepts (Any → String narrowing is fine), but it can't claim \
         a type the dispatch path will mis-coerce.",
        mismatches.len(),
        mismatches.join("\n  ")
    );
}

/// #513 — reverse-direction drift guard.
///
/// Every supported native module (entry in
/// `perry_api_manifest::NATIVE_MODULES`) must have **at least one**
/// counterpart entry in `API_MANIFEST`. Without this, the
/// unimplemented-API gate (#463) silently falls through to the old
/// permissive behavior — `module_has_any_entries(M)` returns false for
/// any module with zero entries, which keeps the same class of bug
/// Justin hit in #455 alive on un-enumerated modules.
///
/// The test is INTENTIONALLY structural: it does NOT enforce that the
/// manifest enumerates every method the runtime supports — that would
/// require shadowing every dispatch table and runtime extern, which is
/// the opposite direction's drift (already covered by
/// `every_dispatch_entry_has_manifest_counterpart`). It only asserts
/// that strict mode is FLIPPED ON for each supported module — the
/// minimum condition for the unimplemented-API check to fire.
///
/// Side-effect-only sub-path imports (`import 'dotenv/config'`, no
/// value binding, no member access) are allowed-listed below — they
/// have no user-facing surface to enumerate, and `module_has_any_entries`
/// returning false for them is benign because no user code ever reads
/// properties on them.
#[test]
fn every_native_module_has_at_least_one_manifest_entry() {
    /// Modules in NATIVE_MODULES whose import is purely a side-effect
    /// (no value binding, no property access). Adding these to the
    /// allowed list documents the exception so a future module that
    /// genuinely lacks coverage doesn't sneak past CI by being added
    /// here.
    const SIDE_EFFECT_ONLY: &[&str] = &["dotenv/config"];

    let mut missing: Vec<&'static str> = Vec::new();
    for &module in perry_api_manifest::NATIVE_MODULES {
        if SIDE_EFFECT_ONLY.contains(&module) {
            continue;
        }
        if !perry_api_manifest::module_has_any_entries(module) {
            missing.push(module);
        }
    }

    assert!(
        missing.is_empty(),
        "{} module(s) in NATIVE_MODULES have zero entries in API_MANIFEST:\n  {}\n\n\
         Add at least one entry per module to crates/perry-api-manifest/src/entries.rs — \
         without it, the unimplemented-API check (#463) silently falls through to the \
         old permissive behavior on those modules. (#513)",
        missing.len(),
        missing.join("\n  ")
    );
}

/// #513 — every well-known binding must appear in API_MANIFEST.
///
/// The well-known bindings table at `crates/perry/well_known_bindings.toml`
/// declares which npm packages route to a bundled `perry-ext-*` crate.
/// Each routed module name must have at least one manifest entry so the
/// unimplemented-API check covers it. Aliases (`mysql2/promise` → same
/// crate as `mysql2`) need their own manifest entries — the user
/// imports the alias directly, so the gate consults the alias name.
///
/// Side-effect-only sub-paths (`dotenv/config`) are allowed-listed in
/// the sibling test above and excluded here too.
#[test]
fn every_well_known_binding_has_manifest_entry() {
    const SIDE_EFFECT_ONLY: &[&str] = &["dotenv/config"];

    // Inline parse of well_known_bindings.toml — small enough that
    // pulling in `toml` as a dev-dep just for this test would be
    // overkill, and the format is regular enough to scan with the
    // standard library. The file is part of the perry crate; resolve
    // its path relative to CARGO_MANIFEST_DIR (this test runs from
    // crates/perry-codegen).
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let toml_path = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("perry")
        .join("well_known_bindings.toml");
    let toml = std::fs::read_to_string(&toml_path)
        .unwrap_or_else(|e| panic!("read {}: {}", toml_path.display(), e));

    let mut missing: Vec<String> = Vec::new();
    for line in toml.lines() {
        let line = line.trim();
        // `[bindings.<name>]` or `[bindings."<name>">]` (quoted form
        // used for names with `/` or `.`).
        let Some(rest) = line.strip_prefix("[bindings.") else {
            continue;
        };
        let Some(name_with_bracket) = rest.strip_suffix(']') else {
            continue;
        };
        // Strip optional surrounding double quotes.
        let name = name_with_bracket
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(name_with_bracket);
        if SIDE_EFFECT_ONLY.contains(&name) {
            continue;
        }
        if !perry_api_manifest::module_has_any_entries(name) {
            missing.push(name.to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "{} well-known binding(s) have zero entries in API_MANIFEST:\n  {}\n\n\
         Each entry in crates/perry/well_known_bindings.toml routes a user \
         import to a bundled perry-ext-* crate. Add at least one manifest \
         entry per routed module so the unimplemented-API check covers it. (#513)",
        missing.len(),
        missing.join("\n  ")
    );
}
