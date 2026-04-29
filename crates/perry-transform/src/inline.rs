//! Function and Method Inlining Pass for Perry HIR
//!
//! This module inlines small functions and methods at their call sites to eliminate
//! call overhead and enable further optimizations.

use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{BinaryOp, Expr, Function, Module, Stmt};
use perry_types::{FuncId, LocalId, Type};
use std::collections::{HashMap, HashSet};

/// Maximum number of statements for a function to be considered for inlining
const MAX_INLINE_STMTS: usize = 10;

/// Information about a method that can be inlined
#[derive(Clone)]
struct MethodCandidate {
    func: Function,
    /// The index of the `this` parameter (if present)
    this_param_id: Option<LocalId>,
}

/// Inline small functions and methods in the module
pub fn inline_functions(module: &mut Module) {
    // Phases 0 + 1 fused (Tier 4.1, v0.5.335): single iteration over
    // module.functions collects both Math.imul polyfill ids AND
    // inlinable-function candidates. Pre-Tier-4 these were two separate
    // `module.functions.iter()` passes back-to-back. Math.imul detection
    // and `is_inlinable` are independent reads with no ordering
    // dependency, so fusing is safe and saves one full module scan.
    let mut imul_polyfill_ids: HashSet<FuncId> = HashSet::new();
    let mut func_candidates: HashMap<FuncId, Function> = HashMap::new();
    for f in module.functions.iter() {
        if detect_math_imul_polyfill(f) {
            imul_polyfill_ids.insert(f.id);
        }
        if is_inlinable(f) {
            func_candidates.insert(f.id, f.clone());
        }
    }

    // Phase 0 mutation pass: rewrite imul call sites in every body.
    // Must run BEFORE the inliner expands those calls, so the polyfill
    // body is never decomposed into 5+ operations — the codegen emits a
    // single `mul i32` instead. Conditional on at least one polyfill
    // being detected so we don't traverse for nothing.
    if !imul_polyfill_ids.is_empty() {
        rewrite_imul_calls_in_stmts(&mut module.init, &imul_polyfill_ids);
        for func in &mut module.functions {
            if !imul_polyfill_ids.contains(&func.id) {
                rewrite_imul_calls_in_stmts(&mut func.body, &imul_polyfill_ids);
            }
        }
        for class in &mut module.classes {
            if let Some(ref mut ctor) = class.constructor {
                rewrite_imul_calls_in_stmts(&mut ctor.body, &imul_polyfill_ids);
            }
            for method in &mut class.methods {
                rewrite_imul_calls_in_stmts(&mut method.body, &imul_polyfill_ids);
            }
        }
    }

    // Phases 2 + 3 fused (Tier 4.1): single iteration over
    // module.classes builds both the inlinable-method map AND the
    // class-name lookup. class_names is unconditional (covers every
    // class regardless of native_extends), so it lives at the top of
    // the loop body before the native_extends short-circuit for method
    // collection.
    let mut method_candidates: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let mut class_names: HashMap<String, String> = HashMap::new();
    for class in &module.classes {
        class_names.insert(class.name.clone(), class.name.clone());

        // Don't inline methods from classes with native parents (e.g.,
        // EventEmitter) — the `this` reference needs special handling
        // in those contexts. The class_name lookup above still records
        // the type so other passes can reference it.
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if is_inlinable(method) {
                // Methods don't have 'this' as a parameter in the HIR;
                // they access it via Expr::This. So this_param_id is
                // None.
                method_candidates.insert(
                    (class.name.clone(), method.name.clone()),
                    MethodCandidate {
                        func: method.clone(),
                        this_param_id: None,
                    },
                );
            }
        }
    }

    // Compute a MODULE-WIDE max LocalId used as the starting point for all
    // inliner-allocated local IDs. CRITICAL: LocalIds are globally unique across
    // the whole module (HIR lowering uses a single `fresh_local` counter), so any
    // newly allocated ID must exceed the max used ANYWHERE in the module — not
    // just in the current scope (init / function body / method body). Otherwise
    // the inliner can produce a module-level Let whose id collides with a class
    // method's parameter id, and the subsequent module_var_data_ids loader in
    // codegen silently skips loading the global (because `locals.contains_key(id)`
    // is already true for the method parameter), leaving the method reading the
    // wrong value from the class field.
    let module_max_id = find_max_local_id_in_module(module);

    // Phase 4: Inline calls in init statements.
    // Method calls are always safe (they access `this.field` via pointer indirection).
    // Standalone functions are safe ONLY if they are "pure" — i.e. they don't read or
    // write module-level variables. Module-level variables are cached in locals during
    // compile_init, so an inlined function that reads a module variable modified by a
    // prior call would see the stale cached value. Pure functions (which only use their
    // own parameters and body locals) avoid this problem entirely.
    {
        let pure_func_candidates: HashMap<FuncId, Function> = func_candidates
            .iter()
            .filter(|(_, f)| is_pure_function(f))
            .map(|(id, f)| (*id, f.clone()))
            .collect();
        let mut next_local_id = module_max_id + 1;
        let mut local_types: HashMap<LocalId, String> = HashMap::new();
        inline_calls_in_stmts(
            &mut module.init,
            &pure_func_candidates,
            &method_candidates,
            &class_names,
            &mut local_types,
            &mut next_local_id,
        );
    }

    // Phase 5: Inline calls in function bodies
    //
    // Each function body now uses a private ID counter that starts after the
    // module-wide max AND any IDs previously allocated by the init-phase inliner.
    // We maintain a running `next_module_id` so each phase advances the shared
    // counter, preventing collisions between phases.
    let mut next_module_id = module_max_id + 1;
    // Advance past any IDs consumed by the init phase by re-scanning the module.
    next_module_id = next_module_id.max(find_max_local_id_in_module(module) + 1);
    for func in &mut module.functions {
        if func_candidates.contains_key(&func.id) {
            continue;
        }
        let mut local_id = next_module_id;
        let mut local_types: HashMap<LocalId, String> = HashMap::new();
        // Add function parameters to local_types
        for param in &func.params {
            if let Type::Named(class_name) = &param.ty {
                local_types.insert(param.id, class_name.clone());
            }
        }
        inline_calls_in_stmts(
            &mut func.body,
            &func_candidates,
            &method_candidates,
            &class_names,
            &mut local_types,
            &mut local_id,
        );
        next_module_id = local_id;
    }

    // Phase 6: Inline calls in class method bodies
    for class in &mut module.classes {
        for method in &mut class.methods {
            // Skip if this method is itself a candidate (avoid recursion)
            if method_candidates.contains_key(&(class.name.clone(), method.name.clone())) {
                continue;
            }
            let mut local_id = next_module_id;
            let mut local_types: HashMap<LocalId, String> = HashMap::new();
            for param in &method.params {
                if let Type::Named(class_name) = &param.ty {
                    local_types.insert(param.id, class_name.clone());
                }
            }
            inline_calls_in_stmts(
                &mut method.body,
                &func_candidates,
                &method_candidates,
                &class_names,
                &mut local_types,
                &mut local_id,
            );
            next_module_id = local_id;
        }
    }
}

