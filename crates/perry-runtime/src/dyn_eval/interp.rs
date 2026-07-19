//! Statement interpreter + call machinery for #6559.
//!
//! Supported statement subset (everything the ajv / fast-json-stringify /
//! find-my-way / zod code generators emit):
//! var/let/const (incl. destructuring + defaults), expression statements,
//! if/else, for / for-of / for-in, while, do-while, switch, break/continue
//! (with labels), labeled statements, return, throw, try/catch/finally,
//! blocks, function declarations (hoisted per scope), empty statements.
//! Unsupported statements throw the #6559 diagnostic TypeError naming the
//! construct (see `bridge::throw_unsupported`).

use std::cell::RefCell;
use std::collections::HashMap;

use perry_parser::swc_ecma_ast as ast;

use super::bridge::{self, throw_unsupported};
use super::expr::eval_expr;
use super::{
    call_depth_enter, call_depth_leave, env, lookup_fn, root_get, root_push, root_set,
    roots_len, roots_truncate, InterpBody, InterpFn,
};

/// Per-invocation context threaded through the walkers. Values live in the
/// rooted stack; the context only carries indices (GC-move-proof).
pub(crate) struct Ctx {
    /// Root index of the frame's `this`.
    pub this_idx: usize,
    /// Root index the frame's return value is written to.
    pub ret_idx: usize,
}

/// Statement completion. Thrown exceptions never appear here — they longjmp
/// through the runtime's exception machinery.
pub(crate) enum Flow {
    Normal,
    Return,
    Break(Option<String>),
    Continue(Option<String>),
}

// ── nested-function registration cache ────────────────────────────────────

thread_local! {
    /// AST-node address → registered fn id. Nested function/arrow expressions
    /// evaluate many times (ajv validators run per request); registering the
    /// node once keeps the registry bounded by the number of syntactic
    /// functions. Addresses are stable because `FN_REGISTRY` holds every
    /// `InterpFn` alive for the program's lifetime.
    static NODE_FN_IDS: RefCell<HashMap<usize, u32>> = RefCell::new(HashMap::new());
}

fn fn_id_for_node(addr: usize, build: impl FnOnce() -> InterpFn) -> u32 {
    if let Some(id) = NODE_FN_IDS.with(|m| m.borrow().get(&addr).copied()) {
        return id;
    }
    let id = super::register_fn(build());
    NODE_FN_IDS.with(|m| m.borrow_mut().insert(addr, id));
    id
}

// ── construction ───────────────────────────────────────────────────────────

pub(crate) fn build_interp_fn(params: Vec<ast::Pat>, body: InterpBody) -> InterpFn {
    let mut hoisted_vars = Vec::new();
    if let InterpBody::Block(stmts) = &body {
        collect_var_names(stmts, &mut hoisted_vars);
    }
    InterpFn {
        params,
        body,
        hoisted_vars,
    }
}

/// Eager construction-time rejection of constructs that can never run.
/// Deep rejection stays lazy (interpretation-time) so this scan does not need
/// a full AST visitor; the wrapper-level checks here are the ones
/// feature-probing callers depend on failing fast.
pub(crate) fn scan_function_supported(func: &ast::Function) {
    if func.is_generator {
        throw_unsupported("generator function");
    }
    if func.is_async {
        throw_unsupported("async function (no async codegen target needs it yet)");
    }
    for p in &func.params {
        scan_param_supported(&p.pat);
    }
}

fn scan_param_supported(pat: &ast::Pat) {
    if let ast::Pat::Rest(_) = pat {
        throw_unsupported("rest parameter (...args)");
    }
}

/// `var` hoisting prepass: collect `var` names declared anywhere in the
/// function body (excluding nested function bodies), plus for/for-in/for-of
/// heads. They are pre-bound `undefined` in the function scope at call entry
/// — ajv re-`var`s the same name in sibling blocks and reads it across them.
fn collect_var_names(stmts: &[ast::Stmt], out: &mut Vec<String>) {
    for s in stmts {
        collect_var_names_stmt(s, out);
    }
}

