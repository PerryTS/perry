use crate::ir::{Expr, Module, Stmt};
use perry_types::LocalId;
use std::collections::HashMap;

/// Fix local native instance method calls within the same module
///
/// This function tracks variables that are assigned from native module creation functions
/// (like mysql.createPool(), mysql.createConnection()) and transforms subsequent method
/// calls on those variables into NativeMethodCall expressions.
///
/// For example:
/// ```typescript
/// const pool = mysql.createPool({...});  // Tracked as mysql2/promise pool
/// await pool.execute(sql, params);       // Transformed to NativeMethodCall
/// ```
pub fn fix_local_native_instances(module: &mut Module) {
    // Build maps for tracking native instances:
    // - by name (for ExternFuncRef - imported variables)
    // - by LocalId (for LocalGet - local variables)
    let mut local_native_instances: HashMap<String, (String, String)> = HashMap::new();
    let mut local_id_native_instances: HashMap<LocalId, (String, String)> = HashMap::new();

    // Issue #341: pre-build a global class → field → native-instance map.
    // For each user class, scan the constructor body and field
    // initializers for `this.<field> = new Database(...)` (or any
    // other native instance creation). We also track instance fields
    // whose declared type is a Named type that resolves to a class
    // we've seen this same shape on. Used by the rewriter to handle
    // both `this.<field>.method()` (in class methods) and
    // `<local>.<field>.method()` (after the inliner copies a class
    // method into a caller's body and substitutes `this` with the
    // receiver local — the shape that was breaking the SIGSEGV repro
    // in #341).
    let class_field_natives: HashMap<String, HashMap<String, (String, String)>> =
        build_class_field_natives(module);

    // Scan init statements for native instance creations (recursively)
    for stmt in &module.init {
        scan_stmt_for_native_instances(
            stmt,
            &mut local_native_instances,
            &mut local_id_native_instances,
        );
    }

    // Issue #341: also track which locals hold user-class instances,
    // so `s.<field>.method()` can dispatch through the class field map.
    let mut init_local_user_classes: HashMap<LocalId, String> = HashMap::new();
    for stmt in &module.init {
        scan_stmt_for_user_class_instances(stmt, &mut init_local_user_classes);
    }

    // Transform method calls on these native instances in init
    for stmt in &mut module.init {
        fix_native_instance_stmt_with_locals(
            stmt,
            &local_native_instances,
            &local_id_native_instances,
        );
        fix_class_field_stmt(stmt, &class_field_natives, &init_local_user_classes, None);
    }

    // Process each function separately with its own local variable scope
    for func in &mut module.functions {
        // Build per-function local mapping by scanning all statements recursively
        let mut func_local_ids: HashMap<LocalId, (String, String)> =
            local_id_native_instances.clone();
        let mut func_local_names: HashMap<String, (String, String)> =
            local_native_instances.clone();
        let mut func_user_classes: HashMap<LocalId, String> = init_local_user_classes.clone();
        for stmt in &func.body {
            scan_stmt_for_native_instances(stmt, &mut func_local_names, &mut func_local_ids);
            scan_stmt_for_user_class_instances(stmt, &mut func_user_classes);
        }
        // Transform method calls
        for stmt in &mut func.body {
            fix_native_instance_stmt_with_locals(stmt, &func_local_names, &func_local_ids);
            fix_class_field_stmt(stmt, &class_field_natives, &func_user_classes, None);
        }
    }

    for class in &mut module.classes {
        let class_owned_name = class.name.clone();
        let empty_field_map: HashMap<String, (String, String)> = HashMap::new();
        let field_native_instances = class_field_natives
            .get(&class_owned_name)
            .unwrap_or(&empty_field_map);

        if let Some(ctor) = &mut class.constructor {
            let mut ctor_local_ids = local_id_native_instances.clone();
            let mut ctor_local_names = local_native_instances.clone();
            let mut ctor_user_classes: HashMap<LocalId, String> = HashMap::new();
            for stmt in &ctor.body {
                scan_stmt_for_native_instances(stmt, &mut ctor_local_names, &mut ctor_local_ids);
                scan_stmt_for_user_class_instances(stmt, &mut ctor_user_classes);
            }
            for stmt in &mut ctor.body {
                fix_native_instance_stmt_with_locals(stmt, &ctor_local_names, &ctor_local_ids);
                fix_class_field_stmt(
                    stmt,
                    &class_field_natives,
                    &ctor_user_classes,
                    Some(&class_owned_name),
                );
            }
        }
        for method in &mut class.methods {
            let mut method_local_ids = local_id_native_instances.clone();
            let mut method_local_names = local_native_instances.clone();
            let mut method_user_classes: HashMap<LocalId, String> = HashMap::new();
            for stmt in &method.body {
                scan_stmt_for_native_instances(
                    stmt,
                    &mut method_local_names,
                    &mut method_local_ids,
                );
                scan_stmt_for_user_class_instances(stmt, &mut method_user_classes);
            }
            for stmt in &mut method.body {
                fix_native_instance_stmt_with_locals(stmt, &method_local_names, &method_local_ids);
                fix_class_field_stmt(
                    stmt,
                    &class_field_natives,
                    &method_user_classes,
                    Some(&class_owned_name),
                );
            }
        }
        for method in &mut class.static_methods {
            let mut method_local_ids = local_id_native_instances.clone();
            let mut method_local_names = local_native_instances.clone();
            let mut method_user_classes: HashMap<LocalId, String> = HashMap::new();
            for stmt in &method.body {
                scan_stmt_for_native_instances(
                    stmt,
                    &mut method_local_names,
                    &mut method_local_ids,
                );
                scan_stmt_for_user_class_instances(stmt, &mut method_user_classes);
            }
            for stmt in &mut method.body {
                fix_native_instance_stmt_with_locals(stmt, &method_local_names, &method_local_ids);
                fix_class_field_stmt(stmt, &class_field_natives, &method_user_classes, None);
            }
        }
        // Touch field_native_instances so the unused-binding lint is happy
        // even when this class has no entries in the map (the actual use is
        // through `class_field_natives` lookup inside `fix_class_field_stmt`).
        let _ = field_native_instances;
    }
}

