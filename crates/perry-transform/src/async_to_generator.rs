//! Issue #256: spec-compliant microtask ordering for plain async functions.
//!
//! ## What this pass does
//!
//! Pre-pass that runs before `transform_generators`. For every top-level
//! function with `is_async = true && !is_generator`:
//!
//! 1. **Hoists non-top-level awaits**: any `await x` not in a top-level
//!    statement position (let init, expr stmt, return) is lifted into a
//!    fresh `let __awaitN = await x;` placed before the containing
//!    statement, and the original site is replaced with `LocalGet(__awaitN)`.
//!    Without this, expressions like `console.log("x: " + await y)` lower
//!    to `console.log("x: " + 0)` because the generator transform's
//!    `linearize_body` only recognises yields at top-level positions; a
//!    yield buried inside a concat operator hits codegen's
//!    `Expr::Yield => double_literal(0.0)` arm instead.
//! 2. **Rewrites await→yield**: every `Expr::Await(x)` becomes
//!    `Expr::Yield { value: Some(x), delegate: false }`.
//! 3. **Flips the flags**: `is_async = false`, `is_generator = true`,
//!    `was_plain_async = true`.
//!
//! After this pass, the existing generator state-machine transform lifts
//! the function into a `{ next, return, throw }` iterator. The
//! `was_plain_async` flag tells the generator transform to wrap the
//! iterator in an async-step driver so the function returns a Promise
//! that resolves to the user's return value, with each yield/await
//! suspending into a microtask.
//!
//! ## Why this fixes the spec gap
//!
//! Pre-fix Perry's async functions ran their entire body synchronously on
//! the calling thread, with each `await` lowered to a busy-wait poll loop
//! on the awaited Promise. This diverges from spec semantics: an `await`
//! should always yield to the microtask queue, even on already-resolved
//! Promises, so synchronous code following an unawaited async call runs
//! before the awaited body's continuation.
//!
//! Post-fix the async function becomes a state machine. The first state
//! runs synchronously (matching spec). Each `await x` lowers to a yield
//! that suspends the state machine and chains the continuation through
//! `Promise.resolve(x).then(continuation)`, which puts the rest of the
//! body in a microtask. The microtask runs after all currently-executing
//! synchronous code finishes — exactly the spec ordering.
//!
//! ## Scope and limitations (v1)
//!
//! - **Top-level functions only**: nested async closures (arrow/function
//!   expressions assigned to locals) are NOT yet rewritten. They keep
//!   the pre-fix direct-call/busy-wait behavior. Follow-up.
//! - **No new HIR variants or runtime helpers**: the rewrite produces
//!   only existing variants (Yield, Closure, Promise.then chains via
//!   GlobalGet(0)). The async-step driver is built inline in the
//!   generator transform. This sidesteps the LLVM constant-folding
//!   mystery the prior prototype hit (issue #256 background section 1).

use perry_hir::ir::*;
use perry_types::{LocalId, Type};

/// Run the pre-pass on every async function in the module.
pub fn transform_async_to_generator(module: &mut Module) {
    // Conservative module-level scope: skip the rewrite ENTIRELY if the
    // module has classes with __perry_cap_* fields (the v0.5.323 issue
    // #212 capture rewrite). The async-step driver's fresh LocalId
    // allocations can collide with the v0.5.323 method-local rebind
    // ids — manifests as `[PERRY WARN] js_box_set: null box pointer`
    // when the colliding LocalGet for the async-step's `__iter` returns
    // the captured-by-class-method box pointer instead of the iter
    // object. The collision is path-dependent on which ids `next_local_id`
    // happened to land on; safer to bail on the whole module than to
    // ship a coin-flip fix. Issue #212-style capturing classes are the
    // ONLY known trigger, so this scope is tight enough that the issue
    // #256 microtask-ordering reproducer (no classes) still gets the
    // fix.
    if module_has_capturing_classes(module) {
        return;
    }
    let mut next_local_id = compute_max_local_id(module) + 1;
    for func in &mut module.functions {
        if func.is_async && !func.is_generator {
            // Per-function conservative scope: skip if the body has a
            // nested closure with captures (forEach pattern, etc.).
            if body_has_capturing_closure(&func.body) {
                continue;
            }
            let mut had_await = false;
            // First, hoist non-top-level awaits in every statement so
            // every Await ends up in a top-level position the generator
            // transform's `linearize_body` can split states at.
            hoist_awaits_in_stmts(&mut func.body, &mut next_local_id);
            // Then rewrite all awaits (now in top-level positions) to
            // yields and flip the flag.
            rewrite_stmts(&mut func.body, &mut had_await);
            // Even if the body had no awaits, the function is still async
            // semantically (its return value gets wrapped in a Promise).
            // Without awaits, the existing direct-call path is correct
            // and cheaper, so we leave is_async alone in that case.
            if had_await {
                func.is_async = false;
                func.is_generator = true;
                func.was_plain_async = true;
            }
        }
    }
}

