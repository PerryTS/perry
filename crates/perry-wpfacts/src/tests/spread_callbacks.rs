//! A spread argument reaches the positions a hand-written built-in rule
//! reads. `xs.forEach(...[cb])` is `args []`, `spread [[cb]]`; the rules
//! read `c.args.first()` and found no callback, so the callback's body was
//! called inactive while Node ran it (check mode: "inactive body: body
//! entered" on spread/test_gap_6718_spread_call_native_method). The rules
//! now read their positions through `Model::positional`: position `i` is a
//! direct argument at index `i` or before, or a spread argument's element.
//! Each program's marker function `hit` runs only through the value the
//! spread supplies; the control is the same solve without the rule
//! (`noreachspreadargs`, what `PERRY_FACTS_REACH_SPREAD_ARGS=0` gives).

use super::*;

/// The lines of a facts dump that state facts: templates, their fields,
/// the facts and the site verdicts (no node, call or work counts).
fn fact_line(l: &str) -> bool {
    ["T ", "  F ", "FACT ", "S "]
        .iter()
        .any(|p| l.starts_with(p))
}

fn hit_runs(src: &str, rule: bool) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        sabotage: (!rule).then(|| "noreachspreadargs".to_string()),
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

const HEAD: &str = "function hit(...a: any[]): any { return 1; }\nconst xs: any[] = [1, 2, 3];\n";

/// The rules that read a position and ignored the spread: `hit` was
/// inactive without the rule.
const HOLES: &[(&str, &str)] = &[
    ("forEach", "xs.forEach(...[(x: any) => { hit(x); }]);"),
    (
        "findIndex",
        "console.log(xs.findIndex(...[(x: any) => hit(x) > 5]));",
    ),
    (
        "findLast",
        "console.log(xs.findLast(...[(x: any) => hit(x) > 5]));",
    ),
    (
        "findLastIndex",
        "console.log(xs.findLastIndex(...[(x: any) => hit(x) > 5]));",
    ),
    (
        "flatMap",
        "console.log(xs.flatMap(...[(x: any) => [hit(x)]]).length);",
    ),
    (
        "reduce",
        "console.log(xs.reduce(...[(a: any, x: any) => a + hit(x), 0]));",
    ),
    (
        "reduceRight",
        "console.log(xs.reduceRight(...[(a: any, x: any) => a + hit(x), 0]));",
    ),
    (
        "toSorted",
        "console.log(xs.toSorted(...[(a: any, b: any) => hit(a) - b]).length);",
    ),
    (
        "forEach thisArg",
        "xs.forEach(...[function (this: any) { this.h(); }, { h: hit }]);",
    ),
    (
        "reduce without an initial value returns an element",
        "const fs: any[] = [hit]; (fs.reduce(...[(a: any) => a]))();",
    ),
    (
        "splice inserts a spread element",
        "const ys: any[] = []; ys.splice(...[0, 0, hit]); ys[0]();",
    ),
    (
        "iterator forEach",
        "xs.values().forEach(...[(x: any) => { hit(x); }]);",
    ),
    (
        "iterator map",
        "console.log([...xs.values().map(...[(x: any) => hit(x)])].length);",
    ),
    (
        "iterator reduce",
        "console.log(xs.values().reduce(...[(a: any, x: any) => a + hit(x), 0]));",
    ),
    (
        "Map forEach",
        "new Map([[1, 2]]).forEach(...[(v: any) => { hit(v); }]);",
    ),
    (
        "Set forEach thisArg",
        "new Set([1]).forEach(...[function (this: any) { this.h(); }, { h: hit }]);",
    ),
    (
        "then",
        "Promise.resolve(1).then(...[(v: any) => { hit(v); }]);",
    ),
    (
        "catch",
        "Promise.reject(1).catch(...[(v: any) => { hit(v); }]);",
    ),
    (
        "Array.from mapper",
        "console.log((Array.from as any)(...[xs, (x: any) => hit(x)]).length);",
    ),
    (
        "JSON.parse reviver",
        "console.log((JSON.parse as any)(...['{\"a\":1}', (k: any, v: any) => hit(v)]));",
    ),
    (
        "RegExp replace through a spread string argument",
        "console.log(/a/[Symbol.replace](...['abc', (s: any) => String(hit(s))] as [any, any]));",
    ),
];

