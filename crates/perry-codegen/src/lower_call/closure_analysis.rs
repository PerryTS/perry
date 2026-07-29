//! Closure-body local-set / outer-write analysis helpers used by the
//! perry/thread thread-safety check inside `lower_native_method_call`.
//!
//! Besides the original outer-write check, this module hosts the #6185
//! Tier-1 containment walk: a worker closure must not do async work
//! (`await` / async closures — the emitted await loop would drain the
//! process-global completion and timer queues on the worker thread), must
//! not spawn nested thread primitives, and must not read module-scope
//! bindings whose values are heap objects (module globals are process-wide
//! slots read in place, bypassing the capture deep-copy, so a worker would
//! alias the main thread's heap with no synchronization).

/// Walk a statement to collect LocalIds declared inside a closure body —
/// `Stmt::Let` and `Stmt::For` init `let`s. Used by the perry/thread
/// thread-safety check to distinguish inner locals (safe to write) from
/// captures (unsafe). Recurses into nested control-flow but deliberately
/// NOT into nested closures: those have their own inner-id set.
pub fn collect_closure_introduced_ids(
    stmt: &perry_hir::Stmt,
    out: &mut std::collections::HashSet<perry_hir::types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_ref() {
                collect_closure_introduced_ids(init_stmt, out);
            }
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(cc) = catch {
                if let Some((id, _)) = &cc.param {
                    out.insert(*id);
                }
                for s in &cc.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_introduced_ids(body, out),
        _ => {} // Expr, Return, Throw, Break, Continue, LabeledBreak/Continue — don't declare locals
    }
}

