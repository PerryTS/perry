//! Generator function state machine transformation
//!
//! Transforms generator functions (function*) into regular functions
//! that return iterator objects with a next() method implementing
//! a state machine.
//!
//! The next() method contains a `while(true)` loop with `if (__state === N)`
//! blocks. Non-yielding states set __state and `continue`. Yielding states
//! set __state and `return {value, done: false}`.

use perry_hir::ir::*;
use perry_types::{FuncId, LocalId, Type};

/// Transform all generator functions in a module into state machine form.
pub fn transform_generators(module: &mut Module) {
    // Compute the next available local and func IDs by scanning the module
    let mut next_local_id = compute_max_local_id(module) + 1;
    let mut next_func_id = compute_max_func_id(module) + 1;

    for func in &mut module.functions {
        if func.is_generator {
            transform_generator_function(func, &mut next_local_id, &mut next_func_id);
        }
    }
}

/// Find the maximum local ID used in the module.
fn compute_max_local_id(module: &Module) -> LocalId {
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

fn scan_stmts_for_max_local(stmts: &[Stmt], max_id: &mut LocalId) {
    for stmt in stmts {
        scan_stmt_for_max_local(stmt, max_id);
    }
}

fn scan_stmt_for_max_local(stmt: &Stmt, max_id: &mut LocalId) {
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
fn scan_expr_for_max_local(expr: &Expr, max_id: &mut LocalId) {
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
fn compute_max_func_id(module: &Module) -> FuncId {
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

fn scan_stmts_for_max_func(stmts: &[Stmt], max_id: &mut FuncId) {
    for stmt in stmts {
        scan_stmt_for_max_func(stmt, max_id);
    }
}

fn scan_stmt_for_max_func(stmt: &Stmt, max_id: &mut FuncId) {
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

fn scan_expr_for_max_func(expr: &Expr, max_id: &mut FuncId) {
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

/// Allocate a fresh local ID.
/// Recursively rewrite `Stmt::Let { id, init: Some(...) }` to
/// `Stmt::Expr(LocalSet(id, init))` for any id in `hoisted_ids`. Walks
/// into nested control-flow (For init/body, While body, If branches,
/// Try body/catch/finally, Switch case bodies, Labeled body) so a Let
/// nested inside a for-of's desugared loop body still gets routed
/// through the captured box. Issue #256.
fn rewrite_hoisted_lets_in_stmts(
    stmts: &mut [Stmt],
    hoisted_ids: &std::collections::HashSet<LocalId>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_hoisted_lets_in_stmt(stmt, hoisted_ids);
    }
}

fn rewrite_hoisted_lets_in_stmt(stmt: &mut Stmt, hoisted_ids: &std::collections::HashSet<LocalId>) {
    if let Stmt::Let {
        id,
        init: Some(init_expr),
        ..
    } = stmt
    {
        if hoisted_ids.contains(id) {
            *stmt = Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone())));
            return;
        }
    }
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_hoisted_lets_in_stmts(then_branch, hoisted_ids);
            if let Some(eb) = else_branch {
                rewrite_hoisted_lets_in_stmts(eb, hoisted_ids);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_hoisted_lets_in_stmt(i, hoisted_ids);
            }
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_hoisted_lets_in_stmts(body, hoisted_ids);
            if let Some(c) = catch {
                rewrite_hoisted_lets_in_stmts(&mut c.body, hoisted_ids);
            }
            if let Some(f) = finally {
                rewrite_hoisted_lets_in_stmts(f, hoisted_ids);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_hoisted_lets_in_stmts(&mut case.body, hoisted_ids);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_hoisted_lets_in_stmt(body, hoisted_ids);
        }
        _ => {}
    }
}

fn alloc_local(next_id: &mut u32) -> LocalId {
    let id = *next_id;
    *next_id += 1;
    id
}

/// Create an iterator result object: { value: expr, done: bool }
fn make_iter_result(value: Expr, done: bool) -> Expr {
    Expr::Object(vec![
        ("value".to_string(), value),
        ("done".to_string(), Expr::Bool(done)),
    ])
}

/// Wrap any expression in `Promise.resolve(expr)`. Used by async
/// generators so `gen.next()` returns a Promise the caller can
/// `await`, matching JS async-iterator semantics.
///
/// We build the same HIR shape that `Promise.resolve(x)` sourced
/// from user code would produce (`Call { callee: PropertyGet {
/// GlobalGet(0), "resolve" }, args: [x] }`), which the codegen
/// already recognizes and lowers via `js_promise_resolved`.
fn wrap_in_promise_resolve(value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "resolve".to_string(),
        }),
        args: vec![value],
        type_args: vec![],
    }
}

/// Walk a statement list and wrap every `Stmt::Return(Some(v))`
/// in `Promise.resolve(v)`. Recurses through If/While/For/Try/Switch
/// bodies so nested returns inside the state-machine's if-chain are
/// all covered. Used on `.next()` / `.return()` / `.throw()` closure
/// bodies of async generators.
fn wrap_returns_in_promise(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Return(Some(expr)) => {
                let inner = std::mem::replace(expr, Expr::Undefined);
                *expr = wrap_in_promise_resolve(inner);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                wrap_returns_in_promise(then_branch);
                if let Some(eb) = else_branch {
                    wrap_returns_in_promise(eb);
                }
            }
            Stmt::While { body, .. } => wrap_returns_in_promise(body),
            Stmt::DoWhile { body, .. } => wrap_returns_in_promise(body),
            Stmt::For { body, .. } => wrap_returns_in_promise(body),
            Stmt::Labeled { body, .. } => {
                // Box<Stmt> — recurse over a single-element slice.
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                wrap_returns_in_promise(&mut v);
                **body = v.into_iter().next().unwrap();
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                wrap_returns_in_promise(body);
                if let Some(c) = catch {
                    wrap_returns_in_promise(&mut c.body);
                }
                if let Some(f) = finally {
                    wrap_returns_in_promise(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    wrap_returns_in_promise(&mut case.body);
                }
            }
            _ => {}
        }
    }
}

