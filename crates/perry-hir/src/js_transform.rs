//! JavaScript import transformation
//!
//! This module transforms imports from JavaScript modules into V8 runtime calls.
//! When an import comes from a JS module (ModuleKind::Interpreted), this pass:
//! 1. Creates a module handle variable for each JS module
//! 2. Adds initialization code to load the module via JsLoadModule
//! 3. Transforms function calls to imported functions into JsCallFunction calls
//! 4. Transforms method calls on JS objects to JsCallMethod
//! 5. Transforms property access on JS objects to JsGetProperty/JsSetProperty
//! 6. Transforms new expressions for JS classes to JsNew
//! 7. Wraps closures passed to JS functions with JsCreateCallback

use crate::ir::{Expr, Module, ModuleKind, Stmt};
use perry_types::LocalId;
use std::collections::{BTreeMap, HashMap, HashSet};

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

fn transform_stmts(
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

fn transform_stmt(
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
        Stmt::PreallocateBoxes(_) => {}
    }
}

/// Check if an expression produces a JS value
fn is_js_value_expr(expr: &Expr, tracker: &JsValueTracker) -> bool {
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
fn is_js_object_expr(
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

fn transform_expr(
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
            if let Expr::PropertyGet { object, property } = callee.as_mut() {
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
        Expr::NewDynamic { callee, args } => {
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
        Expr::PropertyGet { object, property } => {
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
        Expr::InstanceOf { expr: inner, .. } => {
            transform_expr(inner, js_imports, extern_func_to_js, local_name_to_js, tracker);
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
        Expr::JsonParse(e) | Expr::JsonStringify(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MathFloor(e) | Expr::MathCeil(e) | Expr::MathRound(e) | Expr::MathAbs(e) | Expr::MathSqrt(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
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
        Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
            transform_expr(array, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(value, js_imports, extern_func_to_js, local_name_to_js, tracker);
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
        Expr::ArrayFlat { array } | Expr::ArrayToReversed { array } => {
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
        Expr::StringSplit(a, b) => {
            transform_expr(a, js_imports, extern_func_to_js, local_name_to_js, tracker);
            transform_expr(b, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        Expr::StringFromCharCode(code) => {
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
        Expr::ObjectKeys(e) => {
            transform_expr(e, js_imports, extern_func_to_js, local_name_to_js, tracker);
        }
        // Parse/coerce functions
        Expr::ParseInt { string, radix } => {
            transform_expr(string, js_imports, extern_func_to_js, local_name_to_js, tracker);
            if let Some(r) = radix {
                transform_expr(r, js_imports, extern_func_to_js, local_name_to_js, tracker);
            }
        }
        Expr::ParseFloat(e) | Expr::NumberCoerce(e) | Expr::BigIntCoerce(e) | Expr::StringCoerce(e) | Expr::IsNaN(e) | Expr::IsUndefinedOrBareNan(e) | Expr::IsFinite(e) | Expr::StaticPluginResolve(e) => {
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
        Expr::EnvGet(_) | Expr::ProcessUptime | Expr::ProcessMemoryUsage | Expr::ProcessEnv | Expr::MathRandom | Expr::CryptoRandomUUID | Expr::DateNow |
        Expr::MapNew | Expr::SetNew | Expr::Update { .. } |
        Expr::ArrayPop(_) | Expr::ArrayShift(_) |
        // OS module expressions
        Expr::OsPlatform | Expr::OsArch | Expr::OsHostname | Expr::OsType | Expr::OsRelease |
        Expr::OsHomedir | Expr::OsTmpdir | Expr::OsTotalmem | Expr::OsFreemem | Expr::OsCpus |
        // Additional expressions that don't contain sub-expressions
        _ => {}
    }
}

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
fn scan_for_native_func_returns(
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
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
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
fn scan_for_ident_init_propagation(
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
fn lookup_native_from_init_ident(
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
fn scan_expr_for_closure_returns(
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
        _ => {}
    }
}

fn fix_native_instance_stmt(
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
        Stmt::PreallocateBoxes(_) => {}
    }
}

/// Try to resolve native instance info from an object expression
fn resolve_native_instance<'a>(
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

fn fix_native_instance_expr(
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
        Expr::NewDynamic { callee, args } => {
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
fn build_class_field_natives(
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
fn scan_stmt_for_user_class_instances(stmt: &Stmt, user_classes: &mut HashMap<LocalId, String>) {
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
fn fix_class_field_stmt(
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
        Stmt::PreallocateBoxes(_) => {}
    }
}

/// Issue #341: rewrite `this.<field>.method(args)` and
/// `<localGet>.<field>.method(args)` into `NativeMethodCall` when the
/// field is registered as a native instance for the enclosing class
/// (for `this.*`) or the local's class (for `local.*`).
fn fix_class_field_expr(
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
fn chained_native_class(module: &str, prior_method: &str) -> Option<&'static str> {
    match (module, prior_method) {
        ("better-sqlite3", "prepare") => Some("Statement"),
        ("mongodb", "db") => Some("Database"),
        ("mongodb", "collection") => Some("Collection"),
        ("mysql2", "getConnection") | ("mysql2/promise", "getConnection") => Some("PoolConnection"),
        ("pg", "connect") => Some("PoolClient"),
        ("ioredis", "duplicate") => Some("Redis"),
        _ => None,
    }
}

/// Issue #341: walk a statement looking for `this.<field> = <native creation>`
/// patterns inside class constructors. Records the field name so subsequent
/// method bodies can rewrite `this.<field>.method(...)` calls to
/// `NativeMethodCall`. Recurses through control-flow constructs (if/while/
/// for/try) so guarded assignments still register.
fn scan_stmt_for_field_native_instances(
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
fn scan_stmt_for_native_instances(
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

fn fix_native_instance_stmt_with_locals(
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
        Stmt::PreallocateBoxes(_) => {}
    }
}

fn fix_native_instance_expr_with_locals(
    expr: &mut Expr,
    native_instances: &HashMap<String, (String, String)>,
    local_id_instances: &HashMap<LocalId, (String, String)>,
) {
    match expr {
        // The key case: method calls that might be on native instances
        Expr::Call { callee, args, .. } => {
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
            }

            // Not a native instance call, recurse
            fix_native_instance_expr_with_locals(callee, native_instances, local_id_instances);
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
        Expr::PropertyGet { object, .. } => {
            fix_native_instance_expr_with_locals(object, native_instances, local_id_instances);
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

/// Detect if an expression is creating a native module instance (with context for local variables)
/// Returns Some((module_name, class_name)) if it is
fn detect_native_instance_creation_with_context(
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
                ("net", "Socket") => "Socket",
                ("pg", "connect") => "Client",
                ("http" | "https", "request" | "get") => "ClientRequest",
                ("http", "createServer") => "HttpServer",
                ("https", "createServer") => "HttpsServer",
                ("http2", "createSecureServer") => "Http2SecureServer",
                ("node-cron", "schedule") => "CronJob",
                ("readline", "createInterface") => "Interface",
                _ => return None,
            };
            // For ("net", _) / ("tls", _) factories, `s` belongs to net.Socket's
            // dispatch — register under "net" regardless of which module the
            // factory call lived in (mirrors lower.rs:5517).
            let owning_module = match (module.as_str(), method.as_str()) {
                ("tls", "connect") => "net".to_string(),
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
            match class_name.as_str() {
                "Database" => Some(("better-sqlite3".to_string(), "Database".to_string())),
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
