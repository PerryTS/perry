use super::*;

/// Main monomorphization pass
/// Processes the module and generates specialized versions of generic functions/classes
pub fn monomorphize_module(module: &mut Module) {
    let mut ctx = MonomorphizationContext::new(module);
    let idx = ModuleIndex::new(module);

    // First pass: collect all generic instantiations from the code
    collect_instantiations(module, &mut ctx, &idx);

    // Process work queues until empty
    let mut new_functions = Vec::new();
    let mut new_classes = Vec::new();

    while !ctx.func_work_queue.is_empty() || !ctx.class_work_queue.is_empty() {
        // Process function specializations
        while let Some(request) = ctx.func_work_queue.pop_front() {
            let mangled_args = mangle_type_args(&request.type_args);
            let key = (request.original_id, mangled_args);
            if ctx.processed_funcs.contains(&key) {
                continue;
            }
            ctx.processed_funcs.insert(key);

            // Find the original function
            if let Some(&fi) = idx.func_by_id.get(&request.original_id) {
                let original = &module.functions[fi];
                // Check type parameter constraints
                if let Err(errors) =
                    check_function_constraints(original, &request.type_args, module, &idx)
                {
                    for err in errors {
                        eprintln!(
                            "Warning: Constraint violation in function '{}': {:?}",
                            original.name, err
                        );
                    }
                    // Continue with specialization even on constraint errors (for now)
                }
                let specialized = specialize_function(original, &request.type_args, request.new_id);
                new_functions.push(specialized);
            }
        }

        // Process class specializations
        while let Some(request) = ctx.class_work_queue.pop_front() {
            let mangled_args = mangle_type_args(&request.type_args);
            let key = (request.original_name.clone(), mangled_args);
            if ctx.processed_classes.contains(&key) {
                continue;
            }
            ctx.processed_classes.insert(key);

            // Find the original class
            if let Some(&ci) = idx.class_by_name.get(&request.original_name) {
                let original = &module.classes[ci];
                // Check type parameter constraints
                if let Err(errors) =
                    check_class_constraints(original, &request.type_args, module, &idx)
                {
                    for err in errors {
                        eprintln!(
                            "Warning: Constraint violation in class '{}': {:?}",
                            original.name, err
                        );
                    }
                    // Continue with specialization even on constraint errors (for now)
                }
                let new_id = ctx.fresh_class_id();
                let specialized = specialize_class(original, &request.type_args, new_id);
                new_classes.push(specialized);
            }
        }
    }

    // Add specialized functions and classes to the module
    module.functions.extend(new_functions);
    module.classes.extend(new_classes);

    // Update call sites to use specialized versions
    update_call_sites(module, &ctx);

    // Fill in default arguments for constructor calls
    fill_default_arguments(module);
}

/// Collect all generic instantiations from the module
fn collect_instantiations(module: &Module, ctx: &mut MonomorphizationContext, idx: &ModuleIndex) {
    // Scan all functions for generic calls
    for func in &module.functions {
        collect_instantiations_in_stmts(&func.body, ctx, module, idx);
    }

    // Scan all class methods
    for class in &module.classes {
        if let Some(ref ctor) = class.constructor {
            collect_instantiations_in_stmts(&ctor.body, ctx, module, idx);
        }
        for method in &class.methods {
            collect_instantiations_in_stmts(&method.body, ctx, module, idx);
        }
    }

    // Scan init statements
    collect_instantiations_in_stmts(&module.init, ctx, module, idx);
}

fn collect_instantiations_in_stmts(
    stmts: &[Stmt],
    ctx: &mut MonomorphizationContext,
    module: &Module,
    idx: &ModuleIndex,
) {
    for stmt in stmts {
        collect_instantiations_in_stmt(stmt, ctx, module, idx);
    }
}

