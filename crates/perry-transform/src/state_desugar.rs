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
use std::collections::{HashMap, HashSet};

// #5293: the max-LocalId / max-FuncId scans were copy-pasted here; route through
// the canonical exhaustive-walker implementations in `generator::id_scan`.
use crate::generator::{compute_max_func_id, compute_max_local_id};

/// Counters threaded through the rewrite for fresh `LocalId` / `FuncId`
/// allocation. The NavStack lowering needs both: each call site spawns a
/// closure (one fresh `FuncId`) holding `1 + N` fresh local bindings (host
/// + one per route).
///
/// Also carries `string_consts`, a `LocalId → &'static str` style env of
/// same-module `const X = "literal"` bindings. `try_rewrite_navstack` uses
/// it to const-fold `name: ROUTE_X` into `name: "x"` so the canonical
/// route-table pattern (route names factored into a shared `routes.ts`)
/// doesn't silently bail to the 0-arg `NavStack()` stub. See #1135.
struct FreshIds {
    next_local: LocalId,
    next_func: FuncId,
    string_consts: HashMap<LocalId, String>,
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
    let mut bindings = collect_state_bindings(&module.init);
    if bindings.is_empty() {
        return;
    }
    // #764: a binding declared as `const x = State(initial)` at module-init
    // that is *also* consumed by the handle-based state API surface
    // (`stateOnChange`, `stateBindTextfield`, `stateBindToggle`, etc.) MUST
    // stay handle-based. Those FFI entry points expect the integer Widget
    // handle returned by `perry_ui_state_create`, not a synthetic
    // `"__state_N"` registry key. Rewriting them to `__state_init` +
    // leaving `stateOnChange(x, ...)` untouched silently passes `undefined`
    // as the handle and the callback never fires — exactly the surface
    // symptom in #763 (the `Text(\`...${state.value}...\`)` IIFE desugared
    // by HIR lowering registers `stateOnChange` against a now-`undefined`
    // local). Drop those bindings from the rewrite map so they stay
    // pointing at the real `State(...)` constructor call.
    let handle_uses = collect_handle_based_state_uses(module);
    if !handle_uses.is_empty() {
        bindings.retain(|id, _| !handle_uses.contains(id));
        if bindings.is_empty() {
            return;
        }
    }
    rewrite_init_decls(&mut module.init, &bindings);
    let mut fresh = FreshIds {
        next_local: compute_max_local_id(module).saturating_add(1),
        next_func: compute_max_func_id(module).saturating_add(1),
        string_consts: collect_module_string_consts(&module.init),
    };
    rewrite_stmts(&mut module.init, &bindings, &mut fresh);
    for func in module.functions.iter_mut() {
        rewrite_stmts(&mut func.body, &bindings, &mut fresh);
    }
}

/// #1135 — collect same-module `const X = "literal"` bindings from the
/// top level of `module.init`. The map is consulted in `try_rewrite_navstack`
/// to fold `name: ROUTE_X` route entries into the literal string the rewrite
/// requires, so factoring route names into shared constants (the obvious,
/// idiomatic pattern) doesn't silently fall through to the empty 0-arg
/// `NavStack()` stub.
///
/// Scope: top-level of `module.init` only. Nested-scope `const`s would need
/// scope tracking we don't have here and the `let __nav_route_N = body`
/// scope is built fresh anyway, so nothing inside would be in scope at the
/// NavStack call site. Imported `const`s from a sibling module aren't
/// resolved either — that needs cross-module info — but the bail warning
/// (below) names the local id so the diagnostic is non-silent.
fn collect_module_string_consts(init: &[Stmt]) -> HashMap<LocalId, String> {
    let mut env = HashMap::new();
    for stmt in init {
        if let Stmt::Let {
            id,
            init: Some(Expr::String(s)),
            mutable: false,
            ..
        } = stmt
        {
            env.insert(*id, s.clone());
        }
    }
    env
}

