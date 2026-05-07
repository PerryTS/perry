//! Issue #535 — `perry/ui` `state<T>` desugar pass for non-HarmonyOS targets.
//!
//! Pre-fix: `state<T>(initial)` had no codegen lowering on macOS / iOS / Android
//! / GTK4 / Windows; the strict-API gate raised a hard compile error
//! ("'state' is not a known function"). Only `--target harmonyos` worked,
//! because `perry-codegen-arkts` runs its own harvest that rewrites these
//! shapes into `setText` calls before LLVM ever sees them. The d.ts comment
//! at `types/perry/ui/index.d.ts:263-265` documented this as "deferred to
//! v6.5", which blocked any multi-screen native app per #535.
//!
//! What this pass does (mirrors arkts's collect_state_bindings +
//! rewrite_state_calls_in_stmts at `crates/perry-codegen-arkts/src/lib.rs:1233`
//! and `:3725`, but emits target-agnostic HIR so the LLVM backend can
//! lower it):
//!
//! 1. Walk `module.init` for `let x = state(initial)` declarations. Assign
//!    each a synthetic id (`__state_<N>`).
//!
//! 2. Replace the declaration's initializer with `Expr::Undefined` so the
//!    local survives any not-yet-rewritten escape (function-arg pass, array
//!    push, etc.) without crashing codegen, then prepend a synthetic
//!    `__state_init("synth_id", initial)` call to the init list so the
//!    runtime registry has the initial value before any reader fires.
//!
//! 3. Rewrite every `x.set(v)` / `x.get()` / `x.value` / `x.text()` use
//!    across `module.init`, every function body, and every nested closure
//!    body to the synth-id-keyed runtime equivalent:
//!
//!    - `x.set(v)` → `__state_set("synth_id", v)` — runtime updates the
//!      registry slot AND fires `perry_arkts_set_text` so any text widget
//!      registered under `synth_id` re-renders.
//!    - `x.get()` / `x.value` → `__state_get("synth_id")` — reads from the
//!      registry. Returns `undefined` for unknown ids (matches the JS
//!      `State<T>` semantics for an uninitialized cell).
//!    - `x.text()` → `Text("<initial-as-string>", "synth_id")`. The 2-arg
//!      `Text` form already routes through `perry_ui_text_create_with_id`
//!      which calls `perry_arkts_register_text_id`, so the widget joins
//!      the setText dispatch table automatically.
//!
//! Limitations (v0.5.617):
//!
//! - Only matches `state(...)` declared via `let`/`const` at the top level
//!   of `module.init`. Declarations inside function bodies aren't tracked
//!   yet; they would compile-error the same as today. Real-world apps
//!   (per #535's repro) use top-level state, so this is the right first cut.
//! - Only matches the canonical method-call shapes — `x.set(v)`, `x.get()`,
//!   `x.value`, `x.text()`. If a state escapes through a function arg /
//!   array / object property, the call site there has no `LocalGet(x)`
//!   anchor and the rewrite skips it. Today: that's a follow-up; the
//!   `let x = undefined` shim at least keeps the program compilable.
//! - `.text()` snapshots the initial value at compile time using the
//!   literal initializer. Computed-initial states get an empty initial
//!   string — the first `.set()` corrects it at runtime.
//!
//! HarmonyOS: this pass is gated OFF in `collect_modules.rs` so
//! `perry-codegen-arkts`'s harvest stays the source of truth there.

