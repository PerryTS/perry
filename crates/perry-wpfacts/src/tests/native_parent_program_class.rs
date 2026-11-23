//! PERFNOW23: a program store with an unknown key onto the global object
//! (`g[k] = P`) lets every global name read `P`, so the `extends Error` of
//! a class linked by name to a built-in parent may evaluate to the program
//! class `P`. Before, the native-parent check gave `P` to unknown code: the
//! class escaped with its prototype methods, the instance escaped, and a
//! `new Map()` that may construct `P` then held the realm (the web app's
//! `class DOMException extends Error` with `g[kk] = dom.Event`). A program
//! class is modeled code: `super()` runs it on the live edge, and the
//! instances' chain continues at its prototype object. Only ⊤, a
//! non-class value or `null` escapes, as before.
use super::*;

fn solved(src: &str) -> crate::gen::Gen<'static> {
    let src: &'static str = Box::leak(src.to_string().into_boxed_str());
    let ast = Box::leak(Box::new(
        perry_parser::parse_typescript(src, "t.ts").unwrap(),
    ));
    let hir = Box::leak(Box::new(perry_hir::lower_module(ast, "t", "t.ts").unwrap()));
    let mut g = crate::gen::Gen::new(vec![("t.ts", &*hir)], false, false);
    g.m.max_work = 50_000_000;
    g.register_all();
    g.link_exports();
    g.walk_all();
    g.feed_parent_values();
    g.m.summarize_and_replay();
    g.m.solve();
    g
}

fn tm_of(g: &crate::gen::Gen<'static>, name: &str) -> crate::model::TmplId {
    (g.m.tmpls
        .iter()
        .position(|t| t.display == format!("t.ts:{name}")))
    .unwrap_or_else(|| panic!("class {name}")) as crate::model::TmplId
}

const SRC: &str = "class A extends Error {}\n\
     class P { m() { return 1; } }\n\
     const g: any = globalThis;\n\
     const k = process.argv[2];\n\
     g[k] = P;\n\
     const a = new A();\n\
     const r = new Map();\n\
     r.set(k, 1);\n\
     console.log(a, r.get(k));\n";

/// Red before the fix: `P` (and its prototype) escaped and the realm was
/// held. Now nothing of `P` escapes, the realm stays, and the instances of
/// `A` may inherit from `P.prototype`.
#[test]
fn a_program_class_as_the_native_parent_value_is_modeled() {
    let mut g = solved(SRC);
    assert!(!g.m.realm_held, "realm held");
    let (a, p) = (tm_of(&g, "A"), tm_of(&g, "P"));
    let pc = g.m.tok(crate::model::TokKind::ClassObj(p));
    let pp = g.m.tok(crate::model::TokKind::Proto(p));
    assert!(!g.m.is_escaped(pc), "class P escaped");
    assert!(!g.m.is_escaped(pp), "P.prototype escaped");
    let ap = g.m.tok(crate::model::TokKind::Proto(a));
    let n = g.m.proto_node(ap);
    assert!(
        g.m.nodes[n as usize].pts.to_sorted().contains(&pp),
        "A.prototype's chain lacks P.prototype"
    );
}

/// Control: the parent may also be ⊤ (an unseen module's global): the
/// value escapes as before, with the instance.
#[test]
fn a_top_parent_value_still_escapes() {
    let src = unseen(&SRC.replace("g[k] = P;", "g[k] = P;\ng[k] = (globalThis as any)[k + 1];"));
    let mut g = solved(&src);
    let a = tm_of(&g, "A");
    let inst = g.m.tok(crate::model::TokKind::Obj(a, crate::model::NO_CTX));
    assert!(g.m.is_escaped(inst), "instance of A kept");
}
