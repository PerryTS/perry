//! Interprocedural deforestation: fuse "callee allocates array, fills,
//! returns; caller iterates and copies into outer array" into a single
//! pass where the callee writes directly into an accumulator passed by
//! the caller.
//!
//! ## The pattern
//!
//! Producer (callee):
//! ```ignore
//! function f(p1, p2, ...) {
//!     const out = [];
//!     // ... pushes into `out` ...
//!     // ... possibly recursive call `f(...)` whose result is consumed
//!     //     into `out` ...
//!     return out;
//! }
//! ```
//!
//! Consumer (caller):
//! ```ignore
//! const child = f(args);
//! for (let j = 0; j < child.length; j++) outer.push(child[j]);
//! // child has no other use after the loop
//! ```
//!
//! ## The transformation
//!
//! 1. Add a trailing `__deforest_out: Array<T>` parameter to f.
//! 2. Replace `const out = []` with no-op (the parameter IS the
//!    accumulator).
//! 3. Replace every `out.push(...)` / `out.something` with the param.
//! 4. Replace `return out` with `return undefined`.
//! 5. **Recursive calls inside f**: pass the param through directly.
//! 6. **Consumer call sites**: rewrite the consume-loop pattern to
//!    `f(args, outer)` — no temporary array, no copy loop.
//! 7. **Non-consumer call sites** (e.g. top-level `const all = f(...)`):
//!    rewrite to `const all = []; f(args, all);` so callers that need
//!    the array as a value still get it.
//!
//! ## Why this matters
//!
//! On ABC451D-shaped recursive workloads, every recursive call
//! allocates a fresh array, fills it, returns it, the caller iterates
//! and copies elements into ITS array. Each level multiplies the
//! allocation pressure. After deforestation, ONE array is shared
//! across the entire recursion — the inner recursion writes directly
//! into the top-level accumulator. Manually-rewritten ABC451D drops
//! from ~3.2 s to ~0.74 s on Apple M-series (4.3× faster, within 1.8×
//! of Bun).
//!
//! ## Scope
//!
//! Intra-module only. Cross-module deforestation requires propagating
//! the rewritten signature through every importer and is filed as a
//! separate follow-up.
//!
//! Limitations (transformation bails when these are observed):
//! - Producer's `out` is referenced anywhere besides `push` / member
//!   reads of `length`, `map` produce-style or read-only methods, or
//!   the final `return out`.
//! - Producer has multiple return paths some of which don't return
//!   `out`.
//! - Producer's body assigns `out` (`out = something_else`).
//! - Producer is async or a generator (state-machine flattening would
//!   complicate the rewrite; not blocked by spec, just out of MVP).
//! - A call site has the producer's result `await`-ed (Promise wrap).
//! - Consumer's `outer` is modified between `const child = f()` and
//!   the consume loop in a way that aliases `child`.

use perry_hir::{Expr, Function, Module, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::{HashMap, HashSet};

/// Public entry point. Mutates `module` in place: rewrites every
/// detected producer function to take an accumulator parameter, and
/// rewrites every detected call site to pass an accumulator and elide
/// the consume loop. Functions that don't match the producer shape
/// are left unchanged; modules with no matching functions are no-ops.
pub fn run(module: &mut Module) {
    let producers = detect_producers(module);
    if producers.is_empty() {
        return;
    }

    if std::env::var("PERRY_DEFOREST_DEBUG").is_ok() {
        for (id, p) in &producers {
            eprintln!(
                "[deforest] producer fn_id={} name={} out_local={} param_count={}",
                id,
                module
                    .functions
                    .iter()
                    .find(|f| f.id == *id)
                    .map(|f| f.name.as_str())
                    .unwrap_or("?"),
                p.out_local_id,
                p.original_param_count
            );
        }
    }

    // Allocate fresh LocalIds for the synthetic out-parameter on each
    // producer. The id space is module-wide, so we walk the module to
    // find max + 1 once and bump from there.
    let mut next_local = max_local_id(module) + 1;
    let mut out_param_ids: HashMap<FuncId, LocalId> = HashMap::new();
    for &id in producers.keys() {
        out_param_ids.insert(id, next_local);
        next_local += 1;
    }

    // Phase 2: rewrite producer bodies — add the param, swap `out`
    // references for the param, drop the return.
    for func in &mut module.functions {
        if let Some(info) = producers.get(&func.id) {
            let out_param = out_param_ids[&func.id];
            rewrite_producer_body(func, info, out_param, &producers, &out_param_ids);
        }
    }

    // Phase 3: rewrite call sites in module-init and every function
    // body. The producer's own body already had its recursive call
    // sites rewritten by phase 2 — phase 3 covers callers that are
    // NOT the producer itself (top-level scripts, sibling helpers,
    // etc.).
    rewrite_call_sites_in_stmts(
        &mut module.init,
        &producers,
        &out_param_ids,
        &mut next_local,
    );
    for func in &mut module.functions {
        // Skip the producers themselves — their bodies were already
        // rewritten in phase 2 (which knows the param substitution).
        if producers.contains_key(&func.id) {
            continue;
        }
        rewrite_call_sites_in_stmts(&mut func.body, &producers, &out_param_ids, &mut next_local);
    }
}

/// Per-producer information collected during detection.
#[derive(Debug, Clone)]
struct ProducerInfo {
    /// LocalId of the `let out = []` binding inside the producer body.
    out_local_id: LocalId,
    /// Number of original parameters (before we add the out-param).
    /// Recursive call rewrites need to know this to position the new
    /// arg correctly.
    original_param_count: usize,
    /// Element type of the accumulator. Inferred from the producer's
    /// return type if known, else `Any`. Used for the new param's
    /// declared type.
    elem_ty: Type,
}

/// Walk every function in the module and return a map `FuncId →
/// ProducerInfo` for those matching the deforestable-producer shape.
///
/// After body-shape analysis identifies candidates, a second pass
/// verifies that the candidate isn't "taken by reference" anywhere in
/// the module — i.e., every `Expr::FuncRef(id)` reference must be the
/// direct `callee` of a `Call`/`CallSpread`. If the function is
/// stored to a local, passed as an argument, or otherwise used as a
/// value, the rewrite would break those non-call uses (the new
/// signature requires the out-param) and we conservatively skip it.
fn detect_producers(module: &Module) -> HashMap<FuncId, ProducerInfo> {
    let mut candidates: HashMap<FuncId, ProducerInfo> = HashMap::new();
    for func in &module.functions {
        if let Some(info) = analyze_producer(func) {
            candidates.insert(func.id, info);
        }
    }
    if candidates.is_empty() {
        return candidates;
    }
    // Second pass: bail on any candidate whose FuncRef is used as a
    // value (non-callee position) anywhere in the module.
    let mut by_ref_used: HashSet<FuncId> = HashSet::new();
    for func in &module.functions {
        scan_funcref_misuses(&func.body, &candidates, &mut by_ref_used);
    }
    scan_funcref_misuses(&module.init, &candidates, &mut by_ref_used);
    for class in &module.classes {
        for m in &class.methods {
            scan_funcref_misuses(&m.body, &candidates, &mut by_ref_used);
        }
        if let Some(ctor) = &class.constructor {
            scan_funcref_misuses(&ctor.body, &candidates, &mut by_ref_used);
        }
    }
    candidates.retain(|id, _| !by_ref_used.contains(id));
    if candidates.is_empty() {
        return candidates;
    }
    // Third pass: verify every call site of each surviving candidate
    // is in a supported position (`let X = f(args);` at stmt level
    // or its consumer-fuse extension). Calls in expression position
    // (e.g. `f(args).join(...)`, `someFn(f(args))`, `return f(args)`)
    // are unsupported because the rewrite drops the return value;
    // any caller depending on the return-as-value would be silently
    // broken. Conservatively bail on the producer in those cases.
    let mut unsupported_call: HashSet<FuncId> = HashSet::new();
    for func in &module.functions {
        scan_unsafe_call_sites(&func.body, &candidates, &mut unsupported_call);
    }
    scan_unsafe_call_sites(&module.init, &candidates, &mut unsupported_call);
    for class in &module.classes {
        for m in &class.methods {
            scan_unsafe_call_sites(&m.body, &candidates, &mut unsupported_call);
        }
        if let Some(ctor) = &class.constructor {
            scan_unsafe_call_sites(&ctor.body, &candidates, &mut unsupported_call);
        }
    }
    candidates.retain(|id, _| !unsupported_call.contains(id));
    candidates
}

/// Records `FuncId`s whose calls appear in unsupported expression
/// positions. Supported positions:
/// 1. `Stmt::Let { init: Some(Expr::Call { callee: FuncRef(id), .. }) }` — let-bind producer call
/// 2. `Stmt::Expr(Expr::Call { callee: FuncRef(id), .. })` — bare call (return ignored)
///
/// Anywhere else (e.g. `f(args).join()`, `return f(args)`,
/// `someFn(f(args))`) is unsafe because the rewritten producer
/// returns `undefined`. Any caller relying on the array as a value
/// in expression context would break.
fn scan_unsafe_call_sites(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        scan_stmt_call_sites(s, candidates, out);
    }
}

