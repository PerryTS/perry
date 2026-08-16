//! #7933: releasing an async activation's boxed body locals at its terminal
//! state.
//!
//! The async-to-generator transform boxes every body local of an `async`
//! function (`Stmt::PreallocateBoxes`, one `js_box_alloc_bits` cell per local
//! per invocation) so the synthesized state-machine closures can share them
//! across suspends. Box cells are registered in the runtime's `BOX_REGISTRY`,
//! and `scan_box_roots_mut` marks the JSValue inside every registered cell on
//! every collection. So every local of every activation the program has *ever*
//! run used to stay a live GC root for the life of the process.
//!
//! #7933 (PR #7939) fixed the *retention* half by clearing the releasable cells
//! at a terminal state. It deliberately did not free them: registry
//! monotonicity was what made perry#4898's pointer rejection and #7906's
//! positive pointer cache sound. The cost of that choice was the other half of
//! the bug — cell + registry bytes per completed activation, growing linearly,
//! invisible to every GC counter because none of it is in the GC heap.
//!
//! #8208 makes the release real. `Stmt::ReleaseBoxes` lowers to
//! `js_box_release` / `js_i32_box_release` / `js_bool_box_release`, which clear
//! the cell, de-register it, evict its positive-cache slot, and park it in a
//! per-kind quarantine; `flush_released_boxes` publishes the quarantine to a
//! free pool at the outermost microtask-pump exit once the task queue is empty,
//! and `js_*box_alloc*` pops that pool before calling `std::alloc`. Registry
//! membership is therefore no longer monotonic — but the property perry#4898
//! and #7906 actually depend on survives untouched, because cell memory is
//! never handed back to the allocator: an address minted by `js_box_alloc*`
//! stays 8 readable bytes of box cell for the life of the thread, so "was a
//! box" can never become "is another object".
//!
//! Releasing a cell whose value is still *reachable* would be a silent
//! use-after-release (a wrong answer, not a crash), so a cell is only released
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
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
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

