//! Fill births (`fill_births`): which fused fills `new Array(k).fill(v)`
//! are exported as one birth, with their length.

use super::*;

/// The exported fill births of `src`: `(offset, k)`.
fn births(src: &str) -> Vec<(u32, u32)> {
    let pf = facts(src);
    pf.bindings_for("t.ts")
        .expect("module bindings")
        .fill_births
        .clone()
}

#[test]
fn a_fused_fill_of_a_literal_length_is_a_birth_with_that_length() {
    let src = r#"
const a = new Array<number>(8).fill(0);
const b = new Array(3).fill("x");
const c = new Array(0).fill(1);
console.log(a.length, b.length, c.length);
"#;
    let got = births(src);
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(
        got.iter().map(|&(_, k)| k).collect::<Vec<_>>(),
        vec![8, 3, 0]
    );
}

/// The at-rest facts key the same call by the same offset.
#[test]
fn an_at_rest_birth_is_keyed_as_its_fill_birth() {
    let src = r#"
const a = new Array(8).fill(0);
function put(i: number, v: number) { a[i & 7] = v & 255; }
function get(i: number): number { return a[i & 7] | 0; }
let s = 0;
for (let i = 0; i < 100; i++) { put(i, i * 3); s = (s + get(i + 1)) | 0; }
console.log(s, a.length);
"#;
    let pf = facts(src);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    assert_eq!(mb.rest_births.len(), 1, "{:?}", mb.rest_births);
    assert_eq!(mb.fill_births, vec![(mb.rest_births[0], 8)]);
}

/// Negative controls: a length that is no integer literal (it may throw, or
/// be an element), a length past the dense bound, a `fill` with a start or
/// an end, a fill on a named Array, a program class named `Array`.
#[test]
fn every_other_fill_is_no_birth() {
    for src in [
        "let n = 8; n += 1; const a = new Array(n).fill(0); console.log(a.length);",
        "const a = new Array(-1).fill(0); console.log(a.length);",
        "const a = new Array(1.5).fill(0); console.log(a.length);",
        "const a = new Array(\"x\").fill(0); console.log(a.length);",
        "const a = new Array(2000000).fill(0); console.log(a.length);",
        "const a = new Array(8).fill(0, 2); console.log(a.length);",
        "const a = new Array(8).fill(0, 2, 4); console.log(a.length);",
        "const a = new Array(8); a.fill(0); console.log(a.length);",
        "const a = new Array(4, 5).fill(0); console.log(a.length);",
        "class Array { n = 0; constructor(k: number) { this.n = k; } fill(v: number) { return v; } }\nconsole.log(new Array(8).fill(0));",
    ] {
        let got = births(src);
        assert!(got.is_empty(), "{src}: {got:?}");
    }
}

/// One call, one length: the same site in a function called twice is still
/// one birth.
#[test]
fn a_birth_in_a_function_body_is_exported_once() {
    let src = r#"
function make(): number[] { return new Array<number>(8).fill(0); }
const a = make();
const b = make();
console.log(a.length + b.length);
"#;
    let got = births(src);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].1, 8);
}

/// The probe prints one line per birth.
#[test]
fn the_probe_names_each_birth() {
    let pf = facts_with(
        "const a = new Array(8).fill(0); console.log(a.length);",
        crate::FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    let lines: Vec<&String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# fill birth "))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with(" len=8"), "{lines:?}");
}
