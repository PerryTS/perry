use super::*;

// ============================================================================
// Default Argument Filling
// ============================================================================

/// Resolved padding context for a single module: constructor param defaults
/// keyed by class name, plus free-function fixed-param boundaries keyed by
/// `FuncId`.
pub(crate) struct DefaultFill {
    /// Class name → fixed (pre-rest) constructor param defaults.
    ctors: HashMap<String, Vec<Option<Expr>>>,
    /// `FuncId` → `(fill_end, has_synth_args)`. `fill_end` is the rest-param
    /// position (or the param count when there is no rest) — the boundary up
    /// to which a `FuncRef` call site pads missing args with `undefined`.
    /// `has_synth_args` flags callees that materialize `arguments`; those are
    /// padded by codegen's synth-args path instead, so this pass must skip
    /// them (#5521 — same contract as the during-lowering padding in
    /// `lower/expr_call/mod.rs`).
    funcs: HashMap<FuncId, (usize, bool)>,
}

/// Fill in default arguments for New expressions where fewer args are provided
/// than the constructor expects, and pad missing trailing args on direct
/// `FuncRef` calls. The latter closes #5521: HIR lowering pads a call only
/// when the callee's signature is already registered, so a call to a
/// *forward-referenced* function (defined later in the module — e.g.
/// bcryptjs `hashSync` calling `_hash`) left the missing params reading
/// uninitialized arg registers (a stray `0`). This post-lowering pass has
/// every function's final shape, so padding no longer depends on source
/// order.
pub(crate) fn fill_default_arguments(module: &mut Module) {
    // Build a map of class name -> constructor param defaults
    let mut ctors: HashMap<String, Vec<Option<Expr>>> = HashMap::new();
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
            ctors.insert(class.name.clone(), defaults);
        }
    }

    // Build a map of FuncId -> (fill boundary, synth-args flag) for every
    // free function in the module. Mirrors `lookup_func_defaults`: fill up to
    // the rest param (or the full param count), and never pad synth-`arguments`
    // callees (codegen sizes their `arguments` array from the real arg count).
    let mut funcs: HashMap<FuncId, (usize, bool)> = HashMap::new();
    for func in &module.functions {
        let fill_end = func
            .params
            .iter()
            .position(|p| p.is_rest)
            .unwrap_or(func.params.len());
        let has_synth_args = func
            .params
            .last()
            .is_some_and(|p| p.arguments_object.is_some());
        funcs.insert(func.id, (fill_end, has_synth_args));
    }

    let cx = DefaultFill { ctors, funcs };

    // Fill defaults in init statements
    fill_defaults_in_stmts(&mut module.init, &cx);

    // Fill defaults in function bodies
    for func in &mut module.functions {
        fill_defaults_in_stmts(&mut func.body, &cx);
    }

    // Fill defaults in class methods
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            fill_defaults_in_stmts(&mut ctor.body, &cx);
        }
        for method in &mut class.methods {
            fill_defaults_in_stmts(&mut method.body, &cx);
        }
    }
}

fn fill_defaults_in_stmts(stmts: &mut [Stmt], cx: &DefaultFill) {
    for stmt in stmts {
        fill_defaults_in_stmt(stmt, cx);
    }
}