fn scan_stmt_call_sites(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                // Allowed shape: top-level Call { callee: FuncRef(prod) }
                if let Expr::Call { callee, args, .. } = e {
                    if matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                        // The CALL ITSELF is fine. But its args may
                        // themselves contain producer calls in unsafe
                        // positions; recurse into args only.
                        for a in args {
                            scan_expr_call_sites(a, candidates, out);
                        }
                        return;
                    }
                }
                scan_expr_call_sites(e, candidates, out);
            }
        }
        Stmt::Expr(e) => {
            // Allowed shape: top-level Stmt::Expr(Call { callee: FuncRef(prod) })
            if let Expr::Call { callee, args, .. } = e {
                if matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                    for a in args {
                        scan_expr_call_sites(a, candidates, out);
                    }
                    return;
                }
            }
            scan_expr_call_sites(e, candidates, out);
        }
        Stmt::Throw(e) => scan_expr_call_sites(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr_call_sites(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_call_sites(condition, candidates, out);
            scan_unsafe_call_sites(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                scan_unsafe_call_sites(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_call_sites(condition, candidates, out);
            scan_unsafe_call_sites(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_call_sites(i, candidates, out);
            }
            if let Some(c) = condition {
                scan_expr_call_sites(c, candidates, out);
            }
            if let Some(u) = update {
                scan_expr_call_sites(u, candidates, out);
            }
            scan_unsafe_call_sites(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_unsafe_call_sites(body, candidates, out);
            if let Some(c) = catch {
                scan_unsafe_call_sites(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                scan_unsafe_call_sites(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_call_sites(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr_call_sites(t, candidates, out);
                }
                scan_unsafe_call_sites(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_call_sites(body, candidates, out),
        _ => {}
    }
}

/// Walk an expression. Any `Expr::Call { callee: FuncRef(id) }` here
/// (where `id` is in `candidates`) is in expression position (a
/// nested context, not a top-level Stmt::Let or Stmt::Expr) — record
/// the producer as unsafe.
fn scan_expr_call_sites(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    if let Expr::Call { callee, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            if candidates.contains_key(id) {
                out.insert(*id);
            }
        }
    }
    walk_expr_children(e, &mut |child| scan_expr_call_sites(child, candidates, out));
}

/// Records `FuncId`s whose `Expr::FuncRef(id)` is observed in a
/// non-callee position (function value, callback arg, stored to a
/// local, etc.). The set of "misused" producers is then subtracted
/// from the candidate set so the rewrite only fires on functions
/// whose every use is a direct call.
fn scan_funcref_misuses(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        scan_stmt_funcrefs(s, candidates, out);
    }
}

fn scan_stmt_funcrefs(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                scan_expr_funcrefs(e, candidates, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_funcrefs(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_expr_funcrefs(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_funcrefs(condition, candidates, out);
            scan_funcref_misuses(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                scan_funcref_misuses(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_funcrefs(condition, candidates, out);
            scan_funcref_misuses(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_funcrefs(i, candidates, out);
            }
            if let Some(c) = condition {
                scan_expr_funcrefs(c, candidates, out);
            }
            if let Some(u) = update {
                scan_expr_funcrefs(u, candidates, out);
            }
            scan_funcref_misuses(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_funcref_misuses(body, candidates, out);
            if let Some(c) = catch {
                scan_funcref_misuses(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                scan_funcref_misuses(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_funcrefs(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_expr_funcrefs(t, candidates, out);
                }
                scan_funcref_misuses(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_funcrefs(body, candidates, out),
        _ => {}
    }
}

fn scan_expr_funcrefs(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    // Direct callee FuncRefs are SAFE (they're being called). Visit
    // only the args. Anywhere else (a bare FuncRef in argument
    // position, a let-init, etc.) is a "misuse" and we record it.
    match e {
        Expr::Call { callee, args, .. } => {
            // Don't recurse into the FuncRef callee, but DO recurse
            // into anything else.
            if !matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                scan_expr_funcrefs(callee, candidates, out);
            }
            for a in args {
                scan_expr_funcrefs(a, candidates, out);
            }
            return;
        }
        Expr::CallSpread { callee, args, .. } => {
            if !matches!(callee.as_ref(), Expr::FuncRef(id) if candidates.contains_key(id)) {
                scan_expr_funcrefs(callee, candidates, out);
            }
            for a in args {
                match a {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        scan_expr_funcrefs(e, candidates, out);
                    }
                }
            }
            return;
        }
        Expr::FuncRef(id) if candidates.contains_key(id) => {
            // Bare FuncRef in non-callee position → misuse.
            out.insert(*id);
            return;
        }
        _ => {}
    }
    walk_expr_children(e, &mut |child| scan_expr_funcrefs(child, candidates, out));
}

/// Analyze a single function. Returns `Some(ProducerInfo)` if it
/// matches the shape; `None` otherwise.
///
/// MVP shape (tightened over time):
/// 1. Not async, not generator.
/// 2. Exactly one top-level `let out = []` (empty array literal).
/// 3. Exactly one top-level `return LocalGet(out_id)`.
/// 4. The `return` statement is the LAST top-level stmt in the body.
/// 5. `out` is only referenced in:
///    - `out.push(...)` calls (Expr::ArrayPush / Expr::Call on PropertyGet)
///    - The final `return out`
///    - The consume-loop pattern after a recursive call (handled
///      separately during call-site rewrite, not the body analysis).
/// 6. `out` is never reassigned (LocalSet) outside the initial Let.
/// 7. `out` is never passed to a function call as an argument
///    (excluding `.push` member-call dispatch).
fn analyze_producer(func: &Function) -> Option<ProducerInfo> {
    if func.is_async || func.is_generator {
        return None;
    }
    // Exported functions may have callers in other modules that this
    // intra-module pass can't see. Rewriting the signature would
    // break those external callers. Cross-module deforestation needs
    // either a whole-program analysis pass or a wrapper-shim layer
    // that preserves the original signature for external callers
    // while routing internal calls through the rewritten one — both
    // out of MVP scope, filed as follow-up.
    if func.is_exported {
        return None;
    }
    // Closures (functions with captures) live as runtime closure
    // values whose ABI is fixed by the caller's invocation shape.
    // Rewriting the param list would break the closure-call path's
    // arity check at minimum. Skip for now.
    if !func.captures.is_empty() {
        return None;
    }
    // Bail on any producer whose body contains a closure expression.
    // The analyzer's safe-pattern check doesn't walk into closure
    // bodies — `out.push` inside a `.forEach((x) => out.push(x))`
    // closure body would silently pass detection but break at
    // transformation time because the substitution pass also doesn't
    // walk inner closures (their bodies are separate Function entries
    // in the HIR with their own lowering paths). Conservative scope:
    // skip all closure-using producers. Refinement (deferred): walk
    // closure bodies in both analyzer and substituter.
    if body_has_closure(&func.body) {
        return None;
    }
    // Find the top-level `let out = []` and the top-level `return out`.
    let mut out_local: Option<LocalId> = None;
    let mut return_idx: Option<usize> = None;
    let mut return_local: Option<LocalId> = None;

    for (i, stmt) in func.body.iter().enumerate() {
        match stmt {
            Stmt::Let {
                id,
                init: Some(Expr::Array(elems)),
                ..
            } if elems.is_empty() => {
                if out_local.is_some() {
                    // Multiple `let X = []` candidates — bail. We
                    // could disambiguate by checking which one is
                    // returned, but the simpler path is to just bail.
                    return None;
                }
                out_local = Some(*id);
            }
            Stmt::Return(Some(Expr::LocalGet(id))) => {
                if return_idx.is_some() {
                    // Multiple top-level returns — for safety, bail.
                    // (A fancier implementation could verify each
                    // returns the same out-local.)
                    return None;
                }
                return_idx = Some(i);
                return_local = Some(*id);
            }
            _ => {}
        }
    }

    let out_id = out_local?;
    let ret_id = return_local?;
    if out_id != ret_id {
        return None;
    }
    // Required: the return is the last top-level stmt.
    if return_idx? != func.body.len() - 1 {
        return None;
    }
    // Required: no other return statements anywhere in the body (nested
    // in if/for/while/try/switch). Multiple returns make the rewrite
    // unsound — some paths might return a different shape.
    let mut nested_returns = 0u32;
    for (i, s) in func.body.iter().enumerate() {
        if i == return_idx? {
            continue;
        }
        if stmt_contains_return(s) {
            nested_returns += 1;
        }
    }
    if nested_returns > 0 {
        return None;
    }

    // Now check that `out` is never used in an unsafe shape. Walk the
    // entire body (including nested control flow) and disqualify on
    // any LocalGet(out) / LocalSet(out) outside the allowed contexts.
    let mut analyzer = OutUsageAnalyzer {
        out_id,
        unsafe_use: false,
    };
    for (i, stmt) in func.body.iter().enumerate() {
        // Skip the initial Let (its LocalSet of `out` is fine — it's
        // the binding) and the final Return (its LocalGet of `out` is
        // fine — handled by the rewrite).
        if matches!(stmt, Stmt::Let { id, .. } if *id == out_id) {
            continue;
        }
        if i == return_idx? {
            continue;
        }
        analyzer.visit_stmt(stmt);
        if analyzer.unsafe_use {
            return None;
        }
    }

    let elem_ty = match &func.return_type {
        Type::Array(inner) => (**inner).clone(),
        _ => Type::Any,
    };
    Some(ProducerInfo {
        out_local_id: out_id,
        original_param_count: func.params.len(),
        elem_ty,
    })
}

/// Returns true if any expression anywhere in `stmts` (including
/// nested stmts) is an `Expr::Closure`. Producers with inner closures
/// are conservatively skipped because the analyzer's safe-pattern
/// check doesn't walk closure bodies — a closure body referencing
/// `out` would slip past detection and break the substitution pass.
fn body_has_closure(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_closure)
}

fn stmt_has_closure(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_closure),
        Stmt::Expr(e) | Stmt::Throw(e) => expr_has_closure(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_has_closure),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_has_closure(condition)
                || body_has_closure(then_branch)
                || else_branch.as_ref().is_some_and(|eb| body_has_closure(eb))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_has_closure(condition) || body_has_closure(body)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|i| stmt_has_closure(i))
                || condition.as_ref().is_some_and(expr_has_closure)
                || update.as_ref().is_some_and(expr_has_closure)
                || body_has_closure(body)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body_has_closure(body)
                || catch.as_ref().is_some_and(|c| body_has_closure(&c.body))
                || finally.as_ref().is_some_and(|f| body_has_closure(f))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_closure(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_has_closure) || body_has_closure(&c.body)
                })
        }
        Stmt::Labeled { body, .. } => stmt_has_closure(body),
        _ => false,
    }
}

fn expr_has_closure(e: &Expr) -> bool {
    if matches!(e, Expr::Closure { .. }) {
        return true;
    }
    let mut found = false;
    walk_expr_children(e, &mut |child| {
        if !found && expr_has_closure(child) {
            found = true;
        }
    });
    found
}

/// Recursive: returns true if `stmt` (or any nested stmt inside it)
/// is a `Stmt::Return`. Used by the producer analyzer to gate on
/// "single top-level return" — if any deeper stmt is also a return,
/// the function has multiple control-flow exits and the rewrite is
/// unsafe.
fn stmt_contains_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_return)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| eb.iter().any(stmt_contains_return))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_return)
        }
        Stmt::For { init, body, .. } => {
            init.as_ref().is_some_and(|i| stmt_contains_return(i))
                || body.iter().any(stmt_contains_return)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_return)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_contains_return))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_return))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(stmt_contains_return)),
        Stmt::Labeled { body, .. } => stmt_contains_return(body),
        _ => false,
    }
}