/// One `Stmt::ReleaseBoxes` naming every id in the terminal release set.
///
/// Inside the state-machine step closure each id is a boxed capture, so this
/// lowers to one `js_*box_release` per cell: clear, de-register, evict the
/// positive-cache slot, park for reuse. No allocation and no collection point,
/// so it needs no rooting — but unlike #7933's `js_box_set(cell, undefined)`
/// the cell does NOT stay registered.
pub(crate) fn build_box_release_stmts(ids: &[LocalId]) -> Vec<Stmt> {
    if ids.is_empty() {
        return Vec::new();
    }
    // One `Stmt::ReleaseBoxes` instead of per-id `LocalSet(id, undefined)`
    // stores: codegen lowers it to `js_box_release*` calls that clear the
    // cell AND de-register + park it for reuse, so a completed activation
    // stops costing malloc-side memory, not just GC retention.
    vec![Stmt::ReleaseBoxes(ids.to_vec())]
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

    /// Count how many `Stmt::ReleaseBoxes` lists name `id`, anywhere in a
    /// body, including inside closures (the releases live in the step
    /// closure's terminal arms).
    fn count_release_stores(stmts: &[Stmt], id: LocalId) -> usize {
        let mut n = 0;
        fn walk_stmts(stmts: &[Stmt], id: LocalId, n: &mut usize) {
            for s in stmts {
                match s {
                    Stmt::ReleaseBoxes(ids) if ids.contains(&id) => {
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

    /// The whole activation frame releases at the terminal states — the user
    /// locals, `__gen_sent`, AND the state-machine control cells
    /// (`__gen_state`/`__gen_done`/`__gen_executing`). The control cells are
    /// safe to release because a stray duplicate resume observes the PARKED
    /// values (`js_bool_box_release` parks `true` = the terminal
    /// short-circuit, `js_i32_box_release` parks `-1` = no dispatch case),
    /// which reproduces the pre-release terminal path exactly.
    #[test]
    fn the_whole_activation_frame_is_released() {
        let mut m = async_module(vec![awaited_let(50), Stmt::Return(Some(local_get(50)))]);
        run_async_pipeline(&mut m);
        let body = &m.functions[0].body;
        let prealloc: Vec<LocalId> = body
            .iter()
            .find_map(|s| match s {
                Stmt::PreallocateBoxes(ids) => Some(ids.clone()),
                _ => None,
            })
            .expect("the activation preallocates its boxes");
        let unreleased: Vec<LocalId> = prealloc
            .iter()
            .copied()
            .filter(|id| count_release_stores(body, *id) == 0)
            .collect();
        assert!(
            unreleased.is_empty(),
            "every preallocated cell must be in the terminal release set; \
             {unreleased:?} of {prealloc:?} are not:\n{body:#?}"
        );
        assert!(
            count_release_stores(body, 50) == 2,
            "the user local releases on both terminal arms"
        );
    }

    /// The FORWARD direction of `the_whole_activation_frame_is_released`, and
    /// the invariant codegen's boxing analysis now leans on explicitly.
    ///
    /// `perry-codegen`'s `collect_prealloc_box_ids_in_stmts` deliberately does
    /// NOT let a `ReleaseBoxes` vote on which locals get boxed (a reclamation
    /// hint must not change a local's representation), and
    /// `emit_release_boxes` silently skips any id that is not in
    /// `boxed_vars`. Both are only harmless because the transform never
    /// releases an id it did not also preallocate. If that ever stops being
    /// true the release goes SILENTLY INERT — the leak comes back with every
    /// test still green — so it is asserted here rather than assumed.
    #[test]
    fn every_released_id_is_also_preallocated() {
        let mut m = async_module(vec![
            awaited_let(50),
            Stmt::Let {
                id: 51,
                name: "w".into(),
                ty: perry_hir::types::Type::Any,
                mutable: true,
                init: Some(Expr::Await(Box::new(Expr::LocalGet(50)))),
            },
            Stmt::Return(Some(local_get(51))),
        ]);
        run_async_pipeline(&mut m);
        let body = &m.functions[0].body;

        let prealloc: std::collections::HashSet<LocalId> = body
            .iter()
            .filter_map(|s| match s {
                Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => Some(ids.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(
            !prealloc.is_empty(),
            "the fixture must actually box something, or this test is vacuous"
        );

        let mut released: Vec<LocalId> = Vec::new();
        collect_released_ids(body, &mut released);
        assert!(
            !released.is_empty(),
            "the fixture must actually release something, or this test is vacuous"
        );

        let orphans: Vec<LocalId> = released
            .iter()
            .copied()
            .filter(|id| !prealloc.contains(id))
            .collect();
        assert!(
            orphans.is_empty(),
            "released ids {orphans:?} are never preallocated, so codegen will \
             skip them and the release becomes a silent no-op; \
             preallocated={prealloc:?}"
        );
    }

    /// Every id named by a `ReleaseBoxes` anywhere in `stmts`, including
    /// inside closures (releases live in the step closure's terminal arms).
    fn collect_released_ids(stmts: &[Stmt], out: &mut Vec<LocalId>) {
        for s in stmts {
            if let Stmt::ReleaseBoxes(ids) = s {
                out.extend(ids.iter().copied());
            }
            let mut exprs: Vec<&Expr> = Vec::new();
            match s {
                Stmt::Let { init: Some(e), .. }
                | Stmt::Expr(e)
                | Stmt::Throw(e)
                | Stmt::Return(Some(e)) => exprs.push(e),
                Stmt::If { condition, .. } => exprs.push(condition),
                Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => {
                    exprs.push(condition)
                }
                Stmt::Switch { discriminant, .. } => exprs.push(discriminant),
                _ => {}
            }
            for e in exprs {
                collect_released_in_expr(e, out);
            }
            match s {
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_released_ids(then_branch, out);
                    if let Some(eb) = else_branch {
                        collect_released_ids(eb, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                    collect_released_ids(body, out)
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_released_ids(body, out);
                    if let Some(c) = catch {
                        collect_released_ids(&c.body, out);
                    }
                    if let Some(f) = finally {
                        collect_released_ids(f, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        collect_released_ids(&c.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => {
                    collect_released_ids(std::slice::from_ref(body.as_ref()), out)
                }
                _ => {}
            }
        }
    }

    fn collect_released_in_expr(e: &Expr, out: &mut Vec<LocalId>) {
        if let Expr::Closure { body, .. } = e {
            collect_released_ids(body, out);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| collect_released_in_expr(c, out));
    }

    #[test]
    fn release_stmts_are_one_release_boxes_stmt() {
        let stmts = build_box_release_stmts(&[3, 5]);
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            Stmt::ReleaseBoxes(ids) => assert_eq!(ids.as_slice(), &[3, 5]),
            other => panic!("unexpected release stmt: {other:?}"),
        }
        assert!(
            build_box_release_stmts(&[]).is_empty(),
            "an empty release set emits nothing"
        );
    }
}
