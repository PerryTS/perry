use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{BinaryOp, Class, Expr, Function, Module, Param, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::{HashMap, HashSet};

use super::*;

pub fn body_contains_super_call(stmts: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        match expr {
            Expr::SuperCall(_) | Expr::SuperMethodCall { .. } => true,
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => check_expr(left) || check_expr(right),
            Expr::Unary { operand, .. } => check_expr(operand),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => check_expr(condition) || check_expr(then_expr) || check_expr(else_expr),
            Expr::Call { callee, args, .. } => check_expr(callee) || args.iter().any(check_expr),
            Expr::Array(elements) => elements.iter().any(check_expr),
            Expr::IndexGet { object, index } => check_expr(object) || check_expr(index),
            Expr::IndexSet {
                object,
                index,
                value,
            } => check_expr(object) || check_expr(index) || check_expr(value),
            Expr::PropertyGet { object, .. } => check_expr(object),
            Expr::PropertySet { object, value, .. } => check_expr(object) || check_expr(value),
            Expr::LocalSet(_, value) => check_expr(value),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => check_expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    || then_branch.iter().any(check_stmt)
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(check_stmt))
            }
            Stmt::While { condition, body } => check_expr(condition) || body.iter().any(check_stmt),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i))
                    || condition.as_ref().is_some_and(check_expr)
                    || update.as_ref().is_some_and(check_expr)
                    || body.iter().any(check_stmt)
            }
            _ => false,
        }
    }

    stmts.iter().any(check_stmt)
}

/// Returns true if the body references the dynamic `this` or `new.target`
/// bindings — directly (`Expr::This` / `Expr::NewTarget`) or through a nested
/// arrow that lexically captures them (`captures_this` / `captures_new_target`).
///
/// These bindings belong to the function's OWN invocation: spec
/// `OrdinaryCallBindThis` binds `this` from the call's thisArgument (which for a
/// plain `f()` call is `undefined` in strict code, the global object in sloppy
/// code), and `new.target` from the construct. Substituting the body into the
/// caller (what the inliner does) silently rebinds them to the CALLER's frame —
/// a strict callee whose `this` is `undefined` would instead read the caller's
/// `this` (e.g. the global object), and `typeof this` flips from `"undefined"`
/// to `"object"`. Refs test262 language/function-code `10.4.3-1-*` strict-mode
/// `this`. Reject inlining of any such function.
pub fn body_references_dynamic_this(stmts: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        if matches!(expr, Expr::This | Expr::NewTarget) {
            return true;
        }
        // An arrow that lexically uses the enclosing `this`/`new.target` records
        // it via these flags. We do NOT descend into the closure body for a bare
        // `Expr::This` (that one is the closure's OWN binding when it isn't an
        // arrow); `walk_expr_children` only yields a closure's param defaults,
        // which DO execute in the enclosing frame, so those are still checked.
        if let Expr::Closure {
            captures_this,
            captures_new_target,
            ..
        } = expr
        {
            if *captures_this || *captures_new_target {
                return true;
            }
        }
        let mut found = false;
        walk_expr_children(expr, &mut |child| {
            if !found && check_expr(child) {
                found = true;
            }
        });
        found
    }

    fn check_stmt(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let { init, .. } => init.as_ref().is_some_and(check_expr),
            Stmt::Expr(expr) | Stmt::Throw(expr) => check_expr(expr),
            Stmt::Return(expr) => expr.as_ref().is_some_and(check_expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    || then_branch.iter().any(check_stmt)
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(check_stmt))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                check_expr(condition) || body.iter().any(check_stmt)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i))
                    || condition.as_ref().is_some_and(check_expr)
                    || update.as_ref().is_some_and(check_expr)
                    || body.iter().any(check_stmt)
            }
            _ => false,
        }
    }

    stmts.iter().any(check_stmt)
}

