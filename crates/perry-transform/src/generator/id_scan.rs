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
pub fn scan_expr_for_max_local(expr: &Expr, max_id: &mut LocalId) {
    match expr {
        Expr::LocalGet(id) => *max_id = (*max_id).max(*id),
        Expr::LocalSet(id, value) => {
            *max_id = (*max_id).max(*id);
            scan_expr_for_max_local(value, max_id);
        }
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
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
        Expr::Call { callee, args, .. } => {
            scan_expr_for_max_local(callee, max_id);
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            scan_expr_for_max_local(callee, max_id);
            for a in args {
                let inner = match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => e,
                };
                scan_expr_for_max_local(inner, max_id);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::NewDynamic { callee, args } => {
            scan_expr_for_max_local(callee, max_id);
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        // Issue #393: NativeMethodCall args carry the route handler
        // closures emitted by `app.post('/route', async (req, reply) => { ... })`
        // and the WS callback closures from `wss.on('listening', () => ...)`.
        // Without this arm those closures' params are invisible to the scanner,
        // and the async-to-generator step closures the next pass synthesizes
        // collide with their LocalIds — manifesting as `js_box_set: invalid box
        // pointer` warnings + a SIGSEGV at hub-scale (perry-hub).
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                scan_expr_for_max_local(obj, max_id);
            }
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::SuperCall(args) => {
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for a in args {
                scan_expr_for_max_local(a, max_id);
            }
        }
        Expr::Await(inner) | Expr::Unary { operand: inner, .. } => {
            scan_expr_for_max_local(inner, max_id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            scan_expr_for_max_local(left, max_id);
            scan_expr_for_max_local(right, max_id);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            scan_expr_for_max_local(condition, max_id);
            scan_expr_for_max_local(then_expr, max_id);
            scan_expr_for_max_local(else_expr, max_id);
        }
        Expr::PropertyGet { object, .. } => scan_expr_for_max_local(object, max_id),
        Expr::PropertySet { object, value, .. } => {
            scan_expr_for_max_local(object, max_id);
            scan_expr_for_max_local(value, max_id);
        }
        Expr::IndexGet { object, index } => {
            scan_expr_for_max_local(object, max_id);
            scan_expr_for_max_local(index, max_id);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            scan_expr_for_max_local(object, max_id);
            scan_expr_for_max_local(index, max_id);
            scan_expr_for_max_local(value, max_id);
        }
        Expr::Array(items) => {
            for item in items {
                scan_expr_for_max_local(item, max_id);
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                scan_expr_for_max_local(v, max_id);
            }
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                scan_expr_for_max_local(e, max_id);
            }
        }
        Expr::Yield { value: Some(v), .. } => scan_expr_for_max_local(v, max_id),
        // Issue #531: ArrayPush/ArrayPushSpread carry a `value`/`source`
        // expression that frequently nests `Closure { func_id, ... }` —
        // e.g. `ops.push(await runOp('name', N, async () => ...))` lowers
        // to `ArrayPush { value: Await(Call { args: [..., Closure {...}] }) }`.
        // Without these arms the scanner misses the closure's params/captures
        // (and the matching arm in `scan_expr_for_max_func` misses its
        // FuncId), and the synthesized async-step closures the next pass
        // emits collide with them — manifesting as `js_box_set/get: invalid
        // box pointer 0x0` warnings + silently elided closure bodies.
        Expr::ArrayPush { value, .. } => scan_expr_for_max_local(value, max_id),
        Expr::ArrayPushSpread { source, .. } => scan_expr_for_max_local(source, max_id),
        // Array fast-path variants — each has a closure callback whose
        // parameter LocalIds would otherwise be invisible to the scanner.
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            scan_expr_for_max_local(array, max_id);
            scan_expr_for_max_local(callback, max_id);
        }
        Expr::ArraySort { array, comparator } => {
            scan_expr_for_max_local(array, max_id);
            scan_expr_for_max_local(comparator, max_id);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            scan_expr_for_max_local(array, max_id);
            scan_expr_for_max_local(callback, max_id);
            if let Some(i) = initial {
                scan_expr_for_max_local(i, max_id);
            }
        }
        Expr::ArrayToSorted { array, comparator } => {
            scan_expr_for_max_local(array, max_id);
            if let Some(c) = comparator {
                scan_expr_for_max_local(c, max_id);
            }
        }
        Expr::ObjectGroupBy { items, key_fn } => {
            scan_expr_for_max_local(items, max_id);
            scan_expr_for_max_local(key_fn, max_id);
        }
        _ => {}
    }
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
            .chain(class.constructor.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            scan_stmts_for_max_func(&m.body, &mut max_id);
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
        Stmt::While { body, .. } => scan_stmts_for_max_func(body, max_id),
        Stmt::For { body, .. } => scan_stmts_for_max_func(body, max_id),
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
        Stmt::Switch { cases, .. } => {
            for case in cases {
                scan_stmts_for_max_func(&case.body, max_id);
            }
        }
        _ => {}
    }
}