/// Walks an HIR subtree looking for unsafe uses of a target local.
/// "Safe" uses are limited to:
/// - `out.push(value)` — both as `Expr::ArrayPush { array: LocalGet(out), value }`
///   and as a generic `Expr::Call { callee: PropertyGet { LocalGet(out), "push" } }`.
/// - `out[index]` reads (Expr::IndexGet) — they don't escape the
///   array, and the rewrite doesn't change their semantics.
/// - `out.length` reads (PropertyGet `.length`) — same.
/// - The consumer-pattern shape (a parent `for` loop reading
///   `child.length` / `child[j]` and calling `outer.push`) — checked
///   at call-site time.
struct OutUsageAnalyzer {
    out_id: LocalId,
    unsafe_use: bool,
}

impl OutUsageAnalyzer {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        if self.unsafe_use {
            return;
        }
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(e) = &c.test {
                        self.visit_expr(e);
                    }
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            // Other stmt kinds: no expressions to visit at this level.
            _ => {}
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if self.unsafe_use {
            return;
        }
        // Check for SAFE patterns first; if matched, dive only into
        // their non-`out` subexpressions and skip the catch-all walk
        // below (which would otherwise flag the LocalGet(out) inside
        // them as unsafe).
        match e {
            Expr::ArrayPush { array_id, value } => {
                if *array_id == self.out_id {
                    // Safe: out.push(v). Visit only `value`.
                    self.visit_expr(value);
                    return;
                }
            }
            Expr::ArrayPushSpread { array_id, source } => {
                if *array_id == self.out_id {
                    self.visit_expr(source);
                    return;
                }
            }
            Expr::PropertyGet { object, property } => {
                if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.out_id)
                    && property == "length"
                {
                    // Safe: out.length read.
                    return;
                }
            }
            Expr::IndexGet { object, index } => {
                if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.out_id) {
                    // Safe: out[idx] read. Still need to visit index
                    // because it might contain its own out-references.
                    self.visit_expr(index);
                    return;
                }
            }
            Expr::LocalSet(id, value) if *id == self.out_id => {
                // out = X — disallowed except for the initial Let
                // (which the caller filters out). Any post-init
                // reassignment breaks the rewrite.
                self.unsafe_use = true;
                return;
            }
            Expr::LocalGet(id) if *id == self.out_id => {
                // Bare LocalGet(out) outside of a safe parent pattern.
                self.unsafe_use = true;
                return;
            }
            _ => {}
        }
        // Catch-all: walk children.
        walk_expr_children(e, &mut |child| self.visit_expr(child));
    }
}