/// Transform a single generator function into a state machine.
fn transform_generator_function(
    func: &mut Function,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
) {
    // Remember whether this was an async generator (`async function*`).
    // Async generators are still lowered via the same state-machine
    // transform, but:
    //
    //   (1) The outer wrapper must NOT be marked `is_async` anymore —
    //       otherwise `Stmt::Return` in the LLVM backend wraps the
    //       `{ next, return, throw }` iterator object in
    //       `js_promise_resolved`, so `gen.next()` at the call site
    //       dereferences a Promise pointer as if it were an object
    //       and segfaults.
    //
    //   (2) The `.next()` / `.return()` / `.throw()` closure bodies
    //       wrap their iter-result object in a resolved Promise, so
    //       callers can still write `await gen.next()` and get
    //       `{ value, done }` back (matching async-generator semantics
    //       where `.next()` always returns a Promise).
    //
    // A non-async generator keeps the direct iter-result return path.
    let is_async_generator = func.is_async;
    func.is_async = false;

    let state_id = alloc_local(next_local_id);
    let done_id = alloc_local(next_local_id);
    let sent_id = alloc_local(next_local_id); // value passed by caller via next(val)

    // Collect all states from the generator body
    let mut states: Vec<State> = Vec::new();
    let mut current: Vec<Stmt> = Vec::new();
    let mut state_num: u32 = 0;

    // Track IDs allocated during linearization (e.g. yield* delegation vars)
    let local_id_before = *next_local_id;
    // Catches collected during linearization. Each entry is
    // (catch_param_id, catch_body, post_catch_state). Used by the .throw()
    // closure to re-route the exception into the catch handler — and after
    // running the catch body, transition `__gen_state` to `post_catch_state`
    // so the driver's next iter.next(undefined) call resumes execution at
    // the stmts that follow the try/catch in source order (issue #621).
    let mut catches: Vec<(Option<LocalId>, Vec<Stmt>, u32)> = Vec::new();
    linearize_body(
        &func.body,
        &mut states,
        &mut current,
        &mut state_num,
        state_id,
        next_local_id,
        sent_id,
        &mut catches,
    );
    let extra_local_ids: Vec<LocalId> = (local_id_before..*next_local_id).collect();

    // Push final state (code after last yield / end of function)
    states.push(State {
        num: state_num,
        body: current,
        exit: StateExit::Done,
    });

    // Collect hoisted var IDs first so we know which Lets to rewrite
    let hoisted_ids: std::collections::HashSet<LocalId> = collect_hoisted_vars(&func.body)
        .iter()
        .map(|(id, _, _)| *id)
        .collect();

    // Rewrite `Let { id, init: Some(expr) }` → `Expr(LocalSet(id, expr))` for hoisted
    // variables inside state bodies. Without this, the Let creates a fresh local that
    // shadows the captured box, and subsequent mutations in other states don't see the
    // update.
    //
    // Issue #256: must recurse into nested control-flow (For/While/If/Try/Switch
    // bodies). A for-of loop inside a state body desugars to a `for (let i = 0;
    // i < arr.length; ++i) { let v = arr[i]; ... }` shape; without the recursion
    // the inner `let v` and `let i` stay as Lets and create shadow slots that
    // hide the outer captured box. Manifested as `for (const v of arr) sum += v`
    // returning sum=0 inside transformed async functions (test_issue_233).
    for state in &mut states {
        rewrite_hoisted_lets_in_stmts(&mut state.body, &hoisted_ids);
    }

    // Build the if-chain inside while(true)
    let mut while_body: Vec<Stmt> = Vec::new();
    for state in &states {
        let mut case_body = state.body.clone();
        match &state.exit {
            StateExit::Yield { value, next_state } => {
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                case_body.push(Stmt::Return(Some(make_iter_result(value.clone(), false))));
            }
            StateExit::Goto(next_state) => {
                case_body.push(Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(*next_state as f64)),
                )));
                case_body.push(Stmt::Continue);
            }
            StateExit::Done => {
                // Check if the body already has a return (from the user's `return expr`)
                // — at ANY depth, since user code can `return` inside `if` /
                // `try` / `switch` etc. inside a state body. Without the
                // recursion (#594), a user `return X` inside an
                // `if (cond) { return X }` block fell through both rewrites
                // — the bare `Return(X)` reached the iterator caller and
                // `__step_r.done` access threw "Cannot read properties of
                // undefined".
                let has_return = body_contains_return(&case_body);
                if has_return {
                    // Rewrite existing returns to iter results, and prepend done=true
                    // Insert done=true BEFORE the return so it's reachable.
                    // Both passes recurse through nested control flow so a
                    // `return X` at any depth inside this state body is
                    // covered.
                    prepend_done_before_returns(&mut case_body, done_id);
                    rewrite_returns_as_done(&mut case_body);
                    // The body still needs a trailing iter-result if NOT every
                    // path returns (e.g. `if (cond) return X` falls through
                    // when `cond` is false). Append a default
                    // `__gen_done = true; return { value: undefined, done: true }`
                    // unless the LAST stmt is unconditionally a Return.
                    let last_is_return = matches!(case_body.last(), Some(Stmt::Return(_)));
                    if !last_is_return {
                        case_body.push(Stmt::Expr(Expr::LocalSet(
                            done_id,
                            Box::new(Expr::Bool(true)),
                        )));
                        case_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
                    }
                } else {
                    // No explicit return: add done + default return
                    case_body.push(Stmt::Expr(Expr::LocalSet(
                        done_id,
                        Box::new(Expr::Bool(true)),
                    )));
                    case_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
                }
            }
        }

        while_body.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(state_id)),
                right: Box::new(Expr::Number(state.num as f64)),
            },
            then_branch: case_body,
            else_branch: None,
        });
    }

    // Default: done
    while_body.push(Stmt::Expr(Expr::LocalSet(
        done_id,
        Box::new(Expr::Bool(true)),
    )));
    while_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));

    // The next() closure parameter — receives the value from next(val) calls
    let next_param_id = alloc_local(next_local_id);

    // Build next() method body
    let mut next_body = vec![
        // __sent = <param from next(val)>
        Stmt::Expr(Expr::LocalSet(
            sent_id,
            Box::new(Expr::LocalGet(next_param_id)),
        )),
        // if (__done) return { value: undefined, done: true };
        Stmt::If {
            condition: Expr::LocalGet(done_id),
            then_branch: vec![Stmt::Return(Some(make_iter_result(Expr::Undefined, true)))],
            else_branch: None,
        },
        // while (true) { if-chain }
        Stmt::While {
            condition: Expr::Bool(true),
            body: while_body,
        },
    ];
    if is_async_generator {
        wrap_returns_in_promise(&mut next_body);
    }

    // Build the new function body
    let mut new_body: Vec<Stmt> = Vec::new();

    // let __state = 0
    new_body.push(Stmt::Let {
        id: state_id,
        name: "__gen_state".to_string(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Number(0.0)),
    });

    // let __done = false
    new_body.push(Stmt::Let {
        id: done_id,
        name: "__gen_done".to_string(),
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(false)),
    });

    // Hoist variable declarations from the original body
    let hoisted = collect_hoisted_vars(&func.body);
    for (var_id, var_name, var_ty) in &hoisted {
        new_body.push(Stmt::Let {
            id: *var_id,
            name: var_name.clone(),
            ty: var_ty.clone(),
            mutable: true,
            init: None,
        });
    }
    // Also hoist any extra locals allocated during linearization (e.g. yield* delegation)
    for extra_id in &extra_local_ids {
        new_body.push(Stmt::Let {
            id: *extra_id,
            name: format!("__gen_tmp_{}", extra_id),
            ty: Type::Any,
            mutable: true,
            init: None,
        });
    }

    // __sent variable for two-way yield: stores value from next(val) calls
    new_body.push(Stmt::Let {
        id: sent_id,
        name: "__gen_sent".to_string(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Undefined),
    });

    // Build captures: state, done, sent, params, hoisted vars, extra locals
    let mut captures = vec![state_id, done_id, sent_id];
    let mut mutable_captures = vec![state_id, done_id, sent_id];
    for param in &func.params {
        captures.push(param.id);
    }
    for (var_id, _, _) in &hoisted {
        captures.push(*var_id);
        mutable_captures.push(*var_id);
    }
    for extra_id in &extra_local_ids {
        captures.push(*extra_id);
        mutable_captures.push(*extra_id);
    }
    captures.sort();
    captures.dedup();
    mutable_captures.sort();
    mutable_captures.dedup();

    let next_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    // For the `was_plain_async` path we inline `next_body` directly
    // into the step closure (see below) rather than wrap it in a
    // separate `next_closure`. Defer building `next_closure` so we can
    // hand the raw `next_body` to `build_async_step_driver_direct`.

    // Build .return(value) closure — immediately marks done and returns {value, done: true}
    let return_param_id = alloc_local(next_local_id);
    let return_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let mut return_body: Vec<Stmt> = vec![
        Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
        Stmt::Return(Some(make_iter_result(
            Expr::LocalGet(return_param_id),
            true,
        ))),
    ];
    if is_async_generator {
        wrap_returns_in_promise(&mut return_body);
    }
    let return_closure = Expr::Closure {
        func_id: return_func_id_val,
        params: vec![perry_hir::Param {
            id: return_param_id,
            name: "__ret_val".to_string(),
            ty: Type::Any,
            is_rest: false,
            default: None,
        }],
        return_type: Type::Any,
        body: return_body,
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // Build .throw(error) closure.
    // Simplified catch routing: if any catch was seen during linearization, the throw
    // closure assigns the first catch's param to the thrown value and inlines the
    // catch body. Nested / multiple independent catches are not supported yet —
    // the first `catch (e)` block wins. Catches must not contain `yield` themselves
    // (the transform doesn't lift them into the state machine).
    let throw_param_id = alloc_local(next_local_id);
    let throw_func_id_val = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let mut throw_body: Vec<Stmt> = Vec::new();
    if let Some((catch_param_id, catch_body, post_catch_state)) = catches.first().cloned() {
        // Assign catch parameter from the thrown value so the catch body can read `e`.
        if let Some(cp_id) = catch_param_id {
            throw_body.push(Stmt::Expr(Expr::LocalSet(
                cp_id,
                Box::new(Expr::LocalGet(throw_param_id)),
            )));
        }
        // Inline the catch body. Any `Let { id, init: Some(...) }` for a hoisted
        // var is rewritten to LocalSet so the captured box is updated instead of
        // shadowed (mirrors the rewrite in the next() closure above).
        let mut rewritten = catch_body;
        for stmt in &mut rewritten {
            if let Stmt::Let {
                id,
                init: Some(init_expr),
                ..
            } = stmt
            {
                if hoisted_ids.contains(id) {
                    *stmt = Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone())));
                }
            }
        }
        // Rewrite every `Expr::Yield(v)` inside the catch body back to
        // `Expr::Await(v)`. The earlier `transform_async_to_generator`
        // pass blanket-rewrote every `await` to `yield` across the whole
        // function body, including inside catch clauses. But catches are
        // NOT lifted into state-machine states by `linearize_body` — they
        // get stashed verbatim and inlined into the `__async_throw`
        // closure (a regular sync closure, `is_async: false`,
        // `is_generator: false`). Yields in that closure hit the
        // `Expr::Yield => double_literal(0.0)` codegen arm — the await
        // operand is evaluated for side effects, the result is discarded,
        // and `const r = await helperFn()` binds `r = 0`. User code that
        // relies on the awaited value silently sees `0` (or fails
        // downstream with `Cannot read properties of 0`). The flip-back
        // restores the original await semantics; awaits inside the catch
        // body run via the busy-wait codegen path (which drains
        // microtasks while polling promise state — safe because
        // `__async_throw` is invoked from Task::AsyncStep dispatch where
        // the microtask runner is already on the stack and re-entrant
        // drains are no-ops on an empty queue). Doesn't recurse into
        // nested closures (those have their own await/yield context).
        rewrite_yield_to_await_in_stmts(&mut rewritten);
        // Rewrite every `Return X` inside the catch body to
        // `Return make_iter_result(X, true)`. The catch body lives inside
        // the `__async_throw` closure; its `Return` exits __async_throw
        // (not the original async function). For the closure's return
        // value to communicate "done with value X" to the outer step
        // body, it must hand back an iter-result with done=true so the
        // step's post-dispatch `IterResultGetDone` check fires and the
        // step returns `AsyncStepDone(X)` — resolving the function's
        // returned promise with X. Without this rewrite, the catch's
        // `return X` exits __async_throw with the raw value, the scratch
        // slots retain whatever the awaited rejection set them to (the
        // rejected promise), and the step's post-dispatch re-chains the
        // same rejected promise — infinite loop, `try { await Promise.
        // reject(...) } catch { return "caught"; }` hangs forever.
        // `make_iter_result(X, true)` produces `Expr::Object({value:X,
        // done:true})`, which the later `rewrite_iter_results_to_scratch`
        // pass converts to a scratch-slot write + the actual return
        // value. Recurses through If/While/Try/Switch/Labeled so a
        // `return` nested under conditional control flow in the catch
        // body is rewritten too.
        rewrite_catch_returns_to_iter_result(&mut rewritten);
        throw_body.extend(rewritten);
        // Issue #621: after the catch body runs, transition `__gen_state`
        // to the post-try-catch state and return `{ value: undefined,
        // done: false }`. The async-step driver sees done=false and calls
        // iter.next(undefined), which dispatches the post-try state and
        // runs whatever stmts followed the try/catch in source order.
        // (If the catch body itself contained a `return X` the trailing
        // state-set + return are unreachable; the inlined return takes
        // priority.)
        throw_body.push(Stmt::Expr(Expr::LocalSet(
            state_id,
            Box::new(Expr::Number(post_catch_state as f64)),
        )));
        throw_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, false))));
    } else {
        // Issue #619: no user catch was seen during linearization — rethrow so
        // the async-step driver's outer try can re-deliver the error and
        // (per spec) the function's returned Promise rejects. Without this,
        // iter.throw(e) silently returned {done:true} and a sync throw at
        // an `await` position resolved to undefined instead of rejecting.
        throw_body.push(Stmt::Throw(Expr::LocalGet(throw_param_id)));
    }
    if is_async_generator {
        wrap_returns_in_promise(&mut throw_body);
    }
    let throw_closure = Expr::Closure {
        func_id: throw_func_id_val,
        params: vec![perry_hir::Param {
            id: throw_param_id,
            name: "__throw_val".to_string(),
            ty: Type::Any,
            is_rest: false,
            default: None,
        }],
        return_type: Type::Any,
        body: throw_body,
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    if func.was_plain_async {
        // Issue #256: this function was originally a plain async function;
        // the async_to_generator pre-pass rewrote await→yield. Wrap the
        // iterator in an async-step driver so the function returns a
        // Promise that respects spec microtask ordering. See
        // `build_async_step_driver_direct` for the structure.
        //
        // Perf: for plain-async generators we skip the `__iter` object
        // allocation entirely AND the `return` closure (never invoked
        // for plain-async — the spec `return()` method only runs when
        // user code calls it directly on a generator object, which
        // can't happen here since the function returns a Promise, not
        // an iterator). We further FUSE the `__next` body directly
        // into the step closure body — eliminating the per-call
        // `__next` allocation, the closure dispatch, and the captures-
        // box re-lookup that the separate closure-call path required.
        // The throw path stays as a separate closure (cold), since its
        // catch routing is tangled with state-machine post-catch
        // transitions and the fusion benefit there is marginal.
        // When no user try/catch with awaits was lifted by linearize_body
        // (`catches` empty), the throw closure body collapses to a single
        // `throw __throw_val` — pure rethrow, no captures referenced.
        // Skip the closure construction entirely and let the step driver
        // emit `Stmt::Throw(value)` inline in its is-error arm, saving one
        // closure allocation per async-fn invocation (50k/run on the
        // promise_all_chains kernel).
        let throw_closure_for_step = if catches.is_empty() {
            None
        } else {
            let mut tcs = throw_closure;
            rewrite_iter_results_to_scratch(&mut tcs);
            Some(tcs)
        };
        let mut next_body_for_step = next_body;
        rewrite_iter_results_in_stmts(&mut next_body_for_step);
        let wrapper_stmts = build_async_step_driver_direct(
            next_body_for_step,
            next_param_id,
            captures.clone(),
            mutable_captures.clone(),
            throw_closure_for_step,
            next_local_id,
            next_func_id,
        );
        for s in wrapper_stmts {
            new_body.push(s);
        }
        // Keep was_plain_async = true so codegen can populate
        // local_async_funcs and is_promise_expr() correctly recognises
        // calls to this function as Promise-returning (issue #269 fix).
        // The flag is safe to keep set — the generator transform only
        // checks it here, and codegen only reads it.
    } else {
        // Plain generator: build the iterator object and return it directly.
        let next_closure = Expr::Closure {
            func_id: next_func_id_val,
            params: vec![perry_hir::Param {
                id: next_param_id,
                name: "__val".to_string(),
                ty: Type::Any,
                is_rest: false,
                default: None,
            }],
            return_type: Type::Any,
            body: next_body,
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this: false,
            enclosing_class: None,
            is_async: false,
        };
        let iter_obj = Expr::Object(vec![
            ("next".to_string(), next_closure),
            ("return".to_string(), return_closure),
            ("throw".to_string(), throw_closure),
        ]);
        new_body.push(Stmt::Return(Some(iter_obj)));
    }

    func.body = new_body;
    func.is_generator = false;
}