use perry_hir::walker::walk_expr_children_mut;
use perry_hir::{Expr, Module, Param, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::HashMap;

/// Counters threaded through the rewrite for fresh `LocalId` / `FuncId`
/// allocation. The NavStack lowering needs both: each call site spawns a
/// closure (one fresh `FuncId`) holding `1 + N` fresh local bindings (host
/// + one per route).
struct FreshIds {
    next_local: LocalId,
    next_func: FuncId,
}

impl FreshIds {
    fn fresh_local(&mut self) -> LocalId {
        let id = self.next_local;
        self.next_local += 1;
        id
    }
    fn fresh_func(&mut self) -> FuncId {
        let id = self.next_func;
        self.next_func += 1;
        id
    }
}

/// One `state<T>` declaration the pass has decided to rewrite.
struct StateBinding {
    /// Synthetic id baked into all rewritten call sites. Format
    /// `__state_<N>` (zero-based, declaration order in `module.init`).
    /// Stable across re-runs because the iteration order is deterministic.
    synth_id: String,
    /// The original initial-value expression. Used by `.text()` rewrites
    /// to compute the literal string the bound `Text` widget displays
    /// before the first `.set()` call updates it.
    initial: Expr,
}

/// Run the desugar. No-op when the module has no `state<T>` declarations.
pub fn run(module: &mut Module) {
    let bindings = collect_state_bindings(&module.init);
    if bindings.is_empty() {
        return;
    }
    rewrite_init_decls(&mut module.init, &bindings);
    let mut fresh = FreshIds {
        next_local: compute_max_local_id(module).saturating_add(1),
        next_func: compute_max_func_id(module).saturating_add(1),
    };
    rewrite_stmts(&mut module.init, &bindings, &mut fresh);
    for func in module.functions.iter_mut() {
        rewrite_stmts(&mut func.body, &bindings, &mut fresh);
    }
}

/// Walk the entire module to find the highest `LocalId` already in use.
/// Mirrors `async_to_generator::compute_max_local_id` shape (param scan
/// + stmt scan + class member scan) so allocations don't collide with
/// `ctx.fresh_local()` ids inside class methods or with later transforms
/// that allocate from the same global namespace.
fn compute_max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    let mut walk_stmts = |stmts: &[Stmt], max_id: &mut LocalId| {
        for stmt in stmts {
            scan_stmt_local_ids(stmt, max_id);
        }
    };
    for func in &module.functions {
        for p in &func.params {
            max_id = max_id.max(p.id);
        }
        walk_stmts(&func.body, &mut max_id);
    }
    walk_stmts(&module.init, &mut max_id);
    for global in &module.globals {
        max_id = max_id.max(global.id);
    }
    for class in &module.classes {
        for method in &class.methods {
            for p in &method.params {
                max_id = max_id.max(p.id);
            }
            walk_stmts(&method.body, &mut max_id);
        }
        if let Some(ctor) = &class.constructor {
            for p in &ctor.params {
                max_id = max_id.max(p.id);
            }
            walk_stmts(&ctor.body, &mut max_id);
        }
    }
    max_id
}

fn compute_max_func_id(module: &Module) -> FuncId {
    let mut max_id: FuncId = 0;
    for func in &module.functions {
        max_id = max_id.max(func.id);
    }
    let mut walk_stmts = |stmts: &[Stmt], max_id: &mut FuncId| {
        for stmt in stmts {
            scan_stmt_func_ids(stmt, max_id);
        }
    };
    walk_stmts(&module.init, &mut max_id);
    for func in &module.functions {
        walk_stmts(&func.body, &mut max_id);
    }
    for class in &module.classes {
        for method in &class.methods {
            max_id = max_id.max(method.id);
            walk_stmts(&method.body, &mut max_id);
        }
        if let Some(ctor) = &class.constructor {
            max_id = max_id.max(ctor.id);
            walk_stmts(&ctor.body, &mut max_id);
        }
    }
    max_id
}

