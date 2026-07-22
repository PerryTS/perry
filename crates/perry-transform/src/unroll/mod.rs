//! HIR-level static-trip-count for-loop full-unroll pass.
//!
//! Detects the canonical small-fixed-trip-count for-loop shape:
//!
//! ```text
//! for (let i = LO; i {<,<=} HI; i++) { body }
//! ```
//!
//! where `LO` and `HI` are integer literals and the resulting trip count is
//! small (≤ `MAX_TRIP_COUNT`). Replaces the entire `Stmt::For` with N copies
//! of the body, with every `Expr::LocalGet(i)` substituted by
//! `Expr::Integer(LO + n)` for the n-th copy.
//!
//! Motivation — `image_convolution`'s 5×5 blur kernel:
//!
//! ```text
//! for (let ky = -2; ky <= 2; ky++) {
//!   const krow = KERNEL[ky + 2];
//!   for (let kx = -2; kx <= 2; kx++) {
//!     const k = krow[kx + 2];
//!     rAcc += src[idx] * k;
//!     ...
//!   }
//! }
//! ```
//!
//! With the kx loop unrolled (`kx` substituted with -2, -1, 0, 1, 2), the
//! `KERNEL[ky + 2][kx + 2]` access folds to `KERNEL[ky + 2][0..4]`. With
//! the ky loop also unrolled, both indices are compile-time integers and
//! Perry's existing flat-const machinery (`Expr::IndexGet` on a flat
//! `[25 x i32]` global with literal indices) replaces the load with a
//! constant. LLVM then specializes mul-by-1 to no-op, mul-by-4 to a
//! 2-bit shift, and mul-by-16 to a 4-bit shift — matching Zig's
//! ~130 ms scalar blur kernel instead of Perry's SIMD-bound ~240 ms.
//!
//! ## Safety guards (rejected shapes)
//!
//! Bodies that contain any of the following are NOT unrolled. Each shape
//! has a specific reason it can't be safely unrolled by N-copy substitution:
//!
//! - **`break` / `continue` / labeled break/continue** — would need to be
//!   rewritten as `LabeledBreak` to a synthetic label wrapping the
//!   unrolled stmts. Out of scope for v1.
//! - **`Stmt::Labeled`** — same reason; the label is loop-scoped and
//!   would alias with siblings post-unroll.
//! - **Closures capturing the IV** — each iteration needs to capture a
//!   different value of `i`, but unrolling produces stmts at the caller
//!   scope where `i` no longer exists. Substituting `LocalGet(i)` to
//!   `Integer(N)` inside the closure body works only for closures that
//!   capture-by-value at construction time AND aren't called after the
//!   IV's loop-scope ends. Conservative: reject all closures referencing
//!   the IV.
//! - **`LocalSet(i, ...)` or `Update { id: i }` inside body** — user is
//!   manually mutating the IV; unrolling would lose those writes.
//!   Allowed only in the for-loop's own `update` slot (by definition).
//! - **Nested `Stmt::For` shadowing the same IV id** — can't happen if
//!   HIR has unique LocalIds (which it should), but the analysis bails
//!   defensively.
//! - **`Stmt::Try`** — `try { for {...} }` is fine, but `for { try {...} }`
//!   would need each unrolled iteration to share the same `catch` /
//!   `finally`. Conservative: reject.
//!
//! ## Trip count bound
//!
//! `MAX_TRIP_COUNT = 8`. Image_convolution's kernels are 5 trips each;
//! at 5×5 = 25 total inlined stmts per pixel and ~150 byte body per
//! statement, the unrolled module init is bounded at ~5KB extra IR per
//! kernel-bearing function. Larger trips would inflate code size faster
//! than LLVM constant-folding can claw back.

use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{CallArg, CompareOp, Expr, Module, Stmt, UpdateOp};
use std::collections::HashMap;

pub(crate) mod escape_analysis;
use escape_analysis::compute_loop_escaping_ids;

// #5293: the max-LocalId / max-FuncId scans were copy-pasted here; route through
// the canonical exhaustive-walker implementations in `generator::id_scan`.
use crate::generator::{compute_max_func_id, compute_max_local_id};

/// Maximum trip count we'll fully unroll. 8 covers the canonical
/// image-kernel shapes (3×3, 5×5, 7×7) without blowing up code size.
const MAX_TRIP_COUNT: i64 = 8;