/// Check if statements contain a closure that captures any of the given local IDs
pub fn body_contains_closure_capturing(
    stmts: &[Stmt],
    captured_ids: &std::collections::HashSet<LocalId>,
) -> bool {
    fn check_expr(expr: &Expr, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match expr {
            Expr::Closure { captures, body, .. } => {
                // Check if any capture is in the set of IDs we're looking for
                for capture_id in captures {
                    if captured_ids.contains(capture_id) {
                        return true;
                    }
                }
                // Also check the closure body for nested closures
                body_contains_closure_capturing(body, captured_ids)
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                check_expr(left, captured_ids) || check_expr(right, captured_ids)
            }
            Expr::Unary { operand, .. } => check_expr(operand, captured_ids),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                check_expr(condition, captured_ids)
                    || check_expr(then_expr, captured_ids)
                    || check_expr(else_expr, captured_ids)
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee, captured_ids) || args.iter().any(|a| check_expr(a, captured_ids))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, captured_ids)),
            Expr::IndexGet { object, index } => {
                check_expr(object, captured_ids) || check_expr(index, captured_ids)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                check_expr(object, captured_ids)
                    || check_expr(index, captured_ids)
                    || check_expr(value, captured_ids)
            }
            Expr::PropertyGet { object, .. } => check_expr(object, captured_ids),
            Expr::PropertySet { object, value, .. } => {
                check_expr(object, captured_ids) || check_expr(value, captured_ids)
            }
            Expr::LocalSet(_, value) => check_expr(value, captured_ids),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr, captured_ids),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, captured_ids)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, captured_ids)
                    || then_branch.iter().any(|s| check_stmt(s, captured_ids))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| check_stmt(s, captured_ids)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, captured_ids)
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i, captured_ids))
                    || condition
                        .as_ref()
                        .is_some_and(|c| check_expr(c, captured_ids))
                    || update.as_ref().is_some_and(|u| check_expr(u, captured_ids))
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            _ => false,
        }
    }

    stmts.iter().any(|s| check_stmt(s, captured_ids))
}

/// Collect every LocalId that appears in some nested closure's `captures` or
/// `mutable_captures` list anywhere inside `stmts`. Used by the call-site
/// inliners to decide which params *must* be materialized as a Let rather
/// than substituted in-place with the call argument.
///
/// Why this matters (issue #858): closure bodies have a stable `func_id`,
/// which codegen compiles ONCE from whichever `Expr::Closure` occurrence
/// `collect_closures_in_*` saw first (typically the original definition
/// inside the enclosing function). The body of that compiled function reads
/// captured locals from indexed slots in the per-closure capture array. If
/// the inliner substitutes a literal (`Integer(2026)`) for a captured
/// `LocalGet` inside the closure's body at a call site, the call-site
/// `Expr::Closure` ends up with an empty `captures` list — so
/// `compute_auto_captures` at the closure-creation site emits zero capture
/// slots. But the compiled body (from the original occurrence) still reads
/// slot 0, getting an uninitialized value. The visible symptom is that a
/// closure-captured numeric param reads as `0` inside an object-literal
/// method shorthand (`function makeDT(y){ return { toDate(){ ... y ... } } }`).
///
/// Fix: when the inlined function's body contains a closure that captures
/// one of the params, force the inliner to introduce a setup `Let` for that
/// param (instead of substituting the arg literal in place). The closure
/// body then continues to reference the param via `LocalGet(fresh_id)` and
/// `captures: [fresh_id]`, preserving the func_id <-> capture-shape contract.
pub fn collect_closure_captured_local_ids(
    stmts: &[Stmt],
    out: &mut std::collections::HashSet<LocalId>,
) {
    fn visit_expr(e: &Expr, out: &mut std::collections::HashSet<LocalId>) {
        if let Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } = e
        {
            for id in captures {
                out.insert(*id);
            }
            for id in mutable_captures {
                out.insert(*id);
            }
            // Nested closures inside the body can also capture outer
            // params transitively (e.g. `function f(y){ return { m(){ return
            // [].map(_ => y); } } }`). Walk the body so we don't miss them.
            collect_closure_captured_local_ids(body, out);
        }
        // Descend into immediate sub-expressions for non-Closure variants
        // (and into Param defaults for Closure). The walker is exhaustive,
        // so any new HIR variant carrying an Expr is automatically covered.
        perry_hir::walker::walk_expr_children(e, &mut |sub| visit_expr(sub, out));
    }

    fn visit_stmt(s: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
        match s {
            Stmt::Let { init: Some(e), .. } => visit_expr(e, out),
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => visit_expr(e, out),
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit_expr(condition, out);
                for s in then_branch {
                    visit_stmt(s, out);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        visit_stmt(s, out);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                visit_expr(condition, out);
                for s in body {
                    visit_stmt(s, out);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    visit_stmt(i, out);
                }
                if let Some(c) = condition {
                    visit_expr(c, out);
                }
                if let Some(u) = update {
                    visit_expr(u, out);
                }
                for s in body {
                    visit_stmt(s, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                visit_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        visit_expr(t, out);
                    }
                    for s in &case.body {
                        visit_stmt(s, out);
                    }
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    visit_stmt(s, out);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        visit_stmt(s, out);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        visit_stmt(s, out);
                    }
                }
            }
            Stmt::Labeled { body, .. } => visit_stmt(body, out),
            _ => {}
        }
    }

    for s in stmts {
        visit_stmt(s, out);
    }
}

