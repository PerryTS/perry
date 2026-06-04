use super::*;

// ============================================================================
// Default Argument Filling
// ============================================================================

/// Fill in default arguments for New expressions where fewer args are provided
/// than the constructor expects
pub(crate) fn fill_default_arguments(module: &mut Module) {
    // Build a map of class name -> constructor param defaults
    let mut ctor_defaults: HashMap<String, Vec<Option<Expr>>> = HashMap::new();
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            // Stop at a trailing rest parameter. `constructor(...e)` (or
            // `constructor(a, b = 5, ...e)`) accepts zero or more trailing
            // args, so the call-site padding below must never synthesize an
            // `undefined` for the rest slot: doing so makes `new C()` collect
            // `[undefined]` instead of `[]`, and any `e.forEach(...)` over that
            // bogus element throws (marked's `new q` / hono's verb-method
            // setup). Only the leading fixed params (which DO get default-fill
            // checks prepended to the ctor body) are eligible for padding.
            let defaults: Vec<Option<Expr>> = ctor
                .params
                .iter()
                .take_while(|p| !p.is_rest)
                .map(|p| p.default.clone())
                .collect();
            ctor_defaults.insert(class.name.clone(), defaults);
        }
    }

    // Fill defaults in init statements
    fill_defaults_in_stmts(&mut module.init, &ctor_defaults);

    // Fill defaults in function bodies
    for func in &mut module.functions {
        fill_defaults_in_stmts(&mut func.body, &ctor_defaults);
    }

    // Fill defaults in class methods
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            fill_defaults_in_stmts(&mut ctor.body, &ctor_defaults);
        }
        for method in &mut class.methods {
            fill_defaults_in_stmts(&mut method.body, &ctor_defaults);
        }
    }
}

fn fill_defaults_in_stmts(stmts: &mut [Stmt], ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    for stmt in stmts {
        fill_defaults_in_stmt(stmt, ctor_defaults);
    }
}

fn fill_defaults_in_stmt(stmt: &mut Stmt, ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                fill_defaults_in_expr(expr, ctor_defaults);
            }
        }
        Stmt::Expr(expr) => fill_defaults_in_expr(expr, ctor_defaults),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                fill_defaults_in_expr(e, ctor_defaults);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_stmts(then_branch, ctor_defaults);
            if let Some(else_b) = else_branch {
                fill_defaults_in_stmts(else_b, ctor_defaults);
            }
        }
        Stmt::While { condition, body } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Stmt::DoWhile { body, condition } => {
            fill_defaults_in_stmts(body, ctor_defaults);
            fill_defaults_in_expr(condition, ctor_defaults);
        }
        Stmt::Labeled { body, .. } => {
            fill_defaults_in_stmt(body, ctor_defaults);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                fill_defaults_in_stmt(init_stmt, ctor_defaults);
            }
            if let Some(cond) = condition {
                fill_defaults_in_expr(cond, ctor_defaults);
            }
            if let Some(upd) = update {
                fill_defaults_in_expr(upd, ctor_defaults);
            }
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Stmt::Throw(expr) => fill_defaults_in_expr(expr, ctor_defaults),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fill_defaults_in_stmts(body, ctor_defaults);
            if let Some(ref mut c) = catch {
                fill_defaults_in_stmts(&mut c.body, ctor_defaults);
            }
            if let Some(f) = finally {
                fill_defaults_in_stmts(f, ctor_defaults);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fill_defaults_in_expr(discriminant, ctor_defaults);
            for case in cases {
                fill_defaults_in_stmts(&mut case.body, ctor_defaults);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) => {}
    }
}

