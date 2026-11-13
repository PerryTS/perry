//! A fold HIR chose by the receiver's NAME is also an ordinary call.
//! `pre_scan_weakref_locals` records bare local names: a module holding
//! `const r = new WeakRef(x)` lowers every `r.deref()` to
//! `Expr::WeakRefDeref`, a FinalizationRegistry name every `.register` /
//! `.unregister` to its node, whatever the binding called `r` holds at that
//! site. The runtime helpers test the receiver's kind and hand any other
//! receiver to ordinary dispatch (`dispatch_foreign_weak_receiver`), so the
//! receiver's own method runs. Without that call the solve called its body
//! inactive while Node ran it (check mode: "inactive body: body entered" on
//! map/test_gap_weakref_receiver_shapes_7947).
//! Each program's marker function `hit` runs only through the method under
//! test; the control is the same solve without the rule (`noreachfolds`,
//! what `PERRY_FACTS_REACH_FOLDS=0` gives).

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
        sabotage: (!rule).then(|| "noreachfolds".to_string()),
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

/// The fold's node is in the lowered module: the fixtures test the rule,
/// not an ordinary call.
fn folded(src: &str, node: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    format!("{hir:?}").contains(node)
}

const FIXTURES: &[(&str, &str, &str)] = &[
    (
        "deref of an object literal",
        "WeakRefDeref",
        r#"
function hit() { return 40; }
const wr = new WeakRef({});
console.log(typeof wr.deref());
function collide(): unknown { const wr = { deref: () => hit() }; return wr.deref(); }
console.log(collide());
"#,
    ),
    (
        "deref of a class instance",
        "WeakRefDeref",
        r#"
function hit() { return 40; }
class Cache { deref() { return hit(); } }
const wr = new WeakRef({});
console.log(typeof wr.deref());
function collide(): unknown { const wr = new Cache(); return wr.deref(); }
console.log(collide());
"#,
    ),
    (
        "deref of a parameter",
        "WeakRefDeref",
        r#"
function hit() { return 40; }
const wr = new WeakRef({});
console.log(typeof wr.deref());
function collide(wr: { deref: () => number }): unknown { return wr.deref(); }
console.log(collide({ deref: () => hit() }));
"#,
    ),
    (
        "register of an object literal",
        "FinalizationRegistryRegister",
        r#"
function hit() { return 40; }
const target = {};
const reg = new FinalizationRegistry<number>(() => {});
reg.register(target, 1);
function collide(): unknown { const reg = { register: (a: object, b: number) => hit() }; return reg.register(target, 3); }
console.log(collide());
"#,
    ),
    (
        "unregister of an object literal",
        "FinalizationRegistryUnregister",
        r#"
function hit() { return 40; }
const target = {};
const reg = new FinalizationRegistry<number>(() => {});
reg.register(target, 1, target);
function collide(): unknown { const reg = { unregister: (a: object) => hit() }; return reg.unregister(target); }
console.log(collide());
"#,
    ),
];

#[test]
fn a_foreign_receiver_of_a_name_keyed_fold_runs_its_own_method() {
    for (name, node, src) in FIXTURES {
        assert!(folded(src, node), "{name}: the site is the fold {node}");
        assert!(hit_runs(src, true), "{name}: the receiver's method runs");
        assert!(
            !hit_runs(src, false),
            "{name}: without the rule the body was called inactive (the control)"
        );
    }
}

/// The method's result is the expression's: a caller that calls what the
/// receiver's `deref` returned runs it.
#[test]
fn the_folds_result_holds_what_the_method_returns() {
    let src = r#"
function hit() { return 40; }
const wr = new WeakRef({});
console.log(typeof wr.deref());
function collide(): any { const wr = { deref: () => hit }; return wr.deref(); }
console.log(collide()());
"#;
    assert!(folded(src, "WeakRefDeref"));
    assert!(hit_runs(src, true));
    assert!(!hit_runs(src, false));
}

/// A genuine WeakRef is served by the helper without a lookup: its `deref`
/// reads the target and calls nothing, so the fold's result is the target
/// alone, as without the rule (same facts dump).
#[test]
fn a_genuine_weak_ref_keeps_its_facts() {
    let src = r#"
class T { tag = "T"; }
const wr = new WeakRef(new T());
const reg = new FinalizationRegistry<number>(() => {});
const o = wr.deref();
reg.register(o as object, 1, o as object);
console.log((o as any).tag, reg.unregister(o as object));
"#;
    assert!(folded(src, "WeakRefDeref"));
    assert!(folded(src, "FinalizationRegistryRegister"));
    assert!(folded(src, "FinalizationRegistryUnregister"));
    let with = facts(src);
    let without = facts_with(
        src,
        FactsConfig {
            sabotage: Some("noreachfolds".into()),
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
