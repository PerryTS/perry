//! CallSpread (function call with spread args).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

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

            // console.log(...arr) / .info / .warn / .error / .debug — bundle
            // every regular arg + every spread source into a single array,
            // then dispatch to js_console_{log,warn,error}_spread. Without
            // this, the generic closure-spread path below treats `console.log`
            // as a closure value and js_closure_call_apply_with_spread fails
            // to dispatch (issue #407). Mirrors the multi-arg console.* path
            // in the Expr::Call codegen at lower_call.rs.
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
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
                                let part_handle = unbox_to_i64(blk, &part_box);
                                acc_handle = ctx.block().call(
                                    I64,
                                    "js_array_concat",
                                    &[(I64, &acc_handle), (I64, &part_handle)],
                                );
                            }
                        }
                    }
                    let runtime_fn = match property.as_str() {
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
                        let (declared_count, has_rest, _) = sig;

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
                        if has_rest && declared_count == 1 {
                            let arr_box = lower_expr(ctx, spread_expr)?;
                            return Ok(ctx.block().call(DOUBLE, &fname, &[(DOUBLE, &arr_box)]));
                        }

                        // Lower the spread source as an array.
                        let arr_box = lower_expr(ctx, spread_expr)?;
                        let blk = ctx.block();
                        let arr_handle = unbox_to_i64(blk, &arr_box);

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
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                let skip = matches!(
                    object.as_ref(),
                    Expr::GlobalGet(_) | Expr::NativeModuleRef(_) | Expr::ExternFuncRef { .. }
                );
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
                                let part_handle = unbox_to_i64(ctx.block(), &part_box);
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
                    let bytes_global = format!("@{}", entry.bytes_global);
                    let name_len_str = entry.byte_len.to_string();
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_native_call_method_apply",
                        &[
                            (DOUBLE, &recv_box),
                            (PTR, &bytes_global),
                            (I64, &name_len_str),
                            (I64, &acc_handle),
                        ],
                    ));
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

            // Marshal regular args into a stack buffer (or null/0 if none).
            let (regs_ptr, regs_len) = if regular_count == 0 {
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, regular_count);
                let mut idx = 0usize;
                for a in args {
                    if let CallArg::Expr(e) = a {
                        let v = lower_expr(ctx, e)?;
                        let slot = ctx
                            .block()
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
                unbox_to_i64(blk, &arr_box)
            } else {
                // Concat all spread sources into a fresh array.
                let acc = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                let mut acc_handle = acc;
                for a in args {
                    if let CallArg::Spread(e) = a {
                        let part_box = lower_expr(ctx, e)?;
                        let blk = ctx.block();
                        let part_handle = unbox_to_i64(blk, &part_box);
                        acc_handle = ctx.block().call(
                            I64,
                            "js_array_concat",
                            &[(I64, &acc_handle), (I64, &part_handle)],
                        );
                    }
                }
                acc_handle
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