fn scan_stmt_local_ids(stmt: &Stmt, max_id: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *max_id = (*max_id).max(*id);
            if let Some(e) = init {
                scan_expr_local_ids(e, max_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_local_ids(e, max_id),
        Stmt::Return(Some(e)) => scan_expr_local_ids(e, max_id),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_local_ids(condition, max_id);
            for s in then_branch {
                scan_stmt_local_ids(s, max_id);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    scan_stmt_local_ids(s, max_id);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_local_ids(condition, max_id);
            for s in body {
                scan_stmt_local_ids(s, max_id);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_local_ids(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_local_ids(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_local_ids(u, max_id);
            }
            for s in body {
                scan_stmt_local_ids(s, max_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            for s in body {
                scan_stmt_local_ids(s, max_id);
            }
            if let Some(c) = catch {
                if let Some((id, _)) = c.param {
                    *max_id = (*max_id).max(id);
                }
                for s in &c.body {
                    scan_stmt_local_ids(s, max_id);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    scan_stmt_local_ids(s, max_id);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_local_ids(discriminant, max_id);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_local_ids(t, max_id);
                }
                for s in &case.body {
                    scan_stmt_local_ids(s, max_id);
                }
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_local_ids(body, max_id),
        _ => {}
    }
}

fn scan_expr_local_ids(e: &Expr, max_id: &mut LocalId) {
    match e {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
            *max_id = (*max_id).max(*id);
        }
        _ => {}
    }
    use perry_hir::walker::walk_expr_children;
    walk_expr_children(e, &mut |child| scan_expr_local_ids(child, max_id));
    if let Expr::Closure { params, body, .. } = e {
        for p in params {
            *max_id = (*max_id).max(p.id);
        }
        for s in body {
            scan_stmt_local_ids(s, max_id);
        }
    }
}

fn scan_stmt_func_ids(stmt: &Stmt, max_id: &mut FuncId) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => scan_expr_func_ids(e, max_id),
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_func_ids(e, max_id),
        Stmt::Return(Some(e)) => scan_expr_func_ids(e, max_id),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_func_ids(condition, max_id);
            for s in then_branch {
                scan_stmt_func_ids(s, max_id);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    scan_stmt_func_ids(s, max_id);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_func_ids(condition, max_id);
            for s in body {
                scan_stmt_func_ids(s, max_id);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_func_ids(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_func_ids(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_func_ids(u, max_id);
            }
            for s in body {
                scan_stmt_func_ids(s, max_id);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            for s in body {
                scan_stmt_func_ids(s, max_id);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    scan_stmt_func_ids(s, max_id);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    scan_stmt_func_ids(s, max_id);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_func_ids(discriminant, max_id);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_func_ids(t, max_id);
                }
                for s in &case.body {
                    scan_stmt_func_ids(s, max_id);
                }
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_func_ids(body, max_id),
        _ => {}
    }
}

fn scan_expr_func_ids(e: &Expr, max_id: &mut FuncId) {
    match e {
        Expr::FuncRef(id) => *max_id = (*max_id).max(*id),
        Expr::Closure { func_id, body, .. } => {
            *max_id = (*max_id).max(*func_id);
            for s in body {
                scan_stmt_func_ids(s, max_id);
            }
        }
        _ => {}
    }
    use perry_hir::walker::walk_expr_children;
    walk_expr_children(e, &mut |child| scan_expr_func_ids(child, max_id));
}

/// Walk `module.init` for `let x = state(initial)` and assign each a
/// synth id. Mirrors `perry-codegen-arkts::collect_state_bindings`.
fn collect_state_bindings(init: &[Stmt]) -> HashMap<LocalId, StateBinding> {
    let mut map = HashMap::new();
    let mut counter: usize = 0;
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(call_expr),
            ..
        } = stmt
        {
            if let Expr::NativeMethodCall {
                module,
                method,
                object: None,
                args,
                ..
            } = call_expr
            {
                if module == "perry/ui" && method == "state" && args.len() == 1 {
                    let synth_id = format!("__state_{}", counter);
                    counter += 1;
                    map.insert(
                        *id,
                        StateBinding {
                            synth_id,
                            initial: args[0].clone(),
                        },
                    );
                }
            }
        }
    }
    map
}

/// Replace every matched `let x = state(initial)` statement with a pair:
/// (1) `let x = undefined` to keep the LocalId alive for any not-rewritten
/// escape, and (2) a synthetic `__state_init("synth_id", initial)` call
/// that primes the runtime registry. The init must run BEFORE any
/// rewritten reader (`__state_get(...)`) downstream, so we place it
/// immediately after the declaration in source order.
fn rewrite_init_decls(init: &mut Vec<Stmt>, bindings: &HashMap<LocalId, StateBinding>) {
    let mut new_stmts: Vec<Stmt> = Vec::with_capacity(init.len() + bindings.len());
    for stmt in init.drain(..) {
        match stmt {
            Stmt::Let {
                id,
                ref name,
                ref ty,
                mutable,
                init: Some(_),
            } if bindings.contains_key(&id) => {
                let binding = &bindings[&id];
                new_stmts.push(Stmt::Let {
                    id,
                    name: name.clone(),
                    ty: ty.clone(),
                    mutable,
                    init: Some(Expr::Undefined),
                });
                new_stmts.push(Stmt::Expr(state_init_call(
                    &binding.synth_id,
                    binding.initial.clone(),
                )));
            }
            other => new_stmts.push(other),
        }
    }
    *init = new_stmts;
}

/// Recursively rewrite every `Stmt` in `stmts`. Descends into block-shaped
/// children (if/while/for/etc.) so closures buried in `Button(label, () =>
/// state.set(...))` are visited.
fn rewrite_stmts(
    stmts: &mut Vec<Stmt>,
    bindings: &HashMap<LocalId, StateBinding>,
    fresh: &mut FreshIds,
) {
    for stmt in stmts.iter_mut() {
        rewrite_stmt(stmt, bindings, fresh);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, bindings: &HashMap<LocalId, StateBinding>, fresh: &mut FreshIds) {
    match stmt {
        Stmt::Expr(e) => rewrite_expr(e, bindings, fresh),
        Stmt::Return(Some(e)) => rewrite_expr(e, bindings, fresh),
        Stmt::Throw(e) => rewrite_expr(e, bindings, fresh),
        Stmt::Let { init: Some(e), .. } => rewrite_expr(e, bindings, fresh),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(condition, bindings, fresh);
            rewrite_stmts(then_branch, bindings, fresh);
            if let Some(eb) = else_branch {
                rewrite_stmts(eb, bindings, fresh);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_expr(condition, bindings, fresh);
            rewrite_stmts(body, bindings, fresh);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                rewrite_stmt(i.as_mut(), bindings, fresh);
            }
            if let Some(c) = condition {
                rewrite_expr(c, bindings, fresh);
            }
            if let Some(u) = update {
                rewrite_expr(u, bindings, fresh);
            }
            rewrite_stmts(body, bindings, fresh);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmts(body, bindings, fresh);
            if let Some(c) = catch {
                rewrite_stmts(&mut c.body, bindings, fresh);
            }
            if let Some(f) = finally {
                rewrite_stmts(f, bindings, fresh);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_expr(discriminant, bindings, fresh);
            for case in cases {
                if let Some(t) = &mut case.test {
                    rewrite_expr(t, bindings, fresh);
                }
                rewrite_stmts(&mut case.body, bindings, fresh);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body.as_mut(), bindings, fresh),
        _ => {}
    }
}

/// Detect the four state-method shapes most-specific first, then fall
/// through to a generic recursion over child exprs. The recursion uses
/// `walk_expr_children_mut` plus an explicit closure-body descent
/// (`walk_expr_children_mut` intentionally doesn't enter Closure bodies —
/// see `crates/perry-hir/src/walker.rs:23-25`).
///
/// Children are rewritten BEFORE the outer match. This matters for
/// `state.set(state.get() + 1)` and similar nested patterns: without
/// the inner-first walk, the outer `.set` rewrite would clone the arg
/// expression containing the un-rewritten inner `.get()`, leaving it
/// as a plain `LocalGet(state).get()` on a holder that's now `undefined`.
fn rewrite_expr(e: &mut Expr, bindings: &HashMap<LocalId, StateBinding>, fresh: &mut FreshIds) {
    walk_expr_children_mut(e, &mut |child| rewrite_expr(child, bindings, fresh));

    if let Expr::Closure { body, .. } = e {
        rewrite_stmts(body, bindings, fresh);
    }

    if let Some(replacement) = try_rewrite_navstack(e, bindings, fresh) {
        *e = replacement;
        return;
    }

    if let Some(replacement) = try_rewrite_state_access(e, bindings) {
        *e = replacement;
    }
}

/// Detect `NavStack(LocalGet(state_id), Array([{name, body}, ...]))` where
/// `state_id` is one of our state bindings. Lower the call to an IIFE
/// (closure-as-Call with empty args) whose body:
///   1. Allocates the host via the existing 0-arg `NavStack()` form.
///   2. For each route: binds the body widget to a fresh local, calls
///      `widgetAddChild(host, body)`, then `__navstack_register_route(
///      synth_id, name, body)` which records the route + sets initial
///      visibility (hides any route whose name doesn't match the current
///      state value).
///   3. Returns the host.
///
/// The IIFE shape is the same one `try_desugar_reactive_text` uses at
/// HIR lowering time (`lower.rs:7367`); we replicate it here because
/// state_desugar runs after AST→HIR lowering and the call site is buried
/// in expression position (the `body:` of an `App({body: NavStack(...)})`
/// config), so we can't hoist the construction to surrounding statements.
///
/// Routes with non-string `name` literals or shapes other than the canonical
/// `{ name: string, body: Widget }` object literal silently bail and the
/// original NavStack call falls through to its 0-arg dispatch (no routing
/// behavior — same as today's pre-fix behavior, but at least no compile
/// error). Refs #535.
fn try_rewrite_navstack(
    e: &Expr,
    bindings: &HashMap<LocalId, StateBinding>,
    fresh: &mut FreshIds,
) -> Option<Expr> {
    let (state_id, route_array) = match e {
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            args,
            ..
        } if module == "perry/ui" && method == "NavStack" && args.len() == 2 => {
            let state_id = match &args[0] {
                Expr::LocalGet(id) => *id,
                _ => return None,
            };
            let route_array = match &args[1] {
                Expr::Array(items) => items,
                _ => return None,
            };
            (state_id, route_array)
        }
        _ => return None,
    };
    let binding = bindings.get(&state_id)?;
    let synth_id = binding.synth_id.clone();

    // Extract (name, body) pairs from each route. Route entries are HIR
    // anonymous-shape `New { class_name: __AnonShape_<hash>, args: [name,
    // body] }` — that's what `lower_object_literal` emits for `{name: ...,
    // body: ...}` (see lower_decl.rs's anon-shape harvest). We don't try
    // to handle other shapes (spread, dynamic property keys) — they bail.
    let mut routes: Vec<(String, Expr)> = Vec::with_capacity(route_array.len());
    for route in route_array {
        let shape_args = match route {
            Expr::New { args, .. } => args,
            _ => return None,
        };
        if shape_args.len() != 2 {
            return None;
        }
        let name = match &shape_args[0] {
            Expr::String(s) => s.clone(),
            _ => return None,
        };
        let body = shape_args[1].clone();
        routes.push((name, body));
    }
    if routes.is_empty() {
        return None;
    }

    let host_id = fresh.fresh_local();
    let mut body_stmts: Vec<Stmt> = Vec::with_capacity(2 + 3 * routes.len());

    // let __nav_host = NavStack();
    body_stmts.push(Stmt::Let {
        id: host_id,
        name: format!("__nav_host_{}", host_id),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "NavStack".to_string(),
            args: vec![],
        }),
    });

    for (route_name, route_body) in routes {
        let route_id = fresh.fresh_local();
        // let __nav_route_N = <body>;
        body_stmts.push(Stmt::Let {
            id: route_id,
            name: format!("__nav_route_{}", route_id),
            ty: Type::Any,
            mutable: false,
            init: Some(route_body),
        });
        // widgetAddChild(__nav_host, __nav_route_N);
        body_stmts.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "widgetAddChild".to_string(),
            args: vec![Expr::LocalGet(host_id), Expr::LocalGet(route_id)],
        }));
        // __navstack_register_route("__state_X", "name", __nav_route_N);
        body_stmts.push(Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "__navstack_register_route".to_string(),
            args: vec![
                Expr::String(synth_id.clone()),
                Expr::String(route_name),
                Expr::LocalGet(route_id),
            ],
        }));
    }
    body_stmts.push(Stmt::Return(Some(Expr::LocalGet(host_id))));

    let func_id = fresh.fresh_func();
    let closure = Expr::Closure {
        func_id,
        params: Vec::<Param>::new(),
        return_type: Type::Any,
        body: body_stmts,
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        enclosing_class: None,
        is_async: false,
    };
    Some(Expr::Call {
        callee: Box::new(closure),
        args: vec![],
        type_args: vec![],
    })
}

