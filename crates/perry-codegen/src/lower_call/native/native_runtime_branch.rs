{
    if module == "__perry_runtime" && class_name.is_none() && object.is_none() {
        match method {
            "importMetaResolve" | "importMetaResolveValue" => {
                // #11789 sweep: `import.meta.resolve(specifier, parent)` holds
                // the specifier across the parent's evaluation.
                let (values, arg_group) = super::lower_call_args_rooted(ctx, args)?;
                let values = values
                    .iter()
                    .map(|value| (DOUBLE, value.as_str()))
                    .collect::<Vec<_>>();
                let name = if method == "importMetaResolve" {
                    "js_import_meta_resolve"
                } else {
                    "js_import_meta_resolve_value"
                };
                let result = ctx.block().call(DOUBLE, name, &values);
                arg_group.release(ctx);
                return Ok(result);
            }
            "iteratorNextMethod" => {
                let iter = lower_expr(ctx, &args[0])?;
                return Ok(ctx.block().call(DOUBLE, "js_iterator_next_method", &[(DOUBLE, &iter)]));
            }
            "iteratorStep" => {
                let (values, roots) = super::lower_call_args_rooted(ctx, &args[..2])?;
                let iter = &values[0];
                let next = &values[1];
                let Expr::LocalSet(value_id, output_expr) = &args[2] else {
                    return Err(anyhow::anyhow!("iteratorStep requires a compiler-owned output binding"));
                };
                let slot = ctx.func.alloca_entry(DOUBLE);
                let out = if matches!(args.get(3), Some(Expr::Bool(false))) { "null" } else { &slot };
                let done = ctx.block().call(
                    I32, "js_iterator_step", &[(DOUBLE, iter), (DOUBLE, next), (crate::types::PTR, out)],
                );
                let value = if out == "null" { double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)) } else { ctx.block().load(DOUBLE, &slot) };
                roots.release(ctx);
                crate::expr::invalidate_local_write_facts(ctx, *value_id);
                // Value can carry a heap edge. The regular local writer owns
                // its root/barrier and also handles module and captured slots.
                crate::expr::bind_lowered_value_to_local(ctx, *value_id, &value, output_expr)?;
                return Ok(crate::expr::i32_bool_to_nanbox(ctx.block(), &done));
            }
            "iteratorNextResult" => {
                let iter = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_iterator_next_result", &[(DOUBLE, &iter)]));
            }
            // #10524: the runtime guard of array destructuring over a source
            // with no static array proof — a NaN-boxed boolean, the same shape
            // `Expr::ArrayIterationPatched` produces for the proven arm.
            "arrayDestructureNeedsIterator" => {
                let source = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                let blk = ctx.block();
                let needs = blk.call(
                    I32,
                    "js_array_destructure_needs_iterator",
                    &[(DOUBLE, &source)],
                );
                return Ok(crate::expr::i32_bool_to_nanbox(blk, &needs));
            }
            "iteratorCloseIfNotDone" | "iteratorCloseOnThrow" => {
                let count = if method == "iteratorCloseOnThrow" { 3 } else { 2 };
                let (values, roots) = super::lower_call_args_rooted(ctx, &args[..args.len().min(count)])?;
                let values = values.iter().map(|v| (DOUBLE, v.as_str())).collect::<Vec<_>>();
                let name = if count == 3 { "js_iterator_close_on_throw" } else { "js_iterator_close_if_not_done" };
                let result = ctx.block().call(DOUBLE, name, &values);
                roots.release(ctx);
                return Ok(result);
            }
            "requireObjectCoercible" => {
                let val = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                // #5247 (coverage gap): under `--debug-symbols`, the
                // destructuring lowering passes the object-pattern's source byte
                // offset as a second literal arg. Emit a `js_set_call_location`
                // immediately before the coercibility check so the
                // "Cannot convert undefined or null to object" throw renders
                // `at <file>:<line>` for THIS destructure rather than the stale
                // last-tracked call (which can be in an unrelated module). No-op
                // in the default build (offset arg absent / locations disabled).
                if ctx.strings.debug_locations_enabled() {
                    if let Some(Expr::Number(off)) = args.get(1) {
                        let byte_offset = *off as u32;
                        crate::expr::calls::emit_call_location_at(ctx, byte_offset);
                    }
                }
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_require_object_coercible",
                    &[(DOUBLE, &val)],
                ));
            }
            "iteratorRestToArray" => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
                let iter = arg_values.first().cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let next = arg_values.get(1).cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let done = arg_values.get(2).cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let result = ctx.block().call(
                    DOUBLE,
                    "js_iterator_step_rest_to_array",
                    &[(DOUBLE, &iter), (DOUBLE, &next), (DOUBLE, &done)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            // Next.js wall 53: runtime `require(absolutePath.json)` fallback.
            "requireJsonDisk" => {
                let specifier = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_require_json_disk",
                    &[(DOUBLE, &specifier)],
                ));
            }
            // require.resolve node_modules subpath fallback.
            "requireResolveNodeModules" => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let from = arg_values.first().cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let specifier = arg_values.get(1).cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let result = ctx.block().call(
                    DOUBLE,
                    "js_require_resolve_node_modules",
                    &[(DOUBLE, &from), (DOUBLE, &specifier)],
                );
                arg_group.release(ctx);
                return Ok(result);
            }
            // Publish the CJS record before its body so same-thread recursive
            // requires see its current exports, including replacements.
            "registerPathModulePartial" => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let path = arg_values.first().cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let exports = arg_values.get(1).cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                ctx.block().call_void(
                    "js_register_path_module_partial",
                    &[(DOUBLE, &path), (DOUBLE, &exports)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            // #6769: link `module.parent` before the wrapper body runs.
            "linkPathModuleParent" => {
                let record = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                ctx.block()
                    .call_void("js_link_path_module_parent", &[(DOUBLE, &record)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // Next.js wall 54: publish the final exports by absolute path.
            "registerPathModule" => {
                let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
                let path = arg_values.first().cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                let exports = arg_values.get(1).cloned().unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                ctx.block().call_void(
                    "js_register_path_module",
                    &[(DOUBLE, &path), (DOUBLE, &exports)],
                );
                let result = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                arg_group.release(ctx);
                return Ok(result);
            }
            // Next.js wall 54: resolve runtime `require(absolutePath.js)`.
            "requirePathModule" => {
                let path = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_require_path_module", &[(DOUBLE, &path)]));
            }
            // Presence bit for a real path module whose export value is
            // JavaScript `undefined` (which cannot itself signal a hit).
            "hasPathModule" => {
                let path = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                return Ok(ctx
                    .block()
                    .call(DOUBLE, "js_has_path_module", &[(DOUBLE, &path)]));
            }
            // #10735: publish/read the shared CJS "main module" —
            // `require.main` for the process entry module vs. for every
            // other CJS-wrapped module. See `cjs_wrap/wrap.rs` and
            // `perry-runtime::module_require::{js_set_cjs_main_module,
            // js_get_cjs_main_module}`.
            "setCjsMainModule" => {
                let module = args.first().map_or_else(
                    || Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))),
                    |arg| lower_expr(ctx, arg),
                )?;
                ctx.block()
                    .call_void("js_set_cjs_main_module", &[(DOUBLE, &module)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "getCjsMainModule" => {
                return Ok(ctx.block().call(DOUBLE, "js_get_cjs_main_module", &[]));
            }
            // #10360: seeded into every module init under `--platform bun`
            // so the runtime can follow Bun where its Web APIs differ from
            // Node's (e.g. the Response null-body-status check).
            "setBunPlatform" => {
                ctx.block().call_void("js_set_bun_platform", &[]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            _ => {}
        }
    }

    // Web Fetch API dispatch — Response / Headers / Request / static
    // factories. Handled before the receiver-less early-out so that
    // `Response.json(v)` (object.is_none()) finds its runtime function.
    if let Some(val) = lower_fetch_native_method(ctx, module, method, object, args)? {
        return Ok(val);
    }

    // `perry/i18n.t(key, params?)` is the i18n entry point. The
    // perry-transform i18n pass already replaced the first arg with
    // an `Expr::I18nString { key, string_idx, params, ... }` containing
    // all the metadata the codegen needs to resolve the translation
    // at compile time. The wrapping `t()` call is therefore identity:
    // we just lower `args[0]` (the I18nString) and return its value.
    // Without this case, the receiver-less early-out below would
    // discard the I18nString and return `double 0.0`, which prints
    // as `0` instead of the translated text — the symptom that broke
    // the v0.5.7 i18n test before this fix landed.
    if module == "perry/i18n" && method == "t" && object.is_none() {
        if let Some(first) = args.first() {
            return lower_expr(ctx, first);
        }
    }

    // Node util.types predicate calls lower to a receiver-less
    // NativeMethodCall with either the direct `util/types` key or the
    // object-valued `util.types` namespace key.
    if matches!(module, "util/types" | "util.types") && class_name.is_none() && object.is_none() {
        if method == "isAsyncFunction" {
            if let Some(is_async) = args
                .first()
                .and_then(|arg| util_types_arg_is_async_function_static(ctx, arg))
            {
                return Ok(nanbox_bool_literal(is_async));
            }
            let value = if let Some(first) = args.first() {
                lower_expr(ctx, first)?
            } else {
                double_literal(0.0)
            };
            return Ok(ctx.block().call(
                DOUBLE,
                "js_util_types_is_async_function",
                &[(DOUBLE, &value)],
            ));
        }
        let runtime = match method {
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
        };
        if let Some(runtime) = runtime {
            let value = if let Some(first) = args.first() {
                lower_expr(ctx, first)?
            } else {
                crate::nanbox::double_literal(0.0)
            };
            return Ok(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &value)]));
        }
    }

    // `BigInt.asIntN(bits, x)` / `BigInt.asUintN(bits, x)` (#bigint statics).
    // Lowered to a receiver-less NativeMethodCall on the "bigint" module; emit
    // a direct call to the runtime entry (ToIndex + BigInt brand check +
    // two's-complement wrap).
    if module == "bigint" && object.is_none() {
        let runtime = match method {
            "asIntN" => Some("js_bigint_as_int_n_call"),
            "asUintN" => Some("js_bigint_as_uint_n_call"),
            _ => None,
        };
        if let Some(runtime) = runtime {
            // #11789 sweep: `bits` is held across the value's evaluation.
            let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, args)?;
            let bits = arg_values
                .first()
                .cloned()
                .unwrap_or_else(|| crate::nanbox::double_literal(0.0));
            let value = arg_values
                .get(1)
                .cloned()
                .unwrap_or_else(|| crate::nanbox::double_literal(0.0));
            let result = ctx
                .block()
                .call(DOUBLE, runtime, &[(DOUBLE, &bits), (DOUBLE, &value)]);
            arg_group.release(ctx);
            return Ok(result);
        }
    }

    // node:perf_hooks → native/perf_hooks.rs (performance.* + PerformanceObserver).
    if let Some(v) = perf_hooks::lower_perf_hooks_method(ctx, module, method, object, args)? {
        return Ok(v);
    }

    // node:v8 (#3137/#3138/#3140). serialize/deserialize + heap-stat/snapshot
    // helpers route to the `js_v8_*` runtime entry points. All are receiver-less
    // statics.
    if module == "v8" && object.is_none() {
        let runtime = match method {
            "serialize" => Some(("js_v8_serialize", 1usize)),
            "deserialize" => Some(("js_v8_deserialize", 1)),
            "getHeapStatistics" => Some(("js_v8_get_heap_statistics", 0)),
            "getHeapCodeStatistics" => Some(("js_v8_get_heap_code_statistics", 0)),
            "getHeapSpaceStatistics" => Some(("js_v8_get_heap_space_statistics", 0)),
            "cachedDataVersionTag" => Some(("js_v8_cached_data_version_tag", 0)),
            "getHeapSnapshot" => Some(("js_v8_get_heap_snapshot", 1)),
            "writeHeapSnapshot" => Some(("js_v8_write_heap_snapshot", 2)),
            // #3679: diagnostic-control / coverage helpers — Node-shaped no-op
            // callables returning `undefined` (Perry has no V8 engine to drive
            // real flag mutation or coverage capture). Args are evaluated for
            // side effects then ignored.
            "setFlagsFromString"
            | "takeCoverage"
            | "stopCoverage"
            | "setHeapSnapshotNearHeapLimit" => Some(("js_v8_noop_undefined", 0)),
            _ => None,
        };
        if let Some((fname, arity)) = runtime {
            // #11789 sweep: every argument (the surplus ones, which Node
            // ignores, included) is evaluated in order and each is held across
            // the ones after it.
            let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, args)?;
            let lowered: Vec<String> = (0..arity)
                .map(|i| {
                    arg_values.get(i).cloned().unwrap_or_else(|| {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    })
                })
                .collect();
            let call_args: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
            let result = ctx.block().call(DOUBLE, fname, &call_args);
            arg_group.release(ctx);
            return Ok(result);
        }
    }

    // #3679: chained sub-namespace calls fold to a NativeMethodCall with a
    // `class_name` (`v8.startupSnapshot.isBuildingSnapshot()`,
    // `v8.promiseHooks.onInit(fn)`). Dispatch them statically.
    if module == "v8" {
        // startupSnapshot helpers ignore their arguments (Perry never builds a
        // snapshot); evaluate args for side effects then call the no-arg helper.
        let v8_sub = match (class_name, method) {
            (Some("startupSnapshot"), "isBuildingSnapshot") => Some("js_v8_is_building_snapshot"),
            (
                Some("startupSnapshot"),
                "addSerializeCallback" | "addDeserializeCallback" | "setDeserializeMainFunction",
            ) => Some("js_v8_throw_not_building_snapshot"),
            _ => None,
        };
        if let Some(fname) = v8_sub {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(ctx.block().call(DOUBLE, fname, &[]));
        }

        // #3139: promiseHooks registrars install real lifecycle hooks. Pass the
        // callback (onInit/&c.) or options object (createHook) as the first arg.
        let v8_hook = match (class_name, method) {
            (Some("promiseHooks"), "onInit") => Some("js_v8_promise_hooks_on_init"),
            (Some("promiseHooks"), "onBefore") => Some("js_v8_promise_hooks_on_before"),
            (Some("promiseHooks"), "onAfter") => Some("js_v8_promise_hooks_on_after"),
            (Some("promiseHooks"), "onSettled") => Some("js_v8_promise_hooks_on_settled"),
            (Some("promiseHooks"), "createHook") => Some("js_v8_promise_hooks_create_hook"),
            _ => None,
        };
        if let Some(fname) = v8_hook {
            // #11789 sweep: the callback is held across the surplus arguments.
            let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, args)?;
            let arg = arg_values.first().cloned().unwrap_or_else(|| {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            });
            let result = ctx.block().call(DOUBLE, fname, &[(DOUBLE, &arg)]);
            arg_group.release(ctx);
            return Ok(result);
        }

        // #3142: named-import GCProfiler instances lower their method calls to
        // NativeMethodCall with `class_name == "GCProfiler"`. Route those to
        // the same small runtime state machine as namespace-member calls.
        if class_name == Some("GCProfiler") && matches!(method, "start" | "stop") {
            if let Some(object) = object {
                // #11789 sweep: the profiler is held across the (ignored)
                // arguments' evaluation.
                let (recv, _extras, arg_group) = super::lower_operands_rooted(ctx, object, args)?;
                let fname = if method == "start" {
                    "js_v8_gc_profiler_start"
                } else {
                    "js_v8_gc_profiler_stop"
                };
                let result = ctx.block().call(DOUBLE, fname, &[(DOUBLE, &recv)]);
                arg_group.release(ctx);
                return Ok(result);
            }
        }
    }

    if module == "crypto"
        && class_name == Some("ECDH")
        && method == "convertKey"
        && object.is_none()
    {
        // #11789 sweep: key, encodings and format are each held across the
        // ones after them.
        let (arg_values, arg_group) = super::lower_call_args_rooted(ctx, args)?;
        let lowered: Vec<String> = (0..5)
            .map(|i| {
                arg_values.get(i).cloned().unwrap_or_else(|| {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                })
            })
            .collect();
        let blk = ctx.block();
        let result = blk.call(
            DOUBLE,
            "js_crypto_ecdh_convert_key",
            &[
                (DOUBLE, &lowered[0]),
                (DOUBLE, &lowered[1]),
                (DOUBLE, &lowered[2]),
                (DOUBLE, &lowered[3]),
                (DOUBLE, &lowered[4]),
            ],
        );
        arg_group.release(ctx);
        return Ok(result);
    }

    if module == "crypto"
        && class_name == Some("Certificate")
        && matches!(
            method,
            "verifySpkac" | "exportPublicKey" | "exportChallenge"
        )
        && object.is_none()
    {
        let input = if let Some(arg) = args.first() {
            lower_expr(ctx, arg)?
        } else {
            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
        };
        let runtime = match method {
            "verifySpkac" => "js_crypto_certificate_verify_spkac",
            "exportPublicKey" => "js_crypto_certificate_export_public_key",
            "exportChallenge" => "js_crypto_certificate_export_challenge",
            _ => unreachable!(),
        };
        return Ok(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &input)]));
    }
}
