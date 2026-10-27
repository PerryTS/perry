use super::*;

pub(super) fn util_types_arg_is_async_function_static(
    ctx: &FnCtx<'_>,
    expr: &Expr,
) -> Option<bool> {
    match expr {
        Expr::FuncRef(fid) => Some(ctx.local_async_funcs.contains(fid)),
        Expr::Closure { is_async, .. } => Some(*is_async),
        Expr::LocalGet(id) => match ctx.stable_local_type_proof(id) {
            Some(HirType::Function(ft)) => Some(ft.is_async),
            _ => None,
        },
        _ => None,
    }
}

/// Lower every argument of a `console.*` call into `group`, left to right, with
/// each already-evaluated value rooted across the evaluation of the ones that
/// follow it, and re-read below the last of them.
///
/// **Every** argument, including the ones the method ignores. Node evaluates a
/// call's whole argument list before it invokes the callee, so
/// `console.dir(o, opts, sideEffect())` runs `sideEffect` BEFORE anything is
/// printed. Two arms here used to evaluate the surplus arguments after the
/// print (`dir`) or not at all (`time` / `count`), which is an observable
/// difference rather than a formatting one — see #7649.
///
/// The window per argument is "the arguments after it": the consuming
/// `js_console_*` call is emitted immediately below the last one, and an
/// argument that is a literal or a proven non-pointer costs nothing, so a call
/// like `console.table(rows)` emits exactly the IR it emitted before.
pub(super) fn lower_console_args<'a>(
    ctx: &mut FnCtx<'_>,
    group: &mut crate::rooting::RootedGroup<'a>,
    args: &'a [Expr],
) -> Result<Vec<String>> {
    for (i, arg) in args.iter().enumerate() {
        let collects = any_operand_may_collect(ctx, args[i + 1..].iter());
        group.lower(ctx, arg, collects)?;
    }
    group.reread_all(ctx)
}

pub(super) fn nanbox_bool_literal(value: bool) -> String {
    double_literal(f64::from_bits(if value {
        crate::nanbox::TAG_TRUE
    } else {
        crate::nanbox::TAG_FALSE
    }))
}

pub(super) fn lower_util_types_predicate_arg(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::NativeMethodCall {
        module,
        class_name,
        method,
        object,
        args,
        ..
    } = expr
    else {
        return Ok(None);
    };
    let is_direct_util_types_module =
        matches!(module.as_str(), "util/types" | "util.types") && class_name.is_none();
    if !is_direct_util_types_module || object.is_some() {
        return Ok(None);
    }
    if method == "isAsyncFunction" {
        if let Some(is_async) = args
            .first()
            .and_then(|arg| util_types_arg_is_async_function_static(ctx, arg))
        {
            return Ok(Some(nanbox_bool_literal(is_async)));
        }
        let value = if let Some(first) = args.first() {
            lower_expr(ctx, first)?
        } else {
            double_literal(0.0)
        };
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_util_types_is_async_function",
            &[(DOUBLE, &value)],
        )));
    }
    let Some(runtime) = (match method.as_str() {
        "isArgumentsObject" => Some("js_util_types_is_arguments_object"),
        "isPromise" => Some("js_util_types_is_promise"),
        "isBigIntObject" => Some("js_util_types_is_big_int_object"),
        "isArrayBuffer" => Some("js_util_types_is_array_buffer"),
        "isSharedArrayBuffer" => Some("js_util_types_is_shared_array_buffer"),
        "isAnyArrayBuffer" => Some("js_util_types_is_any_array_buffer"),
        "isArrayBufferView" => Some("js_util_types_is_array_buffer_view"),
        "isDataView" => Some("js_util_types_is_data_view"),
        "isTypedArray" => Some("js_util_types_is_typed_array"),
        "isUint8Array" => Some("js_util_types_is_uint8_array"),
        "isInt8Array" => Some("js_util_types_is_int8_array"),
        "isInt16Array" => Some("js_util_types_is_int16_array"),
        "isUint16Array" => Some("js_util_types_is_uint16_array"),
        "isInt32Array" => Some("js_util_types_is_int32_array"),
        "isUint32Array" => Some("js_util_types_is_uint32_array"),
        "isFloat16Array" => Some("js_util_types_is_float16_array"),
        "isFloat32Array" => Some("js_util_types_is_float32_array"),
        "isFloat64Array" => Some("js_util_types_is_float64_array"),
        "isUint8ClampedArray" => Some("js_util_types_is_uint8_clamped_array"),
        "isBigInt64Array" => Some("js_util_types_is_big_int64_array"),
        "isBigUint64Array" => Some("js_util_types_is_big_uint64_array"),
        "isMap" => Some("js_util_types_is_map"),
        "isMapIterator" => Some("js_util_types_is_map_iterator"),
        "isProxy" => Some("js_util_types_is_proxy"),
        "isExternal" => Some("js_util_types_is_external"),
        "isModuleNamespaceObject" => Some("js_util_types_is_module_namespace_object"),
        "isSet" => Some("js_util_types_is_set"),
        "isSetIterator" => Some("js_util_types_is_set_iterator"),
        "isWeakMap" => Some("js_util_types_is_weak_map"),
        "isWeakSet" => Some("js_util_types_is_weak_set"),
        "isDate" => Some("js_util_types_is_date"),
        "isRegExp" => Some("js_util_types_is_reg_exp"),
        "isAsyncFunction" => Some("js_util_types_is_async_function"),
        "isGeneratorFunction" => Some("js_util_types_is_generator_function"),
        "isGeneratorObject" => Some("js_util_types_is_generator_object"),
        "isNativeError" => Some("js_util_types_is_native_error"),
        "isKeyObject" => Some("js_util_types_is_key_object"),
        "isCryptoKey" => Some("js_util_types_is_crypto_key"),
        "isNumberObject" => Some("js_util_types_is_number_object"),
        "isStringObject" => Some("js_util_types_is_string_object"),
        "isBooleanObject" => Some("js_util_types_is_boolean_object"),
        "isSymbolObject" => Some("js_util_types_is_symbol_object"),
        "isBoxedPrimitive" => Some("js_util_types_is_boxed_primitive"),
        _ => None,
    }) else {
        return Ok(None);
    };
    let value = if let Some(first) = args.first() {
        lower_expr(ctx, first)?
    } else {
        double_literal(0.0)
    };
    Ok(Some(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &value)])))
}