/// Build the async-step driver (issue #256). Returns the statements that
/// take the place of the plain `return iter_obj` that a normal generator
/// would emit. Equivalent TypeScript:
///
/// ```ts
/// const __iter = <iter_obj>;
/// let __step;
/// __step = (value, isError) => {
///     let r;
///     try {
///         r = isError ? __iter.throw(value) : __iter.next(value);
///     } catch (e) {
///         return Promise.reject(e);
///     }
///     if (r.done) return Promise.resolve(r.value);
///     return Promise.resolve(r.value).then(
///         v => __step(v, false),
///         e => __step(e, true),
///     );
/// };
/// return __step(undefined, false);
/// ```
///
/// The two-step `let __step; __step = ...;` pattern is required because
/// Perry's closure-capture analysis silently produces `NaN` for the
/// `const f = (...)=>f(...)` form (verified at v0.5.362 — see issue #256
/// background investigation). With the two-step pattern, the closure
/// captures `__step` mutably; by the time `__step(undefined, false)` is
/// invoked at the outer return site, the box holds the closure value and
/// the recursive references inside `.then` callbacks resolve correctly.
fn build_async_step_driver(
    iter_obj: Expr,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
) -> Vec<Stmt> {
    let iter_id = alloc_local(next_local_id);
    let step_id = alloc_local(next_local_id);

    // Step closure params + locals
    let value_param_id = alloc_local(next_local_id);
    let is_error_param_id = alloc_local(next_local_id);
    let r_id = alloc_local(next_local_id);
    let catch_e_id = alloc_local(next_local_id);

    // Inner .then arrow params
    let then_v_param_id = alloc_local(next_local_id);
    let then_e_param_id = alloc_local(next_local_id);

    let step_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let then_v_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };
    let then_e_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };

    let any_ty = Type::Any;
    let bool_ty = Type::Boolean;

    // Helper builders
    let promise_global = || Expr::GlobalGet(0);
    let promise_resolve = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "resolve".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };
    let promise_reject = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "reject".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };

    // Build the two .then arrows: (v) => __step(v, false) and (e) => __step(e, true)
    let then_v_arrow = Expr::Closure {
        func_id: then_v_func_id,
        params: vec![perry_hir::Param {
            id: then_v_param_id,
            name: "__step_v".to_string(),
            ty: any_ty.clone(),
            is_rest: false,
            default: None,
        }],
        return_type: any_ty.clone(),
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::LocalGet(step_id)),
            args: vec![Expr::LocalGet(then_v_param_id), Expr::Bool(false)],
            type_args: vec![],
        }))],
        captures: vec![step_id],
        mutable_captures: vec![step_id],
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };
    let then_e_arrow = Expr::Closure {
        func_id: then_e_func_id,
        params: vec![perry_hir::Param {
            id: then_e_param_id,
            name: "__step_e".to_string(),
            ty: any_ty.clone(),
            is_rest: false,
            default: None,
        }],
        return_type: any_ty.clone(),
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::LocalGet(step_id)),
            args: vec![Expr::LocalGet(then_e_param_id), Expr::Bool(true)],
            type_args: vec![],
        }))],
        captures: vec![step_id],
        mutable_captures: vec![step_id],
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // step body
    //   let r;
    //   try {
    //       r = isError ? __iter.throw(value) : __iter.next(value);
    //   } catch (e) {
    //       return Promise.reject(e);
    //   }
    //   if (r.done) return Promise.resolve(r.value);
    //   return Promise.resolve(r.value).then(<then_v>, <then_e>);
    let iter_throw_call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "throw".to_string(),
        }),
        args: vec![Expr::LocalGet(value_param_id)],
        type_args: vec![],
    };
    let iter_next_call = Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(iter_id)),
            property: "next".to_string(),
        }),
        args: vec![Expr::LocalGet(value_param_id)],
        type_args: vec![],
    };
    let dispatch_iter = Expr::Conditional {
        condition: Box::new(Expr::LocalGet(is_error_param_id)),
        then_expr: Box::new(iter_throw_call),
        else_expr: Box::new(iter_next_call),
    };

    // Perf: the generator's `next()` writes its iter-result into a
    // thread-local scratch (rewritten by `rewrite_iter_results_to_scratch`)
    // and returns `undefined`, so we never read `r.done` / `r.value`
    // off a heap-allocated `{value, done}` object. Drop the local `r`
    // and replace those PropertyGets with the scratch getters.
    let _ = r_id; // kept for ABI/local-id parity; unused on the fast path
    let step_body: Vec<Stmt> = vec![
        // try { __iter.next(value); } catch (e) {
        //     if (isError) return Promise.reject(e);
        //     return __step(e, true);
        // }
        Stmt::Try {
            body: vec![Stmt::Expr(dispatch_iter)],
            catch: Some(CatchClause {
                param: Some((catch_e_id, "__step_catch_e".to_string())),
                body: vec![
                    Stmt::If {
                        condition: Expr::LocalGet(is_error_param_id),
                        then_branch: vec![Stmt::Return(Some(promise_reject(Expr::LocalGet(
                            catch_e_id,
                        ))))],
                        else_branch: None,
                    },
                    Stmt::Return(Some(Expr::Call {
                        callee: Box::new(Expr::LocalGet(step_id)),
                        args: vec![Expr::LocalGet(catch_e_id), Expr::Bool(true)],
                        type_args: vec![],
                    })),
                ],
            }),
            finally: None,
        },
        // if (js_iter_result_get_done()) return Promise.resolve(js_iter_result_get_value());
        Stmt::If {
            condition: Expr::IterResultGetDone,
            then_branch: vec![Stmt::Return(Some(promise_resolve(
                Expr::IterResultGetValue,
            )))],
            else_branch: None,
        },
        // return Promise.resolve(js_iter_result_get_value()).then(<then_v>, <then_e>);
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(promise_resolve(Expr::IterResultGetValue)),
                property: "then".to_string(),
            }),
            args: vec![then_v_arrow, then_e_arrow],
            type_args: vec![],
        })),
    ];

    let step_closure = Expr::Closure {
        func_id: step_func_id,
        params: vec![
            perry_hir::Param {
                id: value_param_id,
                name: "__step_value".to_string(),
                ty: any_ty.clone(),
                is_rest: false,
                default: None,
            },
            perry_hir::Param {
                id: is_error_param_id,
                name: "__step_is_error".to_string(),
                ty: bool_ty.clone(),
                is_rest: false,
                default: None,
            },
        ],
        return_type: any_ty.clone(),
        body: step_body,
        captures: vec![iter_id, step_id],
        mutable_captures: vec![step_id],
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // Outer wrapper:
    //   let __iter = <iter_obj>;
    //   let __step;        // declared, init=undefined
    //   __step = <step_closure>;
    //   return __step(undefined, false);
    vec![
        Stmt::Let {
            id: iter_id,
            name: "__async_iter".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(iter_obj),
        },
        Stmt::Let {
            id: step_id,
            name: "__async_step".to_string(),
            ty: any_ty.clone(),
            mutable: true,
            init: None,
        },
        Stmt::Expr(Expr::LocalSet(step_id, Box::new(step_closure))),
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::LocalGet(step_id)),
            args: vec![Expr::Undefined, Expr::Bool(false)],
            type_args: vec![],
        })),
    ]
}

