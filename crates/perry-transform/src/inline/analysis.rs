use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::walk_expr_children;
use perry_hir::{Class, Expr, Function, Module, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

pub fn find_max_local_id_in_module(module: &Module) -> LocalId {
    // 2026-07-02 audit (#5143 family, prime #684/S1 suspect): this private
    // scan missed class fields[].init / static_fields[].init /
    // computed_members / extends_expr — containers whose closures CAN hold
    // the module's numerically-highest LocalIds (they are the last-lowered
    // constructs of a big module). The inliner mints ids at max+1, so a miss
    // aliases a live id and the module-global loader silently skips loading
    // (locals.contains_key already true) — reading the wrong value. Take the
    // max of the legacy walk and the generator pass's hardened exhaustive
    // scan (single source of truth; #5293) so no container is missed.
    let hardened = crate::generator::compute_max_local_id(module);
    let mut max_id: LocalId = hardened;
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
pub fn is_inlinable(func: &Function) -> bool {
    // Don't inline async functions
    if func.is_async {
        return false;
    }

    // Don't inline generator functions. The body uses `yield` and (often) a
    // terminating `return value` to drive the state machine that
    // `transform_generators` later builds. Inlining the body into the caller
    // erases that contract: the `yield` exprs leak into the caller's
    // statement list and the call expression collapses to the function's
    // last `return` value (or `undefined`), so `gen()` no longer produces
    // an iterator — it produces the bare return value. Issue #457.
    if func.is_generator {
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

    // Don't inline a function whose body builds a closure over one of the
    // callee's OWN bindings — a parameter or a local. Those ids do not exist in
    // the caller's scope.
    //
    // Parameters were already excluded. Locals are the same hazard, and worse:
    // a captured-and-mutated local is *boxed*, and the caller has no idea. The
    // closure body is compiled once, from the original function, and reads its
    // capture slot as a box pointer (`js_closure_get_capture_bits` ->
    // `js_box_get_bits`). Cloning the body into a call site re-derives the
    // local there as a plain slot, so the call site stores the local's *value*
    // into the capture slot instead of a box. The closure then dereferences
    // that value as a box pointer: every read comes back `undefined` and every
    // write from inside the closure is lost.
    //
    //     function mk() { let k = 0; return () => { for (const x of [1]) { k = 7; } return k; }; }
    //     const g = mk();
    //     g();   // undefined, expected 7
    let mut callee_bindings: std::collections::HashSet<LocalId> =
        func.params.iter().map(|p| p.id).collect();
    collect_declared_local_ids(&func.body, &mut callee_bindings);
    if body_contains_closure_capturing(&func.body, &callee_bindings) {
        return false;
    }

    // Don't inline methods containing super.method() or super() calls.
    // These rely on the enclosing class context (ThisContext with parent_class)
    // which is lost once the body is inlined into the caller.
    if body_contains_super_call(&func.body) {
        return false;
    }

    // Don't inline functions that reference the dynamic `this` / `new.target`
    // bindings. Those belong to the callee's own invocation, but the inliner
    // substitutes the body into the CALLER's frame, where `this`/`new.target`
    // resolve to the caller's binding. A strict callee called as `f()` has
    // `this === undefined`; inlined into a sloppy caller it would instead read
    // the caller's `this` (the global object), so `typeof this` flips from
    // `"undefined"` to `"object"`. Refs test262 language/function-code
    // `10.4.3-1-*`.
    if body_references_dynamic_this(&func.body) {
        return false;
    }

    // Don't inline functions that call themselves. The single-Return-of-Call
    // pattern in `try_inline_simple_call` (and the multi-Let-then-Return
    // pattern) substitutes the body verbatim; when the body is
    // `return self(args)` the substituted result IS another call to `self`,
    // which `inline_calls_in_expr` immediately re-inlines, and the recursion
    // unbounded → perry-main stack overflow (issue #733: 2-line repro
    // `function f(): any { return f(); }; f();` blew the 64 MB compiler
    // stack on Windows). The infinite-recursion case has no useful
    // inlining anyway (we can't unroll the call into a finite expression
    // tree). Reject any function whose body contains a call whose callee
    // is its own FuncRef.
    if body_calls_func(&func.body, func.id) {
        return false;
    }

    true
}

/// Inlinability for a *method* invoked with a known (exact) receiver. Identical
/// to [`is_inlinable`] except the dynamic-`this` rejection is relaxed: the
/// method-inliner substitutes `this` for the concrete receiver
/// (`substitute_this_in_stmts`), so an ordinary `this.field` accessor/mutator
/// IS inlinable. `new.target` and nested closures remain rejected (see
/// [`method_body_blocks_this_substitution`]). Without this, every method that
/// touches `this` — i.e. essentially all of them — was excluded from inlining,
/// so a hot `obj.method()` in a loop always paid full dispatch + a non-inlined
/// call.
pub fn is_inlinable_method(func: &Function) -> bool {
    if func.is_async || func.is_generator {
        return false;
    }
    if !func.captures.is_empty() {
        return false;
    }
    if func.params.iter().any(|p| p.is_rest) {
        return false;
    }
    if func.body.len() > MAX_INLINE_STMTS {
        return false;
    }
    if !has_simple_control_flow(&func.body) {
        return false;
    }
    let param_ids: std::collections::HashSet<LocalId> = func.params.iter().map(|p| p.id).collect();
    if body_contains_closure_capturing(&func.body, &param_ids) {
        return false;
    }
    if body_contains_super_call(&func.body) {
        return false;
    }
    if method_body_blocks_this_substitution(&func.body) {
        return false;
    }
    if body_calls_func(&func.body, func.id) {
        return false;
    }
    true
}

/// Check if `stmts` contains any `Expr::Call { callee: FuncRef(target_id) }`,
/// recursively. Stops at closure boundaries — a self-reference inside a nested
/// closure is a value-position read, not a same-frame recursive tail, and the
/// closure body becomes a separate function at codegen.
pub fn body_calls_func(stmts: &[Stmt], target_id: FuncId) -> bool {
    fn check_expr(expr: &Expr, target_id: FuncId) -> bool {
        match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::FuncRef(fid) = callee.as_ref() {
                    if *fid == target_id {
                        return true;
                    }
                }
                check_expr(callee, target_id) || args.iter().any(|a| check_expr(a, target_id))
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                check_expr(left, target_id) || check_expr(right, target_id)
            }
            Expr::Unary { operand, .. } => check_expr(operand, target_id),
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                check_expr(condition, target_id)
                    || check_expr(then_expr, target_id)
                    || check_expr(else_expr, target_id)
            }
            Expr::Array(elements) => elements.iter().any(|e| check_expr(e, target_id)),
            Expr::IndexGet { object, index } => {
                check_expr(object, target_id) || check_expr(index, target_id)
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                check_expr(object, target_id)
                    || check_expr(index, target_id)
                    || check_expr(value, target_id)
            }
            Expr::PropertyGet { object, .. } => check_expr(object, target_id),
            Expr::PropertySet { object, value, .. } => {
                check_expr(object, target_id) || check_expr(value, target_id)
            }
            Expr::LocalSet(_, value) => check_expr(value, target_id),
            // Closure bodies are NOT walked: their calls happen in a different
            // frame at runtime. A nested closure that calls the outer is fine
            // for inlining purposes (the closure body itself won't be inlined
            // here — it's a separate codegen unit).
            _ => false,
        }
    }
    fn check_stmt(stmt: &Stmt, target_id: FuncId) -> bool {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            } => check_expr(expr, target_id),
            Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                check_expr(expr, target_id)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, target_id)
                    || then_branch.iter().any(|s| check_stmt(s, target_id))
                    || else_branch
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| check_stmt(s, target_id)))
            }
            Stmt::While { condition, body } => {
                check_expr(condition, target_id) || body.iter().any(|s| check_stmt(s, target_id))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| check_stmt(i, target_id))
                    || condition.as_ref().is_some_and(|c| check_expr(c, target_id))
                    || update.as_ref().is_some_and(|u| check_expr(u, target_id))
                    || body.iter().any(|s| check_stmt(s, target_id))
            }
            _ => false,
        }
    }
    stmts.iter().any(|s| check_stmt(s, target_id))
}