fn collect_var_names_stmt(stmt: &ast::Stmt, out: &mut Vec<String>) {
    use ast::Stmt::*;
    match stmt {
        Decl(ast::Decl::Var(v)) if v.kind == ast::VarDeclKind::Var => {
            for d in &v.decls {
                collect_pat_names(&d.name, out);
            }
        }
        Block(b) => collect_var_names(&b.stmts, out),
        If(i) => {
            collect_var_names_stmt(&i.cons, out);
            if let Some(alt) = &i.alt {
                collect_var_names_stmt(alt, out);
            }
        }
        For(f) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(v)) = &f.init {
                if v.kind == ast::VarDeclKind::Var {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
            }
            collect_var_names_stmt(&f.body, out);
        }
        ForIn(f) => {
            if let ast::ForHead::VarDecl(v) = &f.left {
                if v.kind == ast::VarDeclKind::Var {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
            }
            collect_var_names_stmt(&f.body, out);
        }
        ForOf(f) => {
            if let ast::ForHead::VarDecl(v) = &f.left {
                if v.kind == ast::VarDeclKind::Var {
                    for d in &v.decls {
                        collect_pat_names(&d.name, out);
                    }
                }
            }
            collect_var_names_stmt(&f.body, out);
        }
        While(w) => collect_var_names_stmt(&w.body, out),
        DoWhile(w) => collect_var_names_stmt(&w.body, out),
        Labeled(l) => collect_var_names_stmt(&l.body, out),
        Switch(sw) => {
            for case in &sw.cases {
                collect_var_names(&case.cons, out);
            }
        }
        Try(t) => {
            collect_var_names(&t.block.stmts, out);
            if let Some(h) = &t.handler {
                collect_var_names(&h.body.stmts, out);
            }
            if let Some(f) = &t.finalizer {
                collect_var_names(&f.stmts, out);
            }
        }
        _ => {}
    }
}

fn collect_pat_names(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(b) => out.push(b.id.sym.to_string()),
        ast::Pat::Assign(a) => collect_pat_names(&a.left, out),
        ast::Pat::Array(a) => {
            for p in a.elems.iter().flatten() {
                collect_pat_names(p, out);
            }
        }
        ast::Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => collect_pat_names(&kv.value, out),
                    ast::ObjectPatProp::Assign(a) => out.push(a.key.id.sym.to_string()),
                    ast::ObjectPatProp::Rest(r) => collect_pat_names(&r.arg, out),
                }
            }
        }
        _ => {}
    }
}

// ── closures & the native thunk ────────────────────────────────────────────

/// Sentinel capture value for "no lexical this — read the dynamic receiver".
const NO_LEXICAL_THIS: u64 = crate::value::TAG_HOLE;

/// Allocate the first-class runtime closure for an interpreted function.
/// Captures: [0] fn id (number), [1] defining environment (traced pointer),
/// [2] lexical `this` for arrows (or the hole sentinel).
pub(crate) fn alloc_interp_closure(fn_id: u32, def_env: f64, lexical_this: Option<f64>) -> f64 {
    ensure_thunk_registered();
    let env_idx = root_push(def_env);
    let this_idx = root_push(lexical_this.unwrap_or(f64::from_bits(NO_LEXICAL_THIS)));
    let closure = crate::closure::js_closure_alloc(interp_thunk as *const u8, 3);
    if closure.is_null() {
        roots_truncate(env_idx);
        bridge::throw_range_error("out of memory allocating dynamic function");
    }
    crate::closure::js_closure_set_capture_f64(closure, 0, fn_id as f64);
    crate::closure::js_closure_set_capture_bits(closure, 1, root_get(env_idx).to_bits());
    crate::closure::js_closure_set_capture_bits(closure, 2, root_get(this_idx).to_bits());
    // Record the capture layout + fire write barriers so the GC traces the
    // environment / lexical-this slots (they are heap pointers).
    unsafe {
        crate::closure::rebuild_closure_layout_and_barriers(closure, 3);
    }
    roots_truncate(env_idx);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Highest argument count deliverable to the shared thunk. The dispatcher
/// pads/truncates to the registered arity, so every interpreted function
/// receives exactly this many (missing args read `undefined` — exactly what
/// default-parameter binding needs).
const THUNK_ARITY: usize = 16;

fn ensure_thunk_registered() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        crate::closure::js_register_closure_arity(interp_thunk as *const u8, THUNK_ARITY as u32);
    });
}

