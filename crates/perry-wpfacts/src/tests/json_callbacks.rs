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

/// Unknown without the rule already: what the replacer returns escapes
/// with its result, and so does what a call of the built-in as a value
/// (an alias) hands it. The rule keeps them unknown.
const ALREADY_UNKNOWN: &[(&str, &str)] = &[
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
        "the built-in called as a value",
        r#"
const p = { n: 1 };
const ser = JSON.stringify;
ser({ p }, (k: string, v: any) => { if (k === "p") v.n = "alias"; return v; });
console.log(p.n * 2);
"#,
    ),
];

#[test]
fn a_value_the_replacer_returns_or_an_alias_gets_stays_unknown() {
    for (what, src) in ALREADY_UNKNOWN {
        let with = n_kinds(src, true);
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

/// The receivers the facts dump names for the reads of `.name`.
fn name_receivers(src: &str) -> Vec<String> {
    dump(src, true)
        .lines()
        .filter(|l| l.starts_with("S ") && l.contains(" .name "))
        .filter_map(|l| l.split(" recv=").nth(1))
        .map(|r| r.split(' ').next().unwrap_or("").to_string())
        .collect()
}

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// InternalizeJSONProperty fixes the key list, then makes an ordinary
/// [[Get]] of each key with the reviver running in between (CWN's
/// counterexample): a reviver that deletes a later key makes that [[Get]]
/// read the chain, so the reviver receives `Object.prototype.toString`.
/// Without a delete, defineProperty or escape the reviver gets the parse
/// objects' own values only and nothing of the chain.
#[test]
fn a_reviver_that_deletes_a_later_key_gets_what_the_chain_holds() {
    let deleted = r#"
let seen = "";
JSON.parse('{"a":1,"toString":2}', function (this: any, k: string, v: any) {
  if (k === "a") delete this.toString;
  if (k === "toString") seen = v.name;
  return v;
});
console.log(seen);
"#;
    let recv = name_receivers(deleted);
    assert!(
        recv.iter()
            .any(|r| r.contains("ObjectProtoMethod") || r.contains("builtin-fn")),
        "the deleted key's [[Get]] reads the chain: {recv:?}"
    );
    let kept = r#"
let seen = "";
JSON.parse('{"a":1,"toString":2}', function (this: any, k: string, v: any) {
  if (k === "toString") seen = v.name;
  return v;
});
console.log(seen);
"#;
    let recv = name_receivers(kept);
    assert!(
        !recv
            .iter()
            .any(|r| r.contains("ObjectProtoMethod") || r.contains("builtin-fn")),
        "no delete: own values only: {recv:?}"
    );
}

/// A reviver that defines an accessor on a later key: the walk's [[Get]]
/// runs the getter.
#[test]
fn a_reviver_that_defines_a_later_accessor_runs_its_getter() {
    let src = r#"
function hit() { return "got"; }
let seen: any = null;
JSON.parse('{"a":1,"b":2}', function (this: any, k: string, v: any) {
  if (k === "a") Object.defineProperty(this, "b", { get() { return hit(); }, configurable: true, enumerable: true });
  if (k === "b") seen = v;
  return v;
});
console.log(seen);
"#;
    assert!(hit_runs(src), "the getter's body is active");
}
