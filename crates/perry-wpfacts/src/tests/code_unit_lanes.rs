//! Rule CU and comma-expression forwarding (`gen/in_bounds.rs`
//! `code_unit_read`, `gen/narrow.rs` `narrow_assigned_expr`): the FNV
//! loop of Next's ETag (`e ^= a.charCodeAt(c++), d = 435 * e, ...`) keeps
//! its lanes inside int32.

use super::*;
use crate::FactsConfig;

fn rep_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rep "))
        .cloned()
        .collect()
}

fn code_unit_program(guard: &str, key: &str) -> String {
    format!(
        r#"
function h(a: string): number {{
  let b = a.length, c = 0, e = 0, d = 0;
  for (; {guard};) e ^= a.charCodeAt({key}), d = 435 * e;
  return d;
}}
console.log(h("abc"));
"#
    )
}

fn holds_code_unit(ls: &[String]) -> bool {
    ls.iter().any(|l| l.contains("range=[0,65535]"))
}

#[test]
fn a_charcode_below_the_length_is_a_code_unit() {
    for (guard, key) in [("c < b", "c++"), ("c < a.length", "c++")] {
        let ls = rep_lines(&code_unit_program(guard, key));
        assert!(holds_code_unit(&ls), "{guard} / {key}: {ls:#?}");
        // `d = 435 * e` is inside int32.
        assert!(
            ls.iter()
                .all(|l| !l.contains("demand=1") || l.contains("rep=i32")),
            "{guard} / {key}: {ls:#?}"
        );
    }
}

#[test]
fn a_charcode_that_may_be_past_the_end_is_no_code_unit() {
    // `<=` reaches the length, `++c` reads past the tested value, and a
    // counter that also falls may be negative: each may read NaN.
    for (guard, key) in [("c <= b", "c++"), ("c < b", "++c"), ("c < b", "c--")] {
        let ls = rep_lines(&code_unit_program(guard, key));
        assert!(!holds_code_unit(&ls), "{guard} / {key}: {ls:#?}");
    }
}

#[test]
fn a_length_alias_of_a_written_receiver_is_no_bound() {
    let src = r#"
function h(a: string): number {
  let b = a.length, c = 0, e = 0;
  for (; c < b;) { e ^= a.charCodeAt(c++); a = a + a; }
  return e;
}
console.log(h("abc"));
"#;
    assert!(!holds_code_unit(&rep_lines(src)));
}

#[test]
fn a_write_in_a_comma_expression_is_what_its_later_elements_read() {
    // `f += f >>> 16` reads the `435 * k` just written, not every value of
    // `f`: without the forwarding the sum grows past int32 and widens.
    let src = r#"
function g(n: number): number {
  let f = 0, k = 7;
  for (let i = 0; i < n; i++) f = 435 * k, f += f >>> 16, k = f & 65535;
  return k;
}
console.log(g(100));
"#;
    let ls = rep_lines(src);
    assert!(
        ls.iter()
            .all(|l| !l.contains("demand=1") || l.contains("rep=i32")),
        "{ls:#?}"
    );
}

#[test]
fn a_closure_that_writes_the_local_ends_no_forwarding_it_can_break() {
    // `x` is captured mutably: no comma forwarding, its interval stays the
    // join of every write (the closure's 1e10 too).
    let src = r#"
function g(): number {
  let x = 1;
  const w = () => { x = 1e10; };
  const r = (x = 5, w(), x | 0);
  return r + x;
}
console.log(g());
"#;
    let ls = rep_lines(src);
    assert!(ls.iter().any(|l| l.contains("10000000000")), "{ls:#?}");
}
