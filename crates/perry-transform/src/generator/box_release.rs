//! #7933: releasing an async activation's boxed body locals at its terminal
//! state.
//!
//! The async-to-generator transform boxes every body local of an `async`
//! function (`Stmt::PreallocateBoxes`, one `js_box_alloc_bits` cell per local
//! per invocation) so the synthesized state-machine closures can share them
//! across suspends. Box cells are registered in the runtime's `BOX_REGISTRY`
//! and are **never freed** — that monotonicity is what makes perry#4898's
//! pointer rejection and #7906's positive pointer cache sound — and
//! `scan_box_roots_mut` marks the JSValue inside every registered cell on every
//! collection. So every local of every activation the program has *ever* run
//! stays a live GC root for the life of the process.
//!
//! The fix is to **clear** (not free) an activation's cells when its state
//! machine reaches a terminal state: a `js_box_set(cell, undefined)` keeps the
//! address registered and readable — a stale reader sees `undefined`, which is
//! already the defined value of an uninitialised boxed local (perry#4926) — and
//! drops the retention, which is the entire cost.
//!
//! Clearing a cell whose value is still *reachable* would be a silent
//! use-after-clear (a wrong answer, not a crash), so a cell is only cleared
//! when no closure in the function can hold its address. This module computes
//! that set.
//!
//! ## Why "referenced by a closure" is the right, and sufficient, test
//!
//! A box address is never a JS value: `LocalGet`/`LocalSet` on a boxed local
//! lower to `js_box_get`/`js_box_set` on the cell, and the raw address only
//! ever leaves the activation through a **closure capture slot**. Codegen
//! forwards the address into a capture slot for exactly the ids in
//! `compute_auto_captures(closure) ∩ boxed_vars`, and `compute_auto_captures`
//! is `explicit captures ∪ collect_ref_ids_in_stmts(closure body)`.
//!
//! [`closure_visible_ids`] returns a **superset** of that: the explicit
//! `captures` *and* `mutable_captures` lists plus
//! `perry_hir::analysis::collect_local_refs_expr` over the whole closure
//! expression (which descends into nested closures). An id it misses is an id
//! codegen's own free-variable walk also misses, so no capture slot for that id
//! exists and clearing its cell is unobservable.
//!
//! The one construct that breaks that argument is sloppy-mode `with`:
//! `Expr::WithGet`/`Expr::WithSet` carry a fallback `LocalId` as a *leaf field*
//! that `collect_local_refs_expr` does not report. A body containing either
//! poisons the analysis outright (`None`), and the caller clears nothing.

use perry_hir::ir::*;
use perry_hir::types::LocalId;
use std::collections::HashSet;

struct Scan {
    out: HashSet<LocalId>,
    /// Set when a construct is seen whose LocalId references cannot be
    /// enumerated (sloppy `with`). The whole analysis is then unusable.
    poisoned: bool,
}

/// Every `LocalId` that some closure inside `stmts` can observe — its declared
/// capture lists plus every local referenced anywhere in its body (transitively
/// through nested closures).
///
/// Returns `None` when the body contains a construct whose local references
/// cannot be enumerated; callers must then treat *every* id as escaping.
pub(crate) fn closure_visible_ids(stmts: &[Stmt]) -> Option<HashSet<LocalId>> {
    let mut scan = Scan {
        out: HashSet::new(),
        poisoned: false,
    };
    scan_stmts(stmts, &mut scan);
    if scan.poisoned {
        None
    } else {
        Some(scan.out)
    }
}

fn scan_stmts(stmts: &[Stmt], scan: &mut Scan) {
    for stmt in stmts {
        scan_stmt(stmt, scan);
    }
}

