use perry_hir::walker::walk_expr_children_mut;
use perry_hir::{Expr, Stmt};
use perry_types::LocalId;
use std::collections::HashMap;

pub fn substitute_locals(
    expr: &mut Expr,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    match expr {
        Expr::LocalGet(id) => {
            if let Some(replacement) = param_map.get(id) {
                *expr = replacement.clone();
            }
            return;
        }
        Expr::LocalSet(id, value) => {
            substitute_locals(value, param_map, next_local_id);
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::Update { id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            // Children (`value`, `start`, `delete_count`, `items`, `target`,
            // `end`, …) are descended into below via the walker.
        }
        Expr::SetAdd { set_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(set_id) {
                *set_id = *new_id;
            }
            // `value` descended via walker.
        }
        // Closure: substitute in body AND remap captures lists. Without
        // remapping captures, an inlined function whose body contains a
        // closure ends up with the closure's captures list referencing the
        // OLD local IDs while the closure body uses the NEW (remapped) IDs.
        // Codegen then can't resolve the captures in the inlined-into FnCtx
        // and falls back to `double_literal(0.0)`, producing null box
        // pointers at runtime (closure-null family). Param defaults also get
        // substituted explicitly here so the walker doesn't double-process
        // them.
        Expr::Closure {
            body,
            captures,
            mutable_captures,
            params,
            ..
        } => {
            for p in params.iter_mut() {
                if let Some(d) = &mut p.default {
                    substitute_locals(d, param_map, next_local_id);
                }
            }
            substitute_locals_in_stmts(body, param_map, next_local_id);
            captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                // Trivial expr inlined directly; closure body no longer
                // references this id, so drop the now-orphan capture.
                Some(_) => false,
                // Not in param_map → outer/module-level; leave unchanged.
                None => true,
            });
            mutable_captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                Some(_) => false,
                None => true,
            });
            return;
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions for non-special variants.
    // The walker is exhaustive on Expr — adding a new variant to ir.rs
    // without updating walker.rs is a compile error.
    walk_expr_children_mut(expr, &mut |child| {
        substitute_locals(child, param_map, next_local_id)
    });
}

/// Substitute Expr::This with a LocalGet reference
pub fn substitute_this(expr: &mut Expr, obj_id: LocalId) {
    if let Expr::This = expr {
        *expr = Expr::LocalGet(obj_id);
        return;
    }

    // Issue #291 / #350: nested closures that captured `this` from the outer
    // method's frame need their own `Expr::This` → `LocalGet(obj_id)` rewrite
    // — after inlining the closure is hoisted into the call site's frame
    // (module init for top-level calls, where `this_stack` is empty), so the
    // codegen-side fallback can't recover a meaningful `this`. Substituting
    // here lets the closure run with the correct receiver.
    //
    // Also: explicitly add `obj_id` to the closure's captures list and clear
    // `captures_this` — the body now reads `LocalGet(obj_id)` rather than
    // `Expr::This`, and `compute_auto_captures` blends explicit + body-scanned
    // ids before excluding module globals, so adding to `captures` ensures the
    // receiver is forwarded through the closure's capture array regardless of
    // where the call site lands.
    //
    // `walk_expr_children_mut` deliberately does NOT recurse into Closure
    // bodies (per its module docs); we descend into the body explicitly
    // before falling through to the walker. Param.default exprs are visited
    // by the walker.
    if let Expr::Closure {
        body,
        captures,
        captures_this,
        ..
    } = expr
    {
        substitute_this_in_stmts(body, obj_id);
        *captures_this = false;
        if !captures.contains(&obj_id) {
            captures.push(obj_id);
        }
    }

    // Descend into every immediate sub-expression. The walker is exhaustive
    // on `Expr` — adding a new variant to `ir.rs` without updating
    // `walker.rs` is a compile error. This closes the bug class (issue #350)
    // where new HIR variants like `Expr::ArrayIsArray(inner)` containing
    // nested `PropertyGet → This` chains silently fell through the previous
    // ad-hoc match and left `Expr::This` references unsubstituted in inlined
    // method bodies — same shape as the v0.5.408 fix for the closure
    // collector (issue #318).
    walk_expr_children_mut(expr, &mut |child| substitute_this(child, obj_id));
}