/// Issue #341: build a global map `class_name → field_name → (module, native_class)`
/// from constructor bodies and field initializers across all classes in the module.
pub fn build_class_field_natives(
    module: &Module,
) -> HashMap<String, HashMap<String, (String, String)>> {
    let mut out: HashMap<String, HashMap<String, (String, String)>> = HashMap::new();
    for class in &module.classes {
        let mut field_map: HashMap<String, (String, String)> = HashMap::new();
        if let Some(ctor) = &class.constructor {
            for stmt in &ctor.body {
                scan_stmt_for_field_native_instances(stmt, &mut field_map);
            }
        }
        for field in &class.fields {
            if let Some(init) = &field.init {
                if let Some((module_name, class_name)) =
                    detect_native_instance_creation_with_context(init, &HashMap::new())
                {
                    field_map.insert(field.name.clone(), (module_name, class_name));
                }
            }
        }
        if !field_map.is_empty() {
            out.insert(class.name.clone(), field_map);
        }
    }
    out
}

/// Issue #341: scan a statement for `let s = new ClassName(...)` and
/// record the local id → class name mapping. Lets the rewriter
/// recognise `s.field.method()` in code that called a class method
/// which the inliner has already copied (substituting `this` with the
/// receiver local). Recurses through control-flow constructs so
/// guarded `let` bindings still register.
pub fn scan_stmt_for_user_class_instances(
    stmt: &Stmt,
    user_classes: &mut HashMap<LocalId, String>,
) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            if let Some(init_expr) = init {
                if let Expr::New { class_name, .. } = init_expr {
                    user_classes.insert(*id, class_name.clone());
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                scan_stmt_for_user_class_instances(s, user_classes);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    scan_stmt_for_user_class_instances(s, user_classes);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                scan_stmt_for_user_class_instances(s, user_classes);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                scan_stmt_for_user_class_instances(init_stmt.as_ref(), user_classes);
            }
            for s in body {
                scan_stmt_for_user_class_instances(s, user_classes);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                scan_stmt_for_user_class_instances(s, user_classes);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    scan_stmt_for_user_class_instances(s, user_classes);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    scan_stmt_for_user_class_instances(s, user_classes);
                }
            }
        }
        _ => {}
    }
}