/// Apply the unroll pass to every function in `module` (including methods,
/// constructors, getters, setters, and `module.init`). Each Function whose
/// body actually changed (at least one for-loop unrolled in-place) gets its
/// `was_unrolled` flag set to `true` so the codegen-side channel-vector
/// SIMD gate can skip the manual `<4 x i32>` reduction that fights LLVM's
/// freedom to constant-fold the now-literal kernel coefficients.
///
/// `module.init`'s unroll status is tracked separately on the Module via
/// the `init_was_unrolled` field — image_convolution puts the blur kernel
/// inline at module level, not inside a function, so the flag must travel
/// with module.init.
pub fn unroll_static_loops(module: &mut Module) {
    // Allocators for fresh LocalIds and FuncIds handed out per unrolled
    // iteration. Each cloned body needs its declarations (Stmt::Let,
    // Closure params, CatchClause::param) AND any cloned `Expr::Closure`s'
    // `func_id` renamed so the N copies don't alias each other — see
    // `refresh_local_ids` and #456. (Two closures with the same FuncId
    // collapse to one compiled function in codegen, which would make every
    // unrolled iteration's `() => captured` read the same global.)
    let mut next_local_id = compute_max_local_id(module).saturating_add(1);
    let mut next_func_id = compute_max_func_id(module).saturating_add(1);

    let mut init_changed = false;
    unroll_in_stmts(
        &mut module.init,
        &mut init_changed,
        &mut next_local_id,
        &mut next_func_id,
    );
    if init_changed {
        module.init_was_unrolled = true;
    }
    for f in &mut module.functions {
        let mut changed = false;
        unroll_in_stmts(
            &mut f.body,
            &mut changed,
            &mut next_local_id,
            &mut next_func_id,
        );
        if changed {
            f.was_unrolled = true;
        }
    }
    for c in &mut module.classes {
        if let Some(ctor) = &mut c.constructor {
            let mut changed = false;
            unroll_in_stmts(
                &mut ctor.body,
                &mut changed,
                &mut next_local_id,
                &mut next_func_id,
            );
            if changed {
                ctor.was_unrolled = true;
            }
        }
        for m in &mut c.methods {
            let mut changed = false;
            unroll_in_stmts(
                &mut m.body,
                &mut changed,
                &mut next_local_id,
                &mut next_func_id,
            );
            if changed {
                m.was_unrolled = true;
            }
        }
        for (_name, g) in &mut c.getters {
            let mut changed = false;
            unroll_in_stmts(
                &mut g.body,
                &mut changed,
                &mut next_local_id,
                &mut next_func_id,
            );
            if changed {
                g.was_unrolled = true;
            }
        }
        for (_name, s) in &mut c.setters {
            let mut changed = false;
            unroll_in_stmts(
                &mut s.body,
                &mut changed,
                &mut next_local_id,
                &mut next_func_id,
            );
            if changed {
                s.was_unrolled = true;
            }
        }
        // Field initializers are expressions, not stmt vectors. The
        // canonical case (a literal-init field with no for-loop in the
        // initializer) doesn't need walking; complex initializers would
        // benefit from unroll if they contained loops, but the gain is
        // marginal and we'd need an Expr-level unroll variant. Skip.
    }
}

/// Walk `stmts` and unroll any qualifying `Stmt::For` in place. Recurses
/// into nested control flow (if/while/for/switch/try) so inner loops also
/// get a shot at unrolling. Outer loops are unrolled FIRST — once the
/// outer is gone, the inner loop appears N times in the unrolled output
/// and each copy is then walked again to unroll the inner if it qualifies.
///
/// `changed` is set to `true` whenever any `Stmt::For` in `stmts` (or
/// recursively inside its children) gets unrolled. The caller uses this
/// to mark the enclosing Function's `was_unrolled` flag so codegen can
/// disable the channel-vector SIMD reduction in unrolled bodies.
fn unroll_in_stmts(
    stmts: &mut Vec<Stmt>,
    changed: &mut bool,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    // #2308: compute the set of loop-body-declared ids that escape their
    // loop (function-scoped `var`s read outside the loop body) ONCE per
    // scope-entry, on the ORIGINAL (un-unrolled) body, then thread it
    // through the recursion. Unrolling rewrites each cloned body's
    // declarations to fresh ids via `refresh_local_ids`; an escaping `var`
    // must instead keep its original id across every copy, otherwise the
    // post-loop read binds to an id no copy ever writes. See the
    // `refresh_local_ids` / `alloc_fresh` skip and the doc on
    // `compute_loop_escaping_ids`.
    //
    // `stmts` here is the function/method/init/ctor body (every call site
    // in `unroll_static_loops` passes a top-level body). The escaping set
    // is function-scoped, so recomputing it for nested blocks would be
    // both wasteful and wrong (a nested block can't see all the `var`'s
    // use sites). The `_rec` variant carries the same set down.
    let protected = compute_loop_escaping_ids(stmts);
    unroll_in_stmts_rec(stmts, changed, next_local_id, next_func_id, &protected);
}

fn unroll_in_stmts_rec(
    stmts: &mut Vec<Stmt>,
    changed: &mut bool,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
    protected: &std::collections::HashSet<LocalId>,
) {
    let mut i = 0;
    while i < stmts.len() {
        // Recurse into nested control flow first so an inner unrollable
        // loop becomes N copies in its parent's body BEFORE the parent's
        // unroll pass clones the parent's body N more times. This ordering
        // means each unrolled iteration of an inner loop gets cloned by
        // any enclosing outer loop, but the outer's body is already
        // simplified. Same end result either way for correctness; this
        // ordering is just slightly less work.
        recurse_into_nested(
            &mut stmts[i],
            changed,
            next_local_id,
            next_func_id,
            protected,
        );

        if let Some(unrolled) = try_unroll_for(&stmts[i], next_local_id, next_func_id, protected) {
            // Replace stmts[i] with `unrolled`'s contents.
            let inserted = unrolled.len();
            stmts.splice(i..=i, unrolled);
            *changed = true;
            i += inserted;
        } else {
            i += 1;
        }
    }
}

/// Recurse into the children of a control-flow stmt so nested for-loops
/// get an unroll attempt. `Stmt::For` itself is NOT recursed into here
/// (the outer driver handles it via try_unroll_for); but its body is
/// walked so inner unrollable loops get processed before the outer's
/// unroll attempt.
fn recurse_into_nested(
    stmt: &mut Stmt,
    changed: &mut bool,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
    protected: &std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            unroll_in_stmts_rec(then_branch, changed, next_local_id, next_func_id, protected);
            if let Some(eb) = else_branch {
                unroll_in_stmts_rec(eb, changed, next_local_id, next_func_id, protected);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            unroll_in_stmts_rec(body, changed, next_local_id, next_func_id, protected);
        }
        Stmt::For { body, .. } => {
            // Inner-first: unroll any qualifying loops inside this for's
            // body before deciding whether to unroll this for itself.
            unroll_in_stmts_rec(body, changed, next_local_id, next_func_id, protected);
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                unroll_in_stmts_rec(&mut c.body, changed, next_local_id, next_func_id, protected);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            unroll_in_stmts_rec(body, changed, next_local_id, next_func_id, protected);
            if let Some(c) = catch {
                unroll_in_stmts_rec(&mut c.body, changed, next_local_id, next_func_id, protected);
            }
            if let Some(f) = finally {
                unroll_in_stmts_rec(f, changed, next_local_id, next_func_id, protected);
            }
        }
        Stmt::Labeled { body, .. } => {
            recurse_into_nested(body, changed, next_local_id, next_func_id, protected);
        }
        _ => {}
    }
}