/// Attempt to rewrite `e` if it matches a state access on a known
/// state-bound local. Returns `Some(new_expr)` for a match, `None`
/// otherwise. Does not recurse into children — the caller does that
/// after this returns `None`.
fn try_rewrite_state_access(e: &Expr, bindings: &HashMap<LocalId, StateBinding>) -> Option<Expr> {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::PropertyGet { object, property } = callee.as_ref() {
            if let Expr::LocalGet(state_id) = object.as_ref() {
                if let Some(binding) = bindings.get(state_id) {
                    return match property.as_str() {
                        "get" if args.is_empty() => Some(state_get_call(&binding.synth_id)),
                        "set" if args.len() == 1 => {
                            Some(state_set_call(&binding.synth_id, args[0].clone()))
                        }
                        "text" if args.is_empty() => Some(state_text_call(binding)),
                        _ => None,
                    };
                }
            }
        }
    }
    if let Expr::PropertyGet { object, property } = e {
        if property == "value" {
            if let Expr::LocalGet(state_id) = object.as_ref() {
                if let Some(binding) = bindings.get(state_id) {
                    return Some(state_get_call(&binding.synth_id));
                }
            }
        }
    }
    None
}

fn state_init_call(synth_id: &str, initial: Expr) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "__state_init".to_string(),
        args: vec![Expr::String(synth_id.to_string()), initial],
    }
}

fn state_get_call(synth_id: &str) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "__state_get".to_string(),
        args: vec![Expr::String(synth_id.to_string())],
    }
}

fn state_set_call(synth_id: &str, value: Expr) -> Expr {
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "__state_set".to_string(),
        args: vec![Expr::String(synth_id.to_string()), value],
    }
}

fn state_text_call(binding: &StateBinding) -> Expr {
    let initial_str = match &binding.initial {
        Expr::String(s) => s.clone(),
        Expr::Number(n) => format_number(*n),
        Expr::Integer(n) => n.to_string(),
        Expr::Bool(b) => b.to_string(),
        _ => String::new(),
    };
    Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "Text".to_string(),
        args: vec![
            Expr::String(initial_str),
            Expr::String(binding.synth_id.clone()),
        ],
    }
}

/// Render a numeric literal the way JS's `String(n)` would for typical
/// initials — integers without a decimal point, fractions with one. Avoids
/// pulling in a heavier formatter for the v0.5.617 first cut.
fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}
