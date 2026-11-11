//! `call_kinds`: the primitive kinds of a call's result, joined over the
//! copies of the call.

use crate::model::prim;
use crate::FactsConfig;

/// The kinds of the call that starts at `at` (SWC offsets are 1-based).
fn kinds(src: &str, at: &str, cfg: &FactsConfig) -> Option<u16> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], cfg);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let offset = src.find(at).expect("call in source") as u32 + 1;
    let mut found = mb.call_kinds.iter().filter(|(o, _, _)| *o == offset);
    let first = found.next().map(|(_, _, k)| *k);
    assert!(found.next().is_none(), "one call starts at {at}");
    first
}

const SRC: &str = r#"
function name(i: number): string { return "abcdefgh".charAt(i & 7) + String(i >> 3); }
function promo(p: number): string | null { if (p === 5) return "q"; return null; }
function mixed(i: number): string | number { return i > 2 ? "s" : i; }
function obj(i: number): { v: number } { return { v: i }; }
const RUN = Math.random() < 2 ? 1 : 0;
let out = "";
for (let i = 0; i < 9 * RUN; i++) {
  out += name(i) + name(i + 1) + (promo(i) ?? "");
  out += String(mixed(i)) + obj(i).v;
}
console.log(out);
"#;

#[test]
fn a_call_that_returns_only_strings_is_a_string_call() {
    let cfg = FactsConfig::default();
    assert_eq!(kinds(SRC, "name(i) +", &cfg), Some(prim::STR));
    assert_eq!(kinds(SRC, "name(i + 1)", &cfg), Some(prim::STR));
}

#[test]
fn a_nullable_string_call_names_both_kinds() {
    let cfg = FactsConfig::default();
    assert_eq!(kinds(SRC, "promo(i)", &cfg), Some(prim::STR | prim::NULL));
}

#[test]
fn a_mixed_call_names_both_kinds_and_an_object_call_is_no_primitive_call() {
    let cfg = FactsConfig::default();
    assert_eq!(kinds(SRC, "mixed(i)", &cfg), Some(prim::STR | prim::NUM));
    assert_eq!(kinds(SRC, "obj(i)", &cfg), None);
}

#[test]
fn the_negative_control_claims_every_call_a_string_call() {
    let cfg = FactsConfig {
        sabotage: Some("stringcall".to_string()),
        ..Default::default()
    };
    assert_eq!(kinds(SRC, "mixed(i)", &cfg), Some(prim::STR));
    assert_eq!(kinds(SRC, "obj(i)", &cfg), Some(prim::STR));
}

#[test]
fn the_probe_names_each_site() {
    let module = perry_parser::parse_typescript(SRC, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        diagnostics: true,
        ..Default::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let lines: Vec<_> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# call_kinds "))
        .collect();
    assert!(lines.iter().any(|l| l.ends_with("kinds=str")), "{lines:#?}");
    assert!(
        lines.iter().any(|l| l.ends_with("kinds=str|null")),
        "{lines:#?}"
    );
}

/// `name(i).trim()` holds two calls that start at `name`: the span tells
/// the string call `name(i)` from the call of its `trim`.
#[test]
fn calls_that_start_at_one_offset_are_told_apart_by_their_end() {
    let src = SRC.replace(
        "console.log(out);",
        "console.log(out, name(RUN).trim().length);",
    );
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let offset = src.find("name(RUN)").expect("call in source") as u32 + 1;
    let inner_end = offset + "name(RUN)".len() as u32;
    let at: Vec<_> = mb
        .call_kinds
        .iter()
        .filter(|(o, _, _)| *o == offset)
        .collect();
    assert!(
        at.iter()
            .any(|(_, e, k)| *e == inner_end && *k == prim::STR),
        "{at:#?}"
    );
    assert!(at.iter().all(|(_, e, _)| *e >= inner_end), "{at:#?}");
}
