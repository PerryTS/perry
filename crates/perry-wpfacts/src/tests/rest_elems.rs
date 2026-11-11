//! I32 at rest, PR B (RFC §10.7): which fused fill Arrays are stored at rest
//! (`rest_elems`), as the dump's `# rest arr` probe lines and the exported
//! element sites report them.

use super::*;
use crate::FactsConfig;

/// The `# rest arr` lines of `src`.
fn arr_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rest arr "))
        .cloned()
        .collect()
}

/// The one `# rest arr` line of `src`.
fn arr_line(src: &str) -> String {
    let ls = arr_lines(src);
    assert_eq!(ls.len(), 1, "{ls:#?}");
    ls[0].clone()
}

/// An int Array that only element sites touch, and `extra` beside them.
fn program(extra: &str) -> String {
    format!(
        r#"
const a = new Array(8).fill(0);
function put(i: number, v: number) {{ a[i & 7] = v & 255; }}
function get(i: number): number {{ return a[i & 7] | 0; }}
let s = 0;
for (let i = 0; i < 100; i++) {{ put(i, i * 3); s = (s + get(i + 1)) | 0; }}
{extra}
console.log(s, a.length);
"#
    )
}

#[test]
fn an_int_array_that_only_element_sites_touch_is_at_rest() {
    let line = arr_line(&program(""));
    assert!(line.ends_with(" at_rest=1"), "{line}");
}

#[test]
fn its_element_sites_are_exported_with_the_floor() {
    let src = program("");
    let pf = facts(&src);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    let at = |text: &str| src.find(text).expect("site in source") as u32 + 1;
    assert_eq!(mb.rest_writes, vec![(at("a[i & 7] = v"), 8)]);
    assert_eq!(mb.rest_reads, vec![(at("a[i & 7] | 0"), 8)]);
    assert_eq!(mb.rest_births.len(), 1, "{:?}", mb.rest_births);
}

#[test]
fn a_generic_reader_keeps_the_array_double() {
    for extra in [
        "console.log(JSON.stringify(a));",
        "console.log(a.indexOf(3));",
        "for (const x of a) s += x;",
        "console.log([...a].length);",
        "console.log(Object.values(a).length);",
        "const [x0] = a; s += x0;",
        "a.fill(1);",
    ] {
        let line = arr_line(&program(extra));
        assert!(line.contains(" at_rest=0 "), "{extra}: {line}");
    }
}

#[test]
fn a_length_that_moves_or_a_non_int_element_keeps_the_array_double() {
    for (extra, why) in [
        ("a.push(1);", ""),
        ("a[0] = 2147483648;", "elem_not"),
        // A possible -0 is tolerated (PR D) unless a use observes its sign.
        ("a[1] = (s & 1) * -1; console.log(1 / a[1]);", "elem_not"),
        ("a[2] = s / 3;", "elem_not"),
    ] {
        let line = arr_line(&program(extra));
        assert!(
            line.contains(" at_rest=0 ") && line.contains(why),
            "{extra}: {line}"
        );
    }
}

#[test]
fn a_site_shared_with_another_array_keeps_both_double() {
    let line = arr_line(&program(
        "const b = [1, 2, 3]; const c = s > 5 ? a : b; s += c[1] | 0;",
    ));
    assert!(line.contains(" at_rest=0 why=mixed_site "), "{line}");
}

#[test]
fn only_a_fused_fill_birth_is_a_candidate() {
    let src = r#"
const a = new Array(8);
for (let i = 0; i < 8; i++) a[i] = i & 3;
console.log(a[3] | 0);
"#;
    assert!(arr_lines(src).is_empty(), "{:#?}", arr_lines(src));
}