/// Generic child-walker for `Expr` — visits every direct sub-expression.
/// Conservative: any expression kind we don't know how to walk into
/// concretely is treated as a leaf (the analyzer will then visit only
/// what `walk_expr_children` enumerates, missing nothing important
/// for the producer-detection pass).
fn walk_expr_children(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    use Expr::*;
    match e {
        Undefined
        | Null
        | Bool(_)
        | Number(_)
        | Integer(_)
        | BigInt(_)
        | String(_)
        | WtfString(_)
        | LocalGet(_)
        | GlobalGet(_)
        | FuncRef(_)
        | ExternFuncRef { .. }
        | NativeModuleRef(_) => {}
        I18nString { params, .. } => {
            for (_, e) in params {
                f(e);
            }
        }
        LocalSet(_, e)
        | GlobalSet(_, e)
        | Unary { operand: e, .. }
        | TypeOf(e)
        | Void(e)
        | Await(e)
        | InstanceOf { expr: e, .. } => f(e),
        Update { .. } => {}
        Binary { left, right, .. } | Compare { left, right, .. } | Logical { left, right, .. } => {
            f(left);
            f(right);
        }
        Call { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        CallSpread { callee, args, .. } => {
            f(callee);
            for a in args {
                match a {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => f(e),
                }
            }
        }
        Object(fields) => {
            for (_, v) in fields {
                f(v);
            }
        }
        ObjectSpread { parts } => {
            for (_, v) in parts {
                f(v);
            }
        }
        ObjectAssign { target, sources } => {
            f(target);
            for s in sources {
                f(s);
            }
        }
        Array(elems) => {
            for e in elems {
                f(e);
            }
        }
        ArraySpread(elems) => {
            for elem in elems {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => f(e),
                }
            }
        }
        Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            f(condition);
            f(then_expr);
            f(else_expr);
        }
        In { property, object } => {
            f(property);
            f(object);
        }
        Yield { value, .. } => {
            if let Some(v) = value {
                f(v);
            }
        }
        New { args, .. } => {
            for a in args {
                f(a);
            }
        }
        NewDynamic { callee, args } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        PropertyGet { object, .. } => f(object),
        PropertySet { object, value, .. } => {
            f(object);
            f(value);
        }
        PropertyUpdate { object, .. } => f(object),
        IndexGet { object, index } => {
            f(object);
            f(index);
        }
        IndexSet {
            object,
            index,
            value,
        } => {
            f(object);
            f(index);
            f(value);
        }
        ArrayPush { value, .. } => f(value),
        ArrayPushSpread { source, .. } => f(source),
        // Conservative default: don't recurse into less-common variants
        // for the MVP. Detection will reject these as unsafe via the
        // catch-all `LocalGet(out)` check at the parent level.
        _ => {}
    }
}