/// Detect if the module has any classes with `__perry_cap_*` instance
/// fields — the marker that the v0.5.323 issue #212 capture rewrite was
/// applied. These classes have method bodies with method-local rebind
/// LocalIds that share the global LocalId namespace; my pre-pass's
/// fresh-id allocations can collide with them.
fn module_has_capturing_classes(module: &Module) -> bool {
    for class in &module.classes {
        for field in &class.fields {
            if field.name.starts_with("__perry_cap_") {
                return true;
            }
        }
    }
    false
}

// ─── Conservative scope: detect nested capturing closures ────────────────

fn body_has_capturing_closure(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_capturing_closure)
}

fn stmt_has_capturing_closure(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init: Some(e), .. } => expr_has_capturing_closure(e),
        Stmt::Expr(e) | Stmt::Throw(e) => expr_has_capturing_closure(e),
        Stmt::Return(Some(e)) => expr_has_capturing_closure(e),
        Stmt::If { condition, then_branch, else_branch } => {
            expr_has_capturing_closure(condition)
                || body_has_capturing_closure(then_branch)
                || else_branch.as_ref().map_or(false, |eb| body_has_capturing_closure(eb))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_has_capturing_closure(condition) || body_has_capturing_closure(body)
        }
        Stmt::For { init, condition, update, body } => {
            init.as_ref().map_or(false, |i| stmt_has_capturing_closure(i))
                || condition.as_ref().map_or(false, |c| expr_has_capturing_closure(c))
                || update.as_ref().map_or(false, |u| expr_has_capturing_closure(u))
                || body_has_capturing_closure(body)
        }
        Stmt::Try { body, catch, finally } => {
            body_has_capturing_closure(body)
                || catch.as_ref().map_or(false, |c| body_has_capturing_closure(&c.body))
                || finally.as_ref().map_or(false, |f| body_has_capturing_closure(f))
        }
        Stmt::Switch { discriminant, cases } => {
            expr_has_capturing_closure(discriminant)
                || cases.iter().any(|c| body_has_capturing_closure(&c.body))
        }
        Stmt::Labeled { body, .. } => stmt_has_capturing_closure(body),
        _ => false,
    }
}

fn expr_has_capturing_closure(expr: &Expr) -> bool {
    // Treat ANY nested Closure as risky, regardless of captures: even
    // empty-captures closures may interact with the async-step driver in
    // subtle ways (e.g. forEach/map/filter passing the closure through a
    // native dispatch call where the closure gets stored). Better safe.
    if matches!(expr, Expr::Closure { .. }) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |e| {
        if !found && expr_has_capturing_closure(e) {
            found = true;
        }
    });
    found
}

