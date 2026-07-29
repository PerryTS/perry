use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Function, Stmt};

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

/// Like `body_references_dynamic_this`, but tolerates a *direct* `Expr::This`,
/// which the method-inliner rewrites to the concrete receiver via
/// `substitute_this_in_stmts`. Returns true only for constructs the
/// substitution can NOT safely rewrite, so the method must stay un-inlined:
/// `new.target` (left untouched), and any nested closure (a regular function
/// has its own `this`/`new.target`; an arrow captures the enclosing one —
/// neither binding is rewritten here). Conservative: rejecting *all* nested
/// closures keeps `this.field` accessors/mutators inlinable without risking a
/// mis-bound `this` inside a nested function.
pub fn method_body_blocks_this_substitution(stmts: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        if matches!(expr, Expr::NewTarget | Expr::Closure { .. }) {
            return true;
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

/// Check if statements contain a closure that captures any of the given local IDs.
///
/// Delegates to [`collect_closure_captured_local_ids`], which walks the HIR via
/// the exhaustive `perry_hir::walker` — so it sees closures nested inside ANY
/// expression, including array-method callbacks (`xs.filter(t => …)`,
/// `.map`/`.reduce`/…), which lower to dedicated `Expr::ArrayFilter` /
/// `Expr::ArrayMap` nodes rather than `Expr::Call`. The previous hand-rolled
/// walker only matched a fixed set of `Expr` variants (`Call`, `Binary`,
/// `PropertyGet`, …) and silently returned `false` for those array-method
/// nodes. That let a function whose ONLY param-capturing closure lived inside a
/// `.filter(...)` callback pass the `is_inlinable` gate and get inlined — at
/// which point the closure literal is duplicated with its captures remapped to
/// the caller's locals while the body is still codegen-compiled ONCE under the
/// original capture ids. When the caller's local has a different
/// value-representation (e.g. heap-boxed because it lives across an `await` in
/// an async caller, vs. the callee's unboxed by-value param), the remapped
/// creation site forwards a raw box pointer that the shared body reads as a
/// plain value → the captured object is observed as a `number`. Reusing the
/// exhaustive collector closes that gap for every Expr variant, present and
/// future.
pub fn body_contains_closure_capturing(
    stmts: &[Stmt],
    captured_ids: &std::collections::HashSet<LocalId>,
) -> bool {
    if captured_ids.is_empty() {
        return false;
    }
    let mut captured_by_closures = std::collections::HashSet::new();
    collect_closure_captured_local_ids(stmts, &mut captured_by_closures);
    captured_by_closures
        .iter()
        .any(|id| captured_ids.contains(id))
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

/// Collect every LocalId WRITTEN by the statements — `LocalSet` and
/// `Update` (++/--) targets, including inside nested closure bodies.
///
/// Used by the inliner: a parameter the body mutates must be materialised
/// as a fresh setup `Let` (a copy) rather than substituted with the caller's
/// argument expression in place. Substituting a `LocalGet(x)` makes the
/// body's `param++` rewrite into `x++` and MUTATE THE CALLER'S LOCAL —
/// test262 S13.2.1_A6 (`function f(a){ a++ } var x=1; f(x)` left x===2).
pub fn collect_mutated_local_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
    fn visit_expr(e: &Expr, out: &mut std::collections::HashSet<LocalId>) {
        match e {
            Expr::LocalSet(id, _) => {
                out.insert(*id);
            }
            Expr::Update { id, .. } => {
                out.insert(*id);
            }
            Expr::Closure { body, .. } => collect_mutated_local_ids(body, out),
            _ => {}
        }
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
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
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
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::Param;
    use std::collections::HashSet;

    fn param(id: LocalId) -> Param {
        Param {
            id,
            name: format!("p{id}"),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    fn capturing_closure(func_id: u32, capture: LocalId, body_param: LocalId) -> Expr {
        Expr::Closure {
            func_id,
            params: vec![param(body_param)],
            return_type: Type::Any,
            // Body reads the captured local — what makes it a real capture.
            body: vec![Stmt::Return(Some(Expr::LocalGet(capture)))],
            captures: vec![capture],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    // Regression: a closure that captures a param but lives inside an
    // `Array.filter` callback must be detected, so `is_inlinable` rejects the
    // enclosing function. The old hand-rolled walker only matched `Expr::Call`
    // / `Binary` / `PropertyGet` / … and silently returned `false` for the
    // dedicated `Expr::ArrayFilter` node, letting such a function be inlined —
    // which duplicated the closure with remapped captures while the single
    // compiled body kept the original (different-box-state) capture ids, so an
    // async caller's heap-boxed local was forwarded as a raw box pointer and
    // read back as a `number`. (caps `can()` 500 on `GET /admin`.)
    #[test]
    fn detects_capture_inside_array_filter_callback() {
        // function f(allCaps) {
        //   return xs.filter((t) => can({ caps: allCaps }, t));  // captures allCaps
        // }
        let body = vec![Stmt::Return(Some(Expr::ArrayFilter {
            array: Box::new(Expr::LocalGet(99)),
            callback: Box::new(capturing_closure(7, 1, 2)),
        }))];
        let param_ids: HashSet<LocalId> = [1].into_iter().collect();
        assert!(
            body_contains_closure_capturing(&body, &param_ids),
            "closure capturing param inside ArrayFilter callback must be detected"
        );
    }

    // Same hazard via the other array-iteration node the old walker missed.
    #[test]
    fn detects_capture_inside_array_map_callback() {
        let body = vec![Stmt::Return(Some(Expr::ArrayMap {
            array: Box::new(Expr::LocalGet(99)),
            callback: Box::new(capturing_closure(7, 1, 2)),
        }))];
        let param_ids: HashSet<LocalId> = [1].into_iter().collect();
        assert!(body_contains_closure_capturing(&body, &param_ids));
    }

    // Sanity: a closure that captures a NON-param local must not trip the
    // param-capture check (avoids over-rejecting inline candidates).
    #[test]
    fn ignores_closure_not_capturing_any_param() {
        let body = vec![Stmt::Return(Some(Expr::ArrayFilter {
            array: Box::new(Expr::LocalGet(99)),
            // Captures id 5, but we only ask about param id 1.
            callback: Box::new(capturing_closure(7, 5, 2)),
        }))];
        let param_ids: HashSet<LocalId> = [1].into_iter().collect();
        assert!(!body_contains_closure_capturing(&body, &param_ids));
    }

    // The pre-existing direct-Call path must keep working.
    #[test]
    fn detects_capture_inside_plain_call_arg() {
        let body = vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::LocalGet(50)),
            args: vec![capturing_closure(7, 1, 2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))];
        let param_ids: HashSet<LocalId> = [1].into_iter().collect();
        assert!(body_contains_closure_capturing(&body, &param_ids));
    }
}

/// Every binding the function itself introduces: `let`/`const` declarations
/// (including a `for` init and a `catch` param) and any id the lowering has
/// already marked for boxing.
///
/// These are exactly the ids that do not exist in a caller's scope, so a
/// closure capturing one of them cannot survive having the body cloned into a
/// call site — see the closure guard in [`super::analysis::is_inlinable`].
pub fn collect_declared_local_ids(stmts: &[Stmt], out: &mut std::collections::HashSet<LocalId>) {
    for s in stmts {
        match s {
            Stmt::Let { id, .. } => {
                out.insert(*id);
            }
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                out.extend(ids.iter().copied());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_declared_local_ids(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_declared_local_ids(else_branch, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_declared_local_ids(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_declared_local_ids(std::slice::from_ref(init.as_ref()), out);
                }
                collect_declared_local_ids(body, out);
            }
            Stmt::Labeled { body, .. } => {
                collect_declared_local_ids(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_declared_local_ids(body, out);
                if let Some(catch) = catch {
                    if let Some((id, _)) = &catch.param {
                        out.insert(*id);
                    }
                    collect_declared_local_ids(&catch.body, out);
                }
                if let Some(finally) = finally {
                    collect_declared_local_ids(finally, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_declared_local_ids(&case.body, out);
                }
            }
            Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Throw(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
        }
    }
}
