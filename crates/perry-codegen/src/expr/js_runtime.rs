//! perry-jsruntime / V8 interop (Js* variants).
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
        Expr::JsLoadModule { path } => {
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(path);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let blk = ctx.block();
            let len_str = byte_len.to_string();
            let handle_i64 = blk.call(
                I64,
                "js_load_module",
                &[(PTR, &bytes_global), (I64, &len_str)],
            );
            // Pass as f64 to fit the lower_expr return contract; consumers
            // (JsCallFunction/JsGetExport/JsNew) bitcast back to i64 before
            // passing to runtime FFIs that expect a u64 handle.
            Ok(blk.bitcast_i64_to_double(&handle_i64))
        }

        Expr::JsGetExport {
            module_handle,
            export_name,
        } => {
            let handle_dbl = lower_expr(ctx, module_handle)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(export_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let blk = ctx.block();
            let handle_i64 = blk.bitcast_double_to_i64(&handle_dbl);
            let len_str = byte_len.to_string();
            Ok(blk.call(
                DOUBLE,
                "js_get_export",
                &[(I64, &handle_i64), (PTR, &bytes_global), (I64, &len_str)],
            ))
        }

        Expr::JsCallFunction {
            module_handle,
            func_name,
            args,
        } => {
            let handle_dbl = lower_expr(ctx, module_handle)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(func_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let handle_i64 = ctx.block().bitcast_double_to_i64(&handle_dbl);
            let (args_ptr, args_len_str) = lower_js_args_array(ctx, &lowered_args);
            let len_str = byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_call_function",
                &[
                    (I64, &handle_i64),
                    (PTR, &bytes_global),
                    (I64, &len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            ))
        }

        Expr::JsCallMethod {
            object,
            method_name,
            args,
        } => {
            let obj_dbl = lower_expr(ctx, object)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(method_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let (args_ptr, args_len_str) = lower_js_args_array(ctx, &lowered_args);
            let len_str = byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_call_method",
                &[
                    (DOUBLE, &obj_dbl),
                    (PTR, &bytes_global),
                    (I64, &len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            ))
        }

        Expr::JsCallValue { callee, args } => {
            let func_dbl = lower_expr(ctx, callee)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let (args_ptr, args_len_str) = lower_js_args_array(ctx, &lowered_args);
            Ok(ctx.block().call(
                DOUBLE,
                "js_call_value",
                &[(DOUBLE, &func_dbl), (PTR, &args_ptr), (I64, &args_len_str)],
            ))
        }

        Expr::JsGetProperty {
            object,
            property_name,
        } => {
            let obj_dbl = lower_expr(ctx, object)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(property_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let len_str = byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_get_property",
                &[(DOUBLE, &obj_dbl), (PTR, &bytes_global), (I64, &len_str)],
            ))
        }

        Expr::JsSetProperty {
            object,
            property_name,
            value,
        } => {
            let obj_dbl = lower_expr(ctx, object)?;
            let val_dbl = lower_expr(ctx, value)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(property_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let len_str = byte_len.to_string();
            ctx.block().call_void(
                "js_set_property",
                &[
                    (DOUBLE, &obj_dbl),
                    (PTR, &bytes_global),
                    (I64, &len_str),
                    (DOUBLE, &val_dbl),
                ],
            );
            Ok(val_dbl)
        }

        Expr::JsNew {
            module_handle,
            class_name,
            args,
        } => {
            let handle_dbl = lower_expr(ctx, module_handle)?;
            let (bytes_global, byte_len) = {
                let idx = ctx.strings.intern(class_name);
                let entry = ctx.strings.entry(idx);
                (format!("@{}", entry.bytes_global), entry.byte_len)
            };
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let handle_i64 = ctx.block().bitcast_double_to_i64(&handle_dbl);
            let (args_ptr, args_len_str) = lower_js_args_array(ctx, &lowered_args);
            let len_str = byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_new_instance",
                &[
                    (I64, &handle_i64),
                    (PTR, &bytes_global),
                    (I64, &len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            ))
        }

        Expr::JsNewFromHandle { constructor, args } => {
            let ctor_dbl = lower_expr(ctx, constructor)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let (args_ptr, args_len_str) = lower_js_args_array(ctx, &lowered_args);
            Ok(ctx.block().call(
                DOUBLE,
                "js_new_from_handle",
                &[(DOUBLE, &ctor_dbl), (PTR, &args_ptr), (I64, &args_len_str)],
            ))
        }

        // `JsCreateCallback` (issue #248 Phase 2B): wrap a Perry closure
        // as a V8 callable. The runtime FFI
        // `js_create_callback(func_ptr, closure_env, param_count)` registers
        // a JS function whose trampoline (perry-jsruntime/src/interop.rs:993,
        // `native_callback_trampoline`) calls
        // `func_ptr(closure_env, args_ptr, args_len)` — but Perry closure
        // bodies expect `(closure_ptr, arg0, arg1, ...)` per arity. Bridge
        // is the `js_closure_call_array` runtime helper added alongside
        // (`crates/perry-runtime/src/closure.rs`) which takes the i64
        // closure pointer and dispatches to the right `js_closure_callN`
        // based on `args_len`. Codegen passes:
        //   func_ptr     = ptrtoint @js_closure_call_array to i64
        //   closure_env  = unbox(closure)  — raw *ClosureHeader as i64
        //   param_count  = static usize from HIR
        // Result is a NaN-boxed JS handle (V8-handle tag 0x7FFB) that JS
        // code can call like any other JS function.
        Expr::JsCreateCallback {
            closure,
            param_count,
        } => {
            let closure_dbl = lower_expr(ctx, closure)?;
            let blk = ctx.block();
            let closure_i64 = unbox_to_i64(blk, &closure_dbl);
            // ptrtoint of a function symbol: assigns a fresh SSA register
            // and emits the conversion. The resulting i64 is the address
            // of `js_closure_call_array`, which we hand to js_create_callback
            // as its trampoline target.
            let func_addr = blk.next_reg();
            blk.emit_raw(format!(
                "{} = ptrtoint ptr @js_closure_call_array to i64",
                func_addr
            ));
            let pcount = (*param_count as i64).to_string();
            Ok(blk.call(
                DOUBLE,
                "js_create_callback",
                &[(I64, &func_addr), (I64, &closure_i64), (I64, &pcount)],
            ))
        }
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