/// Find the maximum LocalId used ANYWHERE in the module: init statements,
/// function bodies, class constructors, class method bodies, class field
/// initializers, and closure bodies nested inside any of the above. Used to
/// compute a safe starting point for inliner-allocated local IDs so they don't
/// collide with existing HIR ids anywhere in the module.
fn find_max_local_id_in_module(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    max_id = max_id.max(find_max_local_id(&module.init));
    for func in &module.functions {
        for param in &func.params {
            max_id = max_id.max(param.id);
        }
        max_id = max_id.max(find_max_local_id(&func.body));
    }
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            for param in &ctor.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&ctor.body));
        }
        for method in &class.methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&method.body));
        }
        for (_, getter) in &class.getters {
            for param in &getter.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&getter.body));
        }
        for (_, setter) in &class.setters {
            for param in &setter.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&setter.body));
        }
        for method in &class.static_methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            max_id = max_id.max(find_max_local_id(&method.body));
        }
    }
    max_id
}

/// Check if a function is suitable for inlining
fn is_inlinable(func: &Function) -> bool {
    // Don't inline async functions
    if func.is_async {
        return false;
    }

    // Don't inline functions with captures (closures)
    if !func.captures.is_empty() {
        return false;
    }

    // Don't inline functions with rest parameters. The current call-site
    // arg-handling maps each formal param to one actual arg via param_map;
    // a rest param needs the trailing args bundled into a synthetic
    // `Expr::Array(...)` setup_stmt, which the inliner does not emit.
    // Without that, only the first trailing arg ends up bound to the
    // rest param (as a scalar), and the body's `parts.length` /
    // `parts[i]` / `parts.join(...)` then operate on whatever scalar
    // value happened to be passed — strings get treated as
    // single-element arrays, numbers as raw doubles, etc.
    if func.params.iter().any(|p| p.is_rest) {
        return false;
    }

    // Don't inline functions that are too large
    if func.body.len() > MAX_INLINE_STMTS {
        return false;
    }

    // Check for simple patterns
    if !has_simple_control_flow(&func.body) {
        return false;
    }

    // Don't inline functions that return closures capturing parameters
    // When inlined, the parameter IDs won't exist in the outer context
    let param_ids: std::collections::HashSet<LocalId> = func.params.iter().map(|p| p.id).collect();
    if body_contains_closure_capturing(&func.body, &param_ids) {
        return false;
    }

    // Don't inline methods containing super.method() or super() calls.
    // These rely on the enclosing class context (ThisContext with parent_class)
    // which is lost once the body is inlined into the caller.
    if body_contains_super_call(&func.body) {
        return false;
    }

    true
}

/// Check if a body contains Expr::SuperCall or Expr::SuperMethodCall (recursively).
fn body_contains_super_call(stmts: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        match expr {
            Expr::SuperCall(_) | Expr::SuperMethodCall { .. } => true,
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => check_expr(left) || check_expr(right),
            Expr::Unary { operand, .. } => check_expr(operand),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => check_expr(condition) || check_expr(then_expr) || check_expr(else_expr),
            Expr::Call { callee, args, .. } => {
                check_expr(callee) || args.iter().any(|a| check_expr(a))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e)),
            Expr::IndexGet { object, index } => check_expr(object) || check_expr(index),
            Expr::IndexSet {
                object,
                index,
                value,
            } => check_expr(object) || check_expr(index) || check_expr(value),
            Expr::PropertyGet { object, .. } => check_expr(object),
            Expr::PropertySet { object, value, .. } => check_expr(object) || check_expr(value),
            Expr::LocalSet(_, value) => check_expr(value),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => check_expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    || then_branch.iter().any(check_stmt)
                    || else_branch
                        .as_ref()
                        .map_or(false, |b| b.iter().any(check_stmt))
            }
            Stmt::While { condition, body } => check_expr(condition) || body.iter().any(check_stmt),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(false, |i| check_stmt(i))
                    || condition.as_ref().map_or(false, |c| check_expr(c))
                    || update.as_ref().map_or(false, |u| check_expr(u))
                    || body.iter().any(check_stmt)
            }
            _ => false,
        }
    }

    stmts.iter().any(check_stmt)
}

/// Check if statements contain a closure that captures any of the given local IDs
fn body_contains_closure_capturing(
    stmts: &[Stmt],
    captured_ids: &std::collections::HashSet<LocalId>,
) -> bool {
    fn check_expr(expr: &Expr, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match expr {
            Expr::Closure { captures, body, .. } => {
                // Check if any capture is in the set of IDs we're looking for
                for capture_id in captures {
                    if captured_ids.contains(capture_id) {
                        return true;
                    }
                }
                // Also check the closure body for nested closures
                body_contains_closure_capturing(body, captured_ids)
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                check_expr(left, captured_ids) || check_expr(right, captured_ids)
            }
            Expr::Unary { operand, .. } => check_expr(operand, captured_ids),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                check_expr(condition, captured_ids)
                    || check_expr(then_expr, captured_ids)
                    || check_expr(else_expr, captured_ids)
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee, captured_ids) || args.iter().any(|a| check_expr(a, captured_ids))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, captured_ids)),
            Expr::IndexGet { object, index } => {
                check_expr(object, captured_ids) || check_expr(index, captured_ids)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                check_expr(object, captured_ids)
                    || check_expr(index, captured_ids)
                    || check_expr(value, captured_ids)
            }
            Expr::PropertyGet { object, .. } => check_expr(object, captured_ids),
            Expr::PropertySet { object, value, .. } => {
                check_expr(object, captured_ids) || check_expr(value, captured_ids)
            }
            Expr::LocalSet(_, value) => check_expr(value, captured_ids),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt, captured_ids: &std::collections::HashSet<LocalId>) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr, captured_ids),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, captured_ids)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, captured_ids)
                    || then_branch.iter().any(|s| check_stmt(s, captured_ids))
                    || else_branch
                        .as_ref()
                        .map_or(false, |b| b.iter().any(|s| check_stmt(s, captured_ids)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, captured_ids)
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(false, |i| check_stmt(i, captured_ids))
                    || condition
                        .as_ref()
                        .map_or(false, |c| check_expr(c, captured_ids))
                    || update
                        .as_ref()
                        .map_or(false, |u| check_expr(u, captured_ids))
                    || body.iter().any(|s| check_stmt(s, captured_ids))
            }
            _ => false,
        }
    }

    stmts.iter().any(|s| check_stmt(s, captured_ids))
}