fn collect_instantiations_in_stmt(
    stmt: &Stmt,
    ctx: &mut MonomorphizationContext,
    module: &Module,
    idx: &ModuleIndex,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                collect_instantiations_in_expr(expr, ctx, module, idx);
            }
        }
        Stmt::Expr(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_stmts(then_branch, ctx, module, idx);
            if let Some(else_b) = else_branch {
                collect_instantiations_in_stmts(else_b, ctx, module, idx);
            }
        }
        Stmt::While { condition, body } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        Stmt::DoWhile { body, condition } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
            collect_instantiations_in_expr(condition, ctx, module, idx);
        }
        Stmt::Labeled { body, .. } => {
            collect_instantiations_in_stmt(body, ctx, module, idx);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_instantiations_in_stmt(init_stmt, ctx, module, idx);
            }
            if let Some(cond) = condition {
                collect_instantiations_in_expr(cond, ctx, module, idx);
            }
            if let Some(upd) = update {
                collect_instantiations_in_expr(upd, ctx, module, idx);
            }
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        Stmt::Throw(expr) => collect_instantiations_in_expr(expr, ctx, module, idx),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
            if let Some(c) = catch {
                collect_instantiations_in_stmts(&c.body, ctx, module, idx);
            }
            if let Some(f) = finally {
                collect_instantiations_in_stmts(f, ctx, module, idx);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_instantiations_in_expr(discriminant, ctx, module, idx);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_instantiations_in_expr(test, ctx, module, idx);
                }
                collect_instantiations_in_stmts(&case.body, ctx, module, idx);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) => {}
    }
}