/// The single native entry every interpreted closure shares. Reads its
/// identity + environment from capture slots and runs the tree-walker.
extern "C" fn interp_thunk(
    closure: *const crate::closure::ClosureHeader,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    a4: f64,
    a5: f64,
    a6: f64,
    a7: f64,
    a8: f64,
    a9: f64,
    a10: f64,
    a11: f64,
    a12: f64,
    a13: f64,
    a14: f64,
    a15: f64,
) -> f64 {
    let fn_id = crate::closure::js_closure_get_capture_f64(closure, 0) as u32;
    let def_env = f64::from_bits(crate::closure::js_closure_get_capture_bits(closure, 1));
    let this_bits = crate::closure::js_closure_get_capture_bits(closure, 2);
    let this = if this_bits == NO_LEXICAL_THIS {
        crate::object::js_implicit_this_get()
    } else {
        f64::from_bits(this_bits)
    };
    let args = [
        a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15,
    ];
    invoke_interp_fn(fn_id, def_env, this, &args)
}

/// Run one interpreted call: fresh scope chained to the defining env, params
/// bound (destructuring + defaults), vars hoisted, function declarations
/// hoisted, body executed.
pub(crate) fn invoke_interp_fn(fn_id: u32, def_env: f64, this: f64, args: &[f64]) -> f64 {
    let fun = match lookup_fn(fn_id) {
        Some(f) => f,
        None => bridge::throw_type_error(
            "perry runtime interpreter (#6559): interpreted function is not available on this \
             thread",
        ),
    };
    if call_depth_enter().is_err() {
        bridge::throw_range_error("Maximum call stack size exceeded (interpreted)");
    }
    let base = roots_len();
    // Root the incoming this + every argument: default-value evaluation and
    // destructuring allocate, which can move any of them.
    let this_idx = root_push(this);
    let ret_idx = root_push(bridge::undefined());
    let def_env_idx = root_push(def_env);
    // Root only the arguments a parameter will actually bind. The thunk always
    // delivers THUNK_ARITY slots, but a validator declaring one or two params
    // (the overwhelming case) needn't pay 16 `root_push`es per call — there is
    // no `arguments` object, so surplus args are unobservable (#6693).
    let nargs = fun.params.len().min(args.len()).min(THUNK_ARITY);
    let mut arg_idxs = [0usize; THUNK_ARITY];
    for (i, slot) in arg_idxs.iter_mut().enumerate().take(nargs) {
        *slot = root_push(args[i]);
    }
    let call_env = env::env_new(root_get(def_env_idx));
    let env_idx = root_push(call_env);

    let ctx = Ctx { this_idx, ret_idx };

    // Parameters.
    for (i, pat) in fun.params.iter().enumerate() {
        let value = if i < nargs {
            root_get(arg_idxs[i])
        } else {
            bridge::undefined()
        };
        bind_pattern(&ctx, pat, value, env_idx, true);
    }
    // `var` hoisting (params win over the undefined pre-binding).
    for name in &fun.hoisted_vars {
        if !env::is_bound(root_get(env_idx), name) {
            env::define(root_get(env_idx), name, bridge::undefined());
        }
    }

    match &fun.body {
        InterpBody::Expr(e) => {
            let v = eval_expr(&ctx, e, env_idx);
            root_set(ret_idx, v);
        }
        InterpBody::Block(stmts) => {
            hoist_fn_decls(&ctx, stmts, env_idx);
            let _ = exec_stmts(&ctx, stmts, env_idx);
        }
    }

    let result = root_get(ret_idx);
    roots_truncate(base);
    call_depth_leave();
    result
}

/// Create the interpreted closure for a function expression / declaration /
/// arrow at its evaluation site.
pub(crate) fn make_function_value(
    ctx: &Ctx,
    params: Vec<ast::Pat>,
    body: InterpBody,
    is_arrow: bool,
    name: Option<String>,
    node_addr: usize,
    env_idx: usize,
) -> f64 {
    let fn_id = fn_id_for_node(node_addr, || build_interp_fn(params, body));
    let lexical_this = if is_arrow {
        Some(root_get(ctx.this_idx))
    } else {
        None
    };
    if let Some(fn_name) = name.filter(|_| !is_arrow) {
        // Named function expression: its body sees its own name. Chain a
        // one-binding scope between the defining env and the body env.
        let name_env = env::env_new(root_get(env_idx));
        let name_env_idx = root_push(name_env);
        let closure = alloc_interp_closure(fn_id, root_get(name_env_idx), lexical_this);
        let closure_idx = root_push(closure);
        env::define(root_get(name_env_idx), &fn_name, root_get(closure_idx));
        let closure = root_get(closure_idx);
        roots_truncate(name_env_idx);
        closure
    } else {
        alloc_interp_closure(fn_id, root_get(env_idx), lexical_this)
    }
}