/// Issue #341: top-level entry point — walks a statement and rewrites
/// both `this.<field>.method()` (when `current_class` is set) and
/// `<local>.<field>.method()` (when the local holds a known user
/// class) into `NativeMethodCall` for any field that's been registered
/// as a native instance.
pub fn fix_class_field_stmt(
    stmt: &mut Stmt,
    class_field_natives: &HashMap<String, HashMap<String, (String, String)>>,
    user_classes: &HashMap<LocalId, String>,
    current_class: Option<&str>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            fix_class_field_expr(e, class_field_natives, user_classes, current_class);
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                fix_class_field_expr(e, class_field_natives, user_classes, current_class);
            }
        }
        Stmt::Return(Some(e)) => {
            fix_class_field_expr(e, class_field_natives, user_classes, current_class)
        }
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fix_class_field_expr(condition, class_field_natives, user_classes, current_class);
            for s in then_branch {
                fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            fix_class_field_expr(condition, class_field_natives, user_classes, current_class);
            for s in body {
                fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
            }
        }
        Stmt::Labeled { body, .. } => {
            fix_class_field_stmt(body, class_field_natives, user_classes, current_class);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                fix_class_field_stmt(
                    init_stmt.as_mut(),
                    class_field_natives,
                    user_classes,
                    current_class,
                );
            }
            if let Some(cond) = condition {
                fix_class_field_expr(cond, class_field_natives, user_classes, current_class);
            }
            if let Some(upd) = update {
                fix_class_field_expr(upd, class_field_natives, user_classes, current_class);
            }
            for s in body {
                fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
            }
            if let Some(ref mut catch_clause) = catch {
                for s in &mut catch_clause.body {
                    fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fix_class_field_expr(
                discriminant,
                class_field_natives,
                user_classes,
                current_class,
            );
            for case in cases {
                if let Some(test) = &mut case.test {
                    fix_class_field_expr(test, class_field_natives, user_classes, current_class);
                }
                for s in &mut case.body {
                    fix_class_field_stmt(s, class_field_natives, user_classes, current_class);
                }
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

/// Issue #341: rewrite `this.<field>.method(args)` and
/// `<localGet>.<field>.method(args)` into `NativeMethodCall` when the
/// field is registered as a native instance for the enclosing class
/// (for `this.*`) or the local's class (for `local.*`).
pub fn fix_class_field_expr(
    expr: &mut Expr,
    class_field_natives: &HashMap<String, HashMap<String, (String, String)>>,
    user_classes: &HashMap<LocalId, String>,
    current_class: Option<&str>,
) {
    // Helper: given an inner-receiver expression and a property name,
    // return the (module, native_class) registration if this field is
    // a tracked native instance.
    fn lookup_field_native<'a>(
        receiver: &Expr,
        field: &str,
        class_field_natives: &'a HashMap<String, HashMap<String, (String, String)>>,
        user_classes: &HashMap<LocalId, String>,
        current_class: Option<&str>,
    ) -> Option<&'a (String, String)> {
        match receiver {
            Expr::This => {
                let class_name = current_class?;
                class_field_natives.get(class_name)?.get(field)
            }
            Expr::LocalGet(id) => {
                let class_name = user_classes.get(id)?;
                class_field_natives.get(class_name)?.get(field)
            }
            _ => None,
        }
    }

    match expr {
        Expr::Call { callee, args, .. } => {
            // `<receiver>.<field>.<method>(args)` → NativeMethodCall.
            // First check if the call shape matches and capture the
            // owned data we'd need; if so, build the replacement after
            // the borrow ends so we can `*expr = ...` cleanly.
            enum CallRewrite {
                Direct {
                    module: String,
                    class: String,
                    method: String,
                },
                Chained {
                    module: String,
                    result_class: String,
                    method: String,
                },
            }
            let rewrite: Option<CallRewrite> = match callee.as_mut() {
                Expr::PropertyGet {
                    object: outer_obj,
                    property: method_name,
                } => {
                    let mut direct = None;
                    if let Expr::PropertyGet {
                        object: inner_obj,
                        property: field_name,
                    } = outer_obj.as_ref()
                    {
                        if let Some((module_name, class_name)) = lookup_field_native(
                            inner_obj.as_ref(),
                            field_name,
                            class_field_natives,
                            user_classes,
                            current_class,
                        ) {
                            direct = Some(CallRewrite::Direct {
                                module: module_name.clone(),
                                class: class_name.clone(),
                                method: method_name.clone(),
                            });
                        }
                    }
                    if direct.is_none() {
                        // Recurse into the receiver so any inner
                        // `this.<field>.method()` rewrites land before
                        // we examine the chain.
                        fix_class_field_expr(
                            outer_obj.as_mut(),
                            class_field_natives,
                            user_classes,
                            current_class,
                        );
                        if let Expr::NativeMethodCall {
                            module: prev_module,
                            method: prior_method,
                            ..
                        } = outer_obj.as_ref()
                        {
                            if let Some(result_class) =
                                chained_native_class(prev_module, prior_method)
                            {
                                direct = Some(CallRewrite::Chained {
                                    module: prev_module.clone(),
                                    result_class: result_class.to_string(),
                                    method: method_name.clone(),
                                });
                            }
                        }
                    }
                    direct
                }
                _ => {
                    fix_class_field_expr(callee, class_field_natives, user_classes, current_class);
                    None
                }
            };

            for arg in args.iter_mut() {
                fix_class_field_expr(arg, class_field_natives, user_classes, current_class);
            }

            if let Some(rw) = rewrite {
                let args_owned: Vec<Expr> = std::mem::take(args);
                // Extract receiver: the inner PropertyGet's object (for Direct)
                // or the outer object itself (for Chained — the whole
                // NativeMethodCall is the receiver).
                let receiver = if let Expr::PropertyGet {
                    object: outer_obj, ..
                } = callee.as_mut()
                {
                    match &rw {
                        CallRewrite::Direct { .. } => {
                            // Replace the outer_obj (which is a
                            // PropertyGet { This|local, field }) with
                            // Undefined and use it as the receiver.
                            std::mem::replace(outer_obj.as_mut(), Expr::Undefined)
                        }
                        CallRewrite::Chained { .. } => {
                            // outer_obj is itself a NativeMethodCall —
                            // use it as the receiver.
                            std::mem::replace(outer_obj.as_mut(), Expr::Undefined)
                        }
                    }
                } else {
                    Expr::Undefined
                };
                *expr = match rw {
                    CallRewrite::Direct {
                        module,
                        class,
                        method,
                    } => Expr::NativeMethodCall {
                        module,
                        class_name: Some(class),
                        object: Some(Box::new(receiver)),
                        method,
                        args: args_owned,
                    },
                    CallRewrite::Chained {
                        module,
                        result_class,
                        method,
                    } => Expr::NativeMethodCall {
                        module,
                        class_name: Some(result_class),
                        object: Some(Box::new(receiver)),
                        method,
                        args: args_owned,
                    },
                };
            }
        }
        Expr::Await(inner) => {
            // `await <receiver>.<field>.<method>(args)`
            if let Expr::Call { callee, args, .. } = inner.as_mut() {
                if let Expr::PropertyGet {
                    object: outer_obj,
                    property: method_name,
                } = callee.as_mut()
                {
                    if let Expr::PropertyGet {
                        object: inner_obj,
                        property: field_name,
                    } = outer_obj.as_ref()
                    {
                        if let Some((module_name, class_name)) = lookup_field_native(
                            inner_obj.as_ref(),
                            field_name,
                            class_field_natives,
                            user_classes,
                            current_class,
                        ) {
                            for arg in args.iter_mut() {
                                fix_class_field_expr(
                                    arg,
                                    class_field_natives,
                                    user_classes,
                                    current_class,
                                );
                            }
                            let args_owned: Vec<Expr> = std::mem::take(args);
                            let receiver = std::mem::replace(outer_obj.as_mut(), Expr::Undefined);
                            let module_owned = module_name.clone();
                            let class_owned = class_name.clone();
                            let method_owned = method_name.clone();
                            *inner.as_mut() = Expr::NativeMethodCall {
                                module: module_owned,
                                class_name: Some(class_owned),
                                object: Some(Box::new(receiver)),
                                method: method_owned,
                                args: args_owned,
                            };
                            return;
                        }
                    }
                }
            }
            fix_class_field_expr(inner, class_field_natives, user_classes, current_class);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            fix_class_field_expr(left, class_field_natives, user_classes, current_class);
            fix_class_field_expr(right, class_field_natives, user_classes, current_class);
        }
        Expr::Unary { operand, .. } => {
            fix_class_field_expr(operand, class_field_natives, user_classes, current_class);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fix_class_field_expr(condition, class_field_natives, user_classes, current_class);
            fix_class_field_expr(then_expr, class_field_natives, user_classes, current_class);
            fix_class_field_expr(else_expr, class_field_natives, user_classes, current_class);
        }
        Expr::PropertyGet { object, .. } => {
            fix_class_field_expr(object, class_field_natives, user_classes, current_class);
        }
        Expr::PropertySet { object, value, .. } => {
            fix_class_field_expr(object, class_field_natives, user_classes, current_class);
            fix_class_field_expr(value, class_field_natives, user_classes, current_class);
        }
        Expr::IndexGet { object, index } => {
            fix_class_field_expr(object, class_field_natives, user_classes, current_class);
            fix_class_field_expr(index, class_field_natives, user_classes, current_class);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fix_class_field_expr(object, class_field_natives, user_classes, current_class);
            fix_class_field_expr(index, class_field_natives, user_classes, current_class);
            fix_class_field_expr(value, class_field_natives, user_classes, current_class);
        }
        Expr::Array(items) => {
            for item in items {
                fix_class_field_expr(item, class_field_natives, user_classes, current_class);
            }
        }
        Expr::Object(fields) => {
            for (_, value) in fields {
                fix_class_field_expr(value, class_field_natives, user_classes, current_class);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                fix_class_field_expr(value, class_field_natives, user_classes, current_class);
            }
        }
        Expr::New { args, .. } => {
            for arg in args {
                fix_class_field_expr(arg, class_field_natives, user_classes, current_class);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fix_class_field_expr(obj, class_field_natives, user_classes, current_class);
            }
            for arg in args {
                fix_class_field_expr(arg, class_field_natives, user_classes, current_class);
            }
        }
        _ => {}
    }
}

/// Issue #341: native-module method-chain table — when a method on a
/// known native class returns a *new* native instance, the outer chained
/// call needs to dispatch as a `NativeMethodCall` against the result
/// class. Mirrors the lower-time chaining tables in
/// `expr_call.rs::lower_expr` (the multiple `("better-sqlite3", "prepare")
/// → Some("Statement")` arms) — kept in sync by hand. Returns the
/// produced native class name, or `None` if the chain doesn't propagate.
pub fn chained_native_class(module: &str, prior_method: &str) -> Option<&'static str> {
    match (module, prior_method) {
        ("better-sqlite3", "prepare") => Some("Statement"),
        ("sqlite", "prepare") => Some("StatementSync"),
        ("sqlite", "createTagStore") => Some("SQLTagStore"),
        ("sqlite", "createSession") => Some("Session"),
        ("mongodb", "db") => Some("Database"),
        ("mongodb", "collection") => Some("Collection"),
        ("mysql2", "getConnection") | ("mysql2/promise", "getConnection") => Some("PoolConnection"),
        ("pg", "connect") => Some("PoolClient"),
        ("ioredis", "duplicate") => Some("Redis"),
        // dayjs / moment manipulation methods return a NEW date handle.
        ("dayjs", "add" | "subtract" | "startOf" | "endOf") => Some("App"),
        ("moment", "add" | "subtract" | "startOf" | "endOf" | "clone") => Some("App"),
        _ => None,
    }
}