/// Inspect a `Stmt::For` and, if it matches the canonical
/// integer-literal-bounded shape with a small trip count and a body
/// safe to unroll, return the unrolled stmt sequence. Returns `None`
/// otherwise — caller leaves the original `Stmt::For` in place.
fn try_unroll_for(
    stmt: &Stmt,
    next_local_id: &mut LocalId,
    next_func_id: &mut FuncId,
    protected: &std::collections::HashSet<LocalId>,
) -> Option<Vec<Stmt>> {
    let (init, condition, update, body) = match stmt {
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => (init, condition, update, body),
        _ => return None,
    };

    // 1. Init must be `let i = INTEGER` where INTEGER fits in i64.
    let init_box = init.as_ref()?;
    let (iv_id, lo) = match init_box.as_ref() {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(n)),
            ..
        } => (*id, *n),
        _ => return None,
    };

    // 2. Condition must be `LocalGet(iv_id) {<,<=} INTEGER`.
    let (cmp_op, hi) = match condition.as_ref()? {
        Expr::Compare { op, left, right } => {
            let left_id = match left.as_ref() {
                Expr::LocalGet(id) if *id == iv_id => *id,
                _ => return None,
            };
            let _ = left_id;
            let hi = match right.as_ref() {
                Expr::Integer(n) => *n,
                _ => return None,
            };
            (op, hi)
        }
        _ => return None,
    };

    // 3. Update must be `iv_id++` (Update with op=Increment).
    match update.as_ref()? {
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == iv_id => {}
        _ => return None,
    }

    // 4. Trip count must be small.
    let trips = match cmp_op {
        CompareOp::Lt => hi.saturating_sub(lo),
        CompareOp::Le => hi.saturating_sub(lo).saturating_add(1),
        _ => return None,
    };
    if trips <= 0 || trips > MAX_TRIP_COUNT {
        return None;
    }

    // 5. Body must be safe to unroll (no break/continue/labeled, no
    //    LocalSet/Update on iv_id, no closure capturing iv_id, no Try).
    if !body_is_unrollable(body, iv_id) {
        return None;
    }

    // 6. Emit N copies. For each iteration, clone the body and substitute
    //    every Expr::LocalGet(iv_id) with Expr::Integer(lo + n). The IV's
    //    own `Stmt::Let` from the for's init is dropped — it doesn't
    //    appear in the unrolled output. `update` is dropped likewise (it
    //    only mutated the IV slot which no longer exists post-unroll).
    //
    //    After substitution, `refresh_local_ids` rewrites every binding
    //    declared inside the cloned body (Stmt::Let, Closure params,
    //    CatchClause::param) — and every reference to those bindings —
    //    to fresh ids. Without this, the N copies share LocalIds, which
    //    breaks two things at once:
    //      * codegen emits one `@perry_global_*__<id>` per module-init
    //        Stmt::Let with a referenced id, and N copies of the same
    //        id cause LLVM duplicate-global errors (issue #456); and
    //      * each iteration's `() => captured` closure is supposed to
    //        bind a distinct value, which requires distinct capture ids.
    let mut out: Vec<Stmt> = Vec::with_capacity((trips as usize) * body.len());
    for n in 0..trips {
        let value = lo + n;
        let mut cloned: Vec<Stmt> = body.to_vec();
        for s in &mut cloned {
            substitute_localget_with_int_in_stmt(s, iv_id, value);
        }
        refresh_local_ids(&mut cloned, next_local_id, next_func_id, protected);
        out.extend(cloned);
    }
    Some(out)
}

/// Returns true if the body is safe to unroll. Walks `body` tracking
/// loop nesting depth: `break`/`continue` are rejected at depth 0 (they
/// would target the for-loop being unrolled, which won't exist post-
/// unroll) but allowed at depth ≥ 1 (they target an inner loop that
/// survives the unroll intact, with its own labels and exit blocks).
///
/// The inliner expands every same-module function call as a synthetic
/// `Stmt::DoWhile { body: <inlined>, condition: false }` wrapper, with
/// every `return e` rewritten to `LocalSet(let_id, e); break`. So a
/// caller body that uses any inlined helper (clampIdx, clampU8, …)
/// has nested do-whiles full of breaks targeting the inliner's wrapper
/// loop. Counting those as depth-1 is correct: their breaks exit the
/// inliner's synthetic do-while, not the for being unrolled.
fn body_is_unrollable(body: &[Stmt], iv_id: LocalId) -> bool {
    body.iter().all(|s| stmt_is_unrollable(s, iv_id, 0))
}