/// Check if a function is "pure" for init-inlining purposes: its body only
/// references its own parameters and locally-declared variables.  No GlobalGet,
/// GlobalSet, ExternFuncRef, or NativeMethodCall.  This makes it safe to inline
/// into module init context where module-level variables are cached in locals.
pub fn is_pure_function(func: &Function) -> bool {
    let mut known_ids: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for p in &func.params {
        known_ids.insert(p.id);
    }
    // Collect all Let-declared IDs in the body
    let body_ids = collect_body_local_ids(&func.body);
    for id in body_ids {
        known_ids.insert(id);
    }

    fn expr_is_pure(e: &Expr, known: &std::collections::HashSet<LocalId>) -> bool {
        match e {
            Expr::GlobalGet(_) | Expr::GlobalSet(_, _) => false,
            Expr::ExternFuncRef { .. } => false,
            Expr::NativeMethodCall { .. } => false,
            Expr::LocalGet(id) | Expr::Update { id, .. } => known.contains(id),
            Expr::LocalSet(id, val) => known.contains(id) && expr_is_pure(val, known),
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                expr_is_pure(left, known) && expr_is_pure(right, known)
            }
            Expr::Unary { operand, .. } => expr_is_pure(operand, known),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                expr_is_pure(condition, known)
                    && expr_is_pure(then_expr, known)
                    && expr_is_pure(else_expr, known)
            }
            Expr::Call { callee, args, .. } => {
                expr_is_pure(callee, known) && args.iter().all(|a| expr_is_pure(a, known))
            }
            Expr::Array(elems) => elems.iter().all(|e| expr_is_pure(e, known)),
            Expr::IndexGet { object, index } => {
                expr_is_pure(object, known) && expr_is_pure(index, known)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                expr_is_pure(object, known)
                    && expr_is_pure(index, known)
                    && expr_is_pure(value, known)
            }
            Expr::PropertyGet { object, .. } => expr_is_pure(object, known),
            Expr::PropertySet { object, value, .. } => {
                expr_is_pure(object, known) && expr_is_pure(value, known)
            }
            // Leaf expressions with no variable references are always pure
            Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::FuncRef(_)
            | Expr::This => true,
            // For anything else we haven't explicitly handled, be conservative
            _ => true,
        }
    }

    fn stmt_is_pure(s: &Stmt, known: &std::collections::HashSet<LocalId>) -> bool {
        match s {
            Stmt::Let { init: Some(e), .. } => expr_is_pure(e, known),
            Stmt::Let { init: None, .. } => true,
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => expr_is_pure(e, known),
            Stmt::Return(None) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_is_pure(condition, known)
                    && then_branch.iter().all(|s| stmt_is_pure(s, known))
                    && else_branch
                        .as_ref()
                        .is_none_or(|b| b.iter().all(|s| stmt_is_pure(s, known)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_is_pure(condition, known) && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|i| stmt_is_pure(i, known))
                    && condition.as_ref().is_none_or(|c| expr_is_pure(c, known))
                    && update.as_ref().is_none_or(|u| expr_is_pure(u, known))
                    && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            _ => false, // conservative: reject Switch, Try, etc.
        }
    }

    func.body.iter().all(|s| stmt_is_pure(s, &known_ids))
}