/// Issue #341: walk a statement looking for `this.<field> = <native creation>`
/// patterns inside class constructors. Records the field name so subsequent
/// method bodies can rewrite `this.<field>.method(...)` calls to
/// `NativeMethodCall`. Recurses through control-flow constructs (if/while/
/// for/try) so guarded assignments still register.
pub fn scan_stmt_for_field_native_instances(
    stmt: &Stmt,
    field_instances: &mut HashMap<String, (String, String)>,
) {
    match stmt {
        Stmt::Expr(Expr::PropertySet {
            object,
            property,
            value,
        }) => {
            if matches!(object.as_ref(), Expr::This) {
                if let Some((module_name, class_name)) =
                    detect_native_instance_creation_with_context(value, &HashMap::new())
                {
                    field_instances.insert(property.clone(), (module_name, class_name));
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                scan_stmt_for_field_native_instances(s, field_instances);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    scan_stmt_for_field_native_instances(s, field_instances);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                scan_stmt_for_field_native_instances(s, field_instances);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                scan_stmt_for_field_native_instances(init_stmt.as_ref(), field_instances);
            }
            for s in body {
                scan_stmt_for_field_native_instances(s, field_instances);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                scan_stmt_for_field_native_instances(s, field_instances);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    scan_stmt_for_field_native_instances(s, field_instances);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    scan_stmt_for_field_native_instances(s, field_instances);
                }
            }
        }
        _ => {}
    }
}

