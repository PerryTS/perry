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
        ("a[1] = (s & 1) * -1;", "elem_not"),
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
