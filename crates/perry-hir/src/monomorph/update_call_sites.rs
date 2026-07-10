use super::*;

/// Update call sites to use specialized versions
pub(crate) fn update_call_sites(module: &mut Module, ctx: &MonomorphizationContext) {
    // Build lookup table for inference (before mutating)
    let lookup = InferenceLookup::from_module(module);

    // Update all functions
    for func in &mut module.functions {
        update_call_sites_in_stmts(&mut func.body, ctx, &lookup);
    }

    // Update all class methods
    for class in &mut module.classes {
        if let Some(ref mut ctor) = class.constructor {
            update_call_sites_in_stmts(&mut ctor.body, ctx, &lookup);
        }
        for method in &mut class.methods {
            update_call_sites_in_stmts(&mut method.body, ctx, &lookup);
        }
        for method in &mut class.static_methods {
            update_call_sites_in_stmts(&mut method.body, ctx, &lookup);
        }
    }

    // Update init statements
    update_call_sites_in_stmts(&mut module.init, ctx, &lookup);
}

fn update_call_sites_in_stmts(
    stmts: &mut [Stmt],
    ctx: &MonomorphizationContext,
    lookup: &InferenceLookup,
) {
    for stmt in stmts {
        update_call_sites_in_stmt(stmt, ctx, lookup);
    }
}