/// Recursively scan a statement for native instance creations (Let assignments)
pub fn scan_stmt_for_native_instances(
    stmt: &Stmt,
    local_names: &mut HashMap<String, (String, String)>,
    local_ids: &mut HashMap<LocalId, (String, String)>,
) {
    match stmt {
        Stmt::Let {
            id,
            name,
            init: Some(init_expr),
            ..
        } => {
            if let Some((native_module, class_name)) =
                detect_native_instance_creation_with_context(init_expr, local_ids)
            {
                local_names.insert(name.clone(), (native_module.clone(), class_name.clone()));
                local_ids.insert(*id, (native_module, class_name));
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                scan_stmt_for_native_instances(s, local_names, local_ids);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    scan_stmt_for_native_instances(s, local_names, local_ids);
                }
            }
        }
        Stmt::While { body, .. } => {
            for s in body {
                scan_stmt_for_native_instances(s, local_names, local_ids);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                scan_stmt_for_native_instances(init_stmt.as_ref(), local_names, local_ids);
            }
            for s in body {
                scan_stmt_for_native_instances(s, local_names, local_ids);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                scan_stmt_for_native_instances(s, local_names, local_ids);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    scan_stmt_for_native_instances(s, local_names, local_ids);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    scan_stmt_for_native_instances(s, local_names, local_ids);
                }
            }
        }
        _ => {}
    }
}

