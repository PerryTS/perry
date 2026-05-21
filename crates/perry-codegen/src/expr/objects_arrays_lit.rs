//! Object/Array literals + spread.
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
        Expr::Object(props) => lower_object_literal(ctx, props, None),

        // -------- Arrays (Phase B.3) --------
        // `[a, b, c]` literal: allocate via js_array_alloc(N), then
        // sequentially push each element. js_array_push_f64 may return a
        // new pointer if it had to realloc, so we thread the pointer
        // through each push. Final pointer is NaN-boxed via js_nanbox_pointer
        // (POINTER_TAG, not STRING_TAG).
        Expr::Array(elements) => lower_array_literal(ctx, elements),

        // `[a, ...b, c]` literal with spread elements. Each Spread
        // element calls `js_array_concat(dest, src)` to copy from
        // source; each Expr element calls `js_array_push_f64`. Both
        // may realloc, so we thread the pointer through.
        Expr::ArraySpread(elements) => {
            use perry_hir::ArrayElement;
            if let [ArrayElement::Spread(e)] = elements.as_slice() {
                // Hot path for `[...arr]` / `[...set]` / `[...string]`:
                // avoid allocating an empty destination and then routing
                // through `js_array_concat`'s append machinery. The clone
                // helper already contains the same array/string/set/map/
                // typed-array materialization arms used by Array.from.
                //
                // Spread of `null` / `undefined` must throw `TypeError`
                // per ES2015 §12.2.5 (GetIterator). Route through
                // `js_array_clone_for_spread`, which checks the NaN-box
                // tag bits on the raw boxed value before stripping, so
                // null/undefined throws while every other input is
                // forwarded to the regular clone path.
                let src_box = lower_expr(ctx, e)?;
                let cloned =
                    ctx.block()
                        .call(I64, "js_array_clone_for_spread", &[(DOUBLE, &src_box)]);
                return Ok(nanbox_pointer_inline(ctx.block(), &cloned));
            }
            let cap_str = (elements.len() as u32).to_string();
            let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_str)]);
            for elem in elements {
                match elem {
                    ArrayElement::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        current_arr = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ArrayElement::Spread(e) => {
                        if is_string_expr(ctx, e) {
                            // String spread: `[..."hello"]` → split into
                            // individual character strings.
                            let src_box = lower_expr(ctx, e)?;
                            let blk = ctx.block();
                            let src_handle = unbox_to_i64(blk, &src_box);
                            let char_arr =
                                blk.call(I64, "js_string_to_char_array", &[(I64, &src_handle)]);
                            current_arr = blk.call(
                                I64,
                                "js_array_concat",
                                &[(I64, &current_arr), (I64, &char_arr)],
                            );
                        } else {
                            let src_box = lower_expr(ctx, e)?;
                            let blk = ctx.block();
                            let src_handle = unbox_to_i64(blk, &src_box);
                            current_arr = blk.call(
                                I64,
                                "js_array_concat",
                                &[(I64, &current_arr), (I64, &src_handle)],
                            );
                        }
                    }
                }
            }
            Ok(nanbox_pointer_inline(ctx.block(), &current_arr))
        }

        // `arr[i]` index access. INLINE FAST PATH for typed-Number arrays:
        // skip the runtime function call, do the address arithmetic
        // directly. The ArrayHeader layout is `{ length: u32, capacity:
        // u32, elements: [f64; N] }` — elements start at offset 8.
        //
        // Equivalent to:
        //   element_ptr = arr_ptr + 8 + idx*8
        //   load double, ptr element_ptr
        //
        // Saves a function call (~5-10 ns) per access. For
        // bench_array_ops with ~400K reads per iteration this is a
        // major performance win.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
