//! `Reflect.construct(T, args, N)`: the object made has `N.prototype` as
//! its [[Prototype]]. Where the model holds that object as ⊤ (a built-in
//! `T`, a function `N`), the members of `N.prototype` are on the chain of a
//! ⊤ value; the solve dropped the new target there, so a method the
//! program then called on the result was called inactive while Node ran it
//! (check mode: "inactive body: body entered" on
//! map/test_gap_weakz2_birth_site).
//! Each program's marker function `hit` runs only through a member of the
//! new target's `prototype`; the control is the same solve without the rule
//! (`noreachnewtarget`, what `PERRY_FACTS_REACH_NEW_TARGET=0` gives).

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
        sabotage: (!rule).then(|| "noreachnewtarget".to_string()),
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

const HEAD: &str = "function hit(...a: any[]): any { return 1; }\n";

const HOLES: &[(&str, &str)] = &[
    (
        "a built-in target, a function new target with a replaced prototype",
        "function NT() {} NT.prototype = { has: () => hit() }; const o: any = Reflect.construct(WeakMap, [], NT); console.log(o.has({}));",
    ),
    (
        "a built-in target, a member stored on the function's own prototype",
        "function NT() {} (NT as any).prototype.extra = function () { return hit(); }; const o: any = Reflect.construct(Map, [], NT); console.log(o.extra());",
    ),
    (
        "a built-in target, a class new target",
        "class NT { extra() { return hit(); } } const o: any = Reflect.construct(Set, [], NT); console.log(o.extra());",
    ),
    (
        "Array as the target",
        "function NT() {} NT.prototype = { extra: () => hit() }; const o: any = Reflect.construct(Array, [], NT); console.log(o.extra());",
    ),
    (
        "a program function target, a function new target",
        "function T(this: any) { this.x = 1; } function NT() {} NT.prototype = { extra: () => hit() }; const o: any = Reflect.construct(T, [], NT); console.log(o.extra());",
    ),
    (
        "a program class target, a function new target",
        "class T { x = 1; } function NT() {} NT.prototype = { extra: () => hit() }; const o: any = Reflect.construct(T, [], NT); console.log(o.extra());",
    ),
    (
        "a getter on the new target's prototype",
        "function NT() {} NT.prototype = { get extra() { return hit(); } }; const o: any = Reflect.construct(WeakSet, [], NT); console.log(o.extra);",
    ),
];

/// A class new target of a program target names its layout: reached before.
const COVERED: &[(&str, &str)] = &[
    (
        "a program class target, a class new target",
        "class T { x = 1; } class NT { extra() { return hit(); } } const o: any = Reflect.construct(T, [], NT); console.log(o.extra());",
    ),
    (
        "the target as its own new target",
        "class T { extra() { return hit(); } } const o: any = Reflect.construct(T, []); console.log(o.extra());",
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
fn the_new_targets_prototype_members_run() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, on, _)| !on).collect();
    assert!(
        bad.is_empty(),
        "not reached with the rule: {bad:?}\nall: {t:?}"
    );
}

#[test]
fn without_the_rule_the_new_target_was_dropped() {
    let t = table(HOLES);
    let bad: Vec<_> = t.iter().filter(|(_, _, off)| *off).collect();
    assert!(
        bad.is_empty(),
        "reached without the rule (no control): {bad:?}\nall: {t:?}"
    );
}

#[test]
fn a_class_new_target_of_a_program_target_was_reached_before() {
    let t = table(COVERED);
    let bad: Vec<_> = t.iter().filter(|(_, on, off)| !on || !off).collect();
    assert!(bad.is_empty(), "lost: {bad:?}\nall: {t:?}");
}

/// `Reflect.construct` whose allocation the model names (a class new
/// target of a program target) keeps its facts: nothing goes to ⊤.
#[test]
fn a_named_allocation_keeps_its_facts() {
    let src = r#"
class Two { r = 1; constructor() { console.log(this.r); } }
class Other { pad = 99; r = 7; m() { return this.pad; } }
const a: any = Reflect.construct(Two, [], Other);
const b: any = Reflect.construct(Two, []);
console.log(a.r, a.m(), b.r);
"#;
    let dump = |sab: Option<&str>| {
        facts_with(
            src,
            FactsConfig {
                sabotage: sab.map(str::to_string),
                ..Default::default()
            },
        )
        .dump_text()
        .lines()
        .filter(|l| fact_line(l))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join("\n")
    };
    assert_eq!(dump(None), dump(Some("noreachnewtarget")));
}
