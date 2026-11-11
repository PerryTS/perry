//! I32 at rest, PR C (RFC §10.7): which class fields are I32 lanes
//! (`rest_fields`), as the dump's `# rest lane` probe lines and
//! `FieldFacts::rest` report them.

use super::*;
use crate::FactsConfig;

/// The `# rest lane` lines of `src`.
fn lane_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rest lane "))
        .cloned()
        .collect()
}

/// The `# rest lane` line of `src` naming `field` (`Class.key`).
fn lane_line(src: &str, field: &str) -> String {
    let ls = lane_lines(src);
    let hit: Vec<&String> = ls
        .iter()
        .filter(|l| l.contains(&format!("{field} ")))
        .collect();
    assert_eq!(hit.len(), 1, "{field}: {ls:#?}");
    hit[0].clone()
}

/// A class with an int32 field that only named sites touch, and `extra`
/// beside it.
fn program(extra: &str) -> String {
    format!(
        r#"
class Acc {{
  v = 0;
  bump(k: number): void {{ this.v = (this.v + k) & 1023; }}
  read(): number {{ return this.v ^ 5; }}
}}
const a = new Acc();
let s = 0;
for (let i = 0; i < 100; i++) {{ a.bump(i); s = (s + a.read()) | 0; }}
{extra}
console.log(s);
"#
    )
}

#[test]
fn an_int_field_that_only_named_sites_touch_is_a_lane() {
    let line = lane_line(&program(""), "Acc.v");
    assert!(line.ends_with(" at_rest=1"), "{line}");
    let pf = facts(&program(""));
    let acc = pf
        .templates
        .iter()
        .find(|t| t.display.ends_with("Acc"))
        .expect("Acc template");
    assert!(
        acc.fields.iter().any(|f| f.key == "v" && f.rest),
        "{acc:#?}"
    );
}

#[test]
fn an_object_literal_class_copy_is_a_lane() {
    let src = program(
        r#"
interface Snap { v: number; }
function snap(x: Acc): Snap { return { v: x.read() & 255 }; }
const keep: Snap[] = [];
for (let i = 0; i < 10; i++) keep.push(snap(a));
let t = 0;
for (const k of keep) t = (t + k.v) | 0;
console.log(t);
"#,
    );
    let ls = lane_lines(&src);
    assert!(
        ls.iter()
            .any(|l| l.contains(".v ") && l.contains("AnonShape") && l.ends_with(" at_rest=1")),
        "{ls:#?}"
    );
}

#[test]
fn a_generic_reader_refuses_the_class() {
    for (extra, why) in [
        ("console.log(JSON.stringify(a));", "why="),
        ("console.log(Object.keys(a).length);", "why="),
        ("const c = { ...a }; console.log(c.v);", "why="),
        (
            "const k = process.argv.length > 9 ? 'w' : 'v'; (a as any)[k] = 3;",
            "why=",
        ),
        ("console.log(a);", "why="),
        ("const ch = Object.create(a); console.log(ch.v);", "why="),
    ] {
        let line = lane_line(&program(extra), "Acc.v");
        assert!(
            line.contains(" at_rest=0 ") && line.contains(why),
            "{extra}: {line}"
        );
    }
}

#[test]
fn a_class_with_heritage_is_refused() {
    let src = program("class Sub extends Acc { u = 1; } const sb = new Sub(); sb.bump(3); console.log(sb.read(), sb.u);");
    let line = lane_line(&src, "Acc.v");
    assert!(line.contains(" at_rest=0 why=extended"), "{line}");
}

#[test]
fn a_field_that_is_not_int32_has_no_lane_line() {
    // 2^31, a fraction: the field rep is F64, not I32 (PR A): no lane line.
    for v in ["2147483648 + k", "k + 0.5"] {
        let src = format!(
            r#"
class W {{ v = 0; put(k: number): void {{ this.v = {v}; }} read(): number {{ return this.v; }} }}
const w = new W();
let s = 0;
for (let i = 0; i < 10; i++) {{ w.put(i); s += w.read(); }}
console.log(s);
"#
        );
        assert!(
            !lane_lines(&src).iter().any(|l| l.contains("W.v ")),
            "{v}: {:#?}",
            lane_lines(&src)
        );
    }
}

#[test]
fn the_sabotage_admits_an_f64_field() {
    let src = r#"
class W { v = 0; put(k: number): void { this.v = 2147483648 + k; } read(): number { return this.v % 1000; } }
const w = new W();
let s = 0;
for (let i = 0; i < 10; i++) { w.put(i); s += w.read(); }
console.log(s);
"#;
    assert!(!lane_lines(src).iter().any(|l| l.contains("W.v ")));
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            sabotage: Some("restlane".into()),
            ..Default::default()
        },
    );
    let line = pf
        .rep_probe
        .iter()
        .find(|l| l.starts_with("# rest lane ") && l.contains("W.v "))
        .cloned()
        .unwrap_or_default();
    assert!(line.ends_with(" at_rest=1"), "{line}");
}