/// Check if a function is "pure" for init-inlining purposes: its body only
/// references its own parameters and locally-declared variables.  No GlobalGet,
/// GlobalSet, ExternFuncRef, or NativeMethodCall.  This makes it safe to inline
/// into module init context where module-level variables are cached in locals.
fn is_pure_function(func: &Function) -> bool {
    let mut known_ids: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    for p in &func.params {
        known_ids.insert(p.id);
    }
    // Collect all Let-declared IDs in the body
    let body_ids = collect_body_local_ids(&func.body);
    for id in body_ids {
        known_ids.insert(id);
    }

    fn expr_is_pure(e: &Expr, known: &std::collections::HashSet<LocalId>) -> bool {
        match e {
            Expr::GlobalGet(_) | Expr::GlobalSet(_, _) => false,
            Expr::ExternFuncRef { .. } => false,
            Expr::NativeMethodCall { .. } => false,
            Expr::LocalGet(id) | Expr::Update { id, .. } => known.contains(id),
            Expr::LocalSet(id, val) => known.contains(id) && expr_is_pure(val, known),
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                expr_is_pure(left, known) && expr_is_pure(right, known)
            }
            Expr::Unary { operand, .. } => expr_is_pure(operand, known),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                expr_is_pure(condition, known)
                    && expr_is_pure(then_expr, known)
                    && expr_is_pure(else_expr, known)
            }
            Expr::Call { callee, args, .. } => {
                expr_is_pure(callee, known) && args.iter().all(|a| expr_is_pure(a, known))
            }
            Expr::Array(elems) => elems.iter().all(|e| expr_is_pure(e, known)),
            Expr::IndexGet { object, index } => {
                expr_is_pure(object, known) && expr_is_pure(index, known)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                expr_is_pure(object, known)
                    && expr_is_pure(index, known)
                    && expr_is_pure(value, known)
            }
            Expr::PropertyGet { object, .. } => expr_is_pure(object, known),
            Expr::PropertySet { object, value, .. } => {
                expr_is_pure(object, known) && expr_is_pure(value, known)
            }
            // Leaf expressions with no variable references are always pure
            Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::FuncRef(_)
            | Expr::This => true,
            // For anything else we haven't explicitly handled, be conservative
            _ => true,
        }
    }

    fn stmt_is_pure(s: &Stmt, known: &std::collections::HashSet<LocalId>) -> bool {
        match s {
            Stmt::Let { init: Some(e), .. } => expr_is_pure(e, known),
            Stmt::Let { init: None, .. } => true,
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => expr_is_pure(e, known),
            Stmt::Return(None) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_is_pure(condition, known)
                    && then_branch.iter().all(|s| stmt_is_pure(s, known))
                    && else_branch
                        .as_ref()
                        .map_or(true, |b| b.iter().all(|s| stmt_is_pure(s, known)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_is_pure(condition, known) && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(true, |i| stmt_is_pure(i, known))
                    && condition.as_ref().map_or(true, |c| expr_is_pure(c, known))
                    && update.as_ref().map_or(true, |u| expr_is_pure(u, known))
                    && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            _ => false, // conservative: reject Switch, Try, etc.
        }
    }

    func.body.iter().all(|s| stmt_is_pure(s, &known_ids))
}

/// Check if statements have simple control flow suitable for inlining
fn has_simple_control_flow(stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Return(_) => {}
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !has_simple_control_flow(then_branch) {
                    return false;
                }
                if let Some(else_b) = else_branch {
                    if !has_simple_control_flow(else_b) {
                        return false;
                    }
                }
            }
            Stmt::While { .. }
            | Stmt::DoWhile { .. }
            | Stmt::For { .. }
            | Stmt::Try { .. }
            | Stmt::Switch { .. }
            | Stmt::Labeled { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::Throw(_) => {
                return false;
            }
        }
    }
    true
}

/// Find the maximum local ID used in statements
fn find_max_local_id(stmts: &[Stmt]) -> LocalId {
    let mut max_id: LocalId = 0;

    // Track every LocalId encountered. Per-variant handling for the LocalId
    // fields owned directly by an Expr; descent into sub-expressions is
    // delegated to `walk_expr_children` (single source of truth — see
    // `perry_hir::walker` for why). Pre-refactor this fn carried its own
    // ad-hoc walker with a `_ => {}` catch-all, which silently undercounted
    // any new LocalId-bearing variant (issues #167, #169, #214).
    fn check_expr(expr: &Expr, max_id: &mut LocalId) {
        match expr {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::Update { id, .. } => {
                *max_id = (*max_id).max(*id);
            }
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                *max_id = (*max_id).max(*array_id);
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::SetAdd { set_id, .. } => {
                *max_id = (*max_id).max(*set_id);
            }
            Expr::Closure {
                params,
                body,
                captures,
                mutable_captures,
                ..
            } => {
                // Closure has THREE LocalId sources: params, captures,
                // mutable_captures. The body's nested LocalGets contribute via
                // check_stmt. Param defaults need check_expr too. Short-circuit
                // (`return`) so the walker below doesn't double-descend into
                // Param defaults.
                for param in params {
                    *max_id = (*max_id).max(param.id);
                    if let Some(d) = &param.default {
                        check_expr(d, max_id);
                    }
                }
                for id in captures {
                    *max_id = (*max_id).max(*id);
                }
                for id in mutable_captures {
                    *max_id = (*max_id).max(*id);
                }
                for stmt in body {
                    check_stmt(stmt, max_id);
                }
                return;
            }
            _ => {}
        }
        // Descend into all immediate sub-expressions. Exhaustive on Expr —
        // a new variant added to ir.rs without updating walker.rs is a
        // compile error.
        walk_expr_children(expr, &mut |child| check_expr(child, max_id));
    }

    fn check_stmt(stmt: &Stmt, max_id: &mut LocalId) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                *max_id = (*max_id).max(*id);
                if let Some(expr) = init {
                    check_expr(expr, max_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, max_id);
            }
            Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, max_id);
                for s in then_branch {
                    check_stmt(s, max_id);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::While { condition, body } => {
                check_expr(condition, max_id);
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::DoWhile { body, condition } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                check_expr(condition, max_id);
            }
            Stmt::Labeled { body, .. } => {
                check_stmt(body, max_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    check_stmt(i, max_id);
                }
                if let Some(c) = condition {
                    check_expr(c, max_id);
                }
                if let Some(u) = update {
                    check_expr(u, max_id);
                }
                for s in body {
                    check_stmt(s, max_id);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    check_stmt(s, max_id);
                }
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        *max_id = (*max_id).max(*id);
                    }
                    for s in &c.body {
                        check_stmt(s, max_id);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, max_id);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_expr(test, max_id);
                    }
                    for s in &case.body {
                        check_stmt(s, max_id);
                    }
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        }
    }

    for stmt in stmts {
        check_stmt(stmt, &mut max_id);
    }

    max_id
}