/// Exhaustive over `Stmt` on purpose: a new statement variant that can hold an
/// expression must be routed here explicitly rather than silently hiding a
/// closure from the escape analysis.
fn scan_stmt(stmt: &Stmt, scan: &mut Scan) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                scan_expr(e, scan);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr(e, scan),
        Stmt::Return(e) => {
            if let Some(e) = e {
                scan_expr(e, scan);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr(condition, scan);
            scan_stmts(then_branch, scan);
            if let Some(eb) = else_branch {
                scan_stmts(eb, scan);
            }
        }
        Stmt::While { condition, body } => {
            scan_expr(condition, scan);
            scan_stmts(body, scan);
        }
        Stmt::DoWhile { body, condition } => {
            scan_stmts(body, scan);
            scan_expr(condition, scan);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                scan_stmt(init, scan);
            }
            if let Some(c) = condition {
                scan_expr(c, scan);
            }
            if let Some(u) = update {
                scan_expr(u, scan);
            }
            scan_stmts(body, scan);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts(body, scan);
            if let Some(c) = catch {
                scan_stmts(&c.body, scan);
            }
            if let Some(f) = finally {
                scan_stmts(f, scan);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr(discriminant, scan);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr(t, scan);
                }
                scan_stmts(&case.body, scan);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt(body, scan),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn scan_expr(expr: &Expr, scan: &mut Scan) {
    match expr {
        // Sloppy-mode `with`: the fallback LocalId is a leaf field that the
        // shared free-variable walk does not report, so the analysis cannot be
        // trusted on this body at all.
        Expr::WithGet { .. } | Expr::WithSet { .. } => {
            scan.poisoned = true;
        }
        Expr::Closure {
            body,
            captures,
            mutable_captures,
            ..
        } => {
            scan.out.extend(captures.iter().copied());
            scan.out.extend(mutable_captures.iter().copied());
            let mut refs: Vec<LocalId> = Vec::new();
            let mut visited: HashSet<usize> = HashSet::new();
            perry_hir::analysis::collect_local_refs_expr(expr, &mut refs, &mut visited);
            scan.out.extend(refs);
            // Keep descending: nested closures contribute their own explicit
            // capture lists, and a `with` anywhere inside must still poison.
            scan_stmts(body, scan);
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| scan_expr(child, scan));
}

/// `LocalSet(id, undefined)` per id — inside the state-machine step closure
/// each id is a boxed capture, so this lowers to one
/// `js_box_set(cell, TAG_UNDEFINED)`: no allocation, no collection point, and
/// the cell stays registered.
pub(crate) fn build_box_release_stmts(ids: &[LocalId]) -> Vec<Stmt> {
    ids.iter()
        .map(|id| Stmt::Expr(Expr::LocalSet(*id, Box::new(Expr::Undefined))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_get(id: LocalId) -> Expr {
        Expr::LocalGet(id)
    }

    fn closure(body: Vec<Stmt>, captures: Vec<LocalId>) -> Expr {
        Expr::Closure {
            func_id: 900,
            params: Vec::new(),
            return_type: perry_hir::types::Type::Any,
            body,
            captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_strict: false,
            is_async: false,
            is_generator: false,
        }
    }

    /// A local read only by straight-line body code is not closure-visible, so
    /// its cell is clearable.
    #[test]
    fn plain_body_local_is_not_closure_visible() {
        let body = vec![
            Stmt::Let {
                id: 1,
                name: "v".into(),
                ty: perry_hir::types::Type::Any,
                mutable: true,
                init: Some(Expr::Number(1.0)),
            },
            Stmt::Return(Some(local_get(1))),
        ];
        let ids = closure_visible_ids(&body).expect("not poisoned");
        assert!(ids.is_empty(), "no closure in the body: {:?}", ids);
    }

    /// The negative case this whole module exists for: a local a closure can
    /// read must be reported even when the HIR capture list is empty (codegen
    /// auto-detects those captures from the body).
    #[test]
    fn closure_body_reference_is_visible_without_an_explicit_capture() {
        let inner = vec![Stmt::Return(Some(local_get(7)))];
        let body = vec![Stmt::Return(Some(closure(inner, Vec::new())))];
        let ids = closure_visible_ids(&body).expect("not poisoned");
        assert!(
            ids.contains(&7),
            "auto-detected capture must escape: {ids:?}"
        );
    }

    /// An explicit capture list entry counts even if the body never mentions it.
    #[test]
    fn explicit_capture_list_entry_is_visible() {
        let body = vec![Stmt::Expr(closure(Vec::new(), vec![11]))];
        let ids = closure_visible_ids(&body).expect("not poisoned");
        assert!(ids.contains(&11), "{ids:?}");
    }

    /// Transitive: a closure nested two deep still exposes the outer local.
    #[test]
    fn nested_closure_reference_is_visible() {
        let innermost = vec![Stmt::Return(Some(local_get(21)))];
        let middle = vec![Stmt::Return(Some(closure(innermost, Vec::new())))];
        let body = vec![Stmt::Expr(closure(middle, Vec::new()))];
        let ids = closure_visible_ids(&body).expect("not poisoned");
        assert!(ids.contains(&21), "{ids:?}");
    }

    /// Closures buried under control flow are reached (a `_ => {}` statement
    /// arm here would silently make every such local look clearable).
    #[test]
    fn closure_under_control_flow_is_visible() {
        let inner = vec![Stmt::Return(Some(local_get(31)))];
        let body = vec![Stmt::Try {
            body: vec![Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: None,
                    body: vec![Stmt::Labeled {
                        label: "l".into(),
                        body: Box::new(Stmt::While {
                            condition: Expr::Bool(true),
                            body: vec![Stmt::Expr(closure(inner, Vec::new()))],
                        }),
                    }],
                }],
            }],
            catch: None,
            finally: None,
        }];
        let ids = closure_visible_ids(&body).expect("not poisoned");
        assert!(ids.contains(&31), "{ids:?}");
    }

    /// Sloppy `with` poisons the analysis: its fallback LocalId is a leaf the
    /// shared walk does not report, so nothing may be cleared.
    #[test]
    fn with_expression_poisons_the_analysis() {
        let body = vec![Stmt::Expr(Expr::WithGet {
            object: Box::new(Expr::Undefined),
            property: "x".into(),
            fallback: Box::new(local_get(41)),
        })];
        assert!(
            closure_visible_ids(&body).is_none(),
            "`with` must poison the analysis"
        );
    }

    // ── End-to-end: the transform actually emits (and withholds) the stores ──

    fn async_module(body: Vec<Stmt>) -> Module {
        let f = Function {
            id: 1,
            name: "f".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: perry_hir::types::Type::Any,
            body,
            is_strict: true,
            is_async: true,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        };
        let mut m = Module::new("t");
        m.functions.push(f);
        m
    }

    fn run_async_pipeline(m: &mut Module) {
        crate::async_to_generator::transform_async_to_generator(m);
        crate::generator::transform_generators(m);
    }

    /// Count `LocalSet(id, undefined)` statements anywhere in a body, including
    /// inside closures (the release stores live in the step closure).
    fn count_release_stores(stmts: &[Stmt], id: LocalId) -> usize {
        let mut n = 0;
        fn walk_stmts(stmts: &[Stmt], id: LocalId, n: &mut usize) {
            for s in stmts {
                match s {
                    Stmt::Expr(Expr::LocalSet(sid, value))
                        if *sid == id && matches!(**value, Expr::Undefined) =>
                    {
                        *n += 1;
                    }
                    _ => {}
                }
                let mut sub: Vec<&Expr> = Vec::new();
                collect_stmt_exprs(s, &mut sub);
                for e in sub {
                    walk_expr(e, id, n);
                }
                for body in stmt_child_bodies(s) {
                    walk_stmts(body, id, n);
                }
            }
        }
        fn walk_expr(e: &Expr, id: LocalId, n: &mut usize) {
            if let Expr::Closure { body, .. } = e {
                walk_stmts(body, id, n);
            }
            perry_hir::walker::walk_expr_children(e, &mut |c| walk_expr(c, id, n));
        }
        fn collect_stmt_exprs<'a>(s: &'a Stmt, out: &mut Vec<&'a Expr>) {
            match s {
                Stmt::Let { init: Some(e), .. }
                | Stmt::Expr(e)
                | Stmt::Throw(e)
                | Stmt::Return(Some(e)) => out.push(e),
                Stmt::If { condition, .. } => out.push(condition),
                Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => {
                    out.push(condition)
                }
                Stmt::For {
                    condition, update, ..
                } => {
                    if let Some(c) = condition {
                        out.push(c);
                    }
                    if let Some(u) = update {
                        out.push(u);
                    }
                }
                Stmt::Switch { discriminant, .. } => out.push(discriminant),
                _ => {}
            }
        }
        fn stmt_child_bodies(s: &Stmt) -> Vec<&[Stmt]> {
            match s {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let mut v: Vec<&[Stmt]> = vec![then_branch.as_slice()];
                    if let Some(eb) = else_branch {
                        v.push(eb.as_slice());
                    }
                    v
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                    vec![body.as_slice()]
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    let mut v: Vec<&[Stmt]> = vec![body.as_slice()];
                    if let Some(c) = catch {
                        v.push(c.body.as_slice());
                    }
                    if let Some(f) = finally {
                        v.push(f.as_slice());
                    }
                    v
                }
                Stmt::Switch { cases, .. } => cases.iter().map(|c| c.body.as_slice()).collect(),
                Stmt::Labeled { body, .. } => vec![std::slice::from_ref(body.as_ref())],
                _ => Vec::new(),
            }
        }
        walk_stmts(stmts, id, &mut n);
        n
    }

    fn awaited_let(id: LocalId) -> Stmt {
        Stmt::Let {
            id,
            name: "v".into(),
            ty: perry_hir::types::Type::Any,
            mutable: false,
            init: Some(Expr::Await(Box::new(Expr::Integer(1)))),
        }
    }

    /// The positive case: a body local that survives an `await` is boxed, no
    /// closure can see it, so the terminal states must release it. Two stores —
    /// one on the resolve arm, one on the reject arm.
    #[test]
    fn a_confined_body_local_is_released_at_the_terminal_states() {
        let mut m = async_module(vec![awaited_let(50), Stmt::Return(Some(local_get(50)))]);
        run_async_pipeline(&mut m);
        assert_eq!(
            count_release_stores(&m.functions[0].body, 50),
            2,
            "expected a release on each terminal arm:\n{:#?}",
            m.functions[0].body
        );
    }

    /// The negative case that makes this safe: the same local, but a closure
    /// escapes with it. Releasing it would be a silent use-after-clear, so the
    /// transform must emit no store at all.
    #[test]
    fn a_body_local_a_closure_can_see_is_never_released() {
        let escaping = closure(vec![Stmt::Return(Some(local_get(50)))], Vec::new());
        let mut m = async_module(vec![awaited_let(50), Stmt::Return(Some(escaping))]);
        run_async_pipeline(&mut m);
        assert_eq!(
            count_release_stores(&m.functions[0].body, 50),
            0,
            "a closure-visible local must never be released:\n{:#?}",
            m.functions[0].body
        );
    }

    /// `__gen_sent` (the value the last `await` delivered) is released too, and
    /// the control locals are not: an `undefined` `__gen_done` would drop a late
    /// resume into the dispatch loop with no matching state.
    #[test]
    fn the_state_machine_control_locals_are_not_released() {
        let mut m = async_module(vec![awaited_let(50), Stmt::Return(Some(local_get(50)))]);
        run_async_pipeline(&mut m);
        let body = &m.functions[0].body;
        // `PreallocateBoxes` lists the activation's cells; ids 0..=3 of the
        // transform's own allocation are state/done/sent/executing. Find the
        // prealloc list and assert at most one of its transform-internal ids is
        // released (that one is `__gen_sent`).
        let prealloc: Vec<LocalId> = body
            .iter()
            .find_map(|s| match s {
                Stmt::PreallocateBoxes(ids) => Some(ids.clone()),
                _ => None,
            })
            .expect("the activation preallocates its boxes");
        let released: Vec<LocalId> = prealloc
            .iter()
            .copied()
            .filter(|id| count_release_stores(body, *id) > 0)
            .collect();
        // Exactly the user local (50) and `__gen_sent`.
        assert_eq!(
            released.len(),
            2,
            "released set should be {{user local, __gen_sent}}, got {released:?} of {prealloc:?}"
        );
        assert!(released.contains(&50), "{released:?}");
    }

    #[test]
    fn release_stmts_are_undefined_stores() {
        let stmts = build_box_release_stmts(&[3, 5]);
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                assert_eq!(*id, 3);
                assert!(matches!(**value, Expr::Undefined));
            }
            other => panic!("unexpected release stmt: {other:?}"),
        }
    }
}