/// Like `build_async_step_driver` but skips the `__iter` object
/// allocation entirely. Used for `was_plain_async = true` generators
/// where the iter object is never observable from user code (the
/// async-step driver wraps the generator into a Promise-returning
/// shape; the user never holds an iterator handle). Captures the
/// next/throw closures directly as locals so the step body's
/// `__iter.next(value)` becomes a single LocalGet+Call instead of a
/// PropertyGet+Call. Also drops the `return` closure (never invoked
/// for plain-async — spec `gen.return()` can't be called when the
/// function returns a Promise instead of an iterator).
fn build_async_step_driver_direct(
    next_body: Vec<Stmt>,
    next_param_id: LocalId,
    next_captures: Vec<LocalId>,
    next_mutable_captures: Vec<LocalId>,
    throw_closure_expr: Option<Expr>,
    next_local_id: &mut u32,
    next_func_id: &mut u32,
) -> Vec<Stmt> {
    // When `throw_closure_expr` is None, the function had no awaiting
    // try/catch so the throw path is a plain rethrow — we inline it
    // directly into the step body and skip the per-invocation
    // `__async_throw` allocation entirely.
    let throw_id = throw_closure_expr
        .as_ref()
        .map(|_| alloc_local(next_local_id));
    // #691 Phase 2: step closure no longer captures itself. Body
    // uses `Expr::CurrentStepClosure` (reads INLINE_TRAP.current_step
    // TLS) wherever it previously did `LocalGet(step_id)`. The
    // wrapper still needs a local to hand the freshly-constructed
    // closure to `Expr::AsyncFirstCall`, but it's a regular immutable
    // let (no `js_box_alloc`).
    let step_id = alloc_local(next_local_id);

    // Step closure params + locals
    let value_param_id = alloc_local(next_local_id);
    let is_error_param_id = alloc_local(next_local_id);
    let catch_e_id = alloc_local(next_local_id);

    let step_func_id = {
        let id = *next_func_id;
        *next_func_id += 1;
        id
    };

    let any_ty = Type::Any;
    let bool_ty = Type::Boolean;

    let promise_global = || Expr::GlobalGet(0);
    let promise_resolve = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "resolve".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };
    let promise_reject = |arg: Expr| Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(promise_global()),
            property: "reject".to_string(),
        }),
        args: vec![arg],
        type_args: vec![],
    };

    // Rewrite every Return inside next_body to LabeledBreak(__step_done)
    // so they fall through to step's post-dispatch code instead of
    // exiting step entirely. The IterResultSet expression sets the
    // (value, done) TLS slots; LabeledBreak escapes the inlined body.
    let step_done_label = "__step_done".to_string();
    let mut next_body = next_body;
    rewrite_returns_to_labeled_break(&mut next_body, &step_done_label);

    // The inlined next_body references `next_param_id` (the original
    // `__val` parameter of the next closure). After fusion that ID
    // becomes a local of step; we initialize it from value_param_id
    // before running the body.
    let mut else_branch: Vec<Stmt> = vec![Stmt::Let {
        id: next_param_id,
        name: "__val".to_string(),
        ty: any_ty.clone(),
        mutable: false,
        init: Some(Expr::LocalGet(value_param_id)),
    }];
    else_branch.extend(next_body);

    // step body
    //   try {
    //     "__step_done": do {
    //        if (isError) {
    //            // when no user catch: throw value; (caught by outer try)
    //            // when user catch: __throw(value);
    //        } else { let __val = value; <next_body inlined> }
    //     } while (false);
    //   } catch (e) {
    //     if (isError) return Promise.reject(e);
    //     return __step(e, true);
    //   }
    //   if (js_iter_result_get_done()) return Promise.resolve(js_iter_result_get_value());
    //   return AsyncStepChain(js_iter_result_get_value(), __step);
    let throw_arm: Vec<Stmt> = if let Some(tid) = throw_id {
        vec![Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(tid)),
            args: vec![Expr::LocalGet(value_param_id)],
            type_args: vec![],
        })]
    } else {
        // No __async_throw closure was constructed (callee passed None).
        // The throw body would have been a plain rethrow, so inline it:
        // the outer try/catch re-enters __step(e, true) which then hits
        // this same path with isError=true a second time, and the catch
        // arm returns Promise.reject (the `if (isError)` short-circuit).
        vec![Stmt::Throw(Expr::LocalGet(value_param_id))]
    };
    let dispatch_inner = Stmt::If {
        condition: Expr::LocalGet(is_error_param_id),
        then_branch: throw_arm,
        else_branch: Some(else_branch),
    };

    // Wrap dispatch in `do { dispatch; } while(false)` so the
    // wrapping `Stmt::Labeled` registers its label on a loop —
    // codegen's `label_targets` map is populated only for for/while/
    // do-while bodies, so plain `Stmt::Labeled { body: If }` would
    // leave LabeledBreak with no jump target. DoWhile with a constant-
    // false condition runs the body exactly once.
    let labeled_loop = Stmt::Labeled {
        label: step_done_label.clone(),
        body: Box::new(Stmt::DoWhile {
            body: vec![dispatch_inner],
            condition: Expr::Bool(false),
        }),
    };

    let step_body: Vec<Stmt> = vec![
        Stmt::Try {
            body: vec![labeled_loop],
            catch: Some(CatchClause {
                param: Some((catch_e_id, "__step_catch_e".to_string())),
                body: vec![
                    Stmt::If {
                        condition: Expr::LocalGet(is_error_param_id),
                        then_branch: vec![Stmt::Return(Some(promise_reject(Expr::LocalGet(
                            catch_e_id,
                        ))))],
                        else_branch: None,
                    },
                    // #691 Phase 2: recursive `__step(catch_e, true)`
                    // re-entry now resolves the callee through TLS
                    // instead of a captured local. lower_call
                    // recognizes CurrentStepClosure as a callee and
                    // dispatches via `js_closure_call2` just like the
                    // captured-local path.
                    Stmt::Return(Some(Expr::Call {
                        callee: Box::new(Expr::CurrentStepClosure),
                        args: vec![Expr::LocalGet(catch_e_id), Expr::Bool(true)],
                        type_args: vec![],
                    })),
                ],
            }),
            finally: None,
        },
        Stmt::If {
            condition: Expr::IterResultGetDone,
            // Optimized: AsyncStepDone reuses INLINE_TRAP_NEXT instead
            // of allocating a fresh `Promise.resolve(value)` Promise.
            // Saves one js_promise_resolved alloc per async function
            // call (50k/run on promise_all_chains). #691 Phase 2:
            // step closure ptr now read from TLS.
            then_branch: vec![Stmt::Return(Some(Expr::AsyncStepDone {
                value: Box::new(Expr::IterResultGetValue),
                step_closure: Box::new(Expr::CurrentStepClosure),
            }))],
            else_branch: None,
        },
        Stmt::Return(Some(Expr::AsyncStepChain {
            value: Box::new(Expr::IterResultGetValue),
            step_closure: Box::new(Expr::CurrentStepClosure),
        })),
    ];

    // step closure captures = next_captures + [throw_id?]
    // #691 Phase 2: step_id is NOT captured — the body reads its own
    // pointer via `Expr::CurrentStepClosure` (INLINE_TRAP.current_step
    // TLS). This saves one capture slot per step closure and removes
    // the per-invocation `js_box_alloc` for step_id.
    let mut step_captures: Vec<LocalId> = next_captures;
    if let Some(tid) = throw_id {
        step_captures.push(tid);
    }
    step_captures.sort();
    step_captures.dedup();
    let step_mut_captures: Vec<LocalId> = next_mutable_captures;

    let step_closure = Expr::Closure {
        func_id: step_func_id,
        params: vec![
            perry_hir::Param {
                id: value_param_id,
                name: "__step_value".to_string(),
                ty: any_ty.clone(),
                is_rest: false,
                default: None,
            },
            perry_hir::Param {
                id: is_error_param_id,
                name: "__step_is_error".to_string(),
                ty: bool_ty.clone(),
                is_rest: false,
                default: None,
            },
        ],
        return_type: any_ty.clone(),
        body: step_body,
        captures: step_captures,
        mutable_captures: step_mut_captures,
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // Outer wrapper:
    //   let __throw = <throw_closure>;   // omitted when throw_id is None
    //   let __step = <step_closure>;     // #691 Phase 2: immutable,
    //                                    //   no js_box_alloc
    //   return AsyncFirstCall(__step);   // sets TLS, calls
    //                                    //   step(undefined, false)
    let mut wrapper: Vec<Stmt> = Vec::with_capacity(3);
    if let (Some(tid), Some(tc_expr)) = (throw_id, throw_closure_expr) {
        wrapper.push(Stmt::Let {
            id: tid,
            name: "__async_throw".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(tc_expr),
        });
    }
    wrapper.extend([
        Stmt::Let {
            id: step_id,
            name: "__async_step".to_string(),
            ty: any_ty.clone(),
            mutable: false,
            init: Some(step_closure),
        },
        Stmt::Return(Some(Expr::AsyncFirstCall {
            step_closure: Box::new(Expr::LocalGet(step_id)),
        })),
    ]);
    wrapper
}

struct State {
    num: u32,
    body: Vec<Stmt>,
    exit: StateExit,
}

enum StateExit {
    /// Yield a value and advance to next_state
    Yield { value: Expr, next_state: u32 },
    /// Goto another state (non-yielding transition)
    Goto(u32),
    /// Function is done
    Done,
}

