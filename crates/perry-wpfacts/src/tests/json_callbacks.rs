//! A `JSON.stringify` replacer function is called with `this` = each
//! holder and (key, value) for every value the serialization visits, the
//! root's wrapper `{"": value}` included, after `toJSON(key)`; a
//! `JSON.parse` reviver likewise on every parsed value, its result stored
//! into the holder. The solve only escaped the function (unknown code,
//! unknown arguments), so an object whose only way to the replacer was the
//! serialization kept its proven field kinds while the replacer stored a
//! string into them (check mode: "F2 kind (raw f64)" on
//! object/test_vr_replacer_hole_main). Each program stores a string into a
//! Number field `n` only through the callback; the control is the same
//! solve without the rule (`noreachjsonreplacer`, what
//! `PERRY_FACTS_REACH_JSON_REPLACER=0` gives). A replacer that is absent,
//! a literal `null` / `undefined` or a literal array is never called and
//! loses nothing.

use super::*;

fn dump(src: &str, rule: bool) -> String {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        sabotage: (!rule).then(|| "noreachjsonreplacer".to_string()),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    crate::compute(&[("t.ts".to_string(), &hir)], &cfg).dump_text()
}

/// The kinds the facts state for field `n` of every layout.
fn n_kinds(src: &str, rule: bool) -> Vec<String> {
    dump(src, rule)
        .lines()
        .filter(|l| l.starts_with("FACT F2 kind ") && l.contains(".n = "))
        .map(|l| l.rsplit(".n = ").next().unwrap_or("").to_string())
        .collect()
}

/// The lines of a facts dump that state facts: templates, their fields,
/// the facts and the site verdicts (no node, call or work counts).
fn fact_lines(src: &str, rule: bool) -> Vec<String> {
    dump(src, rule)
        .lines()
        .filter(|l| {
            ["T ", "  F ", "FACT ", "S "]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .map(str::to_string)
        .collect()
}

const HOLES: &[(&str, &str)] = &[
    (
        "the replacer stores through `this`",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
JSON.stringify({ p, tag: "t" }, function (this: any, k: string, v: any) {
  if (k === "tag") this.p.n = "str";
  return v;
});
console.log(p.n * 2);
"#,
    ),
    (
        "the replacer stores into its value argument",
        r#"
const p = { n: 1 };
JSON.stringify({ p }, (k: string, v: any) => { if (k === "p") v.n = "two"; return v; });
console.log(p.n * 2);
"#,
    ),
    (
        "a nested element, the replacer kept and stored later",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
const kept: any[] = [];
JSON.stringify({ a: [{ b: p }] }, (k: string, v: any) => { if (k === "b") kept.push(v); return v; });
kept[0].n = "late";
console.log(p.n * 2);
"#,
    ),
    (
        "the root's holder `{\"\": value}`",
        r#"
const p = { n: 1 };
let wrap: any = null;
JSON.stringify(p, function (this: any, k: string, v: any) { if (k === "") wrap = this; return v; });
wrap[""].n = "w";
console.log(p.n * 2);
"#,
    ),
    (
        "what toJSON returns",
        r#"
const inner = { n: 2 };
const t = { toJSON() { return { inner }; } };
JSON.stringify({ t }, function (this: any, k: string, v: any) { if (k === "inner") v.n = "x"; return v; });
console.log(inner.n * 2);
"#,
    ),
    (
        "what the replacer returns is visited",
        r#"
class P { n: number; constructor() { this.n = 3; } }
const made = new P();
JSON.stringify({ k: 1 }, function (this: any, k: string, v: any) {
  if (k === "k") return made;
  if (k === "n") this.n = "nnn";
  return v;
});
console.log(made.n * 2);
"#,
    ),
    (
        "a bound replacer",
        r#"
const p = { n: 1 };
const ctx = { tag: "c" };
function rep(this: any, k: string, v: any) { if (k === "p") v.n = this.tag; return v; }
JSON.stringify({ p }, rep.bind(ctx));
console.log(p.n * 2);
"#,
    ),
    (
        "the built-in called as a value",
        r#"
const p = { n: 1 };
const ser = JSON.stringify;
ser({ p }, (k: string, v: any) => { if (k === "p") v.n = "alias"; return v; });
console.log(p.n * 2);
"#,
    ),
    (
        "a reviver stores through `this` into a value it returned",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const pp = new P();
const o: any = JSON.parse('{"a":{"x":1},"b":3}', function (this: any, k: string, v: any) {
  if (k === "a") return pp;
  if (k === "b") this.a.n = "changed";
  return v;
});
console.log(pp.n * 2, o.a === pp);
"#,
    ),
];

#[test]
fn a_json_callback_sees_the_values_it_is_called_with() {
    for (what, src) in HOLES {
        let with = n_kinds(src, true);
        let without = n_kinds(src, false);
        assert!(
            without.iter().any(|k| k == "num"),
            "{what}: the control proves `n` a Number ({without:?})"
        );
        assert!(
            !with.iter().any(|k| k == "num"),
            "{what}: `n` holds a string the callback stored ({with:?})"
        );
    }
}

const CONTROLS: &[(&str, &str)] = &[
    (
        "no replacer",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
console.log(JSON.stringify({ p }), JSON.stringify(p, undefined, 2), p.n * 2);
"#,
    ),
    (
        "a literal null / undefined replacer",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
console.log(JSON.stringify({ p }, null), JSON.stringify({ p }, undefined, 1), p.n * 2);
"#,
    ),
    (
        "a literal array replacer",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
console.log(JSON.stringify({ p }, ["p", "n"]), JSON.stringify(p, ["n"], 2), p.n * 2);
"#,
    ),
    (
        "JSON.parse without a reviver",
        r#"
class P { n: number; constructor() { this.n = 1; } }
const p = new P();
const o: any = JSON.parse('{"a":1}');
console.log(o.a, p.n * 2);
"#,
    ),
];

#[test]
fn an_uncalled_replacer_keeps_every_fact() {
    for (what, src) in CONTROLS {
        assert_eq!(
            fact_lines(src, true),
            fact_lines(src, false),
            "{what}: the rule changes no fact line"
        );
        assert!(
            n_kinds(src, true).iter().any(|k| k == "num"),
            "{what}: `n` stays a Number"
        );
    }
}

#[test]
fn a_replacer_that_only_reads_keeps_the_kind() {
    let src = r#"
class P { n: number; constructor() { this.n = 5; } }
const p = new P();
let c = 0;
console.log(JSON.stringify({ p }, (k: string, v: any) => { c++; return v; }), p.n * 2, c);
"#;
    assert!(
        n_kinds(src, true).iter().any(|k| k == "num"),
        "`n` stays a Number"
    );
}
