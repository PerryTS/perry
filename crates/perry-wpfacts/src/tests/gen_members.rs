//! A generator object has the members it inherits. It is its step closure
//! in the model, and the function-protocol rule gave it `next`, `return`
//! and `throw` only: `gen().map(f)` ran nothing and `g[Symbol.iterator]`
//! read no value (a live receiver with an empty points-to set), so `f`, or
//! the step body behind `g[Symbol.iterator].call(g).next()`, was called
//! inactive while Node ran it (check mode: "inactive body: body entered" on
//! generator/test_gap_6696_generator_symbol_iterator and
//! iterator/test_gap_iterator_helpers_2874).
//! Each program's marker function `hit` runs only through the member under
//! test; the control is the same solve without the rules
//! (`noreachgenmembers`, what `PERRY_FACTS_REACH_GEN_MEMBERS=0` gives).

use super::*;

/// The lines of a facts dump that state facts: templates, their fields,
/// the facts and the site verdicts (no node, call or work counts).
fn fact_line(l: &str) -> bool {
    ["T ", "  F ", "FACT ", "S "]
        .iter()
        .any(|p| l.starts_with(p))
}

fn solved(src: &str, rule: bool) -> (ProgramFacts, u32) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let cfg = FactsConfig {
        sabotage: (!rule).then(|| "noreachgenmembers".to_string()),
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
    (pf, id)
}

fn hit_runs(src: &str, rule: bool) -> bool {
    let (pf, id) = solved(src, rule);
    assert!(pf.body_reach.complete, "activated");
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const HEAD: &str =
    "function hit(...a: any[]): any { return 1; }\nfunction* gen(): any { yield 1; yield 2; }\n";

/// A helper's callback, the values a helper yields, the generator behind
/// its own `[Symbol.iterator]`: inactive without the rules.
const HOLES: &[(&str, &str)] = &[
    ("map", "console.log([...gen().map((x: any) => hit(x))].length);"),
    ("filter", "console.log([...gen().filter((x: any) => hit(x) > 0)].length);"),
    ("flatMap", "console.log([...gen().flatMap((x: any) => [hit(x)])].length);"),
    ("forEach", "gen().forEach((x: any) => { hit(x); });"),
    ("some", "console.log(gen().some((x: any) => hit(x) > 5));"),
    ("every", "console.log(gen().every((x: any) => hit(x) > 5));"),
    ("find", "console.log(gen().find((x: any) => hit(x) > 5));"),
    ("reduce", "console.log(gen().reduce((a: any, x: any) => a + hit(x), 0));"),
    ("take then forEach", "gen().take(1).forEach((x: any) => { hit(x); });"),
    ("drop then map", "console.log(gen().drop(1).map((x: any) => hit(x)).toArray().length);"),
    (
        "a helper's callback receives what the generator yields",
        "function* fns(): any { yield hit; } fns().forEach((f: any) => { f(); });",
    ),
    (
        "toArray holds what the generator yields",
        "function* fns(): any { yield hit; } fns().toArray()[0]();",
    ),
    (
        "map yields what its callback returns",
        "for (const f of gen().map(() => hit)) f();",
    ),
    (
        "the generator is its own iterator",
        "function* body(): any { hit(); yield 1; } const g: any = body(); console.log(g[Symbol.iterator]().next().value);",
    ),
    (
        "the self-iterator read as a value and called",
        "function* body(): any { hit(); yield 1; } function drive(g: any) { const f = g[Symbol.iterator]; const it = f.call(g); return it.next().value; } console.log(drive(body()));",
    ),
    (
        "next read as a value and called",
        "function* body(): any { hit(); yield 1; } const g: any = body(); const n = g.next; console.log(n.call(g).value);",
    ),
    (
        "an async generator is its own async iterator",
        "async function* body(): any { hit(); yield 1; } const g: any = body(); g[Symbol.asyncIterator]().next();",
    ),
];

/// What already reached the generator: the protocol itself.
const COVERED: &[(&str, &str)] = &[
    ("for-of", "function* body(): any { hit(); yield 1; } for (const x of body()) console.log(x);"),
    ("next", "function* body(): any { hit(); yield 1; } console.log(body().next().value);"),
    ("spread", "function* body(): any { hit(); yield 1; } console.log([...body()].length);"),
    (
        "a member the program stored on the generator function's prototype",
        "(gen as any).prototype.extra = function () { return hit(); }; console.log((gen() as any).extra());",
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
fn a_generator_objects_inherited_members_run() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, on, _)| !on).collect();
    assert!(
        bad.is_empty(),
        "not reached with the rules: {bad:?}\nall: {t:?}"
    );
}

#[test]
fn without_the_rules_they_reached_nothing() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, _, off)| *off).collect();
    assert!(
        bad.is_empty(),
        "reached without the rules (no control): {bad:?}\nall: {t:?}"
    );
}

#[test]
fn the_protocol_itself_reached_the_generator_before() {
    let t = table(COVERED);
    let bad: Vec<_> = t.iter().filter(|(_, on, off)| !on || !off).collect();
    assert!(bad.is_empty(), "lost: {bad:?}\nall: {t:?}");
}

/// A program that only steps its generators (for-of, `next`, spread,
/// `yield*`) names none of the inherited members: same facts.
#[test]
fn a_program_that_only_steps_keeps_its_facts() {
    let src = r#"
class P { x = 1; }
function* inner() { yield new P(); }
function* outer() { yield* inner(); yield new P(); }
let s = 0;
for (const p of outer()) s += p.x;
const g = outer();
const r = g.next();
if (!r.done) s += r.value.x;
console.log(s, [...inner()].length);
"#;
    let dump = |rule: bool| {
        solved(&format!("function hit() {{}}\n{src}"), rule)
            .0
            .dump_text()
            .lines()
            .filter(|l| fact_line(l))
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(dump(true), dump(false));
}