/// I32 at rest B2: a class field's `Arr` representation carries the storage
/// kind of the Arrays it holds (`rest_elems::mark_fields`), which the
/// derive could not know before the admission.
#[test]
fn a_field_holding_an_at_rest_array_has_an_at_rest_rep() {
    let field = |extra: &str| {
        let src = format!(
            r#"
class Grid {{
  cells: number[] = new Array(16).fill(0);
  at(i: number): number {{ return this.cells[i & 15] | 0; }}
  put(i: number, v: number): void {{ this.cells[i & 15] = v & 255; }}
}}
const g = new Grid();
let s = 0;
for (let i = 0; i < 64; i++) {{ g.put(i, i * 5); s = (s + g.at(i + 1)) | 0; }}
{extra}
console.log(s);
"#
        );
        let pf = facts(&src);
        let f = tmpl(&pf, "Grid")
            .fields
            .iter()
            .find(|f| f.key == "cells")
            .expect("cells")
            .rep;
        match f {
            crate::Rep::Arr(r) => (r.at_rest, arr_line(&src)),
            other => panic!("cells is {other:?}"),
        }
    };
    let (rest, line) = field("");
    assert!(rest && line.ends_with(" at_rest=1"), "{line}");
    // Negative control: a generic reader keeps the Array double, and the
    // field's representation says so.
    let (rest, line) = field("console.log(JSON.stringify(g.cells));");
    assert!(!rest && line.contains(" at_rest=0 "), "{line}");
}

/// The capacity reservation: a length that moves inside a small hull
/// (`hi <= 2 * lo`, `hi <= 1024`) is at rest, its birth reserves `hi`, and
/// the floor stays the birth length.
#[test]
fn a_length_that_moves_inside_a_small_hull_reserves_its_upper_bound() {
    let src = program("function grow(i: number, v: number) { a[i & 15] = v & 255; }\ngrow(s, 3);");
    let line = arr_line(&src);
    assert!(line.ends_with(" at_rest=1 cap=16"), "{line}");
    let pf = facts(&src);
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    assert_eq!(mb.rest_births.len(), 1, "{:?}", mb.rest_births);
    assert_eq!(mb.rest_reserves, vec![(mb.rest_births[0], 16)]);
    assert_eq!(mb.rest_writes.len(), 2, "{:?}", mb.rest_writes);
    assert!(mb.rest_writes.iter().all(|&(_, floor)| floor == 8));
    assert!(mb.rest_reads.iter().all(|&(_, floor)| floor == 8));
    // A fixed length reserves nothing.
    let pf = facts(&program(""));
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    assert_eq!(mb.rest_births.len(), 1);
    assert!(mb.rest_reserves.is_empty(), "{:?}", mb.rest_reserves);
}

/// Negative controls of the reservation: a hull past twice the birth
/// length, a hull past 1024 slots, and a key that is no integer interval.
#[test]
fn a_hull_over_the_cap_or_unbounded_keeps_the_array_double() {
    for (extra, why) in [
        (
            "function grow(i: number) { a[i & 31] = 1; }\ngrow(s);",
            "why=length_over_cap len=[8,32]",
        ),
        (
            "function grow(i: number) { a[i / 3] = 1; }\ngrow(s);",
            "why=length_moves len=top",
        ),
    ] {
        let line = arr_line(&program(extra));
        assert!(
            line.contains(" at_rest=0 ") && line.contains(why),
            "{extra}: {line}"
        );
    }
    let big = |key: &str| {
        format!(
            r#"
const a = new Array(1000).fill(0);
function put(i: number, v: number) {{ a[{key}] = v & 255; }}
function get(i: number): number {{ return a[i & 511] | 0; }}
let s = 0;
for (let i = 0; i < 100; i++) {{ put(i, i * 3); s = (s + get(i + 1)) | 0; }}
console.log(s, a.length);
"#
        )
    };
    let line = arr_line(&big("i & 1023"));
    assert!(line.ends_with(" at_rest=1 cap=1024"), "{line}");
    let line = arr_line(&big("(i & 1023) + 512"));
    assert!(
        line.contains(" at_rest=0 why=length_over_cap len=[1000,1536]"),
        "{line}"
    );
}