/// Walk the entire module and return the set of `LocalId`s passed as the
/// first positional argument to a handle-based state API on `perry/ui`.
/// These bindings can't be safely keyed-rewritten because the runtime FFI
/// (`perry_ui_state_on_change`, `perry_ui_state_bind_textfield`, …) takes
/// the integer Widget handle, not a `"__state_N"` registry key. See the
/// call site in `run()` for the original incident (#763 / #764).
///
/// The match is intentionally conservative: only methods that are known
/// to consume a `State<T>` as their receiver-equivalent first arg. Methods
/// the rewrite pass already handles end-to-end (`NavStack`, `ForEach`,
/// `state.set` / `state.value` / `state.get` / `state.text`) are NOT in
/// this list — those have a complete keyed-side implementation.
fn is_handle_based_state_api(method: &str) -> bool {
    matches!(
        method,
        "stateOnChange"
            | "stateBindTextfield"
            | "stateBindToggle"
            | "stateBindSlider"
            | "stateBindVisibility"
            | "stateBindTextNumeric"
    )
}

fn collect_handle_based_state_uses(module: &Module) -> HashSet<LocalId> {
    let mut out = HashSet::new();
    let walk_stmts = |stmts: &[Stmt], out: &mut HashSet<LocalId>| {
        for stmt in stmts {
            scan_stmt_handle_uses(stmt, out);
        }
    };
    walk_stmts(&module.init, &mut out);
    for func in &module.functions {
        walk_stmts(&func.body, &mut out);
    }
    for class in &module.classes {
        for method in &class.methods {
            walk_stmts(&method.body, &mut out);
        }
        if let Some(ctor) = &class.constructor {
            walk_stmts(&ctor.body, &mut out);
        }
    }
    out
}