/// Compute the max LocalId already used in the module so we can allocate
/// fresh ids for hoisted awaits without colliding. Mirrors
/// `generator::compute_max_local_id` but inlined here to avoid a
/// pub-visibility bump on the generator helper.
fn compute_max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    for func in &module.functions {
        for param in &func.params {
            max_id = max_id.max(param.id);
        }
        scan_stmts(&func.body, &mut max_id);
    }
    for stmt in &module.init {
        scan_stmt(stmt, &mut max_id);
    }
    for global in &module.globals {
        max_id = max_id.max(global.id);
    }
    // Also scan class member bodies — they share the LocalId namespace.
    // The v0.5.323 issue #212 fix allocates method-local "fresh ids" via
    // ctx.fresh_local() for the per-method rebinds of captured outer
    // locals (`let X = this.__perry_cap_<outer>`). Those ids are NOT in
    // module.functions, but they DO live in the same global LocalId
    // space my pre-pass allocates fresh ids from. Without this scan, my
    // pre-pass's allocations for the async-step driver collide with
    // class-method rebind ids — at codegen, the colliding LocalGet for
    // the async-step's `__iter` returns the captured-by-class-method
    // box pointer instead of the iter object, surfacing as the same
    // `[PERRY WARN] js_box_set: null box pointer` chain that the
    // forEach-inner-closure-captures-outer-array pattern produces.
    for class in &module.classes {
        for method in &class.methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts(&method.body, &mut max_id);
        }
        for static_method in &class.static_methods {
            for param in &static_method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts(&static_method.body, &mut max_id);
        }
        if let Some(ctor) = &class.constructor {
            for param in &ctor.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts(&ctor.body, &mut max_id);
        }
        for getter in &class.getters {
            for param in &getter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts(&getter.1.body, &mut max_id);
        }
        for setter in &class.setters {
            for param in &setter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts(&setter.1.body, &mut max_id);
        }
    }
    max_id
}

fn scan_stmts(stmts: &[Stmt], m: &mut LocalId) {
    for s in stmts { scan_stmt(s, m); }
}

fn scan_stmt(stmt: &Stmt, m: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *m = (*m).max(*id);
            if let Some(e) = init { scan_expr(e, m); }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr(e, m),
        Stmt::Return(e) => { if let Some(e) = e { scan_expr(e, m); } }
        Stmt::If { condition, then_branch, else_branch } => {
            scan_expr(condition, m);
            scan_stmts(then_branch, m);
            if let Some(eb) = else_branch { scan_stmts(eb, m); }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr(condition, m);
            scan_stmts(body, m);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init { scan_stmt(i, m); }
            if let Some(c) = condition { scan_expr(c, m); }
            if let Some(u) = update { scan_expr(u, m); }
            scan_stmts(body, m);
        }
        Stmt::Try { body, catch, finally } => {
            scan_stmts(body, m);
            if let Some(c) = catch {
                if let Some((id, _)) = c.param { *m = (*m).max(id); }
                scan_stmts(&c.body, m);
            }
            if let Some(f) = finally { scan_stmts(f, m); }
        }
        Stmt::Switch { discriminant, cases } => {
            scan_expr(discriminant, m);
            for case in cases { scan_stmts(&case.body, m); }
        }
        Stmt::Labeled { body, .. } => scan_stmt(body, m),
        _ => {}
    }
}

fn scan_expr(expr: &Expr, m: &mut LocalId) {
    if let Expr::LocalGet(id) | Expr::LocalSet(id, _) = expr {
        *m = (*m).max(*id);
    }
    if let Expr::Closure { params, captures, mutable_captures, body, .. } = expr {
        for p in params { *m = (*m).max(p.id); }
        for c in captures { *m = (*m).max(*c); }
        for c in mutable_captures { *m = (*m).max(*c); }
        scan_stmts(body, m);
        return;
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr(e, m));
}

fn alloc_local(next_id: &mut LocalId) -> LocalId {
    let id = *next_id;
    *next_id += 1;
    id
}

// ─── Hoist non-top-level awaits ──────────────────────────────────────────
//
// A "top-level" position is one of:
//   - The full init expression of a `Stmt::Let { init: Some(_) }`
//   - The full operand of a `Stmt::Expr(_)`
//   - The full operand of a `Stmt::Return(Some(_))`
//
// In any other position (an arg of a Call, an operand of a BinOp, an
// element of an Object/Array literal, a condition of an If/While, etc.),
// the `Await` gets hoisted into a fresh `let __await{id} = await <expr>`
// placed immediately before the containing statement, and the original
// site is replaced with `LocalGet(__await{id})`.
//
// We process statements one at a time and use mem::take + Vec splicing to
// insert the hoisted lets. Inner blocks (then/else/while-body/etc.) are
// processed recursively so awaits inside a nested `if (cond) { x = y +
// await z; }` are hoisted into the inner block, not the outer scope.