fn fill_defaults_in_stmt(stmt: &mut Stmt, cx: &DefaultFill) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                fill_defaults_in_expr(expr, cx);
            }
        }
        Stmt::Expr(expr) => fill_defaults_in_expr(expr, cx),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                fill_defaults_in_expr(e, cx);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fill_defaults_in_expr(condition, cx);
            fill_defaults_in_stmts(then_branch, cx);
            if let Some(else_b) = else_branch {
                fill_defaults_in_stmts(else_b, cx);
            }
        }
        Stmt::While { condition, body } => {
            fill_defaults_in_expr(condition, cx);
            fill_defaults_in_stmts(body, cx);
        }
        Stmt::DoWhile { body, condition } => {
            fill_defaults_in_stmts(body, cx);
            fill_defaults_in_expr(condition, cx);
        }
        Stmt::Labeled { body, .. } => {
            fill_defaults_in_stmt(body, cx);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                fill_defaults_in_stmt(init_stmt, cx);
            }
            if let Some(cond) = condition {
                fill_defaults_in_expr(cond, cx);
            }
            if let Some(upd) = update {
                fill_defaults_in_expr(upd, cx);
            }
            fill_defaults_in_stmts(body, cx);
        }
        Stmt::Throw(expr) => fill_defaults_in_expr(expr, cx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            fill_defaults_in_stmts(body, cx);
            if let Some(ref mut c) = catch {
                fill_defaults_in_stmts(&mut c.body, cx);
            }
            if let Some(f) = finally {
                fill_defaults_in_stmts(f, cx);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            fill_defaults_in_expr(discriminant, cx);
            for case in cases {
                fill_defaults_in_stmts(&mut case.body, cx);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn fill_defaults_in_expr(expr: &mut Expr, cx: &DefaultFill) {
    match expr {
        Expr::New {
            class_name, args, ..
        } => {
            // First, recurse into the arguments
            for arg in args.iter_mut() {
                fill_defaults_in_expr(arg, cx);
            }

            // Check if we need to fill in defaults
            if let Some(defaults) = cx.ctors.get(class_name) {
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
            fill_defaults_in_expr(val, cx);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            fill_defaults_in_expr(left, cx);
            fill_defaults_in_expr(right, cx);
        }
        Expr::Unary { operand, .. } => {
            fill_defaults_in_expr(operand, cx);
        }
        Expr::Update { .. } => {
            // Update expressions (++/--) don't contain sub-expressions
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            fill_defaults_in_expr(condition, cx);
            fill_defaults_in_expr(then_expr, cx);
            fill_defaults_in_expr(else_expr, cx);
        }
        Expr::Call { callee, args, .. } => {
            fill_defaults_in_expr(callee, cx);
            for arg in args.iter_mut() {
                fill_defaults_in_expr(arg, cx);
            }
            // #5521: pad missing trailing args on a direct call to a known
            // free function. During-lowering padding (lower/expr_call/mod.rs)
            // only fires when the callee's signature is already registered, so
            // a forward-referenced callee (defined later in the module) is left
            // under-padded and reads uninitialized arg registers. Here every
            // function shape is final, so pad up to the rest boundary. Skip
            // synth-`arguments` callees (codegen pads those) and never inflate
            // a call that already supplies enough args.
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                if let Some(&(fill_end, has_synth_args)) = cx.funcs.get(func_id) {
                    if !has_synth_args {
                        for _ in args.len()..fill_end {
                            args.push(Expr::Undefined);
                        }
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for elem in elements {
                fill_defaults_in_expr(elem, cx);
            }
        }
        Expr::Object(fields) => {
            for (_, val) in fields {
                fill_defaults_in_expr(val, cx);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, val) in parts {
                fill_defaults_in_expr(val, cx);
            }
        }
        Expr::IndexGet { object, index } => {
            fill_defaults_in_expr(object, cx);
            fill_defaults_in_expr(index, cx);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            fill_defaults_in_expr(object, cx);
            fill_defaults_in_expr(index, cx);
            fill_defaults_in_expr(value, cx);
        }
        Expr::PropertyGet { object, .. } => {
            fill_defaults_in_expr(object, cx);
        }
        Expr::PropertySet { object, value, .. } => {
            fill_defaults_in_expr(object, cx);
            fill_defaults_in_expr(value, cx);
        }
        Expr::PropertyUpdate { object, .. } => {
            fill_defaults_in_expr(object, cx);
        }
        Expr::Await(inner) => {
            fill_defaults_in_expr(inner, cx);
        }
        Expr::TypeOf(inner) => {
            fill_defaults_in_expr(inner, cx);
        }
        Expr::Void(inner) => {
            fill_defaults_in_expr(inner, cx);
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                fill_defaults_in_expr(v, cx);
            }
        }
        Expr::InstanceOf { expr, .. } => {
            fill_defaults_in_expr(expr, cx);
        }
        Expr::Closure { body, .. } => {
            fill_defaults_in_stmts(body, cx);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                fill_defaults_in_expr(obj, cx);
            }
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        Expr::NativeArenaAlloc(size) | Expr::NativeArenaDispose(size) => {
            fill_defaults_in_expr(size, cx);
        }
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            fill_defaults_in_expr(owner, cx);
            fill_defaults_in_expr(byte_offset, cx);
            fill_defaults_in_expr(length, cx);
        }
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            fill_defaults_in_expr(owner, cx);
            fill_defaults_in_expr(byte_offset, cx);
            fill_defaults_in_expr(count, cx);
        }
        Expr::NativeMemoryFillU32 { view, value } => {
            fill_defaults_in_expr(view, cx);
            fill_defaults_in_expr(value, cx);
        }
        Expr::NativeMemoryCopy { dst, src } => {
            fill_defaults_in_expr(dst, cx);
            fill_defaults_in_expr(src, cx);
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            fill_defaults_in_expr(home, cx);
            fill_defaults_in_expr(key, cx);
            fill_defaults_in_expr(receiver, cx);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            fill_defaults_in_expr(key, cx);
            fill_defaults_in_expr(value, cx);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            fill_defaults_in_expr(home, cx);
            fill_defaults_in_expr(key, cx);
            fill_defaults_in_expr(value, cx);
            fill_defaults_in_expr(receiver, cx);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            fill_defaults_in_expr(home, cx);
            fill_defaults_in_expr(key, cx);
            fill_defaults_in_expr(receiver, cx);
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        Expr::SuperCall(args) => {
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            fill_defaults_in_expr(object, cx);
            for arg in args {
                fill_defaults_in_expr(arg, cx);
            }
        }
        _ => {}
    }
}