fn stmt_is_unrollable(stmt: &Stmt, iv_id: LocalId, loop_depth: u32) -> bool {
    match stmt {
        Stmt::Break | Stmt::Continue => loop_depth > 0,
        // Labeled break/continue: even at loop_depth > 0 we don't know
        // whether the label points at our enclosing for or at an inner
        // construct. Conservative: reject. Labeled control flow inside
        // a hot kernel is rare; image_convolution doesn't use it.
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => false,
        // `Stmt::Labeled` would need its label rewritten per-unroll-iter
        // (each unrolled copy needs a unique label name) AND any
        // LabeledBreak inside a sibling stmt could target it. Out of
        // scope for v1.
        Stmt::Labeled { .. } => false,
        Stmt::Try { .. } => false,
        Stmt::Let { id, init, .. } => {
            if *id == iv_id {
                // Shadowing — shouldn't happen since HIR ids are unique
                // and this would be an inner Let with the same id as the
                // outer for-init. Defensive bail.
                return false;
            }
            init.as_ref().is_none_or(|e| expr_is_unrollable(e, iv_id))
        }
        Stmt::Expr(e) | Stmt::Throw(e) => expr_is_unrollable(e, iv_id),
        Stmt::Return(opt) => opt.as_ref().is_none_or(|e| expr_is_unrollable(e, iv_id)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_is_unrollable(condition, iv_id)
                && then_branch
                    .iter()
                    .all(|s| stmt_is_unrollable(s, iv_id, loop_depth))
                && else_branch
                    .as_ref()
                    .is_none_or(|eb| eb.iter().all(|s| stmt_is_unrollable(s, iv_id, loop_depth)))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            // Inner loop: bumps depth so break/continue inside become safe.
            expr_is_unrollable(condition, iv_id)
                && body
                    .iter()
                    .all(|s| stmt_is_unrollable(s, iv_id, loop_depth + 1))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_none_or(|s| stmt_is_unrollable(s, iv_id, loop_depth + 1))
                && condition
                    .as_ref()
                    .is_none_or(|e| expr_is_unrollable(e, iv_id))
                && update.as_ref().is_none_or(|e| expr_is_unrollable(e, iv_id))
                && body
                    .iter()
                    .all(|s| stmt_is_unrollable(s, iv_id, loop_depth + 1))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            // Switch case bodies have `break` that targets the switch (not
            // the enclosing for). Counted as depth + 1 to allow them.
            expr_is_unrollable(discriminant, iv_id)
                && cases.iter().all(|c| {
                    c.test.as_ref().is_none_or(|t| expr_is_unrollable(t, iv_id))
                        && c.body
                            .iter()
                            .all(|s| stmt_is_unrollable(s, iv_id, loop_depth + 1))
                })
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => true,
    }
}

fn expr_is_unrollable(e: &Expr, iv_id: LocalId) -> bool {
    // Reject writes to the IV.
    match e {
        Expr::LocalSet(id, _) if *id == iv_id => return false,
        Expr::Update { id, .. } if *id == iv_id => return false,
        // Closures: reject any closure that even mentions the IV. A
        // closure captured-by-value at construction would semantically
        // freeze the IV's current value, but our HIR captures are by
        // ID; substituting LocalGet(iv) → Integer(N) inside the
        // closure body works only if the closure isn't called outside
        // the IV's live range. The image_convolution kernel doesn't
        // create closures inside its blur loops, so this restriction
        // is free for the target workload.
        Expr::Closure { body, captures, .. } => {
            if captures.contains(&iv_id) {
                return false;
            }
            // Defensive: walk the closure body to catch any direct
            // `LocalGet(iv_id)` reference that wasn't materialized as a
            // capture entry (shouldn't happen in well-formed HIR, but
            // checking is cheap). Closure body's break/continue are
            // always lexically scoped to a loop *inside* the closure
            // (free `break` outside a loop is a JS syntax error), so we
            // start at loop_depth=1 to suppress the always-true Break/
            // Continue rejection.
            if !body.iter().all(|s| stmt_is_unrollable(s, iv_id, 1)) {
                return false;
            }
            return true;
        }
        _ => {}
    }
    // Recurse into all sub-expressions.
    let mut ok = true;
    walk_expr_children(e, &mut |child| {
        if !expr_is_unrollable(child, iv_id) {
            ok = false;
        }
    });
    ok
}

/// Replace every `Expr::LocalGet(iv_id)` in `stmt` with `Expr::Integer(value)`.
/// `LocalSet` / `Update` of `iv_id` are rejected by the unrollability
/// pre-check, so this fn doesn't need to handle them.
fn substitute_localget_with_int_in_stmt(stmt: &mut Stmt, iv_id: LocalId, value: i64) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                substitute_localget_with_int(e, iv_id, value);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => substitute_localget_with_int(e, iv_id, value),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                substitute_localget_with_int(e, iv_id, value);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            substitute_localget_with_int(condition, iv_id, value);
            for s in then_branch {
                substitute_localget_with_int_in_stmt(s, iv_id, value);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    substitute_localget_with_int_in_stmt(s, iv_id, value);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            substitute_localget_with_int(condition, iv_id, value);
            for s in body {
                substitute_localget_with_int_in_stmt(s, iv_id, value);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                substitute_localget_with_int_in_stmt(s, iv_id, value);
            }
            if let Some(c) = condition {
                substitute_localget_with_int(c, iv_id, value);
            }
            if let Some(u) = update {
                substitute_localget_with_int(u, iv_id, value);
            }
            for s in body {
                substitute_localget_with_int_in_stmt(s, iv_id, value);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            substitute_localget_with_int(discriminant, iv_id, value);
            for c in cases {
                if let Some(t) = &mut c.test {
                    substitute_localget_with_int(t, iv_id, value);
                }
                for s in &mut c.body {
                    substitute_localget_with_int_in_stmt(s, iv_id, value);
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                substitute_localget_with_int_in_stmt(s, iv_id, value);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    substitute_localget_with_int_in_stmt(s, iv_id, value);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    substitute_localget_with_int_in_stmt(s, iv_id, value);
                }
            }
        }
        Stmt::Labeled { body, .. } => {
            substitute_localget_with_int_in_stmt(body, iv_id, value);
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn substitute_localget_with_int(expr: &mut Expr, iv_id: LocalId, value: i64) {
    if let Expr::LocalGet(id) = expr {
        if *id == iv_id {
            *expr = Expr::Integer(value);
            return;
        }
    }
    walk_expr_children_mut(expr, &mut |child| {
        substitute_localget_with_int(child, iv_id, value);
    });
}

/// Per-iteration LocalId remap pass for `try_unroll_for`.
///
/// Walks `stmts` and assigns a fresh `LocalId` (drawn from `next_id`) to
/// every binding the body declares — `Stmt::Let { id }`, `Closure::params`,
/// and `CatchClause::param` — then rewrites every reference to those
/// bindings within `stmts` to the new id. This includes:
///   * `Expr::LocalGet`, `Expr::LocalSet`, `Expr::Update.id`
///   * `Closure::captures` and `Closure::mutable_captures` Vecs
///   * `Expr::ArrayPush.array_id`, `ArrayPushSpread`, `ArrayPop`,
///     `ArrayShift`, `ArrayUnshift`, `ArraySplice`, `ArrayCopyWithin`
///   * `Expr::SetAdd.set_id`
///
/// References to LocalIds NOT declared inside `stmts` (outer-scope vars
/// captured by closures, the array `fns` captured from outside the for,
/// etc.) are left unchanged — only ids the body itself introduces get
/// remapped, which is the correct scope discipline.
///
/// `Stmt::Try` and `Stmt::Labeled` are rejected by `body_is_unrollable`
/// so they shouldn't appear here, but the walker handles them defensively
/// in case the unrollability rules ever loosen.
fn refresh_local_ids(
    stmts: &mut [Stmt],
    next_id: &mut LocalId,
    next_func_id: &mut FuncId,
    protected: &std::collections::HashSet<LocalId>,
) {
    // #2308: seed the remap with identity mappings for every protected
    // (loop-escaping `var`) id. `alloc_fresh` reuses an existing remap
    // entry instead of minting a new id, so a protected declaration keeps
    // its ORIGINAL id across every unrolled copy and `lookup` leaves
    // references to it untouched. The post-loop read — which still uses the
    // original id — then binds to the slot all copies write, matching JS
    // function-scoped `var` semantics. Non-protected (block-scoped
    // let/const, closure params, …) ids are absent from the seed and get
    // fresh ids as before, so per-iteration closure captures stay distinct.
    let mut remap: HashMap<LocalId, LocalId> = HashMap::new();
    for &id in protected {
        remap.insert(id, id);
    }
    for s in stmts.iter_mut() {
        refresh_in_stmt(s, &mut remap, next_id, next_func_id);
    }
}

fn alloc_fresh(remap: &mut HashMap<LocalId, LocalId>, next_id: &mut LocalId, id: &mut LocalId) {
    // A pre-seeded entry (see `refresh_local_ids`, #2308) means this id is
    // protected — a loop-escaping `var` that must keep its original id
    // across copies. Reuse the existing mapping rather than minting a new
    // one. For ordinary declarations the id is absent and we allocate fresh.
    if let Some(&existing) = remap.get(id) {
        *id = existing;
        return;
    }
    let new_id = *next_id;
    *next_id = next_id.saturating_add(1);
    remap.insert(*id, new_id);
    *id = new_id;
}

fn lookup(remap: &HashMap<LocalId, LocalId>, id: &mut LocalId) {
    if let Some(&new) = remap.get(id) {
        *id = new;
    }
}

fn refresh_in_stmt(
    stmt: &mut Stmt,
    remap: &mut HashMap<LocalId, LocalId>,
    next_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            alloc_fresh(remap, next_id, id);
            if let Some(e) = init {
                refresh_in_expr(e, remap, next_id, next_func_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => refresh_in_expr(e, remap, next_id, next_func_id),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                refresh_in_expr(e, remap, next_id, next_func_id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            refresh_in_expr(condition, remap, next_id, next_func_id);
            for s in then_branch {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    refresh_in_stmt(s, remap, next_id, next_func_id);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            refresh_in_expr(condition, remap, next_id, next_func_id);
            for s in body {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
            if let Some(c) = condition {
                refresh_in_expr(c, remap, next_id, next_func_id);
            }
            if let Some(u) = update {
                refresh_in_expr(u, remap, next_id, next_func_id);
            }
            for s in body {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
            if let Some(c) = catch {
                if let Some((id, _)) = &mut c.param {
                    alloc_fresh(remap, next_id, id);
                }
                for s in &mut c.body {
                    refresh_in_stmt(s, remap, next_id, next_func_id);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    refresh_in_stmt(s, remap, next_id, next_func_id);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            refresh_in_expr(discriminant, remap, next_id, next_func_id);
            for c in cases {
                if let Some(t) = &mut c.test {
                    refresh_in_expr(t, remap, next_id, next_func_id);
                }
                for s in &mut c.body {
                    refresh_in_stmt(s, remap, next_id, next_func_id);
                }
            }
        }
        Stmt::Labeled { body, .. } => refresh_in_stmt(body, remap, next_id, next_func_id),
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
            for id in ids.iter_mut() {
                alloc_fresh(remap, next_id, id);
            }
        }
    }
}

fn refresh_in_expr(
    expr: &mut Expr,
    remap: &mut HashMap<LocalId, LocalId>,
    next_id: &mut LocalId,
    next_func_id: &mut FuncId,
) {
    // Handle id-bearing variants explicitly so we both remap the id and
    // recurse into any contained sub-expressions ourselves. Variants with
    // no LocalId fall through to the walker.
    match expr {
        Expr::LocalGet(id) => {
            lookup(remap, id);
            return;
        }
        Expr::LocalSet(id, value) => {
            lookup(remap, id);
            refresh_in_expr(value, remap, next_id, next_func_id);
            return;
        }
        Expr::Update { id, .. } => {
            lookup(remap, id);
            return;
        }
        Expr::ArrayPush { array_id, value } => {
            lookup(remap, array_id);
            refresh_in_expr(value, remap, next_id, next_func_id);
            return;
        }
        Expr::ArrayPushSpread { array_id, source } => {
            lookup(remap, array_id);
            refresh_in_expr(source, remap, next_id, next_func_id);
            return;
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            lookup(remap, id);
            return;
        }
        Expr::ArrayUnshift { array_id, value } => {
            lookup(remap, array_id);
            refresh_in_expr(value, remap, next_id, next_func_id);
            return;
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            lookup(remap, array_id);
            refresh_in_expr(start, remap, next_id, next_func_id);
            if let Some(dc) = delete_count {
                refresh_in_expr(dc, remap, next_id, next_func_id);
            }
            for it in items {
                refresh_in_expr(it, remap, next_id, next_func_id);
            }
            return;
        }
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => {
            lookup(remap, array_id);
            refresh_in_expr(target, remap, next_id, next_func_id);
            refresh_in_expr(start, remap, next_id, next_func_id);
            if let Some(e) = end {
                refresh_in_expr(e, remap, next_id, next_func_id);
            }
            return;
        }
        Expr::SetAdd { set_id, value } => {
            lookup(remap, set_id);
            refresh_in_expr(value, remap, next_id, next_func_id);
            return;
        }
        Expr::Closure {
            func_id,
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Each cloned closure must get its own FuncId. Codegen keys
            // compiled functions by FuncId, so two cloned `() => captured`
            // closures sharing one FuncId would collapse into a single
            // compiled function — every iteration's `fns[i]()` would then
            // read the same global. Bumping FuncId per clone keeps each
            // closure on its own compiled body.
            *func_id = *next_func_id;
            *next_func_id = next_func_id.saturating_add(1);

            // Closure params are NEW declarations within the closure
            // scope — allocate fresh ids and add them to the same remap
            // so the closure body's LocalGets pick them up.
            for p in params.iter_mut() {
                alloc_fresh(remap, next_id, &mut p.id);
                if let Some(d) = &mut p.default {
                    refresh_in_expr(d, remap, next_id, next_func_id);
                }
            }
            // captures / mutable_captures are LocalIds referring to the
            // enclosing scope — remap any that the outer scope renamed.
            for c in captures.iter_mut() {
                lookup(remap, c);
            }
            for c in mutable_captures.iter_mut() {
                lookup(remap, c);
            }
            // The walker doesn't descend into Closure bodies (Vec<Stmt>) —
            // we handle that here ourselves.
            for s in body.iter_mut() {
                refresh_in_stmt(s, remap, next_id, next_func_id);
            }
            return;
        }
        Expr::CallSpread { callee, args, .. } => {
            // walk_expr_children_mut visits CallArg children — but to
            // avoid relying on its exact behavior, recurse explicitly.
            refresh_in_expr(callee, remap, next_id, next_func_id);
            for a in args {
                let inner = match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => e,
                };
                refresh_in_expr(inner, remap, next_id, next_func_id);
            }
            return;
        }
        _ => {}
    }
    // Default: recurse into all sub-expressions via the walker.
    walk_expr_children_mut(expr, &mut |child| {
        refresh_in_expr(child, remap, next_id, next_func_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::BinaryOp;

    fn ivar(id: LocalId) -> Expr {
        Expr::LocalGet(id)
    }

    fn integer(n: i64) -> Expr {
        Expr::Integer(n)
    }

    /// Test helper: wrap `try_unroll_for` with throwaway fresh-id counters
    /// so individual tests don't have to thread them. Tests pick LocalIds
    /// well below `START` so the remap-allocated ids never collide.
    fn try_unroll(stmt: &Stmt) -> Option<Vec<Stmt>> {
        const START: LocalId = 10_000;
        const FUNC_START: FuncId = 10_000;
        let mut next_id: LocalId = START;
        let mut next_func_id: FuncId = FUNC_START;
        // These tests exercise a single for-loop in isolation with no
        // enclosing scope, so there are no escaping ids to protect (#2308).
        let protected = std::collections::HashSet::new();
        try_unroll_for(stmt, &mut next_id, &mut next_func_id, &protected)
    }

    /// Test helper: wrap `unroll_in_stmts` with the same throwaway counters.
    fn run_unroll_in_stmts(stmts: &mut Vec<Stmt>, changed: &mut bool) {
        const START: LocalId = 10_000;
        const FUNC_START: FuncId = 10_000;
        let mut next_id: LocalId = START;
        let mut next_func_id: FuncId = FUNC_START;
        unroll_in_stmts(stmts, changed, &mut next_id, &mut next_func_id);
    }

    /// Build `for (let i = lo; i <= hi; i++) { body }`.
    fn make_for(iv_id: LocalId, lo: i64, hi: i64, body: Vec<Stmt>, op: CompareOp) -> Stmt {
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: iv_id,
                name: "i".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(integer(lo)),
            })),
            condition: Some(Expr::Compare {
                op,
                left: Box::new(ivar(iv_id)),
                right: Box::new(integer(hi)),
            }),
            update: Some(Expr::Update {
                id: iv_id,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body,
        }
    }

    #[test]
    fn unrolls_canonical_5_trip_loop() {
        // for (let i = -2; i <= 2; i++) { acc = acc + i; }
        let acc = 100u32;
        let i = 1u32;
        let body = vec![Stmt::Expr(Expr::LocalSet(
            acc,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(acc)),
                right: Box::new(ivar(i)),
            }),
        ))];
        let f = make_for(i, -2, 2, body, CompareOp::Le);
        let unrolled = try_unroll(&f).expect("should unroll");
        assert_eq!(unrolled.len(), 5);
        // Each iteration replaces LocalGet(i) with Integer(-2..2).
        for (n, s) in unrolled.iter().enumerate() {
            let expected_int = -2 + n as i64;
            match s {
                Stmt::Expr(Expr::LocalSet(id, value)) => {
                    assert_eq!(*id, acc);
                    match value.as_ref() {
                        Expr::Binary {
                            op: BinaryOp::Add,
                            right,
                            ..
                        } => match right.as_ref() {
                            Expr::Integer(n2) => assert_eq!(*n2, expected_int),
                            other => panic!("expected Integer, got {:?}", other),
                        },
                        other => panic!("expected Add binary, got {:?}", other),
                    }
                }
                other => panic!("expected Stmt::Expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn unrolls_lt_form() {
        // for (let i = 0; i < 3; i++) { ... }
        let i = 1u32;
        let body = vec![Stmt::Expr(Expr::LocalGet(i))];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        let unrolled = try_unroll(&f).expect("should unroll");
        // i = 0, 1, 2 — 3 trips for `<`.
        assert_eq!(unrolled.len(), 3);
    }

    #[test]
    fn rejects_loop_above_max_trip_count() {
        // for (let i = 0; i < 100; i++) — 100 trips is way above MAX_TRIP_COUNT=8.
        let i = 1u32;
        let body = vec![Stmt::Expr(Expr::LocalGet(i))];
        let f = make_for(i, 0, 100, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_break() {
        let i = 1u32;
        let body = vec![Stmt::Break];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_continue() {
        let i = 1u32;
        let body = vec![Stmt::Continue];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_iv_localset_in_body() {
        // for (let i = 0; i < 3; i++) { i = 99; }   // mutates IV mid-body
        let i = 1u32;
        let body = vec![Stmt::Expr(Expr::LocalSet(i, Box::new(integer(99))))];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_iv_update_in_body() {
        let i = 1u32;
        let body = vec![Stmt::Expr(Expr::Update {
            id: i,
            op: UpdateOp::Increment,
            prefix: false,
        })];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_try() {
        let i = 1u32;
        let body = vec![Stmt::Try {
            body: vec![],
            catch: None,
            finally: None,
        }];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_non_integer_init() {
        // for (let i = X; i < 3; i++) where X isn't an Integer literal
        let i = 1u32;
        let f = Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: i,
                name: "i".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::LocalGet(99)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(i)),
                right: Box::new(integer(3)),
            }),
            update: Some(Expr::Update {
                id: i,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![],
        };
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn rejects_loop_with_non_integer_bound() {
        let i = 1u32;
        let f = Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: i,
                name: "i".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(i)),
                right: Box::new(Expr::LocalGet(99)),
            }),
            update: Some(Expr::Update {
                id: i,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![],
        };
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn unrolls_nested_5x5_kernel() {
        // for (let ky = -2; ky <= 2; ky++) {
        //   for (let kx = -2; kx <= 2; kx++) {
        //     acc = acc + ky + kx;
        //   }
        // }
        let acc = 100u32;
        let ky = 1u32;
        let kx = 2u32;
        let inner_body = vec![Stmt::Expr(Expr::LocalSet(
            acc,
            Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(acc)),
                    right: Box::new(Expr::LocalGet(ky)),
                }),
                right: Box::new(Expr::LocalGet(kx)),
            }),
        ))];
        let inner = make_for(kx, -2, 2, inner_body, CompareOp::Le);
        let outer = make_for(ky, -2, 2, vec![inner], CompareOp::Le);

        // Wrap in a vec and run unroll_in_stmts (so nested unroll fires).
        let mut stmts = vec![outer];
        let mut changed = false;
        run_unroll_in_stmts(&mut stmts, &mut changed);
        assert!(changed, "expected unroll to fire");

        // After full nested unroll: 5 ky × 5 kx = 25 stmts.
        assert_eq!(stmts.len(), 25);
        // Check a specific iteration: iter index = (ky_iter * 5 + kx_iter)
        // for ky=-2..2, kx=-2..2.
        for ky_n in 0..5 {
            for kx_n in 0..5 {
                let stmt_idx = ky_n * 5 + kx_n;
                let expected_ky = -2 + ky_n as i64;
                let expected_kx = -2 + kx_n as i64;
                match &stmts[stmt_idx] {
                    Stmt::Expr(Expr::LocalSet(_, v)) => match v.as_ref() {
                        Expr::Binary {
                            op: BinaryOp::Add,
                            left,
                            right,
                        } => {
                            // left = (acc + ky); right = kx
                            match right.as_ref() {
                                Expr::Integer(n) => assert_eq!(
                                    *n, expected_kx,
                                    "kx mismatch at ({}, {}): got {}, want {}",
                                    ky_n, kx_n, n, expected_kx
                                ),
                                other => panic!("expected kx Integer, got {:?}", other),
                            }
                            match left.as_ref() {
                                Expr::Binary { right: ky_e, .. } => match ky_e.as_ref() {
                                    Expr::Integer(n) => assert_eq!(
                                        *n, expected_ky,
                                        "ky mismatch at ({}, {}): got {}, want {}",
                                        ky_n, kx_n, n, expected_ky
                                    ),
                                    other => panic!("expected ky Integer, got {:?}", other),
                                },
                                other => panic!("expected (acc+ky) Binary, got {:?}", other),
                            }
                        }
                        other => panic!("expected outer Binary, got {:?}", other),
                    },
                    other => panic!("expected Stmt::Expr LocalSet, got {:?}", other),
                }
            }
        }
    }

    #[test]
    fn unrolls_body_with_inliner_dowhile_breaks() {
        // The inliner expands `let xx = clampIdx(x + kx, 0, hi)` into
        // a `let xx = undefined; do { ...; xx = lo; break; ...; xx = hi;
        // break; xx = v; break; } while (false)`. The breaks belong to
        // the inner do-while, not the for being unrolled — the unroll
        // should fire despite the breaks.
        let kx = 1u32;
        let xx = 50u32;
        let body = vec![
            Stmt::Let {
                id: xx,
                name: "xx".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::DoWhile {
                body: vec![
                    Stmt::Expr(Expr::LocalSet(xx, Box::new(Expr::LocalGet(kx)))),
                    Stmt::Break,
                ],
                condition: Expr::Bool(false),
            },
        ];
        let f = make_for(kx, -2, 2, body, CompareOp::Le);
        let unrolled = try_unroll(&f).expect("inner-loop break should not block unroll");
        // 5 iterations × 2 stmts each (Let + DoWhile) = 10 stmts.
        assert_eq!(unrolled.len(), 10);
    }

    #[test]
    fn rejects_top_level_break_that_targets_the_for_itself() {
        // `for (let i = 0; i < 3; i++) { if (i === 1) break; }`
        // The `break` at depth 0 inside the for-body targets the for
        // itself — substituting LocalGet(i) → Integer(N) and dropping the
        // for would leave a stray Stmt::Break with no enclosing loop.
        let i = 1u32;
        let body = vec![Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(i)),
                right: Box::new(Expr::Integer(1)),
            },
            then_branch: vec![Stmt::Break],
            else_branch: None,
        }];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        assert!(try_unroll(&f).is_none());
    }

    #[test]
    fn unrolls_inside_if_branches() {
        let acc = 100u32;
        let i = 1u32;
        let inner = make_for(
            i,
            0,
            3,
            vec![Stmt::Expr(Expr::LocalSet(
                acc,
                Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(acc)),
                    right: Box::new(Expr::LocalGet(i)),
                }),
            ))],
            CompareOp::Lt,
        );
        let if_stmt = Stmt::If {
            condition: integer(1),
            then_branch: vec![inner],
            else_branch: None,
        };
        let mut stmts = vec![if_stmt];
        let mut changed = false;
        run_unroll_in_stmts(&mut stmts, &mut changed);
        assert!(changed, "expected unroll to fire");
        match &stmts[0] {
            Stmt::If { then_branch, .. } => {
                assert_eq!(then_branch.len(), 3, "inner for should unroll to 3 stmts");
            }
            _ => panic!("expected If"),
        }
    }

    /// #2308: a `var` declared in the loop body and read AFTER the loop is
    /// function-scoped. Every unrolled copy must keep the var's ORIGINAL id
    /// (so the last copy's write lands in the slot the post-loop read uses).
    #[test]
    fn loop_escaping_var_keeps_original_id() {
        // for (let i = 0; i < 3; i++) { var t = i; }
        // return t;   // t escapes the loop -> must not be refreshed
        let i = 1u32;
        let t = 2u32;
        let body = vec![Stmt::Let {
            id: t,
            name: "t".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(ivar(i)),
        }];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        let mut stmts = vec![f, Stmt::Return(Some(Expr::LocalGet(t)))];
        let mut changed = false;
        run_unroll_in_stmts(&mut stmts, &mut changed);
        assert!(changed, "expected unroll to fire");
        // 3 unrolled `Let { id: t }` + the trailing return.
        assert_eq!(stmts.len(), 4);
        for s in &stmts[0..3] {
            match s {
                Stmt::Let { id, .. } => assert_eq!(
                    *id, t,
                    "escaping var must keep its original id across copies"
                ),
                other => panic!("expected Stmt::Let, got {:?}", other),
            }
        }
        match &stmts[3] {
            Stmt::Return(Some(Expr::LocalGet(id))) => {
                assert_eq!(*id, t, "post-loop read must still bind the original id")
            }
            other => panic!("expected Return(LocalGet(t)), got {:?}", other),
        }
    }

    /// #2308 guard: a block-scoped `let` declared in the loop body and
    /// referenced ONLY inside it (here, captured by a per-iteration closure)
    /// must still get fresh ids per copy, so each closure binds a distinct
    /// value — the long-standing behavior the escaping analysis must not
    /// disturb.
    #[test]
    fn loop_local_let_still_refreshed_per_copy() {
        // for (let i = 0; i < 3; i++) { let x = i; fns.push(() => x); }
        let i = 1u32;
        let x = 2u32;
        let fns = 3u32; // declared outside the loop (not refreshed)
        let body = vec![
            Stmt::Let {
                id: x,
                name: "x".into(),
                ty: Type::Number,
                mutable: false,
                init: Some(ivar(i)),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: fns,
                value: Box::new(Expr::Closure {
                    func_id: 0,
                    params: vec![],
                    return_type: Type::Number,
                    body: vec![Stmt::Return(Some(Expr::LocalGet(x)))],
                    captures: vec![x],
                    mutable_captures: vec![],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_strict: false,
                    is_async: false,
                    is_generator: false,
                }),
            }),
        ];
        let f = make_for(i, 0, 3, body, CompareOp::Lt);
        let mut stmts = vec![f];
        let mut changed = false;
        run_unroll_in_stmts(&mut stmts, &mut changed);
        assert!(changed, "expected unroll to fire");
        assert_eq!(stmts.len(), 6, "3 trips * 2 body stmts");
        // Collect the `let x` id of each copy — they must all be DISTINCT
        // (refreshed), and the closure in each copy must capture its own id.
        let mut let_ids = Vec::new();
        for pair in stmts.chunks(2) {
            let decl_id = match &pair[0] {
                Stmt::Let { id, .. } => *id,
                other => panic!("expected Let, got {:?}", other),
            };
            match &pair[1] {
                Stmt::Expr(Expr::ArrayPush { value, .. }) => match value.as_ref() {
                    Expr::Closure { captures, .. } => {
                        assert_eq!(captures, &vec![decl_id], "closure captures its copy's x");
                    }
                    other => panic!("expected Closure, got {:?}", other),
                },
                other => panic!("expected ArrayPush, got {:?}", other),
            }
            let_ids.push(decl_id);
        }
        let_ids.sort_unstable();
        let_ids.dedup();
        assert_eq!(
            let_ids.len(),
            3,
            "each copy's `let x` must be a distinct id"
        );
    }
}
