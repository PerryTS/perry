//! VR-0 (DESIGN-p4l4 §2.2): every identity route of the program is a
//! `Cons::Identity` on the compared values, only with value records on.
use crate::model::{Cons, IdentityRoute, Model, TokKind};

fn model(src: &str, on: bool) -> Model {
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let mut g = crate::gen::Gen::new(vec![("t.ts", &hir)], false, false);
    g.m.max_work = 50_000_000;
    g.m.value_records = on;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g.m
}

/// The routes under which a node holding an object literal token is
/// compared (the literal's tokens: `Obj` of a template born in `t.ts`).
fn routes(m: &Model) -> Vec<IdentityRoute> {
    let mut out = Vec::new();
    for n in &m.nodes {
        let holds_obj = n
            .pts
            .to_sorted()
            .into_iter()
            .any(|t| matches!(m.toks[t as usize], TokKind::Obj(..)));
        if !holds_obj {
            continue;
        }
        for c in &n.cons {
            if let Cons::Identity { route } = c {
                if !out.contains(route) {
                    out.push(*route);
                }
            }
        }
    }
    out
}

fn any_identity(m: &Model) -> bool {
    m.nodes
        .iter()
        .any(|n| n.cons.iter().any(|c| matches!(c, Cons::Identity { .. })))
}

#[test]
fn strict_equality_between_objects_is_identity() {
    let src = "const a = { x: 1 };\nconst b = { x: 1 };\nconsole.log(a === b, a !== b);\n";
    assert_eq!(routes(&model(src, true)), vec![IdentityRoute::StrictEq]);
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn a_comparison_with_a_primitive_literal_is_a_presence_test() {
    let src = "const a: { x: number } | undefined = Math.random() > 2 ? undefined : { x: 1 };\nconsole.log(a === undefined, a !== null, a == null, a === 0, a === \"k\");\n";
    let m = model(src, true);
    assert!(!any_identity(&m), "a presence test is no identity");
}

#[test]
fn loose_equality_between_objects_is_identity() {
    let src = "const a = { x: 1 };\nconst b = { x: 2 };\nconsole.log(a == b, a != b);\n";
    assert_eq!(routes(&model(src, true)), vec![IdentityRoute::LooseEq]);
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn switch_cases_are_identity() {
    let src = "const a = { x: 1 };\nconst b = { x: 2 };\nswitch (a) { case b: console.log(1); break; default: console.log(2); }\n";
    assert_eq!(routes(&model(src, true)), vec![IdentityRoute::Switch]);
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn object_is_is_identity() {
    let src = "const a = { x: 1 };\nconst b = { x: 2 };\nconsole.log(Object.is(a, b));\n";
    assert_eq!(routes(&model(src, true)), vec![IdentityRoute::ObjectIs]);
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn is_prototype_of_compares_the_receiver() {
    let src = "const p = { x: 1 };\nconst o = { y: 2 };\nconsole.log(p.isPrototypeOf(o));\n";
    assert_eq!(
        routes(&model(src, true)),
        vec![IdentityRoute::IsPrototypeOf]
    );
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn collection_keys_are_identity() {
    for src in [
        "const k = { x: 1 };\nconst m = new Map<object, number>();\nm.set(k, 1);\nconsole.log(m.size);\n",
        "const k = { x: 1 };\nconst m = new Map<object, number>();\nconsole.log(m.has(k));\n",
        "const k = { x: 1 };\nconst s = new Set<object>();\nconsole.log(s.delete(k));\n",
    ] {
        assert!(
            routes(&model(src, true)).contains(&IdentityRoute::Collection),
            "{src}"
        );
        assert!(!any_identity(&model(src, false)), "{src}");
    }
}

#[test]
fn structured_clone_keeps_sharing() {
    let src = "const a = { x: 1 };\nconst c = structuredClone([a, a]);\nconsole.log(c.length);\n";
    assert!(routes(&model(src, true)).contains(&IdentityRoute::Clone));
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn a_declared_native_identity_slot_is_identity() {
    // Array.prototype.includes declares identity(arg(0)), identity(elems(this)).
    let src = "const a = { x: 1 };\nconst xs = [a];\nconsole.log(xs.includes(a), xs.indexOf(a));\n";
    assert!(routes(&model(src, true)).contains(&IdentityRoute::Native));
    assert!(!any_identity(&model(src, false)));
}

#[test]
fn reads_and_logging_observe_no_identity() {
    // Negative control: field reads, arithmetic and console.log compare nothing.
    let src = "const a = { x: 1, y: 2 };\nconst s = a.x + a.y;\nconsole.log(a, s, `${a.x}`);\n";
    assert!(!any_identity(&model(src, true)));
}

#[test]
fn the_identity_constraint_does_not_change_the_solve() {
    // With and without the switch the nodes hold the same values.
    let src = "const a = { x: 1 };\nconst b = { x: 2 };\nconst m = new Map<object, number>();\nm.set(a, 1);\nfunction f(o: object) { return o === b || Object.is(o, a); }\nswitch (a) { case b: break; }\nconsole.log(f(a), [a].includes(b), m.get(b));\n";
    let on = model(src, true);
    let off = model(src, false);
    assert!(on.nodes.len() >= off.nodes.len());
    let label = |m: &Model, i: usize| m.labels[i];
    for i in 0..off.nodes.len() {
        if label(&on, i) != label(&off, i) {
            continue;
        }
        let a = on.nodes[i].pts.to_sorted();
        let b = off.nodes[i].pts.to_sorted();
        assert_eq!(a, b, "node {i}");
        assert_eq!(on.nodes[i].prim, off.nodes[i].prim, "node {i}");
    }
}