fn update_call_sites_in_stmt(
    stmt: &mut Stmt,
    ctx: &MonomorphizationContext,
    lookup: &InferenceLookup,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                update_call_sites_in_expr(expr, ctx, lookup);
            }
        }
        Stmt::Expr(expr) => update_call_sites_in_expr(expr, ctx, lookup),
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_stmts(then_branch, ctx, lookup);
            if let Some(else_b) = else_branch {
                update_call_sites_in_stmts(else_b, ctx, lookup);
            }
        }
        Stmt::While { condition, body } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        Stmt::DoWhile { body, condition } => {
            update_call_sites_in_stmts(body, ctx, lookup);
            update_call_sites_in_expr(condition, ctx, lookup);
        }
        Stmt::Labeled { body, .. } => {
            update_call_sites_in_stmt(body, ctx, lookup);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                update_call_sites_in_stmt(init_stmt, ctx, lookup);
            }
            if let Some(cond) = condition {
                update_call_sites_in_expr(cond, ctx, lookup);
            }
            if let Some(upd) = update {
                update_call_sites_in_expr(upd, ctx, lookup);
            }
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        Stmt::Throw(expr) => update_call_sites_in_expr(expr, ctx, lookup),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            update_call_sites_in_stmts(body, ctx, lookup);
            if let Some(c) = catch {
                update_call_sites_in_stmts(&mut c.body, ctx, lookup);
            }
            if let Some(f) = finally {
                update_call_sites_in_stmts(f, ctx, lookup);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            update_call_sites_in_expr(discriminant, ctx, lookup);
            for case in cases {
                if let Some(ref mut test) = case.test {
                    update_call_sites_in_expr(test, ctx, lookup);
                }
                update_call_sites_in_stmts(&mut case.body, ctx, lookup);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn update_call_sites_in_expr(
    expr: &mut Expr,
    ctx: &MonomorphizationContext,
    lookup: &InferenceLookup,
) {
    match expr {
        // Update generic function calls to use specialized version
        Expr::Call {
            callee,
            args,
            type_args,
            ..
        } => {
            // First update the callee and args recursively
            update_call_sites_in_expr(callee, ctx, lookup);
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }

            // Check if callee is a FuncRef
            if let Expr::FuncRef(func_id) = callee.as_mut() {
                // Get resolved type args - either explicit or inferred
                let resolved_type_args = if !type_args.is_empty() {
                    Some(type_args.clone())
                } else if let Some(func_info) = lookup.funcs.get(func_id) {
                    if !func_info.type_params.is_empty() {
                        // Try to infer type arguments
                        infer_type_args_from_lookup(func_info, args, lookup)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // If we have type args (explicit or inferred), update to specialized version
                if let Some(ta) = resolved_type_args {
                    let mangled_args = mangle_type_args(&ta);
                    let key = (*func_id, mangled_args);
                    if let Some(&specialized_id) = ctx.specialized_funcs.get(&key) {
                        *func_id = specialized_id;
                        type_args.clear();
                    }
                }
            }
        }

        // Update generic class instantiation to use specialized class
        Expr::New {
            class_name,
            args,
            type_args,
            ..
        } => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }

            // Get resolved type args - either explicit or inferred
            let resolved_type_args = if !type_args.is_empty() {
                Some(type_args.clone())
            } else if let Some(class_info) = lookup.classes.get(class_name) {
                if !class_info.type_params.is_empty() {
                    // Try to infer type arguments from constructor
                    infer_type_args_for_class_from_lookup(class_info, args, lookup)
                } else {
                    None
                }
            } else {
                None
            };

            // If we have type args (explicit or inferred), update to specialized class
            if let Some(ta) = resolved_type_args {
                let mangled_args = mangle_type_args(&ta);
                let key = (class_name.clone(), mangled_args);
                if let Some(specialized_name) = ctx.specialized_classes.get(&key) {
                    *class_name = specialized_name.clone();
                    type_args.clear();
                }
            }
        }

        // Recurse into other expressions
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            update_call_sites_in_expr(val, ctx, lookup);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            update_call_sites_in_expr(left, ctx, lookup);
            update_call_sites_in_expr(right, ctx, lookup);
        }
        Expr::Unary { operand, .. } => {
            update_call_sites_in_expr(operand, ctx, lookup);
        }
        Expr::PropertyGet { object, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
        }
        Expr::PropertySet { object, value, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::PropertyUpdate { object, .. } => {
            update_call_sites_in_expr(object, ctx, lookup);
        }
        Expr::IndexGet { object, index } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(index, ctx, lookup);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            update_call_sites_in_expr(object, ctx, lookup);
            update_call_sites_in_expr(index, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                update_call_sites_in_expr(v, ctx, lookup);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts.iter_mut() {
                update_call_sites_in_expr(v, ctx, lookup);
            }
        }
        Expr::Array(elems) => {
            for e in elems.iter_mut() {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::ArraySpread(elems) => {
            for e in elems.iter_mut() {
                match e {
                    ArrayElement::Expr(expr) => update_call_sites_in_expr(expr, ctx, lookup),
                    ArrayElement::Spread(expr) => update_call_sites_in_expr(expr, ctx, lookup),
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            update_call_sites_in_expr(condition, ctx, lookup);
            update_call_sites_in_expr(then_expr, ctx, lookup);
            update_call_sites_in_expr(else_expr, ctx, lookup);
        }
        Expr::TypeOf(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::Void(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                update_call_sites_in_expr(v, ctx, lookup);
            }
        }
        Expr::InstanceOf { expr, ty_expr, .. } => {
            update_call_sites_in_expr(expr, ctx, lookup);
            if let Some(t) = ty_expr {
                update_call_sites_in_expr(t, ctx, lookup);
            }
        }
        Expr::Await(inner) => update_call_sites_in_expr(inner, ctx, lookup),
        Expr::SuperCall(args) => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            update_call_sites_in_expr(home, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(receiver, ctx, lookup);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            update_call_sites_in_expr(home, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
            update_call_sites_in_expr(receiver, ctx, lookup);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            update_call_sites_in_expr(home, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(receiver, ctx, lookup);
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                update_call_sites_in_expr(obj, ctx, lookup);
            }
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::NativeArenaAlloc(size) | Expr::NativeArenaDispose(size) => {
            update_call_sites_in_expr(size, ctx, lookup);
        }
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            update_call_sites_in_expr(owner, ctx, lookup);
            update_call_sites_in_expr(byte_offset, ctx, lookup);
            update_call_sites_in_expr(length, ctx, lookup);
        }
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            update_call_sites_in_expr(owner, ctx, lookup);
            update_call_sites_in_expr(byte_offset, ctx, lookup);
            update_call_sites_in_expr(count, ctx, lookup);
        }
        Expr::NativeMemoryFillU32 { view, value } => {
            update_call_sites_in_expr(view, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::NativeMemoryCopy { dst, src } => {
            update_call_sites_in_expr(dst, ctx, lookup);
            update_call_sites_in_expr(src, ctx, lookup);
        }
        Expr::FsReadFileSync(path) => update_call_sites_in_expr(path, ctx, lookup),
        Expr::FsWriteFileSync(path, content) => {
            update_call_sites_in_expr(path, ctx, lookup);
            update_call_sites_in_expr(content, ctx, lookup);
        }
        Expr::FsExistsSync(path) | Expr::FsMkdirSync(path) | Expr::FsUnlinkSync(path) => {
            update_call_sites_in_expr(path, ctx, lookup);
        }
        Expr::FsAppendFileSync(path, content) => {
            update_call_sites_in_expr(path, ctx, lookup);
            update_call_sites_in_expr(content, ctx, lookup);
        }
        Expr::PathJoin(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b)
        | Expr::PathWin32Join(a, b) => {
            update_call_sites_in_expr(a, ctx, lookup);
            update_call_sites_in_expr(b, ctx, lookup);
        }
        Expr::PathDirname(p)
        | Expr::PathBasename(p)
        | Expr::PathExtname(p)
        | Expr::PathResolve(p)
        | Expr::PathIsAbsolute(p)
        | Expr::PathToNamespacedPath(p) => {
            update_call_sites_in_expr(p, ctx, lookup);
        }
        Expr::PathWin32 { args, .. } => {
            for e in args {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::ArrayPush { value, .. }
        | Expr::ArrayUnshift { value, .. }
        | Expr::ArrayPushSpread { source: value, .. } => {
            update_call_sites_in_expr(value, ctx, lookup);
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
            update_call_sites_in_expr(array, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
            if let Some(fi) = from_index {
                update_call_sites_in_expr(fi, ctx, lookup);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            update_call_sites_in_expr(array, ctx, lookup);
            update_call_sites_in_expr(start, ctx, lookup);
            if let Some(e) = end {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::ArraySplice {
            array_id: _,
            start,
            delete_count,
            items,
        } => {
            update_call_sites_in_expr(start, ctx, lookup);
            if let Some(dc) = delete_count {
                update_call_sites_in_expr(dc, ctx, lookup);
            }
            for item in items {
                update_call_sites_in_expr(item, ctx, lookup);
            }
        }
        Expr::StringSplit(string, delimiter) => {
            update_call_sites_in_expr(string, ctx, lookup);
            update_call_sites_in_expr(delimiter, ctx, lookup);
        }
        Expr::StringFromCharCode(code) | Expr::StringFromCharCodeSpread(code) => {
            update_call_sites_in_expr(code, ctx, lookup);
        }
        Expr::MapNew => {}
        Expr::MapNewFromArray(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::MapSet { map, key, value } => {
            update_call_sites_in_expr(map, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            update_call_sites_in_expr(map, ctx, lookup);
            update_call_sites_in_expr(key, ctx, lookup);
        }
        Expr::MapSize(map)
        | Expr::MapClear(map)
        | Expr::MapEntries(map)
        | Expr::MapKeys(map)
        | Expr::MapValues(map) => {
            update_call_sites_in_expr(map, ctx, lookup);
        }
        Expr::SetNew => {}
        Expr::SetNewFromArray(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::SetAdd { set_id: _, value } => {
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            update_call_sites_in_expr(set, ctx, lookup);
            update_call_sites_in_expr(value, ctx, lookup);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            update_call_sites_in_expr(set, ctx, lookup);
        }
        // JSON operations
        Expr::JsonParse(expr)
        | Expr::JsonStringify(expr)
        | Expr::JsonRawJson(expr)
        | Expr::JsonIsRawJson(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
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
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            update_call_sites_in_expr(base, ctx, lookup);
            update_call_sites_in_expr(exp, ctx, lookup);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args.iter_mut() {
                update_call_sites_in_expr(arg, ctx, lookup);
            }
        }
        Expr::MathMinSpread(e) | Expr::MathMaxSpread(e) => {
            update_call_sites_in_expr(e, ctx, lookup);
        }
        Expr::MathRandom => {}
        // Crypto operations
        Expr::CryptoRandomBytes(expr) | Expr::CryptoSha256(expr) | Expr::CryptoMd5(expr) => {
            update_call_sites_in_expr(expr, ctx, lookup);
        }
        Expr::CryptoRandomUUID => {}
        Expr::CryptoRandomUUIDv7 => {}
        // Date operations
        Expr::DateNow => {}
        Expr::DateNew(args) => {
            for a in args {
                update_call_sites_in_expr(a, ctx, lookup);
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
            update_call_sites_in_expr(date, ctx, lookup);
        }
        Expr::Sequence(exprs) => {
            for e in exprs.iter_mut() {
                update_call_sites_in_expr(e, ctx, lookup);
            }
        }
        Expr::Closure { body, .. } => {
            update_call_sites_in_stmts(body, ctx, lookup);
        }
        // Primitives and simple references don't need updating
        _ => {}
    }
}

/// Infer type arguments using the lightweight FuncInfo (for update phase)
fn infer_type_args_from_lookup(
    func_info: &FuncInfo,
    args: &[Expr],
    lookup: &InferenceLookup,
) -> Option<Vec<Type>> {
    if func_info.type_params.is_empty() {
        return None;
    }

    let mut bindings: HashMap<String, Type> = HashMap::new();

    for (param_idx, param) in func_info.params.iter().enumerate() {
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        if param.is_rest {
            let arg_tys: Option<Vec<Type>> = args
                .iter()
                .skip(param_idx)
                .map(|arg| infer_expr_type_from_lookup(arg, lookup))
                .collect();
            if let Some(arg_tys) = arg_tys {
                if !unify_rest_param_types(&param.ty, &arg_tys, &mut bindings) {
                    return None;
                }
            }
            break;
        }

        if let Some(arg) = args.get(param_idx) {
            if let Some(arg_ty) = infer_expr_type_from_lookup(arg, lookup) {
                if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                    return None;
                }
            }
        } else {
            break;
        }
    }

    let mut inferred_args = Vec::new();
    for type_param in &func_info.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            inferred_args.push((**default).clone());
        } else {
            return None;
        }
    }

    Some(inferred_args)
}

/// Infer type arguments for class using the lightweight ClassInfo (for update phase)
fn infer_type_args_for_class_from_lookup(
    class_info: &ClassInfo,
    args: &[Expr],
    lookup: &InferenceLookup,
) -> Option<Vec<Type>> {
    if class_info.type_params.is_empty() {
        return None;
    }

    let ctor_params = class_info.constructor_params.as_ref()?;

    let mut bindings: HashMap<String, Type> = HashMap::new();

    for (param_idx, param) in ctor_params.iter().enumerate() {
        if !type_contains_type_var(&param.ty) {
            continue;
        }

        if param.is_rest {
            let arg_tys: Option<Vec<Type>> = args
                .iter()
                .skip(param_idx)
                .map(|arg| infer_expr_type_from_lookup(arg, lookup))
                .collect();
            if let Some(arg_tys) = arg_tys {
                if !unify_rest_param_types(&param.ty, &arg_tys, &mut bindings) {
                    return None;
                }
            }
            break;
        }

        if let Some(arg) = args.get(param_idx) {
            if let Some(arg_ty) = infer_expr_type_from_lookup(arg, lookup) {
                if !unify_types(&param.ty, &arg_ty, &mut bindings) {
                    return None;
                }
            }
        } else {
            break;
        }
    }

    let mut inferred_args = Vec::new();
    for type_param in &class_info.type_params {
        if let Some(ty) = bindings.get(&type_param.name) {
            inferred_args.push(ty.clone());
        } else if let Some(ref default) = type_param.default {
            inferred_args.push((**default).clone());
        } else {
            return None;
        }
    }

    Some(inferred_args)
}

/// Infer expression type using the lookup table (for update phase)
fn infer_expr_type_from_lookup(expr: &Expr, lookup: &InferenceLookup) -> Option<Type> {
    match expr {
        Expr::Number(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => Some(Type::Number),
        Expr::String(_) => Some(Type::String),
        Expr::Bool(_) => Some(Type::Boolean),
        Expr::Null => Some(Type::Null),
        Expr::Undefined => Some(Type::Void),
        Expr::NativeMemoryFillU32 { .. } | Expr::NativeMemoryCopy { .. } => Some(Type::Void),
        Expr::BigInt(_) => Some(Type::BigInt),

        Expr::Array(elems) => {
            if let Some(first) = elems.first() {
                if let Some(elem_ty) = infer_expr_type_from_lookup(first, lookup) {
                    return Some(Type::Array(Box::new(elem_ty)));
                }
            }
            Some(Type::Array(Box::new(Type::Any)))
        }

        Expr::Object(_) | Expr::ObjectSpread { .. } => Some(Type::Object(ObjectType::default())),

        Expr::Call {
            callee, type_args, ..
        } => {
            if let Expr::FuncRef(func_id) = callee.as_ref() {
                if let Some(func_info) = lookup.funcs.get(func_id) {
                    if !type_args.is_empty() && !func_info.type_params.is_empty() {
                        let subs: HashMap<String, Type> = func_info
                            .type_params
                            .iter()
                            .zip(type_args.iter())
                            .map(|(p, t)| (p.name.clone(), t.clone()))
                            .collect();
                        return Some(substitute_type(&func_info.return_type, &subs));
                    }
                    return Some(func_info.return_type.clone());
                }
            }
            None
        }

        Expr::New { class_name, .. } => Some(Type::Named(class_name.clone())),

        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                let left_ty = infer_expr_type_from_lookup(left, lookup);
                let right_ty = infer_expr_type_from_lookup(right, lookup);
                if matches!(left_ty, Some(Type::BigInt)) || matches!(right_ty, Some(Type::BigInt)) {
                    Some(Type::BigInt)
                } else {
                    Some(Type::Number)
                }
            }
            BinaryOp::UShr => Some(Type::Number),
        },

        Expr::Compare { .. } => Some(Type::Boolean),

        Expr::Logical { left, right, .. } => infer_expr_type_from_lookup(left, lookup)
            .or_else(|| infer_expr_type_from_lookup(right, lookup)),

        Expr::Unary { op, operand } => match op {
            UnaryOp::Neg | UnaryOp::BitNot => {
                if matches!(
                    infer_expr_type_from_lookup(operand, lookup),
                    Some(Type::BigInt)
                ) {
                    Some(Type::BigInt)
                } else {
                    Some(Type::Number)
                }
            }
            UnaryOp::Pos => Some(Type::Number),
            UnaryOp::Not => Some(Type::Boolean),
        },

        Expr::TypeOf(_) => Some(Type::String),
        Expr::Void(_) => Some(Type::Void),
        Expr::InstanceOf { .. } => Some(Type::Boolean),

        Expr::Conditional { then_expr, .. } => infer_expr_type_from_lookup(then_expr, lookup),

        _ => None,
    }
}
