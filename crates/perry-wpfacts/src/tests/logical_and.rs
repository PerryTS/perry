//! `l && r` is `l` only when `l` is falsy (ES ToBoolean: every object is
//! truthy). The result holds `r` and the falsy part of `l`: its primitive
//! kinds that have a falsy value, ⊤ as those primitives only, and the
//! parameter or argument markers it carries. No object or function token
//! of the left side reaches the result.

use super::*;

/// The verdicts of the sites of `key` (reads or calls).
fn sites(pf: &ProgramFacts, key: &str, call: bool) -> Vec<Verdict> {
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| s.verdict.clone())
        .collect()
}

fn proven(pf: &ProgramFacts, key: &str, call: bool) -> bool {
    let v = sites(pf, key, call);
    !v.is_empty() && v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(pf: &ProgramFacts, key: &str, call: bool) -> bool {
    let v = sites(pf, key, call);
    !v.is_empty() && v.iter().all(|v| matches!(v, Verdict::No(_)))
}

fn native(pf: &ProgramFacts, key: &str) -> bool {
    let v = sites(pf, key, true);
    !v.is_empty() && v.iter().all(|v| *v == Verdict::NativeMember)
}

/// ⊤ on the left: the callee of `(U && g)(ctx)` is `g` or a falsy
/// primitive (calling one throws before `ctx` is seen), never an unknown
/// function, so `ctx` stays unescaped and the read `ctx.qx` is proven.
#[test]
fn top_and_fn_callee_keeps_arg_unescaped() {
    let pf = facts(
        r#"
        const U: any = (globalThis as any).u;
        function g(c: { qx: number }): number { return 1; }
        const ctx = { qx: 1 };
        (U && g)(ctx);
        console.log(ctx.qx);
    "#,
    );
    assert!(
        proven(&pf, "qx", false),
        "{:?}\n{}",
        sites(&pf, "qx", false),
        pf.dump_text()
    );
}

/// An escaped object or function on the left: the read `(L && R).qy`
/// sees `R` only.
#[test]
fn object_left_does_not_reach_the_read() {
    for (what, left) in [
        (
            "an escaped object",
            "const L: any = { zz: 1 }; (globalThis as any).keep = L;",
        ),
        (
            "a function",
            "function L(): void {} (globalThis as any).keep = L;",
        ),
    ] {
        let pf = facts(&format!(
            "{left}
            const R = {{ qy: 2 }};
            const v = (L as any) && R;
            console.log(v.qy);"
        ));
        assert!(
            proven(&pf, "qy", false),
            "{what}: {:?}\n{}",
            sites(&pf, "qy", false),
            pf.dump_text()
        );
    }
}

/// Soundness twins: the shapes the rule must not narrow.
#[test]
fn or_coalesce_and_top_right_keep_the_escape() {
    for (what, call) in [
        ("||: a truthy ⊤ is the result", "(U || g)(ctx);"),
        ("??: a non-nullish ⊤ is the result", "(U ?? g)(ctx);"),
        ("⊤ on the right", "((g as any) && U)(ctx);"),
    ] {
        let pf = facts(&format!(
            "const U: any = (globalThis as any).u;
            function g(c: {{ qx: number }}): number {{ return 1; }}
            const ctx = {{ qx: 1 }};
            {call}
            console.log(ctx.qx);"
        ));
        assert!(
            unproven(&pf, "qx", false),
            "{what}: {:?}\n{}",
            sites(&pf, "qx", false),
            pf.dump_text()
        );
    }
}

/// A falsy primitive on the left stays in the result: `trim` on
/// `l && "s"` sees the left primitive (a number, an empty string, the
/// falsy primitives of ⊤, `null` beside an object). A rule that passed
/// nothing of the left would leave a string only and prove `trim` native.
#[test]
fn falsy_left_primitives_stay() {
    for (what, left) in [
        ("0", "const z: any = [0][0];"),
        ("NaN", "const z: any = 0 / [0][0];"),
        ("an empty string or a number", "const z: any = [0][0] > 1 ? \"\" : [0][0];"),
        ("⊤", "const z: any = (globalThis as any).u;"),
        ("an object or null", "const z: any = [0][0] > 1 ? { a: 1 } : null;"),
        ("an object or 0", "const z: any = [0][0] > 1 ? { a: 1 } : 0;"),
    ] {
        let pf = facts(&format!(
            "{left}
            const v: any = z && \"s\";
            console.log(typeof v, v.trim());"
        ));
        assert!(
            !native(&pf, "trim"),
            "{what}: {:?}\n{}",
            sites(&pf, "trim", true),
            pf.dump_text()
        );
    }
}

/// The control: a string on the left, whose falsy value is the empty
/// string, keeps the result a string, so `trim` stays native.
#[test]
fn string_left_stays_a_string() {
    let pf = facts(
        r#"
        function k(z: string): string { const v = z && "s"; return v.trim(); }
        console.log(k("a"), k(""));
    "#,
    );
    assert!(
        native(&pf, "trim"),
        "{:?}\n{}",
        sites(&pf, "trim", true),
        pf.dump_text()
    );
}

/// The rule itself on one node: a parameter marker passes (it stands for a
/// caller's value), an object token does not, ⊤ passes as the falsy
/// primitive kinds only, and of the primitive kinds only `symbol` (never
/// falsy) is dropped; a number keeps `number` (0, -0 and NaN are falsy).
#[test]
fn falsy_cons_passes_markers_and_falsy_kinds_only() {
    use crate::model::{guard, prim, Cons, TokKind, TOP};
    let mut m = super::copy_solver_model();
    let mk = m.tok(TokKind::ModNs(900));
    let o = m.tok(TokKind::ModNs(901));
    m.param_marker.insert(mk, (0, 0));
    let (l, d, t, u) = (m.node_l("l"), m.node_l("d"), m.node_l("t"), m.node_l("u"));
    let (n, e) = (m.node_l("n"), m.node_l("e"));
    // The nodes of one run, so the marker does not cross (MRK).
    for x in [l, d, t, u, n, e] {
        m.nodes[x as usize].frame = 1;
    }
    m.cons(l, Cons::Falsy { dst: d });
    m.cons(t, Cons::Falsy { dst: u });
    m.cons(n, Cons::Falsy { dst: e });
    m.add(l, &[mk, o], prim::SYMBOL);
    m.add_tok(t, TOP);
    m.add(n, &[o], prim::NUM | prim::SYMBOL);
    m.solve();
    let has = |m: &crate::model::Model, x, t| m.nodes[x as usize].pts.contains_tok(t);
    assert!(has(&m, d, mk), "the marker passes");
    assert!(!has(&m, d, o), "no object");
    assert_eq!(m.nodes[d as usize].prim, 0, "symbol is never falsy");
    assert!(!has(&m, u, TOP), "no ⊤");
    assert_eq!(m.nodes[u as usize].prim, guard::prims(guard::FALSY), "the falsy kinds of ⊤");
    assert!(!has(&m, e, o), "no object");
    assert_eq!(m.nodes[e as usize].prim, prim::NUM, "a number stays; symbol goes");
}
