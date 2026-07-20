use crate::ir::{Expr, Module, ModuleKind, Stmt};
use perry_types::LocalId;
use std::collections::{HashMap, HashSet};

/// Information about a JavaScript module import
#[derive(Debug, Clone)]
pub struct JsImportInfo {
    /// Local variable ID for the module handle
    pub handle_var_id: LocalId,
    /// Path to the JS module file
    pub path: String,
    /// Mapping from exported name to local variable name
    pub exports: HashMap<String, String>,
}

/// Context for tracking JS values during transformation
#[derive(Debug, Clone, Default)]
struct JsValueTracker {
    /// LocalIds that hold JS values (from imports or JS function results)
    js_locals: HashSet<LocalId>,
    /// Class names that are JS classes (from imports)
    js_classes: HashSet<String>,
}

impl JsValueTracker {
    fn new() -> Self {
        Self::default()
    }

    fn mark_js_local(&mut self, id: LocalId) {
        self.js_locals.insert(id);
    }

    fn is_js_local(&self, id: LocalId) -> bool {
        self.js_locals.contains(&id)
    }

    fn mark_js_class(&mut self, name: &str) {
        self.js_classes.insert(name.to_string());
    }

    fn is_js_class(&self, name: &str) -> bool {
        self.js_classes.contains(name)
    }
}

/// Transform JavaScript module imports into V8 runtime calls
///
/// This function modifies the module in place:
/// - Adds variables to store module handles
/// - Adds init statements to load modules
/// - Transforms calls to imported functions
/// - Transforms method calls and property access on JS objects
/// - Transforms new expressions for JS classes
pub fn transform_js_imports(module: &mut Module) {
    // Collect JS imports and their specifiers
    let mut js_imports: HashMap<String, JsImportInfo> = HashMap::new();
    let mut next_handle_id: u32 = 50000; // Start with high ID to avoid conflicts

    // Map from local variable name to (module_source, export_name)
    let mut local_name_to_js: HashMap<String, (String, String)> = HashMap::new();
    // Map from ExternFuncRef name to (module_source, export_name)
    let mut extern_func_to_js: HashMap<String, (String, String)> = HashMap::new();

    // Track JS value origins
    let mut tracker = JsValueTracker::new();

    for import in &module.imports {
        if import.module_kind == ModuleKind::Interpreted {
            let path = import
                .resolved_path
                .clone()
                .unwrap_or(import.source.clone());
            let mut exports = HashMap::new();

            for spec in &import.specifiers {
                match spec {
                    crate::ir::ImportSpecifier::Named { imported, local } => {
                        exports.insert(imported.clone(), local.clone());
                        extern_func_to_js
                            .insert(imported.clone(), (import.source.clone(), imported.clone()));
                        local_name_to_js
                            .insert(local.clone(), (import.source.clone(), imported.clone()));
                        // If this looks like a class name (starts with uppercase), mark it
                        if local
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false)
                        {
                            tracker.mark_js_class(local);
                        }
                    }
                    crate::ir::ImportSpecifier::Default { local } => {
                        exports.insert("default".to_string(), local.clone());
                        extern_func_to_js.insert(
                            local.clone(),
                            (import.source.clone(), "default".to_string()),
                        );
                        local_name_to_js.insert(
                            local.clone(),
                            (import.source.clone(), "default".to_string()),
                        );
                        // Default exports with uppercase names are likely classes
                        if local
                            .chars()
                            .next()
                            .map(|c| c.is_uppercase())
                            .unwrap_or(false)
                        {
                            tracker.mark_js_class(local);
                        }
                    }
                    crate::ir::ImportSpecifier::Namespace { local } => {
                        exports.insert("*".to_string(), local.clone());
                        extern_func_to_js
                            .insert(local.clone(), (import.source.clone(), "*".to_string()));
                        local_name_to_js
                            .insert(local.clone(), (import.source.clone(), "*".to_string()));
                    }
                }
            }

            js_imports.insert(
                import.source.clone(),
                JsImportInfo {
                    handle_var_id: next_handle_id,
                    path,
                    exports,
                },
            );
            next_handle_id += 1;
        }
    }

    if js_imports.is_empty() {
        return;
    }

    // Note: We no longer create Let statements for module handles.
    // Instead, JsLoadModule expressions are inlined directly where module handles are needed.
    // V8 caches loaded modules internally, so this is efficient.

    // Transform all statements
    transform_stmts(
        &mut module.init,
        &js_imports,
        &extern_func_to_js,
        &local_name_to_js,
        &mut tracker,
    );

    for func in &mut module.functions {
        let mut func_tracker = tracker.clone();
        transform_stmts(
            &mut func.body,
            &js_imports,
            &extern_func_to_js,
            &local_name_to_js,
            &mut func_tracker,
        );
    }

    for class in &mut module.classes {
        for method in &mut class.methods {
            let mut method_tracker = tracker.clone();
            transform_stmts(
                &mut method.body,
                &js_imports,
                &extern_func_to_js,
                &local_name_to_js,
                &mut method_tracker,
            );
        }
        for (_, getter) in &mut class.getters {
            let mut getter_tracker = tracker.clone();
            transform_stmts(
                &mut getter.body,
                &js_imports,
                &extern_func_to_js,
                &local_name_to_js,
                &mut getter_tracker,
            );
        }
        for (_, setter) in &mut class.setters {
            let mut setter_tracker = tracker.clone();
            transform_stmts(
                &mut setter.body,
                &js_imports,
                &extern_func_to_js,
                &local_name_to_js,
                &mut setter_tracker,
            );
        }
        for method in &mut class.static_methods {
            let mut method_tracker = tracker.clone();
            transform_stmts(
                &mut method.body,
                &js_imports,
                &extern_func_to_js,
                &local_name_to_js,
                &mut method_tracker,
            );
        }
        if let Some(ctor) = &mut class.constructor {
            let mut ctor_tracker = tracker.clone();
            transform_stmts(
                &mut ctor.body,
                &js_imports,
                &extern_func_to_js,
                &local_name_to_js,
                &mut ctor_tracker,
            );
        }
    }
}