/// Hoist `function f(...) {}` declarations of a statement list into the
/// current scope (evaluated before any statement runs — ajv's serializers
/// call later-declared helpers).
fn hoist_fn_decls(ctx: &Ctx, stmts: &[ast::Stmt], env_idx: usize) {
    for stmt in stmts {
        if let ast::Stmt::Decl(ast::Decl::Fn(f)) = stmt {
            if f.function.is_generator || f.function.is_async {
                throw_unsupported("generator/async function declaration");
            }
            let name = f.ident.sym.to_string();
            let value = make_function_value(
                ctx,
                f.function.params.iter().map(|p| p.pat.clone()).collect(),
                InterpBody::Block(
                    f.function
                        .body
                        .as_ref()
                        .map(|b| b.stmts.clone())
                        .unwrap_or_default(),
                ),
                false,
                Some(name.clone()),
                f.function.as_ref() as *const ast::Function as usize,
                env_idx,
            );
            let value_idx = root_push(value);
            env::define(root_get(env_idx), &name, root_get(value_idx));
            roots_truncate(value_idx);
        }
    }
}

// ── statements ─────────────────────────────────────────────────────────────

pub(crate) fn exec_stmts(ctx: &Ctx, stmts: &[ast::Stmt], env_idx: usize) -> Flow {
    for stmt in stmts {
        match exec_stmt(ctx, stmt, env_idx) {
            Flow::Normal => {}
            other => return other,
        }
    }
    Flow::Normal
}

fn exec_block_scope(ctx: &Ctx, block: &ast::BlockStmt, env_idx: usize) -> Flow {
    let child = env::env_new(root_get(env_idx));
    let child_idx = root_push(child);
    hoist_fn_decls(ctx, &block.stmts, child_idx);
    let flow = exec_stmts(ctx, &block.stmts, child_idx);
    roots_truncate(child_idx);
    flow
}

pub(crate) fn exec_stmt(ctx: &Ctx, stmt: &ast::Stmt, env_idx: usize) -> Flow {
    use ast::Stmt::*;
    match stmt {
        Expr(e) => {
            let _ = eval_expr(ctx, &e.expr, env_idx);
            Flow::Normal
        }
        Decl(decl) => exec_decl(ctx, decl, env_idx),
        Block(b) => exec_block_scope(ctx, b, env_idx),
        Empty(_) => Flow::Normal,
        Debugger(_) => Flow::Normal,
        Return(r) => {
            let value = match &r.arg {
                Some(e) => eval_expr(ctx, e, env_idx),
                None => bridge::undefined(),
            };
            root_set(ctx.ret_idx, value);
            Flow::Return
        }
        If(i) => {
            let test = eval_expr(ctx, &i.test, env_idx);
            if bridge::truthy(test) {
                exec_stmt(ctx, &i.cons, env_idx)
            } else if let Some(alt) = &i.alt {
                exec_stmt(ctx, alt, env_idx)
            } else {
                Flow::Normal
            }
        }
        Throw(t) => {
            let value = eval_expr(ctx, &t.arg, env_idx);
            crate::exception::js_throw(value)
        }
        While(w) => exec_while(ctx, w, env_idx, None),
        DoWhile(w) => exec_do_while(ctx, w, env_idx, None),
        For(f) => exec_for(ctx, f, env_idx, None),
        ForIn(f) => exec_for_in(ctx, f, env_idx, None),
        ForOf(f) => exec_for_of(ctx, f, env_idx, None),
        Switch(sw) => exec_switch(ctx, sw, env_idx),
        Break(b) => Flow::Break(b.label.as_ref().map(|l| l.sym.to_string())),
        Continue(c) => Flow::Continue(c.label.as_ref().map(|l| l.sym.to_string())),
        Labeled(l) => exec_labeled(ctx, l, env_idx),
        Try(t) => exec_try(ctx, t, env_idx),
        With(_) => throw_unsupported("with statement"),
    }
}

