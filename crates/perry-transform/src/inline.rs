//! Function and Method Inlining Pass for Perry HIR
//!
//! This module inlines small functions and methods at their call sites to eliminate
//! call overhead and enable further optimizations.

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
    // Phase 0: Detect Math.imul polyfill functions and replace their call sites
    // with Expr::MathImul(a, b). This runs BEFORE inlining so the polyfill body
    // is never decomposed into 5+ operations — the codegen emits a single `mul i32`.
    let imul_polyfill_ids: HashSet<FuncId> = module.functions.iter()
        .filter(|f| detect_math_imul_polyfill(f))
        .map(|f| f.id)
        .collect();
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

    // Phase 1: Identify inlinable functions
    let func_candidates: HashMap<FuncId, Function> = module.functions.iter()
        .filter(|f| is_inlinable(f))
        .map(|f| (f.id, f.clone()))
        .collect();

    // Phase 2: Identify inlinable methods (class_name, method_name) -> MethodCandidate
    let mut method_candidates: HashMap<(String, String), MethodCandidate> = HashMap::new();
    for class in &module.classes {
        // Don't inline methods from classes with native parents (e.g., EventEmitter)
        // because the `this` reference needs special handling in those contexts
        if class.native_extends.is_some() {
            continue;
        }

        for method in &class.methods {
            if is_inlinable(method) {
                // Note: Methods don't have 'this' as a parameter in the HIR.
                // They access 'this' via Expr::This. So this_param_id is None.
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

    // Phase 3: Build class name lookup for types
    let class_names: HashMap<String, String> = module.classes.iter()
        .map(|c| (c.name.clone(), c.name.clone()))
        .collect();

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
        let pure_func_candidates: HashMap<FuncId, Function> = func_candidates.iter()
            .filter(|(_, f)| is_pure_function(f))
            .map(|(id, f)| (*id, f.clone()))
            .collect();
        let mut next_local_id = module_max_id + 1;
        let mut local_types: HashMap<LocalId, String> = HashMap::new();
        inline_calls_in_stmts(&mut module.init, &pure_func_candidates, &method_candidates, &class_names, &mut local_types, &mut next_local_id);
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
        inline_calls_in_stmts(&mut func.body, &func_candidates, &method_candidates, &class_names, &mut local_types, &mut local_id);
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
            inline_calls_in_stmts(&mut method.body, &func_candidates, &method_candidates, &class_names, &mut local_types, &mut local_id);
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
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
            Expr::Compare { left, right, .. } => {
                check_expr(left) || check_expr(right)
            }
            Expr::Unary { operand, .. } => check_expr(operand),
            Expr::Conditional { condition, then_expr, else_expr } => {
                check_expr(condition) || check_expr(then_expr) || check_expr(else_expr)
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee) || args.iter().any(|a| check_expr(a))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e)),
            Expr::IndexGet { object, index } => check_expr(object) || check_expr(index),
            Expr::IndexSet { object, index, value } => {
                check_expr(object) || check_expr(index) || check_expr(value)
            }
            Expr::PropertyGet { object, .. } => check_expr(object),
            Expr::PropertySet { object, value, .. } => check_expr(object) || check_expr(value),
            Expr::LocalSet(_, value) => check_expr(value),
            _ => false,
        }
    }

    fn check_stmt(stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let { init: Some(expr), .. } => check_expr(expr),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => check_expr(expr),
            Stmt::If { condition, then_branch, else_branch } => {
                check_expr(condition)
                    || then_branch.iter().any(check_stmt)
                    || else_branch.as_ref().map_or(false, |b| b.iter().any(check_stmt))
            }
            Stmt::While { condition, body } => {
                check_expr(condition) || body.iter().any(check_stmt)
            }
            Stmt::For { init, condition, update, body } => {
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
fn body_contains_closure_capturing(stmts: &[Stmt], captured_ids: &std::collections::HashSet<LocalId>) -> bool {
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
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
            Expr::Compare { left, right, .. } => {
                check_expr(left, captured_ids) || check_expr(right, captured_ids)
            }
            Expr::Unary { operand, .. } => check_expr(operand, captured_ids),
            Expr::Conditional { condition, then_expr, else_expr } => {
                check_expr(condition, captured_ids) ||
                check_expr(then_expr, captured_ids) ||
                check_expr(else_expr, captured_ids)
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee, captured_ids) ||
                args.iter().any(|a| check_expr(a, captured_ids))
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, captured_ids)),
            Expr::IndexGet { object, index } => {
                check_expr(object, captured_ids) || check_expr(index, captured_ids)
            }
            Expr::IndexSet { object, index, value } => {
                check_expr(object, captured_ids) ||
                check_expr(index, captured_ids) ||
                check_expr(value, captured_ids)
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
            Stmt::Let { init: Some(expr), .. } => check_expr(expr, captured_ids),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, captured_ids)
            }
            Stmt::If { condition, then_branch, else_branch } => {
                check_expr(condition, captured_ids) ||
                then_branch.iter().any(|s| check_stmt(s, captured_ids)) ||
                else_branch.as_ref().map_or(false, |b| b.iter().any(|s| check_stmt(s, captured_ids)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, captured_ids) ||
                body.iter().any(|s| check_stmt(s, captured_ids))
            }
            Stmt::For { init, condition, update, body } => {
                init.as_ref().map_or(false, |i| check_stmt(i, captured_ids)) ||
                condition.as_ref().map_or(false, |c| check_expr(c, captured_ids)) ||
                update.as_ref().map_or(false, |u| check_expr(u, captured_ids)) ||
                body.iter().any(|s| check_stmt(s, captured_ids))
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
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                expr_is_pure(left, known) && expr_is_pure(right, known)
            }
            Expr::Unary { operand, .. } => expr_is_pure(operand, known),
            Expr::Conditional { condition, then_expr, else_expr } => {
                expr_is_pure(condition, known) && expr_is_pure(then_expr, known) && expr_is_pure(else_expr, known)
            }
            Expr::Call { callee, args, .. } => {
                expr_is_pure(callee, known) && args.iter().all(|a| expr_is_pure(a, known))
            }
            Expr::Array(elems) => elems.iter().all(|e| expr_is_pure(e, known)),
            Expr::IndexGet { object, index } => expr_is_pure(object, known) && expr_is_pure(index, known),
            Expr::IndexSet { object, index, value } => {
                expr_is_pure(object, known) && expr_is_pure(index, known) && expr_is_pure(value, known)
            }
            Expr::PropertyGet { object, .. } => expr_is_pure(object, known),
            Expr::PropertySet { object, value, .. } => expr_is_pure(object, known) && expr_is_pure(value, known),
            // Leaf expressions with no variable references are always pure
            Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) | Expr::String(_)
            | Expr::Null | Expr::Undefined | Expr::FuncRef(_) | Expr::This => true,
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
            Stmt::If { condition, then_branch, else_branch } => {
                expr_is_pure(condition, known)
                    && then_branch.iter().all(|s| stmt_is_pure(s, known))
                    && else_branch.as_ref().map_or(true, |b| b.iter().all(|s| stmt_is_pure(s, known)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_is_pure(condition, known) && body.iter().all(|s| stmt_is_pure(s, known))
            }
            Stmt::For { init, condition, update, body } => {
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
            Stmt::If { then_branch, else_branch, .. } => {
                if !has_simple_control_flow(then_branch) {
                    return false;
                }
                if let Some(else_b) = else_branch {
                    if !has_simple_control_flow(else_b) {
                        return false;
                    }
                }
            }
            Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } | Stmt::Try { .. } |
            Stmt::Switch { .. } | Stmt::Labeled { .. } |
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) |
            Stmt::Throw(_) => {
                return false;
            }
        }
    }
    true
}

