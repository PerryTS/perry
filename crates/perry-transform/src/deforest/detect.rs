//! Producer-shape detection: walk the module looking for functions
//! whose body matches the `let out = []; ...; return out` shape, then
//! second-pass-validate that every reference to those candidates is
//! either a direct call or a supported call-site position.

use super::*;

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
pub fn detect_producers(module: &Module) -> HashMap<FuncId, ProducerInfo> {
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
        for (_, getter) in &class.getters {
            scan_funcref_misuses(&getter.body, &candidates, &mut by_ref_used);
        }
        for (_, setter) in &class.setters {
            scan_funcref_misuses(&setter.body, &candidates, &mut by_ref_used);
        }
        for m in &class.static_methods {
            scan_funcref_misuses(&m.body, &candidates, &mut by_ref_used);
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
            // A member body that references `super` can't have its
            // producer call sites rewritten — the deforest rewrite
            // introduces synthetic locals that corrupt [[HomeObject]]
            // setup, breaking `super.x` / `super[e]` / `super(...)`.
            // Treat any producer called from such a body as an unsafe
            // call site to exclude it from deforestation entirely.
            // Refs #5780 cluster A / #5772.
            if body_has_super(&m.body) {
                flag_producer_calls_in_super_body(&m.body, &candidates, &mut unsupported_call);
            } else {
                scan_unsafe_call_sites(&m.body, &candidates, &mut unsupported_call);
            }
        }
        if let Some(ctor) = &class.constructor {
            if body_has_super(&ctor.body) {
                flag_producer_calls_in_super_body(&ctor.body, &candidates, &mut unsupported_call);
            } else {
                scan_unsafe_call_sites(&ctor.body, &candidates, &mut unsupported_call);
            }
        }
        for (_, getter) in &class.getters {
            if body_has_super(&getter.body) {
                flag_producer_calls_in_super_body(&getter.body, &candidates, &mut unsupported_call);
            } else {
                scan_unsafe_call_sites(&getter.body, &candidates, &mut unsupported_call);
            }
        }
        for (_, setter) in &class.setters {
            if body_has_super(&setter.body) {
                flag_producer_calls_in_super_body(&setter.body, &candidates, &mut unsupported_call);
            } else {
                scan_unsafe_call_sites(&setter.body, &candidates, &mut unsupported_call);
            }
        }
        for m in &class.static_methods {
            if body_has_super(&m.body) {
                flag_producer_calls_in_super_body(&m.body, &candidates, &mut unsupported_call);
            } else {
                scan_unsafe_call_sites(&m.body, &candidates, &mut unsupported_call);
            }
        }
    }
    candidates.retain(|id, _| !unsupported_call.contains(id));
    if candidates.is_empty() {
        return candidates;
    }
    // Fourth pass: bail on any candidate referenced inside a closure
    // body. Neither the detection scans above nor the phase-3 call-site
    // rewriter descend into closure bodies, so a producer called from
    // inside a closure would have its signature rewritten while the
    // in-closure call site kept the original arity — a mismatch that
    // miscompiles to a SIGSEGV. Refs #5136.
    let mut in_closure: HashSet<FuncId> = HashSet::new();
    for func in &module.functions {
        scan_producers_used_in_closures(&func.body, &candidates, &mut in_closure);
    }
    scan_producers_used_in_closures(&module.init, &candidates, &mut in_closure);
    for class in &module.classes {
        for m in &class.methods {
            scan_producers_used_in_closures(&m.body, &candidates, &mut in_closure);
        }
        if let Some(ctor) = &class.constructor {
            scan_producers_used_in_closures(&ctor.body, &candidates, &mut in_closure);
        }
        for (_, getter) in &class.getters {
            scan_producers_used_in_closures(&getter.body, &candidates, &mut in_closure);
        }
        for (_, setter) in &class.setters {
            scan_producers_used_in_closures(&setter.body, &candidates, &mut in_closure);
        }
        for m in &class.static_methods {
            scan_producers_used_in_closures(&m.body, &candidates, &mut in_closure);
        }
    }
    candidates.retain(|id, _| !in_closure.contains(id));
    candidates
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
pub fn analyze_producer(func: &Function) -> Option<ProducerInfo> {
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
pub fn body_has_closure(stmts: &[Stmt]) -> bool {
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

/// Returns true if any expression anywhere in `stmts` (including
/// nested stmts and nested control flow) is a `super` reference:
/// `SuperPropertyGet`, `SuperCall`, `SuperMethodCall`, etc.
///
/// Used to exclude class member bodies that use `super` from deforest
/// call-site rewrites — the rewrite introduces synthetic locals that
/// corrupt [[HomeObject]] setup, breaking super property access and
/// super calls. Refs #5780 cluster A.
pub fn body_has_super(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_super)
}

fn stmt_has_super(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_super),
        Stmt::Expr(e) | Stmt::Throw(e) => expr_has_super(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_has_super),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_has_super(condition)
                || body_has_super(then_branch)
                || else_branch.as_ref().is_some_and(|eb| body_has_super(eb))
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_has_super(condition) || body_has_super(body)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|i| stmt_has_super(i))
                || condition.as_ref().is_some_and(expr_has_super)
                || update.as_ref().is_some_and(expr_has_super)
                || body_has_super(body)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body_has_super(body)
                || catch.as_ref().is_some_and(|c| body_has_super(&c.body))
                || finally.as_ref().is_some_and(|f| body_has_super(f))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_super(discriminant)
                || cases
                    .iter()
                    .any(|c| c.test.as_ref().is_some_and(expr_has_super) || body_has_super(&c.body))
        }
        Stmt::Labeled { body, .. } => stmt_has_super(body),
        _ => false,
    }
}