/// Returns the highest LocalId seen anywhere in the module — used as
/// the seed for fresh-id allocation when adding synthetic params /
/// temporaries.
fn max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    for f in &module.functions {
        for p in &f.params {
            max_id = max_id.max(p.id);
        }
        max_in_stmts(&f.body, &mut max_id);
    }
    max_in_stmts(&module.init, &mut max_id);
    for c in &module.classes {
        for m in &c.methods {
            for p in &m.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&m.body, &mut max_id);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                max_id = max_id.max(p.id);
            }
            max_in_stmts(&ctor.body, &mut max_id);
        }
    }
    max_id
}

fn max_in_stmts(stmts: &[Stmt], max_id: &mut LocalId) {
    for s in stmts {
        max_in_stmt(s, max_id);
    }
}

fn max_in_stmt(stmt: &Stmt, max_id: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *max_id = (*max_id).max(*id);
            if let Some(e) = init {
                max_in_expr(e, max_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => max_in_expr(e, max_id),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                max_in_expr(e, max_id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            max_in_expr(condition, max_id);
            max_in_stmts(then_branch, max_id);
            if let Some(eb) = else_branch {
                max_in_stmts(eb, max_id);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            max_in_expr(condition, max_id);
            max_in_stmts(body, max_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                max_in_stmt(i, max_id);
            }
            if let Some(c) = condition {
                max_in_expr(c, max_id);
            }
            if let Some(u) = update {
                max_in_expr(u, max_id);
            }
            max_in_stmts(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            max_in_stmts(body, max_id);
            if let Some(c) = catch {
                if let Some((id, _)) = &c.param {
                    *max_id = (*max_id).max(*id);
                }
                max_in_stmts(&c.body, max_id);
            }
            if let Some(f) = finally {
                max_in_stmts(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            max_in_expr(discriminant, max_id);
            for c in cases {
                if let Some(t) = &c.test {
                    max_in_expr(t, max_id);
                }
                max_in_stmts(&c.body, max_id);
            }
        }
        Stmt::Labeled { body, .. } => max_in_stmt(body, max_id),
        Stmt::PreallocateBoxes(ids) => {
            for id in ids {
                *max_id = (*max_id).max(*id);
            }
        }
        _ => {}
    }
}

fn max_in_expr(e: &Expr, max_id: &mut LocalId) {
    match e {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => *max_id = (*max_id).max(*id),
        Expr::Update { id, .. } => *max_id = (*max_id).max(*id),
        _ => {}
    }
    walk_expr_children(e, &mut |child| max_in_expr(child, max_id));
}

/// Phase 2 — rewrite a producer's body to use the new out-param.
/// Removes the `let out = []` line, replaces every `LocalGet(out)`
/// that survives the analyzer (push targets, array reads on the
/// accumulator) with `LocalGet(out_param)`, drops the trailing
/// `return out`, and rewrites recursive calls within the consume
/// pattern to pass the param through.
fn rewrite_producer_body(
    func: &mut Function,
    info: &ProducerInfo,
    out_param: LocalId,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
) {
    // 1. Insert the synthetic param at the END of the param list.
    func.params.push(perry_hir::Param {
        id: out_param,
        name: "__deforest_out".to_string(),
        ty: Type::Array(Box::new(info.elem_ty.clone())),
        default: None,
        decorators: Vec::new(),
        is_rest: false,
    });

    // 2. Drop the trailing `return out`.
    if matches!(func.body.last(), Some(Stmt::Return(_))) {
        func.body.pop();
    }

    // 3. Drop the leading `let out = []` (or any position where the
    //    out-local is bound).
    func.body
        .retain(|s| !matches!(s, Stmt::Let { id, .. } if *id == info.out_local_id));

    // 4. Substitute every reference to `out_local_id` with
    //    `out_param`. Same shape walk as the analyzer; this time we
    //    mutate.
    let mut subst = SubstituteLocal {
        from: info.out_local_id,
        to: out_param,
    };
    for s in &mut func.body {
        subst.visit_stmt(s);
    }

    // 5. Rewrite call sites inside the producer body — both
    //    consumer-pattern call sites (fuse into pass-through) and
    //    bare recursive calls (pass `out_param` through directly).
    let mut next_local = max_local_id_for_func(func) + 1;
    rewrite_call_sites_in_stmts_with_local_pass(
        &mut func.body,
        producers,
        out_param_ids,
        &mut next_local,
        Some(out_param),
    );
}

/// Mutating equivalent of `OutUsageAnalyzer`'s walker — substitutes
/// every `LocalGet(from)` and every `LocalSet(from, ...)` with `to`.
/// Doesn't touch other local references.
struct SubstituteLocal {
    from: LocalId,
    to: LocalId,
}

impl SubstituteLocal {
    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &mut c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(t) = &mut c.test {
                        self.visit_expr(t);
                    }
                    for s in &mut c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            _ => {}
        }
    }

    fn visit_expr(&mut self, e: &mut Expr) {
        // Direct local-id field rewrites. These variants have a
        // `LocalId` (not `Box<Expr>`) field referencing the array
        // being mutated; the generic walker doesn't visit those, so
        // they need explicit handling here.
        match e {
            Expr::LocalGet(id) if *id == self.from => {
                *id = self.to;
                return;
            }
            Expr::LocalSet(id, val) if *id == self.from => {
                *id = self.to;
                self.visit_expr(val);
                return;
            }
            Expr::Update { id, .. } if *id == self.from => {
                *id = self.to;
                return;
            }
            Expr::ArrayPush { array_id, value } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(value);
                return;
            }
            Expr::ArrayPushSpread { array_id, source } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(source);
                return;
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                if *id == self.from {
                    *id = self.to;
                }
                return;
            }
            Expr::ArrayUnshift { array_id, value } => {
                if *array_id == self.from {
                    *array_id = self.to;
                }
                self.visit_expr(value);
                return;
            }
            _ => {}
        }
        walk_expr_children_mut(e, &mut |child| self.visit_expr(child));
    }
}

/// Mutable child-walker for Expr. Mirrors `walk_expr_children`.
fn walk_expr_children_mut(e: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    use Expr::*;
    match e {
        Undefined
        | Null
        | Bool(_)
        | Number(_)
        | Integer(_)
        | BigInt(_)
        | String(_)
        | WtfString(_)
        | LocalGet(_)
        | GlobalGet(_)
        | FuncRef(_)
        | ExternFuncRef { .. }
        | NativeModuleRef(_)
        | Update { .. } => {}
        I18nString { params, .. } => {
            for (_, e) in params {
                f(e);
            }
        }
        LocalSet(_, e)
        | GlobalSet(_, e)
        | Unary { operand: e, .. }
        | TypeOf(e)
        | Void(e)
        | Await(e)
        | InstanceOf { expr: e, .. } => f(e),
        Binary { left, right, .. } | Compare { left, right, .. } | Logical { left, right, .. } => {
            f(left);
            f(right);
        }
        Call { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        CallSpread { callee, args, .. } => {
            f(callee);
            for a in args {
                match a {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => f(e),
                }
            }
        }
        Object(fields) => {
            for (_, v) in fields {
                f(v);
            }
        }
        ObjectSpread { parts } => {
            for (_, v) in parts {
                f(v);
            }
        }
        ObjectAssign { target, sources } => {
            f(target);
            for s in sources {
                f(s);
            }
        }
        Array(elems) => {
            for e in elems {
                f(e);
            }
        }
        ArraySpread(elems) => {
            for elem in elems {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => f(e),
                }
            }
        }
        Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            f(condition);
            f(then_expr);
            f(else_expr);
        }
        In { property, object } => {
            f(property);
            f(object);
        }
        Yield { value, .. } => {
            if let Some(v) = value {
                f(v);
            }
        }
        New { args, .. } => {
            for a in args {
                f(a);
            }
        }
        NewDynamic { callee, args } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        PropertyGet { object, .. } => f(object),
        PropertySet { object, value, .. } => {
            f(object);
            f(value);
        }
        PropertyUpdate { object, .. } => f(object),
        IndexGet { object, index } => {
            f(object);
            f(index);
        }
        IndexSet {
            object,
            index,
            value,
        } => {
            f(object);
            f(index);
            f(value);
        }
        // #853: the `PropertyUpdate` arm earlier in this match (around
        // line 1511) already covers this variant. Duplicate removed.
        ArrayPush { value, .. } => f(value),
        ArrayPushSpread { source, .. } => f(source),
        _ => {}
    }
}