fn exec_decl(ctx: &Ctx, decl: &ast::Decl, env_idx: usize) -> Flow {
    match decl {
        ast::Decl::Var(v) => {
            exec_var_decl(ctx, v, env_idx);
            Flow::Normal
        }
        // Handled by `hoist_fn_decls` before the statement list runs.
        ast::Decl::Fn(_) => Flow::Normal,
        ast::Decl::Class(_) => throw_unsupported("class declaration"),
        ast::Decl::Using(_) => throw_unsupported("using declaration"),
        // TS-only declaration forms can't appear (`.cjs` parse), but keep the
        // diagnostic for safety.
        _ => throw_unsupported("TypeScript declaration in runtime-generated code"),
    }
}

fn exec_var_decl(ctx: &Ctx, v: &ast::VarDecl, env_idx: usize) {
    let is_var = v.kind == ast::VarDeclKind::Var;
    for d in &v.decls {
        if is_var && d.init.is_none() {
            // `var x;` re-declaration must NOT reset an existing value — the
            // binding already exists undefined via function-entry hoisting.
            continue;
        }
        let value = match &d.init {
            Some(e) => eval_expr(ctx, e, env_idx),
            None => bridge::undefined(),
        };
        // `var` writes the FUNCTION-scope hoisted binding (assign through
        // the chain); let/const declare in the current block scope.
        bind_pattern(ctx, &d.name, value, env_idx, !is_var);
    }
}

/// Bind `pat = value` into `env_idx`. `declare` distinguishes declaration
/// (define in this scope) from destructuring assignment (assign through the
/// chain).
pub(crate) fn bind_pattern(
    ctx: &Ctx,
    pat: &ast::Pat,
    value: f64,
    env_idx: usize,
    declare: bool,
) {
    match pat {
        ast::Pat::Ident(b) => {
            let name: &str = &b.id.sym;
            if declare {
                let value_idx = root_push(value);
                env::define(root_get(env_idx), name, root_get(value_idx));
                roots_truncate(value_idx);
            } else {
                env::assign(root_get(env_idx), name, value);
            }
        }
        ast::Pat::Assign(a) => {
            let value = if bridge::is_undefined(value) {
                eval_expr(ctx, &a.right, env_idx)
            } else {
                value
            };
            bind_pattern(ctx, &a.left, value, env_idx, declare);
        }
        ast::Pat::Object(o) => {
            if bridge::is_nullish(value) {
                bridge::throw_type_error("Cannot destructure a nullish value");
            }
            let src_idx = root_push(value);
            for prop in &o.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let sub = read_pat_key(ctx, &kv.key, src_idx, env_idx);
                        bind_pattern(ctx, &kv.value, sub, env_idx, declare);
                    }
                    ast::ObjectPatProp::Assign(a) => {
                        let name: &str = &a.key.id.sym;
                        let mut sub = bridge::get_member(root_get(src_idx), name);
                        if bridge::is_undefined(sub) {
                            if let Some(default) = &a.value {
                                sub = eval_expr(ctx, default, env_idx);
                            }
                        }
                        let sub_idx = root_push(sub);
                        if declare {
                            env::define(root_get(env_idx), name, root_get(sub_idx));
                        } else {
                            env::assign(root_get(env_idx), name, root_get(sub_idx));
                        }
                        roots_truncate(sub_idx);
                    }
                    ast::ObjectPatProp::Rest(_) => {
                        throw_unsupported("object rest pattern ({...rest})")
                    }
                }
            }
            roots_truncate(src_idx);
        }
        ast::Pat::Array(a) => {
            if bridge::is_nullish(value) {
                bridge::throw_type_error("Cannot destructure a nullish value");
            }
            let src_idx = root_push(value);
            for (i, elem) in a.elems.iter().enumerate() {
                let Some(p) = elem else { continue };
                if let ast::Pat::Rest(_) = p {
                    throw_unsupported("array rest pattern ([...rest])");
                }
                let idx_key = bridge::make_number(i as f64);
                let sub = bridge::get_index(root_get(src_idx), idx_key);
                bind_pattern(ctx, p, sub, env_idx, declare);
            }
            roots_truncate(src_idx);
        }
        ast::Pat::Rest(_) => throw_unsupported("rest pattern"),
        ast::Pat::Expr(e) => {
            // Destructuring-assignment member target: `[a.x] = arr`.
            super::expr::assign_to_target_expr(ctx, e, value, env_idx);
        }
        ast::Pat::Invalid(_) => throw_unsupported("invalid pattern"),
    }
}