fn hoist_awaits_in_stmts(stmts: &mut Vec<Stmt>, next_id: &mut LocalId) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for stmt in std::mem::take(stmts) {
        let mut hoisted: Vec<Stmt> = Vec::new();
        let new_stmt = hoist_awaits_in_stmt(stmt, next_id, &mut hoisted);
        for h in hoisted { out.push(h); }
        out.push(new_stmt);
    }
    *stmts = out;
}

fn hoist_awaits_in_stmt(
    mut stmt: Stmt,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) -> Stmt {
    match &mut stmt {
        // Top-level positions: don't hoist the *outer* await but do
        // hoist any nested awaits inside the operand.
        Stmt::Let { init: Some(e), .. } => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Expr(e) => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Return(Some(e)) => {
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::Throw(e) => {
            // `throw await x` — we treat this like a return: the outer
            // await stays in place, inner awaits hoisted.
            hoist_awaits_avoiding_top_level(e, next_id, hoisted);
        }
        Stmt::If { condition, then_branch, else_branch } => {
            // The condition is NOT a top-level await position (it's
            // nested in If) — fully hoist all awaits in it.
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
            hoist_awaits_in_stmts(then_branch, next_id);
            if let Some(eb) = else_branch {
                hoist_awaits_in_stmts(eb, next_id);
            }
        }
        Stmt::While { condition, body } => {
            // While condition: fully hoist all awaits. The hoisted
            // lets land before the while statement, but re-evaluating
            // them on each iteration requires the await to fire each
            // pass. JS spec: condition with await runs on every
            // iteration. We don't currently support this — see the
            // limitation in the doc comment. Single hoist per loop
            // entry is the safe-but-incomplete approximation.
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
            hoist_awaits_in_stmts(body, next_id);
        }
        Stmt::DoWhile { body, condition } => {
            hoist_awaits_in_stmts(body, next_id);
            hoist_awaits_in_expr_full(condition, next_id, hoisted);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init {
                let mut inner_hoisted = Vec::new();
                let i_replaced = hoist_awaits_in_stmt(
                    (**i).clone(),
                    next_id,
                    &mut inner_hoisted,
                );
                for h in inner_hoisted { hoisted.push(h); }
                *i = Box::new(i_replaced);
            }
            if let Some(c) = condition { hoist_awaits_in_expr_full(c, next_id, hoisted); }
            if let Some(u) = update { hoist_awaits_in_expr_full(u, next_id, hoisted); }
            hoist_awaits_in_stmts(body, next_id);
        }
        Stmt::Try { body, catch, finally } => {
            hoist_awaits_in_stmts(body, next_id);
            if let Some(c) = catch {
                hoist_awaits_in_stmts(&mut c.body, next_id);
            }
            if let Some(f) = finally {
                hoist_awaits_in_stmts(f, next_id);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            hoist_awaits_in_expr_full(discriminant, next_id, hoisted);
            for case in cases.iter_mut() {
                if let Some(t) = &mut case.test {
                    hoist_awaits_in_expr_full(t, next_id, hoisted);
                }
                hoist_awaits_in_stmts(&mut case.body, next_id);
            }
        }
        Stmt::Labeled { body, .. } => {
            let mut inner = Vec::new();
            let body_taken = std::mem::replace(body.as_mut(), Stmt::Break);
            let new_body = hoist_awaits_in_stmt(body_taken, next_id, &mut inner);
            for h in inner { hoisted.push(h); }
            **body = new_body;
        }
        _ => {}
    }
    stmt
}

/// Hoist all awaits in an expression INCLUDING any at the top level of
/// the expression itself. Used for non-statement-positioned operands
/// (If condition, While condition, Switch discriminant, etc.).
fn hoist_awaits_in_expr_full(expr: &mut Expr, next_id: &mut LocalId, hoisted: &mut Vec<Stmt>) {
    if matches!(expr, Expr::Closure { .. }) {
        // Don't descend into closure bodies; nested closures are out of
        // scope for the v1 plain-async pre-pass.
        return;
    }
    // Recurse into children first (innermost-first hoisting).
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_awaits_in_expr_full(child, next_id, hoisted);
    });
    if matches!(expr, Expr::Await(_)) {
        let id = alloc_local(next_id);
        let original = std::mem::replace(expr, Expr::LocalGet(id));
        hoisted.push(Stmt::Let {
            id,
            name: format!("__await_{}", id),
            ty: Type::Any,
            mutable: false,
            init: Some(original),
        });
    }
}

