//! A built-in runs the program code its operands hold while it converts
//! them: `fill`'s start and end are ToIntegerOrInfinity'd (ECMA-262
//! 23.1.3.7, ToPrimitive: `valueOf`), `toLocaleString` invokes each
//! element's own `toLocaleString` (ECMA-402 19.5.1). The receivers are
//! `any`, so each call stays a call of the native. CXCHK0 gap witnesses
//! test_gap_array_fill_coercion_callbacks, test_gap_array_fill_coercion_gc,
//! test_gap_array_dyndispatch_2803_2808_2791.

use super::*;

/// Whether `hit` is an active body in `src`.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn fill_coerces_its_start_and_end() {
    for (name, src) in [
        (
            "start",
            r#"
function hit() { return 0; }
const a: any = [1, 2, 3];
a.fill(7, { valueOf() { return hit(); } });
console.log(a.join(","));
"#,
        ),
        (
            "end",
            r#"
function hit() { return 2; }
const a: any = [1, 2, 3];
a.fill(7, 0, { valueOf() { return hit(); } });
console.log(a.join(","));
"#,
        ),
    ] {
        assert!(hit_runs(src), "{name}: hit inactive, Node runs it");
    }
}

#[test]
fn fill_does_not_coerce_its_value() {
    // Control: the value is stored, never converted.
    let src = r#"
function hit() { return 0; }
const a: any = [1, 2, 3];
a.fill({ valueOf() { return hit(); } });
console.log(a.length);
"#;
    assert!(!hit_runs(src), "hit active, Node never runs it");
}

#[test]
fn to_locale_string_invokes_each_elements_own() {
    let src = r#"
function hit() { return "x"; }
const obj = { toLocaleString(l: any, o: any) { return hit(); } };
console.log(([obj] as any).toLocaleString("xx-YY", { tag: "opt" }));
"#;
    assert!(hit_runs(src), "hit inactive, Node runs it");
    // Control: toString joins, it never calls an element's toLocaleString.
    let src = r#"
function hit() { return "x"; }
const obj = { toLocaleString(l: any, o: any) { return hit(); } };
console.log(([obj] as any).toString());
"#;
    assert!(!hit_runs(src), "toString: hit active, Node never runs it");
}
