use perry_hir::types::{FuncId, LocalId, Type};
use perry_hir::{Expr, Function, Param, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

pub fn stmt_contains_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(stmt_contains_return))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(stmt_contains_return)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_return)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_contains_return))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_return))
        }
        Stmt::Labeled { body, .. } => stmt_contains_return(body.as_ref()),
        // Loops: an inner Return would terminate the OUTER function so we
        // still need to convert; but the do-while(false) wrapping handles it
        // because we descend through the loop body too. Loops don't appear in
        // is_inlinable bodies (has_simple_control_flow rejects them) so this
        // is mainly defensive.
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_return)
        }
        Stmt::For { body, .. } => body.iter().any(stmt_contains_return),
        _ => false,
    }
}

/// Replace every `Stmt::Return(Some(e))` in `stmts` (recursively) with
/// `Stmt::Expr(LocalSet(let_id, e)); Stmt::Break`, and every
/// `Stmt::Return(None)` with a single `Stmt::Break`. Used to convert the body
/// of an inlined function into the body of a synthetic `do { ... } while
/// (false)` wrapper at a Let-binding call site, so the value flowing through
/// `return` ends up bound to the original `let` variable.
///
/// Does NOT descend into loop bodies or `Stmt::Labeled` (those would
/// short-circuit via the inner `break` instead of breaking out of the
/// synthetic do-while). This is fine because `is_inlinable` rejects functions
/// with loops or labeled stmts, so an inlinable callee body has no such
/// structures.
pub fn convert_returns_in_stmts(stmts: &mut Vec<Stmt>, let_id: LocalId) {
    let mut i = 0;
    while i < stmts.len() {
        match &mut stmts[i] {
            Stmt::Return(opt) => {
                let break_stmt = Stmt::Break;
                if let Some(e) = opt.take() {
                    let assign = Stmt::Expr(Expr::LocalSet(let_id, Box::new(e)));
                    stmts[i] = assign;
                    stmts.insert(i + 1, break_stmt);
                    i += 2;
                    continue;
                } else {
                    stmts[i] = break_stmt;
                    i += 1;
                    continue;
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                convert_returns_in_stmts(then_branch, let_id);
                if let Some(eb) = else_branch {
                    convert_returns_in_stmts(eb, let_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    convert_returns_in_stmts(&mut c.body, let_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                convert_returns_in_stmts(body, let_id);
                if let Some(c) = catch {
                    convert_returns_in_stmts(&mut c.body, let_id);
                }
                if let Some(f) = finally {
                    convert_returns_in_stmts(f, let_id);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Inline function and method calls in a list of statements.
///
/// `enclosing_class`, when set, names the class whose method body these stmts
/// belong to. It enables inlining of `this.someMethod()` calls — without it
/// such calls fall through to runtime dispatch because the inliner only
/// recognizes `Expr::LocalGet(obj_id)` as a method receiver. The Phase 6
/// driver passes the class name; Phases 4 (init) and 5 (top-level functions)
/// pass `None`.
/// Exact-receiver facts that stay valid on *every* iteration of a loop body:
/// the subset of `outer` whose receiver local is never reassigned anywhere in
/// the loop (`body` plus any condition/update exprs in `extra_exprs`). A
/// receiver reassigned mid-loop could hold a different (sub)class on a later
/// iteration, so its fact is dropped — keeping direct method inlining sound
/// while still inlining calls on loop-invariant receivers declared *before* the
/// loop (e.g. `const c = new Counter(); for (…) c.increment();`). Before this,
/// loop bodies were seeded with empty facts, so such calls were never inlined.
///
/// `collect_mutated_local_ids` recurses into closures and nested loops and
/// catches every `LocalSet`/`Update`, so "not mutated in the loop" is a sound
/// (conservative) proxy for "fact holds on every iteration".
fn loop_invariant_seed_facts(
    outer: &ExactReceiverFacts,
    body: &[Stmt],
    extra_exprs: &[&Expr],
) -> ExactReceiverFacts {
    if outer.is_empty() {
        return ExactReceiverFacts::new();
    }
    let mut mutated = std::collections::HashSet::new();
    collect_mutated_local_ids(body, &mut mutated);
    for e in extra_exprs {
        collect_mutated_local_ids(
            std::slice::from_ref(&Stmt::Expr((*e).clone())),
            &mut mutated,
        );
    }
    outer
        .iter()
        .filter(|(id, _)| !mutated.contains(*id))
        .map(|(id, f)| (*id, f.clone()))
        .collect()
}

pub fn inline_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    class_names: &HashMap<String, String>,
    local_types: &mut HashMap<LocalId, String>,
    exact_receiver_facts: &mut ExactReceiverFacts,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) {
    // Same cap as `inline_calls_in_expr`, sharing one thread-local depth
    // budget: the two functions are mutually recursive, and a stmts-side
    // chain (nested blocks, closure bodies, re-walks of freshly inlined
    // bodies) that never passes through the expr-side guard would otherwise
    // recurse unbounded — half of #6593. Bailing out skips further inlining
    // in this subtree, which is semantics-preserving (the inliner is an
    // optimization pass); clear `exact_receiver_facts` because we no longer
    // track what the skipped statements may reference or mutate.
    let Some(_recursion_guard) = enter_inline_expr_recursion() else {
        exact_receiver_facts.clear();
        return;
    };
    let mut i = 0;
    while i < stmts.len() {
        // Track local variable types from Let statements
        if let Stmt::Let { id, ty, init, .. } = &stmts[i] {
            if let Type::Named(class_name) = ty {
                local_types.insert(*id, class_name.clone());
            }
            // Also check if init is a New expression
            if let Some(Expr::New { class_name, .. }) = init {
                local_types.insert(*id, class_name.clone());
            }
        }

        let mut new_stmts: Option<Vec<Stmt>> = None;
        let mut exact_effect_handled = false;

        match &mut stmts[i] {
            Stmt::Expr(expr) => {
                if let Some((mut inlined_stmts, _result_expr)) = try_inline_call(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ) {
                    // When inlining into Stmt::Expr context (result discarded),
                    // convert Stmt::Return(Some(expr)) to Stmt::Expr(expr) and
                    // remove Stmt::Return(None). This prevents emitting a
                    // `ret` terminator mid-block (e.g., inside a for loop body).
                    // Only do this if returns are in safe positions (trailing).
                    let has_nested_return = inlined_stmts
                        .iter()
                        .take(inlined_stmts.len().saturating_sub(1))
                        .any(|s| {
                            fn stmt_has_return(s: &Stmt) -> bool {
                                match s {
                                    Stmt::Return(_) => true,
                                    Stmt::If {
                                        then_branch,
                                        else_branch,
                                        ..
                                    } => {
                                        then_branch.iter().any(stmt_has_return)
                                            || else_branch
                                                .as_ref()
                                                .is_some_and(|eb| eb.iter().any(stmt_has_return))
                                    }
                                    _ => false,
                                }
                            }
                            stmt_has_return(s)
                        });
                    if has_nested_return {
                        // Can't safely convert early returns; skip inlining
                        let hoisted = inline_calls_in_expr(
                            expr,
                            func_candidates,
                            method_candidates,
                            local_types,
                            exact_receiver_facts,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        );
                        if !hoisted.is_empty() {
                            new_stmts = Some(hoisted);
                        }
                    } else {
                        // Convert trailing return to expression (discard result)
                        if let Some(last) = inlined_stmts.last_mut() {
                            match last {
                                Stmt::Return(Some(ret_expr)) => {
                                    let e = std::mem::replace(ret_expr, Expr::Undefined);
                                    *last = Stmt::Expr(e);
                                }
                                Stmt::Return(None) => {
                                    inlined_stmts.pop();
                                }
                                _ => {}
                            }
                        }
                        new_stmts = Some(inlined_stmts);
                    }
                } else {
                    let hoisted = inline_calls_in_expr(
                        expr,
                        func_candidates,
                        method_candidates,
                        local_types,
                        exact_receiver_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                    if !hoisted.is_empty() {
                        // Hoisted stmts from multi-stmt inlining inside expressions
                        // (e.g., `h = imul32(h, p)` → Let setup stmts + modified expr)
                        // Splice them before the current statement, keeping the stmt itself.
                        let current = stmts.remove(i);
                        let hoisted_len = hoisted.len();
                        for (j, s) in hoisted.into_iter().enumerate() {
                            stmts.insert(i + j, s);
                        }
                        stmts.insert(i + hoisted_len, current);
                        i += hoisted_len + 1;
                        continue;
                    }
                }
            }
            Stmt::Let { init: Some(_), .. } => {
                // First try inlining a top-level Call as the Let's init.
                // Pattern:    `let r = f(args)`  or  `let r = this.m(args)`
                // becomes:    `let r = undefined; do { /* inlined body, with
                //               every Return(Some(e)) replaced by
                //               Expr(LocalSet(r, e)); Break */ } while (false)`
                // The wrapper is needed because the inlined body may have early
                // returns inside `if` branches; converting them all uniformly to
                // LocalSet+Break preserves semantics. After this rewrite the
                // returned object literal (e.g. `{entityId, componentType,
                // component}` from `World.resolveSetOperation`) lives in the
                // caller's stmt list as a non-escaping `Let r = undefined;
                // LocalSet r = { ... }`, where the existing
                // `non_escaping_object_literals` collector can then scalar-
                // replace it during codegen.
                //
                // For the simple trailing-only case (no nested returns), we
                // collapse the wrapper: the inlined setup stmts run inline,
                // then the trailing `Return(Some(e))` becomes the original
                // `Let { id: let_id, init: Some(e) }`.
                let (let_id, let_name, let_ty, let_mutable) = match &stmts[i] {
                    Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        ..
                    } => (*id, name.clone(), ty.clone(), *mutable),
                    _ => unreachable!(),
                };
                let init_expr = match &stmts[i] {
                    Stmt::Let { init: Some(e), .. } => e.clone(),
                    _ => unreachable!(),
                };
                let mut handled = false;
                if matches!(&init_expr, Expr::Call { .. }) {
                    if let Some((mut inlined_stmts, _)) = try_inline_call(
                        &init_expr,
                        func_candidates,
                        method_candidates,
                        local_types,
                        exact_receiver_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    ) {
                        let has_nested_return = inlined_stmts
                            .iter()
                            .take(inlined_stmts.len().saturating_sub(1))
                            .any(stmt_contains_return);
                        let trailing_is_return =
                            matches!(inlined_stmts.last(), Some(Stmt::Return(Some(_))));
                        if !has_nested_return && trailing_is_return {
                            // Collapse: convert the trailing Return into the
                            // original Let-binding.
                            if let Some(last) = inlined_stmts.last_mut() {
                                if let Stmt::Return(Some(ret_expr)) = last {
                                    let e = std::mem::replace(ret_expr, Expr::Undefined);
                                    *last = Stmt::Let {
                                        id: let_id,
                                        name: let_name.clone(),
                                        ty: let_ty.clone(),
                                        mutable: let_mutable,
                                        init: Some(e),
                                    };
                                }
                            }
                            new_stmts = Some(inlined_stmts);
                            handled = true;
                        } else if inlined_stmts.iter().any(stmt_contains_return) {
                            // Nested-return case: wrap in `do { ... } while (false)`,
                            // converting every Return(Some(e)) to LocalSet+Break and
                            // every Return(None) to Break. The original Let becomes
                            // a mutable seed initialized to undefined; the wrapper
                            // body then writes the result via LocalSet.
                            convert_returns_in_stmts(&mut inlined_stmts, let_id);
                            let mut wrapped: Vec<Stmt> = Vec::with_capacity(2);
                            wrapped.push(Stmt::Let {
                                id: let_id,
                                name: let_name.clone(),
                                ty: let_ty.clone(),
                                // Force mutable — even though the source was a
                                // const, we now write to it via LocalSet.
                                mutable: true,
                                init: Some(Expr::Undefined),
                            });
                            wrapped.push(Stmt::DoWhile {
                                body: inlined_stmts,
                                condition: Expr::Bool(false),
                            });
                            new_stmts = Some(wrapped);
                            handled = true;
                        }
                    }
                }
                if !handled {
                    // Fall back to nested-arg hoisting (existing behavior).
                    let hoisted = match &mut stmts[i] {
                        Stmt::Let {
                            init: Some(expr), ..
                        } => inline_calls_in_expr(
                            expr,
                            func_candidates,
                            method_candidates,
                            local_types,
                            exact_receiver_facts,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ),
                        _ => Vec::new(),
                    };
                    if !hoisted.is_empty() {
                        let current = stmts.remove(i);
                        let hoisted_len = hoisted.len();
                        for (j, s) in hoisted.into_iter().enumerate() {
                            stmts.insert(i + j, s);
                        }
                        stmts.insert(i + hoisted_len, current);
                        i += hoisted_len + 1;
                        continue;
                    }
                }
            }
            Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                let hoisted = inline_calls_in_expr(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if !hoisted.is_empty() {
                    let current = stmts.remove(i);
                    let hoisted_len = hoisted.len();
                    for (j, s) in hoisted.into_iter().enumerate() {
                        stmts.insert(i + j, s);
                    }
                    stmts.insert(i + hoisted_len, current);
                    i += hoisted_len + 1;
                    continue;
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut condition_candidate = condition.clone();
                let mut condition_facts = exact_receiver_facts.clone();
                let hoisted = inline_calls_in_expr(
                    &mut condition_candidate,
                    func_candidates,
                    method_candidates,
                    local_types,
                    &mut condition_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if hoisted.is_empty() {
                    *condition = condition_candidate;
                    *exact_receiver_facts = condition_facts;
                }
                invalidate_exact_receivers_for_expr(condition, exact_receiver_facts);
                let after_condition_facts = exact_receiver_facts.clone();
                let mut then_facts = after_condition_facts.clone();
                inline_calls_in_stmts(
                    then_branch,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    &mut then_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                let mut else_facts = after_condition_facts;
                if let Some(else_b) = else_branch {
                    inline_calls_in_stmts(
                        else_b,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        &mut else_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                }
                *exact_receiver_facts = intersect_exact_receiver_facts(&then_facts, &else_facts);
                exact_effect_handled = true;
            }
            Stmt::While { condition, body } => {
                let mut empty_facts = ExactReceiverFacts::new();
                let mut condition_candidate = condition.clone();
                let hoisted = inline_calls_in_expr(
                    &mut condition_candidate,
                    func_candidates,
                    method_candidates,
                    local_types,
                    &mut empty_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if hoisted.is_empty() {
                    *condition = condition_candidate;
                }
                let mut body_facts =
                    loop_invariant_seed_facts(exact_receiver_facts, body, &[&*condition]);
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    &mut body_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                exact_receiver_facts.clear();
                exact_effect_handled = true;
            }
            Stmt::DoWhile { body, condition } => {
                let mut body_facts =
                    loop_invariant_seed_facts(exact_receiver_facts, body, &[&*condition]);
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    &mut body_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                let mut empty_facts = ExactReceiverFacts::new();
                let mut condition_candidate = condition.clone();
                let hoisted = inline_calls_in_expr(
                    &mut condition_candidate,
                    func_candidates,
                    method_candidates,
                    local_types,
                    &mut empty_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if hoisted.is_empty() {
                    *condition = condition_candidate;
                }
                exact_receiver_facts.clear();
                exact_effect_handled = true;
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_stmts = vec![*init_stmt.clone()];
                    let mut init_facts = ExactReceiverFacts::new();
                    inline_calls_in_stmts(
                        &mut init_stmts,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        &mut init_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                    if init_stmts.len() == 1 {
                        **init_stmt = init_stmts.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    let mut empty_facts = ExactReceiverFacts::new();
                    let mut condition_candidate = cond.clone();
                    let hoisted = inline_calls_in_expr(
                        &mut condition_candidate,
                        func_candidates,
                        method_candidates,
                        local_types,
                        &mut empty_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                    if hoisted.is_empty() {
                        *cond = condition_candidate;
                    }
                }
                if let Some(upd) = update {
                    let mut empty_facts = ExactReceiverFacts::new();
                    let _hoisted = inline_calls_in_expr(
                        upd,
                        func_candidates,
                        method_candidates,
                        local_types,
                        &mut empty_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    );
                }
                let mut for_extra: Vec<&Expr> = Vec::new();
                if let Some(c) = condition.as_ref() {
                    for_extra.push(c);
                }
                if let Some(u) = update.as_ref() {
                    for_extra.push(u);
                }
                let mut body_facts =
                    loop_invariant_seed_facts(exact_receiver_facts, body, &for_extra);
                // The for-init can also rebind a receiver local (e.g.
                // `for (c = makeOther(); …)`), so drop facts it mutates too —
                // otherwise a stale pre-loop class could survive into the body
                // and allow an unsound inline. (`init` is a Stmt, not an Expr,
                // so it can't go through `for_extra`.)
                if let Some(init_stmt) = init.as_ref() {
                    let mut init_mutated: std::collections::HashSet<LocalId> =
                        std::collections::HashSet::new();
                    collect_mutated_local_ids(
                        std::slice::from_ref(init_stmt.as_ref()),
                        &mut init_mutated,
                    );
                    body_facts.retain(|id, _| !init_mutated.contains(id));
                }
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    &mut body_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                exact_receiver_facts.clear();
                exact_effect_handled = true;
            }
            _ => {}
        }

        if let Some(mut inlined) = new_stmts {
            // Recursively inline calls within the just-inlined block before
            // splicing it back. Without this, the body of an inlined method
            // (e.g. `World.set` once it's expanded inline) would itself contain
            // un-inlined calls — like `this.resolveSetOperation(...)` — that
            // get skipped because the outer iterator advances past their
            // positions. Doing the inner pass first means subsequent layers of
            // small-function calls collapse cleanly: `world.set(e, C, v)` →
            // `this.resolveSetOperation(...) + this.commandBuffer.set(...)` →
            // scalar-replaced `{entityId, componentType, component}` literal.
            //
            // Termination relies on `is_inlinable` rejecting recursive
            // functions in practice (the static-call analysis here only
            // matches when both the receiver class and method are statically
            // known, so cyclic call chains either don't form or get filtered
            // out by other criteria). Phase 6's "skip if itself a candidate"
            // gate (line ~190) already prevents the most direct case.
            inline_calls_in_stmts(
                &mut inlined,
                func_candidates,
                method_candidates,
                class_names,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            );
            stmts.remove(i);
            let inlined_len = inlined.len();
            for (j, stmt) in inlined.drain(..).enumerate() {
                stmts.insert(i + j, stmt);
            }
            i += inlined_len.max(1);
        } else {
            if !exact_effect_handled {
                apply_exact_receiver_stmt_effect(&stmts[i], exact_receiver_facts);
            }
            i += 1;
        }
    }
}

/// Inline function and method calls in an expression.
/// Returns setup statements that must be spliced before the enclosing statement.
pub fn inline_calls_in_expr(
    expr: &mut Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    exact_receiver_facts: &mut ExactReceiverFacts,
    next_local_id: &mut LocalId,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Vec<Stmt> {
    let Some(_recursion_guard) = enter_inline_expr_recursion() else {
        // The inliner is an optimization pass. Very deeply nested generated
        // expression/closure trees can exceed Perry's compiler stack if we
        // chase every child recursively; skipping deeper inlining is
        // semantics-preserving. Clear exact receiver facts because we are no
        // longer proving what the skipped expression may reference or mutate.
        exact_receiver_facts.clear();
        return Vec::new();
    };

    // First try to inline this expression if it's a call
    if let Some((stmts, mut result)) = try_inline_simple_call(
        expr,
        func_candidates,
        method_candidates,
        local_types,
        exact_receiver_facts,
        next_local_id,
        enclosing_class,
        class_field_types,
    ) {
        apply_exact_receiver_stmt_effects(&stmts, exact_receiver_facts);
        let inner = inline_calls_in_expr(
            &mut result,
            func_candidates,
            method_candidates,
            local_types,
            exact_receiver_facts,
            next_local_id,
            enclosing_class,
            class_field_types,
        );
        *expr = result;
        let mut all = stmts;
        all.extend(inner);
        return all;
    }

    // Otherwise recurse into sub-expressions, collecting hoisted stmts
    let mut hoisted = Vec::new();
    match expr {
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            hoisted.extend(inline_calls_in_expr(
                left,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                right,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Logical { left, right, .. } => {
            hoisted.extend(inline_calls_in_expr(
                left,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));

            let after_left_facts = exact_receiver_facts.clone();
            let mut right_candidate = (**right).clone();
            let mut right_facts = after_left_facts.clone();
            let right_hoisted = inline_calls_in_expr(
                &mut right_candidate,
                func_candidates,
                method_candidates,
                local_types,
                &mut right_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            );
            if right_hoisted.is_empty() {
                **right = right_candidate;
            } else {
                right_facts = after_left_facts.clone();
                invalidate_exact_receivers_for_expr(right.as_ref(), &mut right_facts);
            }
            *exact_receiver_facts = intersect_exact_receiver_facts(&after_left_facts, &right_facts);
            return hoisted;
        }
        Expr::Unary { operand, .. } => {
            hoisted.extend(inline_calls_in_expr(
                operand,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            hoisted.extend(inline_calls_in_expr(
                condition,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));

            let after_condition_facts = exact_receiver_facts.clone();

            let mut then_candidate = (**then_expr).clone();
            let mut then_facts = after_condition_facts.clone();
            let then_hoisted = inline_calls_in_expr(
                &mut then_candidate,
                func_candidates,
                method_candidates,
                local_types,
                &mut then_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            );
            if then_hoisted.is_empty() {
                **then_expr = then_candidate;
            } else {
                then_facts = after_condition_facts.clone();
                invalidate_exact_receivers_for_expr(then_expr.as_ref(), &mut then_facts);
            }

            let mut else_candidate = (**else_expr).clone();
            let mut else_facts = after_condition_facts.clone();
            let else_hoisted = inline_calls_in_expr(
                &mut else_candidate,
                func_candidates,
                method_candidates,
                local_types,
                &mut else_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            );
            if else_hoisted.is_empty() {
                **else_expr = else_candidate;
            } else {
                else_facts = after_condition_facts;
                invalidate_exact_receivers_for_expr(else_expr.as_ref(), &mut else_facts);
            }

            *exact_receiver_facts = intersect_exact_receiver_facts(&then_facts, &else_facts);
            return hoisted;
        }
        Expr::Call { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            for arg in args.iter_mut() {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
            exact_receiver_facts.clear();
        }
        Expr::Array(elements) => {
            for elem in elements {
                hoisted.extend(inline_calls_in_expr(
                    elem,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
                kill_referenced_exact_receivers(elem, exact_receiver_facts);
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
                kill_referenced_exact_receivers(v, exact_receiver_facts);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
                kill_referenced_exact_receivers(v, exact_receiver_facts);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            exact_receiver_facts,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ));
                        kill_referenced_exact_receivers(e, exact_receiver_facts);
                    }
                    perry_hir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            for arg in args.iter_mut() {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            exact_receiver_facts,
                            next_local_id,
                            enclosing_class,
                            class_field_types,
                        ));
                    }
                }
            }
            exact_receiver_facts.clear();
        }
        Expr::IndexGet { object, index } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            exact_receiver_facts.clear();
        }
        Expr::PropertyGet { object, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::PropertySet { object, value, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            exact_receiver_facts.clear();
        }
        Expr::LocalSet(id, value) => {
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            exact_receiver_facts.remove(id);
            kill_referenced_exact_receivers(value.as_ref(), exact_receiver_facts);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                hoisted.extend(inline_calls_in_expr(
                    obj,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
            for arg in args.iter_mut() {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    exact_receiver_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                ));
            }
            exact_receiver_facts.clear();
        }
        // Issue #169: a Call nested inside a Uint8Array index/set/length
        // (e.g. `buf[clamp(i)]`) wouldn't be inlined without these arms.
        Expr::Uint8ArrayGet { array, index } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
            kill_referenced_exact_receivers(array.as_ref(), exact_receiver_facts);
            kill_referenced_exact_receivers(index.as_ref(), exact_receiver_facts);
            kill_referenced_exact_receivers(value.as_ref(), exact_receiver_facts);
        }
        Expr::Uint8ArrayLength(arr) => {
            hoisted.extend(inline_calls_in_expr(
                arr,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Uint8ArrayNew(Some(arg)) => {
            hoisted.extend(inline_calls_in_expr(
                arg,
                func_candidates,
                method_candidates,
                local_types,
                exact_receiver_facts,
                next_local_id,
                enclosing_class,
                class_field_types,
            ));
        }
        Expr::Sequence(exprs) => {
            // A comma-sequence evaluates its elements left-to-right; element
            // `i>0` runs only AFTER the side effects of elements `0..i`. Inline
            // setup statements (the `let <param> = <arg>` arg-bindings) bubble
            // up to *before the enclosing statement*, so hoisting a later
            // element's setup would move its arg reads ahead of the earlier
            // stores those reads depend on. For an esbuild `__esm` schema
            // factory — one big comma-sequence of `Global = ctor({...})`
            // assignments where a later object literal reads an
            // earlier-assigned schema var (`C31 = KP({ k: YE7.optional() })`) —
            // that reorders the read of `YE7` ahead of its store, yielding
            // `undefined` and a `Cannot read properties of undefined` throw.
            //
            // Only the first element is evaluated before any sibling side
            // effect, so only its setup may safely hoist. For later elements,
            // inline into a candidate clone and commit only when it needs no
            // hoisted setup (a pure substitution stays in place); otherwise
            // leave the element as its original runtime call — always correct,
            // just un-inlined. Mirrors the clone-and-revert guard the
            // short-circuit `Logical` / `Conditional` arms already use.
            for (idx, item) in exprs.iter_mut().enumerate() {
                if idx == 0 {
                    hoisted.extend(inline_calls_in_expr(
                        item,
                        func_candidates,
                        method_candidates,
                        local_types,
                        exact_receiver_facts,
                        next_local_id,
                        enclosing_class,
                        class_field_types,
                    ));
                    continue;
                }
                let before_facts = exact_receiver_facts.clone();
                let mut candidate = item.clone();
                let mut candidate_facts = before_facts.clone();
                let item_hoisted = inline_calls_in_expr(
                    &mut candidate,
                    func_candidates,
                    method_candidates,
                    local_types,
                    &mut candidate_facts,
                    next_local_id,
                    enclosing_class,
                    class_field_types,
                );
                if item_hoisted.is_empty() {
                    *item = candidate;
                    *exact_receiver_facts = candidate_facts;
                } else {
                    *exact_receiver_facts = before_facts;
                    invalidate_exact_receivers_for_expr(item, exact_receiver_facts);
                }
            }
        }
        // Descend into closure bodies. Without this, the inliner never
        // visits the body of an arrow/`function(){}` literal — which
        // is a significant gap because test fixtures wrap their entire
        // workload in `describe(() => it(() => { ... }))` callbacks, and
        // the test loop's hot calls (e.g. `world.set(...)`) live exclusively
        // inside those nested closures. Use the closure's HIR-recorded
        // `enclosing_class` (Some(class) iff the closure captures `this`
        // from a class method) so calls of the form `this.method(...)`
        // inside an arrow inside a class method still resolve correctly.
        // Default-param expressions are NOT descended into here — they
        // execute in the closure's context but are evaluated at the call
        // site of the closure, where param types may differ.
        Expr::Closure {
            body,
            params,
            captures,
            mutable_captures,
            enclosing_class: closure_enclosing,
            ..
        } => {
            // Seed local_types entries for each param with a Named class
            // type. Exact receiver facts intentionally do not flow into
            // closures: the closure may run later, after aliases or own
            // property writes have changed dispatch.
            let mut closure_local_types = local_types.clone();
            let mut closure_exact_receiver_facts = ExactReceiverFacts::new();
            for p in params.iter() {
                if let Type::Named(class_name) = &p.ty {
                    closure_local_types.insert(p.id, class_name.clone());
                }
            }
            // Hoist any setup stmts produced by inlining inside the body
            // up to the call-site context. For closures these typically
            // would be empty, but stay defensive.
            inline_calls_in_stmts(
                body,
                func_candidates,
                method_candidates,
                &HashMap::new(),
                &mut closure_local_types,
                &mut closure_exact_receiver_facts,
                next_local_id,
                closure_enclosing.as_deref(),
                class_field_types,
            );
            for id in captures.iter().chain(mutable_captures.iter()) {
                exact_receiver_facts.remove(id);
            }
            exact_receiver_facts.clear();
            for param in params {
                if let Some(default) = &param.default {
                    invalidate_exact_receivers_for_expr(default, exact_receiver_facts);
                }
            }
        }
        _ => {
            invalidate_exact_receivers_for_expr(expr, exact_receiver_facts);
        }
    }
    hoisted
}

pub fn build_inline_arg_bindings(
    params: &[Param],
    args: &[Expr],
    closure_captures: &HashSet<LocalId>,
    mutated_params: &HashSet<LocalId>,
    next_local_id: &mut LocalId,
) -> Option<(Vec<Stmt>, HashMap<LocalId, Expr>)> {
    if params.iter().any(|param| param.is_rest) {
        return None;
    }

    let mut setup = Vec::new();
    let mut param_map = HashMap::new();

    for (index, param) in params.iter().enumerate() {
        if let Some(arg) = args.get(index) {
            let trivial_in_closure = is_trivial_expr(arg)
                && !matches!(arg, Expr::LocalGet(_))
                && closure_captures.contains(&param.id);
            // A param the body WRITES must get its own copy: substituting
            // the caller's LocalGet would alias the write onto the caller's
            // local (`function f(a){a++}; f(x)` mutated x — S13.2.1_A6),
            // and substituting a literal would produce `5++`.
            let force_let = mutated_params.contains(&param.id);
            if is_trivial_expr(arg) && !trivial_in_closure && !force_let {
                param_map.insert(param.id, arg.clone());
            } else {
                let fresh = *next_local_id;
                *next_local_id += 1;
                setup.push(Stmt::Let {
                    id: fresh,
                    name: param.name.clone(),
                    ty: param.ty.clone(),
                    mutable: force_let,
                    init: Some(arg.clone()),
                });
                param_map.insert(param.id, Expr::LocalGet(fresh));
            }
        } else {
            let fresh = *next_local_id;
            *next_local_id += 1;
            setup.push(Stmt::Let {
                id: fresh,
                name: param.name.clone(),
                ty: param.ty.clone(),
                mutable: true,
                init: Some(Expr::Undefined),
            });
            param_map.insert(param.id, Expr::LocalGet(fresh));
        }
    }

    for arg in args.iter().skip(params.len()) {
        setup.push(Stmt::Expr(arg.clone()));
    }

    Some((setup, param_map))
}

/// Try to inline a simple function or method call.
/// Handles two patterns:
/// 1. Single `Return(expr)` body — classic expression-level inline
/// 2. `[Let*, Return(expr)]` body — setup stmts + result expression
pub fn try_inline_simple_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    _local_types: &HashMap<LocalId, String>,
    exact_receiver_facts: &ExactReceiverFacts,
    next_local_id: &mut LocalId,
    _enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(Vec<Stmt>, Expr)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Check for regular function call
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                // Pattern 1: single Return(expr)
                if func.body.len() == 1 {
                    if let Stmt::Return(Some(return_expr)) = &func.body[0] {
                        // Issue #858: params that are captured by some
                        // closure inside the body MUST be materialized as a
                        // fresh `Let` rather than substituted in place with
                        // the (possibly-literal) call argument. Otherwise
                        // the literal silently rewrites the closure body's
                        // `LocalGet(param) -> Integer(N)`, the captures list
                        // empties out at the call site, and the compiled
                        // closure body (which still reads slot 0 from the
                        // original `func_id`-keyed occurrence) reads
                        // garbage / 0. See `collect_closure_captured_local_ids`.
                        let mut closure_capt: std::collections::HashSet<LocalId> =
                            std::collections::HashSet::new();
                        collect_closure_captured_local_ids(&func.body, &mut closure_capt);

                        let mut mutated: std::collections::HashSet<LocalId> =
                            std::collections::HashSet::new();
                        collect_mutated_local_ids(&func.body, &mut mutated);
                        let (setup_stmts, param_map) = build_inline_arg_bindings(
                            &func.params,
                            args,
                            &closure_capt,
                            &mutated,
                            next_local_id,
                        )?;
                        let mut result = return_expr.clone();
                        substitute_locals(&mut result, &param_map, next_local_id);
                        return Some((setup_stmts, result));
                    }
                }

                // Pattern 2: [Let (const)*, Return(expr)] — e.g. imul32 polyfill
                // All statements except the last must be immutable Let declarations,
                // and the last must be Return(Some(expr)).
                if func.body.len() > 1 {
                    let last = func.body.last().unwrap();
                    if let Stmt::Return(Some(return_expr)) = last {
                        let all_lets = func.body[..func.body.len() - 1].iter().all(|s| {
                            matches!(
                                s,
                                Stmt::Let {
                                    mutable: false,
                                    init: Some(_),
                                    ..
                                }
                            )
                        });
                        if all_lets {
                            // Issue #858: params closure-captured anywhere in
                            // the body MUST be materialized as a Let even if
                            // the arg is a literal (Integer/Number/...).
                            // Substituting a literal in place inside a
                            // closure body silently empties the closure's
                            // captures list, breaking the func_id <->
                            // capture-shape contract codegen relies on.
                            let mut closure_capt: std::collections::HashSet<LocalId> =
                                std::collections::HashSet::new();
                            collect_closure_captured_local_ids(&func.body, &mut closure_capt);

                            let mut mutated: std::collections::HashSet<LocalId> =
                                std::collections::HashSet::new();
                            collect_mutated_local_ids(&func.body, &mut mutated);
                            let (mut setup, mut param_map) = build_inline_arg_bindings(
                                &func.params,
                                args,
                                &closure_capt,
                                &mutated,
                                next_local_id,
                            )?;

                            // Remap body-local IDs
                            let body_ids = collect_body_local_ids(&func.body);
                            for old_id in &body_ids {
                                if !param_map.contains_key(old_id) {
                                    let fresh = *next_local_id;
                                    *next_local_id += 1;
                                    param_map.insert(*old_id, Expr::LocalGet(fresh));
                                }
                            }

                            // Then clone the body Let stmts with substituted inits
                            for stmt in &func.body[..func.body.len() - 1] {
                                if let Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable,
                                    init: Some(init_expr),
                                } = stmt
                                {
                                    let new_id =
                                        if let Some(Expr::LocalGet(fresh)) = param_map.get(id) {
                                            *fresh
                                        } else {
                                            *id
                                        };
                                    let mut new_init = init_expr.clone();
                                    substitute_locals(&mut new_init, &param_map, next_local_id);
                                    setup.push(Stmt::Let {
                                        id: new_id,
                                        name: name.clone(),
                                        ty: ty.clone(),
                                        mutable: *mutable,
                                        init: Some(new_init),
                                    });
                                }
                            }

                            // Build result expression from the Return
                            let mut result = return_expr.clone();
                            substitute_locals(&mut result, &param_map, next_local_id);

                            return Some((setup, result));
                        }
                    }
                }
            }
        }

        // Check for method call: callee is PropertyGet { object: LocalGet(id), property: method_name }
        if let Expr::PropertyGet {
            object,
            property: method_name,
            ..
        } = callee.as_ref()
        {
            // Method inlining requires an exact `let obj = new C(...)`
            // receiver proof. Declared/parameter types are not enough:
            // virtual dispatch can pick a subclass override at runtime, and
            // own-property writes can shadow the prototype method.
            let receiver: Option<(String, Option<LocalId>)> = match object.as_ref() {
                Expr::LocalGet(obj_id) => exact_receiver_facts
                    .get(obj_id)
                    .map(|fact| (fact.class_name.clone(), Some(*obj_id))),
                _ => None,
            };
            // Silence unused-warning for the resolver helper when this
            // simple path doesn't reach it. The richer path below uses it.
            let _ = class_field_types;
            if let Some((class_name, obj_id_opt)) = receiver {
                // Look up the method candidate
                if let Some(method_candidate) =
                    method_candidates.get(&(class_name, method_name.clone()))
                {
                    // Preserve normal `obj.method` lookup and argument
                    // evaluation. Direct substitution would bypass
                    // own-property/accessor shadows and zip() would drop
                    // extra actual args plus their side effects.
                    if !method_candidate.method_lookup_safe {
                        return None;
                    }
                    if method_contains_lexical_super(&method_candidate.func) {
                        return None;
                    }

                    // Check for single return statement
                    if method_candidate.func.body.len() == 1 {
                        if let Stmt::Return(Some(return_expr)) = &method_candidate.func.body[0] {
                            // Issue #858: see fn-call branch above. Same
                            // closure-captured-param materialization rule
                            // applies to method-call inlining.
                            let mut closure_capt: std::collections::HashSet<LocalId> =
                                std::collections::HashSet::new();
                            collect_closure_captured_local_ids(
                                &method_candidate.func.body,
                                &mut closure_capt,
                            );

                            let mut mutated: std::collections::HashSet<LocalId> =
                                std::collections::HashSet::new();
                            collect_mutated_local_ids(&method_candidate.func.body, &mut mutated);
                            let (setup_stmts, mut param_map) = build_inline_arg_bindings(
                                &method_candidate.func.params,
                                args,
                                &closure_capt,
                                &mutated,
                                next_local_id,
                            )?;

                            // Map 'this' parameter to the receiver object (only
                            // when the receiver is a LocalGet — for an
                            // `Expr::This` receiver the body's `Expr::This`
                            // already references the same `this` as the
                            // caller, so we leave it alone).
                            if let (Some(this_id), Some(obj_id)) =
                                (method_candidate.this_param_id, obj_id_opt)
                            {
                                param_map.insert(this_id, Expr::LocalGet(obj_id));
                            }

                            let mut result = return_expr.clone();
                            substitute_locals(&mut result, &param_map, next_local_id);

                            // Also substitute Expr::This with the receiver
                            // (only when the receiver was a LocalGet).
                            if let Some(obj_id) = obj_id_opt {
                                substitute_this(&mut result, obj_id);
                            }

                            // Caller only consumes `(setup, result)` —
                            // returning a non-empty setup is fine; the
                            // setup_stmts are hoisted before the call site.
                            if setup_stmts.is_empty() {
                                return Some((vec![], result));
                            }
                            return Some((setup_stmts, result));
                        }
                    }

                    // Handle void methods (no return or empty return)
                    if method_candidate.func.body.len() <= 2 {
                        let mut is_void_method = true;
                        let mut inlined_stmts = Vec::new();

                        // Issue #858: same closure-captured-param rule. We
                        // build any required setup Lets once for the whole
                        // void-method body (all Expr stmts share the same
                        // param substitution).
                        let mut closure_capt: std::collections::HashSet<LocalId> =
                            std::collections::HashSet::new();
                        collect_closure_captured_local_ids(
                            &method_candidate.func.body,
                            &mut closure_capt,
                        );
                        let mut mutated: std::collections::HashSet<LocalId> =
                            std::collections::HashSet::new();
                        collect_mutated_local_ids(&method_candidate.func.body, &mut mutated);
                        let (setup_for_params, mut shared_param_map) = build_inline_arg_bindings(
                            &method_candidate.func.params,
                            args,
                            &closure_capt,
                            &mutated,
                            next_local_id,
                        )?;
                        if let (Some(this_id), Some(obj_id)) =
                            (method_candidate.this_param_id, obj_id_opt)
                        {
                            shared_param_map.insert(this_id, Expr::LocalGet(obj_id));
                        }

                        for stmt in &method_candidate.func.body {
                            match stmt {
                                Stmt::Return(None) => {}
                                Stmt::Expr(e) => {
                                    let mut expr = e.clone();
                                    substitute_locals(&mut expr, &shared_param_map, next_local_id);
                                    if let Some(obj_id) = obj_id_opt {
                                        substitute_this(&mut expr, obj_id);
                                    }
                                    inlined_stmts.push(Stmt::Expr(expr));
                                }
                                _ => {
                                    is_void_method = false;
                                    break;
                                }
                            }
                        }

                        if is_void_method && !inlined_stmts.is_empty() {
                            let mut all = setup_for_params;
                            all.extend(inlined_stmts);
                            return Some((all, Expr::Undefined));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Try to inline a call that may have multiple statements
pub fn try_inline_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    _local_types: &HashMap<LocalId, String>,
    exact_receiver_facts: &ExactReceiverFacts,
    next_local_id: &mut LocalId,
    _enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(Vec<Stmt>, Option<Expr>)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Handle regular function calls
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                // Extra actual args are evaluated before a JS call even when
                // the callee declares fewer params. The current inliner maps
                // params with zip(), so it cannot preserve those trailing
                // side effects. Keep the call intact instead.
                if args.len() > func.params.len() {
                    return None;
                }

                let mut setup_stmts: Vec<Stmt> = Vec::new();
                let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                // Issue #858: see fn-call branch in try_inline_simple_call.
                // Params closure-captured by the inlined body must be
                // materialized as a Let even when the arg is a literal,
                // otherwise the in-place substitution silently rewrites the
                // closure body's `LocalGet` -> literal at the call site only,
                // while codegen still compiles the original `func_id` body
                // expecting capture slot 0 to hold the param.
                let mut closure_capt: std::collections::HashSet<LocalId> =
                    std::collections::HashSet::new();
                collect_closure_captured_local_ids(&func.body, &mut closure_capt);

                let mut mutated: std::collections::HashSet<LocalId> =
                    std::collections::HashSet::new();
                collect_mutated_local_ids(&func.body, &mut mutated);

                for (param, arg) in func.params.iter().zip(args.iter()) {
                    let trivial_in_closure = is_trivial_expr(arg)
                        && !matches!(arg, Expr::LocalGet(_))
                        && closure_capt.contains(&param.id);
                    // Body-written params get a copy — see
                    // build_inline_arg_bindings (S13.2.1_A6).
                    let force_let = mutated.contains(&param.id);
                    if is_trivial_expr(arg) && !trivial_in_closure && !force_let {
                        param_map.insert(param.id, arg.clone());
                    } else {
                        let local_id = *next_local_id;
                        *next_local_id += 1;

                        setup_stmts.push(Stmt::Let {
                            id: local_id,
                            name: param.name.clone(),
                            ty: param.ty.clone(),
                            mutable: force_let,
                            init: Some(arg.clone()),
                        });

                        param_map.insert(param.id, Expr::LocalGet(local_id));
                    }
                }

                // Trailing optional/default params with no matching arg —
                // see the matching block in the method-call branch below for
                // the rationale. Without this, references to the unmatched
                // param's source-side LocalId leak into the destination.
                for param in func.params.iter().skip(args.len()) {
                    let local_id = *next_local_id;
                    *next_local_id += 1;
                    setup_stmts.push(Stmt::Let {
                        id: local_id,
                        name: param.name.clone(),
                        ty: param.ty.clone(),
                        mutable: true,
                        init: Some(Expr::Undefined),
                    });
                    param_map.insert(param.id, Expr::LocalGet(local_id));
                }

                let mut inlined_body = func.body.clone();

                // Collect all LocalIds from Let statements in the body and remap them
                let body_local_ids = collect_body_local_ids(&inlined_body);
                for old_id in body_local_ids {
                    param_map.entry(old_id).or_insert_with(|| {
                        let new_id = *next_local_id;
                        *next_local_id += 1;
                        Expr::LocalGet(new_id)
                    });
                }

                substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);

                setup_stmts.extend(inlined_body);

                return Some((setup_stmts, None));
            }
        }

        // Handle method calls
        if let Expr::PropertyGet {
            object,
            property: method_name,
            ..
        } = callee.as_ref()
        {
            // Method inlining requires an exact `let obj = new C(...)`
            // receiver proof. Declared/parameter types are not enough:
            // virtual dispatch can pick a subclass override at runtime, and
            // own-property writes can shadow the prototype method.
            let mut setup_stmts: Vec<Stmt> = Vec::new();
            // Receiver resolution: only exact LocalGet receivers are inlined.
            // PropertyGet chains (e.g. `world.commandBuffer.set(...)`) are
            // technically resolvable via `class_field_types`, but when we
            // tried inlining them by materializing the chain into a fresh
            // `Let __recv = world.commandBuffer` followed by a substituted
            // body, sync-hotpath regressed from 57 → 86 ms and the whole
            // perf-comprehensive table widened. The runtime
            // js_native_call_method dispatch (with the IC) ends up cheaper
            // at scale than the emitted alloca + store + load + inlined
            // body — likely because of the shadow-frame tracking the
            // `Named`-typed materialization triggers, and how LLVM ends up
            // handling the larger inlined body under register pressure.
            // Left as a follow-up; `class_field_types` is plumbed through
            // and `resolve_receiver_class` is defined so the next attempt
            // can swap in here without re-threading the signatures.
            let _ = class_field_types;
            let receiver: Option<(String, Option<LocalId>)> = match object.as_ref() {
                Expr::LocalGet(obj_id) => exact_receiver_facts
                    .get(obj_id)
                    .map(|fact| (fact.class_name.clone(), Some(*obj_id))),
                _ => None,
            };
            if let Some((class_name, obj_id_opt)) = receiver {
                if let Some(method_candidate) =
                    method_candidates.get(&(class_name, method_name.clone()))
                {
                    // Preserve normal `obj.method` lookup and argument
                    // evaluation. Direct substitution would bypass
                    // own-property/accessor shadows and zip() would drop
                    // extra actual args plus their side effects.
                    if !method_candidate.method_lookup_safe
                        || args.len() > method_candidate.func.params.len()
                        || method_contains_lexical_super(&method_candidate.func)
                    {
                        return None;
                    }

                    let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                    // Map 'this' parameter to the receiver object (if present
                    // as a param AND we have a concrete obj_id). For
                    // `Expr::This` receivers there's nothing to map — the
                    // body's `Expr::This` stays as-is.
                    if let (Some(this_id), Some(obj_id)) =
                        (method_candidate.this_param_id, obj_id_opt)
                    {
                        param_map.insert(this_id, Expr::LocalGet(obj_id));
                    }

                    // Issue #858: closure-captured params from the inlined
                    // method body must be materialized as Lets even for
                    // literal args. See try_inline_simple_call.
                    let mut closure_capt: std::collections::HashSet<LocalId> =
                        std::collections::HashSet::new();
                    collect_closure_captured_local_ids(
                        &method_candidate.func.body,
                        &mut closure_capt,
                    );

                    // Map parameters to arguments
                    // Note: Method params don't include 'this' - they use Expr::This instead
                    for (param, arg) in method_candidate.func.params.iter().zip(args.iter()) {
                        let trivial_in_closure = is_trivial_expr(arg)
                            && !matches!(arg, Expr::LocalGet(_))
                            && closure_capt.contains(&param.id);
                        if is_trivial_expr(arg) && !trivial_in_closure {
                            param_map.insert(param.id, arg.clone());
                        } else {
                            let local_id = *next_local_id;
                            *next_local_id += 1;

                            setup_stmts.push(Stmt::Let {
                                id: local_id,
                                name: param.name.clone(),
                                ty: param.ty.clone(),
                                mutable: false,
                                init: Some(arg.clone()),
                            });

                            param_map.insert(param.id, Expr::LocalGet(local_id));
                        }
                    }

                    // Trailing optional/default params with no matching arg:
                    // allocate a fresh local for each so the param's source-
                    // class LocalId doesn't leak into the destination scope
                    // (where it can collide with an unrelated local — e.g.
                    // `World.createQuery(componentTypes, filter = {})` called
                    // with one arg leaves `filter`'s body refs unsubstituted,
                    // and the `if (filter === undefined) filter = {}`
                    // prologue then writes to whatever destination local
                    // happens to share that id).
                    for param in method_candidate.func.params.iter().skip(args.len()) {
                        let local_id = *next_local_id;
                        *next_local_id += 1;
                        setup_stmts.push(Stmt::Let {
                            id: local_id,
                            name: param.name.clone(),
                            ty: param.ty.clone(),
                            mutable: true,
                            init: Some(Expr::Undefined),
                        });
                        param_map.insert(param.id, Expr::LocalGet(local_id));
                    }

                    // Clone and substitute the method body
                    let mut inlined_body = method_candidate.func.body.clone();

                    // Collect all LocalIds from Let statements in the body and remap them
                    let body_local_ids = collect_body_local_ids(&inlined_body);
                    for old_id in body_local_ids {
                        param_map.entry(old_id).or_insert_with(|| {
                            let new_id = *next_local_id;
                            *next_local_id += 1;
                            Expr::LocalGet(new_id)
                        });
                    }

                    substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);
                    if let Some(obj_id) = obj_id_opt {
                        substitute_this_in_stmts(&mut inlined_body, obj_id);
                    }

                    setup_stmts.extend(inlined_body);

                    return Some((setup_stmts, None));
                }
            }
        }
    }
    None
}

/// Check if an expression is trivial (safe to duplicate)
pub fn is_trivial_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested_closure_expr(depth: usize) -> Expr {
        let mut expr = Expr::Integer(1);
        for func_id in 0..depth as u32 {
            expr = Expr::Closure {
                func_id,
                params: Vec::new(),
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(expr))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: false,
                is_async: false,
                is_generator: false,
                is_strict: false,
            };
        }
        expr
    }

    #[test]
    fn inline_expr_skips_extremely_deep_closure_trees() {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let mut expr = nested_closure_expr(MAX_INLINE_EXPR_RECURSION_DEPTH + 64);
                let mut exact_receiver_facts = ExactReceiverFacts::new();
                let mut next_local_id = 1;

                let hoisted = inline_calls_in_expr(
                    &mut expr,
                    &HashMap::new(),
                    &HashMap::new(),
                    &HashMap::new(),
                    &mut exact_receiver_facts,
                    &mut next_local_id,
                    None,
                    &HashMap::new(),
                );

                assert!(hoisted.is_empty());
                assert!(exact_receiver_facts.is_empty());
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// Deeply nested `if (…) { leaf } else { <next level> }` chain — the
    /// #6593 shape. Unlike the linear closure fixture above, every level
    /// makes MULTIPLE guarded descents (condition expr, then-branch stmts,
    /// else-branch stmts), which is exactly what made the pi bundle
    /// unbounded twice over:
    ///  1. `inline_calls_in_stmts` had no guard at all, so the
    ///     stmts→stmts else-chain recursed once per level; and
    ///  2. even with the stmts guard, the eager
    ///     `then_some(InlineExprRecursionGuard)` refunded a depth unit on
    ///     every bail, so the condition's bail at cap let the else-branch
    ///     descend one level deeper — forever.
    fn nested_if_else_chain(depth: usize) -> Vec<Stmt> {
        let mut stmts = vec![Stmt::Expr(Expr::Integer(0))];
        for _ in 0..depth {
            stmts = vec![Stmt::If {
                condition: Expr::Integer(1),
                then_branch: vec![Stmt::Expr(Expr::Integer(2))],
                else_branch: Some(stmts),
            }];
        }
        stmts
    }

    #[test]
    fn inline_stmts_skips_extremely_deep_branching_stmt_trees() {
        // Deep enough that one stack frame per level overflows the 32MB
        // test stack if either the stmts-entry guard is missing or the
        // guard's bail path refunds budget (both variants crashed here
        // before the #6593 fix); shallow enough that the fixture's own
        // recursive drop glue stays far from the limit.
        const DEPTH: usize = MAX_INLINE_EXPR_RECURSION_DEPTH * 200;
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let mut stmts = nested_if_else_chain(DEPTH);
                let mut local_types = HashMap::new();
                let mut exact_receiver_facts = ExactReceiverFacts::new();
                let mut next_local_id = 1;

                inline_calls_in_stmts(
                    &mut stmts,
                    &HashMap::new(),
                    &HashMap::new(),
                    &HashMap::new(),
                    &mut local_types,
                    &mut exact_receiver_facts,
                    &mut next_local_id,
                    None,
                    &HashMap::new(),
                );

                assert_eq!(stmts.len(), 1);
                assert!(exact_receiver_facts.is_empty());
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
