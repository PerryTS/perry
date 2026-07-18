use crate::ir::{Expr, Module, ModuleKind, Stmt};
use std::collections::{BTreeMap, HashMap};

/// Information about a native instance exported from another module
#[derive(Debug, Clone)]
pub struct ExportedNativeInstance {
    /// The native module (e.g., "pg")
    pub native_module: String,
    /// The native class (e.g., "Pool")
    pub native_class: String,
}

/// Fix cross-module native instance method calls
///
/// This function transforms method calls on variables that are imported native instances
/// from other TypeScript modules. For example, if module A exports `pool = new Pool()` and
/// module B imports `pool` from A, this function will transform `pool.query()` in B to
/// a NativeMethodCall.
///
/// # Arguments
/// * `module` - The HIR module to transform
/// * `exported_instances` - Map from (resolved_path, export_name) to native instance info
pub fn fix_cross_module_native_instances(
    module: &mut Module,
    exported_instances: &BTreeMap<(String, String), ExportedNativeInstance>,
    exported_func_return_instances: &BTreeMap<(String, String), ExportedNativeInstance>,
) {
    // Build a map from local variable names to native instance info
    let mut local_native_instances: HashMap<String, (String, String)> = HashMap::new();
    // Build a map from imported function local names to native return info
    let mut func_return_instances: HashMap<String, (String, String)> = HashMap::new();

    for import in &module.imports {
        // Only check imports from local TypeScript modules (NativeCompiled)
        if import.module_kind != ModuleKind::NativeCompiled {
            continue;
        }

        let resolved_path = match &import.resolved_path {
            Some(p) => p.clone(),
            None => continue,
        };

        for spec in &import.specifiers {
            let (local_name, exported_name) = match spec {
                crate::ir::ImportSpecifier::Named { imported, local } => {
                    (local.clone(), imported.clone())
                }
                crate::ir::ImportSpecifier::Default { local } => (local.clone(), local.clone()),
                crate::ir::ImportSpecifier::Namespace { .. } => continue,
            };

            // Check if this import is a native instance
            let key = (resolved_path.clone(), exported_name.clone());
            if let Some(info) = exported_instances.get(&key) {
                local_native_instances.insert(
                    local_name.clone(),
                    (info.native_module.clone(), info.native_class.clone()),
                );
            }

            // Check if this import is a function that returns a native instance
            let func_key = (resolved_path.clone(), exported_name);
            if let Some(info) = exported_func_return_instances.get(&func_key) {
                func_return_instances.insert(
                    local_name,
                    (info.native_module.clone(), info.native_class.clone()),
                );
            }
        }
    }

    // Scan for variables assigned from calls to native-returning functions
    // Maps LocalId -> (module_name, class_name)
    let mut local_id_native_instances: HashMap<perry_types::LocalId, (String, String)> =
        HashMap::new();

    if !func_return_instances.is_empty() {
        // Scan init statements
        for stmt in &module.init {
            scan_for_native_func_returns(
                stmt,
                &func_return_instances,
                &mut local_native_instances,
                &mut local_id_native_instances,
            );
        }
        // Scan function bodies
        for func in &module.functions {
            for stmt in &func.body {
                scan_for_native_func_returns(
                    stmt,
                    &func_return_instances,
                    &mut local_native_instances,
                    &mut local_id_native_instances,
                );
            }
        }
        // Scan class methods
        for class in &module.classes {
            if let Some(ctor) = &class.constructor {
                for stmt in &ctor.body {
                    scan_for_native_func_returns(
                        stmt,
                        &func_return_instances,
                        &mut local_native_instances,
                        &mut local_id_native_instances,
                    );
                }
            }
            for method in &class.methods {
                for stmt in &method.body {
                    scan_for_native_func_returns(
                        stmt,
                        &func_return_instances,
                        &mut local_native_instances,
                        &mut local_id_native_instances,
                    );
                }
            }
            for method in &class.static_methods {
                for stmt in &method.body {
                    scan_for_native_func_returns(
                        stmt,
                        &func_return_instances,
                        &mut local_native_instances,
                        &mut local_id_native_instances,
                    );
                }
            }
        }
    }

    // Variable-to-variable propagation: `let sock: Socket = plainSock` —
    // run a fixed-point scan so each rebind of an already-tracked native
    // instance keeps the dispatch information. Without this, a typed
    // ident-rebind drops the (module, class) tag and `sock.on(...)`
    // falls through to typed-interface dispatch on the small handle.
    {
        let mut changed = true;
        while changed {
            changed = false;
            // Init block
            for stmt in &module.init {
                if scan_for_ident_init_propagation(
                    stmt,
                    &mut local_native_instances,
                    &mut local_id_native_instances,
                ) {
                    changed = true;
                }
            }
            for func in &module.functions {
                for stmt in &func.body {
                    if scan_for_ident_init_propagation(
                        stmt,
                        &mut local_native_instances,
                        &mut local_id_native_instances,
                    ) {
                        changed = true;
                    }
                }
            }
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    for stmt in &ctor.body {
                        if scan_for_ident_init_propagation(
                            stmt,
                            &mut local_native_instances,
                            &mut local_id_native_instances,
                        ) {
                            changed = true;
                        }
                    }
                }
                for method in &class.methods {
                    for stmt in &method.body {
                        if scan_for_ident_init_propagation(
                            stmt,
                            &mut local_native_instances,
                            &mut local_id_native_instances,
                        ) {
                            changed = true;
                        }
                    }
                }
                for method in &class.static_methods {
                    for stmt in &method.body {
                        if scan_for_ident_init_propagation(
                            stmt,
                            &mut local_native_instances,
                            &mut local_id_native_instances,
                        ) {
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    if local_native_instances.is_empty() && local_id_native_instances.is_empty() {
        return;
    }

    // Transform statements in init
    for stmt in &mut module.init {
        fix_native_instance_stmt(stmt, &local_native_instances, &local_id_native_instances);
    }

    // Transform statements in functions
    for func in &mut module.functions {
        for stmt in &mut func.body {
            fix_native_instance_stmt(stmt, &local_native_instances, &local_id_native_instances);
        }
    }

    // Transform statements in class methods
    for class in &mut module.classes {
        if let Some(ctor) = &mut class.constructor {
            for stmt in &mut ctor.body {
                fix_native_instance_stmt(stmt, &local_native_instances, &local_id_native_instances);
            }
        }
        for method in &mut class.methods {
            for stmt in &mut method.body {
                fix_native_instance_stmt(stmt, &local_native_instances, &local_id_native_instances);
            }
        }
        for method in &mut class.static_methods {
            for stmt in &mut method.body {
                fix_native_instance_stmt(stmt, &local_native_instances, &local_id_native_instances);
            }
        }
    }
}

/// Scan for `let x = await func()` or `let x = func()` where func returns a native instance
pub fn scan_for_native_func_returns(
    stmt: &Stmt,
    func_return_instances: &HashMap<String, (String, String)>,
    local_native_instances: &mut HashMap<String, (String, String)>,
    local_id_native_instances: &mut HashMap<perry_types::LocalId, (String, String)>,
) {
    match stmt {
        Stmt::Let { id, name, init, .. } => {
            if let Some(init_expr) = init {
                // Unwrap Await if present
                let call_expr = match init_expr {
                    Expr::Await(inner) => inner.as_ref(),
                    other => other,
                };
                // Check if it's a call to a function that returns a native instance
                if let Expr::Call { callee, .. } = call_expr {
                    let func_name = match callee.as_ref() {
                        Expr::ExternFuncRef { name, .. } => Some(name.as_str()),
                        Expr::FuncRef(_) => None, // local funcs already handled by lower.rs
                        _ => None,
                    };
                    if let Some(fname) = func_name {
                        if let Some((module, class)) = func_return_instances.get(fname) {
                            local_native_instances
                                .insert(name.clone(), (module.clone(), class.clone()));
                            local_id_native_instances.insert(*id, (module.clone(), class.clone()));
                        }
                    }
                }
                // Recurse into any closures embedded in the init expression
                // (e.g. `new Promise((resolve, reject) => { const sock = openSocket(...) })`).
                scan_expr_for_closure_returns(
                    init_expr,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => {
            scan_expr_for_closure_returns(
                e,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
        }
        Stmt::Return(Some(e)) => {
            scan_expr_for_closure_returns(
                e,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                scan_for_native_func_returns(
                    s,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    scan_for_native_func_returns(
                        s,
                        func_return_instances,
                        local_native_instances,
                        local_id_native_instances,
                    );
                }
            }
        }
        // `Labeled`/`DoWhile` are how async/generator lowering builds its state
        // machine (`Try { Labeled { __step_done: DoWhile { switch on __gen_state } } }`),
        // so without these arms the scan stops at the first `await` boundary and
        // every native instance inside an async function stays untagged — while
        // `fix_native_instance_stmt` below *does* walk both. Keep the two
        // traversals in sync: a shape the fixer rewrites but the scan never
        // reaches produces no diagnostic, just a silently untyped receiver.
        Stmt::Labeled { body, .. } => {
            scan_for_native_func_returns(
                body,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                scan_for_native_func_returns(
                    s,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                scan_for_native_func_returns(
                    s,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
            if let Some(catch_block) = catch {
                for s in &catch_block.body {
                    scan_for_native_func_returns(
                        s,
                        func_return_instances,
                        local_native_instances,
                        local_id_native_instances,
                    );
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    scan_for_native_func_returns(
                        s,
                        func_return_instances,
                        local_native_instances,
                        local_id_native_instances,
                    );
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    scan_for_native_func_returns(
                        s,
                        func_return_instances,
                        local_native_instances,
                        local_id_native_instances,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Scan a statement (recursively) for `let x = y` patterns where `y` is
/// already a known native instance. When found, propagate the (module,
/// class) tag to `x` so its later `.on(...) / .write(...)` dispatches go
/// to the native runtime instead of the typed-interface fallback.
///
/// Returns `true` when at least one new propagation happened — the caller
/// fixes a point by re-running until stable.
pub fn scan_for_ident_init_propagation(
    stmt: &Stmt,
    local_native_instances: &mut HashMap<String, (String, String)>,
    local_id_native_instances: &mut HashMap<perry_types::LocalId, (String, String)>,
) -> bool {
    let mut changed = false;
    match stmt {
        Stmt::Let { id, name, init, .. } => {
            if let Some(init_expr) = init {
                if let Some((module, class)) = lookup_native_from_init_ident(
                    init_expr,
                    local_native_instances,
                    local_id_native_instances,
                ) {
                    let info = (module, class);
                    if local_native_instances
                        .insert(name.clone(), info.clone())
                        .is_none()
                    {
                        changed = true;
                    }
                    if local_id_native_instances.insert(*id, info).is_none() {
                        changed = true;
                    }
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                if scan_for_ident_init_propagation(
                    s,
                    local_native_instances,
                    local_id_native_instances,
                ) {
                    changed = true;
                }
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    if scan_for_ident_init_propagation(
                        s,
                        local_native_instances,
                        local_id_native_instances,
                    ) {
                        changed = true;
                    }
                }
            }
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            for s in body {
                if scan_for_ident_init_propagation(
                    s,
                    local_native_instances,
                    local_id_native_instances,
                ) {
                    changed = true;
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                if scan_for_ident_init_propagation(
                    s,
                    local_native_instances,
                    local_id_native_instances,
                ) {
                    changed = true;
                }
            }
            if let Some(catch_block) = catch {
                for s in &catch_block.body {
                    if scan_for_ident_init_propagation(
                        s,
                        local_native_instances,
                        local_id_native_instances,
                    ) {
                        changed = true;
                    }
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    if scan_for_ident_init_propagation(
                        s,
                        local_native_instances,
                        local_id_native_instances,
                    ) {
                        changed = true;
                    }
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    if scan_for_ident_init_propagation(
                        s,
                        local_native_instances,
                        local_id_native_instances,
                    ) {
                        changed = true;
                    }
                }
            }
        }
        _ => {}
    }
    changed
}

/// If the init expression resolves to a known native instance via a
/// LocalGet (HIR's representation of an ident reference), return its
/// (module, class). TS type casts are stripped at lowering time, so we
/// only need to inspect LocalGet here.
pub fn lookup_native_from_init_ident(
    expr: &Expr,
    _local_native_instances: &HashMap<String, (String, String)>,
    local_id_native_instances: &HashMap<perry_types::LocalId, (String, String)>,
) -> Option<(String, String)> {
    if let Expr::LocalGet(id) = expr {
        return local_id_native_instances.get(id).cloned();
    }
    None
}

/// Walk an expression for nested closures and scan their bodies. Catches
/// `const sock = openSocket(...)` when wrapped in a closure passed to
/// `new Promise(...)`, `setTimeout(...)`, callback args, etc.
pub fn scan_expr_for_closure_returns(
    expr: &Expr,
    func_return_instances: &HashMap<String, (String, String)>,
    local_native_instances: &mut HashMap<String, (String, String)>,
    local_id_native_instances: &mut HashMap<perry_types::LocalId, (String, String)>,
) {
    match expr {
        Expr::Closure { body, .. } => {
            for s in body {
                scan_for_native_func_returns(
                    s,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Expr::Call { callee, args, .. } => {
            scan_expr_for_closure_returns(
                callee,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
            for a in args {
                scan_expr_for_closure_returns(
                    a,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            scan_expr_for_closure_returns(
                callee,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
            for a in args {
                let inner = match a {
                    crate::ir::CallArg::Expr(v) | crate::ir::CallArg::Spread(v) => v,
                };
                scan_expr_for_closure_returns(
                    inner,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                scan_expr_for_closure_returns(
                    a,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                scan_expr_for_closure_returns(
                    obj,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
            for a in args {
                scan_expr_for_closure_returns(
                    a,
                    func_return_instances,
                    local_native_instances,
                    local_id_native_instances,
                );
            }
        }
        Expr::Await(inner) => scan_expr_for_closure_returns(
            inner,
            func_return_instances,
            local_native_instances,
            local_id_native_instances,
        ),
        // `app = buildServer()` as an *assignment* rather than a `let` with an
        // initializer. Async/generator lowering rewrites
        //   const app = buildServer();  await app.listen(...)
        // into a hoisted box plus `Expr(LocalSet(0, Call(buildServer)))` inside
        // the step closure, so the `Stmt::Let { init: Some(call) }` arm above
        // never sees it and `app` stays untagged — `app.listen(...)` then lowers
        // to a generic property call, hits codegen's unknown-native-method arm,
        // and silently no-ops. Tag by LocalId here (the name is gone by this
        // point; LocalId is what `LocalGet`-based dispatch resolves against).
        Expr::LocalSet(id, value) => {
            let call_expr = match value.as_ref() {
                Expr::Await(inner) => inner.as_ref(),
                other => other,
            };
            if let Expr::Call { callee, .. } = call_expr {
                if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                    if let Some((module, class)) = func_return_instances.get(name.as_str()) {
                        local_id_native_instances.insert(*id, (module.clone(), class.clone()));
                    }
                }
            }
            scan_expr_for_closure_returns(
                value,
                func_return_instances,
                local_native_instances,
                local_id_native_instances,
            );
        }
        _ => {}
    }
}

pub fn fix_native_instance_stmt(
    stmt: &mut Stmt,
    native_instances: &HashMap<String, (String, String)>,
    local_id_instances: &HashMap<perry_types::LocalId, (String, String)>,
) {
    match stmt {
        Stmt::Expr(expr) => fix_native_instance_expr(expr, native_instances, local_id_instances),
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                fix_native_instance_expr(e, native_instances, local_id_instances);
            }
        }
        Stmt::Return(Some(e)) => fix_native_instance_expr(e, native_instances, local_id_instances),
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fix_native_instance_expr(condition, native_instances, local_id_instances);
            for s in then_branch {
                fix_native_instance_stmt(s, native_instances, local_id_instances);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    fix_native_instance_stmt(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::While { condition, body } => {
            fix_native_instance_expr(condition, native_instances, local_id_instances);
            for s in body {
                fix_native_instance_stmt(s, native_instances, local_id_instances);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                fix_native_instance_stmt(s, native_instances, local_id_instances);
            }
            fix_native_instance_expr(condition, native_instances, local_id_instances);
        }
        Stmt::Labeled { body, .. } => {
            fix_native_instance_stmt(body, native_instances, local_id_instances);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                fix_native_instance_stmt(init_stmt, native_instances, local_id_instances);
            }
            if let Some(e) = condition {
                fix_native_instance_expr(e, native_instances, local_id_instances);
            }
            if let Some(e) = update {
                fix_native_instance_expr(e, native_instances, local_id_instances);
            }
            for s in body {
                fix_native_instance_stmt(s, native_instances, local_id_instances);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fix_native_instance_expr(discriminant, native_instances, local_id_instances);
            for case in cases {
                if let Some(ref mut test) = case.test {
                    fix_native_instance_expr(test, native_instances, local_id_instances);
                }
                for s in &mut case.body {
                    fix_native_instance_stmt(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                fix_native_instance_stmt(s, native_instances, local_id_instances);
            }
            if let Some(catch_block) = catch {
                for s in &mut catch_block.body {
                    fix_native_instance_stmt(s, native_instances, local_id_instances);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    fix_native_instance_stmt(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::Throw(e) => fix_native_instance_expr(e, native_instances, local_id_instances),
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

/// Try to resolve native instance info from an object expression
pub fn resolve_native_instance<'a>(
    object: &Expr,
    native_instances: &'a HashMap<String, (String, String)>,
    local_id_instances: &'a HashMap<perry_types::LocalId, (String, String)>,
) -> Option<(&'a String, &'a String)> {
    match object {
        Expr::ExternFuncRef { name, .. } => native_instances.get(name).map(|(m, c)| (m, c)),
        Expr::LocalGet(id) => local_id_instances.get(id).map(|(m, c)| (m, c)),
        _ => None,
    }
}

pub fn fix_native_instance_expr(
    expr: &mut Expr,
    native_instances: &HashMap<String, (String, String)>,
    local_id_instances: &HashMap<perry_types::LocalId, (String, String)>,
) {
    match expr {
        // The key case: method calls that might be on native instances
        Expr::Call { callee, args, .. } => {
            // Check if this is a method call: obj.method(args)
            if let Expr::PropertyGet { object, property } = callee.as_mut() {
                // Check if the object is a native instance (ExternFuncRef or LocalGet)
                if let Some((native_module, native_class)) =
                    resolve_native_instance(object.as_ref(), native_instances, local_id_instances)
                {
                    let native_module = native_module.clone();
                    let native_class = native_class.clone();
                    // Transform args first
                    for arg in args.iter_mut() {
                        fix_native_instance_expr(arg, native_instances, local_id_instances);
                    }
                    let args_owned: Vec<Expr> = std::mem::take(args);
                    let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                    // Transform to NativeMethodCall
                    *expr = Expr::NativeMethodCall {
                        module: native_module,
                        class_name: Some(native_class),
                        object: Some(Box::new(object_expr)),
                        method: property.clone(),
                        args: args_owned,
                    };
                    return;
                }
            }

            // Not a native instance call, recurse
            fix_native_instance_expr(callee, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        // Recurse into other expressions
        Expr::Binary { left, right, .. } => {
            fix_native_instance_expr(left, native_instances, local_id_instances);
            fix_native_instance_expr(right, native_instances, local_id_instances);
        }
        Expr::Unary { operand, .. } => {
            fix_native_instance_expr(operand, native_instances, local_id_instances);
        }
        Expr::Logical { left, right, .. } => {
            fix_native_instance_expr(left, native_instances, local_id_instances);
            fix_native_instance_expr(right, native_instances, local_id_instances);
        }
        Expr::Compare { left, right, .. } => {
            fix_native_instance_expr(left, native_instances, local_id_instances);
            fix_native_instance_expr(right, native_instances, local_id_instances);
        }
        Expr::LocalSet(_, value) => {
            fix_native_instance_expr(value, native_instances, local_id_instances);
        }
        Expr::GlobalSet(_, value) => {
            fix_native_instance_expr(value, native_instances, local_id_instances);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fix_native_instance_expr(condition, native_instances, local_id_instances);
            fix_native_instance_expr(then_expr, native_instances, local_id_instances);
            fix_native_instance_expr(else_expr, native_instances, local_id_instances);
        }
        Expr::Array(elements) => {
            for elem in elements {
                fix_native_instance_expr(elem, native_instances, local_id_instances);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    crate::ir::ArrayElement::Expr(e) => {
                        fix_native_instance_expr(e, native_instances, local_id_instances)
                    }
                    crate::ir::ArrayElement::Spread(e) => {
                        fix_native_instance_expr(e, native_instances, local_id_instances)
                    }
                    crate::ir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(properties) => {
            for (_, value) in properties {
                fix_native_instance_expr(value, native_instances, local_id_instances);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                fix_native_instance_expr(value, native_instances, local_id_instances);
            }
        }
        Expr::PropertyGet { object, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
        }
        Expr::PropertySet { object, value, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
            fix_native_instance_expr(value, native_instances, local_id_instances);
        }
        Expr::PropertyUpdate { object, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
        }
        Expr::IndexGet { object, index } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
            fix_native_instance_expr(index, native_instances, local_id_instances);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
            fix_native_instance_expr(index, native_instances, local_id_instances);
            fix_native_instance_expr(value, native_instances, local_id_instances);
        }
        Expr::Await(inner) => {
            // Handle Await(Call{PropertyGet{obj...}}) pattern for native instances
            if let Expr::Call { callee, args, .. } = inner.as_mut() {
                if let Expr::PropertyGet { object, property } = callee.as_mut() {
                    if let Some((native_module, native_class)) = resolve_native_instance(
                        object.as_ref(),
                        native_instances,
                        local_id_instances,
                    ) {
                        let native_module = native_module.clone();
                        let native_class = native_class.clone();
                        // Transform args first
                        for arg in args.iter_mut() {
                            fix_native_instance_expr(arg, native_instances, local_id_instances);
                        }
                        let args_owned: Vec<Expr> = std::mem::take(args);
                        let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                        // Replace the inner Call with NativeMethodCall (wrapped by Await)
                        *inner.as_mut() = Expr::NativeMethodCall {
                            module: native_module,
                            class_name: Some(native_class),
                            object: Some(Box::new(object_expr)),
                            method: property.clone(),
                            args: args_owned,
                        };
                        return;
                    }
                }
            }
            // Otherwise, just recurse
            fix_native_instance_expr(inner, native_instances, local_id_instances);
        }
        Expr::Closure { body, .. } => {
            for stmt in body {
                fix_native_instance_stmt(stmt, native_instances, local_id_instances);
            }
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                fix_native_instance_expr(e, native_instances, local_id_instances);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fix_native_instance_expr(obj, native_instances, local_id_instances);
            }
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::New { args, .. } | Expr::SuperCall(args) => {
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            fix_native_instance_expr(callee, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        // JS interop expressions that may contain native instance calls
        Expr::JsCallMethod { object, args, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::JsCallFunction {
            module_handle,
            args,
            ..
        } => {
            fix_native_instance_expr(module_handle, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            fix_native_instance_expr(closure, native_instances, local_id_instances);
        }
        Expr::JsGetProperty { object, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
        }
        Expr::JsSetProperty { object, value, .. } => {
            fix_native_instance_expr(object, native_instances, local_id_instances);
            fix_native_instance_expr(value, native_instances, local_id_instances);
        }
        Expr::JsNew {
            module_handle,
            args,
            ..
        } => {
            fix_native_instance_expr(module_handle, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::JsNewFromHandle { constructor, args } => {
            fix_native_instance_expr(constructor, native_instances, local_id_instances);
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        Expr::JsGetExport { module_handle, .. } => {
            fix_native_instance_expr(module_handle, native_instances, local_id_instances);
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                fix_native_instance_expr(arg, native_instances, local_id_instances);
            }
        }
        // Many more expressions can contain sub-expressions, but for the first pass,
        // we'll focus on the most common patterns
        _ => {}
    }
}
