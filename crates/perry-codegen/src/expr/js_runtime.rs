//! perry-jsruntime / V8 interop (Js* variants).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::native_value::MaterializationReason;
use crate::types::{DOUBLE, I64, PTR};

use super::{
    downgrade_buffer_aliases_in_expr, lower_expr, lower_js_args_array, unbox_to_i64, FnCtx,
};

fn downgrade_unknown_call_expr(ctx: &mut FnCtx<'_>, expr: &Expr) {
    downgrade_buffer_aliases_in_expr(ctx, expr, MaterializationReason::UnknownCallEscape);
}

fn downgrade_unknown_call_args(ctx: &mut FnCtx<'_>, args: &[Expr]) {
    for arg in args {
        downgrade_unknown_call_expr(ctx, arg);
    }
}

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
            downgrade_unknown_call_expr(ctx, module_handle);
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
            downgrade_unknown_call_expr(ctx, module_handle);
            downgrade_unknown_call_args(ctx, args);
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
            downgrade_unknown_call_expr(ctx, object);
            downgrade_unknown_call_args(ctx, args);
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
            downgrade_unknown_call_expr(ctx, callee);
            downgrade_unknown_call_args(ctx, args);
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
            downgrade_unknown_call_expr(ctx, object);
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
            downgrade_unknown_call_expr(ctx, object);
            downgrade_unknown_call_expr(ctx, value);
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
            downgrade_unknown_call_expr(ctx, module_handle);
            downgrade_unknown_call_args(ctx, args);
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
            downgrade_unknown_call_expr(ctx, constructor);
            downgrade_unknown_call_args(ctx, args);
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
            downgrade_unknown_call_expr(ctx, closure);
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
