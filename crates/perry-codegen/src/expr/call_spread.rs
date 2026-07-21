//! CallSpread (function call with spread args).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::native_value::MaterializationReason;
use crate::type_analysis::receiver_class_name;
use crate::types::{DOUBLE, I32, I64};

use super::{downgrade_buffer_aliases_in_expr, lower_expr, nanbox_pointer_inline, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::CallSpread { callee, args, .. } => {
            use perry_hir::CallArg;
            let spread_count = args
                .iter()
                .filter(|a| matches!(a, CallArg::Spread(_)))
                .count();
            let regular_count = args
                .iter()
                .filter(|a| matches!(a, CallArg::Expr(_)))
                .count();
            downgrade_buffer_aliases_in_expr(ctx, callee, MaterializationReason::UnknownCallEscape);
            for arg in args {
                match arg {
                    CallArg::Expr(expr) | CallArg::Spread(expr) => {
                        downgrade_buffer_aliases_in_expr(
                            ctx,
                            expr,
                            MaterializationReason::UnknownCallEscape,
                        )
                    }
                }
            }

            // console.log(...arr) / .info / .warn / .error / .debug — bundle
            // every regular arg + every spread source into a single array,
            // then dispatch to js_console_{log,warn,error}_spread. Without
            // this, the generic closure-spread path below treats `console.log`
            // as a closure value and js_closure_call_apply_with_spread fails
            // to dispatch (issue #407). Mirrors the multi-arg console.* path
            // in the Expr::Call codegen at lower_call.rs.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && matches!(
                        property.as_str(),
                        "log" | "info" | "warn" | "error" | "debug"
                    )
                {
                    let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                    for a in args {
                        match a {
                            CallArg::Expr(e) => {
                                let v = lower_expr(ctx, e)?;
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_push_f64",
                                    &[(I64, &acc_handle), (DOUBLE, &v)],
                                );
                            }
                            CallArg::Spread(e) => {
                                let part_box = lower_expr(ctx, e)?;
                                let blk = ctx.block();
                                let part_handle =
                                    blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &part_box)]);
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_concat",
                                    &[(I64, &acc_handle), (I64, &part_handle)],
                                );
                            }
                        }
                    }
                    let runtime_fn = match property.as_str() {
                        "info" => "js_console_info_spread",
                        "debug" => "js_console_debug_spread",
                        "warn" => "js_console_warn_spread",
                        "error" => "js_console_error_spread",
                        _ => "js_console_log_spread",
                    };
                    ctx.block().call_void(runtime_fn, &[(I64, &acc_handle)]);
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
            }

            if let Expr::FuncRef(fid) = callee.as_ref() {
                if spread_count == 1 && regular_count == 0 {
                    if let (Some(fname), Some(sig)) = (
                        ctx.func_names.get(fid).cloned(),
                        ctx.func_signatures.get(fid).copied(),
                    ) {
                        let (declared_count, has_rest, _, synthetic_is_rest) = sig;

                        // Find the spread source expression.
                        let spread_expr = args
                            .iter()
                            .find_map(|a| match a {
                                CallArg::Spread(e) => Some(e),
                                _ => None,
                            })
                            .expect("spread_count == 1 guarantees one Spread");

                        // Issue #653 followup: rest-bearing function. The
                        // declared "param count" includes the rest param,
                        // which from the callee's perspective IS the array.
                        // Spreading `...arr` into `f(...arr)` where `f`
                        // has shape `(...rest)` should pass `arr` directly
                        // as the single rest-array param — NOT extract
                        // element[0] and pass that as a primitive (which
                        // would set `rest` to a string and `rest.length`
                        // to that string's char count). The element-extract
                        // fast path stays correct for non-rest fixed-arity
                        // callees.
                        if ctx.func_synthetic_arguments.contains(fid) && synthetic_is_rest {
                            let arr_box = lower_expr(ctx, spread_expr)?;
                            let blk = ctx.block();
                            let arr_handle =
                                blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);
                            let arr_value = nanbox_pointer_inline(ctx.block(), &arr_handle);
                            let fixed_count = declared_count.saturating_sub(1);
                            let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
                            for i in 0..fixed_count {
                                let idx = format!("{}", i);
                                let blk = ctx.block();
                                let elem = blk.call(
                                    DOUBLE,
                                    "js_array_get_f64",
                                    &[(I64, &arr_handle), (I32, &idx)],
                                );
                                lowered.push(elem);
                            }
                            lowered.push(arr_value);
                            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                            return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
                        }

                        if has_rest && declared_count == 1 {
                            let arr_box = lower_expr(ctx, spread_expr)?;
                            let arr_handle = ctx.block().call(
                                I64,
                                "js_array_like_to_array",
                                &[(DOUBLE, &arr_box)],
                            );
                            let arr_value = nanbox_pointer_inline(ctx.block(), &arr_handle);
                            return Ok(ctx.block().call(DOUBLE, &fname, &[(DOUBLE, &arr_value)]));
                        }

                        // Lower the spread source as an array.
                        let arr_box = lower_expr(ctx, spread_expr)?;
                        let blk = ctx.block();
                        let arr_handle =
                            blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);

                        // Extract `declared_count` elements from the array.
                        let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
                        for i in 0..declared_count {
                            let idx = format!("{}", i);
                            let blk = ctx.block();
                            let elem = blk.call(
                                DOUBLE,
                                "js_array_get_f64",
                                &[(I64, &arr_handle), (I32, &idx)],
                            );
                            lowered.push(elem);
                        }

                        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
                    }
                }
            }

            // Method-call shape `recv.method(...args)` on an any-typed receiver
            // (refs #421, hono blocker): without this arm, the closure-callee
            // path below evaluates `recv.method` via `js_object_get_field_by_name`
            // which returns undefined for class-prototype methods on dynamically
            // typed receivers, and `js_closure_call_apply_with_spread` then
            // silently no-ops. SmartRouter.match's inner `router.add(...routes[i])`
            // hit exactly this — the inner router never received the route
            // entries, so match returned empty `[[],[]]` even though the outer
            // SmartRouter had the routes in #routes. Bundle every arg (regular
            // + spread) into a single JS array, then dispatch through the new
            // `js_native_call_method_apply` runtime helper which materialises
            // the array into a temp buffer and forwards to `js_native_call_method`.
            //
            // Skip the same callee shapes the regular-Call path skips: GlobalGet
            // (e.g. `console.log` — handled by the spread-bundling arm above),
            // NativeModuleRef (dedicated codegen elsewhere), and ExternFuncRef
            // (the previous `FuncRef` arm catches the FuncRef case; ExternFuncRef
            // here means a top-level imported function reference, not a method).
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                let mut skip = matches!(
                    object.as_ref(),
                    Expr::GlobalGet(_) | Expr::NativeModuleRef(_) | Expr::ExternFuncRef { .. }
                );
                // `recv.prop(...args)` where `prop` is an instance ACCESSOR
                // (`get prop()`) is NOT a method call: it must READ the accessor
                // (running the getter, which yields a function) and CALL that
                // function with the spread args. The method-apply path below
                // dispatches `prop` by name via `js_native_call_method`, which
                // looks up a same-named METHOD and throws "prop is not a
                // function" for an accessor. Skip it so the closure-callee path
                // lowers the callee `PropertyGet{recv, prop}` (invoking the
                // getter) and applies the spread to its result. Refs test262
                // language/arguments-object cls-*-spread-operator getter calls.
                if !skip {
                    if let Some(cls) = receiver_class_name(ctx, object) {
                        let mut cur = Some(cls);
                        while let Some(c) = cur {
                            let Some(ci) = ctx.classes.get(&c) else {
                                break;
                            };
                            if ci.getters.iter().any(|(n, _)| n == property) {
                                skip = true;
                                break;
                            }
                            cur = ci.extends_name.clone();
                        }
                    }
                }
                if !skip {
                    let recv_box = lower_expr(ctx, object)?;
                    // Build a single JS array containing every arg in order.
                    let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                    for a in args {
                        match a {
                            CallArg::Expr(e) => {
                                let v = lower_expr(ctx, e)?;
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_push_f64",
                                    &[(I64, &acc_handle), (DOUBLE, &v)],
                                );
                            }
                            CallArg::Spread(e) => {
                                let part_box = lower_expr(ctx, e)?;
                                let part_handle = ctx.block().call(
                                    I64,
                                    "js_array_like_to_array",
                                    &[(DOUBLE, &part_box)],
                                );
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_concat",
                                    &[(I64, &acc_handle), (I64, &part_handle)],
                                );
                            }
                        }
                    }
                    let key_idx = ctx.strings.intern(property);
                    let entry = ctx.strings.entry(key_idx);
                    let key_handle_global = format!("@{}", entry.handle_global);
                    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                    let method_id =
                        ctx.block()
                            .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_native_call_method_apply_by_id",
                        &[(DOUBLE, &recv_box), (I64, &method_id), (I64, &acc_handle)],
                    ));
                }
            }

            // Computed-member method call with spread: `recv[key](...args)`.
            // The literal `recv.method(...args)` shape is handled by the
            // PropertyGet arm above; the computed sibling lowers to a
            // `Call`/`CallSpread` with an `IndexGet` callee. Without this arm it
            // fell through to the closure-callee path below, which lowers
            // `recv[key]` to a bare method VALUE and calls it with no `this` —
            // so the method observed `this` = a field-less prototype stub
            // (missing instance data fields AND inherited methods). This is the
            // spread counterpart of the non-spread `js_native_call_method_{str_key,
            // value}` routing in `lower_call/early_branches.rs`. Bundle every
            // regular + spread arg into one array, then dispatch through
            // `js_native_call_method_value_apply`, which resolves the method by
            // the runtime key and binds `this = recv`. Skip a numeric index on a
            // non-class receiver (`arr[i](...)` array-element call), mirroring
            // the non-spread path, so element-call semantics are unchanged.
            if let Expr::IndexGet { object, index } = callee.as_ref() {
                let object_is_class_ref = matches!(object.as_ref(), Expr::ClassRef(_))
                    || matches!(object.as_ref(), Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name));
                if !(crate::type_analysis::is_numeric_expr(ctx, index) && !object_is_class_ref) {
                    let recv_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                    for a in args {
                        match a {
                            CallArg::Expr(e) => {
                                let v = lower_expr(ctx, e)?;
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_push_f64",
                                    &[(I64, &acc_handle), (DOUBLE, &v)],
                                );
                            }
                            CallArg::Spread(e) => {
                                let part_box = lower_expr(ctx, e)?;
                                let part_handle = ctx.block().call(
                                    I64,
                                    "js_array_like_to_array",
                                    &[(DOUBLE, &part_box)],
                                );
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_concat",
                                    &[(I64, &acc_handle), (I64, &part_handle)],
                                );
                            }
                        }
                    }
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_native_call_method_value_apply",
                        &[(DOUBLE, &recv_box), (DOUBLE, &key_box), (I64, &acc_handle)],
                    ));
                }
            }

            // #6475: `ns.fn(...spread)` where `ns` is a namespace import and
            // `fn` is a REST function export (effect's
            // `Schema.Union(...Array.from(...))`). The regular-call path routes
            // through `try_lower_namespace_member_call`, which resolves the
            // export's `perry_fn_<src>__fn` symbol directly. The spread path
            // instead fell to `lower_expr(callee)` below, which resolves
            // `ns.fn` as a VALUE — and `property_get`'s bare-name `class_ids`
            // lookup returns a same-named CLASS ref for the collision
            // `Schema.Union` (a Union CLASS exists in an unrelated module),
            // baking it as the spread callee → "value is not a function". A
            // rest function's `perry_fn` symbol takes its trailing args as a
            // single bundled array, so bundle the whole spread into that rest
            // slot and call the symbol directly, exactly as the regular path
            // does for `fixed_count == 0`. Gated on a known rest-function
            // export, so class members and non-rest shapes are untouched.
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if let Expr::ExternFuncRef { name: ns_name, .. } = object.as_ref() {
                    if ctx.namespace_imports.contains(ns_name)
                        && ctx.imported_func_has_rest.contains(property)
                        && ctx
                            .imported_func_param_counts
                            .get(property)
                            .copied()
                            .unwrap_or(1)
                            == 1
                    {
                        let source_prefix_opt = ctx
                            .namespace_member_prefixes
                            .get(&(ns_name.clone(), property.clone()))
                            .cloned()
                            .or_else(|| ctx.import_function_prefixes.get(property).cloned());
                        if let Some(source_prefix) = source_prefix_opt {
                            let origin_suffix = crate::expr::import_origin_suffix_ns(
                                ctx.import_function_origin_names,
                                ctx.namespace_member_origin_names,
                                ns_name,
                                property,
                            );
                            let symbol = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                            // Bundle every arg (regular + spread) into the single
                            // rest array, in source order.
                            let mut acc = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                            for a in args {
                                match a {
                                    CallArg::Expr(e) => {
                                        let v = lower_expr(ctx, e)?;
                                        acc = ctx.block().call(
                                            I64,
                                            "js_array_push_f64",
                                            &[(I64, &acc), (DOUBLE, &v)],
                                        );
                                    }
                                    CallArg::Spread(e) => {
                                        let part_box = lower_expr(ctx, e)?;
                                        let part = ctx.block().call(
                                            I64,
                                            "js_array_like_to_array",
                                            &[(DOUBLE, &part_box)],
                                        );
                                        acc = ctx.block().call(
                                            I64,
                                            "js_array_concat",
                                            &[(I64, &acc), (I64, &part)],
                                        );
                                    }
                                }
                            }
                            let rest_box = nanbox_pointer_inline(ctx.block(), &acc);
                            ctx.pending_declares
                                .push((symbol.clone(), DOUBLE, vec![DOUBLE]));
                            return Ok(ctx.block().call(DOUBLE, &symbol, &[(DOUBLE, &rest_box)]));
                        }
                    }
                }
            }

            // Closure callee path: `cb(reg0, reg1, ..., ...spread)` where
            // `cb` is a closure value (not a known FuncRef). We lower the
            // callee to its NaN-boxed value, marshal regular args into a
            // function-entry-allocated stack buffer, fold spread sources
            // into a single array (concat when multiple), then call
            // `js_closure_call_apply_with_spread`. This is what makes
            // patterns like `archetype.forEachWithComponents(types, cb)`
            // → `cb(entity, ...components)` actually invoke the user
            // callback (issue #412).
            //
            // The signature must match `runtime_decls.rs`:
            //   fn(closure_box: f64, regs_ptr: ptr, reg_count: i64,
            //      spread_arr_handle: i64) -> f64
            let cb_box = lower_expr(ctx, callee)?;

            // `js_closure_call_apply_with_spread` appends the spread array
            // AFTER the register args, which silently reorders interleaved
            // calls like `f(5, ...[6,7,8], 9)` → `[5, 9, 6, 7, 8]`. Detect a
            // regular arg appearing *after* a spread and, in that case, build a
            // single source-ordered array and pass everything through the
            // spread channel (regs = none) so positional order is preserved
            // (spec ArgumentListEvaluation). The split path stays for the
            // common `f(a, b, ...c)` shape (regs strictly before spreads).
            let first_spread = args.iter().position(|a| matches!(a, CallArg::Spread(_)));
            let interleaved = first_spread
                .map(|fs| args[fs + 1..].iter().any(|a| matches!(a, CallArg::Expr(_))))
                .unwrap_or(false);

            let (regs_ptr, regs_len, spread_handle) = if interleaved {
                // Build one array containing every arg in source order, then
                // apply it as the entire argument list.
                let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                for a in args {
                    match a {
                        CallArg::Expr(e) => {
                            let v = lower_expr(ctx, e)?;
                            acc_handle = ctx.block().call(
                                I64,
                                "js_array_push_f64",
                                &[(I64, &acc_handle), (DOUBLE, &v)],
                            );
                        }
                        CallArg::Spread(e) => {
                            let part_box = lower_expr(ctx, e)?;
                            let part_handle = ctx.block().call(
                                I64,
                                "js_array_like_to_array",
                                &[(DOUBLE, &part_box)],
                            );
                            acc_handle = ctx.block().call(
                                I64,
                                "js_array_concat",
                                &[(I64, &acc_handle), (I64, &part_handle)],
                            );
                        }
                    }
                }
                ("null".to_string(), "0".to_string(), acc_handle)
            } else {
                // Marshal regular args into a stack buffer (or null/0 if none).
                let (regs_ptr, regs_len) = if regular_count == 0 {
                    ("null".to_string(), "0".to_string())
                } else {
                    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, regular_count);
                    let mut idx = 0usize;
                    for a in args {
                        if let CallArg::Expr(e) = a {
                            let v = lower_expr(ctx, e)?;
                            let slot =
                                ctx.block()
                                    .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", idx))]);
                            ctx.block().store(DOUBLE, &v, &slot);
                            idx += 1;
                        }
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, regular_count, buf_reg
                    ));
                    (ptr_reg, regular_count.to_string())
                };

                // Marshal spread sources. 0 → "0" handle; 1 → unbox the one
                // array; multiple → concat onto a fresh array.
                let spread_handle = if spread_count == 0 {
                    "0".to_string()
                } else if spread_count == 1 {
                    let spread_expr = args
                        .iter()
                        .find_map(|a| match a {
                            CallArg::Spread(e) => Some(e),
                            _ => None,
                        })
                        .expect("spread_count == 1 guarantees one Spread");
                    let arr_box = lower_expr(ctx, spread_expr)?;
                    let blk = ctx.block();
                    blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)])
                } else {
                    // Concat all spread sources into a fresh array.
                    let acc = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                    let mut acc_handle = acc;
                    for a in args {
                        if let CallArg::Spread(e) = a {
                            let part_box = lower_expr(ctx, e)?;
                            let blk = ctx.block();
                            let part_handle =
                                blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &part_box)]);
                            acc_handle = ctx.block().call(
                                I64,
                                "js_array_concat",
                                &[(I64, &acc_handle), (I64, &part_handle)],
                            );
                        }
                    }
                    acc_handle
                };
                (regs_ptr, regs_len, spread_handle)
            };

            let result = ctx.block().call(
                DOUBLE,
                "js_closure_call_apply_with_spread",
                &[
                    (DOUBLE, &cb_box),
                    (crate::types::PTR, &regs_ptr),
                    (I64, &regs_len),
                    (I64, &spread_handle),
                ],
            );
            Ok(result)
        }

        // -------- Math.fround --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