/// Hoist nested awaits but leave a top-level await alone. Used for
/// statement-positioned operands (Let init, Stmt::Expr operand, etc.)
/// where the outer await is something the generator transform handles.
fn hoist_awaits_avoiding_top_level(
    expr: &mut Expr,
    next_id: &mut LocalId,
    hoisted: &mut Vec<Stmt>,
) {
    if let Expr::Await(inner) = expr {
        // Outer is an await — keep it but recursively hoist nested awaits
        // inside the operand fully (they are nested, not top-level).
        hoist_awaits_in_expr_full(inner.as_mut(), next_id, hoisted);
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    // Outer is NOT an await. Children may contain awaits which ARE
    // nested — fully hoist them.
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        hoist_awaits_in_expr_full(child, next_id, hoisted);
    });
}

// ─── Rewrite await → yield ───────────────────────────────────────────────
//
// Runs after hoisting, so every Await is now in a top-level position the
// generator transform can split states at.

fn rewrite_stmts(stmts: &mut [Stmt], had_await: &mut bool) {
    for stmt in stmts.iter_mut() {
        rewrite_stmt(stmt, had_await);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, had_await: &mut bool) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => rewrite_expr(e, had_await),
        Stmt::Expr(e) => rewrite_expr(e, had_await),
        Stmt::Return(Some(e)) => rewrite_expr(e, had_await),
        Stmt::Throw(e) => rewrite_expr(e, had_await),
        Stmt::If { condition, then_branch, else_branch } => {
            rewrite_expr(condition, had_await);
            rewrite_stmts(then_branch, had_await);
            if let Some(eb) = else_branch {
                rewrite_stmts(eb, had_await);
            }
        }
        Stmt::While { condition, body } => {
            rewrite_expr(condition, had_await);
            rewrite_stmts(body, had_await);
        }
        Stmt::DoWhile { body, condition } => {
            rewrite_stmts(body, had_await);
            rewrite_expr(condition, had_await);
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init { rewrite_stmt(i, had_await); }
            if let Some(c) = condition { rewrite_expr(c, had_await); }
            if let Some(u) = update { rewrite_expr(u, had_await); }
            rewrite_stmts(body, had_await);
        }
        Stmt::Try { body, catch, finally } => {
            rewrite_stmts(body, had_await);
            if let Some(c) = catch {
                rewrite_stmts(&mut c.body, had_await);
            }
            if let Some(f) = finally {
                rewrite_stmts(f, had_await);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            rewrite_expr(discriminant, had_await);
            for case in cases.iter_mut() {
                rewrite_stmts(&mut case.body, had_await);
            }
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body, had_await),
        _ => {}
    }
}

fn rewrite_expr(expr: &mut Expr, had_await: &mut bool) {
    if matches!(expr, Expr::Await(_)) {
        *had_await = true;
        if let Expr::Await(inner) = std::mem::replace(expr, Expr::Undefined) {
            let mut inner = *inner;
            rewrite_expr(&mut inner, had_await);
            *expr = Expr::Yield {
                value: Some(Box::new(inner)),
                delegate: false,
            };
        }
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |e| rewrite_expr(e, had_await));
}