/// Return true only when direct method inlining preserves JS method lookup.
///
/// A call `obj.method()` checks own properties before prototype methods.
/// Instance fields (`method = ...`), computed instance fields (`[expr] = ...`),
/// and accessors can all shadow the prototype method that the HIR inliner is
/// about to substitute directly. If we cannot prove the class chain is free of
/// those hazards, keep the call intact for the normal dispatch path.
pub fn method_lookup_is_unshadowed(classes: &[Class], class_name: &str, method_name: &str) -> bool {
    let Some((data_fields, getter_names, setter_names)) =
        class_chain_property_sets(classes, class_name)
    else {
        return false;
    };

    // Index classes by name for O(1) chain-walk lookups instead of an
    // O(classes) linear scan per level. `entry().or_insert` keeps the FIRST
    // class for a duplicate name, matching the prior `iter().find()` semantics
    // (duplicate class names occur cross-module, e.g. Effect's same-named
    // `Type` in SchemaAST and ParseResult).
    let mut by_name: HashMap<&str, &Class> = HashMap::with_capacity(classes.len());
    for c in classes {
        by_name.entry(c.name.as_str()).or_insert(c);
    }

    let mut cur = Some(class_name);
    let mut depth = 0usize;
    while let Some(name) = cur {
        let Some(class) = by_name.get(name).copied() else {
            return false;
        };
        if class.extends_expr.is_some() || class.native_extends.is_some() {
            return false;
        }
        if class
            .fields
            .iter()
            .any(|f| f.key_expr.is_some() || f.name == method_name)
        {
            return false;
        }
        if class.getters.iter().any(|(n, _)| n == method_name)
            || class.setters.iter().any(|(n, _)| n == method_name)
        {
            return false;
        }
        if class.fields.iter().any(|f| {
            f.init.as_ref().is_some_and(|init| {
                construction_expr_can_affect_method_lookup(
                    init,
                    method_name,
                    &data_fields,
                    &getter_names,
                    &setter_names,
                )
            })
        }) {
            return false;
        }
        if class.constructor.as_ref().is_some_and(|ctor| {
            construction_stmts_can_affect_method_lookup(
                &ctor.body,
                method_name,
                &data_fields,
                &getter_names,
                &setter_names,
            )
        }) {
            return false;
        }
        cur = class.extends_name.as_deref();
        depth += 1;
        if depth > 32 {
            return false;
        }
    }
    true
}