/// Linearize the generator body into a sequence of states.
/// Splits at yield points and handles for-loops with yields.
fn linearize_body(
    stmts: &[Stmt],
    states: &mut Vec<State>,
    current: &mut Vec<Stmt>,
    state_num: &mut u32,
    state_id: LocalId,
    #[allow(unused_variables)] next_local_id: &mut u32,
    sent_id: LocalId,
    catches: &mut Vec<(Option<LocalId>, Vec<Stmt>, u32)>,
) {
    for stmt in stmts {
        match stmt {
            // yield* delegation: iterate the inner iterator and yield each value
            Stmt::Expr(Expr::Yield {
                value: Some(inner),
                delegate: true,
            }) => {
                // Desugar yield* into:
                //   let __del_iter = inner_expr;  (inner is a generator call)
                //   let __del_result = __del_iter.next();
                //   while (!__del_result.done) {
                //     yield __del_result.value;
                //     __del_result = __del_iter.next();
                //   }
                // We don't actually need real vars — we can inline this as states.
                // But the simplest approach: expand into statements and re-linearize.
                let del_iter_id = alloc_local(next_local_id);
                let del_result_id = alloc_local(next_local_id);

                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };

                // Add hoisted var declarations to current (they'll be emitted in the state body)
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(*inner.clone()),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(next_call.clone()),
                )));

                // Build the while loop with yield
                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(del_result_id, Box::new(next_call))),
                ];

                let while_stmt = Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: while_body,
                };

                // Now linearize the expanded while (it contains a yield, so the while handler picks it up)
                linearize_body(
                    &[while_stmt],
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                );
            }

            // yield expr at statement level (non-delegate)
            Stmt::Expr(Expr::Yield {
                value,
                delegate: false,
            })
            | Stmt::Expr(Expr::Yield { value, .. }) => {
                let yield_val = value
                    .as_ref()
                    .map(|v| *v.clone())
                    .unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
            }

            // return (yield expr)  — i.e. `return await x` after async→generator rewrite
            // The yield must be emitted as a real yield state so the async-step driver can
            // await the expression; the continuation state then returns {value: __sent, done: true}
            // where __sent is the resolved value delivered back by the step driver.
            Stmt::Return(Some(yield_expr @ Expr::Yield { .. })) => {
                let yield_val = if let Expr::Yield { value, .. } = yield_expr {
                    value
                        .as_ref()
                        .map(|v| *v.clone())
                        .unwrap_or(Expr::Undefined)
                } else {
                    unreachable!()
                };
                // Flush pre-return code as a yield state
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
                // Continuation state: return { value: __sent, done: true }
                current.push(Stmt::Return(Some(make_iter_result(
                    Expr::LocalGet(sent_id),
                    true,
                ))));
                let cont_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: cont_state,
                    body: std::mem::take(current),
                    exit: StateExit::Done,
                });
            }

            // return expr (terminal - ends the generator)
            Stmt::Return(val) => {
                // Add the return with {value: expr, done: true} wrapping
                let return_val = val.clone().unwrap_or(Expr::Undefined);
                current.push(Stmt::Return(Some(make_iter_result(return_val, true))));
                // Flush current as a terminal state
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Done,
                });
            }

            // For-loop containing yield(s)
            Stmt::For {
                init,
                condition,
                update,
                body,
            } if body_contains_yield(body) => {
                // State N: pre-loop code + init, goto condition check
                let init_state = *state_num;
                *state_num += 1;
                let mut init_body = std::mem::take(current);
                // Add init statement (typically `let i = start`)
                // But we need to convert it to an assignment since the var is hoisted
                if let Some(init_stmt) = init {
                    match init_stmt.as_ref() {
                        Stmt::Let {
                            id,
                            init: Some(init_expr),
                            ..
                        } => {
                            init_body
                                .push(Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone()))));
                        }
                        other => init_body.push(other.clone()),
                    }
                }
                let cond_state = *state_num;
                states.push(State {
                    num: init_state,
                    body: init_body,
                    exit: StateExit::Goto(cond_state),
                });

                // State N+1: condition check
                *state_num += 1;
                let body_state = *state_num;
                // Condition check: if true, fall through to body; if false, done
                let cond_body = if let Some(cond) = condition {
                    // Build the done return as part of the else branch
                    vec![Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(cond.clone()),
                        },
                        then_branch: vec![
                            // Loop ended - jump past the loop
                            Stmt::Expr(Expr::LocalSet(
                                state_id,
                                Box::new(Expr::Number(0.0)), // placeholder, fixed below
                            )),
                            // Continue the while(true) so the Goto exit doesn't overwrite state
                            Stmt::Continue,
                        ],
                        else_branch: None,
                    }]
                } else {
                    vec![]
                };
                // We'll fix the after-loop state number after processing body
                states.push(State {
                    num: cond_state,
                    body: cond_body,
                    exit: StateExit::Goto(body_state),
                });

                // Pre-rewrite the body so any top-level `break` / `continue`
                // inside (but NOT inside nested loops / switch / closure)
                // becomes a placeholder state assignment + dispatch-continue.
                // After body processing we know what the loop's break and
                // continue targets are; the fix-up pass below replaces the
                // sentinel numbers. Without this, `for (let i ...) { await
                // x; if (cond) break; }` lowers the inner `break` as a raw
                // `Stmt::Break` — the state-machine-emitted while(true) loop
                // exits early, then the post-dispatch code reads the scratch
                // iter-result set by the await (done=false), returns
                // `AsyncStepChain(stale_promise, step)`, and the chain loops
                // forever on a stale promise. Same shape covers `continue`
                // skipping the body tail without going through the update.
                let body_states_before = states.len();
                let body_current_before = current.len();
                let mut body_rewritten = body.clone();
                rewrite_break_continue_in_stmts(&mut body_rewritten, state_id);

                // Process loop body (may contain yields)
                linearize_body(
                    &body_rewritten,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                );

                // Body-tail state: contains the user body's residual stmts
                // (everything after the last yield in the body). On fall-
                // through it transitions to `update_state` so the for-loop
                // semantics (run body, run update, re-check cond) hold.
                let tail_state = *state_num;
                *state_num += 1;
                let tail_body = std::mem::take(current);

                // `continue` target: a dedicated state that ONLY runs the
                // update expression and then jumps back to cond. Distinct
                // from `tail_state` so a user-`continue` from inside the
                // body skips the post-continue body residual but still runs
                // the for-loop's update expression. Without this split, a
                // user `continue` written inside the post-yield body region
                // would land back in the tail state, re-execute the body
                // residual, and (depending on guard placement) loop forever
                // on the same iteration.
                let update_state = *state_num;
                *state_num += 1;
                let mut update_body: Vec<Stmt> = Vec::new();
                if let Some(upd) = update {
                    update_body.push(Stmt::Expr(upd.clone()));
                }

                // Push tail_state pointing at update_state.
                states.push(State {
                    num: tail_state,
                    body: tail_body,
                    exit: StateExit::Goto(update_state),
                });
                // Push update_state pointing at cond_state.
                states.push(State {
                    num: update_state,
                    body: update_body,
                    exit: StateExit::Goto(cond_state),
                });

                // Fix up the condition state's false branch to jump to after-loop state
                let after_loop_state = *state_num;
                // Find the condition state and fix the placeholder
                for state in states.iter_mut() {
                    if state.num == cond_state {
                        fix_placeholder_state(&mut state.body, state_id, after_loop_state);
                    }
                }
                // Fix break / continue placeholders that landed in the
                // newly-created states (from body and tail_state) or in
                // the trailing `current` buffer (none for For — tail_state
                // already drained it, but covered for symmetry).
                fix_break_continue_sentinels(
                    &mut states[body_states_before..],
                    state_id,
                    after_loop_state,
                    update_state,
                );
                fix_break_continue_sentinels_in_stmts(
                    &mut current[body_current_before..],
                    state_id,
                    after_loop_state,
                    update_state,
                );
            }

            // While-loop containing yield(s) - similar to for-loop
            Stmt::While {
                condition,
                body: while_body,
            } if body_contains_yield(while_body) => {
                // Pre-loop code gets its own state (if non-empty)
                let pre_body = std::mem::take(current);
                if !pre_body.is_empty() {
                    let pre_state = *state_num;
                    *state_num += 1;
                    let cond_target = *state_num; // will be the cond_state below
                    states.push(State {
                        num: pre_state,
                        body: pre_body,
                        exit: StateExit::Goto(cond_target),
                    });
                }

                let cond_state = *state_num;
                *state_num += 1;

                let body_state = *state_num;
                // Condition check
                states.push(State {
                    num: cond_state,
                    body: vec![Stmt::If {
                        condition: Expr::Unary {
                            op: UnaryOp::Not,
                            operand: Box::new(condition.clone()),
                        },
                        then_branch: vec![
                            Stmt::Expr(Expr::LocalSet(
                                state_id,
                                Box::new(Expr::Number(0.0)), // placeholder
                            )),
                            Stmt::Continue,
                        ],
                        else_branch: None,
                    }],
                    exit: StateExit::Goto(body_state),
                });

                // Pre-rewrite while body's break/continue sentinels.
                // For a while-loop, `continue` jumps back to the condition
                // state (no separate update); `break` jumps to after_loop.
                let while_states_before = states.len();
                let while_current_before = current.len();
                let mut while_body_rewritten = while_body.clone();
                rewrite_break_continue_in_stmts(&mut while_body_rewritten, state_id);

                // Process body
                linearize_body(
                    &while_body_rewritten,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                );

                // After body, goto condition
                let loop_back_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: loop_back_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(cond_state),
                });

                // Fix placeholder
                let after_loop = *state_num;
                for state in states.iter_mut() {
                    if state.num == cond_state {
                        fix_placeholder_state(&mut state.body, state_id, after_loop);
                    }
                }
                // Fix break / continue sentinels inside the while-body states
                // (`continue` here jumps to the cond_state; `break` jumps to
                // after_loop).
                fix_break_continue_sentinels(
                    &mut states[while_states_before..],
                    state_id,
                    after_loop,
                    cond_state,
                );
                fix_break_continue_sentinels_in_stmts(
                    &mut current[while_current_before..],
                    state_id,
                    after_loop,
                    cond_state,
                );
            }

            // Try-catch containing yield(s) — linearize the try body directly and
            // stash the catch body so the .throw() closure can inline it.
            // Limitations: no per-state exception handler tracking, so only the
            // first catch encountered will run on .throw(). Catches themselves
            // must not yield — they run to completion inside the throw closure.
            Stmt::Try {
                body,
                catch,
                finally,
            } if body_contains_yield(body)
                || finally.as_ref().is_some_and(|f| body_contains_yield(f)) =>
            {
                // Issue #256: widen the guard to also fire when yields live ONLY
                // in the finally block. `await using` desugars to
                // `try { body } finally { await dispose() }` — the body may have
                // no awaits while the finally has one, and pre-fix this fell into
                // the catch-all which compiled the whole try/finally as a single
                // unit inside one state — the yield-in-finally then hit the
                // codegen `Expr::Yield => double_literal(0.0)` arm and the await
                // was silently fire-and-forgotten.
                if body_contains_yield(body) {
                    // Linearize the try body directly (yields become normal states)
                    linearize_body(
                        body,
                        states,
                        current,
                        state_num,
                        state_id,
                        next_local_id,
                        sent_id,
                        catches,
                    );
                } else {
                    // Body has no yields: push as-is to current state.
                    for s in body {
                        current.push(s.clone());
                    }
                }

                // Issue #621: if the try has a catch handler, split the
                // post-await happy-path continuation (currently in `current`)
                // from the post-try-catch continuation. Stmts that follow
                // the LAST yield in the try body should only run on the
                // happy path; the throw path runs the catch body and then
                // resumes at the stmts AFTER the try/catch. Without this
                // split, both paths land in the same state and the catch
                // path incorrectly runs the post-await stmts.
                if catch.is_some() {
                    if !current.is_empty() {
                        let happy_state = *state_num;
                        *state_num += 1;
                        let goto_target = *state_num;
                        states.push(State {
                            num: happy_state,
                            body: std::mem::take(current),
                            exit: StateExit::Goto(goto_target),
                        });
                    }
                }
                let post_catch_state = *state_num;

                // Stash the catch so transform_generator_function can inline it
                // into the .throw() closure later.
                if let Some(catch_clause) = catch {
                    let param_id = catch_clause.param.as_ref().map(|(id, _)| *id);
                    catches.push((param_id, catch_clause.body.clone(), post_catch_state));
                }

                // Finally block: linearize if it has yields (await-using path),
                // otherwise push as-is.
                if let Some(fin) = finally {
                    if body_contains_yield(fin) {
                        linearize_body(
                            fin,
                            states,
                            current,
                            state_num,
                            state_id,
                            next_local_id,
                            sent_id,
                            catches,
                        );
                    } else {
                        for s in fin {
                            current.push(s.clone());
                        }
                    }
                }
            }

            // If-statement containing yield(s) — linearize both branches
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } if body_contains_yield(then_branch)
                || else_branch.as_ref().is_some_and(|e| body_contains_yield(e)) =>
            {
                // Flush pre-if code as its own state
                let pre_state = *state_num;
                *state_num += 1;
                let pre_body = std::mem::take(current);

                let then_state = *state_num;
                // We'll figure out else_state and after_state as we go
                // For now, emit the condition check with a branch
                let else_state_placeholder = 0u32; // fixed below

                states.push(State {
                    num: pre_state,
                    body: {
                        let mut b = pre_body;
                        b.push(Stmt::If {
                            condition: condition.clone(),
                            then_branch: vec![
                                Stmt::Expr(Expr::LocalSet(
                                    state_id,
                                    Box::new(Expr::Number(then_state as f64)),
                                )),
                                Stmt::Continue,
                            ],
                            else_branch: Some(vec![
                                Stmt::Expr(Expr::LocalSet(
                                    state_id,
                                    Box::new(Expr::Number(else_state_placeholder as f64)),
                                )),
                                Stmt::Continue,
                            ]),
                        });
                        b
                    },
                    exit: StateExit::Done, // won't be reached (branches above jump)
                });

                // Linearize then-branch
                linearize_body(
                    then_branch,
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                );
                // After then-branch, flush into a goto-after state
                let then_end_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: then_end_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(0), // placeholder for after_state
                });

                // Linearize else-branch
                let else_state = *state_num;
                if let Some(else_stmts) = else_branch {
                    linearize_body(
                        else_stmts,
                        states,
                        current,
                        state_num,
                        state_id,
                        next_local_id,
                        sent_id,
                        catches,
                    );
                }
                let else_end_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: else_end_state,
                    body: std::mem::take(current),
                    exit: StateExit::Goto(0), // placeholder for after_state
                });

                let after_state = *state_num;

                // Fix else_state_placeholder in pre_state
                for state in states.iter_mut() {
                    if state.num == pre_state {
                        fix_placeholder_state(&mut state.body, state_id, else_state);
                    }
                }
                // Fix then_end → after and else_end → after
                for state in states.iter_mut() {
                    if state.num == then_end_state || state.num == else_end_state {
                        if let StateExit::Goto(ref mut target) = state.exit {
                            if *target == 0 {
                                *target = after_state;
                            }
                        }
                    }
                }
            }

            // Let with yield* delegation initializer: `const x = yield* inner()`.
            // Per spec: x receives the value returned by inner when it completes
            // (the `value` field of `{value, done: true}`). Without this arm
            // the catch-all below treated `yield*` like `yield`, sending the
            // iterator object itself as the yielded value and assigning __sent
            // (typically undefined) to x — both wrong.
            Stmt::Let {
                id,
                init:
                    Some(Expr::Yield {
                        value: Some(inner),
                        delegate: true,
                    }),
                mutable,
                ty,
                name,
            } => {
                let del_iter_id = alloc_local(next_local_id);
                let del_result_id = alloc_local(next_local_id);

                let next_call = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_iter_id)),
                        property: "next".to_string(),
                    }),
                    args: vec![],
                    type_args: vec![],
                };

                current.push(Stmt::Expr(Expr::LocalSet(
                    del_iter_id,
                    Box::new(*inner.clone()),
                )));
                current.push(Stmt::Expr(Expr::LocalSet(
                    del_result_id,
                    Box::new(next_call.clone()),
                )));

                let while_body = vec![
                    Stmt::Expr(Expr::Yield {
                        value: Some(Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "value".to_string(),
                        })),
                        delegate: false,
                    }),
                    Stmt::Expr(Expr::LocalSet(del_result_id, Box::new(next_call))),
                ];

                let while_stmt = Stmt::While {
                    condition: Expr::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(del_result_id)),
                            property: "done".to_string(),
                        }),
                    },
                    body: while_body,
                };

                linearize_body(
                    &[while_stmt],
                    states,
                    current,
                    state_num,
                    state_id,
                    next_local_id,
                    sent_id,
                    catches,
                );

                // After the loop, the iterator's final `value` (from
                // {value, done:true}) becomes the value of `yield* expr`.
                current.push(Stmt::Let {
                    id: *id,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(del_result_id)),
                        property: "value".to_string(),
                    }),
                    mutable: *mutable,
                    ty: ty.clone(),
                    name: name.clone(),
                });
            }

            // Let with yield initializer: `const x = yield expr` (two-way yield)
            // After resuming, `x` receives the value passed by the caller via next(val),
            // which is stored in __sent by the next() closure preamble.
            Stmt::Let {
                id,
                init: Some(Expr::Yield { value, .. }),
                mutable,
                ty,
                name,
            } => {
                let yield_val = value
                    .as_ref()
                    .map(|v| *v.clone())
                    .unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield {
                        value: yield_val,
                        next_state: *state_num,
                    },
                });
                // Assign __sent (the value from next(val)) to the target local
                current.push(Stmt::Let {
                    id: *id,
                    init: Some(Expr::LocalGet(sent_id)),
                    mutable: *mutable,
                    ty: ty.clone(),
                    name: name.clone(),
                });
            }

            // Regular statement (no yield) - accumulate
            other => {
                current.push(other.clone());
            }
        }
    }
}