pub fn transform_stmts(
    stmts: &mut Vec<Stmt>,
    js_imports: &HashMap<String, JsImportInfo>,
    extern_func_to_js: &HashMap<String, (String, String)>,
    local_name_to_js: &HashMap<String, (String, String)>,
    tracker: &mut JsValueTracker,
) {
    for stmt in stmts.iter_mut() {
        transform_stmt(
            stmt,
            js_imports,
            extern_func_to_js,
            local_name_to_js,
            tracker,
        );
    }
}

pub fn transform_stmt(
    stmt: &mut Stmt,
    js_imports: &HashMap<String, JsImportInfo>,
    extern_func_to_js: &HashMap<String, (String, String)>,
    local_name_to_js: &HashMap<String, (String, String)>,
    tracker: &mut JsValueTracker,
) {
    match stmt {
        Stmt::Expr(expr) => {
            transform_expr(
                expr,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::Let {
            id,
            init: Some(expr),
            ..
        } => {
            transform_expr(
                expr,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            // If the init expression produces a JS value, mark this local as JS
            if is_js_value_expr(expr, tracker) {
                tracker.mark_js_local(*id);
            }
        }
        Stmt::Let { init: None, .. } => {}
        Stmt::Return(Some(expr)) => {
            transform_expr(
                expr,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            transform_expr(
                condition,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            transform_stmts(
                then_branch,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            if let Some(else_b) = else_branch {
                transform_stmts(
                    else_b,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
        }
        Stmt::While { condition, body } => {
            transform_expr(
                condition,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            transform_stmts(
                body,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::DoWhile { body, condition } => {
            transform_stmts(
                body,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            transform_expr(
                condition,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                transform_stmt(
                    init_stmt,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
            if let Some(cond) = condition {
                transform_expr(
                    cond,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
            if let Some(upd) = update {
                transform_expr(
                    upd,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
            transform_stmts(
                body,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::Labeled { body, .. } => {
            transform_stmt(
                body,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            transform_expr(
                discriminant,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            for case in cases {
                if let Some(test) = &mut case.test {
                    transform_expr(
                        test,
                        js_imports,
                        extern_func_to_js,
                        local_name_to_js,
                        tracker,
                    );
                }
                transform_stmts(
                    &mut case.body,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
        }
        Stmt::Throw(expr) => {
            transform_expr(
                expr,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            transform_stmts(
                body,
                js_imports,
                extern_func_to_js,
                local_name_to_js,
                tracker,
            );
            if let Some(catch_clause) = catch {
                transform_stmts(
                    &mut catch_clause.body,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
            if let Some(finally_body) = finally {
                transform_stmts(
                    finally_body,
                    js_imports,
                    extern_func_to_js,
                    local_name_to_js,
                    tracker,
                );
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

/// Check if an expression produces a JS value
pub fn is_js_value_expr(expr: &Expr, tracker: &JsValueTracker) -> bool {
    match expr {
        // Direct JS interop expressions
        Expr::JsLoadModule { .. } => true,
        Expr::JsGetExport { .. } => true,
        Expr::JsCallFunction { .. } => true,
        Expr::JsCallMethod { .. } => true,
        Expr::JsCallValue { .. } => true,
        Expr::JsGetProperty { .. } => true,
        Expr::JsNew { .. } => true,
        Expr::JsNewFromHandle { .. } => true,
        Expr::JsCreateCallback { .. } => true,
        // Local variables that are known to be JS values
        Expr::LocalGet(id) => tracker.is_js_local(*id),
        // Property access on JS objects returns JS values
        Expr::PropertyGet { object, .. } => is_js_value_expr(object, tracker),
        // Calls that return JS objects (e.g., chained method calls or require())
        Expr::Call { callee, .. } => {
            match callee.as_ref() {
                // require() call - GlobalGet(0) is typically require
                Expr::GlobalGet(0) => true,
                // If the callee is a property get on a JS object, the result is likely JS
                Expr::PropertyGet { object, .. } => is_js_value_expr(object, tracker),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Check if an expression is a JS object (for method calls)
pub fn is_js_object_expr(
    expr: &Expr,
    tracker: &JsValueTracker,
    extern_func_to_js: &HashMap<String, (String, String)>,
) -> bool {
    match expr {
        // Direct JS interop results
        Expr::JsLoadModule { .. } => true,
        Expr::JsGetExport { .. } => true,
        Expr::JsCallFunction { .. } => true,
        Expr::JsCallMethod { .. } => true,
        Expr::JsCallValue { .. } => true,
        Expr::JsGetProperty { .. } => true,
        Expr::JsNew { .. } => true,
        Expr::JsNewFromHandle { .. } => true,
        // Local variables that are known to be JS values
        Expr::LocalGet(id) => tracker.is_js_local(*id),
        // ExternFuncRef that references a JS import
        Expr::ExternFuncRef { name, .. } => extern_func_to_js.contains_key(name),
        // Property access on JS objects returns JS values
        Expr::PropertyGet { object, .. } => is_js_object_expr(object, tracker, extern_func_to_js),
        // Call to require() returns JS value - GlobalGet(0) is typically the require function
        // Pattern: require('module').Something
        Expr::Call { callee, .. } => {
            match callee.as_ref() {
                // require() call - GlobalGet(0) is typically require
                Expr::GlobalGet(0) => true,
                // Method call on a JS object returns JS value
                Expr::PropertyGet { object, .. } => {
                    is_js_object_expr(object, tracker, extern_func_to_js)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn transform_expr(
    expr: &mut Expr,
    js_imports: &HashMap<String, JsImportInfo>,
    extern_func_to_js: &HashMap<String, (String, String)>,
    local_name_to_js: &HashMap<String, (String, String)>,
    tracker: &mut JsValueTracker,
) {
    // Handle different expression types
    match expr {
        // Call expressions - may be method calls on JS objects or direct function calls
        Expr::Call { callee, args, .. } => {
            // First check if this is a method call on a JS object: obj.method(args)
            if let Expr::PropertyGet { object, property, .. } = callee.as_mut() {
                // Transform the object first
                transform_expr(object.as_mut(), js_imports, extern_func_to_js, local_name_to_js, tracker);

                // Check if the object is a JS value
                if is_js_object_expr(object, tracker, extern_func_to_js) {
                    // Transform args, wrapping closures for JS callbacks
                    let transformed_args: Vec<Expr> = args.iter_mut().map(|arg| {
                        // For closures passed to JS, mark their parameters as JS values
                        // BEFORE transforming the closure body
                        if let Expr::Closure { params, body, .. } = arg {
                            let param_count = params.len();
                            // Create a new tracker with the closure params marked as JS values
                            let mut closure_tracker = tracker.clone();
                            for param in params.iter() {
                                closure_tracker.mark_js_local(param.id);
                            }
                            // Transform the closure body with the updated tracker
                            transform_stmts(body, js_imports, extern_func_to_js, local_name_to_js, &mut closure_tracker);
                            Expr::JsCreateCallback {
                                closure: Box::new(std::mem::replace(arg, Expr::Undefined)),
                                param_count,
                            }
                        } else {
                            transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
                            std::mem::replace(arg, Expr::Undefined)
                        }
                    }).collect();

                    // Replace with JsCallMethod
                    let method_name = property.clone();
                    let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);
                    *expr = Expr::JsCallMethod {
                        object: Box::new(object_expr),
                        method_name,
                        args: transformed_args,
                    };
                    return;
                }
            }

            // Check if this is a call to an imported JS function
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if let Some((module_source, export_name)) = extern_func_to_js.get(name) {
                    if let Some(info) = js_imports.get(module_source) {
                        // Transform args, wrapping closures for JS callbacks
                        let transformed_args: Vec<Expr> = args.iter_mut().map(|arg| {
                            // For closures passed to JS, mark their parameters as JS values
                            // BEFORE transforming the closure body
                            if let Expr::Closure { params, body, .. } = arg {
                                let param_count = params.len();
                                // Create a new tracker with the closure params marked as JS values
                                let mut closure_tracker = tracker.clone();
                                for param in params.iter() {
                                    closure_tracker.mark_js_local(param.id);
                                }
                                // Transform the closure body with the updated tracker
                                transform_stmts(body, js_imports, extern_func_to_js, local_name_to_js, &mut closure_tracker);
                                Expr::JsCreateCallback {
                                    closure: Box::new(std::mem::replace(arg, Expr::Undefined)),
                                    param_count,
                                }
                            } else {
                                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
                                std::mem::replace(arg, Expr::Undefined)
                            }
                        }).collect();

                        // Replace with JsCallFunction
                        *expr = Expr::JsCallFunction {
                            module_handle: Box::new(Expr::JsLoadModule { path: info.path.clone() }),
                            func_name: export_name.clone(),
                            args: transformed_args,
                        };
                        return;
                    }
                }
            }

            // Call through a JavaScript function value, e.g. a decorator factory
            // result from `@nestjs/common` (`const dec = Injectable(); dec(target)`).
            transform_expr(callee, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if is_js_object_expr(callee, tracker, extern_func_to_js) {
                let transformed_args: Vec<Expr> = args
                    .iter_mut()
                    .map(|arg| {
                        if let Expr::Closure { params, body, .. } = arg {
                            let param_count = params.len();
                            let mut closure_tracker = tracker.clone();
                            for param in params.iter() {
                                closure_tracker.mark_js_local(param.id);
                            }
                            transform_stmts(
                                body,
                                js_imports,
                                extern_func_to_js,
                                local_name_to_js,
                                &mut closure_tracker,
                            );
                            Expr::JsCreateCallback {
                                closure: Box::new(std::mem::replace(arg, Expr::Undefined)),
                                param_count,
                            }
                        } else {
                            transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
                            std::mem::replace(arg, Expr::Undefined)
                        }
                    })
                    .collect();
                let callee_expr = std::mem::replace(callee.as_mut(), Expr::Undefined);
                *expr = Expr::JsCallValue {
                    callee: Box::new(callee_expr),
                    args: transformed_args,
                };
                return;
            }

            // Not a JS import call, transform normally
            for arg in args.iter_mut() {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }

        // New expressions - may be for JS classes
        Expr::New { class_name, args, .. } => {
            // Classes with native codegen support should NOT be converted to JsNew
            // even if imported from JS modules - the codegen handles them directly
            const NATIVE_CODEGEN_CLASSES: &[&str] = &[
                "Redis", "Command", "Pool", "WebSocket", "WebSocketServer",
                "LRUCache", "Big", "Decimal", "BigNumber", "URLSearchParams",
            ];
            // Check if this is a JS class (but not one handled natively)
            if !NATIVE_CODEGEN_CLASSES.contains(&class_name.as_str()) && tracker.is_js_class(class_name) {
                // Find the module that exports this class
                if let Some((module_source, export_name)) = local_name_to_js.get(class_name) {
                    if let Some(info) = js_imports.get(module_source) {
                        // Transform args
                        for arg in args.iter_mut() {
                            transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
                        }

                        // Replace with JsNew
                        *expr = Expr::JsNew {
                            module_handle: Box::new(Expr::JsLoadModule { path: info.path.clone() }),
                            class_name: export_name.clone(),
                            args: std::mem::take(args),
                        };
                        return;
                    }
                }
            }

            // Not a JS class, transform args normally
            for arg in args.iter_mut() {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }

        // Dynamic new expressions - may be for JS classes (e.g., new ObjectId(str))
        Expr::NewDynamic { callee, args, .. } => {
            // Transform the callee first
            transform_expr(callee, js_imports, extern_func_to_js, local_name_to_js, tracker);

            // Transform args
            for arg in args.iter_mut() {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }

            // Check if the callee is a JS value (e.g., from JS import)
            // This includes JsGetExport, JsGetProperty, LocalGet of JS locals, etc.
            if is_js_object_expr(callee, tracker, extern_func_to_js) {
                // Transform to JsNewFromHandle - this lets us call `new` on any JS value
                let constructor_expr = std::mem::replace(callee.as_mut(), Expr::Undefined);
                let args_owned = std::mem::take(args);
                *expr = Expr::JsNewFromHandle {
                    constructor: Box::new(constructor_expr),
                    args: args_owned,
                };
            }
        }

        // Property access - may be on JS objects
        Expr::PropertyGet { object, property, .. } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);

            // Check if the object is a JS value
            if is_js_object_expr(object, tracker, extern_func_to_js) {
                let property_name = property.clone();
                let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);
                *expr = Expr::JsGetProperty {
                    object: Box::new(object_expr),
                    property_name,
                };
            }
        }

        // Property set - may be on JS objects
        Expr::PropertySet { object, property, value } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);

            // Check if the object is a JS value
            if is_js_object_expr(object, tracker, extern_func_to_js) {
                let property_name = property.clone();
                let object_expr = std::mem::replace(object.as_mut(), Expr::Undefined);
                let value_expr = std::mem::replace(value.as_mut(), Expr::Undefined);
                *expr = Expr::JsSetProperty {
                    object: Box::new(object_expr),
                    property_name,
                    value: Box::new(value_expr),
                };
            }
        }

        Expr::ExternFuncRef { name, .. } => {
            // Check if this is a reference to an imported JS value (not a call)
            if let Some((module_source, export_name)) = extern_func_to_js.get(name.as_str()) {
                if let Some(info) = js_imports.get(module_source) {
                    *expr = Expr::JsGetExport {
                        module_handle: Box::new(Expr::JsLoadModule { path: info.path.clone() }),
                        export_name: export_name.clone(),
                    };
                }
            }
        }

        // Transform all other expression types recursively
        Expr::Binary { left, right, .. } => {
            transform_expr(left, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(right, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Unary { operand, .. } => {
            transform_expr(operand, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Logical { left, right, .. } => {
            transform_expr(left, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(right, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Compare { left, right, .. } => {
            transform_expr(left, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(right, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::LocalSet(id, value) => {
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            // If the value is a JS value, mark this local as JS
            if is_js_value_expr(value, tracker) {
                tracker.mark_js_local(*id);
            }
        }
        Expr::GlobalSet(_, value) => {
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            transform_expr(condition, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(then_expr, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(else_expr, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Array(elements) => {
            for elem in elements {
                transform_expr(elem, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    crate::ir::ArrayElement::Expr(e) => transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker),
                    crate::ir::ArrayElement::Spread(e) => transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker),
                    crate::ir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(properties) => {
            for (_, value) in properties {
                transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::PropertyUpdate { object, .. } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::IndexGet { object, index } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(index, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::IndexSet { object, index, value } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(index, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::TypeOf(inner) => {
            transform_expr(inner, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::InstanceOf {
            expr: inner,
            ty_expr,
            ..
        } => {
            transform_expr(inner, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(t) = ty_expr {
                transform_expr(t, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::Await(inner) => {
            transform_expr(inner, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Closure { body, .. } => {
            transform_stmts(body, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        // Native method calls may have expressions in args
        // If the object is a JS value, convert to JsCallMethod for V8 dispatch
        Expr::NativeMethodCall { object, args, method, module, .. } => {
            // Transform children first
            if let Some(obj) = object.as_mut() {
                transform_expr(obj.as_mut(), js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
            for arg in args.iter_mut() {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }

            // Check if the object is a JS value - if so, dispatch through V8
            if let Some(obj) = object {
                if is_js_object_expr(obj, tracker, extern_func_to_js) {
                    let method_name = method.clone();
                    let object_expr = std::mem::replace(obj.as_mut(), Expr::Undefined);
                    let args_owned: Vec<Expr> = std::mem::take(args);
                    *expr = Expr::JsCallMethod {
                        object: Box::new(object_expr),
                        method_name,
                        args: args_owned,
                    };
                    return;
                }
            }

            // Check if the module itself is a JS import (object: None = static method)
            if object.is_none() {
                if let Some((module_source, export_name)) = extern_func_to_js.get(module.as_str()) {
                    if let Some(info) = js_imports.get(module_source) {
                        let method_name = method.clone();
                        let module_expr = Expr::JsGetExport {
                            module_handle: Box::new(Expr::JsLoadModule { path: info.path.clone() }),
                            export_name: export_name.clone(),
                        };
                        let args_owned: Vec<Expr> = std::mem::take(args);
                        *expr = Expr::JsCallMethod {
                            object: Box::new(module_expr),
                            method_name,
                            args: args_owned,
                        };
                    }
                }
            }
        }
        Expr::StaticMethodCall {
            class_name, args, ..
        } => {
            // Issue: Effect.pipe(map) chain — when `class_name` is a JS-imported
            // value (e.g. `import { Effect } from 'effect'`), the
            // StaticMethodCall is routed through `js_call_v8_member_method`
            // (see emit_v8_member_method_call). Any Closure args need to be
            // wrapped in JsCreateCallback so V8 sees a real v8::Function
            // instead of a raw native function pointer. Without this,
            // `Effect.map((x) => x + 1)` passed the arrow as a number/pointer
            // and Effect's internal `f` ended up "not a function".
            if extern_func_to_js.contains_key(class_name) {
                for arg in args.iter_mut() {
                    if let Expr::Closure { params, body, .. } = arg {
                        let param_count = params.len();
                        let mut closure_tracker = tracker.clone();
                        for param in params.iter() {
                            closure_tracker.mark_js_local(param.id);
                        }
                        transform_stmts(
                            body,
                            js_imports,
                            extern_func_to_js,
                            local_name_to_js,
                            &mut closure_tracker,
                        );
                        let owned = std::mem::replace(arg, Expr::Undefined);
                        *arg = Expr::JsCreateCallback {
                            closure: Box::new(owned),
                            param_count,
                        };
                    } else {
                        transform_expr(
                            arg,
                            js_imports,
                            extern_func_to_js,
                            local_name_to_js,
                            tracker,
                        );
                    }
                }
            } else {
                for arg in args {
                    transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
                }
            }
        }
        Expr::StaticFieldSet { value, .. } => {
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::SuperCall(args) => {
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            transform_expr(home, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(receiver, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            transform_expr(home, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(receiver, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            transform_expr(home, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(receiver, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        // Dynamic environment variable access
        Expr::EnvGetDynamic(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // File system / path / JSON / Math / Crypto operations
        Expr::FsReadFileSync(e) | Expr::FsExistsSync(e) | Expr::FsMkdirSync(e) | Expr::FsUnlinkSync(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::FsWriteFileSync(a, b) | Expr::FsAppendFileSync(a, b) | Expr::PathJoin(a, b) | Expr::PathMatchesGlob(a, b) | Expr::PathResolveJoin(a, b) | Expr::PathWin32Join(a, b) | Expr::MathPow(a, b) | Expr::MathImul(a, b) => {
            transform_expr(a, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(b, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::PathDirname(e) | Expr::PathBasename(e) | Expr::PathExtname(e) | Expr::PathResolve(e) | Expr::PathIsAbsolute(e) | Expr::PathToNamespacedPath(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::JsonParse(e) | Expr::JsonStringify(e) | Expr::JsonRawJson(e) | Expr::JsonIsRawJson(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MathFloor(e) | Expr::MathCeil(e) | Expr::MathRound(e) | Expr::MathTrunc(e) | Expr::MathSign(e) | Expr::MathAbs(e) | Expr::MathSqrt(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::PathWin32 { args, .. } => {
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::CryptoRandomBytes(e) | Expr::CryptoSha256(e) | Expr::CryptoMd5(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Array methods
        Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        }
        | Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(fi) = from_index {
                transform_expr(fi, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(start, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(e) = end {
                transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArraySplice { start, delete_count, items, .. } => {
            transform_expr(start, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(dc) = delete_count {
                transform_expr(dc, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
            for item in items {
                transform_expr(item, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArrayForEach { array, callback } | Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(callback, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(callback, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(init) = initial {
                transform_expr(init, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(sep) = separator {
                transform_expr(sep, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ArrayFlat { array }
        | Expr::ArrayToReversed { array }
        | Expr::ArrayReverseValue { receiver: array } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ArrayToSorted { array, comparator } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(cmp) = comparator { transform_expr(cmp, js_imports, extern_func_to_js, local_name_to_js, tracker); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(start, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(delete_count, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for item in items { transform_expr(item, js_imports, extern_func_to_js, local_name_to_js, tracker); }
        }
        Expr::ArrayWith { array, index, value } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(index, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            transform_expr(target, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(start, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(e) = end { transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker); }
        }
        Expr::ArrayCopyWithinValue { receiver, target, start, end } => {
            transform_expr(receiver, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(target, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(start, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(e) = end { transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker); }
        }
        Expr::StringSplit(a, b) => {
            transform_expr(a, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(b, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::StringFromCharCode(code) | Expr::StringFromCharCodeSpread(code) => {
            transform_expr(code, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Map/Set methods
        Expr::MapSet { map, key, value } => {
            transform_expr(map, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            transform_expr(map, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MapSize(e) | Expr::MapClear(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::SetAdd { value, .. } => {
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            transform_expr(set, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::SetSize(e) | Expr::SetClear(e) | Expr::SetValues(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Date methods
        Expr::DateNew(args) => {
            for a in args {
                transform_expr(a, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::DateGetTime(e) | Expr::DateToISOString(e) | Expr::DateGetFullYear(e) |
        Expr::DateGetMonth(e) | Expr::DateGetDate(e) | Expr::DateGetDay(e) | Expr::DateGetHours(e) |
        Expr::DateGetMinutes(e) | Expr::DateGetSeconds(e) | Expr::DateGetMilliseconds(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // RegExp methods
        Expr::RegExpTest { regex, string } => {
            transform_expr(regex, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(string, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::StringMatch { string, regex } => {
            transform_expr(string, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(regex, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::StringReplace { string, pattern, replacement } => {
            transform_expr(string, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(pattern, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(replacement, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Object operations
        Expr::ObjectKeys(e) | Expr::ForInKeys(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Parse/coerce functions
        Expr::ParseInt { string, radix } => {
            transform_expr(string, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(r) = radix {
                transform_expr(r, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ParseFloat(e) | Expr::NumberCoerce(e) | Expr::BigIntCoerce(e) | Expr::StringCoerce(e) | Expr::ObjectCoerce(e) | Expr::IsNaN(e) | Expr::IsUndefinedOrBareNan(e) | Expr::IsFinite(e) | Expr::StaticPluginResolve(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // JS Runtime expressions (already transformed, just recurse into subexpressions)
        Expr::JsLoadModule { .. } => {}
        Expr::JsGetExport { module_handle, .. } => {
            transform_expr(module_handle, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::JsCallFunction { module_handle, args, .. } => {
            transform_expr(module_handle, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::JsGetProperty { object, .. } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::JsSetProperty { object, value, .. } => {
            transform_expr(object, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::JsNew { module_handle, args, .. } => {
            transform_expr(module_handle, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::JsNewFromHandle { constructor, args } => {
            transform_expr(constructor, js_imports, extern_func_to_js, local_name_to_js, tracker);
            for arg in args {
                transform_expr(arg, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            transform_expr(closure, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => {
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(target, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(property_key) = property_key {
                transform_expr(property_key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => {
            transform_expr(key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(target, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(property_key) = property_key {
                transform_expr(property_key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => {
            transform_expr(target, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(property_key) = property_key {
                transform_expr(property_key, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        // Expressions that don't need transformation
        Expr::Number(_) | Expr::Integer(_) | Expr::BigInt(_) | Expr::String(_) | Expr::Bool(_) |
        Expr::Null | Expr::Undefined | Expr::This | Expr::LocalGet(_) | Expr::GlobalGet(_) |
        Expr::FuncRef(_) | Expr::ClassRef(_) | Expr::EnumMember { .. } |
        Expr::RegExp { .. } | Expr::NativeModuleRef(_) | Expr::StaticFieldGet { .. } |
        Expr::EnvGet(_) | Expr::ProcessUptime | Expr::ProcessMemoryUsage | Expr::ProcessEnv | Expr::MathRandom | Expr::CryptoRandomUUID | Expr::CryptoRandomUUIDv7 | Expr::DateNow |
        Expr::MapNew | Expr::SetNew | Expr::Update { .. } |
        Expr::ArrayPop(_) | Expr::ArrayShift(_) |
        // OS module expressions
        Expr::OsPlatform | Expr::OsArch | Expr::OsHostname | Expr::OsType | Expr::OsRelease |
        Expr::OsHomedir | Expr::OsTmpdir | Expr::OsTotalmem | Expr::OsFreemem | Expr::OsCpus |
        // Additional expressions that don't contain sub-expressions
        _ => {}
    }
}
