//! ID scanning helpers: find max LocalId / FuncId across module.

use super::*;

/// Find the maximum local ID used in the module.
pub fn compute_max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    for func in &module.functions {
        for param in &func.params {
            max_id = max_id.max(param.id);
        }
        scan_stmts_for_max_local(&func.body, &mut max_id);
    }
    for stmt in &module.init {
        scan_stmt_for_max_local(stmt, &mut max_id);
    }
    for global in &module.globals {
        max_id = max_id.max(global.id);
    }
    // Also scan class member bodies — they share the LocalId namespace.
    // The v0.5.323 issue #212 fix allocates method-local rebind ids per
    // class method per captured outer local; without this scan, the
    // generator transform's freshly-allocated state/done/sent/wrapper
    // ids could collide with those rebind ids and corrupt unrelated
    // class-method codegen.
    for class in &module.classes {
        for method in &class.methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&method.body, &mut max_id);
        }
        for static_method in &class.static_methods {
            for param in &static_method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&static_method.body, &mut max_id);
        }
        for member in &class.computed_members {
            scan_expr_for_max_local(&member.key_expr, &mut max_id);
            for param in &member.function.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&member.function.body, &mut max_id);
        }
        if let Some(ctor) = &class.constructor {
            for param in &ctor.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&ctor.body, &mut max_id);
        }
        for getter in &class.getters {
            for param in &getter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&getter.1.body, &mut max_id);
        }
        for setter in &class.setters {
            for param in &setter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&setter.1.body, &mut max_id);
        }
    }
    max_id
}

pub fn scan_stmts_for_max_local(stmts: &[Stmt], max_id: &mut LocalId) {
    for stmt in stmts {
        scan_stmt_for_max_local(stmt, max_id);
    }
}

