//! ObjectRest..FsAppendFileSync (BigInt + Set methods + filesystem writers).
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
    emit_root_nanbox_store_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
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
        Expr::ObjectRest {
            object,
            exclude_keys,
        } => {
            let obj_box = lower_expr(ctx, object)?;
            let key_handle_globals: Vec<String> = exclude_keys
                .iter()
                .map(|k| {
                    let idx = ctx.strings.intern(k);
                    format!("@{}", ctx.strings.entry(idx).handle_global)
                })
                .collect();
            let blk = ctx.block();
            let obj_handle = {
                let bits = blk.bitcast_double_to_i64(&obj_box);
                blk.and(I64, &bits, POINTER_MASK_I64)
            };
            let n_str = (exclude_keys.len() as u32).to_string();
            let keys_arr = blk.call(I64, "js_array_alloc_with_length", &[(I32, &n_str)]);
            for (i, handle_global) in key_handle_globals.iter().enumerate() {
                let idx_str = i.to_string();
                let key_box = blk.load(DOUBLE, handle_global);
                blk.call_void(
                    "js_array_set_f64_unchecked",
                    &[(I64, &keys_arr), (I32, &idx_str), (DOUBLE, &key_box)],
                );
            }
            let rest_ptr = blk.call(
                I64,
                "js_object_rest",
                &[(I64, &obj_handle), (I64, &keys_arr)],
            );
            Ok(nanbox_pointer_inline(blk, &rest_ptr))
        }

        // -------- BigInt(literal) --------
        // The HIR carries the literal as a string for arbitrary
        // precision. We hand it to the runtime as a UTF-8 byte
        // pointer + length.
        //
        // Tagged with BIGINT_TAG (not POINTER_TAG): `typeof 5n`
        // reads the top 16 bits to distinguish `"bigint"` from
        // `"object"`, and `js_dynamic_add`/`_sub`/`_mul`/`_div`/`_mod`
        // use `JSValue::is_bigint()` which also checks that tag —
        // literals tagged as POINTER_TAG fooled both sites, which is
        // why arithmetic used to collapse to `NaN`. Closes GH #33.
        Expr::BigInt(s) => {
            let bytes_idx = ctx.strings.intern(s);
            let bytes_global = format!("@{}", ctx.strings.entry(bytes_idx).bytes_global);
            let len_str = (s.len() as u32).to_string();
            let blk = ctx.block();
            let result = blk.call(
                I64,
                "js_bigint_from_string",
                &[(PTR, &bytes_global), (I32, &len_str)],
            );
            Ok(nanbox_bigint_inline(blk, &result))
        }

        // -------- BigInt(value) coercion --------
        // `BigInt(42)`, `BigInt("9223372036854775807")`, `BigInt(someBigInt)`.
        // The runtime helper inspects the NaN-box tag and dispatches:
        // bigint → pass-through, int32 → i64 conversion, string →
        // parse, undefined/null → 0n, f64 → truncate-to-i64. Result
        // is a raw `BigIntHeader*`; we NaN-box with BIGINT_TAG so
        // later sites see `typeof === "bigint"` and the dynamic-
        // arithmetic check `is_bigint()` both succeed.
        Expr::BigIntCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let ptr = blk.call(I64, "js_bigint_from_f64", &[(DOUBLE, &v)]);
            Ok(nanbox_bigint_inline(blk, &ptr))
        }

        // -------- arr.sort(comparator) -> same array (in place) --------
        // The HIR variant always carries a comparator. If the comparator
        // is a synthetic "default" marker we'd want js_array_sort_default;
        // for now we always use the user-comparator path.
        Expr::ArraySort { array, comparator } => {
            let arr_box = lower_expr(ctx, array)?;
            let cmp_box = lower_expr(ctx, comparator)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #2796: validate the comparator (function | undefined) before
            // sorting — throws TypeError for any other value, returns 0
            // (default sort) for undefined.
            let cmp_handle = blk.call(I64, "js_validate_array_comparator", &[(DOUBLE, &cmp_box)]);
            let result = blk.call(
                I64,
                "js_array_sort_with_comparator",
                &[(I64, &arr_handle), (I64, &cmp_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.reduce(callback, initial?) -> value --------
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let (has_init, init_d) = if let Some(init_expr) = initial {
                let v = lower_expr(ctx, init_expr)?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0x7FF8000000000000".to_string()) // NaN bits won't actually be used
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            // Convert literal NaN bits to a double via bitcast — but the
            // string above isn't valid LLVM. Use a real NaN literal instead.
            let init_use = if has_init == "1" {
                init_d
            } else {
                // LLVM treats `0x7FF8000000000000` as a hex double literal
                // when written as `0x7FF8000000000000` — but the safe way
                // is to just use `0x7FF8000000000000` via the IR's hex
                // form for doubles. Use plain `0.0` since it's unused.
                "0.0".to_string()
            };
            let runtime_fn = if matches!(expr, Expr::ArrayReduceRight { .. }) {
                "js_array_reduce_right"
            } else {
                "js_array_reduce"
            };
            Ok(blk.call(
                DOUBLE,
                runtime_fn,
                &[
                    (I64, &arr_handle),
                    (I64, &cb_handle),
                    (I32, &has_init),
                    (DOUBLE, &init_use),
                ],
            ))
        }

        // -------- enum members lower to constants --------
        Expr::EnumMember {
            enum_name,
            member_name,
        } => {
            let key = (enum_name.clone(), member_name.clone());
            let val = ctx.enums.get(&key).ok_or_else(|| {
                anyhow!(
                    "perry-codegen: enum member {}.{} not found in enums table",
                    enum_name,
                    member_name
                )
            })?;
            match val {
                perry_hir::EnumValue::Number(n) => Ok(double_literal(*n as f64)),
                perry_hir::EnumValue::String(s) => {
                    // Intern the string and load the handle global at the
                    // use site, just like a regular string literal.
                    let key_idx = ctx.strings.intern(s);
                    let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    Ok(ctx.block().load(DOUBLE, &handle_global))
                }
            }
        }

        // -------- fs.existsSync(path) -> boolean --------
        Expr::FsExistsSync(path) => {
            let p = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_fs_exists_sync", &[(DOUBLE, &p)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Number(value) coercion --------
        Expr::NumberCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)]))
        }

        // -------- set.add(value) — updates the local in place --------
        Expr::SetAdd { set_id, value } => {
            let v = lower_expr(ctx, value)?;
            let set_box = lower_expr(ctx, &Expr::LocalGet(*set_id))?;
            let blk = ctx.block();
            let set_handle = unbox_to_i64(blk, &set_box);
            let new_handle = blk.call(I64, "js_set_add", &[(I64, &set_handle), (DOUBLE, &v)]);
            let new_box = nanbox_pointer_inline(blk, &new_handle);
            // Write back to the storage so subsequent reads see the
            // possibly-realloc'd pointer.
            if let Some(&capture_idx) = ctx.closure_captures.get(set_id) {
                let closure_ptr = ctx
                    .current_closure_ptr
                    .clone()
                    .ok_or_else(|| anyhow!("SetAdd captured but no current_closure_ptr"))?;
                let idx_str = capture_idx.to_string();
                ctx.block().call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_ptr), (I32, &idx_str), (DOUBLE, &new_box)],
                );
            } else if let Some(slot) = ctx.locals.get(set_id).cloned() {
                ctx.block().store(DOUBLE, &new_box, &slot);
            } else if let Some(global_name) = ctx.module_globals.get(set_id).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): module global Set slot is a registered mutable GC root.
                emit_root_nanbox_store_on_block(ctx.block(), &new_box, &g_ref);
            }
            Ok(new_box)
        }

        // -------- set.has(value) -> boolean --------
        Expr::SetHas { set, value } => {
            let s_box = lower_expr(ctx, set)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_has", &[(I64, &s_handle), (DOUBLE, &v_box)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- set.delete(value) -> boolean --------
        Expr::SetDelete { set, value } => {
            let s_box = lower_expr(ctx, set)?;
            let v_box = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_delete", &[(I64, &s_handle), (DOUBLE, &v_box)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- set.size -> number --------
        Expr::SetSize(set) => {
            let s_box = lower_expr(ctx, set)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let i32_v = blk.call(I32, "js_set_size", &[(I64, &s_handle)]);
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }

        Expr::FsWriteFileSync(path, content) => {
            let p = lower_expr(ctx, path)?;
            let c = lower_expr(ctx, content)?;
            // js_fs_write_file_sync returns i32 (1=success). Discard the
            // result; fs.writeFileSync is void in JS.
            let _ = ctx
                .block()
                .call(I32, "js_fs_write_file_sync", &[(DOUBLE, &p), (DOUBLE, &c)]);
            Ok(double_literal(0.0))
        }

        Expr::FsAppendFileSync(path, content) => {
            // Issue #226. JS and WASM backends already had arms for this
            // HIR variant; the LLVM backend was missing the case here next
            // to `FsWriteFileSync`. The runtime fn
            // (`crates/perry-runtime/src/fs.rs::js_fs_append_file_sync`)
            // is correct; the codegen + lowerer plumbing was the gap.
            // The companion fix in `crates/perry-hir/src/lower/expr_call.rs`
            // extends the namespace-import path (`fs.appendFileSync` via
            // `import * as fs from "fs"`) — without that the variant
            // never gets emitted for the common usage shape. Returns i32
            // (1=success) which we discard; appendFileSync is void in JS.
            let p = lower_expr(ctx, path)?;
            let c = lower_expr(ctx, content)?;
            let _ = ctx
                .block()
                .call(I32, "js_fs_append_file_sync", &[(DOUBLE, &p), (DOUBLE, &c)]);
            Ok(double_literal(0.0))
        }

        // -------- NativeMethodCall (Phase H.1) --------
        // Perry's HIR uses NativeMethodCall { module, method, object, args }
        // for method calls on natively-typed receivers — specifically for
        // typed arrays (where `push`/`pop`/etc. on `T[]` get this shape
        // instead of the generic ArrayPush/Pop variants), and for
        // module-level calls (mysql.createConnection, redis.set, etc.).
        //
        // Phase H.1 handles the most common shape: `array.push_single`,
        // `array.push`, `array.pop_back` on typed arrays. The object is
        // a PropertyGet on a class instance (`this.items`) or a LocalGet.
        // We chain a get + push + set so reallocations are reflected
        // back in the source.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