fn read_pat_key(ctx: &Ctx, key: &ast::PropName, src_idx: usize, env_idx: usize) -> f64 {
    match key {
        ast::PropName::Ident(i) => bridge::get_member(root_get(src_idx), &i.sym),
        ast::PropName::Str(s) => {
            bridge::get_member(root_get(src_idx), &String::from_utf8_lossy(s.value.as_bytes()))
        }
        ast::PropName::Num(n) => {
            let k = bridge::make_number(n.value);
            bridge::get_index(root_get(src_idx), k)
        }
        ast::PropName::Computed(c) => {
            let k = eval_expr(ctx, &c.expr, env_idx);
            bridge::get_index(root_get(src_idx), k)
        }
        ast::PropName::BigInt(_) => throw_unsupported("bigint property key in pattern"),
    }
}

// ── loops ──────────────────────────────────────────────────────────────────

/// Shared break/continue handling. Returns `Some(flow)` when the loop must
/// terminate with `flow`, `None` to continue iterating.
fn loop_flow(flow: Flow, label: Option<&str>) -> Option<Flow> {
    match flow {
        Flow::Normal => None,
        Flow::Continue(None) => None,
        Flow::Continue(Some(l)) if Some(l.as_str()) == label => None,
        Flow::Break(None) => Some(Flow::Normal),
        Flow::Break(Some(l)) if Some(l.as_str()) == label => Some(Flow::Normal),
        other => Some(other),
    }
}

fn exec_while(ctx: &Ctx, w: &ast::WhileStmt, env_idx: usize, label: Option<&str>) -> Flow {
    loop {
        let test = eval_expr(ctx, &w.test, env_idx);
        if !bridge::truthy(test) {
            return Flow::Normal;
        }
        if let Some(flow) = loop_flow(exec_stmt(ctx, &w.body, env_idx), label) {
            return flow;
        }
    }
}

fn exec_do_while(
    ctx: &Ctx,
    w: &ast::DoWhileStmt,
    env_idx: usize,
    label: Option<&str>,
) -> Flow {
    loop {
        if let Some(flow) = loop_flow(exec_stmt(ctx, &w.body, env_idx), label) {
            return flow;
        }
        let test = eval_expr(ctx, &w.test, env_idx);
        if !bridge::truthy(test) {
            return Flow::Normal;
        }
    }
}

fn exec_for(ctx: &Ctx, f: &ast::ForStmt, env_idx: usize, label: Option<&str>) -> Flow {
    // One loop scope holds the init bindings (`for (let i0=0; …)`); the
    // generated code never closes over per-iteration bindings, so a single
    // scope matches observable behavior.
    let loop_env = env::env_new(root_get(env_idx));
    let loop_env_idx = root_push(loop_env);
    match &f.init {
        Some(ast::VarDeclOrExpr::VarDecl(v)) => exec_var_decl(ctx, v, loop_env_idx),
        Some(ast::VarDeclOrExpr::Expr(e)) => {
            let _ = eval_expr(ctx, e, loop_env_idx);
        }
        None => {}
    }
    let flow = loop {
        if let Some(test) = &f.test {
            let t = eval_expr(ctx, test, loop_env_idx);
            if !bridge::truthy(t) {
                break Flow::Normal;
            }
        }
        if let Some(flow) = loop_flow(exec_stmt(ctx, &f.body, loop_env_idx), label) {
            break flow;
        }
        if let Some(update) = &f.update {
            let _ = eval_expr(ctx, update, loop_env_idx);
        }
    };
    roots_truncate(loop_env_idx);
    flow
}

fn bind_for_head(ctx: &Ctx, head: &ast::ForHead, value: f64, env_idx: usize) {
    match head {
        ast::ForHead::VarDecl(v) => {
            let Some(d) = v.decls.first() else {
                return;
            };
            bind_pattern(ctx, &d.name, value, env_idx, true);
        }
        ast::ForHead::Pat(p) => bind_pattern(ctx, p, value, env_idx, false),
        ast::ForHead::UsingDecl(_) => throw_unsupported("using declaration in for head"),
    }
}