/// Substitute Expr::This with a LocalGet reference in statements
pub fn substitute_this_in_stmts(stmts: &mut Vec<Stmt>, obj_id: LocalId) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => {
                substitute_this(expr, obj_id);
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_this(expr, obj_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(then_branch, obj_id);
                if let Some(else_b) = else_branch {
                    substitute_this_in_stmts(else_b, obj_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(body, obj_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_this_in_stmts(&mut init_vec, obj_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_this(cond, obj_id);
                }
                if let Some(upd) = update {
                    substitute_this(upd, obj_id);
                }
                substitute_this_in_stmts(body, obj_id);
            }
            _ => {}
        }
    }
}

/// Substitute local variable references in statements
/// Collect all LocalIds defined by Let statements in a body (for remapping during inlining)
pub fn collect_body_local_ids(stmts: &[Stmt]) -> Vec<LocalId> {
    let mut ids = Vec::new();

    fn collect_from_stmt(stmt: &Stmt, ids: &mut Vec<LocalId>) {
        match stmt {
            Stmt::Let { id, .. } => {
                ids.push(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    collect_from_stmt(s, ids);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::While { body, .. } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_from_stmt(init_stmt, ids);
                }
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
                if let Some(catch_clause) = catch {
                    // Also collect the catch parameter if present
                    if let Some((param_id, _)) = &catch_clause.param {
                        ids.push(*param_id);
                    }
                    for s in &catch_clause.body {
                        collect_from_stmt(s, ids);
                    }
                }
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for s in &case.body {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            _ => {}
        }
    }

    for stmt in stmts {
        collect_from_stmt(stmt, &mut ids);
    }
    ids
}

pub fn substitute_locals_in_stmts(
    stmts: &mut Vec<Stmt>,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let { id, init, .. } => {
                // Remap the Let's id if it's in the param_map
                if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                    *id = *new_id;
                }
                if let Some(expr) = init {
                    substitute_locals(expr, param_map, next_local_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_locals(expr, param_map, next_local_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(then_branch, param_map, next_local_id);
                if let Some(else_b) = else_branch {
                    substitute_locals_in_stmts(else_b, param_map, next_local_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_locals_in_stmts(&mut init_vec, param_map, next_local_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_locals(cond, param_map, next_local_id);
                }
                if let Some(upd) = update {
                    substitute_locals(upd, param_map, next_local_id);
                }
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                // Issue #569: remap each id in the prealloc list. Inlining
                // can rename body locals so the slot+box allocation must
                // refer to the new ids. If a callee with a PreallocateBoxes
                // gets inlined into a caller, its body's hoisted FnDecls
                // still need their boxes set up.
                for id in ids.iter_mut() {
                    if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                        *id = *new_id;
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Clamp-pattern detection (Issue #436 plan #1) ──────────────────────────
//
// These detectors mirror the ones in `perry-codegen/src/collectors.rs`
// (`detect_clamp3` / `detect_clamp_u8`). Duplicated rather than shared via
// `perry-hir` because perry-transform and perry-codegen are sibling crates;
// the patterns are tiny and purely syntactic, so drift is low-risk.
//
// Matched bodies are excluded from the inlinable-functions set so the
// `Expr::Call { callee: FuncRef(clamp_fn_id) }` shape survives at every
// call site. Codegen's `lower_expr_as_i32` and the f64-context arm in
// `lower_call.rs` then emit `@llvm.smin.i32` / `@llvm.smax.i32` inline,
// producing IR that LLVM's auto-vectorizer can lift.
