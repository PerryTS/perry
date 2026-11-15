//! IteratorComplete: a consumer of the iteration protocol reads `done` of
//! each step result before its value, so a getter under `done` runs.
//! The solve read the value only (`Model::yield_key`), and called the
//! getter's body inactive while Node ran it (check mode: "inactive body:
//! body entered" on array/test_gap_array_binding_iterator_close,
//! iterator/test_gap_iterator_close_completion_order,
//! iterator/test_gap_iterret_close_destruct and
//! operators/test_gap_region_numeric_expression_order).
//! Each program's marker function `hit` runs only through the getter; the
//! control is the same solve without the rule (`noreachstepdone`, what
//! `PERRY_FACTS_REACH_STEP_DONE=0` gives).

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
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let cfg = FactsConfig {
        sabotage: (!rule).then(|| "noreachstepdone".to_string()),
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

/// An iterator whose step result carries `done` as a getter.
const IT: &str = r#"
function hit() { return true; }
const it: any = {
  [Symbol.iterator]() { return this; },
  next() { return { get done() { return hit(); }, value: 1 }; },
};
"#;

/// The same with entry pairs as values (Map, Object.fromEntries).
const PAIRS: &str = r#"
function hit() { return true; }
const it: any = {
  [Symbol.iterator]() { return this; },
  next() { return { get done() { return hit(); }, value: ["k", 1] }; },
};
"#;

const CONSUMERS: &[(&str, &str, &str)] = &[
    ("array destructuring", IT, "const [a] = it; console.log(a);"),
    (
        "destructuring with a rest element",
        IT,
        "const [a, ...r] = it; console.log(a, r.length);",
    ),
    ("for-of", IT, "for (const x of it) console.log(x);"),
    ("array spread", IT, "console.log([...it].length);"),
    (
        "argument spread",
        IT,
        "function f(...a: any[]) { return a.length; } console.log(f(...it));",
    ),
    ("Array.from", IT, "console.log(Array.from(it).length);"),
    (
        "Array.from with a mapper",
        IT,
        "console.log(Array.from(it, (x: any) => x).length);",
    ),
    ("new Set", IT, "console.log(new Set(it).size);"),
    ("new Map", PAIRS, "console.log(new Map(it).size);"),
    (
        "Object.fromEntries",
        PAIRS,
        "console.log(Object.keys(Object.fromEntries(it)).length);",
    ),
    (
        "Promise.all",
        IT,
        "Promise.all(it).then((v) => console.log(v.length));",
    ),
    ("Math.max spread", IT, "console.log(Math.max(...it));"),
];

#[test]
fn every_consumer_reads_done_of_a_step_result() {
    for (name, head, tail) in CONSUMERS {
        let src = format!("{head}{tail}\n");
        assert!(hit_runs(&src, true), "{name}: the `done` getter runs");
    }
}

/// `yield*` is lowered to a loop that reads `done` of each pulled result
/// in program code: it reached the getter before, and still does.
#[test]
fn yield_star_read_done_before() {
    let src =
        format!("{IT}function* g(): any {{ yield* it; }} for (const x of g()) console.log(x);\n");
    assert!(hit_runs(&src, true));
    assert!(hit_runs(&src, false));
}

/// Before the rule the getter was reached by no consumer (the controls).
#[test]
fn without_the_rule_the_getter_was_called_inactive() {
    for (name, head, tail) in CONSUMERS {
        let src = format!("{head}{tail}\n");
        assert!(!hit_runs(&src, false), "{name}: the control");
    }
}

/// The iterator a `[Symbol.iterator]()` call returns, distinct from the
/// iterable, is stepped too: its results' `done` is read.
#[test]
fn the_returned_iterators_results_are_read_too() {
    let src = r#"
function hit() { return true; }
const it: any = {
  [Symbol.iterator]() {
    return { next() { return { get done() { return hit(); }, value: 1 }; } };
  },
};
for (const x of it) console.log(x);
"#;
    assert!(hit_runs(src, true));
    assert!(!hit_runs(src, false));
}

/// A `done` data property is read and runs nothing: a program that
/// iterates arrays, a generator and a plain iterator keeps every fact line
/// it had without the rule. What the rule adds is the row of the key it
/// reads, `done`, on an object that is stepped and does not own it (the
/// iterator itself, where a step result node also holds it).
#[test]
fn plain_step_results_keep_their_facts() {
    let src = r#"
function* g() { yield 1; yield 2; }
const it: any = { i: 0, [Symbol.iterator]() { return this; }, next() { return { done: this.i++ > 2, value: this.i }; } };
let s = 0;
for (const x of [1, 2, 3]) s += x;
for (const x of g()) s += x;
for (const x of it) s += x;
const [a, b] = [4, 5];
console.log(s, a, b, [...g()].length, Array.from(it).length);
"#;
    let run = |sab: Option<&str>| -> Vec<String> {
        let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
        let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
        perry_transform::transform_generators(&mut hir);
        let cfg = FactsConfig {
            sabotage: sab.map(str::to_string),
            max_work: Some(50_000_000),
            ..FactsConfig::default()
        };
        crate::compute(&[("t.ts".to_string(), &hir)], &cfg)
            .dump_text()
            .lines()
            .filter(|l| fact_line(l))
            .map(str::to_string)
            .collect()
    };
    let (with, without) = (run(None), run(Some("noreachstepdone")));
    let lost: Vec<_> = without.iter().filter(|l| !with.contains(l)).collect();
    assert!(lost.is_empty(), "fact lines lost: {lost:#?}");
    let added: Vec<_> = with
        .iter()
        .filter(|l| !without.contains(l))
        .filter(|l| !(l.starts_with("  F done ") || l.contains(".done = ")))
        .collect();
    assert!(
        added.is_empty(),
        "fact lines added beside `done`: {added:#?}"
    );
}
