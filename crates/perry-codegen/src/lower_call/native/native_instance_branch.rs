{
    // perry/ui instance method calls: `windowHandle.show()`, `windowHandle.setBody(w)`, etc.
    // The HIR produces these with `object: Some(handle)` and `module: "perry/ui"`.
    // Lower the receiver to get the widget/window handle, then dispatch.
    if module == "perry/ui" {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        if let Some(sig) = perry_ui_instance_method_lookup(method) {
            // Build args: handle is the first arg, then the call args.
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
                Vec::with_capacity(1 + args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> =
                Vec::with_capacity(1 + args.len());
            llvm_args.push((I64, handle));
            runtime_param_types.push(I64);
            for (kind, arg) in sig.args.iter().zip(args.iter()) {
                match kind {
                    UiArgKind::Widget => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let h = unbox_to_i64(blk, &v);
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::Str => {
                        let h = get_raw_string_ptr(ctx, arg)?;
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::F64 => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::Closure => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::I64Raw => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let i = blk.fptosi(DOUBLE, &v, I64);
                        llvm_args.push((I64, i));
                        runtime_param_types.push(I64);
                    }
                }
            }
            let return_type = match sig.ret {
                UiReturnKind::Widget | UiReturnKind::Promise | UiReturnKind::I64AsF64 => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
                UiReturnKind::Str => I64,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            return match sig.ret {
                UiReturnKind::Void => {
                    blk.call_void(sig.runtime, &ref_args);
                    Ok(double_literal(0.0))
                }
                UiReturnKind::Widget | UiReturnKind::Promise => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_pointer_inline(blk, &raw))
                }
                UiReturnKind::F64 => Ok(blk.call(DOUBLE, sig.runtime, &ref_args)),
                UiReturnKind::Str => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_string_inline(blk, &raw))
                }
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
            };
        }
        // Unknown instance method — fail the compile. Previously this
        // lowered the args for side effects and returned TAG_UNDEFINED,
        // which silently swallowed styling calls like `label.setColor(...)`
        // and `btn.setCornerRadius(...)` (see types/perry/ui/index.d.ts
        // for the real method surface — styling uses the free-function
        // `textSetColor(widget, r, g, b, a)` / `setCornerRadius(widget, r)`
        // forms, not instance methods on the widget handle).
        bail!(
            "perry/ui: '.{}(...)' is not a known instance method (args: {}). \
             See types/perry/ui/index.d.ts — widget styling uses free functions \
             like `textSetFontSize(label, 24)` and `widgetSetBackgroundColor(btn, r, g, b, a)`, \
             not instance-method setters.",
            method,
            args.len()
        );
    }

    // perry/plugin PluginApi instance methods: `api.registerHook(...)`, `api.emit(...)`, etc.
    // The HIR produces these with `object: Some(handle)` and `module: "perry/plugin"`.
    if module == "perry/plugin" {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        if let Some(sig) = perry_plugin_instance_method_lookup(method) {
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
                Vec::with_capacity(1 + args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> =
                Vec::with_capacity(1 + args.len());
            llvm_args.push((I64, handle));
            runtime_param_types.push(I64);
            for (kind, arg) in sig.args.iter().zip(args.iter()) {
                match kind {
                    UiArgKind::Widget => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let h = unbox_to_i64(blk, &v);
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::Str => {
                        let h = get_raw_string_ptr(ctx, arg)?;
                        llvm_args.push((I64, h));
                        runtime_param_types.push(I64);
                    }
                    UiArgKind::F64 | UiArgKind::Closure => {
                        let v = lower_expr(ctx, arg)?;
                        llvm_args.push((DOUBLE, v));
                        runtime_param_types.push(DOUBLE);
                    }
                    UiArgKind::I64Raw => {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        let i = blk.fptosi(DOUBLE, &v, I64);
                        llvm_args.push((I64, i));
                        runtime_param_types.push(I64);
                    }
                }
            }
            let return_type = match sig.ret {
                UiReturnKind::Widget
                | UiReturnKind::Promise
                | UiReturnKind::I64AsF64
                | UiReturnKind::Str => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            return match sig.ret {
                UiReturnKind::Void => {
                    blk.call_void(sig.runtime, &ref_args);
                    Ok(double_literal(0.0))
                }
                UiReturnKind::Widget | UiReturnKind::Promise => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_pointer_inline(blk, &raw))
                }
                UiReturnKind::F64 => Ok(blk.call(DOUBLE, sig.runtime, &ref_args)),
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
                UiReturnKind::Str => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_string_inline(blk, &raw))
                }
            };
        }
        bail!(
            "perry/plugin: '.{}(...)' is not a known PluginApi method (args: {}). \
             See types/perry/plugin/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    if module == "array" && method == "fill_generic" {
        let recv_box = lower_expr(ctx, recv)?;
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(lower_expr(ctx, arg)?);
        }
        let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let value = lowered
            .first()
            .cloned()
            .unwrap_or_else(|| undefined.clone());
        let (has_start, start) = if let Some(start) = lowered.get(1) {
            ("1".to_string(), start.clone())
        } else {
            ("0".to_string(), undefined.clone())
        };
        let (has_end, end) = if let Some(end) = lowered.get(2) {
            ("1".to_string(), end.clone())
        } else {
            ("0".to_string(), undefined)
        };
        return Ok(ctx.block().call(
            DOUBLE,
            "js_array_fill_generic",
            &[
                (DOUBLE, &recv_box),
                (DOUBLE, &value),
                (I32, &has_start),
                (DOUBLE, &start),
                (I32, &has_end),
                (DOUBLE, &end),
            ],
        ));
    }

    if module == "array" && method == "push_spread" {
        // Refs #488 drizzle-sqlite: `arr.push(...src)` shape. Pre-fix
        // this had no codegen arm — the catch-all at the end of this
        // function silently lowered receiver + args for side effects and
        // returned `0.0`. drizzle's `mergeQueries` does
        // `result.params.push(...query.params)` so SQL queries went out
        // with empty params and INSERT silently inserted nothing.
        //
        // The HIR shape from `expr_call.rs:4810` packs the spread arg as
        // `args[0]` (the inner spread expression), so we expect exactly
        // one arg with the source array.
        if args.len() != 1 {
            bail!(
                "array.push_spread expects exactly 1 arg, got {}",
                args.len()
            );
        }
        let src_box = lower_expr(ctx, &args[0])?;
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        let orig_handle = arr_handle.clone();
        let src_handle = unbox_to_i64(blk, &src_box);
        let blk = ctx.block();
        let new_handle = blk.call(
            I64,
            "js_array_push_spread_f64",
            &[(I64, &arr_handle), (I64, &src_handle)],
        );
        let blk = ctx.block();
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Same write-back-only-if-realloc'd pattern as push_single.
        let needs_writeback = matches!(recv, Expr::LocalGet(_) | Expr::PropertyGet { .. });
        if needs_writeback {
            let blk = ctx.block();
            let changed = blk.icmp_ne(I64, &new_handle, &orig_handle);
            let wb_idx = ctx.new_block("arr.push_spread.wb");
            let merge_idx = ctx.new_block("arr.push_spread.merge");
            let wb_label = ctx.block_label(wb_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&changed, &wb_label, &merge_label);

            ctx.current_block = wb_idx;
            match recv {
                Expr::LocalGet(id) => {
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property, .. } => {
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &new_box)],
                    );
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        let blk = ctx.block();
        let len_i32 = blk.call(I32, "js_array_length", &[(I64, &new_handle)]);
        return Ok(blk.sitofp(I32, &len_i32, DOUBLE));
    }

    if module == "array" && (method == "push_single" || method == "push") {
        // Lower every argument first so closures and string literals get
        // collected, then lower the receiver once. js_array_push_f64 may
        // realloc on each call, so we thread the returned pointer through
        // and write the final pointer back to the receiver — but ONLY
        // if it actually changed. The runtime returns the same pointer
        // when capacity was sufficient (no grow); the writeback is a
        // no-op in that case but still costs a `js_object_set_field_by_name`
        // call (~50-100 cycles) per push. With amortized doubling, real
        // reallocs are O(log N) of the total pushes — guarding the
        // writeback elides the overhead on the 99.9% no-realloc path.
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let mut arr_handle = unbox_to_i64(blk, &arr_box);
        let orig_handle = arr_handle.clone();
        // Spec §23.1.3.21: Set(O,"length",…,true) fires unconditionally — guard
        // even when args is empty so frozen / non-writable-length throw correctly.
        blk.call_void("js_array_push_guard", &[(I64, &arr_handle)]);
        for v in &lowered {
            let blk = ctx.block();
            arr_handle = blk.call(I64, "js_array_push_f64", &[(I64, &arr_handle), (DOUBLE, v)]);
        }
        let blk = ctx.block();
        let new_handle = arr_handle;
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        // Compare the (possibly-realloc'd) pointer against the original
        // and only run the writeback when it actually differs. Setup
        // wb / merge basic blocks so the write-back path is cold.
        // Match arms decide the writeback shape:
        //   1. recv = LocalGet(id)  → store back to the local's slot
        //   2. recv = PropertyGet { obj, prop } → set obj.prop = new_box
        //   3. anything else → no writeback (array may dangle on realloc,
        //      but we don't crash at codegen — same trade-off as before).
        let needs_writeback = matches!(recv, Expr::LocalGet(_) | Expr::PropertyGet { .. });
        if needs_writeback {
            let blk = ctx.block();
            let changed = blk.icmp_ne(I64, &new_handle, &orig_handle);
            let wb_idx = ctx.new_block("arr.push.wb");
            let merge_idx = ctx.new_block("arr.push.merge");
            let wb_label = ctx.block_label(wb_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&changed, &wb_label, &merge_label);

            ctx.current_block = wb_idx;
            match recv {
                Expr::LocalGet(id) => {
                    if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property, .. } => {
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let key_idx = ctx.strings.intern(property);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &new_box)],
                    );
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        let blk = ctx.block();
        let len_i32 = blk.call(I32, "js_array_length", &[(I64, &new_handle)]);
        return Ok(blk.sitofp(I32, &len_i32, DOUBLE));
    }

    if module == "array" && (method == "pop_back" || method == "pop") {
        if !args.is_empty() {
            bail!("array.pop expects 0 args, got {}", args.len());
        }
        let arr_box = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let arr_handle = unbox_to_i64(blk, &arr_box);
        return Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &arr_handle)]));
    }

    // Generic native module dispatch (with receiver): fastify instance
    // methods (app.get, app.listen, conn.query, etc.), mysql2, ws, pg,
    // ioredis, mongodb, better-sqlite3, etc.
    if let Some(sig) = native_module_lookup(module, true, method, class_name) {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        return lower_native_module_dispatch(ctx, sig, Some(&handle), args);
    }

    // Unknown native method: route to the runtime method dispatcher on the
    // ACTUAL receiver value instead of returning a 0.0 sentinel. The HIR can
    // mis-classify a receiver's class — a webpack closure-captured array `e`
    // gets registered as `FormData` (stale/aliased native-instance type), so
    // `e.indexOf(s)` lowers as `NativeMethodCall{FormData, "indexOf"}`. None of
    // the FormData arms match `indexOf`, and the old `0.0` sentinel made
    // `!~e.indexOf(s)` always 0 → the Next.js `__webpack_require__.t` interop
    // loop ran 0 iterations → empty React namespace → `cacheSignal is not a
    // function`. `js_native_call_method` dispatches on the runtime type, so a
    // real array receiver runs `Array.prototype.indexOf`, a real FormData runs
    // its method, etc. (Same shape as the `new Console(...)` instance path
    // above.) Falls back gracefully for genuinely-unimplemented modules too:
    // the dispatcher returns `undefined` rather than a misleading numeric 0.
    let recv_box = lower_expr(ctx, recv)?;
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for arg in args {
        lowered_args.push(lower_expr(ctx, arg)?);
    }
    let (args_ptr, args_len) = if lowered_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = lowered_args.len();
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        {
            let blk = ctx.block();
            for (i, value) in lowered_args.iter().enumerate() {
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                blk.store(DOUBLE, value, &slot);
            }
        }
        (buf, n.to_string())
    };
    let method_idx = ctx.strings.intern(method);
    let entry = ctx.strings.entry(method_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len = entry.byte_len.to_string();
    // #wall4: null-safe — dispatch real receivers (fixes the mis-typed array
    // `e.indexOf`), but a genuinely nullish receiver returns the 0.0 sentinel
    // instead of hard-throwing (so app-page-turbo's top-level nullish-receiver
    // `.indexOf` doesn't abort the whole external module load → 500).
    Ok(ctx.block().call(
        DOUBLE,
        "js_native_call_method_nullsafe",
        &[
            (DOUBLE, &recv_box),
            (PTR, &bytes_global),
            (I64, &name_len),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    ))
}