pub fn scan_stmt_for_max_local(stmt: &Stmt, max_id: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *max_id = (*max_id).max(*id);
            if let Some(e) = init {
                scan_expr_for_max_local(e, max_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_for_max_local(e, max_id),
        Stmt::Return(e) => {
            if let Some(e) = e {
                scan_expr_for_max_local(e, max_id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(then_branch, max_id);
            if let Some(eb) = else_branch {
                scan_stmts_for_max_local(eb, max_id);
            }
        }
        Stmt::While { condition, body } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::DoWhile { body, condition } => {
            scan_stmts_for_max_local(body, max_id);
            scan_expr_for_max_local(condition, max_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_max_local(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_for_max_local(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_for_max_local(u, max_id);
            }
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts_for_max_local(body, max_id);
            if let Some(c) = catch {
                // The catch param's LocalId only appears in the
                // `CatchClause::param` field; if the catch body never
                // reads/writes it (`catch (e) {}`), no LocalGet/LocalSet
                // carries it, and the body-only scan misses it. The
                // generator transform then allocates `__gen_state` over
                // the catch param's id — every `LocalSet(__gen_state, N)`
                // clobbers `e` and vice versa, so the state machine never
                // advances and the function returns undefined without
                // ever entering state 0. Repro: any async function with
                // `try { await … } catch (e) {}` and an unused catch
                // binding (hono's compose dispatches every middleware
                // through this exact shape).
                if let Some((id, _)) = c.param {
                    *max_id = (*max_id).max(id);
                }
                scan_stmts_for_max_local(&c.body, max_id);
            }
            if let Some(f) = finally {
                scan_stmts_for_max_local(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_max_local(discriminant, max_id);
            for case in cases {
                scan_stmts_for_max_local(&case.body, max_id);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_max_local(body, max_id),
        // PreallocateBoxes carries raw LocalIds (the boxed locals of a
        // closure-conversion pass that ran before this scan); they are
        // usually also visible via captures lists, but max them here so the
        // scan does not depend on that invariant.
        Stmt::PreallocateBoxes(ids) => {
            for id in ids {
                *max_id = (*max_id).max(*id);
            }
        }
        _ => {}
    }
}

/// Walk an expression for any LocalIds it carries — Closure params/captures,
/// LocalGet/LocalSet, and recursively into all sub-expressions. Without this
/// scan, IIFE-style closures emitted into module init (or any
/// `Expr::Call(Closure { params: [...], body: [...] }, args)` shape) hide
/// their parameter LocalIds from `compute_max_local_id`, and the generator
/// transform's freshly-allocated `__gen_state`/`__gen_done`/`__gen_sent`
/// locals collide with them. The collision corrupts every LocalGet/LocalSet
/// in either the IIFE body or the generator state machine and produces
/// silent miscompilation or segfaults.
///
/// Per-variant handling below covers only the LocalId fields an Expr owns
/// directly; descent into sub-expressions is delegated to
/// `perry_hir::walker::walk_expr_children` (exhaustively matched, so a new
/// Expr variant that forgets the walker is a compile error). Pre-#4851 this
/// fn carried its own ad-hoc child walker with a `_ => {}` catch-all, which
/// silently skipped any variant without an explicit arm — `Expr::ObjectAssign`
/// (`Object.assign(p, { a: () => 1, b: () => 2 })`) hid its sources' closures
/// from the scan, the transform minted colliding ids, and codegen (one LLVM
/// function per id) compiled the wrong body for the async-step closure.
pub fn scan_expr_for_max_local(expr: &Expr, max_id: &mut LocalId) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => *max_id = (*max_id).max(*id),
        Expr::Update { id, .. } => *max_id = (*max_id).max(*id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => *max_id = (*max_id).max(*array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => *max_id = (*max_id).max(*id),
        Expr::SetAdd { set_id, .. } => *max_id = (*max_id).max(*set_id),
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Param DEFAULT exprs are visited by the walker below; the
            // body is a Stmt list the expr walker cannot see, so scan it
            // here.
            for p in params {
                *max_id = (*max_id).max(p.id);
            }
            for c in captures {
                *max_id = (*max_id).max(*c);
            }
            for c in mutable_captures {
                *max_id = (*max_id).max(*c);
            }
            scan_stmts_for_max_local(body, max_id);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr_for_max_local(e, max_id));
}

/// Find the maximum func ID used in the module.
pub fn compute_max_func_id(module: &Module) -> FuncId {
    let mut max_id: FuncId = 0;
    for func in &module.functions {
        max_id = max_id.max(func.id);
        scan_stmts_for_max_func(&func.body, &mut max_id);
    }
    for stmt in &module.init {
        scan_stmt_for_max_func(stmt, &mut max_id);
    }
    // Issue #154: class member bodies share the FuncId namespace — their
    // nested closures (executors, callbacks, dispose-method bodies)
    // carry FuncIds that the iterator state-machine transform must not
    // collide with. Mirrors the v0.5.323 class-scan added to
    // `compute_max_local_id` for issue #212. Without this, an
    // `await using r = new R()` where `R[Symbol.asyncDispose]()` awaits
    // a user-built Promise has its executor closure's FuncId reused for
    // the synthesized iterator-return, and at codegen the iterator-
    // return body wins. The body reads past the (0-cap) executor's
    // capture array and `js_box_set`s a code-segment pointer → SIGBUS.
    for class in &module.classes {
        for m in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.computed_members.iter().map(|member| &member.function))
            .chain(class.constructor.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            scan_stmts_for_max_func(&m.body, &mut max_id);
        }
        for member in &class.computed_members {
            scan_expr_for_max_func(&member.key_expr, &mut max_id);
        }
    }
    max_id
}

pub fn scan_stmts_for_max_func(stmts: &[Stmt], max_id: &mut FuncId) {
    for stmt in stmts {
        scan_stmt_for_max_func(stmt, max_id);
    }
}

pub fn scan_stmt_for_max_func(stmt: &Stmt, max_id: &mut FuncId) {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            scan_expr_for_max_func(expr, max_id);
        }
        Stmt::Let {
            init: Some(expr), ..
        } => scan_expr_for_max_func(expr, max_id),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_max_func(condition, max_id);
            scan_stmts_for_max_func(then_branch, max_id);
            if let Some(eb) = else_branch {
                scan_stmts_for_max_func(eb, max_id);
            }
        }
        // Mirror scan_stmt_for_max_local: a closure declared inside a
        // do-while or labeled loop must still bump the max func id, or the
        // generator transform can mint a colliding func id (#1824 gap class).
        // Loop/switch HEAD expressions can nest closures too (`while
        // ((() => f())())`, `for (let g = () => 1;;)`), so scan them like
        // the local-id twin does, not just the bodies.
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_for_max_func(condition, max_id);
            scan_stmts_for_max_func(body, max_id);
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_max_func(body, max_id),
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_max_func(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_for_max_func(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_for_max_func(u, max_id);
            }
            scan_stmts_for_max_func(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts_for_max_func(body, max_id);
            if let Some(c) = catch {
                scan_stmts_for_max_func(&c.body, max_id);
            }
            if let Some(f) = finally {
                scan_stmts_for_max_func(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_max_func(discriminant, max_id);
            for case in cases {
                scan_stmts_for_max_func(&case.body, max_id);
            }
        }
        _ => {}
    }
}

/// Walk an expression for any FuncIds it carries — `FuncRef`s and `Closure`s,
/// recursively through all sub-expressions and closure bodies.
///
/// Per-variant handling below covers only the FuncId fields an Expr owns
/// directly; descent into sub-expressions is delegated to
/// `perry_hir::walker::walk_expr_children` (exhaustively matched, so a new
/// Expr variant that forgets the walker is a compile error). Pre-#4851 this
/// fn carried its own ad-hoc child walker with a `_ => {}` catch-all that
/// silently skipped any variant without an explicit arm. The catch-all bit
/// repeatedly (#393, #531, #1824 each added one more arm); #4851 was the
/// same class: `Object.assign(promise, { a: () => 1, b: () => 2 })` lowers
/// to `ObjectAssign { sources: [New { __AnonShape, args: [Closure, ...] }] }`,
/// no `ObjectAssign` arm existed, so the literal's closures were invisible
/// here. `transform_generators` then minted the SAME FuncId for the
/// `__async_step` closure of a caller awaiting that function, codegen (one
/// LLVM function per FuncId) compiled the arrow body in place of the step
/// closure, and the await's continuation was silently dropped — `await
/// makeReq()` never resumed (the Stripe SDK's auto-pagination
/// `Object.assign` hit exactly this).
pub fn scan_expr_for_max_func(expr: &Expr, max_id: &mut FuncId) {
    match expr {
        Expr::FuncRef(id) => *max_id = (*max_id).max(*id),
        Expr::Closure { func_id, body, .. } => {
            // Param DEFAULT exprs are visited by the walker below; the
            // body is a Stmt list the expr walker cannot see, so scan it
            // here.
            *max_id = (*max_id).max(*func_id);
            scan_stmts_for_max_func(body, max_id);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr_for_max_func(e, max_id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_types::Type;

    /// #4851: `Object.assign(p, { a: () => 1, b: () => 2 })` lowers to
    /// `ObjectAssign { sources: [New { __AnonShape, args: [Closure, ...] }] }`.
    /// The pre-#4851 ad-hoc scanners had no `ObjectAssign` arm, so the
    /// literal's closures were invisible to the max-id scans —
    /// `transform_generators` minted the same FuncId for its synthesized
    /// `__async_step` closure and codegen compiled the arrow body in its
    /// place, silently dropping the await continuation of any caller that
    /// awaited the function directly. The scans now recurse through
    /// `perry_hir::walker::walk_expr_children`, so ANY variant that nests a
    /// closure keeps its ids visible.
    #[test]
    fn object_assign_sources_visible_to_id_scans() {
        let closure = Expr::Closure {
            func_id: 7,
            params: vec![Param {
                id: 9,
                name: "x".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Integer(1)))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        let expr = Expr::ObjectAssign {
            target: Box::new(Expr::LocalGet(3)),
            sources: vec![Expr::New {
                class_name: "__AnonShape_test".to_string(),
                args: vec![closure],
                type_args: Vec::new(),
            }],
        };

        let mut max_func: FuncId = 0;
        scan_expr_for_max_func(&expr, &mut max_func);
        assert_eq!(max_func, 7, "closure FuncId inside ObjectAssign sources");

        let mut max_local: LocalId = 0;
        scan_expr_for_max_local(&expr, &mut max_local);
        assert_eq!(
            max_local, 9,
            "closure param LocalId inside ObjectAssign sources"
        );
    }
}