/// Calls a spread argument already reached (declared effects, the opaque
/// summary, a rule that falls back on a spread): they stay reached.
const COVERED: &[(&str, &str)] = &[
    // A direct argument beside a spread is read at its index.
    (
        "forEach with a callback after the spread",
        "const none: any[] = []; xs.forEach(...none, (x: any) => { hit(x); });",
    ),
    (
        "then passes the value through when the spread is empty",
        "const none: any[] = []; Promise.resolve(hit).then(...none).then((f: any) => f());",
    ),
    (
        "map",
        "console.log(xs.map(...[(x: any) => hit(x)]).length);",
    ),
    (
        "filter",
        "console.log(xs.filter(...[(x: any) => hit(x) > 5]).length);",
    ),
    ("some", "console.log(xs.some(...[(x: any) => hit(x) > 5]));"),
    (
        "every",
        "console.log(xs.every(...[(x: any) => hit(x) > 5]));",
    ),
    ("find", "console.log(xs.find(...[(x: any) => hit(x) > 5]));"),
    (
        "sort",
        "console.log(xs.sort(...[(a: any, b: any) => hit(a) - b]).length);",
    ),
    (
        "call",
        "(function (this: any, x: any) { hit(x); }).call(...[null, 1]);",
    ),
    (
        "apply",
        "(function (this: any, x: any) { hit(x); }).apply(...[null, [1]]);",
    ),
    (
        "a program function",
        "function f(cb: any) { cb(); } f(...[hit]);",
    ),
    (
        "setTimeout",
        "(setTimeout as any)(...[() => { hit(); }, 0]);",
    ),
    (
        "string replace",
        "console.log('abc'.replace(...['a', (s: any) => String(hit(s))] as [any, any]));",
    ),
    (
        "typed array forEach",
        "new Uint8Array(2).forEach(...[(x: any) => { hit(x); }] as [any]);",
    ),
];

fn table(list: &[(&str, &str)]) -> Vec<(String, bool, bool)> {
    list.iter()
        .map(|(name, tail)| {
            let src = format!("{HEAD}{tail}\n");
            (
                name.to_string(),
                hit_runs(&src, true),
                hit_runs(&src, false),
            )
        })
        .collect()
}

#[test]
fn a_spread_argument_reaches_every_position_a_rule_reads() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, on, _)| !on).collect();
    assert!(
        bad.is_empty(),
        "not reached with the rule: {bad:?}\nall: {t:?}"
    );
}

/// Each of the holes was one: without the rule `hit` was called inactive.
#[test]
fn without_the_rule_the_spread_reached_no_position() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, _, off)| *off).collect();
    assert!(
        bad.is_empty(),
        "reached without the rule (no control): {bad:?}\nall: {t:?}"
    );
}

#[test]
fn the_calls_a_spread_already_reached_stay_reached() {
    let t = table(COVERED);
    let bad: Vec<_> = t.iter().filter(|(_, on, off)| !on || !off).collect();
    assert!(bad.is_empty(), "lost: {bad:?}\nall: {t:?}");
}

/// A call without a spread argument is read as before: same facts.
#[test]
fn a_call_without_spread_keeps_its_facts() {
    let src = r#"
const xs = [1, 2, 3];
let s = 0;
xs.forEach((x) => { s += x; });
const m = new Map<number, number>([[1, 2]]);
m.forEach((v, k) => { s += v + k; });
console.log(s, xs.reduce((a, x) => a + x, 0), xs.findIndex((x) => x > 1), xs.flatMap((x) => [x, x]).length);
Promise.resolve(1).then((v) => console.log(v));
"#;
    let with = facts(src);
    let without = facts_with(
        src,
        FactsConfig {
            sabotage: Some("noreachspreadargs".into()),
            ..Default::default()
        },
    );
    let strip = |pf: &ProgramFacts| {
        pf.dump_text()
            .lines()
            .filter(|l| fact_line(l))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(strip(&with), strip(&without));
}