/// Walk a statement looking for LocalSet / Update whose target LocalId is
/// NOT in `inner_ids` — i.e. the closure is writing to a captured or
/// module-level variable. Does NOT recurse into nested Closure expressions
/// (those are a separate scope with their own check when they're passed to
/// a threading primitive).
pub fn find_outer_writes_stmt(
    stmt: &perry_hir::Stmt,
    inner_ids: &std::collections::HashSet<perry_hir::types::LocalId>,
    out: &mut Vec<perry_hir::types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                find_outer_writes_expr(expr, inner_ids, out);
            }
        }
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            find_outer_writes_expr(e, inner_ids, out);
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in then_branch {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::While { condition, body } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::DoWhile { condition, body } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            find_outer_writes_expr(condition, inner_ids, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init.as_ref() {
                find_outer_writes_stmt(init_stmt, inner_ids, out);
            }
            if let Some(c) = condition {
                find_outer_writes_expr(c, inner_ids, out);
            }
            if let Some(u) = update {
                find_outer_writes_expr(u, inner_ids, out);
            }
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            find_outer_writes_expr(discriminant, inner_ids, out);
            for case in cases {
                if let Some(val) = &case.test {
                    find_outer_writes_expr(val, inner_ids, out);
                }
                for s in &case.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => find_outer_writes_stmt(body, inner_ids, out),
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn find_outer_writes_expr(
    expr: &perry_hir::Expr,
    inner_ids: &std::collections::HashSet<perry_hir::types::LocalId>,
    out: &mut Vec<perry_hir::types::LocalId>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, val) => {
            if !inner_ids.contains(id) {
                out.push(*id);
            }
            find_outer_writes_expr(val, inner_ids, out);
        }
        Expr::Update { id, .. } if !inner_ids.contains(id) => {
            out.push(*id);
        }
        Expr::Closure { .. } => {
            // Stop at nested closure boundary — it has its own scope and
            // will be checked separately if it's the one being passed to
            // a threading primitive.
        }
        Expr::Binary { left, right, .. } => {
            find_outer_writes_expr(left, inner_ids, out);
            find_outer_writes_expr(right, inner_ids, out);
        }
        Expr::Call { callee, args, .. } => {
            find_outer_writes_expr(callee, inner_ids, out);
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                find_outer_writes_expr(o, inner_ids, out);
            }
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            find_outer_writes_expr(object, inner_ids, out);
        }
        Expr::IndexGet { object, index } => {
            find_outer_writes_expr(object, inner_ids, out);
            find_outer_writes_expr(index, inner_ids, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                find_outer_writes_expr(e, inner_ids, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            find_outer_writes_expr(then_expr, inner_ids, out);
            find_outer_writes_expr(else_expr, inner_ids, out);
        }
        _ => {} // Literals, LocalGet, GlobalGet, etc. — no writes
    }
}

/// A thread-unsafe construct found inside a `perry/thread` worker closure
/// body by `find_thread_hazard_in_body` (#6185 Tier-1 containment).
pub enum ThreadClosureHazard {
    /// The worker closure (or a closure nested inside it) is async: either
    /// `is_async` is still set (awaitless async closure) or its FuncId is in
    /// `Module::async_step_closures` (await-containing closure CPS-rewritten
    /// into a state machine before codegen). Every function containing
    /// `await` has the global await/microtask pump emitted into it, so an
    /// async closure executing on a worker drains other threads' completions
    /// and timers and resolves foreign-heap promises.
    AsyncClosure,
    /// A stray `Expr::Await` / `Expr::ForAwaitToArray` that survived to
    /// codegen inside the worker body (belt-and-braces — the async transform
    /// normally rewrites these away together with their enclosing closure).
    Await,
    /// A nested `spawn` / `parallelMap` / `parallelFilter` call inside the
    /// worker body. Worker-side spawning pumps `PENDING_THREAD_RESULTS` on
    /// whatever thread runs the await loop and can steal another thread's
    /// completion into the wrong arena.
    NestedThreadCall(String),
    /// A read or write of a module-scope binding whose declared/inferred
    /// type is not a thread-transferable primitive. Module globals live in
    /// process-wide slots and are read in place — they do NOT go through
    /// the capture deep-copy — so the worker aliases main-heap objects.
    ModuleGlobalAccess(perry_hir::types::LocalId),
}

/// Types whose module-global slot value can be read from a worker thread
/// without aliasing mutable main-heap structure: numbers and booleans are
/// plain 64-bit copies; strings/bigints are immutable and permanently
/// rooted when they back a module global. `Any` / `Unknown` / type vars are
/// allowed because this is a best-effort AST-level check — rejecting
/// unprovable bindings would flag every untyped numeric global.
fn is_thread_transferable_global_type(ty: &perry_hir::types::Type) -> bool {
    use perry_hir::types::Type;
    match ty {
        Type::Void
        | Type::Null
        | Type::Boolean
        | Type::Number
        | Type::Int32
        | Type::BigInt
        | Type::String
        | Type::StringLiteral(_)
        | Type::Any
        | Type::Unknown
        | Type::Never
        | Type::TypeVar(_) => true,
        Type::Union(members) => members.iter().all(is_thread_transferable_global_type),
        // SharedArrayBuffer is the one EXPLICIT shared-state escape hatch in
        // the threading model: its backing store is a process-global,
        // never-freed allocation (`crate::shared_sab` in perry-runtime)
        // designed to alias the same physical bytes across agents, and the
        // shipped Atomics cross-thread tests read a top-level SAB binding
        // from worker closures. Keep that sanctioned pattern compiling.
        Type::Named(name) => name == "SharedArrayBuffer",
        // Symbol (arena-allocated identity), Array, Tuple, Object, Function,
        // Promise, class/interface instances, Generic (Map/Set/...): heap
        // structure on the spawning thread's arena.
        _ => false,
    }
}

/// Compute the set of module-global LocalIds a worker closure must not
/// touch: ids with a backing `@perry_global_*` slot whose declared/inferred
/// type is not a thread-transferable primitive. Ids with no recorded type
/// are allowed (best-effort — e.g. non-entry module inits don't seed
/// `local_types`, and `module_global_types` skips `Any`).
pub fn hazardous_module_global_ids(
    module_globals: &std::collections::HashMap<u32, String>,
    local_types: &std::collections::HashMap<u32, perry_hir::types::Type>,
) -> std::collections::HashSet<perry_hir::types::LocalId> {
    module_globals
        .keys()
        .filter(|id| {
            local_types
                .get(id)
                .is_some_and(|ty| !is_thread_transferable_global_type(ty))
        })
        .copied()
        .collect()
}

/// Walk a `perry/thread` worker closure body looking for the #6185 Tier-1
/// hazards (see `ThreadClosureHazard`). Unlike `find_outer_writes_stmt`,
/// this DOES recurse into nested closures: anything defined inside the
/// worker body executes on the worker thread when invoked. Returns the
/// first hazard found.
///
/// Best-effort by design: an AST walk can't see through a named callee
/// defined outside the worker body (a top-level `function` that awaits or
/// reads module globals still gets through). The runtime serializer guards
/// (#6188/#6212) remain the backstop for what this walk can't prove.
pub fn find_thread_hazard_in_body(
    body: &[perry_hir::Stmt],
    hazardous_ids: &std::collections::HashSet<perry_hir::types::LocalId>,
    async_step_closures: &std::collections::HashSet<u32>,
) -> Option<ThreadClosureHazard> {
    body.iter()
        .find_map(|s| find_thread_hazard_stmt(s, hazardous_ids, async_step_closures))
}

fn find_thread_hazard_stmt(
    stmt: &perry_hir::Stmt,
    hazardous_ids: &std::collections::HashSet<perry_hir::types::LocalId>,
    async_step_closures: &std::collections::HashSet<u32>,
) -> Option<ThreadClosureHazard> {
    use perry_hir::Stmt;
    let expr = |e| find_thread_hazard_expr(e, hazardous_ids, async_step_closures);
    let stmts = |b: &[Stmt]| find_thread_hazard_in_body(b, hazardous_ids, async_step_closures);
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().and_then(expr),
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => expr(e),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => None,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => expr(condition)
            .or_else(|| stmts(then_branch))
            .or_else(|| else_branch.as_deref().and_then(stmts)),
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            expr(condition).or_else(|| stmts(body))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => init
            .as_deref()
            .and_then(|s| find_thread_hazard_stmt(s, hazardous_ids, async_step_closures))
            .or_else(|| condition.as_ref().and_then(expr))
            .or_else(|| update.as_ref().and_then(expr))
            .or_else(|| stmts(body)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => stmts(body)
            .or_else(|| catch.as_ref().and_then(|cc| stmts(&cc.body)))
            .or_else(|| finally.as_deref().and_then(stmts)),
        Stmt::Switch {
            discriminant,
            cases,
        } => expr(discriminant).or_else(|| {
            cases.iter().find_map(|case| {
                case.test
                    .as_ref()
                    .and_then(expr)
                    .or_else(|| stmts(&case.body))
            })
        }),
        Stmt::Labeled { body, .. } => {
            find_thread_hazard_stmt(body, hazardous_ids, async_step_closures)
        }
    }
}

fn find_thread_hazard_expr(
    e: &perry_hir::Expr,
    hazardous_ids: &std::collections::HashSet<perry_hir::types::LocalId>,
    async_step_closures: &std::collections::HashSet<u32>,
) -> Option<ThreadClosureHazard> {
    use perry_hir::Expr;
    match e {
        Expr::Await(_) | Expr::ForAwaitToArray(_) => return Some(ThreadClosureHazard::Await),
        Expr::Closure {
            func_id,
            is_async,
            params,
            body,
            ..
        } => {
            // A closure defined inside the worker body executes on the
            // worker thread when invoked — same hazard surface as the
            // worker body itself.
            if *is_async || async_step_closures.contains(func_id) {
                return Some(ThreadClosureHazard::AsyncClosure);
            }
            return params
                .iter()
                .filter_map(|p| p.default.as_ref())
                .find_map(|d| find_thread_hazard_expr(d, hazardous_ids, async_step_closures))
                .or_else(|| find_thread_hazard_in_body(body, hazardous_ids, async_step_closures));
        }
        Expr::NativeMethodCall { module, method, .. }
            if module == "perry/thread"
                && matches!(method.as_str(), "spawn" | "parallelMap" | "parallelFilter") =>
        {
            return Some(ThreadClosureHazard::NestedThreadCall(method.clone()));
        }
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. }
            if hazardous_ids.contains(id) =>
        {
            return Some(ThreadClosureHazard::ModuleGlobalAccess(*id));
        }
        // In-place mutation fast-path variants carry their target as a bare
        // LocalId (same set `collect_local_refs_expr` special-cases).
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. }
            if hazardous_ids.contains(array_id) =>
        {
            return Some(ThreadClosureHazard::ModuleGlobalAccess(*array_id));
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id)
            if hazardous_ids.contains(array_id) =>
        {
            return Some(ThreadClosureHazard::ModuleGlobalAccess(*array_id));
        }
        Expr::SetAdd { set_id, .. } if hazardous_ids.contains(set_id) => {
            return Some(ThreadClosureHazard::ModuleGlobalAccess(*set_id));
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions via the canonical walker
    // (exhaustive on Expr, so new variants can't silently skip the check).
    let mut found = None;
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        if found.is_none() {
            found = find_thread_hazard_expr(child, hazardous_ids, async_step_closures);
        }
    });
    found
}
