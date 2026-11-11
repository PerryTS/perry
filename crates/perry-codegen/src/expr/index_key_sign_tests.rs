//! Key signs (`index_key::key_fact`, wpfacts `key_signs`): an element site
//! whose key the facts prove an int32 takes no normalizer, and one whose key
//! is proven non-negative takes no sign test and has no property-key arm.
//! Without the fact (or with an interval past int32) the site keeps both, so
//! each test can fail.

use std::sync::Arc;

use crate::codegen::{ElisionFacts, ProgramFactsView};
use crate::{compile_module, CompileOptions};

const SRC: &str = "export function at(a: number[], i: number): number { return a[i]; }\n\
                   export function any(a: number[], i: number): unknown { return a[i]; }\n\
                   export function put(a: number[], i: number, v: number): void { a[i] = v; }\n";

/// IR of `SRC` with the key interval `key` on every `a[i]` site.
fn ir(key: Option<(i64, i64)>) -> String {
    ir_with(key, None)
}

/// IR of `SRC` with the int32 key interval `key` and the wide key interval
/// `wide` on every `a[i]` site.
fn ir_with(key: Option<(i64, i64)>, wide: Option<(i64, i64)>) -> String {
    let mut elision = ElisionFacts::default();
    // SWC offsets are 1-based; a site's offset is its object's start.
    for (i, _) in SRC.match_indices("a[i]") {
        if let Some(k) = key {
            elision.key_signs.insert(i as u32 + 1, k);
        }
        if let Some(k) = wide {
            elision.wide_keys.insert(i as u32 + 1, k);
        }
    }
    let opts = CompileOptions {
        emit_ir_only: true,
        program_facts: ProgramFactsView {
            elision: Arc::new(elision),
            ..Default::default()
        },
        ..Default::default()
    };
    let src = SRC.to_string();
    let m = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = perry_diagnostics::SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(&src, "ks.ts", &mut cache)
                .expect("parses");
            perry_hir::lower_module(&parsed.module, "ks", "ks.ts").expect("lowers")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread");
    String::from_utf8(compile_module(&m, opts).expect("fixture compiles")).expect("UTF-8 IR")
}

const NORMALIZE: &str = "call i32 @perry_array_index_normalize_i32";

#[test]
fn an_unproven_key_keeps_its_normalizer_and_the_property_key_arm() {
    let without = ir(None);
    assert!(without.contains(NORMALIZE), "{without}");
    assert!(without.contains("aidx.runtime_key"), "{without}");
    // An integer interval past the int32 range is no key fact.
    let wide = ir(Some((0, 4_294_967_295)));
    assert!(wide.contains(NORMALIZE), "{wide}");
    assert!(wide.contains("aidx.runtime_key"), "{wide}");
}

#[test]
fn a_proven_index_takes_no_normalizer_and_no_sign_test() {
    let with = ir(Some((0, 100)));
    assert!(!with.contains(NORMALIZE), "no normalizer\n{with}");
    assert!(
        !with.contains("aidx.runtime_key"),
        "no property-key arm\n{with}"
    );
    assert!(
        !with.contains("ubound.cold"),
        "no negative exit on the miss arm\n{with}"
    );
    assert!(
        with.contains("fptosi double"),
        "one exact conversion\n{with}"
    );
    // The bound stays: the key is not proven inside the Array.
    assert!(with.contains("icmp ult i32"), "{with}");
}

#[test]
fn an_int32_key_that_may_be_negative_keeps_its_sign_arm() {
    let with = ir(Some((-5, 100)));
    assert!(!with.contains(NORMALIZE), "no normalizer\n{with}");
    assert!(
        with.contains("aidx.runtime_key"),
        "a negative key reads the named property\n{with}"
    );
    // The miss arm still sends a negative key to the property-key read.
    assert!(with.contains("ubound.cold"), "{with}");
}

/// Wide keys: a non-negative integer key past int32 is clamped and converted
/// (no normalizer call), and keeps the property-key arm for a key at or past
/// 2^31.
#[test]
fn a_wide_key_is_clamped_and_converted_and_keeps_its_property_key_arm() {
    let with = ir_with(None, Some((0, 1 << 60)));
    assert!(!with.contains(NORMALIZE), "no normalizer\n{with}");
    assert!(
        with.contains("fptoui double") && with.contains("4294967295.0"),
        "the clamp and one unsigned conversion\n{with}"
    );
    assert!(
        with.contains("aidx.runtime_key"),
        "a key past the index range reads the named property\n{with}"
    );
    assert!(with.contains("ubound.cold"), "{with}");
    // The store declines the same keys onto its helper.
    assert!(
        with.contains("js_typed_feedback_array_set_index_or_string"),
        "{with}"
    );
}

/// Negative control: a wide interval that may be negative is no fact.
#[test]
fn a_wide_key_that_may_be_negative_keeps_its_normalizer() {
    let with = ir_with(None, Some((-1, 1 << 60)));
    assert!(with.contains(NORMALIZE), "{with}");
    assert!(!with.contains("fptoui double"), "{with}");
}