/// Inline function and method calls in a list of statements
fn inline_calls_in_stmts(
    stmts: &mut Vec<Stmt>,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    class_names: &HashMap<String, String>,
    local_types: &mut HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
) {
    let mut i = 0;
    while i < stmts.len() {
        // Track local variable types from Let statements
        if let Stmt::Let { id, ty, init, .. } = &stmts[i] {
            if let Type::Named(class_name) = ty {
                local_types.insert(*id, class_name.clone());
            }
            // Also check if init is a New expression
            if let Some(Expr::New { class_name, .. }) = init {
                local_types.insert(*id, class_name.clone());
            }
        }

        let mut new_stmts: Option<Vec<Stmt>> = None;

        match &mut stmts[i] {
            Stmt::Expr(expr) => {
                if let Some((mut inlined_stmts, _result_expr)) = try_inline_call(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ) {
                    // When inlining into Stmt::Expr context (result discarded),
                    // convert Stmt::Return(Some(expr)) to Stmt::Expr(expr) and
                    // remove Stmt::Return(None). This prevents emitting a
                    // `ret` terminator mid-block (e.g., inside a for loop body).
                    // Only do this if returns are in safe positions (trailing).
                    let has_nested_return = inlined_stmts
                        .iter()
                        .take(inlined_stmts.len().saturating_sub(1))
                        .any(|s| {
                            fn stmt_has_return(s: &Stmt) -> bool {
                                match s {
                                    Stmt::Return(_) => true,
                                    Stmt::If {
                                        then_branch,
                                        else_branch,
                                        ..
                                    } => {
                                        then_branch.iter().any(stmt_has_return)
                                            || else_branch
                                                .as_ref()
                                                .map_or(false, |eb| eb.iter().any(stmt_has_return))
                                    }
                                    _ => false,
                                }
                            }
                            stmt_has_return(s)
                        });
                    if has_nested_return {
                        // Can't safely convert early returns; skip inlining
                        let hoisted = inline_calls_in_expr(
                            expr,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                        );
                        if !hoisted.is_empty() {
                            new_stmts = Some(hoisted);
                        }
                    } else {
                        // Convert trailing return to expression (discard result)
                        if let Some(last) = inlined_stmts.last_mut() {
                            match last {
                                Stmt::Return(Some(ret_expr)) => {
                                    let e = std::mem::replace(ret_expr, Expr::Undefined);
                                    *last = Stmt::Expr(e);
                                }
                                Stmt::Return(None) => {
                                    inlined_stmts.pop();
                                }
                                _ => {}
                            }
                        }
                        new_stmts = Some(inlined_stmts);
                    }
                } else {
                    let hoisted = inline_calls_in_expr(
                        expr,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                    );
                    if !hoisted.is_empty() {
                        // Hoisted stmts from multi-stmt inlining inside expressions
                        // (e.g., `h = imul32(h, p)` → Let setup stmts + modified expr)
                        // Splice them before the current statement, keeping the stmt itself.
                        let current = stmts.remove(i);
                        let hoisted_len = hoisted.len();
                        for (j, s) in hoisted.into_iter().enumerate() {
                            stmts.insert(i + j, s);
                        }
                        stmts.insert(i + hoisted_len, current);
                        i += hoisted_len + 1;
                        continue;
                    }
                }
            }
            Stmt::Let {
                init: Some(expr), ..
            } => {
                let hoisted = inline_calls_in_expr(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                );
                if !hoisted.is_empty() {
                    let current = stmts.remove(i);
                    let hoisted_len = hoisted.len();
                    for (j, s) in hoisted.into_iter().enumerate() {
                        stmts.insert(i + j, s);
                    }
                    stmts.insert(i + hoisted_len, current);
                    i += hoisted_len + 1;
                    continue;
                }
            }
            Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                let hoisted = inline_calls_in_expr(
                    expr,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                );
                if !hoisted.is_empty() {
                    let current = stmts.remove(i);
                    let hoisted_len = hoisted.len();
                    for (j, s) in hoisted.into_iter().enumerate() {
                        stmts.insert(i + j, s);
                    }
                    stmts.insert(i + hoisted_len, current);
                    i += hoisted_len + 1;
                    continue;
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let _hoisted = inline_calls_in_expr(
                    condition,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                );
                // Note: hoisting from conditions is rare and complex; skip for now
                inline_calls_in_stmts(
                    then_branch,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                );
                if let Some(else_b) = else_branch {
                    inline_calls_in_stmts(
                        else_b,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        next_local_id,
                    );
                }
            }
            Stmt::While { condition, body } => {
                let _hoisted = inline_calls_in_expr(
                    condition,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                );
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_stmts = vec![*init_stmt.clone()];
                    inline_calls_in_stmts(
                        &mut init_stmts,
                        func_candidates,
                        method_candidates,
                        class_names,
                        local_types,
                        next_local_id,
                    );
                    if init_stmts.len() == 1 {
                        **init_stmt = init_stmts.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    let _hoisted = inline_calls_in_expr(
                        cond,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                    );
                }
                if let Some(upd) = update {
                    let _hoisted = inline_calls_in_expr(
                        upd,
                        func_candidates,
                        method_candidates,
                        local_types,
                        next_local_id,
                    );
                }
                inline_calls_in_stmts(
                    body,
                    func_candidates,
                    method_candidates,
                    class_names,
                    local_types,
                    next_local_id,
                );
            }
            _ => {}
        }

        if let Some(mut inlined) = new_stmts {
            stmts.remove(i);
            let inlined_len = inlined.len();
            for (j, stmt) in inlined.drain(..).enumerate() {
                stmts.insert(i + j, stmt);
            }
            i += inlined_len.max(1);
        } else {
            i += 1;
        }
    }
}