pub fn class_chain_property_sets(
    classes: &[Class],
    class_name: &str,
) -> Option<(HashSet<String>, HashSet<String>, HashSet<String>)> {
    let mut fields = HashSet::new();
    let mut getters = HashSet::new();
    let mut setters = HashSet::new();
    // Index by name for O(1) chain-walk lookups; `entry().or_insert` keeps the
    // first class for a duplicate name, matching the prior `iter().find()`.
    let mut by_name: HashMap<&str, &Class> = HashMap::with_capacity(classes.len());
    for c in classes {
        by_name.entry(c.name.as_str()).or_insert(c);
    }
    let mut cur = Some(class_name);
    let mut depth = 0usize;
    while let Some(name) = cur {
        let class = by_name.get(name).copied()?;
        if class.extends_expr.is_some() || class.native_extends.is_some() {
            return None;
        }
        for field in &class.fields {
            if field.key_expr.is_some() {
                return None;
            }
            fields.insert(field.name.clone());
        }
        for (name, _) in &class.getters {
            getters.insert(name.clone());
        }
        for (name, _) in &class.setters {
            setters.insert(name.clone());
        }
        cur = class.extends_name.as_deref();
        depth += 1;
        if depth > 32 {
            return None;
        }
    }
    Some((fields, getters, setters))
}

pub fn construction_stmts_can_affect_method_lookup(
    stmts: &[Stmt],
    method_name: &str,
    data_fields: &HashSet<String>,
    getter_names: &HashSet<String>,
    setter_names: &HashSet<String>,
) -> bool {
    stmts.iter().any(|stmt| {
        construction_stmt_can_affect_method_lookup(
            stmt,
            method_name,
            data_fields,
            getter_names,
            setter_names,
        )
    })
}

pub fn construction_stmt_can_affect_method_lookup(
    stmt: &Stmt,
    method_name: &str,
    data_fields: &HashSet<String>,
    getter_names: &HashSet<String>,
    setter_names: &HashSet<String>,
) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(|expr| {
            construction_expr_can_affect_method_lookup(
                expr,
                method_name,
                data_fields,
                getter_names,
                setter_names,
            )
        }),
        Stmt::Expr(expr) => construction_expr_can_affect_method_lookup(
            expr,
            method_name,
            data_fields,
            getter_names,
            setter_names,
        ),
        Stmt::Return(Some(_)) | Stmt::Throw(_) => true,
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            construction_expr_can_affect_method_lookup(
                condition,
                method_name,
                data_fields,
                getter_names,
                setter_names,
            ) || construction_stmts_can_affect_method_lookup(
                then_branch,
                method_name,
                data_fields,
                getter_names,
                setter_names,
            ) || else_branch.as_ref().is_some_and(|branch| {
                construction_stmts_can_affect_method_lookup(
                    branch,
                    method_name,
                    data_fields,
                    getter_names,
                    setter_names,
                )
            })
        }
        Stmt::Labeled { body, .. } => construction_stmt_can_affect_method_lookup(
            body,
            method_name,
            data_fields,
            getter_names,
            setter_names,
        ),
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => false,
        Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::Try { .. }
        | Stmt::Switch { .. }
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => true,
    }
}