fn exec_for_in(ctx: &Ctx, f: &ast::ForInStmt, env_idx: usize, label: Option<&str>) -> Flow {
    let obj = eval_expr(ctx, &f.right, env_idx);
    if bridge::is_nullish(obj) {
        return Flow::Normal;
    }
    let keys = bridge::for_in_keys(obj);
    let keys_idx = root_push(keys);
    let len = bridge::array_length(root_get(keys_idx));
    let mut flow = Flow::Normal;
    for i in 0..len {
        let key = bridge::array_get(root_get(keys_idx), i);
        let iter_env = env::env_new(root_get(env_idx));
        let iter_env_idx = root_push(iter_env);
        bind_for_head(ctx, &f.left, key, iter_env_idx);
        let body_flow = exec_stmt(ctx, &f.body, iter_env_idx);
        roots_truncate(iter_env_idx);
        if let Some(f) = loop_flow(body_flow, label) {
            flow = f;
            break;
        }
    }
    roots_truncate(keys_idx);
    flow
}

fn exec_for_of(ctx: &Ctx, f: &ast::ForOfStmt, env_idx: usize, label: Option<&str>) -> Flow {
    if f.is_await {
        throw_unsupported("for await");
    }
    let iterable = eval_expr(ctx, &f.right, env_idx);
    let iter_idx = root_push(iterable);
    // Arrays and strings cover the generated-code corpus; other iterables
    // (Map/Set/generators) throw the diagnostic below so gaps surface.
    let value = root_get(iter_idx);
    if bridge::is_array_value(value) {
        let mut flow = Flow::Normal;
        let len = bridge::array_length(root_get(iter_idx));
        for i in 0..len {
            let elem = bridge::array_get(root_get(iter_idx), i);
            let iter_env = env::env_new(root_get(env_idx));
            let iter_env_idx = root_push(iter_env);
            bind_for_head(ctx, &f.left, elem, iter_env_idx);
            let body_flow = exec_stmt(ctx, &f.body, iter_env_idx);
            roots_truncate(iter_env_idx);
            if let Some(fl) = loop_flow(body_flow, label) {
                flow = fl;
                break;
            }
        }
        roots_truncate(iter_idx);
        return flow;
    }
    if let Some(s) = bridge::read_string(value) {
        let mut flow = Flow::Normal;
        for ch in s.chars() {
            let elem = bridge::make_string(&ch.to_string());
            let iter_env = env::env_new(root_get(env_idx));
            let iter_env_idx = root_push(iter_env);
            bind_for_head(ctx, &f.left, elem, iter_env_idx);
            let body_flow = exec_stmt(ctx, &f.body, iter_env_idx);
            roots_truncate(iter_env_idx);
            if let Some(fl) = loop_flow(body_flow, label) {
                flow = fl;
                break;
            }
        }
        roots_truncate(iter_idx);
        return flow;
    }
    roots_truncate(iter_idx);
    throw_unsupported("for-of over a non-array, non-string iterable")
}

fn exec_labeled(ctx: &Ctx, l: &ast::LabeledStmt, env_idx: usize) -> Flow {
    let label: String = l.label.sym.to_string();
    let flow = match l.body.as_ref() {
        ast::Stmt::While(w) => exec_while(ctx, w, env_idx, Some(&label)),
        ast::Stmt::DoWhile(w) => exec_do_while(ctx, w, env_idx, Some(&label)),
        ast::Stmt::For(f) => exec_for(ctx, f, env_idx, Some(&label)),
        ast::Stmt::ForIn(f) => exec_for_in(ctx, f, env_idx, Some(&label)),
        ast::Stmt::ForOf(f) => exec_for_of(ctx, f, env_idx, Some(&label)),
        other => exec_stmt(ctx, other, env_idx),
    };
    match flow {
        Flow::Break(Some(l2)) if l2 == label => Flow::Normal,
        other => other,
    }
}

fn exec_switch(ctx: &Ctx, sw: &ast::SwitchStmt, env_idx: usize) -> Flow {
    let disc = eval_expr(ctx, &sw.discriminant, env_idx);
    let disc_idx = root_push(disc);
    // The case list shares one block scope (spec: a single lexical env).
    let case_env = env::env_new(root_get(env_idx));
    let case_env_idx = root_push(case_env);

    let mut start: Option<usize> = None;
    for (i, case) in sw.cases.iter().enumerate() {
        if let Some(test) = &case.test {
            let t = eval_expr(ctx, test, case_env_idx);
            if bridge::strict_equals(root_get(disc_idx), t) {
                start = Some(i);
                break;
            }
        }
    }
    if start.is_none() {
        start = sw.cases.iter().position(|c| c.test.is_none());
    }
    let mut flow = Flow::Normal;
    if let Some(start) = start {
        'cases: for case in &sw.cases[start..] {
            match exec_stmts(ctx, &case.cons, case_env_idx) {
                Flow::Normal => {}
                Flow::Break(None) => {
                    flow = Flow::Normal;
                    break 'cases;
                }
                other => {
                    flow = other;
                    break 'cases;
                }
            }
        }
    }
    roots_truncate(disc_idx);
    flow
}