/// Fix the placeholder `0.0` state number in condition branches.
/// Sentinel state-number for `Stmt::Break` placeholders. Chosen to fall well
/// outside any legitimate state count (state numbers grow from 0; even huge
/// async functions stay in the thousands). After body linearization completes,
/// `fix_break_continue_sentinels` swaps every occurrence with the loop's
/// real `after_loop` state number.
const BREAK_SENTINEL: f64 = 1_000_001.0;
/// Sentinel for `Stmt::Continue`. Swapped with the loop's `update_state`
/// (for-loops) or `cond_state` (while-loops) post-linearization.
const CONTINUE_SENTINEL: f64 = 1_000_002.0;

/// Walk a body and rewrite every top-level `Stmt::Break` / `Stmt::Continue`
/// into `[LocalSet(state_id, <sentinel>), Stmt::Continue]`. The trailing
/// `Stmt::Continue` is the state-machine's dispatch-loop continue, which
/// re-enters the while(true) and re-dispatches on the new state. Stops at
/// nested loop / switch / closure boundaries — their own break/continue
/// belong to those constructs, not to us.
fn rewrite_break_continue_in_stmts(stmts: &mut Vec<Stmt>, state_id: LocalId) {
    let mut i = 0;
    while i < stmts.len() {
        let stmt = std::mem::replace(&mut stmts[i], Stmt::Continue);
        match stmt {
            Stmt::Break => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(BREAK_SENTINEL)),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            Stmt::Continue => {
                stmts[i] = Stmt::Expr(Expr::LocalSet(
                    state_id,
                    Box::new(Expr::Number(CONTINUE_SENTINEL)),
                ));
                stmts.insert(i + 1, Stmt::Continue);
                i += 2;
            }
            mut other => {
                rewrite_break_continue_in_stmt(&mut other, state_id);
                stmts[i] = other;
                i += 1;
            }
        }
    }
}

fn rewrite_break_continue_in_stmt(stmt: &mut Stmt, state_id: LocalId) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_break_continue_in_stmts(then_branch, state_id);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_break_continue_in_stmts(eb, state_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_break_continue_in_stmts(body, state_id);
            if let Some(c) = catch.as_mut() {
                rewrite_break_continue_in_stmts(&mut c.body, state_id);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_break_continue_in_stmts(f, state_id);
            }
        }
        // Inside nested loops / switch / labeled / closure expressions, the
        // user's `break`/`continue` belongs to that construct and not to the
        // outer loop the state machine is unrolling. Leave them as-is so the
        // inner linearize_body (if it yields) / regular codegen (if it
        // doesn't) handles them.
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => {}
        Stmt::Switch { .. } => {}
        Stmt::Labeled { .. } => {}
        _ => {}
    }
}

/// Walk a slice of generator states and replace BREAK_SENTINEL /
/// CONTINUE_SENTINEL with their real target state numbers. Called after a
/// For/While body has been fully linearized into the state list.
fn fix_break_continue_sentinels(
    states: &mut [State],
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    for state in states.iter_mut() {
        fix_break_continue_sentinels_in_stmts(
            &mut state.body,
            state_id,
            break_target,
            continue_target,
        );
    }
}