fn fill_defaults_in_expr(expr: &mut Expr, ctor_defaults: &HashMap<String, Vec<Option<Expr>>>) {
    match expr {
        Expr::New {
            class_name, args, ..
        } => {
            // First, recurse into the arguments
            for arg in args.iter_mut() {
                fill_defaults_in_expr(arg, ctor_defaults);
            }

            // Check if we need to fill in defaults
            if let Some(defaults) = ctor_defaults.get(class_name) {
                let param_count = defaults.len();
                let arg_count = args.len();

                if arg_count < param_count {
                    // Fill missing constructor slots with `undefined`.
                    // Constructor bodies already prepend default-param
                    // checks, so default expressions must run in the
                    // constructor boundary rather than at the `new` site.
                    for _ in arg_count..param_count {
                        args.push(Expr::Undefined);
                    }
                }
            }
        }
        // Recurse into sub-expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            fill_defaults_in_expr(val, ctor_defaults);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            fill_defaults_in_expr(left, ctor_defaults);
            fill_defaults_in_expr(right, ctor_defaults);
        }
        Expr::Unary { operand, .. } => {
            fill_defaults_in_expr(operand, ctor_defaults);
        }
        Expr::Update { .. } => {
            // Update expressions (++/--) don't contain sub-expressions
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fill_defaults_in_expr(condition, ctor_defaults);
            fill_defaults_in_expr(then_expr, ctor_defaults);
            fill_defaults_in_expr(else_expr, ctor_defaults);
        }
        Expr::Call { callee, args, .. } => {
            fill_defaults_in_expr(callee, ctor_defaults);
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                fill_defaults_in_expr(elem, ctor_defaults);
            }
        }
        Expr::Object(fields) => {
            for (_, val) in fields {
                fill_defaults_in_expr(val, ctor_defaults);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, val) in parts {
                fill_defaults_in_expr(val, ctor_defaults);
            }
        }
        Expr::IndexGet { object, index } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(index, ctor_defaults);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(index, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::PropertyGet { object, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
        }
        Expr::PropertySet { object, value, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::PropertyUpdate { object, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
        }
        Expr::Await(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::TypeOf(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::Void(inner) => {
            fill_defaults_in_expr(inner, ctor_defaults);
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                fill_defaults_in_expr(v, ctor_defaults);
            }
        }
        Expr::InstanceOf { expr, .. } => {
            fill_defaults_in_expr(expr, ctor_defaults);
        }
        Expr::Closure { body, .. } => {
            fill_defaults_in_stmts(body, ctor_defaults);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fill_defaults_in_expr(obj, ctor_defaults);
            }
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::NativeArenaAlloc(size) | Expr::NativeArenaDispose(size) => {
            fill_defaults_in_expr(size, ctor_defaults);
        }
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            fill_defaults_in_expr(owner, ctor_defaults);
            fill_defaults_in_expr(byte_offset, ctor_defaults);
            fill_defaults_in_expr(length, ctor_defaults);
        }
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            fill_defaults_in_expr(owner, ctor_defaults);
            fill_defaults_in_expr(byte_offset, ctor_defaults);
            fill_defaults_in_expr(count, ctor_defaults);
        }
        Expr::NativeMemoryFillU32 { view, value } => {
            fill_defaults_in_expr(view, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::NativeMemoryCopy { dst, src } => {
            fill_defaults_in_expr(dst, ctor_defaults);
            fill_defaults_in_expr(src, ctor_defaults);
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            fill_defaults_in_expr(home, ctor_defaults);
            fill_defaults_in_expr(key, ctor_defaults);
            fill_defaults_in_expr(receiver, ctor_defaults);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            fill_defaults_in_expr(key, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            fill_defaults_in_expr(home, ctor_defaults);
            fill_defaults_in_expr(key, ctor_defaults);
            fill_defaults_in_expr(value, ctor_defaults);
            fill_defaults_in_expr(receiver, ctor_defaults);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            fill_defaults_in_expr(home, ctor_defaults);
            fill_defaults_in_expr(key, ctor_defaults);
            fill_defaults_in_expr(receiver, ctor_defaults);
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::SuperCall(args) => {
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            fill_defaults_in_expr(object, ctor_defaults);
            for arg in args {
                fill_defaults_in_expr(arg, ctor_defaults);
            }
        }
        _ => {}
    }
}
