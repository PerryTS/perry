//! SuperMethodCall / SuperPropertyGet / FsReadFileBinary.
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
        Expr::SuperMethodCall { method, args } => {
            // Find the current class from the class_stack.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                // No enclosing class — fall back to stub.
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Walk parent chain starting from extends_name.
            let mut parent = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.clone());
            let mut resolved_fn: Option<String> = None;
            while let Some(p) = parent {
                let key = (p.clone(), method.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    resolved_fn = Some(fname);
                    break;
                }
                parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
            }
            let Some(fn_name) = resolved_fn else {
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Lower `this` (from this_stack) + args.
            let this_slot = ctx
                .this_stack
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("super.{}() outside any method body", method))?;
            let this_box = ctx.block().load(DOUBLE, &this_slot);
            let mut lowered: Vec<String> = Vec::with_capacity(args.len() + 1);
            lowered.push(this_box);
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices))
        }

        // -------- super.<prop> as a value (issue #774) --------
        // Walk the parent-class chain. If a parent declares a method
        // with the requested name, materialize it as a closure value
        // via the singleton wrapper (mirroring `Expr::FuncRef`).
        // Otherwise return `undefined` — which is the strict-JS
        // result for instance-field shadows like:
        //
        //     class A { foo = "A"; }
        //     class B extends A { foo = "B"; m() { return super.foo; } }
        //
        // The previous lowering rewrote `super.foo` to `this.foo`, so
        // it silently returned the child override ("B") instead of
        // `undefined`. See issue #774 / PR #774 follow-up.
        //
        // Call-form `super.method(...)` never reaches this arm — it
        // is lowered to `Expr::SuperMethodCall` in lower_call.rs.
        Expr::SuperPropertyGet { property } => {
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                return Ok(undef);
            };
            let mut parent = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.clone());
            let mut resolved_fn: Option<String> = None;
            while let Some(p) = parent {
                let key = (p.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    resolved_fn = Some(fname);
                    break;
                }
                parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
            }
            let Some(fn_name) = resolved_fn else {
                return Ok(undef);
            };
            // Mirror Expr::FuncRef: route through the singleton wrapper
            // so callers can invoke via the closure-call ABI. The
            // `__perry_wrap_<fn>` symbol is emitted by compile_module
            // for every user function.
            //
            // #1126: when the resolved parent method lives in a DIFFERENT
            // module (typical with cross-module class inheritance —
            // rxjs's `OperatorSubscriber extends Subscriber` where
            // `super.complete` / `super._complete` refer to Subscriber.ts's
            // methods), the wrapper's `define` lives in the source TU but
            // is referenced from this consumer TU. LLVM per-TU IR
            // validation rejects the reference unless we forward-declare
            // the symbol. Push into `pending_declares` — `declare_function`
            // dedupes against any later same-TU `define` (`module.rs:67-69`
            // comment) so this is safe for the same-module case too. The
            // signature is informational only (runtime dispatches via
            // ClosureHeader's func_ptr); use the same `(i64)` + 0 doubles
            // shape the imported-function-ref site at `expr/mod.rs:12331`
            // uses for unknown-arity imports.
            let wrap_name = format!("__perry_wrap_{}", fn_name);
            ctx.pending_declares
                .push((wrap_name.clone(), DOUBLE, vec![I64]));
            let blk = ctx.block();
            let wrap_ptr = format!("@{}", wrap_name);
            let closure_handle = blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
            Ok(nanbox_pointer_inline(blk, &closure_handle))
        }

        // -------- fs.readFileSync(path) -> Buffer (no encoding) --------
        // Node returns a Buffer when no encoding is supplied; mirror that.
        // js_fs_read_file_binary returns a raw *mut BufferHeader registered
        // in BUFFER_REGISTRY; NaN-box with POINTER_TAG so downstream
        // console.log / .toString / .length / .[i] dispatch consult the
        // registry and format the value as `<Buffer xx xx ...>` (or the
        // appropriate Buffer behaviour for each method).
        Expr::FsReadFileBinary(path) => {
            let path_box = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let buf_handle = blk.call(I64, "js_fs_read_file_binary", &[(DOUBLE, &path_box)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // -------- instanceof --------
        // Look up the target class's id and call js_instanceof. The
        // runtime walks the object's class chain and returns a
        // NaN-tagged TAG_TRUE/TAG_FALSE double directly — no
        // conversion needed.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