// ── try/catch/finally ──────────────────────────────────────────────────────

/// Interpreted `try`: Rust-side setjmp landing pads through the runtime's
/// exception machinery — the same idiom as the microtask pump
/// (`promise/microtasks.rs`). Each setjmp stays in its frame for the whole
/// protected region; `js_throw` restores the interpreter's rooted stack from
/// the per-try-depth savepoint before the longjmp lands.
///
/// Structure: the try+catch pair runs under `exec_try_catch`; when a
/// finalizer exists, a SECOND outer trap wraps that whole pair so `finally`
/// runs on every exit path — including a throw out of the CATCH body, which
/// a single-trap shape would miss.
fn exec_try(ctx: &Ctx, t: &ast::TryStmt, env_idx: usize) -> Flow {
    use crate::ffi::setjmp::setjmp;

    let Some(finalizer) = &t.finalizer else {
        return exec_try_catch(ctx, t, env_idx);
    };
    let trap = crate::exception::js_try_push();
    // SAFETY: this frame stays alive for the whole protected region; the cast
    // matches libc's signature (see ffi::setjmp).
    let jumped = unsafe { setjmp(trap as *mut std::os::raw::c_int) };
    if jumped == 0 {
        let flow = exec_try_catch(ctx, t, env_idx);
        crate::exception::js_try_end();
        match exec_block_scope(ctx, finalizer, env_idx) {
            Flow::Normal => flow,
            // An abrupt finalizer completion replaces the try/catch result.
            abrupt => abrupt,
        }
    } else {
        // try (or catch) threw. Run the finalizer, then rethrow — unless the
        // finalizer itself completes abruptly, which swallows the exception
        // (spec Completion-record semantics).
        crate::exception::js_try_end();
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        let exc_idx = root_push(exc);
        match exec_block_scope(ctx, finalizer, env_idx) {
            Flow::Normal => {
                let exc = root_get(exc_idx);
                roots_truncate(exc_idx);
                crate::exception::js_throw(exc)
            }
            abrupt => {
                roots_truncate(exc_idx);
                abrupt
            }
        }
    }
}

/// The try-block + catch-handler pair (no finalizer handling).
#[inline(never)]
fn exec_try_catch(ctx: &Ctx, t: &ast::TryStmt, env_idx: usize) -> Flow {
    use crate::ffi::setjmp::setjmp;

    let trap = crate::exception::js_try_push();
    // SAFETY: see exec_try.
    let jumped = unsafe { setjmp(trap as *mut std::os::raw::c_int) };
    if jumped == 0 {
        let flow = protected_block(ctx, t, env_idx);
        crate::exception::js_try_end();
        return flow;
    }
    // A throw from the try block landed here. The pending exception is
    // live; the interpreter savepoint has already restored the rooted stack
    // + call depth to this try's entry state.
    crate::exception::js_try_end();
    let exc = crate::exception::js_get_exception();
    crate::exception::js_clear_exception();

    let Some(handler) = &t.handler else {
        // No catch: let the outer trap (the finalizer wrapper — `try {} finally {}`
        // is the only parse without either) or the caller's trap take it.
        crate::exception::js_throw(exc)
    };
    let exc_idx = root_push(exc);
    let catch_env = env::env_new(root_get(env_idx));
    let catch_env_idx = root_push(catch_env);
    if let Some(param) = &handler.param {
        bind_pattern(ctx, param, root_get(exc_idx), catch_env_idx, true);
    }
    hoist_fn_decls(ctx, &handler.body.stmts, catch_env_idx);
    let flow = exec_stmts(ctx, &handler.body.stmts, catch_env_idx);
    roots_truncate(exc_idx);
    flow
}

#[inline(never)]
fn protected_block(ctx: &Ctx, t: &ast::TryStmt, env_idx: usize) -> Flow {
    exec_block_scope(ctx, &t.block, env_idx)
}