fn expr_has_super(e: &Expr) -> bool {
    if matches!(
        e,
        Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::ObjectSuperPropertyGet { .. }
            | Expr::ObjectSuperPropertySet { .. }
            | Expr::ObjectSuperMethodCall { .. }
    ) {
        return true;
    }
    let mut found = false;
    walk_expr_children(e, &mut |child| {
        if !found && expr_has_super(child) {
            found = true;
        }
    });
    found
}

/// Flag every call to a candidate producer found anywhere in `stmts`
/// as an unsafe call site. Used for class member bodies that contain
/// `super` references — all positions are unsafe because the rewrite
/// would corrupt [[HomeObject]].
fn flag_producer_calls_in_super_body(
    stmts: &[Stmt],
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    for s in stmts {
        flag_producer_calls_in_stmt(s, candidates, out);
    }
}

fn flag_producer_calls_in_stmt(
    stmt: &Stmt,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                flag_producer_calls_in_expr(e, candidates, out);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => flag_producer_calls_in_expr(e, candidates, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                flag_producer_calls_in_expr(e, candidates, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            flag_producer_calls_in_expr(condition, candidates, out);
            flag_producer_calls_in_super_body(then_branch, candidates, out);
            if let Some(eb) = else_branch {
                flag_producer_calls_in_super_body(eb, candidates, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            flag_producer_calls_in_expr(condition, candidates, out);
            flag_producer_calls_in_super_body(body, candidates, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                flag_producer_calls_in_stmt(i, candidates, out);
            }
            if let Some(c) = condition {
                flag_producer_calls_in_expr(c, candidates, out);
            }
            if let Some(u) = update {
                flag_producer_calls_in_expr(u, candidates, out);
            }
            flag_producer_calls_in_super_body(body, candidates, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            flag_producer_calls_in_super_body(body, candidates, out);
            if let Some(c) = catch {
                flag_producer_calls_in_super_body(&c.body, candidates, out);
            }
            if let Some(f) = finally {
                flag_producer_calls_in_super_body(f, candidates, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            flag_producer_calls_in_expr(discriminant, candidates, out);
            for c in cases {
                if let Some(t) = &c.test {
                    flag_producer_calls_in_expr(t, candidates, out);
                }
                flag_producer_calls_in_super_body(&c.body, candidates, out);
            }
        }
        Stmt::Labeled { body, .. } => flag_producer_calls_in_stmt(body, candidates, out),
        _ => {}
    }
}

fn flag_producer_calls_in_expr(
    e: &Expr,
    candidates: &HashMap<FuncId, ProducerInfo>,
    out: &mut HashSet<FuncId>,
) {
    match e {
        Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } => {
            if let Expr::FuncRef(id) = callee.as_ref() {
                if candidates.contains_key(id) {
                    out.insert(*id);
                }
            }
        }
        _ => {}
    }
    walk_expr_children(e, &mut |child| {
        flag_producer_calls_in_expr(child, candidates, out)
    });
}

/// Recursive: returns true if `stmt` (or any nested stmt inside it)
/// is a `Stmt::Return`. Used by the producer analyzer to gate on
/// "single top-level return" — if any deeper stmt is also a return,
/// the function has multiple control-flow exits and the rewrite is
/// unsafe.
pub fn stmt_contains_return(stmt: &Stmt) -> bool {
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
