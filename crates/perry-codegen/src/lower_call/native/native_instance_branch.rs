{
    // perry/ui instance method calls: `windowHandle.show()`, `windowHandle.setBody(w)`, etc.
    // The HIR produces these with `object: Some(handle)` and `module: "perry/ui"`.
    // Lower the receiver to get the widget/window handle, then dispatch.
    if module == "perry/ui" {
        let recv_val = lower_expr(ctx, recv)?;
        let blk = ctx.block();
        let handle = unbox_to_i64(blk, &recv_val);
        if let Some(sig) = perry_ui_instance_method_lookup(method) {
            let uniform_padding =
                sig.method == "setPadding" && args.len() == 1 && sig.args.len() == 4;
            if sig.method == "setPadding" && !uniform_padding && args.len() != sig.args.len() {
                bail!(
                    "perry/ui: '.{}(...)' takes {} argument(s), but it was called with {}.",
                    method,
                    sig.args.len(),
                    args.len()
                );
            }
            // Build args: handle is the first arg, then the call args.
            let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
                Vec::with_capacity(1 + sig.args.len());
            let mut runtime_param_types: Vec<crate::types::LlvmType> =
                Vec::with_capacity(1 + sig.args.len());
            llvm_args.push((I64, handle));
            runtime_param_types.push(I64);
            let mut ui_args_group: Option<crate::rooting::RootedGroup<'_>> = None;
            if uniform_padding {
                // A source expression is evaluated once even when the native
                // four-edge ABI consumes the resulting value four times.
                let value = lower_expr(ctx, &args[0])?;
                for _ in 0..4 {
                    llvm_args.push((DOUBLE, value.clone()));
                    runtime_param_types.push(DOUBLE);
                }
            } else {
                // #11789 sweep: heap-valued arguments are rooted across the
                // ones after them and converted from the re-read.
                ui_args_group = Some(crate::lower_call::ui_tables::lower_ui_args_by_kind(
                    ctx,
                    &sig.args,
                    &args[..sig.args.len().min(args.len())],
                    &mut llvm_args,
                    &mut runtime_param_types,
                )?);
            }
            let return_type = match sig.ret {
                UiReturnKind::Widget
                | UiReturnKind::Promise
                | UiReturnKind::I64AsF64
                | UiReturnKind::I64AsBool => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
                UiReturnKind::Str => I64,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            let ui_result: Result<String> = match sig.ret {
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
                UiReturnKind::I64AsBool => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::lower_call::ui_tables::box_i64_boolean_result(blk, &raw))
                }
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
            };
            if let Some(group) = ui_args_group {
                group.release(ctx);
            }
            return ui_result;
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
            // #11789 sweep: as for the perry/ui instance methods above.
            let plugin_args_group = crate::lower_call::ui_tables::lower_ui_args_by_kind(
                ctx,
                &sig.args,
                &args[..sig.args.len().min(args.len())],
                &mut llvm_args,
                &mut runtime_param_types,
            )?;
            let return_type = match sig.ret {
                UiReturnKind::Widget
                | UiReturnKind::Promise
                | UiReturnKind::I64AsF64
                | UiReturnKind::I64AsBool
                | UiReturnKind::Str => I64,
                UiReturnKind::F64 => DOUBLE,
                UiReturnKind::Void => crate::types::VOID,
            };
            ctx.pending_declares
                .push((sig.runtime.to_string(), return_type, runtime_param_types));
            let ref_args: Vec<(crate::types::LlvmType, &str)> =
                llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let blk = ctx.block();
            let plugin_result: Result<String> = match sig.ret {
                UiReturnKind::Void => {
                    blk.call_void(sig.runtime, &ref_args);
                    Ok(double_literal(0.0))
                }
                UiReturnKind::Widget | UiReturnKind::Promise => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_pointer_inline(blk, &raw))
                }
                UiReturnKind::F64 => Ok(blk.call(DOUBLE, sig.runtime, &ref_args)),
                UiReturnKind::I64AsBool => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::lower_call::ui_tables::box_i64_boolean_result(blk, &raw))
                }
                UiReturnKind::I64AsF64 => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(blk.sitofp(I64, &raw, DOUBLE))
                }
                UiReturnKind::Str => {
                    let raw = blk.call(I64, sig.runtime, &ref_args);
                    Ok(crate::expr::nanbox_string_inline(blk, &raw))
                }
            };
            plugin_args_group.release(ctx);
            return plugin_result;
        }
        bail!(
            "perry/plugin: '.{}(...)' is not a known PluginApi method (args: {}). \
             See types/perry/plugin/index.d.ts for the supported API surface.",
            method,
            args.len()
        );
    }

    if module == "array" && method == "fill_generic" {
        // #11789 sweep: the receiver is held across every argument, each of
        // which is held across the ones after it.
        let (recv_box, lowered, fill_group) = super::lower_operands_rooted(ctx, recv, args)?;
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
        let result = ctx.block().call(
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
        );
        fill_group.release(ctx);
        return Ok(result);
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
        // #11789 sweep: the source is lowered first and held across the
        // receiver's lowering (a property read, which can run an accessor).
        let (src_box, recv_vals, push_group) =
            super::lower_operands_rooted(ctx, &args[0], std::slice::from_ref(recv))?;
        let arr_box = recv_vals[0].clone();
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
        push_group.release(ctx);
        // #11789 sweep: the length is read BEFORE the write-back, which can
        // re-lower the receiver's object expression (a property read) with
        // `new_handle` held in a register.
        let len_i32 = crate::expr::array_length::emit_array_length_i32(ctx, &new_handle);
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
                    if crate::scope_env::access::write_back_boxed_local(ctx, *id, &new_box)? {
                    } else if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property, .. } => {
                    // The pushed array is held across the object expression.
                    let mut wb_group = crate::rooting::open_rooted_group(1);
                    let wb_root = wb_group.adopt_emitted(
                        ctx,
                        crate::rooting::Repr::Boxed,
                        &new_box,
                        crate::rooting::operand_may_collect(ctx, obj_expr),
                    );
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let new_box = wb_group.reread_emitted(ctx, wb_root);
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
                    wb_group.release(ctx);
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        return Ok(ctx.block().uitofp(I32, &len_i32, DOUBLE));
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
        // A combined receiver-shape + nonnegative-index clone gives the ECS
        // `this.packed.push(x)` kernel two constructive facts the ordinary
        // native call cannot use: `x` already has a raw i32 slot, and the
        // receiver field read is guarded by the clone's exact `this` shape.
        // Route only that narrow form to the fused runtime entry. All other
        // calls retain the boxed-value push loop below.
        let u31_param = match (args, recv) {
            (
                [Expr::LocalGet(id)],
                Expr::PropertyGet {
                    object: obj_expr, ..
                },
            ) if ctx.proven_this.is_some()
                && matches!(obj_expr.as_ref(), Expr::This)
                && ctx.spec_i32_params.contains(id)
                && ctx.i32_counter_slots.contains_key(id) => Some(*id),
            _ => None,
        };
        let u31_value = if let Some(id) = u31_param {
            Some(lower_expr_as_i32(ctx, &Expr::LocalGet(id))?)
        } else {
            None
        };
        // #11789 sweep: the arguments are lowered BEFORE the receiver, so
        // each is held across the ones after it AND across the receiver's
        // lowering (a property read can run an accessor) — and across the
        // pushes of the arguments before it, each of which can grow the
        // array. The receiver-is-a-local, single-argument shape (`a.push(x)`)
        // has an empty window and keeps the IR it always had.
        let recv_collects = crate::rooting::operand_may_collect(ctx, recv);
        let mut push_group = crate::rooting::open_rooted_group(args.len());
        let mut push_roots: Vec<usize> = Vec::with_capacity(args.len());
        if u31_value.is_none() {
            for (i, a) in args.iter().enumerate() {
                let collects = recv_collects
                    || i > 0
                    || crate::rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
                push_roots.push(push_group.lower(ctx, a, collects)?);
            }
        }
        let arr_box = lower_expr(ctx, recv)?;
        // #10463: the fused push's length out-parameter is an entry-block
        // alloca; `blk.alloca` here grew the stack on every loop iteration.
        let length_slot = u31_value.as_ref().map(|_| ctx.func.alloca_entry(I32));
        let blk = ctx.block();
        let mut arr_handle = unbox_to_i64(blk, &arr_box);
        let orig_handle = arr_handle.clone();
        let fused_length_slot = if let Some((value, length_slot)) = u31_value.zip(length_slot) {
            let fast_handle = blk.call(
                I64,
                "js_array_push_u31_with_length",
                &[(I64, &arr_handle), (I32, &value), (PTR, &length_slot)],
            );
            // The fused entry is allocate-but-never-reenter: it answers null
            // for every receiver whose push can run user code (indexed
            // descriptors, Proxy traps, foreign families). Those take the same
            // complete guarded push the unfused lowering below performs.
            let handled = blk.icmp_ne(I64, &fast_handle, "0");
            let fast_end = blk.label.clone();
            let generic_idx = ctx.new_block("apush.u31.generic");
            let merge_idx = ctx.new_block("apush.u31.merge");
            let generic_label = ctx.block_label(generic_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block().cond_br(&handled, &merge_label, &generic_label);
            ctx.current_block = generic_idx;
            let generic_handle = {
                let blk = ctx.block();
                blk.call_void("js_array_push_guard", &[(I64, &orig_handle)]);
                let value_double = blk.sitofp(I32, &value, DOUBLE);
                blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &orig_handle), (DOUBLE, &value_double)],
                )
            };
            let generic_length = crate::expr::array_length::emit_array_length_i32(ctx, &generic_handle);
            ctx.block().store(I32, &generic_length, &length_slot);
            let generic_end = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            ctx.current_block = merge_idx;
            arr_handle = ctx.block().phi(
                I64,
                &[(&fast_handle, &fast_end), (&generic_handle, &generic_end)],
            );
            Some(length_slot)
        } else {
            // Spec §23.1.3.21: Set(O,"length",…,true) fires unconditionally — guard
            // even when args is empty so frozen / non-writable-length throw correctly.
            blk.call_void("js_array_push_guard", &[(I64, &arr_handle)]);
            for &root in &push_roots {
                // Re-read per element: the previous push may have grown the
                // array, so the register an argument was lowered into can be
                // stale.
                let v = push_group.reread(ctx, root)?;
                let blk = ctx.block();
                arr_handle =
                    blk.call(I64, "js_array_push_f64", &[(I64, &arr_handle), (DOUBLE, &v)]);
            }
            None
        };
        let blk = ctx.block();
        let new_handle = arr_handle;
        let new_box = nanbox_pointer_inline(blk, &new_handle);
        push_group.release(ctx);
        // #11789 sweep: the length is read BEFORE the write-back, which can
        // re-lower the receiver's object expression (a property read) with
        // `new_handle` held in a register.
        let len_i32 = if let Some(length_slot) = fused_length_slot {
            ctx.block().load(I32, &length_slot)
        } else {
            crate::expr::array_length::emit_array_length_i32(ctx, &new_handle)
        };
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
                    if crate::scope_env::access::write_back_boxed_local(ctx, *id, &new_box)? {
                    } else if let Some(slot) = ctx.locals.get(id).cloned() {
                        ctx.block().store(DOUBLE, &new_box, &slot);
                    } else if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                        let g_ref = format!("@{}", global_name);
                        emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
                    }
                }
                Expr::PropertyGet {
                    object: obj_expr,
                    property, .. } => {
                    // The pushed array is held across the object expression.
                    let mut wb_group = crate::rooting::open_rooted_group(1);
                    let wb_root = wb_group.adopt_emitted(
                        ctx,
                        crate::rooting::Repr::Boxed,
                        &new_box,
                        crate::rooting::operand_may_collect(ctx, obj_expr),
                    );
                    let obj_box = lower_expr(ctx, obj_expr)?;
                    let new_box = wb_group.reread_emitted(ctx, wb_root);
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
                    wb_group.release(ctx);
                }
                _ => unreachable!(),
            }
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
        }
        return Ok(ctx.block().uitofp(I32, &len_i32, DOUBLE));
    }

    if module == "array" && (method == "pop_back" || method == "pop") {
        if !args.is_empty() {
            bail!("array.pop expects 0 args, got {}", args.len());
        }
        let arr_box = lower_expr(ctx, recv)?;
        // Inline plain-array tier with `js_array_pop_f64` behind it
        // (`expr/array_pop.rs`): `this.packed.pop()` on an erased field is
        // this route, and it is the 8–10% pop self time in the wolf-ecs
        // entity cycle.
        return Ok(crate::expr::array_pop::lower_array_pop_inline(
            ctx, &arr_box,
        ));
    }

    // Generic native module dispatch (with receiver): fastify instance
    // methods (app.get, app.listen, conn.query, etc.), mysql2, ws, pg,
    // mongodb, better-sqlite3, etc.
    if let Some(sig) = native_module_lookup(module, true, method, class_name) {
        return lower_native_module_dispatch(ctx, sig, Some(recv), args);
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
    // #11789 sweep: the receiver is held across every argument, each of which
    // is held across the ones after it.
    let (recv_box, lowered_args, call_group) = super::lower_operands_rooted(ctx, recv, args)?;
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
    let result = ctx.block().call(
        DOUBLE,
        "js_native_call_method_nullsafe",
        &[
            (DOUBLE, &recv_box),
            (PTR, &bytes_global),
            (I64, &name_len),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    call_group.release(ctx);
    Ok(result)
}