/// Check if statements have simple control flow suitable for inlining
pub fn has_simple_control_flow(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            // `Stmt::Throw` is allowed: an inlined body that throws just
            // raises the same exception in the caller's frame, which is
            // the correct propagation semantic for JS. Most ECS code
            // hot-paths through `private assert*` helpers shaped as
            // `if (!cond) { throw new Error(...) }` — without inlining,
            // the assertion is an unconditional cross-module dispatch
            // per call.
            Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Return(_) | Stmt::Throw(_) => {}
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !has_simple_control_flow(then_branch) {
                    return false;
                }
                if let Some(else_b) = else_branch {
                    if !has_simple_control_flow(else_b) {
                        return false;
                    }
                }
            }
            Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::Try { .. }
            | Stmt::Switch { .. }
            | Stmt::Labeled { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {
                return false;
            }
            Stmt::PreallocateBoxes(_) => {}
        }
    }
    true
}

/// Find the maximum local ID used in statements
pub fn find_max_local_id(stmts: &[Stmt]) -> LocalId {
    let mut max_id: LocalId = 0;

    // Track every LocalId encountered. Per-variant handling for the LocalId
    // fields owned directly by an Expr; descent into sub-expressions is
    // delegated to `walk_expr_children` (single source of truth — see
    // `perry_hir::walker` for why). Pre-refactor this fn carried its own
    // ad-hoc walker with a `_ => {}` catch-all, which silently undercounted
    // any new LocalId-bearing variant (issues #167, #169, #214).
    fn check_expr(expr: &Expr, max_id: &mut LocalId) {
        match expr {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::Update { id, .. } => {
                *max_id = (*max_id).max(*id);
            }
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                *max_id = (*max_id).max(*array_id);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::SetAdd { set_id, .. } => {
                *max_id = (*max_id).max(*set_id);
            }
            Expr::Closure {
                params,
                body,
                captures,
                mutable_captures,
                ..
            } => {
                // Closure has THREE LocalId sources: params, captures,
                // mutable_captures. The body's nested LocalGets contribute via
                // check_stmt. Param defaults need check_expr too. Short-circuit
                // (`return`) so the walker below doesn't double-descend into
                // Param defaults.
                for param in params {
                    *max_id = (*max_id).max(param.id);
                    if let Some(d) = &param.default {
                        check_expr(d, max_id);
                    }
                }
                for id in captures {
                    *max_id = (*max_id).max(*id);
                }
                for id in mutable_captures {
                    *max_id = (*max_id).max(*id);
                }
                for stmt in body {
                    check_stmt(stmt, max_id);
                }
                return;
            }
            _ => {}
        }
        // Descend into all immediate sub-expressions. Exhaustive on Expr —
        // a new variant added to ir.rs without updating walker.rs is a
        // compile error.
        walk_expr_children(expr, &mut |child| check_expr(child, max_id));
    }

    fn check_stmt(stmt: &Stmt, max_id: &mut LocalId) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                *max_id = (*max_id).max(*id);
                if let Some(expr) = init {
                    check_expr(expr, max_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, max_id);
            }
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, max_id);
                for s in then_branch {
                    check_stmt(s, max_id);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::While { condition, body } => {
                check_expr(condition, max_id);
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::DoWhile { body, condition } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                check_expr(condition, max_id);
            }
            Stmt::Labeled { body, .. } => {
                check_stmt(body, max_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    check_stmt(i, max_id);
                }
                if let Some(c) = condition {
                    check_expr(c, max_id);
                }
                if let Some(u) = update {
                    check_expr(u, max_id);
                }
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        *max_id = (*max_id).max(*id);
                    }
                    for s in &c.body {
                        check_stmt(s, max_id);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, max_id);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_expr(test, max_id);
                    }
                    for s in &case.body {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::PreallocateBoxes(ids) => {
                for id in ids {
                    *max_id = (*max_id).max(*id);
                }
            }
        }
    }

    for stmt in stmts {
        check_stmt(stmt, &mut max_id);
    }

    max_id
}