fn scan_stmt_handle_uses(stmt: &Stmt, out: &mut HashSet<LocalId>) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => scan_expr_handle_uses(e, out),
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_handle_uses(e, out),
        Stmt::Return(Some(e)) => scan_expr_handle_uses(e, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_handle_uses(condition, out);
            for s in then_branch {
                scan_stmt_handle_uses(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    scan_stmt_handle_uses(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_handle_uses(condition, out);
            for s in body {
                scan_stmt_handle_uses(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_handle_uses(i, out);
            }
            if let Some(c) = condition {
                scan_expr_handle_uses(c, out);
            }
            if let Some(u) = update {
                scan_expr_handle_uses(u, out);
            }
            for s in body {
                scan_stmt_handle_uses(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
            ..
        } => {
            for s in body {
                scan_stmt_handle_uses(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    scan_stmt_handle_uses(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    scan_stmt_handle_uses(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_handle_uses(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    scan_expr_handle_uses(t, out);
                }
                for s in &case.body {
                    scan_stmt_handle_uses(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_handle_uses(body, out),
        _ => {}
    }
}

fn scan_expr_handle_uses(e: &Expr, out: &mut HashSet<LocalId>) {
    if let Expr::NativeMethodCall {
        module,
        method,
        object: None,
        args,
        ..
    } = e
    {
        if module == "perry/ui" && is_handle_based_state_api(method) {
            if let Some(Expr::LocalGet(id)) = args.first() {
                out.insert(*id);
            }
        }
    }
    use perry_hir::walker::walk_expr_children;
    walk_expr_children(e, &mut |child| scan_expr_handle_uses(child, out));
    if let Expr::Closure { body, .. } = e {
        for s in body {
            scan_stmt_handle_uses(s, out);
        }
    }
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
                // `state<T>(initial)` and the capital `State<T>(initial)`
                // alias declared in `types/perry/ui/index.d.ts:458` both
                // come through here as `NativeMethodCall` rows. Without
                // accepting both spellings, the issue #612 repro (which
                // uses `State<string>(...)`) bypassed every state rewrite
                // — `__state_init` never fired, the NavStack(state, routes)
                // pattern stayed as a 2-arg NativeMethodCall, and the
                // codegen catch-all silently dropped the routes.
                if module == "perry/ui"
                    && (method == "state" || method == "State")
                    && args.len() == 1
                {
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

    if let Some(replacement) = try_rewrite_foreach(e, bindings, fresh) {
        *e = replacement;
        return;
    }

    if let Some(replacement) = try_rewrite_state_access(e, bindings) {
        *e = replacement;
    }
}

/// Issue #610. Detect `ForEach(LocalGet(state_id), render)` where
/// `state_id` is one of our state bindings. Lower the call to an IIFE
/// (closure-as-Call with empty args) whose body:
///   1. Allocates a host VStack via the existing 0-arg `VStack()` form.
///   2. Calls `__foreach_register(synth_id, host, render)` which records
///      the binding + paints the initial children (matching the current
///      `state(synth_id)` value) via the platform's render handler.
///   3. Returns the host.
///
/// Same IIFE shape as `try_rewrite_navstack`. When the bound state
/// changes via `state.set(n)`, the runtime's `js_state_set` walks
/// `FOREACH_REGISTRY` and re-fires the platform render handler, which
/// clears the host's children and re-invokes `render(i)` for each
/// `i in [0..n)`.
///
/// Bails (returns None) for shapes other than the canonical
/// `ForEach(stateBinding, closureExpr)` — those fall through to the
/// existing codegen which has its own handling for the non-state
/// integer-handle form.
fn try_rewrite_foreach(
    e: &Expr,
    bindings: &HashMap<LocalId, StateBinding>,
    fresh: &mut FreshIds,
) -> Option<Expr> {
    let (state_id, render_closure) = match e {
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            args,
            ..
        } if module == "perry/ui" && method == "ForEach" && args.len() == 2 => {
            let state_id = match &args[0] {
                Expr::LocalGet(id) => *id,
                _ => return None,
            };
            // Render arg can be a Closure literal or a LocalGet of a
            // closure-typed local. Anything else (a stored function ref
            // through some other shape) bails to existing codegen.
            let render = match &args[1] {
                Expr::Closure { .. } | Expr::LocalGet(_) => args[1].clone(),
                _ => return None,
            };
            (state_id, render)
        }
        _ => return None,
    };
    let binding = bindings.get(&state_id)?;
    let synth_id = binding.synth_id.clone();

    let host_id = fresh.fresh_local();
    let render_id = fresh.fresh_local();
    let mut body_stmts: Vec<Stmt> = Vec::with_capacity(4);

    // let __fe_host = VStack(0);
    body_stmts.push(Stmt::Let {
        id: host_id,
        name: format!("__fe_host_{}", host_id),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: "VStack".to_string(),
            args: vec![Expr::Number(0.0), Expr::Array(vec![])],
        }),
    });
    // let __fe_render = <render>; — hoist into a let so the
    // __foreach_register call site sees the same SSA name as we'd want
    // to capture (avoids re-evaluating the closure construction expr).
    body_stmts.push(Stmt::Let {
        id: render_id,
        name: format!("__fe_render_{}", render_id),
        ty: Type::Any,
        mutable: false,
        init: Some(render_closure),
    });
    // __foreach_register("synth_id", __fe_host, __fe_render);
    body_stmts.push(Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "__foreach_register".to_string(),
        args: vec![
            Expr::String(synth_id),
            Expr::LocalGet(host_id),
            Expr::LocalGet(render_id),
        ],
    }));
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
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_strict: false,
        is_async: false,
        is_generator: false,
    };
    Some(Expr::Call {
        callee: Box::new(closure),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    })
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
/// Routes with non-string `name` literals (after const-fold) or shapes other
/// than the canonical `{ name: string, body: Widget }` object literal bail
/// and the original NavStack call falls through to its 0-arg dispatch (no
/// routing behavior — a plain `UIView` with no routes registered). #1135
/// promoted that fallthrough from silent to a `Warning: NavStack(...)` line
/// so an empty container isn't a zero-diagnostic failure mode anymore. Refs
/// #535 / #1135.
fn try_rewrite_navstack(
    e: &Expr,
    bindings: &HashMap<LocalId, StateBinding>,
    fresh: &mut FreshIds,
) -> Option<Expr> {
    // Detect the 2-arg shape first. Only when the call IS the documented
    // 2-arg NavStack and *we* fail to rewrite it do we emit a warning —
    // anything else is some other expression, not our concern.
    let (state_arg, route_array_arg) = match e {
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            args,
            ..
        } if module == "perry/ui" && method == "NavStack" && args.len() == 2 => {
            (&args[0], &args[1])
        }
        _ => return None,
    };

    let state_id = match state_arg {
        Expr::LocalGet(id) => *id,
        other => {
            warn_navstack_bail(&format!(
                "first arg must be a `state<string>(...)` local binding, got {}",
                expr_kind_for_diag(other)
            ));
            return None;
        }
    };
    let route_array = match route_array_arg {
        Expr::Array(items) => items,
        other => {
            warn_navstack_bail(&format!(
                "second arg must be a literal array of `{{name, body}}` route entries, got {}",
                expr_kind_for_diag(other)
            ));
            return None;
        }
    };
    let binding = match bindings.get(&state_id) {
        Some(b) => b,
        None => {
            warn_navstack_bail(&format!(
                "first arg (local #{state_id}) is not a `state<T>()` binding declared in this module — \
                 NavStack only knows how to drive state created via `state<string>(initial)` here"
            ));
            return None;
        }
    };
    let synth_id = binding.synth_id.clone();

    // Extract (name, body) pairs from each route. Route entries are HIR
    // anonymous-shape `New { class_name: __AnonShape_<hash>, args: [name,
    // body] }` — that's what `lower_object_literal` emits for `{name: ...,
    // body: ...}` (see lower_decl.rs's anon-shape harvest). We don't try
    // to handle other shapes (spread, dynamic property keys) — they bail.
    let mut routes: Vec<(String, Expr)> = Vec::with_capacity(route_array.len());
    for (idx, route) in route_array.iter().enumerate() {
        let shape_args = match route {
            Expr::New { args, .. } => args,
            other => {
                warn_navstack_bail(&format!(
                    "route #{idx} must be a `{{name, body}}` object literal, got {}",
                    expr_kind_for_diag(other)
                ));
                return None;
            }
        };
        if shape_args.len() != 2 {
            warn_navstack_bail(&format!(
                "route #{idx} must be a 2-field `{{name, body}}` object literal (no extra fields, no spread), got {} fields",
                shape_args.len()
            ));
            return None;
        }
        let name = match resolve_route_name(&shape_args[0], &fresh.string_consts) {
            Some(s) => s,
            None => {
                warn_navstack_bail(&format!(
                    "route #{idx} `name` must be a string literal or a same-module `const X = \"…\"` (covered by #1135 const-fold), \
                     got {}. Imported route-name constants currently need to be inlined to a string literal at the NavStack call site.",
                    expr_kind_for_diag(&shape_args[0])
                ));
                return None;
            }
        };
        let body = shape_args[1].clone();
        routes.push((name, body));
    }
    if routes.is_empty() {
        warn_navstack_bail(
            "routes array is empty — NavStack(state, []) has no branches to register",
        );
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
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_strict: false,
        is_async: false,
        is_generator: false,
    };
    Some(Expr::Call {
        callee: Box::new(closure),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    })
}

/// #1135 — resolve a route `name` slot to a literal string, accepting
/// either a direct `Expr::String` or a `LocalGet` of a same-module
/// `const X = "literal"` binding (the canonical "factor route names into
/// a shared constants file" pattern). Returns `None` for anything else —
/// the caller emits a warning that names the unresolved kind.
fn resolve_route_name(
    name_expr: &Expr,
    string_consts: &HashMap<LocalId, String>,
) -> Option<String> {
    match name_expr {
        Expr::String(s) => Some(s.clone()),
        Expr::LocalGet(id) => string_consts.get(id).cloned(),
        _ => None,
    }
}

/// #1135 — emit a user-visible warning when a `NavStack(state, routes)`
/// call site matches the documented 2-arg shape but fails one of the
/// rewrite invariants. The pre-fix behavior was to silently fall through
/// to the 0-arg `NavStack()` stub (a blank `UIView` with no routes), which
/// surfaced as a completely empty screen with zero compile diagnostics.
fn warn_navstack_bail(reason: &str) {
    eprintln!(
        "  Warning: NavStack(state, routes) skipped state-driven lowering ({reason}). \
         The call will fall through to the 0-arg `NavStack()` stub (empty container, no routing). \
         See https://github.com/PerryTS/perry/issues/1135"
    );
}

/// #1135 — short label for the unexpected expression kind that caused the
/// rewrite to bail, so the warning is actually useful at a glance.
fn expr_kind_for_diag(e: &Expr) -> &'static str {
    match e {
        Expr::String(_) => "a string literal (this should have matched — file an issue)",
        Expr::Number(_) | Expr::Integer(_) => "a number literal",
        Expr::Bool(_) => "a boolean literal",
        Expr::Null => "null",
        Expr::Undefined => "undefined",
        Expr::LocalGet(_) => "a non-string local (not a `const X = \"...\"` in this module)",
        Expr::GlobalGet(_) => {
            "a module-global read (imported binding) — inline as a string literal"
        }
        Expr::Call { .. } | Expr::CallSpread { .. } => "a function call",
        Expr::Closure { .. } => "a closure",
        Expr::PropertyGet { .. } => "a property access (e.g. `routes.HOME`)",
        Expr::Array(_) | Expr::ArraySpread(_) => "an array literal",
        Expr::Object(_) | Expr::ObjectSpread { .. } => "an object literal",
        Expr::New { .. } | Expr::NewDynamic { .. } => "a `new` / object-literal expression",
        Expr::NativeMethodCall { .. } => "a native method call",
        Expr::Binary { .. } | Expr::Compare { .. } | Expr::Logical { .. } => {
            "a binary expression (string concat / comparison)"
        }
        Expr::Unary { .. } => "a unary expression",
        Expr::Conditional { .. } => "a ternary",
        _ => "an unsupported expression kind",
    }
}

/// Attempt to rewrite `e` if it matches a state access on a known
/// state-bound local. Returns `Some(new_expr)` for a match, `None`
/// otherwise. Does not recurse into children — the caller does that
/// after this returns `None`.
fn try_rewrite_state_access(e: &Expr, bindings: &HashMap<LocalId, StateBinding>) -> Option<Expr> {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::PropertyGet {
            object, property, ..
        } = callee.as_ref()
        {
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
    if let Expr::PropertyGet {
        object, property, ..
    } = e
    {
        if property == "value" {
            if let Expr::LocalGet(state_id) = object.as_ref() {
                if let Some(binding) = bindings.get(state_id) {
                    return Some(state_get_call(&binding.synth_id));
                }
            }
        }
    }
    // HIR's "perry/ui state class" lowering routes `state.set(v)` /
    // `state.value` / `state.get()` / `state.text()` through
    // `NativeMethodCall { module: "perry/ui", class_name: Some("State"),
    // object: Some(LocalGet(state_id)), method: ..., args }` instead of
    // the PropertyGet shape. Recognize that form here so the rewrite
    // actually fires (otherwise `state.value` stays as a no-op method
    // call on `undefined` — see textarea / state docs failures).
    if let Expr::NativeMethodCall {
        module,
        class_name,
        object: Some(obj),
        method,
        args,
        ..
    } = e
    {
        // `.set` carries `class_name: Some("State")`; `.value` / `.get`
        // come through with `class_name: None`. Accept either as long
        // as the receiver resolves to one of our bindings.
        let class_ok = class_name.is_none() || class_name.as_deref() == Some("State");
        if module == "perry/ui" && class_ok {
            if let Expr::LocalGet(state_id) = obj.as_ref() {
                if let Some(binding) = bindings.get(state_id) {
                    return match method.as_str() {
                        "get" | "value" if args.is_empty() => {
                            Some(state_get_call(&binding.synth_id))
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state_let(id: LocalId, method: &str, initial: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("cell_{}", id),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::NativeMethodCall {
                module: "perry/ui".to_string(),
                class_name: None,
                object: None,
                method: method.to_string(),
                args: vec![initial],
            }),
        }
    }

    fn handle_call(method: &str, state_id: LocalId) -> Stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: None,
            object: None,
            method: method.to_string(),
            args: vec![
                Expr::LocalGet(state_id),
                Expr::Closure {
                    func_id: 100,
                    params: vec![],
                    return_type: Type::Any,
                    body: vec![],
                    captures: vec![],
                    mutable_captures: vec![],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_strict: false,
                    is_async: false,
                    is_generator: false,
                },
            ],
        })
    }

    fn set_method_call(state_id: LocalId, value: &str) -> Stmt {
        Stmt::Expr(Expr::NativeMethodCall {
            module: "perry/ui".to_string(),
            class_name: Some("State".to_string()),
            object: Some(Box::new(Expr::LocalGet(state_id))),
            method: "set".to_string(),
            args: vec![Expr::String(value.to_string())],
        })
    }

    /// Returns true if the first init statement still constructs a State
    /// (i.e., the desugar didn't replace it with `Let { init: Undefined }`
    /// + `__state_init`). #764 regression check.
    fn first_let_init_is_state_call(module: &Module) -> bool {
        matches!(
            module.init.first(),
            Some(Stmt::Let {
                init: Some(Expr::NativeMethodCall { method, .. }),
                ..
            }) if method == "State" || method == "state"
        )
    }

    /// True if `module.init` contains a `__state_init` synthetic call —
    /// i.e., the rewrite was applied.
    fn has_state_init_call(module: &Module) -> bool {
        module.init.iter().any(|s| {
            matches!(
                s,
                Stmt::Expr(Expr::NativeMethodCall { method, .. }) if method == "__state_init"
            )
        })
    }

    #[test]
    fn module_init_uppercase_state_with_state_on_change_is_skipped() {
        // #764: `const cell = State("v"); stateOnChange(cell, …); cell.set(...)`
        // at module-init level used to be hijacked into `__state_init` +
        // `__state_set`, leaving `stateOnChange` registered against
        // `undefined`. The fix preserves the original handle-based call.
        let mut module = Module::new("test");
        module
            .init
            .push(state_let(0, "State", Expr::String("v".to_string())));
        module.init.push(handle_call("stateOnChange", 0));
        module.init.push(set_method_call(0, "next"));

        run(&mut module);

        assert!(
            first_let_init_is_state_call(&module),
            "binding should remain `State(...)` when stateOnChange is used at module-init",
        );
        assert!(
            !has_state_init_call(&module),
            "__state_init must not be emitted when the binding is handle-consumed",
        );
    }

    #[test]
    fn module_init_lowercase_state_with_state_on_change_is_also_skipped() {
        // Same gate applies to lowercase `state(...)` — if the user mixes
        // it with the handle-based API, we must not rewrite.
        let mut module = Module::new("test");
        module.init.push(state_let(0, "state", Expr::Number(0.0)));
        module.init.push(handle_call("stateBindTextfield", 0));

        run(&mut module);

        assert!(first_let_init_is_state_call(&module));
        assert!(!has_state_init_call(&module));
    }

    #[test]
    fn module_init_state_without_handle_api_is_still_rewritten() {
        // Sanity: when no handle-based API touches the binding (e.g. the
        // user goes purely through `cell.set` / `cell.value`), the original
        // keyed rewrite must still fire. Otherwise NavStack / ForEach /
        // string-state.text() flows regress.
        let mut module = Module::new("test");
        module
            .init
            .push(state_let(0, "state", Expr::String("v".to_string())));
        module.init.push(set_method_call(0, "next"));

        run(&mut module);

        assert!(
            has_state_init_call(&module),
            "rewrite must still fire for pure keyed-API usage",
        );
        // Original Let should now be `init: Some(Undefined)`.
        let first_init = match &module.init[0] {
            Stmt::Let { init: Some(e), .. } => e.clone(),
            other => panic!("expected Let stmt, got {:?}", other),
        };
        assert!(matches!(first_init, Expr::Undefined));
    }

    #[test]
    fn handle_use_inside_function_body_also_gates_module_init_binding() {
        // The user's #763 shape places `State()` at module-init but uses
        // `stateOnChange` inside `function main()`. The gate must scan all
        // function bodies, not just module.init, to catch this.
        use perry_hir::Function;
        let mut module = Module::new("test");
        module
            .init
            .push(state_let(0, "State", Expr::String("v".to_string())));
        module.functions.push(Function {
            id: 0,
            name: "main".to_string(),
            params: vec![],
            type_params: vec![],
            return_type: Type::Any,
            body: vec![handle_call("stateOnChange", 0)],
            is_strict: false,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        });

        run(&mut module);

        assert!(first_let_init_is_state_call(&module));
        assert!(!has_state_init_call(&module));
    }
}