pub fn scan_expr_for_max_func(expr: &Expr, max_id: &mut FuncId) {
    match expr {
        Expr::FuncRef(id) => *max_id = (*max_id).max(*id),
        Expr::Closure { func_id, body, .. } => {
            *max_id = (*max_id).max(*func_id);
            scan_stmts_for_max_func(body, max_id);
        }
        Expr::Call { callee, args, .. } => {
            scan_expr_for_max_func(callee, max_id);
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            scan_expr_for_max_func(callee, max_id);
            for a in args {
                let inner = match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => e,
                };
                scan_expr_for_max_func(inner, max_id);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::NewDynamic { callee, args } => {
            scan_expr_for_max_func(callee, max_id);
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        // Issue #393: see the matching arm in `scan_expr_for_max_local`.
        // Same gap on the FuncId side — async route handlers passed to
        // `app.post('/r', async (req, reply) => { ... })` register here as
        // `NativeMethodCall { args: [..., Closure { func_id, .. }] }`. Without
        // this arm `transform_generators`'s `next_func_id` starts below those
        // closures' ids and the synthesized async-step closures collide with
        // them at codegen, leaving the alloc site emitting `i32 1` capture
        // slots while the body reads capture[16] off the end of the
        // allocation.
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                scan_expr_for_max_func(obj, max_id);
            }
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::SuperCall(args) => {
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for a in args {
                scan_expr_for_max_func(a, max_id);
            }
        }
        Expr::Await(inner) | Expr::Unary { operand: inner, .. } => {
            scan_expr_for_max_func(inner, max_id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            scan_expr_for_max_func(left, max_id);
            scan_expr_for_max_func(right, max_id);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            scan_expr_for_max_func(condition, max_id);
            scan_expr_for_max_func(then_expr, max_id);
            scan_expr_for_max_func(else_expr, max_id);
        }
        Expr::PropertyGet { object, .. } => scan_expr_for_max_func(object, max_id),
        Expr::IndexGet { object, index } => {
            scan_expr_for_max_func(object, max_id);
            scan_expr_for_max_func(index, max_id);
        }
        Expr::PropertySet { object, value, .. } => {
            scan_expr_for_max_func(object, max_id);
            scan_expr_for_max_func(value, max_id);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            scan_expr_for_max_func(object, max_id);
            scan_expr_for_max_func(index, max_id);
            scan_expr_for_max_func(value, max_id);
        }
        Expr::LocalSet(_, v) => scan_expr_for_max_func(v, max_id),
        Expr::Array(items) => {
            for item in items {
                scan_expr_for_max_func(item, max_id);
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                scan_expr_for_max_func(v, max_id);
            }
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                scan_expr_for_max_func(e, max_id);
            }
        }
        Expr::Yield { value: Some(v), .. } => scan_expr_for_max_func(v, max_id),
        // Issue #531: see the matching arm in `scan_expr_for_max_local`.
        // `ops.push(await runOp(..., async () => ...))` buries the user
        // closure inside `ArrayPush.value`. Without this arm, its FuncId
        // is invisible to `compute_max_func_id`, and the generator
        // transform's synthesized iter `next`/`return`/`throw` closures
        // get func_ids that collide with it — codegen emits one LLVM
        // function per func_id, so one definition wins and the other
        // closure invokes it with mismatched captures (null box pointer).
        Expr::ArrayPush { value, .. } => scan_expr_for_max_func(value, max_id),
        Expr::ArrayPushSpread { source, .. } => scan_expr_for_max_func(source, max_id),
        // Array fast-path variants — each carries a `callback` Closure that
        // would otherwise hide its FuncId from the scanner. Without these
        // arms, hoisting a nested `function*` (which my v0.4.146-followup
        // commit added) caused the generator-state-machine transform's
        // `next_func_id` to start lower than the existing user closure
        // ids, producing duplicate FuncIds and a SIGSEGV at codegen.
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            scan_expr_for_max_func(array, max_id);
            scan_expr_for_max_func(callback, max_id);
        }
        Expr::ArraySort { array, comparator } => {
            scan_expr_for_max_func(array, max_id);
            scan_expr_for_max_func(comparator, max_id);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            scan_expr_for_max_func(array, max_id);
            scan_expr_for_max_func(callback, max_id);
            if let Some(i) = initial {
                scan_expr_for_max_func(i, max_id);
            }
        }
        Expr::ArrayToSorted { array, comparator } => {
            scan_expr_for_max_func(array, max_id);
            if let Some(c) = comparator {
                scan_expr_for_max_func(c, max_id);
            }
        }
        // ObjectGroupBy carries a key_fn closure.
        Expr::ObjectGroupBy { items, key_fn } => {
            scan_expr_for_max_func(items, max_id);
            scan_expr_for_max_func(key_fn, max_id);
        }
        _ => {} // Other variants don't carry FuncIds
    }
}