fn max_local_id_for_func(func: &Function) -> LocalId {
    let mut max_id: LocalId = 0;
    for p in &func.params {
        max_id = max_id.max(p.id);
    }
    max_in_stmts(&func.body, &mut max_id);
    max_id
}

/// Phase 3 — rewrite call sites in a Stmt sequence. Two patterns are
/// recognized:
///
/// **Consumer-fuse:** `let X = f(args); for(j) outer.push(X[j]);`
/// — rewrites to `f(args, outer)` and drops the consume loop. `X` is
/// no longer needed.
///
/// **Pass-through (bare call):** `f(args);` (Stmt::Expr) where `f` is
/// a producer — rewrites to `f(args, fresh_acc)` with a fresh local
/// `let fresh_acc = []` inserted just before the call. The fresh
/// accumulator is dropped; matches the original semantics of "call
/// for side effects, ignore return value".
///
/// **Value-binding (consumed elsewhere):** `let Y = f(args);` where
/// `Y` is used by following stmts in non-consume-loop shapes —
/// rewrites to `let Y = []; f(args, Y);`. After this, `Y` is the
/// populated array, indistinguishable from the pre-rewrite return
/// value.
fn rewrite_call_sites_in_stmts(
    stmts: &mut Vec<Stmt>,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
) {
    rewrite_call_sites_in_stmts_with_local_pass(stmts, producers, out_param_ids, next_local, None);
}

