//! SEEDNA N1: a member that a store through ⊤ may have written onto a
//! builtin prototype holds ⊤ and any primitive, and no builtin function
//! value beside ⊤ (⊤ already stands for any builtin function).
//!
//! N1 changes solve-time state (work, births, census records), not the
//! final facts: the program below has `funcs 0` on main too. So the test
//! reads the solved model: a node that holds ⊤ must not also hold the
//! shared builtin-function token (`BuiltinFn(Native::Any)`).
use super::*;
use crate::model::{Model, Native, TokKind, TOP};
use std::cell::RefCell;

thread_local! {
    /// Per solved node: (index, holds ⊤, holds the shared builtin function).
    static SOLVED: RefCell<Vec<(usize, bool, bool)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn record(m: &Model) {
    let v = m
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let any = n
                .pts
                .to_sorted()
                .iter()
                .any(|&t| m.toks[t as usize] == TokKind::BuiltinFn(Native::Any));
            (i, n.pts.contains_tok(TOP), any)
        })
        .collect();
    SOLVED.with_borrow_mut(|s| *s = v);
}

/// Nodes of the last solve that hold ⊤ beside the shared builtin function.
fn top_beside_builtin_fn() -> Vec<usize> {
    SOLVED.with_borrow(|s| {
        s.iter()
            .filter(|(_, top, any)| *top && *any)
            .map(|(i, _, _)| *i)
            .collect()
    })
}

/// A store through ⊤ with an unknown key (`unknown()[k] = 1`) may write any
/// key onto a builtin prototype; a member the native `f` lacks (`f.zap`) is
/// then the opaque member: `intrinsic_miss` -> `opaque_member` when the
/// store comes first, `replay_native_missing` when it is seen later.
fn written_member(store_first: bool) -> String {
    let store = "unknown()[process.argv[2]] = 1;\n";
    format!(
        "declare function unknown(): any;\n\
         class R {{ v: any; constructor(v: any) {{ this.v = v; }} }}\n\
         const f: any = [1].map;\n\
         {}const r = new R(f.zap);\n{}\
         console.log(r.v);\n",
        if store_first { store } else { "" },
        if store_first { "" } else { store },
    )
}

fn check(store_first: bool) {
    let src = written_member(store_first);
    let pf = facts_with(&src, interp_cfg());
    let v = field(tmpl(&pf, "R"), "v");
    assert!(v.kind.top, "{src}\n{}", pf.dump_text());
    assert_eq!(v.kind.funcs, 0, "{src}\n{}", pf.dump_text());
    let bad = top_beside_builtin_fn();
    assert!(
        bad.is_empty(),
        "nodes holding ⊤ beside the shared builtin function: {bad:?}\n{src}"
    );
}

/// f3, the store first: the read is `intrinsic_miss` -> `opaque_member`.
#[test]
fn f3_written_key_read_after_the_store_is_top_without_a_builtin_function() {
    check(true);
}

/// f3, the store last: the read is replayed (`replay_native_missing`).
#[test]
fn f3_written_key_read_before_the_store_is_top_without_a_builtin_function() {
    check(false);
}

/// Control (N1 keeps ⊤): the same read, its value called with an argument
/// `p`: the callee is ⊤, `p` escapes.
#[test]
fn control_written_key_callee_is_still_top() {
    let src = unseen(
        "class P { x: number; constructor() { this.x = 1; } }\n\
         const k: string = process.argv[2] ?? \"k\";\n\
         const u: any = (globalThis as any).hostObj;\n\
         u[k] = 1;\n\
         const p = new P();\n\
         const f: any = ({} as any)[k];\n\
         f(p);\n\
         console.log(p.x);\n",
    );
    let pf = facts(&src);
    assert!(tmpl(&pf, "P").escape.is_some(), "{}", pf.dump_text());
}
