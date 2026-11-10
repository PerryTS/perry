//! I32 at rest (RFC §10.7): the intervals of field and element nodes, as
//! the dump's `# rest` probe lines report them.

use super::*;
use crate::FactsConfig;

/// The `# rest` lines of `src`.
fn rest_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rest "))
        .cloned()
        .collect()
}

/// The `# rest` line naming `what` (`field C.k`, `elem <display>`).
fn rest(src: &str, what: &str) -> String {
    let ls = rest_lines(src);
    ls.iter()
        .find(|l| l.contains(what))
        .cloned()
        .unwrap_or_else(|| panic!("no `{what}` in {ls:#?}"))
}

/// The representation wpfacts gives field `key` of class `class`.
fn field_rep(src: &str, class: &str, key: &str) -> crate::bindings::Rep {
    let pf = facts(src);
    let t = tmpl(&pf, class);
    t.fields
        .iter()
        .find(|f| f.key == key)
        .unwrap_or_else(|| panic!("no field {key} in {}", pf.dump_text()))
        .rep
}

const FIELDS: &str = r#"
class C {
  small = 0;
  wide = 0;
  frac = 0;
  zero = 1;
  mixed: number | string = 0;
  plain = 0;
  bump(k: number, n: number): number {
    this.small = (this.small + 1) & 7;
    this.wide = 2147483648 + (k & 1);
    this.frac = k / 3;
    this.zero = (k - k) * n;
    if (k > 50) this.mixed = "s";
    this.plain = k & 15;
    return (this.small | 0) + (this.wide & 1) + (this.zero | 0) + this.plain;
  }
}
const c = new C();
let s = 0;
const n = process.argv.length > 100 ? 1 : -1;
for (let i = 0; i < 100; i++) s += c.bump(i, n);
console.log(s, c.frac, c.mixed);
"#;

#[test]
fn a_number_field_takes_the_representation_of_its_nodes() {
    use crate::bindings::Rep;
    assert_eq!(field_rep(FIELDS, "C", "small"), Rep::I32);
    // Past int32, a fraction, a possible -0: Numbers, not int32.
    assert_eq!(field_rep(FIELDS, "C", "wide"), Rep::F64);
    assert_eq!(field_rep(FIELDS, "C", "frac"), Rep::F64);
    assert_eq!(field_rep(FIELDS, "C", "zero"), Rep::F64);
    // A field that may hold a String is no Number field.
    assert_eq!(field_rep(FIELDS, "C", "mixed"), Rep::Boxed);
}

#[test]
fn an_escaped_or_switched_off_field_is_boxed() {
    use crate::bindings::Rep;
    let escaped = format!("{FIELDS}\n(globalThis as any).leak = c;\n");
    assert_eq!(field_rep(&escaped, "C", "small"), Rep::Boxed);
}

#[test]
fn probe_prints_field_and_element_intervals() {
    let src = r#"
class C {
  n = 0;
  b: number[] = new Array<number>(8).fill(0);
  bump(k: number): number {
    this.n = (this.n + 1) & 7;
    this.b[this.n] = k & 3;
    return this.b[this.n] ?? 0;
  }
}
const c = new C();
let s = 0;
for (let i = 0; i < 100; i++) s += c.bump(i);
console.log(s);
"#;
    let f = rest(src, "field t.ts:C.n ");
    assert!(f.contains("range=[0,7]") && f.contains("demand=1"), "{f}");
    assert!(rest(src, "elem ").contains("range=[0,3]"));
}