fn fix_break_continue_sentinels_in_stmts(
    stmts: &mut [Stmt],
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    for stmt in stmts.iter_mut() {
        fix_break_continue_sentinels_in_stmt(stmt, state_id, break_target, continue_target);
    }
}

fn fix_break_continue_sentinels_in_stmt(
    stmt: &mut Stmt,
    state_id: LocalId,
    break_target: u32,
    continue_target: u32,
) {
    match stmt {
        Stmt::Expr(Expr::LocalSet(id, val)) if *id == state_id => {
            if let Expr::Number(n) = val.as_ref() {
                if *n == BREAK_SENTINEL {
                    **val = Expr::Number(break_target as f64);
                } else if *n == CONTINUE_SENTINEL {
                    **val = Expr::Number(continue_target as f64);
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            fix_break_continue_sentinels_in_stmts(
                then_branch,
                state_id,
                break_target,
                continue_target,
            );
            if let Some(eb) = else_branch.as_mut() {
                fix_break_continue_sentinels_in_stmts(eb, state_id, break_target, continue_target);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
        }
        Stmt::For { body, .. } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fix_break_continue_sentinels_in_stmts(body, state_id, break_target, continue_target);
            if let Some(c) = catch.as_mut() {
                fix_break_continue_sentinels_in_stmts(
                    &mut c.body,
                    state_id,
                    break_target,
                    continue_target,
                );
            }
            if let Some(f) = finally.as_mut() {
                fix_break_continue_sentinels_in_stmts(f, state_id, break_target, continue_target);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                fix_break_continue_sentinels_in_stmts(
                    &mut case.body,
                    state_id,
                    break_target,
                    continue_target,
                );
            }
        }
        Stmt::Labeled { body, .. } => {
            fix_break_continue_sentinels_in_stmt(
                body.as_mut(),
                state_id,
                break_target,
                continue_target,
            );
        }
        _ => {}
    }
}

fn fix_placeholder_state(stmts: &mut [Stmt], state_id: LocalId, target_state: u32) {
    fn fix_branch(branch: &mut [Stmt], state_id: LocalId, target_state: u32) {
        for inner in branch.iter_mut() {
            if let Stmt::Expr(Expr::LocalSet(id, val)) = inner {
                if *id == state_id {
                    if let Expr::Number(n) = val.as_ref() {
                        if *n == 0.0 {
                            **val = Expr::Number(target_state as f64);
                        }
                    }
                }
            }
        }
    }
    for stmt in stmts.iter_mut() {
        if let Stmt::If {
            then_branch,
            else_branch,
            ..
        } = stmt
        {
            fix_branch(then_branch, state_id, target_state);
            if let Some(eb) = else_branch {
                fix_branch(eb, state_id, target_state);
            }
        }
    }
}

/// Check if any statement in the body contains a yield expression.
fn body_contains_yield(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(Expr::Yield { .. }) => return true,
            Stmt::Let {
                init: Some(Expr::Yield { .. }),
                ..
            } => return true,
            Stmt::Return(Some(Expr::Yield { .. })) => return true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if body_contains_yield(then_branch) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if body_contains_yield(eb) {
                        return true;
                    }
                }
            }
            Stmt::While { body, .. } => {
                if body_contains_yield(body) {
                    return true;
                }
            }
            Stmt::For { body, .. } => {
                if body_contains_yield(body) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if body_contains_yield(body) {
                    return true;
                }
                if let Some(c) = catch {
                    if body_contains_yield(&c.body) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if body_contains_yield(f) {
                        return true;
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if body_contains_yield(&case.body) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Collect variable declarations that need to be hoisted to the outer scope.
fn collect_hoisted_vars(stmts: &[Stmt]) -> Vec<(LocalId, String, Type)> {
    let mut vars = Vec::new();
    collect_vars_recursive(stmts, &mut vars);
    vars
}

fn collect_vars_recursive(stmts: &[Stmt], vars: &mut Vec<(LocalId, String, Type)>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, name, ty, .. } => {
                vars.push((*id, name.clone(), ty.clone()));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_vars_recursive(then_branch, vars);
                if let Some(eb) = else_branch {
                    collect_vars_recursive(eb, vars);
                }
            }
            Stmt::While { body, .. } => collect_vars_recursive(body, vars),
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_vars_recursive(&[(**init).clone()], vars);
                }
                collect_vars_recursive(body, vars);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_vars_recursive(body, vars);
                if let Some(c) = catch {
                    // Hoist the catch parameter so the .throw() closure can assign to it.
                    if let Some((pid, pname)) = &c.param {
                        vars.push((*pid, pname.clone(), Type::Any));
                    }
                    collect_vars_recursive(&c.body, vars);
                }
                if let Some(f) = finally {
                    collect_vars_recursive(f, vars);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_vars_recursive(&case.body, vars);
                }
            }
            _ => {}
        }
    }
}