pub fn construction_expr_can_affect_method_lookup(
    expr: &Expr,
    method_name: &str,
    data_fields: &HashSet<String>,
    getter_names: &HashSet<String>,
    setter_names: &HashSet<String>,
) -> bool {
    match expr {
        Expr::This => true,
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => {
            property == method_name
                || !data_fields.contains(property)
                || getter_names.contains(property)
        }
        Expr::PropertySet {
            object,
            property,
            value,
        } if matches!(object.as_ref(), Expr::This) => {
            property == method_name
                || setter_names.contains(property)
                || construction_expr_can_affect_method_lookup(
                    value,
                    method_name,
                    data_fields,
                    getter_names,
                    setter_names,
                )
        }
        // #4126 changed `this.field = x` ctor assignments to lower as
        // `PutValueSet` (PutValue descriptor semantics) instead of
        // `PropertySet`. Mirror the `PropertySet { object: This }` arm so a
        // plain field store doesn't get treated as a method-lookup shadow —
        // otherwise every class whose constructor assigns a field disables
        // method inlining + scalar replacement (regressed #945).
        Expr::PutValueSet {
            target, key, value, ..
        } if matches!(target.as_ref(), Expr::This) => {
            match key.as_ref() {
                Expr::String(k) => {
                    k == method_name
                        || setter_names.contains(k)
                        || construction_expr_can_affect_method_lookup(
                            value,
                            method_name,
                            data_fields,
                            getter_names,
                            setter_names,
                        )
                }
                // Computed/dynamic key — can't prove which property is
                // written, so conservatively treat it as lookup-affecting.
                _ => true,
            }
        }
        Expr::LocalSet(_, value) => construction_expr_can_affect_method_lookup(
            value,
            method_name,
            data_fields,
            getter_names,
            setter_names,
        ),
        Expr::SuperCall(args) => args.iter().any(|arg| {
            construction_expr_can_affect_method_lookup(
                arg,
                method_name,
                data_fields,
                getter_names,
                setter_names,
            )
        }),
        Expr::Call { .. }
        | Expr::CallSpread { .. }
        | Expr::NativeMethodCall { .. }
        | Expr::StaticMethodCall { .. }
        | Expr::SuperMethodCall { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::ObjectAssign { .. }
        | Expr::PropertySet { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::IndexSet { .. }
        | Expr::IndexUpdate { .. }
        | Expr::StaticFieldSet { .. }
        | Expr::ClassStaticSymbolSet { .. }
        | Expr::RegisterClassParentDynamic { .. }
        | Expr::RegisterClassStaticSymbol { .. }
        | Expr::ClassExprFresh { .. }
        | Expr::SetFunctionPrototype { .. }
        | Expr::RegisterPrototypeMethod { .. }
        | Expr::RegisterFunctionPrototypeMethod { .. }
        | Expr::ObjectDefineProperty(_, _, _)
        | Expr::ObjectDefineProperties(_, _)
        | Expr::ObjectSetPrototypeOf(_, _)
        | Expr::Delete(_)
        | Expr::ReflectSet { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectDefineProperty { .. }
        | Expr::GlobalSet(_, _)
        | Expr::JsSetProperty { .. }
        | Expr::ProxySet { .. }
        | Expr::ProxyDelete { .. } => true,
        Expr::Closure { captures_this, .. } if *captures_this => true,
        Expr::Closure { .. } => false,
        _ => {
            let mut unsafe_lookup = false;
            walk_expr_children(expr, &mut |child| {
                if construction_expr_can_affect_method_lookup(
                    child,
                    method_name,
                    data_fields,
                    getter_names,
                    setter_names,
                ) {
                    unsafe_lookup = true;
                }
            });
            unsafe_lookup
        }
    }
}
