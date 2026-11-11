//! Key signs (`element_keys::collect_signs`): the int32 key interval of an
//! element site of any operation, joined over every copy and every site at
//! its offset.

use super::*;
use crate::FactsConfig;

/// The key interval of the element site whose object starts at `at`
/// (`ModuleBindings::key_signs`; SWC offsets are 1-based).
fn sign_at(src: &str, at: &str, cfg: FactsConfig) -> Option<(i64, i64)> {
    let offset = src.find(at).expect("site in source") as u32 + 1;
    let pf = facts_with(src, cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.key_signs
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, k)| *k)
}

const TABLE: &str = "const K = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];\n";

#[test]
fn a_read_and_a_store_export_their_int32_key_interval() {
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i & 7] ?? 0; }}\n\
         console.log(f(3) + f(12));"
    );
    assert_eq!(
        sign_at(&src, "K[i & 7]", FactsConfig::default()),
        Some((0, 7))
    );
    // A store has a key sign too (`element_keys` covers reads only).
    let src = "const xs: number[] = [0, 0, 0, 0];\n\
               export function put(i: number, v: number): void { xs[i & 3] = v; }\n\
               put(1, 2); put(6, 3); console.log(xs[1]);";
    assert_eq!(
        sign_at(src, "xs[i & 3]", FactsConfig::default()),
        Some((0, 3))
    );
}

#[test]
fn a_key_that_may_be_negative_keeps_its_lower_bound() {
    // The parameter joins every caller's argument: an int32, not an index.
    let src = format!(
        "{TABLE}function at(i: number): number {{ return K[i] ?? 0; }}\n\
         console.log(at(2) + at(-1) + at(11));"
    );
    assert_eq!(
        sign_at(&src, "K[i]", FactsConfig::default()),
        Some((-1, 11))
    );
}

#[test]
fn a_key_that_is_no_int32_has_no_sign() {
    // A fraction.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i / 2] ?? 0; }}\n\
         console.log(f(3));"
    );
    assert_eq!(sign_at(&src, "K[i / 2]", FactsConfig::default()), None);
    // A string.
    let src = format!(
        "{TABLE}export function f(k: string): unknown {{ return (K as any)[k]; }}\n\
         console.log(f(\"length\"));"
    );
    assert_eq!(sign_at(&src, "(K as any)[k]", FactsConfig::default()), None);
    // A string that names an index: the key is no Number at run time.
    let src = format!(
        "{TABLE}const k: any = \"3\";\n\
         export function f(): unknown {{ return (K as any)[k]; }}\n\
         console.log(f());"
    );
    assert_eq!(sign_at(&src, "(K as any)[k]", FactsConfig::default()), None);
    // An integer past the int32 range.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i >>> 0] ?? 0; }}\n\
         console.log(f(3) + f(-1));"
    );
    assert_eq!(sign_at(&src, "K[i >>> 0]", FactsConfig::default()), None);
    // A Number with no interval inside int32.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i] ?? 0; }}\n\
         console.log(f(Number(process.argv[2])));"
    );
    assert_eq!(sign_at(&src, "K[i]", FactsConfig::default()), None);
}

#[test]
fn a_compound_store_has_one_hull_with_its_read() {
    let src = "const xs: number[] = [0, 0, 0, 0];\n\
               export function bump(i: number): void { xs[i & 3] += 1; }\n\
               bump(1); bump(6); console.log(xs[1]);";
    assert_eq!(
        sign_at(src, "xs[i & 3]", FactsConfig::default()),
        Some((0, 3))
    );
}

#[test]
fn the_sabotage_claims_every_int32_key_non_negative() {
    let src = format!(
        "{TABLE}function at(i: number): number {{ return K[i] ?? 0; }}\n\
         console.log(at(2) + at(-1) + at(11));"
    );
    let cfg = FactsConfig {
        sabotage: Some("keysign".to_string()),
        ..Default::default()
    };
    assert_eq!(sign_at(&src, "K[i]", cfg), Some((0, 11)));
}

/// The wide key interval of the element site whose object starts at `at`
/// (`ModuleBindings::wide_keys`).
fn wide_at(src: &str, at: &str, cfg: FactsConfig) -> Option<(i64, i64)> {
    let offset = src.find(at).expect("site in source") as u32 + 1;
    let pf = facts_with(src, cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    mb.wide_keys
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, k)| *k)
}

#[test]
fn a_non_negative_integer_key_past_int32_is_wide() {
    // A uint32.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i >>> 0] ?? 0; }}\n\
         console.log(f(3) + f(-1));"
    );
    assert_eq!(
        wide_at(&src, "K[i >>> 0]", FactsConfig::default()),
        Some((0, 4_294_967_295))
    );
    assert_eq!(sign_at(&src, "K[i >>> 0]", FactsConfig::default()), None);
    // A counter bounded below by its loop test only.
    let src = "const xs: number[] = [3, 1, 2];\n\
               export function f(n: number): number {\n\
                 let s = 0;\n\
                 let j = n;\n\
                 while (j >= 0 && s < 100) { s += xs[j] ?? 0; j--; }\n\
                 return s;\n\
               }\n\
               console.log(f(Number(process.argv[2]) | 0) + f(2));";
    let wide = wide_at(src, "xs[j]", FactsConfig::default());
    assert!(matches!(wide, Some((0, hi)) if hi > i64::from(i32::MAX)) || wide.is_none());
}

#[test]
fn an_int32_key_and_a_key_that_may_be_negative_are_not_wide() {
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i & 7] ?? 0; }}\n\
         console.log(f(3) + f(12));"
    );
    assert_eq!(wide_at(&src, "K[i & 7]", FactsConfig::default()), None);
    // `(i >>> 0) - 1` may be -1.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[(i >>> 0) - 1] ?? 0; }}\n\
         console.log(f(3) + f(0));"
    );
    assert_eq!(
        wide_at(&src, "K[(i >>> 0) - 1]", FactsConfig::default()),
        None
    );
    // A fraction has no interval.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[(i >>> 0) / 2] ?? 0; }}\n\
         console.log(f(3));"
    );
    assert_eq!(
        wide_at(&src, "K[(i >>> 0) / 2]", FactsConfig::default()),
        None
    );
}

#[test]
fn the_wide_sabotage_claims_a_key_that_may_be_negative() {
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[(i >>> 0) - 1] ?? 0; }}\n\
         console.log(f(3) + f(0));"
    );
    let cfg = FactsConfig {
        sabotage: Some("widekey".to_string()),
        ..Default::default()
    };
    assert_eq!(
        wide_at(&src, "K[(i >>> 0) - 1]", cfg),
        Some((0, 4_294_967_294))
    );
}