fn collect_instantiations_in_expr(
    expr: &Expr,
    ctx: &mut MonomorphizationContext,
    module: &Module,
    idx: &ModuleIndex,
) {
    match expr {
        // Check for generic function calls
        Expr::Call {
            callee,
            args,
            type_args,
            ..
        } => {
            // First collect in the callee and args
            collect_instantiations_in_expr(callee, ctx, module, idx);
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }

            // Check if callee is a function reference
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                // Find the function and check if it's generic
                if let Some(&fi) = idx.func_by_id.get(func_id) {
                    let func = &module.functions[fi];
                    if !func.type_params.is_empty() {
                        // Use explicit type args if provided, otherwise try to infer
                        let resolved_type_args = if !type_args.is_empty() {
                            Some(type_args.clone())
                        } else {
                            // Try to infer type arguments from the call arguments
                            infer_type_args(func, args, module, idx)
                        };

                        if let Some(ta) = resolved_type_args {
                            ctx.request_func_specialization(*func_id, ta);
                        }
                    }
                }
            }
        }

        // Check for generic class instantiation
        Expr::New {
            class_name,
            args,
            type_args,
            ..
        } => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }

            // Find the class
            if let Some(&ci) = idx.class_by_name.get(class_name.as_str()) {
                let class = &module.classes[ci];
                if !class.type_params.is_empty() {
                    // Use explicit type args if provided, otherwise try to infer from constructor
                    let resolved_type_args = if !type_args.is_empty() {
                        Some(type_args.clone())
                    } else if let Some(ref ctor) = class.constructor {
                        // Try to infer from constructor parameters
                        infer_type_args_for_class(class, ctor, args, module, idx)
                    } else {
                        None
                    };

                    if let Some(ta) = resolved_type_args {
                        // Never monomorphize a class instantiation whose type
                        // argument is still an unresolved type variable, e.g.
                        // `new Observable<R>()` inside `lift<R>()`. TypeScript
                        // class generics are erased at runtime, so `new C<R>()`
                        // must construct the SAME class `C` as `new C()`.
                        // Specializing on an unknown type var instead produced a
                        // bogus `C$R` class whose instances are NOT `instanceof
                        // C` and do not inherit `C`'s prototype (e.g. rxjs's
                        // `Observable.prototype[Symbol.observable]`). That broke
                        // rxjs `innerFrom`/`from` ObservableInput detection —
                        // every NestJS interceptor (`next.handle().pipe(...)`)
                        // 500'd because the piped Observable failed both the
                        // `input instanceof Observable` and `isInteropObservable`
                        // checks. Erase the type args and construct the base class.
                        if !ta
                            .iter()
                            .any(crate::monomorph::infer::type_contains_type_var)
                        {
                            ctx.request_class_specialization(class_name, ta);
                        }
                    }
                }
            }
        }

        // Recurse into other expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            collect_instantiations_in_expr(val, ctx, module, idx);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_instantiations_in_expr(left, ctx, module, idx);
            collect_instantiations_in_expr(right, ctx, module, idx);
        }
        Expr::Unary { operand, .. } => {
            collect_instantiations_in_expr(operand, ctx, module, idx);
        }
        Expr::PropertyGet { object, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::PropertyUpdate { object, .. } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
        }
        Expr::IndexGet { object, index } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(index, ctx, module, idx);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_instantiations_in_expr(object, ctx, module, idx);
            collect_instantiations_in_expr(index, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_instantiations_in_expr(v, ctx, module, idx);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_instantiations_in_expr(v, ctx, module, idx);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::ArraySpread(elems) => {
            for e in elems {
                match e {
                    ArrayElement::Expr(expr) => {
                        collect_instantiations_in_expr(expr, ctx, module, idx)
                    }
                    ArrayElement::Spread(expr) => {
                        collect_instantiations_in_expr(expr, ctx, module, idx)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_instantiations_in_expr(condition, ctx, module, idx);
            collect_instantiations_in_expr(then_expr, ctx, module, idx);
            collect_instantiations_in_expr(else_expr, ctx, module, idx);
        }
        Expr::TypeOf(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::Void(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                collect_instantiations_in_expr(v, ctx, module, idx);
            }
        }
        Expr::InstanceOf { expr, .. } => collect_instantiations_in_expr(expr, ctx, module, idx),
        Expr::Await(inner) => collect_instantiations_in_expr(inner, ctx, module, idx),
        Expr::SuperCall(args) => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            collect_instantiations_in_expr(home, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(receiver, ctx, module, idx);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            collect_instantiations_in_expr(home, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
            collect_instantiations_in_expr(receiver, ctx, module, idx);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            collect_instantiations_in_expr(home, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(receiver, ctx, module, idx);
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::NativeArenaAlloc(size) | Expr::NativeArenaDispose(size) => {
            collect_instantiations_in_expr(size, ctx, module, idx);
        }
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            collect_instantiations_in_expr(owner, ctx, module, idx);
            collect_instantiations_in_expr(byte_offset, ctx, module, idx);
            collect_instantiations_in_expr(length, ctx, module, idx);
        }
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            collect_instantiations_in_expr(owner, ctx, module, idx);
            collect_instantiations_in_expr(byte_offset, ctx, module, idx);
            collect_instantiations_in_expr(count, ctx, module, idx);
        }
        Expr::NativeMemoryFillU32 { view, value } => {
            collect_instantiations_in_expr(view, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::NativeMemoryCopy { dst, src } => {
            collect_instantiations_in_expr(dst, ctx, module, idx);
            collect_instantiations_in_expr(src, ctx, module, idx);
        }
        Expr::FsReadFileSync(path) => collect_instantiations_in_expr(path, ctx, module, idx),
        Expr::FsWriteFileSync(path, content) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
            collect_instantiations_in_expr(content, ctx, module, idx);
        }
        Expr::FsExistsSync(path) | Expr::FsMkdirSync(path) | Expr::FsUnlinkSync(path) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
        }
        Expr::FsAppendFileSync(path, content) => {
            collect_instantiations_in_expr(path, ctx, module, idx);
            collect_instantiations_in_expr(content, ctx, module, idx);
        }
        Expr::PathJoin(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b)
        | Expr::PathWin32Join(a, b) => {
            collect_instantiations_in_expr(a, ctx, module, idx);
            collect_instantiations_in_expr(b, ctx, module, idx);
        }
        Expr::PathDirname(p)
        | Expr::PathBasename(p)
        | Expr::PathExtname(p)
        | Expr::PathResolve(p)
        | Expr::PathIsAbsolute(p)
        | Expr::PathToNamespacedPath(p) => {
            collect_instantiations_in_expr(p, ctx, module, idx);
        }
        Expr::PathWin32 { args, .. } => {
            for e in args {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::ArrayPush { value, .. }
        | Expr::ArrayUnshift { value, .. }
        | Expr::ArrayPushSpread { source: value, .. } => {
            collect_instantiations_in_expr(value, ctx, module, idx);
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
            collect_instantiations_in_expr(array, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
            if let Some(fi) = from_index {
                collect_instantiations_in_expr(fi, ctx, module, idx);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            collect_instantiations_in_expr(array, ctx, module, idx);
            collect_instantiations_in_expr(start, ctx, module, idx);
            if let Some(e) = end {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::ArraySplice {
            array_id: _,
            start,
            delete_count,
            items,
        } => {
            collect_instantiations_in_expr(start, ctx, module, idx);
            if let Some(dc) = delete_count {
                collect_instantiations_in_expr(dc, ctx, module, idx);
            }
            for item in items {
                collect_instantiations_in_expr(item, ctx, module, idx);
            }
        }
        Expr::StringSplit(string, delimiter) => {
            collect_instantiations_in_expr(string, ctx, module, idx);
            collect_instantiations_in_expr(delimiter, ctx, module, idx);
        }
        Expr::StringFromCharCode(code) | Expr::StringFromCharCodeSpread(code) => {
            collect_instantiations_in_expr(code, ctx, module, idx);
        }
        Expr::MapNew => {}
        Expr::MapNewFromArray(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::MapSet { map, key, value } => {
            collect_instantiations_in_expr(map, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            collect_instantiations_in_expr(map, ctx, module, idx);
            collect_instantiations_in_expr(key, ctx, module, idx);
        }
        Expr::MapSize(map)
        | Expr::MapClear(map)
        | Expr::MapEntries(map)
        | Expr::MapKeys(map)
        | Expr::MapValues(map) => {
            collect_instantiations_in_expr(map, ctx, module, idx);
        }
        Expr::SetNew => {}
        Expr::SetNewFromArray(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::SetAdd { set_id: _, value } => {
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            collect_instantiations_in_expr(set, ctx, module, idx);
            collect_instantiations_in_expr(value, ctx, module, idx);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            collect_instantiations_in_expr(set, ctx, module, idx);
        }
        // JSON operations
        Expr::JsonParse(expr)
        | Expr::JsonStringify(expr)
        | Expr::JsonRawJson(expr)
        | Expr::JsonIsRawJson(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        // Math operations
        Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathTrunc(expr)
        | Expr::MathSign(expr)
        | Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathLog(expr)
        | Expr::MathLog2(expr)
        | Expr::MathLog10(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            collect_instantiations_in_expr(base, ctx, module, idx);
            collect_instantiations_in_expr(exp, ctx, module, idx);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args {
                collect_instantiations_in_expr(arg, ctx, module, idx);
            }
        }
        Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
            collect_instantiations_in_expr(e, ctx, module, idx);
        }
        Expr::MathRandom => {}
        // Crypto operations
        Expr::CryptoRandomBytes(expr) | Expr::CryptoSha256(expr) | Expr::CryptoMd5(expr) => {
            collect_instantiations_in_expr(expr, ctx, module, idx);
        }
        Expr::CryptoRandomUUID => {}
        Expr::CryptoRandomUUIDv7 => {}
        // Date operations
        Expr::DateNow => {}
        Expr::DateNew(args) => {
            for a in args {
                collect_instantiations_in_expr(a, ctx, module, idx);
            }
        }
        Expr::DateGetTime(date)
        | Expr::DateToISOString(date)
        | Expr::DateGetFullYear(date)
        | Expr::DateGetMonth(date)
        | Expr::DateGetDate(date)
        | Expr::DateGetDay(date)
        | Expr::DateGetHours(date)
        | Expr::DateGetMinutes(date)
        | Expr::DateGetSeconds(date)
        | Expr::DateGetMilliseconds(date) => {
            collect_instantiations_in_expr(date, ctx, module, idx);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_instantiations_in_expr(e, ctx, module, idx);
            }
        }
        Expr::Closure { body, .. } => {
            collect_instantiations_in_stmts(body, ctx, module, idx);
        }
        // Primitives and simple references don't need processing
        _ => {}
    }
}