/// Like `rewrite_call_sites_in_stmts` but additionally aware of an
/// in-scope accumulator local (`current_out`). When set, recursive
/// pass-through calls thread `current_out` through directly instead
/// of allocating a fresh accumulator — this is the inner-recursion
/// fusion that delivers the actual ABC451D speedup.
fn rewrite_call_sites_in_stmts_with_local_pass(
    stmts: &mut Vec<Stmt>,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
    current_out: Option<LocalId>,
) {
    let mut i = 0;
    while i < stmts.len() {
        // Try the consumer-fuse pattern first: `let X = f(args);` followed
        // by `for(j) outer.push(X[j]);`.
        if let Some((consumed_steps, replacement)) =
            try_consumer_fuse_pattern(&stmts[i..], producers, out_param_ids)
        {
            // Remove `consumed_steps` stmts starting at i, replace with
            // `replacement`.
            stmts.drain(i..i + consumed_steps);
            for (offset, s) in replacement.into_iter().enumerate() {
                stmts.insert(i + offset, s);
            }
            // Don't advance i — recurse into the replacement (it has
            // no further patterns to rewrite, so move past).
            i += 1;
            continue;
        }

        // Non-fuse case: rewrite single-stmt patterns in place.
        if let Some(replacement) =
            try_rewrite_single_stmt(&stmts[i], producers, out_param_ids, next_local, current_out)
        {
            stmts[i] = replacement.0;
            // Insert any extra stmts AFTER the rewritten one (e.g. the
            // call after a fresh `let X = []`).
            for (offset, s) in replacement.1.into_iter().enumerate() {
                stmts.insert(i + 1 + offset, s);
            }
            i += 1;
            continue;
        }

        // Recurse into nested control flow.
        match &mut stmts[i] {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    then_branch,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                if let Some(eb) = else_branch {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        eb,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
            }
            Stmt::For { body, .. } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                if let Some(c) = catch {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        &mut c.body,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
                if let Some(f) = finally {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        f,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        &mut c.body,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut tmp = vec![std::mem::replace(
                    body.as_mut(),
                    Stmt::Expr(Expr::Undefined),
                )];
                rewrite_call_sites_in_stmts_with_local_pass(
                    &mut tmp,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                *body = Box::new(tmp.into_iter().next().unwrap());
            }
            _ => {}
        }
        i += 1;
    }
}

/// Try to recognize the consumer-fuse pattern at `stmts[0..]`:
///
///   stmts[0]: Stmt::Let { id: child, init: Some(Call { callee: FuncRef(f), args }) }
///   stmts[1]: Stmt::For { for(j=0; j<child.length; j++) outer.push(child[j]); }
///
/// where `f` is a deforestable producer and `child` has no further
/// uses after stmts[1].
///
/// Returns `Some((consumed_count, replacement_stmts))` where
/// `consumed_count` is 2 (we replace both stmts) and the replacement
/// is `[Stmt::Expr(Call { f, args ++ [outer] })]`.
fn try_consumer_fuse_pattern(
    stmts: &[Stmt],
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
) -> Option<(usize, Vec<Stmt>)> {
    if stmts.len() < 2 {
        return None;
    }
    let (child_id, callee_id, call_args, type_args) = match &stmts[0] {
        Stmt::Let {
            id,
            init:
                Some(Expr::Call {
                    callee,
                    args,
                    type_args,
                }),
            ..
        } => match callee.as_ref() {
            Expr::FuncRef(fid) if producers.contains_key(fid) => {
                (*id, *fid, args.clone(), type_args.clone())
            }
            _ => return None,
        },
        _ => return None,
    };

    // Recognize: `for (let j = 0; j < child.length; j++) outer.push(child[j]);`
    let outer_id = match &stmts[1] {
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => match_consume_loop(child_id, init, condition, update, body)?,
        _ => return None,
    };

    // `child` must not be referenced anywhere AFTER stmts[1].
    let mut later_uses = false;
    for s in &stmts[2..] {
        if stmt_references_local(s, child_id) {
            later_uses = true;
            break;
        }
    }
    if later_uses {
        return None;
    }

    // Build replacement: `f(args, outer);`
    let mut new_args = call_args;
    new_args.push(Expr::LocalGet(outer_id));
    let _ = out_param_ids.get(&callee_id)?; // sanity check producer was sized
    let new_call = Expr::Call {
        callee: Box::new(Expr::FuncRef(callee_id)),
        args: new_args,
        type_args,
    };
    Some((2, vec![Stmt::Expr(new_call)]))
}

/// Match the for-loop shape `for (let j = 0; j < child.length; j++)
/// outer.push(child[j]);` and return `outer`'s LocalId on success.
fn match_consume_loop(
    child_id: LocalId,
    init: &Option<Box<Stmt>>,
    condition: &Option<Expr>,
    update: &Option<Expr>,
    body: &[Stmt],
) -> Option<LocalId> {
    // init: let j = 0
    let init_stmt = init.as_ref()?;
    let j_id = match init_stmt.as_ref() {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        } => *id,
        Stmt::Let {
            id,
            init: Some(Expr::Number(n)),
            ..
        } if *n == 0.0 => *id,
        _ => return None,
    };

    // condition: j < child.length
    let cond = condition.as_ref()?;
    match cond {
        Expr::Compare {
            op: perry_hir::CompareOp::Lt,
            left,
            right,
        } => {
            if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == j_id) {
                return None;
            }
            match right.as_ref() {
                Expr::PropertyGet { object, property } if property == "length" => {
                    if !matches!(object.as_ref(), Expr::LocalGet(id) if *id == child_id) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    }

    // update: j++ or ++j (Update { id: j, op: Inc, .. })
    let upd = update.as_ref()?;
    match upd {
        Expr::Update { id, op, .. }
            if *id == j_id && matches!(op, perry_hir::UpdateOp::Increment) => {}
        _ => return None,
    }

    // body: exactly one stmt — outer.push(child[j])
    if body.len() != 1 {
        return None;
    }
    let push_call = match &body[0] {
        Stmt::Expr(e) => e,
        _ => return None,
    };
    // Match either ArrayPush { array: LocalGet(outer), value: IndexGet { ... } }
    // OR Call { callee: PropertyGet { LocalGet(outer), "push" }, args: [IndexGet...] }
    match push_call {
        Expr::ArrayPush { array_id, value } => {
            if !is_index_get_of(value, child_id, j_id) {
                return None;
            }
            Some(*array_id)
        }
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::PropertyGet { object, property } if property == "push" => {
                let outer_id = match object.as_ref() {
                    Expr::LocalGet(id) => *id,
                    _ => return None,
                };
                if args.len() != 1 {
                    return None;
                }
                if !is_index_get_of(&args[0], child_id, j_id) {
                    return None;
                }
                Some(outer_id)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Match `child[j]` (IndexGet { object: LocalGet(child), index: LocalGet(j) }).
fn is_index_get_of(e: &Expr, child_id: LocalId, j_id: LocalId) -> bool {
    match e {
        Expr::IndexGet { object, index } => {
            matches!(object.as_ref(), Expr::LocalGet(id) if *id == child_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == j_id)
        }
        _ => false,
    }
}

/// Single-stmt-level rewrites. Returns `Some((replacement, extras))`
/// where `extras` are stmts to insert AFTER the replacement.
///
/// Currently handles:
/// - **Bare expression call:** `f(args);` (Stmt::Expr) — rewrites to
///   pass `current_out` (if available) or a fresh accumulator.
/// - **Let-bind producer call:** `let Y = f(args);` — rewrites to
///   `let Y = []; f(args, Y);`.
fn try_rewrite_single_stmt(
    stmt: &Stmt,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
    _current_out: Option<LocalId>,
) -> Option<(Stmt, Vec<Stmt>)> {
    match stmt {
        Stmt::Let {
            id,
            name,
            ty,
            mutable,
            init:
                Some(Expr::Call {
                    callee,
                    args,
                    type_args,
                }),
        } => match callee.as_ref() {
            Expr::FuncRef(fid) if producers.contains_key(fid) => {
                let info = producers.get(fid)?;
                let mut new_args = args.clone();
                new_args.push(Expr::LocalGet(*id));
                let call_stmt = Stmt::Expr(Expr::Call {
                    callee: callee.clone(),
                    args: new_args,
                    type_args: type_args.clone(),
                });
                let let_stmt = Stmt::Let {
                    id: *id,
                    name: name.clone(),
                    ty: Type::Array(Box::new(info.elem_ty.clone())),
                    mutable: *mutable,
                    init: Some(Expr::Array(Vec::new())),
                };
                let _ = out_param_ids; // already validated producer
                let _ = next_local;
                Some((let_stmt, vec![call_stmt]))
            }
            _ => None,
        },
        // `f(args);` as a bare expression: rare for producers, since
        // the return value is the whole point. Skip for now —
        // misclassifying these is more dangerous than missing them.
        _ => None,
    }
}

/// Returns true if `stmt` references `target_id` anywhere in its
/// expressions (including nested).
fn stmt_references_local(stmt: &Stmt, target_id: LocalId) -> bool {
    let mut found = false;
    let mut walker = StmtRefWalker {
        target: target_id,
        found: &mut found,
    };
    walker.visit_stmt(stmt);
    found
}

struct StmtRefWalker<'a> {
    target: LocalId,
    found: &'a mut bool,
}

impl<'a> StmtRefWalker<'a> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        if *self.found {
            return;
        }
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.visit_expr(t);
                    }
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            _ => {}
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        if *self.found {
            return;
        }
        match e {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. }
                if *id == self.target =>
            {
                *self.found = true;
                return;
            }
            _ => {}
        }
        walk_expr_children(e, &mut |child| self.visit_expr(child));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sanity tests at the helper level. End-to-end tests live in
    // test-files/test_deforest_*.ts (compiled + run vs Node).

    #[test]
    fn detects_simple_producer() {
        // function f() { const out = []; out.push(1); return out; }
        let func = Function {
            id: 1,
            name: "f".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Array(Box::new(Type::Number)),
            body: vec![
                Stmt::Let {
                    id: 10,
                    name: "out".to_string(),
                    ty: Type::Array(Box::new(Type::Number)),
                    mutable: false,
                    init: Some(Expr::Array(vec![])),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 10,
                    value: Box::new(Expr::Integer(1)),
                }),
                Stmt::Return(Some(Expr::LocalGet(10))),
            ],
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        };
        let info = analyze_producer(&func).expect("should detect producer");
        assert_eq!(info.out_local_id, 10);
        assert_eq!(info.original_param_count, 0);
        assert!(matches!(info.elem_ty, Type::Number));
    }

    #[test]
    fn rejects_async_producer() {
        let mut func = make_simple_producer();
        func.is_async = true;
        assert!(analyze_producer(&func).is_none());
    }

    #[test]
    fn rejects_producer_with_out_passed_to_call() {
        // function f() { const out = []; helper(out); return out; }
        // Passing `out` to `helper` is unsafe — it might escape.
        let mut func = make_simple_producer();
        // Replace the push with `helper(out)`.
        func.body[1] = Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(99)),
            args: vec![Expr::LocalGet(10)],
            type_args: vec![],
        });
        assert!(analyze_producer(&func).is_none());
    }

    #[test]
    fn rejects_producer_with_reassignment() {
        // function f() { const out = []; out = [1, 2]; return out; }
        let mut func = make_simple_producer();
        func.body[1] = Stmt::Expr(Expr::LocalSet(
            10,
            Box::new(Expr::Array(vec![Expr::Integer(1)])),
        ));
        assert!(analyze_producer(&func).is_none());
    }

    #[test]
    fn rejects_producer_with_multiple_returns() {
        // function f(cond) { const out = []; if (cond) return []; return out; }
        let mut func = make_simple_producer();
        func.body.insert(
            1,
            Stmt::If {
                condition: Expr::Bool(true),
                then_branch: vec![Stmt::Return(Some(Expr::Array(vec![])))],
                else_branch: None,
            },
        );
        assert!(analyze_producer(&func).is_none());
    }

    fn make_simple_producer() -> Function {
        Function {
            id: 1,
            name: "f".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Array(Box::new(Type::Number)),
            body: vec![
                Stmt::Let {
                    id: 10,
                    name: "out".to_string(),
                    ty: Type::Array(Box::new(Type::Number)),
                    mutable: false,
                    init: Some(Expr::Array(vec![])),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 10,
                    value: Box::new(Expr::Integer(1)),
                }),
                Stmt::Return(Some(Expr::LocalGet(10))),
            ],
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: false,
            was_unrolled: false,
        }
    }
}