pub fn fix_native_instance_stmt_with_locals(
    stmt: &mut Stmt,
    native_instances: &HashMap<String, (String, String)>,
    local_id_instances: &HashMap<LocalId, (String, String)>,
) {
    match stmt {
        Stmt::Expr(expr) => {
            fix_native_instance_expr_with_locals(expr, native_instances, local_id_instances)
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                fix_native_instance_expr_with_locals(e, native_instances, local_id_instances);
            }
        }
        Stmt::Return(Some(e)) => {
            fix_native_instance_expr_with_locals(e, native_instances, local_id_instances)
        }
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fix_native_instance_expr_with_locals(condition, native_instances, local_id_instances);
            for s in then_branch {
                fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::While { condition, body } => {
            fix_native_instance_expr_with_locals(condition, native_instances, local_id_instances);
            for s in body {
                fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
            }
            fix_native_instance_expr_with_locals(condition, native_instances, local_id_instances);
        }
        Stmt::Labeled { body, .. } => {
            fix_native_instance_stmt_with_locals(body, native_instances, local_id_instances);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                fix_native_instance_stmt_with_locals(
                    init_stmt.as_mut(),
                    native_instances,
                    local_id_instances,
                );
            }
            if let Some(cond) = condition {
                fix_native_instance_expr_with_locals(cond, native_instances, local_id_instances);
            }
            if let Some(upd) = update {
                fix_native_instance_expr_with_locals(upd, native_instances, local_id_instances);
            }
            for s in body {
                fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
            }
            if let Some(ref mut catch_clause) = catch {
                for s in &mut catch_clause.body {
                    fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::Throw(e) => {
            fix_native_instance_expr_with_locals(e, native_instances, local_id_instances)
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fix_native_instance_expr_with_locals(
                discriminant,
                native_instances,
                local_id_instances,
            );
            for case in cases {
                if let Some(test) = &mut case.test {
                    fix_native_instance_expr_with_locals(
                        test,
                        native_instances,
                        local_id_instances,
                    );
                }
                for s in &mut case.body {
                    fix_native_instance_stmt_with_locals(s, native_instances, local_id_instances);
                }
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

pub fn fix_native_instance_expr_with_locals(
    expr: &mut Expr,
    native_instances: &HashMap<String, (String, String)>,
    local_id_instances: &HashMap<LocalId, (String, String)>,
) {
    match expr {
        // The key case: method calls that might be on native instances
        Expr::Call { callee, args, .. } => {
            let mut recursed_into_property_object = false;
            // Issue #1193: `$(selector)` where `$` is a CheerioAPI handle.
            // Cheerio's only "call-as-function" shape is `load(html)`'s
            // return value used as a selector — rewrite to the existing
            // `cheerio.select` native row.
            if let Expr::LocalGet(local_id) = callee.as_ref() {
                if let Some((module, class)) = local_id_instances.get(local_id) {
                    if module == "cheerio" && class == "CheerioAPI" {
                        for arg in args.iter_mut() {
                            fix_native_instance_expr_with_locals(
                                arg,
                                native_instances,
                                local_id_instances,
                            );
                        }
                        let args_owned: Vec<Expr> = std::mem::take(args);
                        let object_expr = std::mem::replace(callee.as_mut(), Expr::Undefined);
                        *expr = Expr::NativeMethodCall {
                            module: "cheerio".to_string(),
                            class_name: Some("CheerioAPI".to_string()),
                            object: Some(Box::new(object_expr)),
                            method: "select".to_string(),
                            args: args_owned,
                        };
                        return;
                    }
                }
            }

            // Check if this is a method call: obj.method(args)
            if let Expr::PropertyGet { object, property } = callee.as_mut() {
                // Check for LocalGet (local variable)
                if let Expr::LocalGet(local_id) = object.as_ref() {
                    let found = local_id_instances.get(local_id);
                    if let Some((native_module, native_class)) = found {
                        // Transform args first
                        for arg in args.iter_mut() {
                            fix_native_instance_expr_with_locals(
                                arg,
                                native_instances,
                                local_id_instances,
                            );
                        }
                        let args_owned: Vec<Expr> = std::mem::take(args);
                        let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                        // Transform to NativeMethodCall
                        *expr = Expr::NativeMethodCall {
                            module: native_module.clone(),
                            class_name: Some(native_class.clone()),
                            object: Some(Box::new(object_expr)),
                            method: property.clone(),
                            args: args_owned,
                        };
                        return;
                    }
                }
                // Check for ExternFuncRef (imported native instance)
                if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                    if let Some((native_module, native_class)) = native_instances.get(name) {
                        // Transform args first
                        for arg in args.iter_mut() {
                            fix_native_instance_expr_with_locals(
                                arg,
                                native_instances,
                                local_id_instances,
                            );
                        }
                        let args_owned: Vec<Expr> = std::mem::take(args);
                        let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                        // Transform to NativeMethodCall
                        *expr = Expr::NativeMethodCall {
                            module: native_module.clone(),
                            class_name: Some(native_class.clone()),
                            object: Some(Box::new(object_expr)),
                            method: property.clone(),
                            args: args_owned,
                        };
                        return;
                    }
                }
                // Issue #1193: chained `$(sel).text()` / `$(sel).find(...).html()`.
                // Recurse into the object first so any nested `$(sel)` Call
                // has already been rewritten to a cheerio NativeMethodCall.
                // Then if the (rewritten) object is a cheerio call that
                // returns a CheerioSelection, rewrite the outer .method(args)
                // through the cheerio dispatch row too.
                if matches!(
                    object.as_ref(),
                    Expr::Call { .. } | Expr::NativeMethodCall { .. }
                ) {
                    fix_native_instance_expr_with_locals(
                        object,
                        native_instances,
                        local_id_instances,
                    );
                    recursed_into_property_object = true;
                    if let Expr::NativeMethodCall {
                        module: inner_module,
                        method: inner_method,
                        ..
                    } = object.as_ref()
                    {
                        if inner_module == "cheerio"
                            && cheerio_returns_selection(inner_method.as_str())
                        {
                            for arg in args.iter_mut() {
                                fix_native_instance_expr_with_locals(
                                    arg,
                                    native_instances,
                                    local_id_instances,
                                );
                            }
                            let args_owned: Vec<Expr> = std::mem::take(args);
                            let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);
                            *expr = Expr::NativeMethodCall {
                                module: "cheerio".to_string(),
                                class_name: Some("CheerioSelection".to_string()),
                                object: Some(Box::new(object_expr)),
                                method: property.clone(),
                                args: args_owned,
                            };
                            return;
                        }
                    }
                }
            }

            // Not a native instance call, recurse
            if recursed_into_property_object {
                // The fluent-chain case above already walked the receiver
                // of this property call. Walking the whole callee again would
                // revisit the same receiver at every chain level, making long
                // non-native fluent chains exponential.
            } else {
                fix_native_instance_expr_with_locals(callee, native_instances, local_id_instances);
            }
            for arg in args {
                fix_native_instance_expr_with_locals(arg, native_instances, local_id_instances);
            }
        }
        Expr::Await(inner) => {
            // Handle Await(Call{PropertyGet{LocalGet...}}) pattern for async method calls
            if let Expr::Call { callee, args, .. } = inner.as_mut() {
                if let Expr::PropertyGet { object, property } = callee.as_mut() {
                    // Check for LocalGet
                    if let Expr::LocalGet(local_id) = object.as_ref() {
                        if let Some((native_module, native_class)) =
                            local_id_instances.get(local_id)
                        {
                            // Transform args first
                            for arg in args.iter_mut() {
                                fix_native_instance_expr_with_locals(
                                    arg,
                                    native_instances,
                                    local_id_instances,
                                );
                            }
                            let args_owned: Vec<Expr> = std::mem::take(args);
                            let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                            // Replace the inner Call with NativeMethodCall (wrapped by Await)
                            *inner.as_mut() = Expr::NativeMethodCall {
                                module: native_module.clone(),
                                class_name: Some(native_class.clone()),
                                object: Some(Box::new(object_expr)),
                                method: property.clone(),
                                args: args_owned,
                            };
                            return;
                        }
                    }
                    // Check for ExternFuncRef
                    if let Expr::ExternFuncRef { name, .. } = object.as_ref() {
                        if let Some((native_module, native_class)) = native_instances.get(name) {
                            // Transform args first
                            for arg in args.iter_mut() {
                                fix_native_instance_expr_with_locals(
                                    arg,
                                    native_instances,
                                    local_id_instances,
                                );
                            }
                            let args_owned: Vec<Expr> = std::mem::take(args);
                            let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);

                            // Replace the inner Call with NativeMethodCall (wrapped by Await)
                            *inner.as_mut() = Expr::NativeMethodCall {
                                module: native_module.clone(),
                                class_name: Some(native_class.clone()),
                                object: Some(Box::new(object_expr)),
                                method: property.clone(),
                                args: args_owned,
                            };
                            return;
                        }
                    }
                }
            }
            fix_native_instance_expr_with_locals(inner, native_instances, local_id_instances);
        }
        // Recurse into other expressions
        Expr::Binary { left, right, .. } => {
            fix_native_instance_expr_with_locals(left, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(right, native_instances, local_id_instances);
        }
        Expr::Unary { operand, .. } => {
            fix_native_instance_expr_with_locals(operand, native_instances, local_id_instances);
        }
        Expr::Logical { left, right, .. } => {
            fix_native_instance_expr_with_locals(left, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(right, native_instances, local_id_instances);
        }
        Expr::Compare { left, right, .. } => {
            fix_native_instance_expr_with_locals(left, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(right, native_instances, local_id_instances);
        }
        Expr::LocalSet(_, value) => {
            fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
        }
        Expr::GlobalSet(_, value) => {
            fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fix_native_instance_expr_with_locals(condition, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(then_expr, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(else_expr, native_instances, local_id_instances);
        }
        Expr::Array(elements) => {
            for elem in elements {
                fix_native_instance_expr_with_locals(elem, native_instances, local_id_instances);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    crate::ir::ArrayElement::Expr(e) => fix_native_instance_expr_with_locals(
                        e,
                        native_instances,
                        local_id_instances,
                    ),
                    crate::ir::ArrayElement::Spread(e) => fix_native_instance_expr_with_locals(
                        e,
                        native_instances,
                        local_id_instances,
                    ),
                    crate::ir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(properties) => {
            for (_, value) in properties {
                fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
            }
        }
        Expr::PropertyGet { object, property } => {
            // Recurse into the object first so any nested `$(sel)` Call has
            // been rewritten to a cheerio NativeMethodCall.
            fix_native_instance_expr_with_locals(object, native_instances, local_id_instances);
            // Issue #1193: `$(sel).length` reads a property in JS but the
            // cheerio dispatch row models it as a zero-arg method. Rewrite
            // to a NativeMethodCall so codegen routes to js_cheerio_selection_length.
            if property == "length" {
                if let Expr::NativeMethodCall {
                    module: inner_module,
                    method: inner_method,
                    ..
                } = object.as_ref()
                {
                    if inner_module == "cheerio" && cheerio_returns_selection(inner_method.as_str())
                    {
                        let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);
                        *expr = Expr::NativeMethodCall {
                            module: "cheerio".to_string(),
                            class_name: Some("CheerioSelection".to_string()),
                            object: Some(Box::new(object_expr)),
                            method: "length".to_string(),
                            args: Vec::new(),
                        };
                    }
                }
            }
        }
        Expr::PropertySet { object, value, .. } => {
            fix_native_instance_expr_with_locals(object, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
        }
        Expr::IndexGet { object, index } => {
            fix_native_instance_expr_with_locals(object, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(index, native_instances, local_id_instances);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fix_native_instance_expr_with_locals(object, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(index, native_instances, local_id_instances);
            fix_native_instance_expr_with_locals(value, native_instances, local_id_instances);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fix_native_instance_expr_with_locals(obj, native_instances, local_id_instances);
            }
            for arg in args {
                fix_native_instance_expr_with_locals(arg, native_instances, local_id_instances);
            }
        }
        Expr::New { args, .. } | Expr::SuperCall(args) => {
            for arg in args {
                fix_native_instance_expr_with_locals(arg, native_instances, local_id_instances);
            }
        }
        _ => {}
    }
}

/// Issue #1193: cheerio methods whose return value is another
/// `CheerioSelection`. Used by the chained-call rewriter so e.g.
/// `$(sel).first().text()` keeps dispatching through the cheerio
/// NativeMethodCall path instead of falling through to the generic
/// "value is not a function" runtime error.
fn cheerio_returns_selection(method: &str) -> bool {
    matches!(
        method,
        "select" | "find" | "children" | "parent" | "first" | "last" | "eq"
    )
}

/// Detect if an expression is creating a native module instance (with context for local variables)
/// Returns Some((module_name, class_name)) if it is
pub fn detect_native_instance_creation_with_context(
    expr: &Expr,
    local_ids: &HashMap<LocalId, (String, String)>,
) -> Option<(String, String)> {
    match expr {
        Expr::NativeMethodCall {
            module,
            object: None,
            method,
            ..
        } => {
            // Creation functions like mysql.createPool(), mysql.createConnection().
            // The mapping is module-aware: `createConnection` returns a different
            // class shape per module (`net` → Socket, `mysql2` → Connection), and
            // a method-name-only match silently mis-tags `net.createConnection(...)`
            // as ("net", "Connection"), which then misses NATIVE_MODULE_TABLE's
            // `class_filter: Some("Socket")` rows so `s.write(...)` / `s.on(...)`
            // fall through to the silent "Unknown native method" sentinel and
            // never reach perry-ext-net's FFI. Closes #647.
            let class_name = match (module.as_str(), method.as_str()) {
                ("mysql2" | "mysql2/promise", "createPool") => "Pool",
                ("mysql2" | "mysql2/promise", "createConnection") => "Connection",
                ("net", "createConnection" | "connect") => "Socket",
                ("tls", "connect") => "Socket",
                ("tls", "createServer" | "Server") => "Server",
                ("net", "Socket") => "Socket",
                ("pg", "connect") => "Client",
                ("http" | "https", "request" | "get") => "ClientRequest",
                ("http", "createServer") => "HttpServer",
                ("https", "createServer") => "HttpsServer",
                ("http2", "createSecureServer") => "Http2SecureServer",
                ("node-cron", "schedule") => "CronJob",
                ("readline", "createInterface") => "Interface",
                // Issue #1193: `const $ = load(html)` / `loadFragment(html)`
                // returns the jQuery-like callable used as `$(selector)`.
                // Tagging the local as CheerioAPI lets the rewriter below
                // turn `$(sel)` into `NativeMethodCall(cheerio.select, $)`.
                ("cheerio", "load" | "loadFragment") => "CheerioAPI",
                _ => return None,
            };
            // For ("net", _) / ("tls", _) factories, `s` belongs to net.Socket's
            // dispatch — register under "net" regardless of which module the
            // factory call lived in (mirrors lower.rs:5517).
            let owning_module = match (module.as_str(), method.as_str()) {
                ("tls", "connect") => "net".to_string(),
                ("https", "request" | "get") => "http".to_string(),
                _ => module.clone(),
            };
            Some((owning_module, class_name.to_string()))
        }
        Expr::NativeMethodCall {
            module,
            object: Some(_),
            class_name: Some(class),
            method,
            ..
        } => {
            // Instance methods that return new native instances
            match (module.as_str(), class.as_str(), method.as_str()) {
                ("mysql2" | "mysql2/promise", "Pool", "getConnection") => {
                    Some((module.clone(), "PoolConnection".to_string()))
                }
                ("pg", "Pool", "connect") => Some((module.clone(), "PoolClient".to_string())),
                ("ioredis", "Redis", "duplicate") => Some((module.clone(), "Redis".to_string())),
                ("better-sqlite3", "Database", "prepare") => {
                    Some((module.clone(), "Statement".to_string()))
                }
                ("sqlite", "DatabaseSync", "prepare") => {
                    Some((module.clone(), "StatementSync".to_string()))
                }
                ("sqlite", "DatabaseSync", "createTagStore") => {
                    Some((module.clone(), "SQLTagStore".to_string()))
                }
                ("sqlite", "DatabaseSync", "createSession") => {
                    Some((module.clone(), "Session".to_string()))
                }
                _ => None,
            }
        }
        // Handle Call expressions where the object is a known native instance
        // This is the pattern BEFORE transformation: pool.getConnection()
        Expr::Call { callee, .. } => {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                // Check if object is a LocalGet of a known native instance
                if let Expr::LocalGet(local_id) = object.as_ref() {
                    if let Some((module, class)) = local_ids.get(local_id) {
                        // Check if this method returns a native instance
                        return match (module.as_str(), class.as_str(), property.as_str()) {
                            ("mysql2" | "mysql2/promise", "Pool", "getConnection") => {
                                Some((module.clone(), "PoolConnection".to_string()))
                            }
                            ("pg", "Pool", "connect") => {
                                Some((module.clone(), "PoolClient".to_string()))
                            }
                            ("ioredis", "Redis", "duplicate") => {
                                Some((module.clone(), "Redis".to_string()))
                            }
                            ("better-sqlite3", "Database", "prepare") => {
                                Some((module.clone(), "Statement".to_string()))
                            }
                            ("sqlite", "DatabaseSync", "prepare") => {
                                Some((module.clone(), "StatementSync".to_string()))
                            }
                            ("sqlite", "DatabaseSync", "createTagStore") => {
                                Some((module.clone(), "SQLTagStore".to_string()))
                            }
                            ("sqlite", "DatabaseSync", "createSession") => {
                                Some((module.clone(), "Session".to_string()))
                            }
                            _ => None,
                        };
                    }
                }
            }
            // Check for global fetch() call
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if name == "fetch" {
                    // fetch() returns a Response
                    return Some(("fetch".to_string(), "Response".to_string()));
                }
            }
            None
        }
        Expr::New { class_name, .. } => {
            // new Database(...) → better-sqlite3 Database instance
            // new DatabaseSync(...) → node:sqlite DatabaseSync instance
            // (#3183); both reuse the rusqlite backend, tagged under
            // distinct modules so NativeModSig dispatch routes correctly.
            match class_name.as_str() {
                "Database" => Some(("better-sqlite3".to_string(), "Database".to_string())),
                "DatabaseSync" => Some(("sqlite".to_string(), "DatabaseSync".to_string())),
                "StatementSync" => Some(("sqlite".to_string(), "StatementSync".to_string())),
                _ => None,
            }
        }
        Expr::Await(inner) => {
            // Async creation: await mysql.createConnection() or await pool.getConnection() or await fetch()
            detect_native_instance_creation_with_context(inner, local_ids)
        }
        _ => None,
    }
}
