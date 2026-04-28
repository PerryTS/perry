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
            if let Some(e) = init { scan_expr_for_max_local(e, max_id); }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_for_max_local(e, max_id),
        Stmt::Return(e) => {
            if let Some(e) = e { scan_expr_for_max_local(e, max_id); }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(then_branch, max_id);
            if let Some(eb) = else_branch { scan_stmts_for_max_local(eb, max_id); }
        }
        Stmt::While { condition, body } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::DoWhile { body, condition } => {
            scan_stmts_for_max_local(body, max_id);
            scan_expr_for_max_local(condition, max_id);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init { scan_stmt_for_max_local(i, max_id); }
            if let Some(c) = condition { scan_expr_for_max_local(c, max_id); }
            if let Some(u) = update { scan_expr_for_max_local(u, max_id); }
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::Try { body, catch, finally } => {
            scan_stmts_for_max_local(body, max_id);
            if let Some(c) = catch { scan_stmts_for_max_local(&c.body, max_id); }
            if let Some(f) = finally { scan_stmts_for_max_local(f, max_id); }
        }
        Stmt::Switch { discriminant, cases } => {
            scan_expr_for_max_local(discriminant, max_id);
            for case in cases { scan_stmts_for_max_local(&case.body, max_id); }
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
        Expr::Closure { params, body, captures, mutable_captures, .. } => {
            for p in params { *max_id = (*max_id).max(p.id); }
            for c in captures { *max_id = (*max_id).max(*c); }
            for c in mutable_captures { *max_id = (*max_id).max(*c); }
            scan_stmts_for_max_local(body, max_id);
        }
        Expr::Call { callee, args, .. } => {
            scan_expr_for_max_local(callee, max_id);
            for a in args { scan_expr_for_max_local(a, max_id); }
        }
        Expr::New { args, .. } => {
            for a in args { scan_expr_for_max_local(a, max_id); }
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
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
            scan_expr_for_max_local(object, max_id);
            scan_expr_for_max_local(index, max_id);
            scan_expr_for_max_local(value, max_id);
        }
        Expr::Array(items) => {
            for item in items { scan_expr_for_max_local(item, max_id); }
        }
        Expr::Object(fields) => {
            for (_, v) in fields { scan_expr_for_max_local(v, max_id); }
        }
        Expr::Sequence(exprs) => {
            for e in exprs { scan_expr_for_max_local(e, max_id); }
        }
        Expr::Yield { value: Some(v), .. } => scan_expr_for_max_local(v, max_id),
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
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            scan_expr_for_max_local(array, max_id);
            scan_expr_for_max_local(callback, max_id);
            if let Some(i) = initial { scan_expr_for_max_local(i, max_id); }
        }
        Expr::ArrayToSorted { array, comparator } => {
            scan_expr_for_max_local(array, max_id);
            if let Some(c) = comparator { scan_expr_for_max_local(c, max_id); }
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
        Stmt::Let { init: Some(expr), .. } => scan_expr_for_max_func(expr, max_id),
        Stmt::If { condition, then_branch, else_branch } => {
            scan_expr_for_max_func(condition, max_id);
            scan_stmts_for_max_func(then_branch, max_id);
            if let Some(eb) = else_branch { scan_stmts_for_max_func(eb, max_id); }
        }
        Stmt::While { body, .. } => scan_stmts_for_max_func(body, max_id),
        Stmt::For { body, .. } => scan_stmts_for_max_func(body, max_id),
        Stmt::Try { body, catch, finally } => {
            scan_stmts_for_max_func(body, max_id);
            if let Some(c) = catch { scan_stmts_for_max_func(&c.body, max_id); }
            if let Some(f) = finally { scan_stmts_for_max_func(f, max_id); }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases { scan_stmts_for_max_func(&case.body, max_id); }
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
            for a in args { scan_expr_for_max_func(a, max_id); }
        }
        Expr::New { args, .. } => {
            for a in args { scan_expr_for_max_func(a, max_id); }
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
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
            scan_expr_for_max_func(object, max_id);
            scan_expr_for_max_func(index, max_id);
            scan_expr_for_max_func(value, max_id);
        }
        Expr::LocalSet(_, v) => scan_expr_for_max_func(v, max_id),
        Expr::Array(items) => {
            for item in items { scan_expr_for_max_func(item, max_id); }
        }
        Expr::Object(fields) => {
            for (_, v) in fields { scan_expr_for_max_func(v, max_id); }
        }
        Expr::Sequence(exprs) => {
            for e in exprs { scan_expr_for_max_func(e, max_id); }
        }
        Expr::Yield { value: Some(v), .. } => scan_expr_for_max_func(v, max_id),
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
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            scan_expr_for_max_func(array, max_id);
            scan_expr_for_max_func(callback, max_id);
            if let Some(i) = initial { scan_expr_for_max_func(i, max_id); }
        }
        Expr::ArrayToSorted { array, comparator } => {
            scan_expr_for_max_func(array, max_id);
            if let Some(c) = comparator { scan_expr_for_max_func(c, max_id); }
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

fn rewrite_hoisted_lets_in_stmt(
    stmt: &mut Stmt,
    hoisted_ids: &std::collections::HashSet<LocalId>,
) {
    if let Stmt::Let { id, init: Some(init_expr), .. } = stmt {
        if hoisted_ids.contains(id) {
            *stmt = Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone())));
            return;
        }
    }
    match stmt {
        Stmt::If { then_branch, else_branch, .. } => {
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
        Stmt::Try { body, catch, finally } => {
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
            Stmt::If { then_branch, else_branch, .. } => {
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
                *body = Box::new(v.into_iter().next().unwrap());
            }
            Stmt::Try { body, catch, finally } => {
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
fn transform_generator_function(func: &mut Function, next_local_id: &mut u32, next_func_id: &mut u32) {
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
    // Catches collected during linearization: each entry is (catch_param_id, catch_body).
    // Used by the .throw() closure to re-route the exception into the catch handler.
    let mut catches: Vec<(Option<LocalId>, Vec<Stmt>)> = Vec::new();
    linearize_body(&func.body, &mut states, &mut current, &mut state_num, state_id, next_local_id, sent_id, &mut catches);
    let extra_local_ids: Vec<LocalId> = (local_id_before..*next_local_id).collect();

    // Push final state (code after last yield / end of function)
    states.push(State {
        num: state_num,
        body: current,
        exit: StateExit::Done,
    });

    // Collect hoisted var IDs first so we know which Lets to rewrite
    let hoisted_ids: std::collections::HashSet<LocalId> = collect_hoisted_vars(&func.body)
        .iter().map(|(id, _, _)| *id).collect();

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
                let has_return = case_body.iter().any(|s| matches!(s, Stmt::Return(_)));
                if has_return {
                    // Rewrite existing returns to iter results, and prepend done=true
                    // Insert done=true BEFORE the return so it's reachable
                    let mut new_body = Vec::new();
                    for s in case_body.drain(..) {
                        if matches!(s, Stmt::Return(_)) {
                            new_body.push(Stmt::Expr(Expr::LocalSet(
                                done_id,
                                Box::new(Expr::Bool(true)),
                            )));
                        }
                        new_body.push(s);
                    }
                    case_body = new_body;
                    rewrite_returns_as_done(&mut case_body);
                    // Don't add trailing return — body already returns
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
            then_branch: vec![
                Stmt::Return(Some(make_iter_result(Expr::Undefined, true))),
            ],
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

    let next_closure = Expr::Closure {
        func_id: next_func_id_val,
        params: vec![perry_hir::Param { id: next_param_id, name: "__val".to_string(), ty: Type::Any, is_rest: false, default: None }],
        return_type: Type::Any,
        body: next_body,
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // Build .return(value) closure — immediately marks done and returns {value, done: true}
    let return_param_id = alloc_local(next_local_id);
    let return_func_id_val = { let id = *next_func_id; *next_func_id += 1; id };
    let mut return_body: Vec<Stmt> = vec![
        Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
        Stmt::Return(Some(make_iter_result(Expr::LocalGet(return_param_id), true))),
    ];
    if is_async_generator {
        wrap_returns_in_promise(&mut return_body);
    }
    let return_closure = Expr::Closure {
        func_id: return_func_id_val,
        params: vec![perry_hir::Param { id: return_param_id, name: "__ret_val".to_string(), ty: Type::Any, is_rest: false, default: None }],
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
    let throw_func_id_val = { let id = *next_func_id; *next_func_id += 1; id };
    let mut throw_body: Vec<Stmt> = Vec::new();
    if let Some((catch_param_id, catch_body)) = catches.first().cloned() {
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
            if let Stmt::Let { id, init: Some(init_expr), .. } = stmt {
                if hoisted_ids.contains(id) {
                    *stmt = Stmt::Expr(Expr::LocalSet(*id, Box::new(init_expr.clone())));
                }
            }
        }
        throw_body.extend(rewritten);
    }
    throw_body.push(Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))));
    throw_body.push(Stmt::Return(Some(make_iter_result(Expr::Undefined, true))));
    if is_async_generator {
        wrap_returns_in_promise(&mut throw_body);
    }
    let throw_closure = Expr::Closure {
        func_id: throw_func_id_val,
        params: vec![perry_hir::Param { id: throw_param_id, name: "__throw_val".to_string(), ty: Type::Any, is_rest: false, default: None }],
        return_type: Type::Any,
        body: throw_body,
        captures: captures.clone(),
        mutable_captures: mutable_captures.clone(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };

    // Build the iterator object expression.
    let iter_obj = Expr::Object(vec![
        ("next".to_string(), next_closure),
        ("return".to_string(), return_closure),
        ("throw".to_string(), throw_closure),
    ]);

    if func.was_plain_async {
        // Issue #256: this function was originally a plain async function;
        // the async_to_generator pre-pass rewrote await→yield. Wrap the
        // iterator in an async-step driver so the function returns a
        // Promise that respects spec microtask ordering. See
        // `build_async_step_driver` for the structure.
        let wrapper_stmts = build_async_step_driver(iter_obj, next_local_id, next_func_id);
        for s in wrapper_stmts {
            new_body.push(s);
        }
        func.was_plain_async = false; // consumed
    } else {
        // Plain generator: return the iterator object directly.
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

    let step_func_id = { let id = *next_func_id; *next_func_id += 1; id };
    let then_v_func_id = { let id = *next_func_id; *next_func_id += 1; id };
    let then_e_func_id = { let id = *next_func_id; *next_func_id += 1; id };

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

    let step_body: Vec<Stmt> = vec![
        // let r;
        Stmt::Let {
            id: r_id,
            name: "__step_r".to_string(),
            ty: any_ty.clone(),
            mutable: true,
            init: None,
        },
        // try { r = ...; } catch (e) { return Promise.reject(e); }
        Stmt::Try {
            body: vec![Stmt::Expr(Expr::LocalSet(r_id, Box::new(dispatch_iter)))],
            catch: Some(CatchClause {
                param: Some((catch_e_id, "__step_catch_e".to_string())),
                body: vec![Stmt::Return(Some(promise_reject(Expr::LocalGet(catch_e_id))))],
            }),
            finally: None,
        },
        // if (r.done) return Promise.resolve(r.value);
        Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(r_id)),
                property: "done".to_string(),
            },
            then_branch: vec![Stmt::Return(Some(promise_resolve(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(r_id)),
                property: "value".to_string(),
            })))],
            else_branch: None,
        },
        // return Promise.resolve(r.value).then(<then_v>, <then_e>);
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(promise_resolve(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(r_id)),
                    property: "value".to_string(),
                })),
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
    #[allow(unused_variables)]
    next_local_id: &mut u32,
    sent_id: LocalId,
    catches: &mut Vec<(Option<LocalId>, Vec<Stmt>)>,
) {
    for stmt in stmts {
        match stmt {
            // yield* delegation: iterate the inner iterator and yield each value
            Stmt::Expr(Expr::Yield { value: Some(inner), delegate: true }) => {
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
                current.push(Stmt::Expr(Expr::LocalSet(del_iter_id, Box::new(*inner.clone()))));
                current.push(Stmt::Expr(Expr::LocalSet(del_result_id, Box::new(next_call.clone()))));

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
                linearize_body(&[while_stmt], states, current, state_num, state_id, next_local_id, sent_id, catches);
            }

            // yield expr at statement level (non-delegate)
            Stmt::Expr(Expr::Yield { value, delegate: false }) | Stmt::Expr(Expr::Yield { value, .. }) => {
                let yield_val = value.as_ref().map(|v| *v.clone()).unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield { value: yield_val, next_state: *state_num },
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
            Stmt::For { init, condition, update, body }
                if body_contains_yield(body) =>
            {
                // State N: pre-loop code + init, goto condition check
                let init_state = *state_num;
                *state_num += 1;
                let mut init_body = std::mem::take(current);
                // Add init statement (typically `let i = start`)
                // But we need to convert it to an assignment since the var is hoisted
                if let Some(init_stmt) = init {
                    match init_stmt.as_ref() {
                        Stmt::Let { id, init: Some(init_expr), .. } => {
                            init_body.push(Stmt::Expr(Expr::LocalSet(
                                *id,
                                Box::new(init_expr.clone()),
                            )));
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

                // Process loop body (may contain yields)
                linearize_body(body, states, current, state_num, state_id, next_local_id, sent_id, catches);

                // State for update: run update expression, goto condition check
                let update_state = *state_num;
                *state_num += 1;
                let mut update_body = std::mem::take(current);
                if let Some(upd) = update {
                    update_body.push(Stmt::Expr(upd.clone()));
                }
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
            }

            // While-loop containing yield(s) - similar to for-loop
            Stmt::While { condition, body: while_body }
                if body_contains_yield(while_body) =>
            {
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

                // Process body
                linearize_body(while_body, states, current, state_num, state_id, next_local_id, sent_id, catches);

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
            }

            // Try-catch containing yield(s) — linearize the try body directly and
            // stash the catch body so the .throw() closure can inline it.
            // Limitations: no per-state exception handler tracking, so only the
            // first catch encountered will run on .throw(). Catches themselves
            // must not yield — they run to completion inside the throw closure.
            Stmt::Try { body, catch, finally }
                if body_contains_yield(body)
                    || finally.as_ref().map_or(false, |f| body_contains_yield(f)) =>
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
                    linearize_body(body, states, current, state_num, state_id, next_local_id, sent_id, catches);
                } else {
                    // Body has no yields: push as-is to current state.
                    for s in body {
                        current.push(s.clone());
                    }
                }

                // Stash the catch so transform_generator_function can inline it
                // into the .throw() closure later.
                if let Some(catch_clause) = catch {
                    let param_id = catch_clause.param.as_ref().map(|(id, _)| *id);
                    catches.push((param_id, catch_clause.body.clone()));
                }

                // Finally block: linearize if it has yields (await-using path),
                // otherwise push as-is.
                if let Some(fin) = finally {
                    if body_contains_yield(fin) {
                        linearize_body(fin, states, current, state_num, state_id, next_local_id, sent_id, catches);
                    } else {
                        for s in fin {
                            current.push(s.clone());
                        }
                    }
                }
            }

            // If-statement containing yield(s) — linearize both branches
            Stmt::If { condition, then_branch, else_branch }
                if body_contains_yield(then_branch)
                || else_branch.as_ref().map_or(false, |e| body_contains_yield(e)) =>
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
                                Stmt::Expr(Expr::LocalSet(state_id, Box::new(Expr::Number(then_state as f64)))),
                                Stmt::Continue,
                            ],
                            else_branch: Some(vec![
                                Stmt::Expr(Expr::LocalSet(state_id, Box::new(Expr::Number(else_state_placeholder as f64)))),
                                Stmt::Continue,
                            ]),
                        });
                        b
                    },
                    exit: StateExit::Done, // won't be reached (branches above jump)
                });

                // Linearize then-branch
                linearize_body(then_branch, states, current, state_num, state_id, next_local_id, sent_id, catches);
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
                    linearize_body(else_stmts, states, current, state_num, state_id, next_local_id, sent_id, catches);
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
                            if *target == 0 { *target = after_state; }
                        }
                    }
                }
            }

            // Let with yield initializer: `const x = yield expr` (two-way yield)
            // After resuming, `x` receives the value passed by the caller via next(val),
            // which is stored in __sent by the next() closure preamble.
            Stmt::Let { id, init: Some(Expr::Yield { value, .. }), mutable, ty, name } => {
                let yield_val = value.as_ref().map(|v| *v.clone()).unwrap_or(Expr::Undefined);
                let this_state = *state_num;
                *state_num += 1;
                states.push(State {
                    num: this_state,
                    body: std::mem::take(current),
                    exit: StateExit::Yield { value: yield_val, next_state: *state_num },
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
fn fix_placeholder_state(stmts: &mut [Stmt], state_id: LocalId, target_state: u32) {
    fn fix_branch(branch: &mut [Stmt], state_id: LocalId, target_state: u32) {
        for inner in branch.iter_mut() {
            if let Stmt::Expr(Expr::LocalSet(id, val)) = inner {
                if *id == state_id {
                    if let Expr::Number(n) = val.as_ref() {
                        if *n == 0.0 {
                            *val = Box::new(Expr::Number(target_state as f64));
                        }
                    }
                }
            }
        }
    }
    for stmt in stmts.iter_mut() {
        if let Stmt::If { then_branch, else_branch, .. } = stmt {
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
            Stmt::Let { init: Some(Expr::Yield { .. }), .. } => return true,
            Stmt::Return(Some(Expr::Yield { .. })) => return true,
            Stmt::If { then_branch, else_branch, .. } => {
                if body_contains_yield(then_branch) { return true; }
                if let Some(eb) = else_branch {
                    if body_contains_yield(eb) { return true; }
                }
            }
            Stmt::While { body, .. } => {
                if body_contains_yield(body) { return true; }
            }
            Stmt::For { body, .. } => {
                if body_contains_yield(body) { return true; }
            }
            Stmt::Try { body, catch, finally } => {
                if body_contains_yield(body) { return true; }
                if let Some(c) = catch {
                    if body_contains_yield(&c.body) { return true; }
                }
                if let Some(f) = finally {
                    if body_contains_yield(f) { return true; }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if body_contains_yield(&case.body) { return true; }
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
            Stmt::If { then_branch, else_branch, .. } => {
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
            Stmt::Try { body, catch, finally } => {
                collect_vars_recursive(body, vars);
                if let Some(c) = catch {
                    // Hoist the catch parameter so the .throw() closure can assign to it.
                    if let Some((pid, pname)) = &c.param {
                        vars.push((*pid, pname.clone(), Type::Any));
                    }
                    collect_vars_recursive(&c.body, vars);
                }
                if let Some(f) = finally { collect_vars_recursive(f, vars); }
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

/// Rewrite Return(Some(expr)) to Return(Some({value: expr, done: true}))
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
            _ => {}
        }
    }
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
