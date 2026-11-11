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
               put(1, 2); console.log(xs[1]);";
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
         console.log(f(3));"
    );
    assert_eq!(sign_at(&src, "K[i >>> 0]", FactsConfig::default()), None);
    // An unknown Number.
    let src = format!(
        "{TABLE}export function f(i: number): number {{ return K[i] ?? 0; }}\n\
         console.log(f(3));"
    );
    assert_eq!(sign_at(&src, "K[i]", FactsConfig::default()), None);
}

#[test]
fn a_compound_store_has_one_hull_with_its_read() {
    let src = "const xs: number[] = [0, 0, 0, 0];\n\
               export function bump(i: number): void { xs[i & 3] += 1; }\n\
               bump(1); console.log(xs[1]);";
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