/// Rewrite Return(Some(expr)) to Return(Some({value: expr, done: true})).
/// Recurses through If/While/DoWhile/For/Try/Switch/Labeled bodies so a
/// user `return X` nested inside any control flow inside a state's body
/// is wrapped — without this, `return X` inside an `if` then-branch
/// inside the post-await tail of a state slipped through and the bare
/// value got returned to the runtime's `__step_r.done` access (#594).
fn rewrite_returns_as_done(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Return(Some(expr)) => {
                // Don't double-wrap if already an iter result
                if !is_iter_result(expr) {
                    let val = expr.clone();
                    *expr = make_iter_result(val, true);
                }
            }
            Stmt::Return(None) => {
                *stmt = Stmt::Return(Some(make_iter_result(Expr::Undefined, true)));
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_returns_as_done(then_branch);
                if let Some(eb) = else_branch {
                    rewrite_returns_as_done(eb);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                rewrite_returns_as_done(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_returns_as_done(body);
                if let Some(c) = catch {
                    rewrite_returns_as_done(&mut c.body);
                }
                if let Some(f) = finally {
                    rewrite_returns_as_done(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    rewrite_returns_as_done(&mut case.body);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                rewrite_returns_as_done(&mut v);
                **body = v.into_iter().next().unwrap();
            }
            _ => {}
        }
    }
}

/// Walk `stmts` and prepend `LocalSet(done_id, true)` immediately before
/// every `Stmt::Return` — at any depth. Mirrors `rewrite_returns_as_done`
/// but inserts the done-flag set instead of rewriting the return value.
/// Without this, a user `return X` inside an `if`/`try`/etc. nested
/// block of a state body left `__gen_done` false, so the next call to
/// `gen.next()` re-entered the state machine and ran the same state
/// again — surfacing as the iterator producing the user's return value
/// then hanging or producing `undefined` on subsequent ticks. (#594.)
fn prepend_done_before_returns(stmts: &mut Vec<Stmt>, done_id: u32) {
    let mut new_body: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for s in stmts.drain(..) {
        let mut s = s;
        // First recurse so done-flag set lands at the deepest enclosing
        // statement of each nested return.
        match &mut s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                prepend_done_before_returns(then_branch, done_id);
                if let Some(eb) = else_branch {
                    prepend_done_before_returns(eb, done_id);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                prepend_done_before_returns(body, done_id);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                prepend_done_before_returns(body, done_id);
                if let Some(c) = catch {
                    prepend_done_before_returns(&mut c.body, done_id);
                }
                if let Some(f) = finally {
                    prepend_done_before_returns(f, done_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    prepend_done_before_returns(&mut case.body, done_id);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                prepend_done_before_returns(&mut v, done_id);
                **body = v.into_iter().next().unwrap();
            }
            _ => {}
        }
        if matches!(s, Stmt::Return(_)) {
            new_body.push(Stmt::Expr(Expr::LocalSet(
                done_id,
                Box::new(Expr::Bool(true)),
            )));
        }
        new_body.push(s);
    }
    *stmts = new_body;
}

/// True iff any nested statement is a `Stmt::Return(_)` at any depth
/// (recurses through If/While/DoWhile/For/Try/Switch/Labeled). Used by
/// the StateExit::Done arm to decide whether the state body already
/// contains a user-level return that needs rewriting (#594).
fn body_contains_return(stmts: &[Stmt]) -> bool {
    for s in stmts {
        match s {
            Stmt::Return(_) => return true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if body_contains_return(then_branch) {
                    return true;
                }
                if let Some(eb) = else_branch {
                    if body_contains_return(eb) {
                        return true;
                    }
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                if body_contains_return(body) {
                    return true;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if body_contains_return(body) {
                    return true;
                }
                if let Some(c) = catch {
                    if body_contains_return(&c.body) {
                        return true;
                    }
                }
                if let Some(f) = finally {
                    if body_contains_return(f) {
                        return true;
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if body_contains_return(&case.body) {
                        return true;
                    }
                }
            }
            Stmt::Labeled { body, .. } => {
                if body_contains_return(std::slice::from_ref(body.as_ref())) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Check if an expression is already an iterator result object
fn is_iter_result(expr: &Expr) -> bool {
    if let Expr::Object(props) = expr {
        props.len() == 2
            && props.iter().any(|(k, _)| k == "value")
            && props.iter().any(|(k, _)| k == "done")
    } else {
        false
    }
}

/// Walk an expression / statement tree and replace every
/// `Expr::Object({value: V, done: D})` with `Expr::IterResultSet(V, D)`.
/// Recurses through Closure bodies, blocks, control-flow, etc. — needs
/// to descend into the nested generator state-machine closures the
/// generator transform just synthesized.
///
/// Bool literals (`Expr::Bool(true)` / `Expr::Bool(false)`) and number-1
/// patterns are recognized for the `done` half. Other shapes (computed
/// done) fall through to the original object alloc — they're not on
/// the hot path.
fn rewrite_iter_results_to_scratch(expr: &mut Expr) {
    rewrite_expr(expr);
}

/// Rewrite every `Expr::Yield { value, .. }` to `Expr::Await(value)` recursively
/// through statement and expression trees. Does NOT descend into nested closures
/// (their await/yield context is independent).
///
/// Used by the async-step `__async_throw` builder: the global await→yield
/// rewrite in `transform_async_to_generator` runs on the whole function body
/// indiscriminately, but catch bodies aren't lifted into state-machine states —
/// they're inlined into a regular sync closure where yield codegens to
/// `double_literal(0.0)`. Flipping the yields back to awaits restores the
/// busy-wait await semantics for awaits inside catch handlers.
fn rewrite_yield_to_await_in_stmts(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        rewrite_yield_to_await_in_stmt(s);
    }
}

fn rewrite_yield_to_await_in_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => rewrite_yield_to_await_in_expr(e),
        Stmt::Let { init: Some(e), .. } => rewrite_yield_to_await_in_expr(e),
        Stmt::Return(Some(e)) => rewrite_yield_to_await_in_expr(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_yield_to_await_in_expr(condition);
            rewrite_yield_to_await_in_stmts(then_branch);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_yield_to_await_in_stmts(eb);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_yield_to_await_in_expr(condition);
            rewrite_yield_to_await_in_stmts(body);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init.as_mut() {
                rewrite_yield_to_await_in_stmt(i.as_mut());
            }
            if let Some(c) = condition.as_mut() {
                rewrite_yield_to_await_in_expr(c);
            }
            if let Some(u) = update.as_mut() {
                rewrite_yield_to_await_in_expr(u);
            }
            rewrite_yield_to_await_in_stmts(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_yield_to_await_in_stmts(body);
            if let Some(c) = catch.as_mut() {
                rewrite_yield_to_await_in_stmts(&mut c.body);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_yield_to_await_in_stmts(f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_yield_to_await_in_stmts(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_yield_to_await_in_stmt(body.as_mut());
        }
        _ => {}
    }
}

fn rewrite_yield_to_await_in_expr(expr: &mut Expr) {
    if let Expr::Yield { value, .. } = expr {
        let inner = value.take().map(|b| *b).unwrap_or(Expr::Undefined);
        let mut new_inner = inner;
        rewrite_yield_to_await_in_expr(&mut new_inner);
        *expr = Expr::Await(Box::new(new_inner));
        return;
    }
    // Recurse into child expressions but stop at Closure boundaries —
    // a nested closure has its own await/yield context.
    rewrite_yield_to_await_in_expr_children(expr);
}

fn rewrite_yield_to_await_in_expr_children(expr: &mut Expr) {
    use perry_hir::walker::walk_expr_children_mut;
    // Skip Closure bodies — their inner statements have their own
    // independent semantics (already-rewritten or not as their own
    // closure-level pass dictates).
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    walk_expr_children_mut(expr, &mut |child: &mut Expr| {
        rewrite_yield_to_await_in_expr(child);
    });
}

/// Rewrite every `Stmt::Return(Some(X))` to `Stmt::Return(Some(make_iter_result(X, true)))`
/// and `Stmt::Return(None)` to `Stmt::Return(Some(make_iter_result(Undefined, true)))`.
/// Recurses through If/While/DoWhile/For/Try/Switch/Labeled control flow but
/// does NOT descend into nested closures — a `return` inside an inner closure
/// belongs to that closure, not to the catch body.
///
/// Used by the async-step `__async_throw` builder: a user-source `return X`
/// inside a `catch (e) { ... }` block must propagate to the function's
/// returned Promise (resolved with X), not silently exit `__async_throw`
/// while leaving the scratch iter-result slots stale.
fn rewrite_catch_returns_to_iter_result(stmts: &mut Vec<Stmt>) {
    for stmt in stmts.iter_mut() {
        rewrite_catch_returns_to_iter_result_in_stmt(stmt);
    }
}

fn rewrite_catch_returns_to_iter_result_in_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Return(opt) => {
            let value = opt.take().unwrap_or(Expr::Undefined);
            *opt = Some(make_iter_result(value, true));
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_catch_returns_to_iter_result(then_branch);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_catch_returns_to_iter_result(eb);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_catch_returns_to_iter_result(body);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_mut() {
                rewrite_catch_returns_to_iter_result_in_stmt(i.as_mut());
            }
            rewrite_catch_returns_to_iter_result(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_catch_returns_to_iter_result(body);
            if let Some(c) = catch.as_mut() {
                rewrite_catch_returns_to_iter_result(&mut c.body);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_catch_returns_to_iter_result(f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases.iter_mut() {
                rewrite_catch_returns_to_iter_result(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_catch_returns_to_iter_result_in_stmt(body.as_mut());
        }
        _ => {}
    }
}

/// Apply iter-result-to-scratch rewrites directly to a statement list
/// (rather than to a Closure expression). Used by the fused async-step
/// driver, where next_body is inlined into step's body instead of being
/// wrapped in a separate closure.
fn rewrite_iter_results_in_stmts(stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        rewrite_stmt(s);
    }
}

/// Rewrite every `Stmt::Return` inside `stmts` (recursing through
/// nested control flow) to `Expr(value) + LabeledBreak(label)` — or
/// just `LabeledBreak(label)` for `Return(None)`. Used by the fused
/// async-step driver: the inlined next_body's iter-result returns
/// (which used to exit the next closure) now break out to the
/// `__step_done` label so step's post-dispatch code (IterResultGetDone
/// check + AsyncStepChain) runs instead of step itself returning early.
///
/// Does NOT descend into nested closures — those Returns belong to
/// the closures themselves, not to the outer step body.
fn rewrite_returns_to_labeled_break(stmts: &mut Vec<Stmt>, label: &str) {
    let mut i = 0;
    while i < stmts.len() {
        let stmt = std::mem::replace(&mut stmts[i], Stmt::Continue);
        match stmt {
            Stmt::Return(Some(e)) => {
                stmts[i] = Stmt::Expr(e);
                stmts.insert(i + 1, Stmt::LabeledBreak(label.to_string()));
                i += 2;
            }
            Stmt::Return(None) => {
                stmts[i] = Stmt::LabeledBreak(label.to_string());
                i += 1;
            }
            mut other => {
                rewrite_returns_to_labeled_break_in_stmt(&mut other, label);
                stmts[i] = other;
                i += 1;
            }
        }
    }
}

fn rewrite_returns_to_labeled_break_in_stmt(stmt: &mut Stmt, label: &str) {
    match stmt {
        Stmt::Return(_) => {
            unreachable!("rewrite_returns_to_labeled_break_in_stmt should not see a bare Return");
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_returns_to_labeled_break(then_branch, label);
            if let Some(eb) = else_branch.as_mut() {
                rewrite_returns_to_labeled_break(eb, label);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_returns_to_labeled_break(body, label);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_mut() {
                rewrite_returns_to_labeled_break_in_stmt(init_stmt.as_mut(), label);
            }
            rewrite_returns_to_labeled_break(body, label);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_returns_to_labeled_break(body, label);
            if let Some(c) = catch.as_mut() {
                rewrite_returns_to_labeled_break(&mut c.body, label);
            }
            if let Some(f) = finally.as_mut() {
                rewrite_returns_to_labeled_break(f, label);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases.iter_mut() {
                rewrite_returns_to_labeled_break(&mut c.body, label);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_returns_to_labeled_break_in_stmt(body.as_mut(), label);
        }
        _ => {}
    }
}

fn rewrite_expr(expr: &mut Expr) {
    // Match the exact `{value: V, done: bool}` shape and replace
    // BEFORE recursing — once we've rewritten, the children are
    // already visited inside the new IterResultSet's child expression.
    if let Expr::Object(props) = expr {
        if props.len() == 2 {
            let has_value = props.iter().any(|(k, _)| k == "value");
            let has_done = props.iter().any(|(k, _)| k == "done");
            if has_value && has_done {
                // Resolve done to a literal bool.
                let done_expr = props
                    .iter()
                    .find(|(k, _)| k == "done")
                    .map(|(_, v)| v.clone())
                    .unwrap();
                let done_bool = match &done_expr {
                    Expr::Bool(b) => Some(*b),
                    _ => None,
                };
                if let Some(done_b) = done_bool {
                    let mut value_expr = props
                        .iter()
                        .find(|(k, _)| k == "value")
                        .map(|(_, v)| v.clone())
                        .unwrap();
                    // Recurse INTO the value expression first so nested
                    // iter-result allocs (rare but possible inside
                    // user-side returns) are also rewritten.
                    rewrite_expr(&mut value_expr);
                    *expr = Expr::IterResultSet(Box::new(value_expr), done_b);
                    return;
                }
            }
        }
    }
    rewrite_expr_children(expr);
}

fn rewrite_expr_children(expr: &mut Expr) {
    // Recurse into all child Exprs. For Closure, ALSO recurse into
    // body statements (the centralised walker explicitly excludes
    // closure bodies, but we need them — the generator transform's
    // synthesized iter-result returns live inside next/return/throw
    // closures' bodies).
    match expr {
        Expr::Closure { body, params, .. } => {
            for stmt in body.iter_mut() {
                rewrite_stmt(stmt);
            }
            for p in params.iter_mut() {
                if let Some(d) = p.default.as_mut() {
                    rewrite_expr(d);
                }
            }
        }
        _ => {
            perry_hir::walker::walk_expr_children_mut(expr, &mut rewrite_expr);
        }
    }
}

fn rewrite_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => rewrite_expr(e),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
        Stmt::Let { init, .. } => {
            if let Some(e) = init.as_mut() {
                rewrite_expr(e);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(condition);
            for s in then_branch.iter_mut() {
                rewrite_stmt(s);
            }
            if let Some(eb) = else_branch.as_mut() {
                for s in eb.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_expr(condition);
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init.as_mut() {
                rewrite_stmt(s);
            }
            if let Some(e) = condition.as_mut() {
                rewrite_expr(e);
            }
            if let Some(e) = update.as_mut() {
                rewrite_expr(e);
            }
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body.iter_mut() {
                rewrite_stmt(s);
            }
            if let Some(c) = catch.as_mut() {
                for s in c.body.iter_mut() {
                    rewrite_stmt(s);
                }
            }
            if let Some(f) = finally.as_mut() {
                for s in f.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_expr(discriminant);
            for c in cases.iter_mut() {
                if let Some(e) = c.test.as_mut() {
                    rewrite_expr(e);
                }
                for s in c.body.iter_mut() {
                    rewrite_stmt(s);
                }
            }
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body),
    }
}