/// Find the maximum local ID used in statements
fn find_max_local_id(stmts: &[Stmt]) -> LocalId {
    let mut max_id: LocalId = 0;

    fn check_expr(expr: &Expr, max_id: &mut LocalId) {
        match expr {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) => {
                *max_id = (*max_id).max(*id);
            }
            Expr::Update { id, .. } => {
                *max_id = (*max_id).max(*id);
            }
            Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
            Expr::Compare { left, right, .. } => {
                check_expr(left, max_id);
                check_expr(right, max_id);
            }
            Expr::Unary { operand, .. } => {
                check_expr(operand, max_id);
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                check_expr(condition, max_id);
                check_expr(then_expr, max_id);
                check_expr(else_expr, max_id);
            }
            Expr::Call { callee, args, .. } => {
                check_expr(callee, max_id);
                for arg in args {
                    check_expr(arg, max_id);
                }
            }
            Expr::CallSpread { callee, args, .. } => {
                check_expr(callee, max_id);
                for arg in args {
                    match arg {
                        perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                            check_expr(e, max_id);
                        }
                    }
                }
            }
            Expr::Array(elements) => {
                for elem in elements {
                    check_expr(elem, max_id);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                            check_expr(e, max_id);
                        }
                    }
                }
            }
            Expr::Object(fields) => {
                for (_, v) in fields {
                    check_expr(v, max_id);
                }
            }
            Expr::ObjectSpread { parts } => {
                for (_, v) in parts {
                    check_expr(v, max_id);
                }
            }
            Expr::IndexGet { object, index } | Expr::IndexSet { object, index, .. } => {
                check_expr(object, max_id);
                check_expr(index, max_id);
            }
            Expr::PropertyGet { object, .. } | Expr::PropertySet { object, .. } => {
                check_expr(object, max_id);
            }
            Expr::NativeMethodCall { object, args, .. } => {
                if let Some(obj) = object {
                    check_expr(obj, max_id);
                }
                for arg in args {
                    check_expr(arg, max_id);
                }
            }
            // Closure parameters and body contribute to the global LocalId space.
            // Without recursing here, find_max_local_id undercounts and the inliner
            // can allocate colliding IDs for newly inserted Lets.
            Expr::Closure { params, body, captures, mutable_captures, .. } => {
                for param in params {
                    *max_id = (*max_id).max(param.id);
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
            }
            // New/NewDynamic carry argument expressions
            Expr::New { args, .. } => {
                for arg in args {
                    check_expr(arg, max_id);
                }
            }
            Expr::NewDynamic { callee, args } => {
                check_expr(callee, max_id);
                for arg in args {
                    check_expr(arg, max_id);
                }
            }
            // Await/type coercions wrap an inner expression
            Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) |
            Expr::BigIntCoerce(inner) | Expr::NumberCoerce(inner) |
            Expr::BooleanCoerce(inner) | Expr::StringCoerce(inner) |
            Expr::ParseFloat(inner) |
            Expr::StringFromCharCode(inner) |
            Expr::JsonStringify(inner) | Expr::JsonParse(inner) => {
                check_expr(inner, max_id);
            }
            _ => {}
        }
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
            Stmt::If { condition, then_branch, else_branch } => {
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
            Stmt::For { init, condition, update, body } => {
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
            Stmt::Try { body, catch, finally } => {
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
            Stmt::Switch { discriminant, cases } => {
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
                if let Some((mut inlined_stmts, _result_expr)) = try_inline_call(expr, func_candidates, method_candidates, local_types, next_local_id) {
                    // When inlining into Stmt::Expr context (result discarded),
                    // convert Stmt::Return(Some(expr)) to Stmt::Expr(expr) and
                    // remove Stmt::Return(None). This prevents emitting a
                    // `ret` terminator mid-block (e.g., inside a for loop body).
                    // Only do this if returns are in safe positions (trailing).
                    let has_nested_return = inlined_stmts.iter().take(inlined_stmts.len().saturating_sub(1)).any(|s| {
                        fn stmt_has_return(s: &Stmt) -> bool {
                            match s {
                                Stmt::Return(_) => true,
                                Stmt::If { then_branch, else_branch, .. } => {
                                    then_branch.iter().any(stmt_has_return) ||
                                    else_branch.as_ref().map_or(false, |eb| eb.iter().any(stmt_has_return))
                                }
                                _ => false,
                            }
                        }
                        stmt_has_return(s)
                    });
                    if has_nested_return {
                        // Can't safely convert early returns; skip inlining
                        let hoisted = inline_calls_in_expr(expr, func_candidates, method_candidates, local_types, next_local_id);
                        if !hoisted.is_empty() { new_stmts = Some(hoisted); }
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
                    let hoisted = inline_calls_in_expr(expr, func_candidates, method_candidates, local_types, next_local_id);
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
            Stmt::Let { init: Some(expr), .. } => {
                let hoisted = inline_calls_in_expr(expr, func_candidates, method_candidates, local_types, next_local_id);
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
                let hoisted = inline_calls_in_expr(expr, func_candidates, method_candidates, local_types, next_local_id);
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
            Stmt::If { condition, then_branch, else_branch } => {
                let _hoisted = inline_calls_in_expr(condition, func_candidates, method_candidates, local_types, next_local_id);
                // Note: hoisting from conditions is rare and complex; skip for now
                inline_calls_in_stmts(then_branch, func_candidates, method_candidates, class_names, local_types, next_local_id);
                if let Some(else_b) = else_branch {
                    inline_calls_in_stmts(else_b, func_candidates, method_candidates, class_names, local_types, next_local_id);
                }
            }
            Stmt::While { condition, body } => {
                let _hoisted = inline_calls_in_expr(condition, func_candidates, method_candidates, local_types, next_local_id);
                inline_calls_in_stmts(body, func_candidates, method_candidates, class_names, local_types, next_local_id);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    let mut init_stmts = vec![*init_stmt.clone()];
                    inline_calls_in_stmts(&mut init_stmts, func_candidates, method_candidates, class_names, local_types, next_local_id);
                    if init_stmts.len() == 1 {
                        **init_stmt = init_stmts.remove(0);
                    }
                }
                if let Some(cond) = condition {
                    let _hoisted = inline_calls_in_expr(cond, func_candidates, method_candidates, local_types, next_local_id);
                }
                if let Some(upd) = update {
                    let _hoisted = inline_calls_in_expr(upd, func_candidates, method_candidates, local_types, next_local_id);
                }
                inline_calls_in_stmts(body, func_candidates, method_candidates, class_names, local_types, next_local_id);
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
    if let Some((stmts, mut result)) = try_inline_simple_call(expr, func_candidates, method_candidates, local_types, next_local_id) {
        let inner = inline_calls_in_expr(&mut result, func_candidates, method_candidates, local_types, next_local_id);
        *expr = result;
        let mut all = stmts;
        all.extend(inner);
        return all;
    }

    // Otherwise recurse into sub-expressions, collecting hoisted stmts
    let mut hoisted = Vec::new();
    match expr {
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
        Expr::Compare { left, right, .. } => {
            hoisted.extend(inline_calls_in_expr(left, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(right, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::Unary { operand, .. } => {
            hoisted.extend(inline_calls_in_expr(operand, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            hoisted.extend(inline_calls_in_expr(condition, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(then_expr, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(else_expr, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::Call { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(callee, func_candidates, method_candidates, local_types, next_local_id));
            for arg in args {
                hoisted.extend(inline_calls_in_expr(arg, func_candidates, method_candidates, local_types, next_local_id));
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                hoisted.extend(inline_calls_in_expr(elem, func_candidates, method_candidates, local_types, next_local_id));
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                hoisted.extend(inline_calls_in_expr(v, func_candidates, method_candidates, local_types, next_local_id));
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                hoisted.extend(inline_calls_in_expr(v, func_candidates, method_candidates, local_types, next_local_id));
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(e, func_candidates, method_candidates, local_types, next_local_id));
                    }
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            hoisted.extend(inline_calls_in_expr(callee, func_candidates, method_candidates, local_types, next_local_id));
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        hoisted.extend(inline_calls_in_expr(e, func_candidates, method_candidates, local_types, next_local_id));
                    }
                }
            }
        }
        Expr::IndexGet { object, index } => {
            hoisted.extend(inline_calls_in_expr(object, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(index, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::IndexSet { object, index, value } => {
            hoisted.extend(inline_calls_in_expr(object, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(index, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(value, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::PropertyGet { object, .. } => {
            hoisted.extend(inline_calls_in_expr(object, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::PropertySet { object, value, .. } => {
            hoisted.extend(inline_calls_in_expr(object, func_candidates, method_candidates, local_types, next_local_id));
            hoisted.extend(inline_calls_in_expr(value, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::LocalSet(_, value) => {
            hoisted.extend(inline_calls_in_expr(value, func_candidates, method_candidates, local_types, next_local_id));
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                hoisted.extend(inline_calls_in_expr(obj, func_candidates, method_candidates, local_types, next_local_id));
            }
            for arg in args {
                hoisted.extend(inline_calls_in_expr(arg, func_candidates, method_candidates, local_types, next_local_id));
            }
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
                            matches!(s, Stmt::Let { mutable: false, init: Some(_), .. })
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
                                    if let Some(Expr::LocalGet(fresh_id)) = param_map.get(&param.id) {
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
                                if let Stmt::Let { id, name, ty, mutable, init: Some(init_expr) } = stmt {
                                    let new_id = if let Some(Expr::LocalGet(fresh)) = param_map.get(id) {
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
        if let Expr::PropertyGet { object, property: method_name } = callee.as_ref() {
            if let Expr::LocalGet(obj_id) = object.as_ref() {
                // Look up the class type of this local variable
                if let Some(class_name) = local_types.get(obj_id) {
                    // Look up the method candidate
                    if let Some(method_candidate) = method_candidates.get(&(class_name.clone(), method_name.clone())) {
                        // Check for single return statement
                        if method_candidate.func.body.len() == 1 {
                            if let Stmt::Return(Some(return_expr)) = &method_candidate.func.body[0] {
                                let mut param_map: HashMap<LocalId, Expr> = HashMap::new();

                                // Map 'this' parameter to the receiver object
                                if let Some(this_id) = method_candidate.this_param_id {
                                    param_map.insert(this_id, Expr::LocalGet(*obj_id));
                                }

                                // Map parameters to arguments
                                // Note: Method params don't include 'this' - they use Expr::This instead
                                for (param, arg) in method_candidate.func.params.iter().zip(args.iter()) {
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
                                        for (param, arg) in method_candidate.func.params.iter().zip(args.iter()) {
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
        if let Expr::PropertyGet { object, property: method_name } = callee.as_ref() {
            if let Expr::LocalGet(obj_id) = object.as_ref() {
                if let Some(class_name) = local_types.get(obj_id) {
                    if let Some(method_candidate) = method_candidates.get(&(class_name.clone(), method_name.clone())) {
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
    matches!(expr,
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) |
        Expr::String(_) | Expr::WtfString(_) | Expr::Null | Expr::Undefined |
        Expr::LocalGet(_) | Expr::GlobalGet(_)
    )
}

/// Substitute local variable references in an expression
fn substitute_locals(expr: &mut Expr, param_map: &HashMap<LocalId, Expr>, next_local_id: &mut LocalId) {
    match expr {
        Expr::LocalGet(id) => {
            if let Some(replacement) = param_map.get(id) {
                *expr = replacement.clone();
            }
        }
        Expr::LocalSet(id, value) => {
            substitute_locals(value, param_map, next_local_id);
            if let Some(replacement) = param_map.get(id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *id = *new_id;
                }
            }
        }
        Expr::Update { id, .. } => {
            if let Some(Expr::LocalGet(new_id)) = param_map.get(id) {
                *id = *new_id;
            }
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
        Expr::Compare { left, right, .. } => {
            substitute_locals(left, param_map, next_local_id);
            substitute_locals(right, param_map, next_local_id);
        }
        Expr::Unary { operand, .. } => {
            substitute_locals(operand, param_map, next_local_id);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            substitute_locals(condition, param_map, next_local_id);
            substitute_locals(then_expr, param_map, next_local_id);
            substitute_locals(else_expr, param_map, next_local_id);
        }
        Expr::Call { callee, args, .. } => {
            substitute_locals(callee, param_map, next_local_id);
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                substitute_locals(elem, param_map, next_local_id);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        substitute_locals(e, param_map, next_local_id);
                    }
                }
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            substitute_locals(callee, param_map, next_local_id);
            for arg in args {
                match arg {
                    perry_hir::CallArg::Expr(e) | perry_hir::CallArg::Spread(e) => {
                        substitute_locals(e, param_map, next_local_id);
                    }
                }
            }
        }
        Expr::IndexGet { object, index } => {
            substitute_locals(object, param_map, next_local_id);
            substitute_locals(index, param_map, next_local_id);
        }
        Expr::IndexSet { object, index, value } => {
            substitute_locals(object, param_map, next_local_id);
            substitute_locals(index, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::PropertyGet { object, .. } => {
            substitute_locals(object, param_map, next_local_id);
        }
        Expr::PropertySet { object, value, .. } => {
            substitute_locals(object, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::TypeOf(inner)
        | Expr::Void(inner)
        | Expr::Await(inner)
        | Expr::Delete(inner)
        | Expr::StringCoerce(inner)
        | Expr::BooleanCoerce(inner)
        | Expr::NumberCoerce(inner)
        | Expr::IsFinite(inner)
        | Expr::IsNaN(inner)
        | Expr::NumberIsNaN(inner)
        | Expr::NumberIsFinite(inner)
        | Expr::NumberIsInteger(inner)
        | Expr::IsUndefinedOrBareNan(inner)
        | Expr::ParseFloat(inner)
        | Expr::WeakRefNew(inner)
        | Expr::WeakRefDeref(inner)
        | Expr::FinalizationRegistryNew(inner)
        | Expr::ObjectKeys(inner)
        | Expr::ObjectValues(inner)
        | Expr::ObjectEntries(inner)
        | Expr::ObjectFromEntries(inner)
        | Expr::ObjectIsFrozen(inner)
        | Expr::ObjectIsSealed(inner)
        | Expr::ObjectIsExtensible(inner)
        | Expr::ObjectCreate(inner)
        | Expr::ArrayFrom(inner)
        | Expr::Uint8ArrayFrom(inner)
        | Expr::IteratorToArray(inner)
        | Expr::StructuredClone(inner)
        | Expr::QueueMicrotask(inner)
        | Expr::ProcessNextTick(inner)
        | Expr::JsonParse(inner)
        | Expr::JsonStringify(inner)
        | Expr::ArrayIsArray(inner)
        | Expr::MathSqrt(inner)
        | Expr::MathFloor(inner)
        | Expr::MathCeil(inner)
        | Expr::MathRound(inner)
        | Expr::MathAbs(inner)
        | Expr::MathLog(inner)
        | Expr::MathLog2(inner)
        | Expr::MathLog10(inner)
        | Expr::MathLog1p(inner)
        | Expr::MathClz32(inner)
        | Expr::MathMinSpread(inner)
        | Expr::MathMaxSpread(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value { substitute_locals(v, param_map, next_local_id); }
        }
        // Set operations
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            substitute_locals(set, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::SetAdd { value, .. } => {
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            substitute_locals(set, param_map, next_local_id);
        }
        // Map operations
        Expr::MapHas { map, key } | Expr::MapGet { map, key } | Expr::MapDelete { map, key } => {
            substitute_locals(map, param_map, next_local_id);
            substitute_locals(key, param_map, next_local_id);
        }
        Expr::MapSet { map, key, value } => {
            substitute_locals(map, param_map, next_local_id);
            substitute_locals(key, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::MapSize(map) | Expr::MapClear(map) => {
            substitute_locals(map, param_map, next_local_id);
        }
        // Array operations
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            if let Some(replacement) = param_map.get(array_id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *array_id = *new_id;
                }
            }
        }
        Expr::ArrayPush { array_id, value } | Expr::ArrayUnshift { array_id, value } => {
            if let Some(replacement) = param_map.get(array_id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *array_id = *new_id;
                }
            }
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::ArrayPushSpread { array_id, source: value } => {
            if let Some(replacement) = param_map.get(array_id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *array_id = *new_id;
                }
            }
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::ArraySlice { array, start, end } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(start, param_map, next_local_id);
            if let Some(e) = end {
                substitute_locals(e, param_map, next_local_id);
            }
        }
        Expr::ArraySplice { array_id, start, delete_count, items } => {
            if let Some(replacement) = param_map.get(array_id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *array_id = *new_id;
                }
            }
            substitute_locals(start, param_map, next_local_id);
            if let Some(dc) = delete_count {
                substitute_locals(dc, param_map, next_local_id);
            }
            for item in items {
                substitute_locals(item, param_map, next_local_id);
            }
        }
        Expr::ArrayForEach { array, callback } |
        Expr::ArrayMap { array, callback } |
        Expr::ArrayFilter { array, callback } |
        Expr::ArrayFind { array, callback } |
        Expr::ArrayFindIndex { array, callback } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(callback, param_map, next_local_id);
        }
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(callback, param_map, next_local_id);
            if let Some(init) = initial {
                substitute_locals(init, param_map, next_local_id);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            substitute_locals(array, param_map, next_local_id);
            if let Some(sep) = separator {
                substitute_locals(sep, param_map, next_local_id);
            }
        }
        Expr::ArrayFlat { array } | Expr::ArrayToReversed { array } => {
            substitute_locals(array, param_map, next_local_id);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            substitute_locals(array, param_map, next_local_id);
        }
        Expr::ArrayToSorted { array, comparator } => {
            substitute_locals(array, param_map, next_local_id);
            if let Some(cmp) = comparator { substitute_locals(cmp, param_map, next_local_id); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(start, param_map, next_local_id);
            substitute_locals(delete_count, param_map, next_local_id);
            for item in items { substitute_locals(item, param_map, next_local_id); }
        }
        Expr::ArrayWith { array, index, value } => {
            substitute_locals(array, param_map, next_local_id);
            substitute_locals(index, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::ArrayCopyWithin { array_id, target, start, end } => {
            if let Some(replacement) = param_map.get(array_id) {
                if let Expr::LocalGet(new_id) = replacement {
                    *array_id = *new_id;
                }
            }
            substitute_locals(target, param_map, next_local_id);
            substitute_locals(start, param_map, next_local_id);
            if let Some(e) = end { substitute_locals(e, param_map, next_local_id); }
        }
        // Object literal
        Expr::Object(fields) => {
            for (_, value) in fields {
                substitute_locals(value, param_map, next_local_id);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                substitute_locals(value, param_map, next_local_id);
            }
        }
        // JSON operations
        Expr::JsonStringify(inner) | Expr::JsonParse(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        // Path/URL operations
        Expr::PathJoin(a, b) => {
            substitute_locals(a, param_map, next_local_id);
            substitute_locals(b, param_map, next_local_id);
        }
        Expr::PathDirname(p) | Expr::PathBasename(p) | Expr::PathExtname(p) |
        Expr::PathResolve(p) | Expr::PathIsAbsolute(p) | Expr::FileURLToPath(p) => {
            substitute_locals(p, param_map, next_local_id);
        }
        // Math operations
        Expr::MathFloor(inner) | Expr::MathCeil(inner) | Expr::MathRound(inner) |
        Expr::MathAbs(inner) | Expr::MathSqrt(inner) |
        Expr::MathLog(inner) | Expr::MathLog2(inner) | Expr::MathLog10(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            substitute_locals(base, param_map, next_local_id);
            substitute_locals(exp, param_map, next_local_id);
        }
        Expr::MathMin(exprs) | Expr::MathMax(exprs) => {
            for e in exprs {
                substitute_locals(e, param_map, next_local_id);
            }
        }
        // New expressions
        Expr::New { args, .. } => {
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        Expr::NewDynamic { callee, args } => {
            substitute_locals(callee, param_map, next_local_id);
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        Expr::JsNew { module_handle, args, .. } => {
            substitute_locals(module_handle, param_map, next_local_id);
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        Expr::JsNewFromHandle { constructor, args } => {
            substitute_locals(constructor, param_map, next_local_id);
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        // Closure expressions - substitute in body as well
        Expr::Closure { body, .. } => {
            substitute_locals_in_stmts(body, param_map, next_local_id);
        }
        // Native method calls
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                substitute_locals(obj, param_map, next_local_id);
            }
            for arg in args {
                substitute_locals(arg, param_map, next_local_id);
            }
        }
        Expr::NativeModuleRef(_) => {}
        // String operations
        Expr::StringSplit(string, delimiter) => {
            substitute_locals(string, param_map, next_local_id);
            substitute_locals(delimiter, param_map, next_local_id);
        }
        Expr::StringFromCharCode(code) | Expr::StringCoerce(code) => {
            substitute_locals(code, param_map, next_local_id);
        }
        // Type coercions and parsing
        Expr::BigIntCoerce(inner) | Expr::NumberCoerce(inner) | Expr::ParseFloat(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::ParseInt { string, radix } => {
            substitute_locals(string, param_map, next_local_id);
            if let Some(r) = radix {
                substitute_locals(r, param_map, next_local_id);
            }
        }
        // Global set
        Expr::GlobalSet(_, value) => {
            substitute_locals(value, param_map, next_local_id);
        }
        // Sequence
        Expr::Sequence(exprs) => {
            for e in exprs {
                substitute_locals(e, param_map, next_local_id);
            }
        }
        // InstanceOf / In
        Expr::InstanceOf { expr, .. } => {
            substitute_locals(expr, param_map, next_local_id);
        }
        Expr::In { property, object } => {
            substitute_locals(property, param_map, next_local_id);
            substitute_locals(object, param_map, next_local_id);
        }
        // Delete
        Expr::Delete(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        // ObjectRest
        Expr::ObjectRest { object, .. } => {
            substitute_locals(object, param_map, next_local_id);
        }
        // ArrayIsArray
        Expr::ArrayIsArray(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        // RegExp
        Expr::RegExpTest { regex, string } => {
            substitute_locals(regex, param_map, next_local_id);
            substitute_locals(string, param_map, next_local_id);
        }
        Expr::StringMatch { string, regex } => {
            substitute_locals(string, param_map, next_local_id);
            substitute_locals(regex, param_map, next_local_id);
        }
        Expr::StringReplace { string, pattern, replacement } => {
            substitute_locals(string, param_map, next_local_id);
            substitute_locals(pattern, param_map, next_local_id);
            substitute_locals(replacement, param_map, next_local_id);
        }
        // Error
        Expr::ErrorNew(Some(inner)) | Expr::ErrorMessage(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::ErrorNewWithCause { message, cause } => {
            substitute_locals(message, param_map, next_local_id);
            substitute_locals(cause, param_map, next_local_id);
        }
        Expr::TypeErrorNew(m) | Expr::RangeErrorNew(m) | Expr::ReferenceErrorNew(m) | Expr::SyntaxErrorNew(m) => {
            substitute_locals(m, param_map, next_local_id);
        }
        Expr::AggregateErrorNew { errors, message } => {
            substitute_locals(errors, param_map, next_local_id);
            substitute_locals(message, param_map, next_local_id);
        }
        // Date operations
        Expr::DateNew(Some(inner)) | Expr::DateGetTime(inner) |
        Expr::DateToISOString(inner) | Expr::DateGetFullYear(inner) |
        Expr::DateGetMonth(inner) | Expr::DateGetDate(inner) |
        Expr::DateGetHours(inner) | Expr::DateGetMinutes(inner) |
        Expr::DateGetSeconds(inner) | Expr::DateGetMilliseconds(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        // FS operations
        Expr::FsReadFileSync(inner) | Expr::FsExistsSync(inner) |
        Expr::FsMkdirSync(inner) | Expr::FsUnlinkSync(inner) |
        Expr::FsReadFileBinary(inner) | Expr::FsRmRecursive(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::FsWriteFileSync(a, b) | Expr::FsAppendFileSync(a, b) => {
            substitute_locals(a, param_map, next_local_id);
            substitute_locals(b, param_map, next_local_id);
        }
        Expr::ChildProcessSpawnBackground { command, args, log_file, env_json } => {
            substitute_locals(command, param_map, next_local_id);
            if let Some(a) = args { substitute_locals(a, param_map, next_local_id); }
            substitute_locals(log_file, param_map, next_local_id);
            if let Some(e) = env_json { substitute_locals(e, param_map, next_local_id); }
        }
        Expr::ChildProcessGetProcessStatus(h) | Expr::ChildProcessKillProcess(h) => {
            substitute_locals(h, param_map, next_local_id);
        }
        // Buffer operations
        Expr::BufferFrom { data, encoding } => {
            substitute_locals(data, param_map, next_local_id);
            if let Some(enc) = encoding { substitute_locals(enc, param_map, next_local_id); }
        }
        Expr::BufferAlloc { size, fill } => {
            substitute_locals(size, param_map, next_local_id);
            if let Some(f) = fill { substitute_locals(f, param_map, next_local_id); }
        }
        Expr::BufferAllocUnsafe(inner) | Expr::BufferConcat(inner) |
        Expr::BufferIsBuffer(inner) | Expr::BufferByteLength(inner) |
        Expr::BufferLength(inner) => {
            substitute_locals(inner, param_map, next_local_id);
        }
        Expr::BufferToString { buffer, encoding } => {
            substitute_locals(buffer, param_map, next_local_id);
            if let Some(enc) = encoding { substitute_locals(enc, param_map, next_local_id); }
        }
        Expr::BufferSlice { buffer, start, end } => {
            substitute_locals(buffer, param_map, next_local_id);
            if let Some(s) = start { substitute_locals(s, param_map, next_local_id); }
            if let Some(e) = end { substitute_locals(e, param_map, next_local_id); }
        }
        Expr::BufferFill { buffer, value } => {
            substitute_locals(buffer, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::BufferEquals { buffer, other } => {
            substitute_locals(buffer, param_map, next_local_id);
            substitute_locals(other, param_map, next_local_id);
        }
        Expr::BufferIndexGet { buffer, index } => {
            substitute_locals(buffer, param_map, next_local_id);
            substitute_locals(index, param_map, next_local_id);
        }
        Expr::BufferIndexSet { buffer, index, value } => {
            substitute_locals(buffer, param_map, next_local_id);
            substitute_locals(index, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
            substitute_locals(source, param_map, next_local_id);
            substitute_locals(target, param_map, next_local_id);
            if let Some(ts) = target_start { substitute_locals(ts, param_map, next_local_id); }
            if let Some(ss) = source_start { substitute_locals(ss, param_map, next_local_id); }
            if let Some(se) = source_end { substitute_locals(se, param_map, next_local_id); }
        }
        Expr::BufferWrite { buffer, string, offset, encoding } => {
            substitute_locals(buffer, param_map, next_local_id);
            substitute_locals(string, param_map, next_local_id);
            if let Some(o) = offset { substitute_locals(o, param_map, next_local_id); }
            if let Some(e) = encoding { substitute_locals(e, param_map, next_local_id); }
        }
        // JS interop
        Expr::JsGetExport { module_handle, .. } | Expr::JsGetProperty { object: module_handle, .. } => {
            substitute_locals(module_handle, param_map, next_local_id);
        }
        Expr::JsSetProperty { object, value, .. } => {
            substitute_locals(object, param_map, next_local_id);
            substitute_locals(value, param_map, next_local_id);
        }
        Expr::JsCallFunction { module_handle, args, .. } | Expr::JsCallMethod { object: module_handle, args, .. } => {
            substitute_locals(module_handle, param_map, next_local_id);
            for arg in args { substitute_locals(arg, param_map, next_local_id); }
        }
        Expr::JsCreateCallback { closure, .. } => {
            substitute_locals(closure, param_map, next_local_id);
        }
        // Static/Super method calls
        Expr::StaticMethodCall { args, .. } | Expr::SuperCall(args) |
        Expr::SuperMethodCall { args, .. } => {
            for arg in args { substitute_locals(arg, param_map, next_local_id); }
        }
        _ => {}
    }
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
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } |
        Expr::Compare { left, right, .. } => {
            substitute_this(left, obj_id);
            substitute_this(right, obj_id);
        }
        Expr::Unary { operand, .. } => {
            substitute_this(operand, obj_id);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
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
        Expr::IndexSet { object, index, value } => {
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
            if let Some(v) = value { substitute_this(v, obj_id); }
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
        Expr::MathFloor(inner) | Expr::MathCeil(inner) | Expr::MathRound(inner) |
        Expr::MathAbs(inner) | Expr::MathSqrt(inner) |
        Expr::MathLog(inner) | Expr::MathLog2(inner) | Expr::MathLog10(inner) => {
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
        Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } |
        Expr::ArrayForEach { array, callback } | Expr::ArrayFind { array, callback } |
        Expr::ArrayFindIndex { array, callback } | Expr::ArraySort { array, comparator: callback } => {
            substitute_this(array, obj_id);
            substitute_this(callback, obj_id);
        }
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
            substitute_this(array, obj_id);
            substitute_this(callback, obj_id);
            if let Some(init) = initial { substitute_this(init, obj_id); }
        }
        Expr::ArraySlice { array, start, end } => {
            substitute_this(array, obj_id);
            substitute_this(start, obj_id);
            if let Some(e) = end { substitute_this(e, obj_id); }
        }
        Expr::ArrayJoin { array, separator } => {
            substitute_this(array, obj_id);
            if let Some(sep) = separator { substitute_this(sep, obj_id); }
        }
        Expr::ArrayFlat { array } | Expr::ArrayFrom(array) | Expr::ArrayToReversed { array } => {
            substitute_this(array, obj_id);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            substitute_this(array, obj_id);
        }
        Expr::ArrayToSorted { array, comparator } => {
            substitute_this(array, obj_id);
            if let Some(cmp) = comparator { substitute_this(cmp, obj_id); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            substitute_this(array, obj_id);
            substitute_this(start, obj_id);
            substitute_this(delete_count, obj_id);
            for item in items { substitute_this(item, obj_id); }
        }
        Expr::ArrayWith { array, index, value } => {
            substitute_this(array, obj_id);
            substitute_this(index, obj_id);
            substitute_this(value, obj_id);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            substitute_this(target, obj_id);
            substitute_this(start, obj_id);
            if let Some(e) = end { substitute_this(e, obj_id); }
        }
        Expr::ArrayFromMapped { iterable, map_fn } => {
            substitute_this(iterable, obj_id);
            substitute_this(map_fn, obj_id);
        }
        Expr::ArraySplice { start, delete_count, items, .. } => {
            substitute_this(start, obj_id);
            if let Some(dc) = delete_count { substitute_this(dc, obj_id); }
            for item in items { substitute_this(item, obj_id); }
        }
        Expr::StringSplit(s, sep) => {
            substitute_this(s, obj_id);
            substitute_this(sep, obj_id);
        }
        Expr::Await(inner) => {
            substitute_this(inner, obj_id);
        }
        _ => {}
    }
}

/// Substitute Expr::This with a LocalGet reference in statements
fn substitute_this_in_stmts(stmts: &mut Vec<Stmt>, obj_id: LocalId) {
    for stmt in stmts.iter_mut() {
        match stmt {
            Stmt::Let { init: Some(expr), .. } => {
                substitute_this(expr, obj_id);
            }
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                substitute_this(expr, obj_id);
            }
            Stmt::If { condition, then_branch, else_branch } => {
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
            Stmt::For { init, condition, update, body } => {
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
            Stmt::If { then_branch, else_branch, .. } => {
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
            Stmt::Try { body, catch, finally } => {
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

fn substitute_locals_in_stmts(stmts: &mut Vec<Stmt>, param_map: &HashMap<LocalId, Expr>, next_local_id: &mut LocalId) {
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
            Stmt::If { condition, then_branch, else_branch } => {
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
            Stmt::For { init, condition, update, body } => {
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
    if f.is_async || f.is_generator { return false; }
    if f.params.len() != 2 { return false; }
    if f.body.len() != 5 { return false; }

    let p0 = f.params[0].id;
    let p1 = f.params[1].id;

    // First 4 stmts must be immutable Lets with half-word extraction inits
    let mut hi_of = [false; 2]; // hi_of[0] = saw hi-half of p0, hi_of[1] = p1
    let mut lo_of = [false; 2];
    for stmt in &f.body[..4] {
        match stmt {
            Stmt::Let { mutable: false, init: Some(init), .. } => {
                if let Some((pid, is_hi)) = is_half_extract(init, p0, p1) {
                    let idx = if pid == p0 { 0 } else { 1 };
                    if is_hi { hi_of[idx] = true; } else { lo_of[idx] = true; }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if !(hi_of[0] && lo_of[0] && hi_of[1] && lo_of[1]) { return false; }

    // Last stmt: Return(Some(Binary { BitOr, ..., Integer(0) }))
    matches!(&f.body[4], Stmt::Return(Some(Expr::Binary { op: BinaryOp::BitOr, right, .. })) if matches!(right.as_ref(), Expr::Integer(0)))
}

/// Check if an expression extracts the hi or lo 16-bit half of a parameter.
/// Returns `Some((param_id, is_hi))` on match.
fn is_half_extract(e: &Expr, p0: LocalId, p1: LocalId) -> Option<(LocalId, bool)> {
    // Pattern: (param >>> 16) & 0xffff  OR  (param >> 16) & 0xffff  →  hi-half
    // Pattern: param & 0xffff  →  lo-half
    match e {
        Expr::Binary { op: BinaryOp::BitAnd, left, right } => {
            if !matches!(right.as_ref(), Expr::Integer(0xffff)) { return None; }
            match left.as_ref() {
                Expr::Binary { op: BinaryOp::UShr | BinaryOp::Shr, left: inner, right: shift_amt } => {
                    if !matches!(shift_amt.as_ref(), Expr::Integer(16)) { return None; }
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
            Stmt::If { condition, then_branch, else_branch } => {
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
            Stmt::For { init, condition, update, body } => {
                if let Some(init_stmt) = init {
                    rewrite_imul_calls_in_stmts(std::slice::from_mut(init_stmt), imul_ids);
                }
                if let Some(c) = condition { rewrite_imul_calls_in_expr(c, imul_ids); }
                if let Some(u) = update { rewrite_imul_calls_in_expr(u, imul_ids); }
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
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            rewrite_imul_calls_in_expr(left, imul_ids);
            rewrite_imul_calls_in_expr(right, imul_ids);
        }
        Expr::Unary { operand, .. } => rewrite_imul_calls_in_expr(operand, imul_ids),
        Expr::Conditional { condition, then_expr, else_expr } => {
            rewrite_imul_calls_in_expr(condition, imul_ids);
            rewrite_imul_calls_in_expr(then_expr, imul_ids);
            rewrite_imul_calls_in_expr(else_expr, imul_ids);
        }
        Expr::Call { callee, args, .. } => {
            rewrite_imul_calls_in_expr(callee, imul_ids);
            for arg in args { rewrite_imul_calls_in_expr(arg, imul_ids); }
        }
        Expr::LocalSet(_, val) => rewrite_imul_calls_in_expr(val, imul_ids),
        Expr::IndexGet { object, index } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
        }
        Expr::IndexSet { object, index, value } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(index, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        Expr::Array(elems) => { for el in elems { rewrite_imul_calls_in_expr(el, imul_ids); } }
        Expr::PropertyGet { object, .. } => rewrite_imul_calls_in_expr(object, imul_ids),
        Expr::PropertySet { object, value, .. } => {
            rewrite_imul_calls_in_expr(object, imul_ids);
            rewrite_imul_calls_in_expr(value, imul_ids);
        }
        _ => {}
    }
}
