//! IteratorClose by a built-in consumer. A consumer that stops before its
//! iterator is done calls the iterator's `return`: `Array.from(it, f)` when
//! `f` throws, a collection constructor or `Object.fromEntries` on an entry
//! that is no object, a `groupBy` callback that throws, `Promise.all`,
//! an iterator helper over `Iterator.from(it)`. The solve made that call
//! for a for-of and for destructuring only, and called the `return` body
//! inactive while Node ran it (check mode: "inactive body: body entered" on
//! iterator/test_gap_iterret_close_consumers and
//! iterator/test_gap_iterret_close_control).
//! Each program's marker function `hit` runs only through `return`; the
//! control is the same solve without the rule (`noreachiterclose`, what
//! `PERRY_FACTS_REACH_ITER_CLOSE=0` gives).

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
        sabotage: (!rule).then(|| "noreachiterclose".to_string()),
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

const HEAD: &str = r#"
function hit(): any { return {}; }
const it: any = {
  [Symbol.iterator]() { return this; },
  next() { return { value: [1, 2], done: false }; },
  return() { return hit(); },
};
"#;

/// The iterator is what `[Symbol.iterator]()` returns, not the iterable.
const HEAD_SEPARATE: &str = r#"
function hit(): any { return {}; }
const it: any = {
  [Symbol.iterator]() {
    return { next() { return { value: [1, 2], done: false }; }, return() { return hit(); } };
  },
};
"#;

/// Consumers that can stop early: `return` was inactive without the rule.
const CLOSING: &[(&str, &str)] = &[
    (
        "Array.from with a mapper",
        "try { Array.from(it, () => { throw 1; }); } catch (e) {}",
    ),
    (
        "Array.from as a value call",
        "function Locked(this: any) { Object.preventExtensions(this); } try { Array.from.call(Locked, it); } catch (e) {}",
    ),
    ("Array.from", "try { Array.from(it); } catch (e) {}"),
    ("new Map", "try { new Map(it); } catch (e) {}"),
    ("new Set", "try { new Set(it); } catch (e) {}"),
    ("new WeakSet", "try { new WeakSet(it); } catch (e) {}"),
    (
        "Object.fromEntries",
        "try { Object.fromEntries(it); } catch (e) {}",
    ),
    (
        "Object.groupBy",
        "try { Object.groupBy(it, () => { throw 1; }); } catch (e) {}",
    ),
    (
        "Map.groupBy",
        "try { Map.groupBy(it, () => { throw 1; }); } catch (e) {}",
    ),
    ("Promise.all", "Promise.all(it).catch(() => {});"),
    (
        "Iterator.from then a helper",
        "console.log(Iterator.from(it).take(1).toArray().length);",
    ),
];

/// Consumers the solve already closed: a for-of that leaves early,
/// destructuring.
const CLOSED_BEFORE: &[(&str, &str)] = &[
    ("for-of break", "for (const x of it) break;"),
    ("destructuring", "const [a] = it; console.log(a);"),
];

/// Consumers that never close: they run the iterator to its end, and an
/// error from `next` is no close (ECMA-262 13.2.4.1, 13.3.8.1). `return`
/// stays inactive.
const EXHAUSTING: &[(&str, &str)] = &[
    ("array spread", "console.log([...it].length);"),
    (
        "argument spread",
        "function f(...a: any[]) { return a.length; } console.log(f(...it));",
    ),
];

fn table(head: &str, list: &[(&str, &str)]) -> Vec<(String, bool, bool)> {
    list.iter()
        .map(|(name, tail)| {
            let src = format!("{head}{tail}\n");
            (
                name.to_string(),
                hit_runs(&src, true),
                hit_runs(&src, false),
            )
        })
        .collect()
}

#[test]
fn a_consumer_that_can_stop_early_calls_return() {
    for head in [HEAD, HEAD_SEPARATE] {
        let t = table(head, CLOSING);
        let bad: Vec<_> = t.iter().filter(|(_, on, _)| !on).collect();
        assert!(bad.is_empty(), "return not reached: {bad:?}\nall: {t:?}");
    }
}

#[test]
fn without_the_rule_return_was_called_inactive() {
    let t = table(HEAD, CLOSING);
    let bad: Vec<_> = t.iter().filter(|(_, _, off)| *off).collect();
    assert!(
        bad.is_empty(),
        "reached without the rule (no control): {bad:?}\nall: {t:?}"
    );
}

#[test]
fn for_of_and_destructuring_closed_before() {
    let t = table(HEAD, CLOSED_BEFORE);
    let bad: Vec<_> = t.iter().filter(|(_, on, off)| !on || !off).collect();
    assert!(bad.is_empty(), "lost: {bad:?}\nall: {t:?}");
}

#[test]
fn a_consumer_that_runs_to_the_end_calls_no_return() {
    let t = table(HEAD, EXHAUSTING);
    let bad: Vec<_> = t.iter().filter(|(_, on, off)| *on || *off).collect();
    assert!(bad.is_empty(), "return reached: {bad:?}\nall: {t:?}");
}

/// A generator's `finally` runs when a consumer closes it: its body is the
/// step closure `next` already runs, so nothing new is active, and the
/// facts of a program without a `return` method are the same.
#[test]
fn a_program_without_return_keeps_its_facts() {
    let src = r#"
function* g() { yield [1, 2]; yield [3, 4]; }
const m = new Map(g());
const s = new Set([1, 2, 3]);
const a = Array.from(g(), (p) => p[0]);
console.log(m.size, s.size, a.length, Object.fromEntries(g()));
"#;
    let run = |sab: Option<&str>| {
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
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(run(None), run(Some("noreachiterclose")));
}