/// Inline function and method calls in an expression.
/// Returns setup statements that must be spliced before the enclosing statement.
fn inline_calls_in_expr(
    expr: &mut Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
) -> Vec<Stmt> {
    // First try to inline this expression if it's a call
    if let Some((stmts, mut result)) = try_inline_simple_call(
        expr,
        func_candidates,
        method_candidates,
        local_types,
        next_local_id,
    ) {
        let inner = inline_calls_in_expr(
            &mut result,
            func_candidates,
            method_candidates,
            local_types,
            next_local_id,
        );
        *expr = result;
        let mut all = stmts;
        all.extend(inner);
        return all;
    }

    // Otherwise recurse into sub-expressions, collecting hoisted stmts
    let mut hoisted = Vec::new();
    match expr {
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            hoisted.extend(inline_calls_in_expr(
                left,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                right,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Unary { operand, .. } => {
            hoisted.extend(inline_calls_in_expr(
                operand,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            hoisted.extend(inline_calls_in_expr(
                condition,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                then_expr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                else_expr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Call { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            for arg in args {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                hoisted.extend(inline_calls_in_expr(
                    elem,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                hoisted.extend(inline_calls_in_expr(
                    v,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                        ));
                    }
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(
                callee,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(
                            e,
                            func_candidates,
                            method_candidates,
                            local_types,
                            next_local_id,
                        ));
                    }
                }
            }
        }
        Expr::IndexGet { object, index } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::PropertyGet { object, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::PropertySet { object, value, .. } => {
            hoisted.extend(inline_calls_in_expr(
                object,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::LocalSet(_, value) => {
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                hoisted.extend(inline_calls_in_expr(
                    obj,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
            for arg in args {
                hoisted.extend(inline_calls_in_expr(
                    arg,
                    func_candidates,
                    method_candidates,
                    local_types,
                    next_local_id,
                ));
            }
        }
        // Issue #169: a Call nested inside a Uint8Array index/set/length
        // (e.g. `buf[clamp(i)]`) wouldn't be inlined without these arms.
        Expr::Uint8ArrayGet { array, index } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            hoisted.extend(inline_calls_in_expr(
                array,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                index,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
            hoisted.extend(inline_calls_in_expr(
                value,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Uint8ArrayLength(arr) => {
            hoisted.extend(inline_calls_in_expr(
                arr,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        Expr::Uint8ArrayNew(Some(arg)) => {
            hoisted.extend(inline_calls_in_expr(
                arg,
                func_candidates,
                method_candidates,
                local_types,
                next_local_id,
            ));
        }
        _ => {}
    }
    hoisted
}

/// Try to inline a simple function or method call.
/// Handles two patterns:
/// 1. Single `Return(expr)` body — classic expression-level inline
/// 2. `[Let*, Return(expr)]` body — setup stmts + result expression
fn try_inline_simple_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
) -> Option<(Vec<Stmt>, Expr)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Check for regular function call
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                // Pattern 1: single Return(expr)
                if func.body.len() == 1 {
                    if let Stmt::Return(Some(return_expr)) = &func.body[0] {
                        let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                        for (param, arg) in func.params.iter().zip(args.iter()) {
                            param_map.insert(param.id, arg.clone());
                        }
                        let mut result = return_expr.clone();
                        substitute_locals(&mut result, &param_map, next_local_id);
                        return Some((vec![], result));
                    }
                }

                // Pattern 2: [Let (const)*, Return(expr)] — e.g. imul32 polyfill
                // All statements except the last must be immutable Let declarations,
                // and the last must be Return(Some(expr)).
                if func.body.len() > 1 {
                    let last = func.body.last().unwrap();
                    if let Stmt::Return(Some(return_expr)) = last {
                        let all_lets = func.body[..func.body.len() - 1].iter().all(|s| {
                            matches!(
                                s,
                                Stmt::Let {
                                    mutable: false,
                                    init: Some(_),
                                    ..
                                }
                            )
                        });
                        if all_lets {
                            // Build param substitution map
                            let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                            for (param, arg) in func.params.iter().zip(args.iter()) {
                                if is_trivial_expr(arg) {
                                    param_map.insert(param.id, arg.clone());
                                } else {
                                    let fresh = *next_local_id;
                                    *next_local_id += 1;
                                    param_map.insert(param.id, Expr::LocalGet(fresh));
                                    // We'll create the Let for this fresh id below
                                }
                            }

                            // Remap body-local IDs
                            let body_ids = collect_body_local_ids(&func.body);
                            for old_id in &body_ids {
                                if !param_map.contains_key(old_id) {
                                    let fresh = *next_local_id;
                                    *next_local_id += 1;
                                    param_map.insert(*old_id, Expr::LocalGet(fresh));
                                }
                            }

                            // Build setup stmts: param Lets (for non-trivial args) + body Lets
                            let mut setup: Vec<Stmt> = Vec::new();

                            // First, add Lets for non-trivial param args
                            for (param, arg) in func.params.iter().zip(args.iter()) {
                                if !is_trivial_expr(arg) {
                                    if let Some(Expr::LocalGet(fresh_id)) = param_map.get(&param.id)
                                    {
                                        setup.push(Stmt::Let {
                                            id: *fresh_id,
                                            name: param.name.clone(),
                                            ty: param.ty.clone(),
                                            mutable: false,
                                            init: Some(arg.clone()),
                                        });
                                    }
                                }
                            }

                            // Then clone the body Let stmts with substituted inits
                            for stmt in &func.body[..func.body.len() - 1] {
                                if let Stmt::Let {
                                    id,
                                    name,
                                    ty,
                                    mutable,
                                    init: Some(init_expr),
                                } = stmt
                                {
                                    let new_id =
                                        if let Some(Expr::LocalGet(fresh)) = param_map.get(id) {
                                            *fresh
                                        } else {
                                            *id
                                        };
                                    let mut new_init = init_expr.clone();
                                    substitute_locals(&mut new_init, &param_map, next_local_id);
                                    setup.push(Stmt::Let {
                                        id: new_id,
                                        name: name.clone(),
                                        ty: ty.clone(),
                                        mutable: *mutable,
                                        init: Some(new_init),
                                    });
                                }
                            }

                            // Build result expression from the Return
                            let mut result = return_expr.clone();
                            substitute_locals(&mut result, &param_map, next_local_id);

                            return Some((setup, result));
                        }
                    }
                }
            }
        }

        // Check for method call: callee is PropertyGet { object: LocalGet(id), property: method_name }
        if let Expr::PropertyGet {
            object,
            property: method_name,
        } = callee.as_ref()
        {
            if let Expr::LocalGet(obj_id) = object.as_ref() {
                // Look up the class type of this local variable
                if let Some(class_name) = local_types.get(obj_id) {
                    // Look up the method candidate
                    if let Some(method_candidate) =
                        method_candidates.get(&(class_name.clone(), method_name.clone()))
                    {
                        // Check for single return statement
                        if method_candidate.func.body.len() == 1 {
                            if let Stmt::Return(Some(return_expr)) = &method_candidate.func.body[0]
                            {
                                let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                                // Map 'this' parameter to the receiver object
                                if let Some(this_id) = method_candidate.this_param_id {
                                    param_map.insert(this_id, Expr::LocalGet(*obj_id));
                                }

                                // Map parameters to arguments
                                // Note: Method params don't include 'this' - they use Expr::This instead
                                for (param, arg) in
                                    method_candidate.func.params.iter().zip(args.iter())
                                {
                                    param_map.insert(param.id, arg.clone());
                                }

                                let mut result = return_expr.clone();
                                substitute_locals(&mut result, &param_map, next_local_id);

                                // Also substitute Expr::This with the receiver
                                substitute_this(&mut result, *obj_id);

                                return Some((vec![], result));
                            }
                        }

                        // Handle void methods (no return or empty return)
                        if method_candidate.func.body.len() <= 2 {
                            let mut is_void_method = true;
                            let mut inlined_stmts = Vec::new();

                            for stmt in &method_candidate.func.body {
                                match stmt {
                                    Stmt::Return(None) => {}
                                    Stmt::Expr(e) => {
                                        let mut param_map: HashMap<LocalId, Expr> = HashMap::new();
                                        if let Some(this_id) = method_candidate.this_param_id {
                                            param_map.insert(this_id, Expr::LocalGet(*obj_id));
                                        }
                                        // Note: Method params don't include 'this' - they use Expr::This instead
                                        for (param, arg) in
                                            method_candidate.func.params.iter().zip(args.iter())
                                        {
                                            param_map.insert(param.id, arg.clone());
                                        }
                                        let mut expr = e.clone();
                                        substitute_locals(&mut expr, &param_map, next_local_id);
                                        substitute_this(&mut expr, *obj_id);
                                        inlined_stmts.push(Stmt::Expr(expr));
                                    }
                                    _ => {
                                        is_void_method = false;
                                        break;
                                    }
                                }
                            }

                            if is_void_method && !inlined_stmts.is_empty() {
                                return Some((inlined_stmts, Expr::Undefined));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Try to inline a call that may have multiple statements
fn try_inline_call(
    expr: &Expr,
    func_candidates: &HashMap<FuncId, Function>,
    method_candidates: &HashMap<(String, String), MethodCandidate>,
    local_types: &HashMap<LocalId, String>,
    next_local_id: &mut LocalId,
) -> Option<(Vec<Stmt>, Option<Expr>)> {
    if let Expr::Call { callee, args, .. } = expr {
        // Handle regular function calls
        if let Expr::FuncRef(func_id) = callee.as_ref() {
            if let Some(func) = func_candidates.get(func_id) {
                let mut setup_stmts: Vec<Stmt> = Vec::new();
                let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                for (param, arg) in func.params.iter().zip(args.iter()) {
                    if is_trivial_expr(arg) {
                        param_map.insert(param.id, arg.clone());
                    } else {
                        let local_id = *next_local_id;
                        *next_local_id += 1;

                        setup_stmts.push(Stmt::Let {
                            id: local_id,
                            name: param.name.clone(),
                            ty: param.ty.clone(),
                            mutable: false,
                            init: Some(arg.clone()),
                        });

                        param_map.insert(param.id, Expr::LocalGet(local_id));
                    }
                }

                let mut inlined_body = func.body.clone();

                // Collect all LocalIds from Let statements in the body and remap them
                let body_local_ids = collect_body_local_ids(&inlined_body);
                for old_id in body_local_ids {
                    if !param_map.contains_key(&old_id) {
                        let new_id = *next_local_id;
                        *next_local_id += 1;
                        param_map.insert(old_id, Expr::LocalGet(new_id));
                    }
                }

                substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);

                setup_stmts.extend(inlined_body);

                return Some((setup_stmts, None));
            }
        }

        // Handle method calls
        if let Expr::PropertyGet {
            object,
            property: method_name,
        } = callee.as_ref()
        {
            if let Expr::LocalGet(obj_id) = object.as_ref() {
                if let Some(class_name) = local_types.get(obj_id) {
                    if let Some(method_candidate) =
                        method_candidates.get(&(class_name.clone(), method_name.clone()))
                    {
                        let mut setup_stmts: Vec<Stmt> = Vec::new();
                        let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                        // Map 'this' parameter to the receiver object (if present as a param)
                        if let Some(this_id) = method_candidate.this_param_id {
                            param_map.insert(this_id, Expr::LocalGet(*obj_id));
                        }

                        // Map parameters to arguments
                        // Note: Method params don't include 'this' - they use Expr::This instead
                        for (param, arg) in method_candidate.func.params.iter().zip(args.iter()) {
                            if is_trivial_expr(arg) {
                                param_map.insert(param.id, arg.clone());
                            } else {
                                let local_id = *next_local_id;
                                *next_local_id += 1;

                                setup_stmts.push(Stmt::Let {
                                    id: local_id,
                                    name: param.name.clone(),
                                    ty: param.ty.clone(),
                                    mutable: false,
                                    init: Some(arg.clone()),
                                });

                                param_map.insert(param.id, Expr::LocalGet(local_id));
                            }
                        }

                        // Clone and substitute the method body
                        let mut inlined_body = method_candidate.func.body.clone();

                        // Collect all LocalIds from Let statements in the body and remap them
                        let body_local_ids = collect_body_local_ids(&inlined_body);
                        for old_id in body_local_ids {
                            if !param_map.contains_key(&old_id) {
                                let new_id = *next_local_id;
                                *next_local_id += 1;
                                param_map.insert(old_id, Expr::LocalGet(new_id));
                            }
                        }

                        substitute_locals_in_stmts(&mut inlined_body, &param_map, next_local_id);
                        substitute_this_in_stmts(&mut inlined_body, *obj_id);

                        setup_stmts.extend(inlined_body);

                        return Some((setup_stmts, None));
                    }
                }
            }
        }
    }
    None
}

/// Check if an expression is trivial (safe to duplicate)
fn is_trivial_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
    )
}

/// Substitute local variable references in an expression
/// Replace inlined parameters' LocalGets with the actual call-site argument
/// expressions, and remap LocalIds carried by other variants when the param
/// map says so.
///
/// Per-variant work focuses on the LocalId-bearing variants (LocalGet itself
/// is the substitution target; LocalSet / Update / Array*.array_id / SetAdd /
/// Closure.captures need id-only remapping). Descent into all other
/// sub-expressions is delegated to `walk_expr_children_mut` — the central
/// exhaustive walker in `perry_hir::walker`. Pre-refactor this fn carried its
/// own ad-hoc walker with a `_ => {}` catch-all that silently dropped any new
/// variant added to `Expr` (issues #169, #214).
fn substitute_locals(
    expr: &mut Expr,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    match expr {
        Expr::LocalGet(id) => {
            if let Some(replacement) = param_map.get(id) {
                *expr = replacement.clone();
            }
            return;
        }
        Expr::LocalSet(id, value) => {
            substitute_locals(value, param_map, next_local_id);
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::Update { id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
            return;
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(array_id) {
                *array_id = *new_id;
            }
            // Children (`value`, `start`, `delete_count`, `items`, `target`,
            // `end`, …) are descended into below via the walker.
        }
        Expr::SetAdd { set_id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(set_id) {
                *set_id = *new_id;
            }
            // `value` descended via walker.
        }
        // Closure: substitute in body AND remap captures lists. Without
        // remapping captures, an inlined function whose body contains a
        // closure ends up with the closure's captures list referencing the
        // OLD local IDs while the closure body uses the NEW (remapped) IDs.
        // Codegen then can't resolve the captures in the inlined-into FnCtx
        // and falls back to `double_literal(0.0)`, producing null box
        // pointers at runtime (closure-null family). Param defaults also get
        // substituted explicitly here so the walker doesn't double-process
        // them.
        Expr::Closure {
            body,
            captures,
            mutable_captures,
            params,
            ..
        } => {
            for p in params.iter_mut() {
                if let Some(d) = &mut p.default {
                    substitute_locals(d, param_map, next_local_id);
                }
            }
            substitute_locals_in_stmts(body, param_map, next_local_id);
            captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                // Trivial expr inlined directly; closure body no longer
                // references this id, so drop the now-orphan capture.
                Some(_) => false,
                // Not in param_map → outer/module-level; leave unchanged.
                None => true,
            });
            mutable_captures.retain_mut(|id| match param_map.get(id) {
                Some(Expr::LocalGet(new_id)) => {
                    *id = *new_id;
                    true
                }
                Some(_) => false,
                None => true,
            });
            return;
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions for non-special variants.
    // The walker is exhaustive on Expr — adding a new variant to ir.rs
    // without updating walker.rs is a compile error.
    walk_expr_children_mut(expr, &mut |child| {
        substitute_locals(child, param_map, next_local_id)
    });
}

/// Substitute Expr::This with a LocalGet reference
fn substitute_this(expr: &mut Expr, obj_id: LocalId) {
    match expr {
        Expr::This => {
            *expr = Expr::LocalGet(obj_id);
        }
        Expr::PropertyGet { object, .. } => {
            substitute_this(object, obj_id);
        }
        Expr::PropertySet { object, value, .. } => {
            substitute_this(object, obj_id);
            substitute_this(value, obj_id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            substitute_this(left, obj_id);
            substitute_this(right, obj_id);
        }
        Expr::Unary { operand, .. } => {
            substitute_this(operand, obj_id);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            substitute_this(condition, obj_id);
            substitute_this(then_expr, obj_id);
            substitute_this(else_expr, obj_id);
        }
        Expr::Call { callee, args, .. } => {
            substitute_this(callee, obj_id);
            for arg in args {
                substitute_this(arg, obj_id);
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                substitute_this(elem, obj_id);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        substitute_this(e, obj_id);
                    }
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            substitute_this(callee, obj_id);
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        substitute_this(e, obj_id);
                    }
                }
            }
        }
        Expr::IndexGet { object, index } => {
            substitute_this(object, obj_id);
            substitute_this(index, obj_id);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            substitute_this(object, obj_id);
            substitute_this(index, obj_id);
            substitute_this(value, obj_id);
        }
        Expr::LocalSet(_, value) => {
            substitute_this(value, obj_id);
        }
        Expr::TypeOf(inner) => {
            substitute_this(inner, obj_id);
        }
        Expr::Void(inner) => {
            substitute_this(inner, obj_id);
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                substitute_this(v, obj_id);
            }
        }
        Expr::New { args, .. } => {
            for arg in args {
                substitute_this(arg, obj_id);
            }
        }
        Expr::NewDynamic { callee, args } => {
            substitute_this(callee, obj_id);
            for arg in args {
                substitute_this(arg, obj_id);
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                substitute_this(v, obj_id);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                substitute_this(v, obj_id);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                substitute_this(obj, obj_id);
            }
            for arg in args {
                substitute_this(arg, obj_id);
            }
        }
        // Math operations
        Expr::MathFloor(inner)
        | Expr::MathCeil(inner)
        | Expr::MathRound(inner)
        | Expr::MathAbs(inner)
        | Expr::MathSqrt(inner)
        | Expr::MathLog(inner)
        | Expr::MathLog2(inner)
        | Expr::MathLog10(inner) => {
            substitute_this(inner, obj_id);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            substitute_this(base, obj_id);
            substitute_this(exp, obj_id);
        }
        Expr::MathMin(exprs) | Expr::MathMax(exprs) => {
            for e in exprs {
                substitute_this(e, obj_id);
            }
        }
        Expr::MathMinSpread(inner) | Expr::MathMaxSpread(inner) => {
            substitute_this(inner, obj_id);
        }
        // Array operations that may contain This references
        Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
            substitute_this(array, obj_id);
            substitute_this(value, obj_id);
        }
        Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } => {
            substitute_this(value, obj_id);
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        } => {
            substitute_this(array, obj_id);
            substitute_this(callback, obj_id);
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
            substitute_this(array, obj_id);
            substitute_this(callback, obj_id);
            if let Some(init) = initial {
                substitute_this(init, obj_id);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            substitute_this(array, obj_id);
            substitute_this(start, obj_id);
            if let Some(e) = end {
                substitute_this(e, obj_id);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            substitute_this(array, obj_id);
            if let Some(sep) = separator {
                substitute_this(sep, obj_id);
            }
        }
        Expr::ArrayFlat { array } | Expr::ArrayFrom(array) | Expr::ArrayToReversed { array } => {
            substitute_this(array, obj_id);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            substitute_this(array, obj_id);
        }
        Expr::ArrayToSorted { array, comparator } => {
            substitute_this(array, obj_id);
            if let Some(cmp) = comparator {
                substitute_this(cmp, obj_id);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            substitute_this(array, obj_id);
            substitute_this(start, obj_id);
            substitute_this(delete_count, obj_id);
            for item in items {
                substitute_this(item, obj_id);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            substitute_this(array, obj_id);
            substitute_this(index, obj_id);
            substitute_this(value, obj_id);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            substitute_this(target, obj_id);
            substitute_this(start, obj_id);
            if let Some(e) = end {
                substitute_this(e, obj_id);
            }
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            substitute_this(iterable, obj_id);
            substitute_this(map_fn, obj_id);
        }
        Expr::ArraySplice {
            start,
            delete_count,
            items,
            ..
        } => {
            substitute_this(start, obj_id);
            if let Some(dc) = delete_count {
                substitute_this(dc, obj_id);
            }
            for item in items {
                substitute_this(item, obj_id);
            }
        }
        Expr::StringSplit(s, sep) => {
            substitute_this(s, obj_id);
            substitute_this(sep, obj_id);
        }
        Expr::Await(inner) => {
            substitute_this(inner, obj_id);
        }
        // Issue #291: when inlining a method body, nested closures that
        // captured `this` from the outer method's frame need their own
        // `Expr::This` → `LocalGet(obj_id)` rewrite — after inlining the
        // closure is hoisted into the call site's frame (module init for
        // top-level calls, where `this_stack` is empty), so the codegen-
        // side fallback can't recover a meaningful `this`. Substituting
        // here lets the closure run with the correct receiver.
        //
        // Also: explicitly add `obj_id` to the closure's captures list
        // and clear `captures_this` — the body now reads `LocalGet(obj_id)`
        // rather than `Expr::This`, and `compute_auto_captures` blends
        // explicit + body-scanned ids before excluding module globals,
        // so adding to `captures` ensures the receiver is forwarded
        // through the closure's capture array regardless of where the
        // call site lands.
        Expr::Closure {
            body,
            captures,
            captures_this,
            ..
        } => {
            substitute_this_in_stmts(body, obj_id);
            *captures_this = false;
            if !captures.contains(&obj_id) {
                captures.push(obj_id);
            }
        }
        _ => {}
    }
}

/// Substitute Expr::This with a LocalGet reference in statements
fn substitute_this_in_stmts(stmts: &mut Vec<Stmt>, obj_id: LocalId) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => {
                substitute_this(expr, obj_id);
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_this(expr, obj_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(then_branch, obj_id);
                if let Some(else_b) = else_branch {
                    substitute_this_in_stmts(else_b, obj_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_this(condition, obj_id);
                substitute_this_in_stmts(body, obj_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_this_in_stmts(&mut init_vec, obj_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_this(cond, obj_id);
                }
                if let Some(upd) = update {
                    substitute_this(upd, obj_id);
                }
                substitute_this_in_stmts(body, obj_id);
            }
            _ => {}
        }
    }
}

/// Substitute local variable references in statements
/// Collect all LocalIds defined by Let statements in a body (for remapping during inlining)
fn collect_body_local_ids(stmts: &[Stmt]) -> Vec<LocalId> {
    let mut ids = Vec::new();

    fn collect_from_stmt(stmt: &Stmt, ids: &mut Vec<LocalId>) {
        match stmt {
            Stmt::Let { id, .. } => {
                ids.push(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    collect_from_stmt(s, ids);
                }
                if let Some(else_b) = else_branch {
                    for s in else_b {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::While { body, .. } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_from_stmt(init_stmt, ids);
                }
                for s in body {
                    collect_from_stmt(s, ids);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    collect_from_stmt(s, ids);
                }
                if let Some(catch_clause) = catch {
                    // Also collect the catch parameter if present
                    if let Some((param_id, _)) = &catch_clause.param {
                        ids.push(*param_id);
                    }
                    for s in &catch_clause.body {
                        collect_from_stmt(s, ids);
                    }
                }
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for s in &case.body {
                        collect_from_stmt(s, ids);
                    }
                }
            }
            _ => {}
        }
    }

    for stmt in stmts {
        collect_from_stmt(stmt, &mut ids);
    }
    ids
}

fn substitute_locals_in_stmts(
    stmts: &mut Vec<Stmt>,
    param_map: &HashMap<LocalId, Expr>,
    next_local_id: &mut LocalId,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let { id, init, .. } => {
                // Remap the Let's id if it's in the param_map
                if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                    *id = *new_id;
                }
                if let Some(expr) = init {
                    substitute_locals(expr, param_map, next_local_id);
                }
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_locals(expr, param_map, next_local_id);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(then_branch, param_map, next_local_id);
                if let Some(else_b) = else_branch {
                    substitute_locals_in_stmts(else_b, param_map, next_local_id);
                }
            }
            Stmt::While { condition, body } => {
                substitute_locals(condition, param_map, next_local_id);
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    let mut init_vec = vec![*init_stmt.clone()];
                    substitute_locals_in_stmts(&mut init_vec, param_map, next_local_id);
                    if init_vec.len() == 1 {
                        **init_stmt = init_vec.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    substitute_locals(cond, param_map, next_local_id);
                }
                if let Some(upd) = update {
                    substitute_locals(upd, param_map, next_local_id);
                }
                substitute_locals_in_stmts(body, param_map, next_local_id);
            }
            _ => {}
        }
    }
}

// ── Math.imul polyfill detection ──────────────────────────────────────────

/// Detect whether a function is a Math.imul polyfill.
/// Matches the canonical pattern: 2 params, 4 half-word extraction Lets,
/// final Return with recombined multiply `| 0`.
fn detect_math_imul_polyfill(f: &Function) -> bool {
    if f.is_async || f.is_generator {
        return false;
    }
    if f.params.len() != 2 {
        return false;
    }
    if f.body.len() != 5 {
        return false;
    }

    let p0 = f.params[0].id;
    let p1 = f.params[1].id;

    // First 4 stmts must be immutable Lets with half-word extraction inits
    let mut hi_of = [false; 2]; // hi_of[0] = saw hi-half of p0, hi_of[1] = p1
    let mut lo_of = [false; 2];
    for stmt in &f.body[..4] {
        match stmt {
            Stmt::Let {
                mutable: false,
                init: Some(init),
                ..
            } => {
                if let Some((pid, is_hi)) = is_half_extract(init, p0, p1) {
                    let idx = if pid == p0 { 0 } else { 1 };
                    if is_hi {
                        hi_of[idx] = true;
                    } else {
                        lo_of[idx] = true;
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !(hi_of[0] && lo_of[0] && hi_of[1] && lo_of[1]) {
        return false;
    }

    // Last stmt: Return(Some(Binary { BitOr, ..., Integer(0) }))
    matches!(&f.body[4], Stmt::Return(Some(Expr::Binary { op: BinaryOp::BitOr, right, .. })) if matches!(right.as_ref(), Expr::Integer(0)))
}

/// Check if an expression extracts the hi or lo 16-bit half of a parameter.
/// Returns `Some((param_id, is_hi))` on match.
fn is_half_extract(e: &Expr, p0: LocalId, p1: LocalId) -> Option<(LocalId, bool)> {
    // Pattern: (param >>> 16) & 0xffff  OR  (param >> 16) & 0xffff  →  hi-half
    // Pattern: param & 0xffff  →  lo-half
    match e {
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        } => {
            if !matches!(right.as_ref(), Expr::Integer(0xffff)) {
                return None;
            }
            match left.as_ref() {
                Expr::Binary {
                    op: BinaryOp::UShr | BinaryOp::Shr,
                    left: inner,
                    right: shift_amt,
                } => {
                    if !matches!(shift_amt.as_ref(), Expr::Integer(16)) {
                        return None;
                    }
                    match inner.as_ref() {
                        Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, true)),
                        _ => None,
                    }
                }
                Expr::LocalGet(id) if *id == p0 || *id == p1 => Some((*id, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Rewrite `Call(FuncRef(imul_id), [a, b])` → `MathImul(a, b)` in statements.
fn rewrite_imul_calls_in_stmts(stmts: &mut [Stmt], imul_ids: &HashSet<FuncId>) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::Let { init: Some(e), .. } => {
                rewrite_imul_calls_in_expr(e, imul_ids);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(then_branch, imul_ids);
                if let Some(eb) = else_branch {
                    rewrite_imul_calls_in_stmts(eb, imul_ids);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                rewrite_imul_calls_in_expr(condition, imul_ids);
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    rewrite_imul_calls_in_stmts(std::slice::from_mut(init_stmt), imul_ids);
                }
                if let Some(c) = condition {
                    rewrite_imul_calls_in_expr(c, imul_ids);
                }
                if let Some(u) = update {
                    rewrite_imul_calls_in_expr(u, imul_ids);
                }
                rewrite_imul_calls_in_stmts(body, imul_ids);
            }
            _ => {}
        }
    }
}

fn rewrite_imul_calls_in_expr(e: &mut Expr, imul_ids: &HashSet<FuncId>) {
    // Check if this expr is a call to an imul polyfill
    let is_imul = matches!(e, Expr::Call { callee, args, .. }
        if args.len() == 2 && matches!(callee.as_ref(), Expr::FuncRef(fid) if imul_ids.contains(fid)));
    if is_imul {
        if let Expr::Call { args, .. } = std::mem::replace(e, Expr::Undefined) {
            let mut args = args;
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            *e = Expr::MathImul(Box::new(a), Box::new(b));
        }
        // Recurse into the new MathImul operands
        if let Expr::MathImul(a, b) = e {
            rewrite_imul_calls_in_expr(a, imul_ids);
            rewrite_imul_calls_in_expr(b, imul_ids);
        }
        return;
    }

    // Recurse into sub-expressions
    match e {
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            rewrite_imul_calls_in_expr(left, imul_ids);
            rewrite_imul_calls_in_expr(right, imul_ids);
        }
        Expr::Unary { operand, .. } => rewrite_imul_calls_in_expr(operand, imul_ids),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_imul_calls_in_expr(condition, imul_ids);
            rewrite_imul_calls_in_expr(then_expr, imul_ids);
            rewrite_imul_calls_in_expr(else_expr, imul_ids);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_imul_calls_in_expr(callee, imul_ids);
            for arg in args {
                rewrite_imul_calls_in_expr(arg, imul_ids);
            }
        }
        Expr::LocalSet(_, val) => rewrite_imul_calls_in_expr(val, imul_ids),
        Expr::IndexGet { object, index } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        Expr::Array(elems) => {
            for el in elems {
                rewrite_imul_calls_in_expr(el, imul_ids);
            }
        }
        Expr::PropertyGet { object, .. } => rewrite_imul_calls_in_expr(object, imul_ids),
        Expr::PropertySet { object, value, .. } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        _ => {}
    }
}
