//! IndexGet (arr[i] / obj[k]).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
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
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};

use super::arrays_finds::lower_buffer_index_get_i32;
#[allow(unused_imports)]
use super::{
    buffer_access_materialization_reason, buffer_alias_metadata_suffix,
    emit_layout_note_slot_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_typed_feedback_register_site, emit_v8_export_call,
    emit_v8_member_method_call, emit_write_barrier, emit_write_barrier_slot_on_block,
    expr_has_numeric_pointer_free_array_layout, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_typed_array_load, lower_url_string_getter, materialize_js_value, nanbox_bigint_inline,
    nanbox_pointer_inline, nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array,
    raw_f64_layout_fact, try_flat_const_2d_int, try_lower_flat_const_index_get,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};

fn is_width_tracked_typed_array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some(
            "Int8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
        )
    )
}

fn is_uint8array_receiver(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    matches!(
        receiver_class_name(ctx, object).as_deref(),
        Some("Buffer" | "Uint8Array")
    )
}

fn numeric_index_needs_runtime_key(index: &Expr) -> bool {
    // Only a LITERAL numeric key that is not a clean array index in
    // `0..=i32::MAX` needs the runtime key helper: out-of-range/negative
    // integers (`a[2**32-1]`, `a[-1]`), non-integer floats (`a[1.5]`), and
    // non-finite values (`a[NaN]`/`a[Infinity]`). These become string-keyed
    // properties and must reach `js_array_*_index_or_string`.
    //
    // Computed/dynamic numeric indices are deliberately NOT rerouted here:
    // they keep flowing through the typed-feedback numeric-array guard path,
    // which already carries its own out-of-range/non-integer fallback. Sending
    // them to the runtime key helper would defeat the native numeric-array hot
    // path and drop the index guard (regressing the native-region proof and
    // the typed-feedback hot-path tests). (#4557/#4543)
    match index {
        Expr::Integer(i) => *i < 0 || *i > i32::MAX as i64,
        Expr::Number(n) => {
            !(n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n <= i32::MAX as f64)
        }
        _ => false,
    }
}

fn is_async_dispose_symbol_index(index: &Expr) -> bool {
    let Expr::SymbolFor(symbol_name) = index else {
        return false;
    };
    match symbol_name.as_ref() {
        Expr::String(name) => name == "@@__perry_wk_asyncDispose",
        Expr::WtfString(name) => name.as_slice() == b"@@__perry_wk_asyncDispose",
        _ => false,
    }
}

/// True when `object` evaluates to an INT32-tagged class ref or class
/// prototype ref (NaN-box tag `0x7FFE`) rather than a heap-pointer object.
/// Such values must reach `js_object_get_field_by_name` with their tag bits
/// intact — masking with `POINTER_MASK_I64` strips the `0x7FFE` tag and the
/// runtime never routes to the class-static-accessor / prototype-vtable
/// lookup, so `(class { static get 0(){} })['0']` reads `undefined`.
///
/// Covers three shapes:
///   * `Expr::ClassRef` / imported-class `ExternFuncRef` — a class ref value.
///   * `LocalGet` of a local aliased to a class (`var C = class {…}` lowers to
///     `Let { init: ClassRef }`, recording `local_class_aliases["C"] = "C"`).
///     A literal member name (`C.x`) folds to `PropertyGet` and resolves on its
///     own, but an integer-like / empty key stays an `IndexGet` and lands here.
///   * `C.prototype` of either of the above — a prototype ref value, so
///     `C.prototype['']` reaches the instance-vtable getter.
pub(crate) fn index_object_is_class_or_proto_ref(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match object {
        Expr::ClassRef(_) => true,
        Expr::ExternFuncRef { name, .. } => ctx.class_ids.contains_key(name),
        Expr::LocalGet(id) => ctx
            .local_id_to_name
            .get(id)
            .and_then(|name| ctx.local_class_aliases.get(name))
            .map(|cls| ctx.class_ids.contains_key(cls))
            .unwrap_or(false),
        Expr::PropertyGet {
            object: inner,
            property,
        } if property.as_str() == "prototype" => {
            index_object_is_class_or_proto_ref(ctx, inner.as_ref())
        }
        _ => false,
    }
}

/// Compute the receiver handle to pass to `js_object_get_field_by_name`-family
/// helpers from a NaN-boxed receiver value (`obj_bits`). Only a *genuine heap
/// pointer* — POINTER_TAG (`0x7FFD`, plain objects/arrays) or STRING_TAG
/// (`0x7FFF`, heap strings) — may be masked down to a raw pointer for the runtime
/// to dereference. Every other receiver shape keeps its full NaN-boxed bits:
///
/// * an INT32-tagged class ref (`0x7FFE`) keeps its tag so the runtime routes to
///   the static field / method / accessor tables (test262 class/elements
///   propertyHelper `isWritable(C, name)` does `C[name]`);
/// * a plain number, SSO string, bigint, or bool/null/undefined keeps its bits
///   so the by-name runtime helper recognizes the tag and returns `undefined`
///   instead of masking the value's low 48 bits into a bogus heap address and
///   dereferencing it.
///
/// The masking-everything-but-classref predecessor crashed on `(<number>)[k]`:
/// the timestamp float `dayjs(1749820051142)` (`0x4279_7696_70ec_6000`) had its
/// low 48 bits (`0x7696_70ec_6000`) masked into a plausible-looking heap pointer,
/// and `js_typed_feedback_object_get_field_by_name_f64` then deref'd `ptr - 8`
/// for the GcHeader → SIGSEGV (#5429). Keeping full bits routes the number
/// through `normalize_raw_object_addr`, which rejects it (top16 `0x4279` masks to
/// `0`), matching the dotted `n.format` path's receiver-tag triage.
///
/// When the receiver's heap-pointer-ness is known at compile time
/// (`static_known` — a class or `.prototype` ref), pass full bits unconditionally;
/// otherwise branch at runtime on the tag so a runtime class-ref value (e.g. a
/// function parameter bound to a class — `function f(C, k){ return C[k]; }`) is
/// handled too.
pub(crate) fn classref_preserving_handle(
    blk: &mut crate::block::LlBlock,
    obj_bits: &str,
    static_known: bool,
) -> String {
    if static_known {
        return obj_bits.to_string();
    }
    let top16 = blk.lshr(I64, obj_bits, "48");
    // (top16 & 0xFFFD) == 0x7FFD is true for exactly POINTER_TAG (0x7FFD) and
    // STRING_TAG (0x7FFF) — the two heap-pointer-carrying tags.
    let masked_tag = blk.and(I64, &top16, "65533"); // 0xFFFD
    let is_heap_ptr = blk.icmp_eq(I64, &masked_tag, "32765"); // 0x7FFD
    let masked = blk.and(I64, obj_bits, POINTER_MASK_I64);
    blk.select(crate::types::I1, &is_heap_ptr, I64, &masked, obj_bits)
}

fn lower_class_method_bind(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method_name: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(method_name);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let len_str = entry.byte_len.to_string();
    let blk = ctx.block();
    let bytes_i64 = blk.ptrtoint(&bytes_global, I64);
    Ok(blk.call(
        DOUBLE,
        "js_class_method_bind",
        &[(DOUBLE, &recv_box), (I64, &bytes_i64), (I64, &len_str)],
    ))
}

fn lower_guarded_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_box: &str,
    idx_i32: &str,
    block_prefix: &str,
    require_numeric_layout: bool,
    coerce_numeric_fallback: bool,
) -> Result<String> {
    let contract = if require_numeric_layout {
        TypedFeedbackContract::numeric_array_get_index()
    } else {
        TypedFeedbackContract::array_get_index()
    };
    let feedback_site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]",
        contract,
    );
    let fast_idx = ctx.new_block(&format!("{}.fast", block_prefix));
    let fallback_idx = ctx.new_block(&format!("{}.fallback", block_prefix));
    let merge_idx = ctx.new_block(&format!("{}.merge", block_prefix));
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    let guard_ok = {
        let blk = ctx.block();
        let guard_fn = if require_numeric_layout {
            "js_typed_feedback_numeric_array_index_get_guard"
        } else {
            "js_typed_feedback_plain_array_index_get_guard"
        };
        let guard_i32 = blk.call(
            I32,
            guard_fn,
            &[
                (I64, &feedback_site_id),
                (DOUBLE, arr_box),
                (DOUBLE, idx_box),
                (I32, idx_i32),
                (I32, "1"),
            ],
        );
        blk.icmp_ne(I32, &guard_i32, "0")
    };
    ctx.block().cond_br(&guard_ok, &fast_label, &fallback_label);

    ctx.current_block = fallback_idx;
    let fallback_boxed = ctx.block().call(
        DOUBLE,
        "js_typed_feedback_array_index_get_fallback_boxed",
        &[
            (I64, &feedback_site_id),
            (DOUBLE, arr_box),
            (DOUBLE, idx_box),
        ],
    );
    let fallback_val = if require_numeric_layout && coerce_numeric_fallback {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &fallback_boxed)])
    } else {
        fallback_boxed.clone()
    };
    let fallback_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    if require_numeric_layout {
        let fallback = LoweredValue::js_value(fallback_boxed.clone());
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexGet",
            None,
            "js_typed_feedback_array_index_get_fallback_boxed",
            &fallback,
            Some(BoundsState::Unknown),
            None,
            Some(BufferAccessMode::DynamicFallback),
            Some(MaterializationReason::RuntimeApi),
            None,
            None,
            Vec::new(),
            vec![
                raw_f64_layout_fact(
                    None,
                    "rejected",
                    "numeric_array_index_get_guard",
                    Some(MaterializationReason::RuntimeApi),
                ),
                raw_f64_layout_fact(
                    None,
                    "invalidated",
                    "runtime_api",
                    Some(MaterializationReason::RuntimeApi),
                ),
            ],
            false,
            false,
            Vec::new(),
        );
    }

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let arr_bits = fast_blk.bitcast_double_to_i64(arr_box);
    let arr_handle = fast_blk.and(I64, &arr_bits, POINTER_MASK_I64);
    let fast_val = if require_numeric_layout {
        // The `numeric_array_index_get_guard` on the way into this block already
        // proved: a plain, non-forwarded `Array`, in raw-f64 numeric layout,
        // with `index` in bounds (`plain_array_index_guard(.., in_bounds=true)`
        // && `js_array_is_numeric_f64_layout`). So load the slot inline instead
        // of calling `js_array_numeric_get_f64_unboxed`, whose hot path
        // re-validates exactly those same conditions and then does this load.
        // Raw-f64 arrays are dense (no HOLE slots) and the slot holds a raw f64,
        // matching the runtime helper's `return *elements_ptr.add(index)`.
        let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
        let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
        let with_header = fast_blk.add(I64, &byte_offset, "8");
        let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
        fast_blk.load(DOUBLE, &element_ptr)
    } else {
        let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
        let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
        let with_header = fast_blk.add(I64, &byte_offset, "8");
        let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
        let element_ptr = fast_blk.inttoptr(I64, &element_addr);
        let fast_raw = fast_blk.load(DOUBLE, &element_ptr);
        // `new Array(n)` slots are TAG_HOLE internally; JavaScript reads expose
        // `undefined`.
        let fast_raw_bits = fast_blk.bitcast_double_to_i64(&fast_raw);
        let is_hole = fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
        let undef_d = fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
        fast_blk.select(I1, &is_hole, DOUBLE, &undef_d, &fast_raw)
    };
    let fast_end_label = fast_blk.label.clone();
    fast_blk.br(&merge_label);
    if require_numeric_layout {
        let fast = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: fast_val.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "NumericArrayIndexGet",
            None,
            "js_array_numeric_get_f64_unboxed",
            &fast,
            Some(BoundsState::Guarded {
                guard_id: "numeric_array_index_get_guard".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "numeric_array_index_get_guard",
                None,
            )],
            Vec::new(),
            false,
            false,
            Vec::new(),
        );
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&fast_val, &fast_end_label),
            (&fallback_val, &fallback_end_label),
        ],
    ))
}

pub(crate) fn lower_numeric_index_get_for_number_context(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::IndexGet { object, index } = expr else {
        return Ok(None);
    };
    if !is_array_expr(ctx, object) || !expr_has_numeric_pointer_free_array_layout(ctx, object) {
        return Ok(None);
    }

    if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) = (object.as_ref(), index.as_ref()) {
        if ctx
            .bounded_index_pairs
            .iter()
            .any(|fact| fact.index_local_id == *idx_id && fact.array_local_id == *arr_id)
        {
            let arr_box = lower_expr(ctx, object)?;
            let i32_slot_opt = ctx.i32_counter_slots.get(idx_id).cloned();
            let idx_i32 = if let Some(ref i32_slot) = i32_slot_opt {
                ctx.block().load(I32, i32_slot)
            } else {
                let idx_double = lower_expr(ctx, index)?;
                ctx.block().fptosi(DOUBLE, &idx_double, I32)
            };
            let idx_double = ctx.block().sitofp(I32, &idx_i32, DOUBLE);
            return lower_guarded_array_index_get(
                ctx,
                &arr_box,
                &idx_double,
                &idx_i32,
                "bidx.num",
                true,
                true,
            )
            .map(Some);
        }
    }

    let arr_box = lower_expr(ctx, object)?;
    let idx_double = lower_expr(ctx, index)?;
    let idx_i32 = ctx.block().fptosi(DOUBLE, &idx_double, I32);
    lower_guarded_array_index_get(ctx, &arr_box, &idx_double, &idx_i32, "arr", true, true).map(Some)
}

fn lower_bounded_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
) -> Result<String> {
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);

    // Issue #179 Phase 3: lazy-array guard on the bounded-index fast path.
    // Same story as the generic path below: a LazyArrayHeader has unrelated
    // bytes at `arr + 8 + idx*8`, so route through the slow path only when
    // the receiver is lazy. Issue #233: also detect FORWARDED arrays; the
    // slow path's `clean_arr_ptr` follows the chain.
    let gc_type_addr = blk.sub(I64, &arr_handle, "8");
    let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
    let gc_type = blk.load(I8, &gc_type_ptr);
    let is_lazy = blk.icmp_eq(I8, &gc_type, "9"); // GC_TYPE_LAZY_ARRAY
    let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
    let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
    let needs_slow = blk.or(I1, &is_lazy, &is_fwd);
    // Index accessors / custom attribute descriptors (`Object.defineProperty
    // (arr, i, { get })`) divert element reads through the descriptor tables —
    // the raw slot load below would bypass them (test262 sort/precise-*).
    // GcHeader._reserved (u16 at -6) carries OBJ_FLAG_ARRAY_DESCRIPTORS=0x400.
    let obj_flags_addr = blk.sub(I64, &arr_handle, "6");
    let obj_flags_ptr = blk.inttoptr(I64, &obj_flags_addr);
    let obj_flags = blk.load(I16, &obj_flags_ptr);
    let desc_bits = blk.and(I16, &obj_flags, "1024");
    let has_desc = blk.icmp_ne(I16, &desc_bits, "0");
    let needs_slow = blk.or(I1, &needs_slow, &has_desc);

    let lazy_idx = ctx.new_block("bidx.lazy");
    let fast_idx = ctx.new_block("bidx.fast");
    let merge_idx = ctx.new_block("bidx.merge");
    let lazy_label = ctx.block_label(lazy_idx);
    let fast_label = ctx.block_label(fast_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

    ctx.current_block = lazy_idx;
    let lazy_blk = ctx.block();
    let lazy_val = lazy_blk.call(
        DOUBLE,
        "js_array_get_f64",
        &[(I64, &arr_handle), (I32, idx_i32)],
    );
    let lazy_end_label = lazy_blk.label.clone();
    lazy_blk.br(&merge_label);

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let idx_i64 = fast_blk.zext(I32, idx_i32, I64);
    let byte_offset = fast_blk.shl(I64, &idx_i64, "3");
    let with_header = fast_blk.add(I64, &byte_offset, "8");
    let element_addr = fast_blk.add(I64, &arr_handle, &with_header);
    let element_ptr = fast_blk.inttoptr(I64, &element_addr);
    let fast_raw = fast_blk.load(DOUBLE, &element_ptr);
    // `new Array(n)` slots are TAG_HOLE internally; JavaScript reads expose
    // `undefined`.
    let fast_raw_bits = fast_blk.bitcast_double_to_i64(&fast_raw);
    let is_hole = fast_blk.icmp_eq(I64, &fast_raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undef_d = fast_blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let fast_val = fast_blk.select(I1, &is_hole, DOUBLE, &undef_d, &fast_raw);
    let fast_end_label = fast_blk.label.clone();
    fast_blk.br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&fast_val, &fast_end_label), (&lazy_val, &lazy_end_label)],
    ))
}

fn lower_legacy_array_index_get(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
) -> Result<String> {
    let blk = ctx.block();
    let arr_bits = blk.bitcast_double_to_i64(arr_box);
    let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);

    // Lazy/forwarded arrays need the runtime helper because their payload is
    // not the ordinary ArrayHeader element layout. Plain arrays stay fully
    // inline, including the bounds check and HOLE -> undefined translation.
    let gc_type_addr = blk.sub(I64, &arr_handle, "8");
    let gc_type_ptr = blk.inttoptr(I64, &gc_type_addr);
    let gc_type = blk.load(I8, &gc_type_ptr);
    let is_lazy = blk.icmp_eq(I8, &gc_type, "9"); // GC_TYPE_LAZY_ARRAY
    let gc_flags_addr = blk.sub(I64, &arr_handle, "7");
    let gc_flags_ptr = blk.inttoptr(I64, &gc_flags_addr);
    let gc_flags = blk.load(I8, &gc_flags_ptr);
    let fwd_bits = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
    let is_fwd = blk.icmp_ne(I8, &fwd_bits, "0");
    let needs_slow = blk.or(I1, &is_lazy, &is_fwd);
    // Index accessors / custom attribute descriptors (`Object.defineProperty
    // (arr, i, { get })`) divert element reads through the descriptor tables —
    // the raw slot load below would bypass them (test262 sort/precise-*).
    // GcHeader._reserved (u16 at -6) carries OBJ_FLAG_ARRAY_DESCRIPTORS=0x400.
    let obj_flags_addr = blk.sub(I64, &arr_handle, "6");
    let obj_flags_ptr = blk.inttoptr(I64, &obj_flags_addr);
    let obj_flags = blk.load(I16, &obj_flags_ptr);
    let desc_bits = blk.and(I16, &obj_flags, "1024");
    let has_desc = blk.icmp_ne(I16, &desc_bits, "0");
    let needs_slow = blk.or(I1, &needs_slow, &has_desc);

    let lazy_idx = ctx.new_block("arr.lazy");
    let fast_idx = ctx.new_block("arr.fast");
    let merge_idx = ctx.new_block("arr.merge");
    let lazy_label = ctx.block_label(lazy_idx);
    let fast_label = ctx.block_label(fast_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&needs_slow, &lazy_label, &fast_label);

    ctx.current_block = lazy_idx;
    let lazy_blk = ctx.block();
    let lazy_val = lazy_blk.call(
        DOUBLE,
        "js_array_get_f64",
        &[(I64, &arr_handle), (I32, idx_i32)],
    );
    let lazy_end_label = lazy_blk.label.clone();
    lazy_blk.br(&merge_label);

    ctx.current_block = fast_idx;
    let fast_blk = ctx.block();
    let len_i32 = fast_blk.safe_load_i32_from_ptr(&arr_handle);
    let in_bounds = fast_blk.icmp_ult(I32, idx_i32, &len_i32);
    let ok_idx = ctx.new_block("arr.ok");
    let oob_idx = ctx.new_block("arr.oob");
    let ok_label = ctx.block_label(ok_idx);
    let oob_label = ctx.block_label(oob_idx);
    ctx.block().cond_br(&in_bounds, &ok_label, &oob_label);

    ctx.current_block = ok_idx;
    let blk = ctx.block();
    let idx_i64 = blk.zext(I32, idx_i32, I64);
    let byte_offset = blk.shl(I64, &idx_i64, "3");
    let with_header = blk.add(I64, &byte_offset, "8");
    let element_addr = blk.add(I64, &arr_handle, &with_header);
    let element_ptr = blk.inttoptr(I64, &element_addr);
    let raw = blk.load(DOUBLE, &element_ptr);
    let raw_bits = blk.bitcast_double_to_i64(&raw);
    let is_hole = blk.icmp_eq(I64, &raw_bits, crate::nanbox::TAG_HOLE_I64);
    let undef_d = blk.bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let val = blk.select(I1, &is_hole, DOUBLE, &undef_d, &raw);
    let ok_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = oob_idx;
    let undef_bits = crate::nanbox::i64_literal(crate::nanbox::TAG_UNDEFINED);
    let undef_val = ctx.block().bitcast_i64_to_double(&undef_bits);
    let oob_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (&val, &ok_end_label),
            (&undef_val, &oob_end_label),
            (&lazy_val, &lazy_end_label),
        ],
    ))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::IndexGet { object, index } => {
            if receiver_class_name(ctx, object).as_deref() == Some("Server")
                && is_async_dispose_symbol_index(index)
            {
                return lower_class_method_bind(ctx, object, "@@__perry_wk_asyncDispose");
            }
            // Issue #611: `globalThis[<key>]` reads from the persistent
            // global-this singleton. Pre-fix, `Expr::GlobalGet` lowered
            // to the `0.0` sentinel and the generic IndexGet path called
            // `js_object_get_field_by_name_f64(0, key)` which returned
            // undefined — `(globalThis as any)[id] = m; (globalThis as
            // any)[id]` round-trip lost the value. Route through the
            // real singleton (`js_get_global_this`) when receiver is
            // GlobalGet AND the key is string-typed.
            if matches!(object.as_ref(), Expr::GlobalGet(_))
                && (matches!(index.as_ref(), Expr::String(_)) || is_string_expr(ctx, index))
            {
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let key_handle = unbox_str_handle(blk, &key_box);
                return Ok(blk.call(
                    DOUBLE,
                    "js_global_or_console_property_by_name",
                    &[(I64, &key_handle)],
                ));
            }
            if is_width_tracked_typed_array_receiver(ctx, object) {
                // A symbol-keyed read on a typed array (`ta[Symbol.toStringTag]`,
                // `ta[Symbol.iterator]`) must NOT take the dynamic-index helper
                // below — it stringifies the key and reads an ordinary [[Get]],
                // missing the `%TypedArray%.prototype` symbol accessors. Route
                // symbol keys to the symbol-property resolver (mirrors the array
                // path), which exposes `@@toStringTag` (`safe-stable-stringify`)
                // and `@@iterator`.
                if matches!(index.as_ref(), Expr::SymbolFor(_)) {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_object_get_symbol_property",
                        &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
                    ));
                }
                // #2063: a key that isn't provably a number — a method-name
                // string (`ta["copyWithin"]`, `ta[m]` where m iterates method
                // names), a numeric string (`ta["2"]`), or any non-numeric /
                // unknown-typed key — must NOT take the integer-indexed element
                // fast path below. That path blindly `fptosi`s the key; a
                // NaN-boxed string coerces to 0, so `ta["copyWithin"]`/`ta[m]`
                // returned element 0 (`typeof` was "number") and `ta["2"]`
                // returned element 0 instead of element 2. Route such keys
                // through the runtime dispatcher, which reads an element only
                // for a canonical numeric index and otherwise performs an
                // ordinary [[Get]] (the same `js_object_get_field_by_name_f64`
                // the dotted `ta.copyWithin` PropertyGet path uses — resolving
                // the prototype method once reified, undefined until then,
                // never a stray element value). `is_numeric_expr` stays true
                // for literal/loop-counter indices, so every proven element
                // fast path below is preserved.
                if !is_numeric_expr(ctx, index) {
                    let arr_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    let blk = ctx.block();
                    let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                    let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_typed_array_index_get_dynamic",
                        &[(I64, &arr_i64), (DOUBLE, &key_box)],
                    ));
                }
                if let Some(value) = lower_typed_array_load(ctx, object, index)? {
                    return Ok(materialize_js_value(
                        ctx,
                        value,
                        MaterializationReason::RuntimeApi,
                    ));
                }

                // Width-aware typed-array native lowering is only sound for
                // tracked fresh views with proven/guarded element bounds. All
                // aliases, reassigned locals, and unknown bounds stay on the
                // runtime helper, with artifact evidence for the fallback.
                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                let idx_i32 = blk.fptosi(DOUBLE, &idx_double, I32);
                let result = blk.call(
                    DOUBLE,
                    "js_typed_array_get",
                    &[(I64, &arr_i64), (I32, &idx_i32)],
                );
                let slow = LoweredValue::js_value(result.clone());
                ctx.record_lowered_value_with_access_mode(
                    "TypedArrayGet",
                    None,
                    "TypedArrayGet.slow_path",
                    &slow,
                    Some(BoundsState::Unknown),
                    None,
                    Some(BufferAccessMode::DynamicFallback),
                    Some(buffer_access_materialization_reason(ctx, object)),
                    false,
                    false,
                    vec!["typed_array_fallback=untracked_or_unproven".to_string()],
                );
                return Ok(result);
            }
            if is_uint8array_receiver(ctx, object) && !is_numeric_expr(ctx, index) {
                let arr_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_i64 = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                return Ok(blk.call(
                    DOUBLE,
                    "js_typed_array_index_get_dynamic",
                    &[(I64, &arr_i64), (DOUBLE, &key_box)],
                ));
            }
            if is_uint8array_receiver(ctx, object) && is_numeric_expr(ctx, index) {
                let value = lower_buffer_index_get_i32(ctx, object, index)?;
                let reason = buffer_access_materialization_reason(ctx, object);
                return Ok(materialize_js_value(ctx, value, reason));
            }
            // Scalar-replaced array literal: `arr[k]` where arr was bound to
            // `[...]` and never escaped, and k is a compile-time index in
            // range. Loads directly from the kth stack alloca — no heap,
            // no runtime call, no bounds check. See `collect_non_escaping_arrays`.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slots) = ctx.scalar_replaced_arrays.get(id).cloned() {
                    let k = match index.as_ref() {
                        Expr::Integer(k) if *k >= 0 => Some(*k as usize),
                        Expr::Number(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 => {
                            Some(*f as usize)
                        }
                        _ => None,
                    };
                    if let Some(k) = k {
                        if k < slots.len() {
                            let value = ctx.block().load(DOUBLE, &slots[k]);
                            let raw_f64_element =
                                crate::type_analysis::scalar_replaced_array_element_is_raw_f64(
                                    ctx,
                                    object.as_ref(),
                                    index.as_ref(),
                                );
                            let lowered_js = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: value.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarArrayIndexGet",
                                Some(*id),
                                "scalar_array_element_load",
                                &lowered_js,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![
                                    format!("index={}", k),
                                    format!("raw_f64_element={}", raw_f64_element as u8),
                                ],
                            );
                            if raw_f64_element {
                                let lowered_f64 = LoweredValue::f64(value.clone());
                                ctx.record_lowered_value_with_access_mode(
                                    "ScalarArrayIndexGet",
                                    Some(*id),
                                    "scalar_array_element_load.raw_f64",
                                    &lowered_f64,
                                    None,
                                    None,
                                    None,
                                    None,
                                    false,
                                    false,
                                    vec![format!("index={}", k), "raw_f64_element=1".to_string()],
                                );
                            }
                            return Ok(value);
                        }
                    }
                }
            }

            // Issue #50: flat-const 2D int array fast path. Replaces
            // `X[i][j]` (inline) and `krow[j]` (aliased row pattern)
            // with a direct GEP + load from a private `[N x i32]`
            // global emitted at module compile. Skips the arena header
            // + length check + double reload per access. Returns the
            // element as a NaN-boxed double (`sitofp i32 → double`) so
            // callers that expect fp receive the same JSValue shape
            // they already do; callers that expect i32 (via the #49
            // `lower_expr_as_i32` path) collapse the `fptosi(sitofp)`
            // round-trip during instcombine.
            if let Some(v) = try_lower_flat_const_index_get(ctx, object, index)? {
                return Ok(v);
            }

            // String indexing fast path: `s[i]` returns the char at
            // position i as a single-char string. Handled before the
            // array path so `str[0]` doesn't fall through to a raw
            // double load.
            if is_string_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                // #3987: route through the canonical-index runtime helper (it
                // takes the raw NaN-boxed key, not an `fptosi`'d i32) so a valid
                // array index returns its char and every non-canonical key
                // (`NaN`, `1.5`, negatives, OOB, `"01"`, non-numeric strings)
                // returns `undefined` — matching ECMAScript / Node — instead of
                // truncating the index and returning `""` for OOB.
                return Ok(blk.call(
                    DOUBLE,
                    "js_string_index_get",
                    &[(I64, &s_handle), (DOUBLE, &idx_d)],
                ));
            }
            // Issue #514: when the receiver's static type is genuinely
            // unknown (`Type::Any` / `Type::Unknown`) and the index is
            // numeric, route through the runtime tag-aware dispatcher.
            // The pre-fix array fast path interpreted `*StringHeader`
            // pointers as `*ArrayHeader`, returning the byte_len as a
            // subnormal f64 — the load-bearing bug behind hono's
            // mergePath template-literal logic that mixes `s?.[0]` /
            // `s?.at(-1)` / `s?.slice(1)` on `(s: any)` parameters.
            // The gate is narrow (only Type::Any/Unknown) so existing
            // TypedArray, Object-with-numeric-keys, and class-instance
            // fast paths keep their inline-offset reads.
            let recv_ty = crate::type_analysis::static_type_of(ctx, object);
            let recv_unknown = matches!(
                recv_ty,
                None | Some(perry_types::Type::Any) | Some(perry_types::Type::Unknown)
            );
            if recv_unknown && is_numeric_expr(ctx, index) {
                let obj_box = lower_expr(ctx, object)?;
                let idx_d = lower_expr(ctx, index)?;
                let blk = ctx.block();
                return Ok(blk.call(
                    DOUBLE,
                    "js_dyn_index_get",
                    &[(DOUBLE, &obj_box), (DOUBLE, &idx_d)],
                ));
            }
            // Three cases:
            //   1. Receiver is a known array → inline f64 element load
            //   2. Index is a string (literal or string-typed local) →
            //      generic object field access via js_object_get_field_by_name_f64
            //   3. Anything else → fall back to dynamic object field
            //      access by stringifying the index at runtime
            if is_array_expr(ctx, object) {
                // #321: a symbol-keyed array read (`arr[Symbol.iterator]`) must
                // NOT take the numeric fast path below — `fptosi` on the symbol
                // value yields a garbage index (returned a number). Route symbol
                // keys to the symbol-property resolver, which exposes the array
                // iterator for `Symbol.iterator`.
                if matches!(index.as_ref(), Expr::SymbolFor(_)) {
                    let obj_box = lower_expr(ctx, object)?;
                    let key_box = lower_expr(ctx, index)?;
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_object_get_symbol_property",
                        &[(DOUBLE, &obj_box), (DOUBLE, &key_box)],
                    ));
                }
                if !is_numeric_expr(ctx, index) {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_double = lower_expr(ctx, index)?;
                    let arr_handle = {
                        let blk = ctx.block();
                        unbox_to_i64(blk, &arr_box)
                    };
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_array_get_index_or_string",
                        &[(I64, &arr_handle), (DOUBLE, &idx_double)],
                    ));
                }
                if numeric_index_needs_runtime_key(index) {
                    let arr_box = lower_expr(ctx, object)?;
                    let idx_double = lower_expr(ctx, index)?;
                    let arr_handle = {
                        let blk = ctx.block();
                        unbox_to_i64(blk, &arr_box)
                    };
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_array_get_index_or_string",
                        &[(I64, &arr_handle), (DOUBLE, &idx_double)],
                    ));
                }
                let require_numeric_layout =
                    expr_has_numeric_pointer_free_array_layout(ctx, object);
                // Bounded-index fast path (mirrors the IndexSet
                // optimization in the same file): if the surrounding
                // for-loop registered `(counter_id, arr_id)` as
                // bounded via `lower_for`'s `classify_for_length_hoist`,
                // we can skip the bound check + OOB phi entirely.
                // The loop already proved `i < arr.length` and the
                // body provably can't change `arr.length`.
                if let (Expr::LocalGet(arr_id), Expr::LocalGet(idx_id)) =
                    (object.as_ref(), index.as_ref())
                {
                    if ctx.bounded_index_pairs.iter().any(|fact| {
                        fact.index_local_id == *idx_id && fact.array_local_id == *arr_id
                    }) {
                        let arr_box = lower_expr(ctx, object)?;
                        // Grab i32 slot name before mutably borrowing ctx for block().
                        let i32_slot_opt = ctx.i32_counter_slots.get(idx_id).cloned();
                        let idx_i32 = if let Some(ref i32_slot) = i32_slot_opt {
                            ctx.block().load(I32, i32_slot)
                        } else {
                            let idx_double = lower_expr(ctx, index)?;
                            ctx.block().fptosi(DOUBLE, &idx_double, I32)
                        };
                        if require_numeric_layout {
                            let idx_double = ctx.block().sitofp(I32, &idx_i32, DOUBLE);
                            return lower_guarded_array_index_get(
                                ctx,
                                &arr_box,
                                &idx_double,
                                &idx_i32,
                                "bidx.num",
                                true,
                                false,
                            );
                        }
                        return lower_bounded_array_index_get(ctx, &arr_box, &idx_i32);
                    }
                }

                let arr_box = lower_expr(ctx, object)?;
                let idx_double = lower_expr(ctx, index)?;
                let idx_i32 = ctx.block().fptosi(DOUBLE, &idx_double, I32);
                if !require_numeric_layout
                    && !matches!(index.as_ref(), Expr::Integer(_) | Expr::Number(_))
                {
                    return lower_legacy_array_index_get(ctx, &arr_box, &idx_i32);
                }
                return lower_guarded_array_index_get(
                    ctx,
                    &arr_box,
                    &idx_double,
                    &idx_i32,
                    "arr",
                    require_numeric_layout,
                    false,
                );
            }
            // Generic dynamic object access: stringify the index (no-op
            // for already-string keys, format for numeric keys) and
            // call js_object_get_field_by_name_f64.
            if let Expr::String(literal) = index.as_ref() {
                // Static string key: use the interned StringPool entry
                // so we get the same handle as obj["foo"].
                let preserve_class_ref_bits =
                    index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let obj_box = lower_expr(ctx, object)?;
                let key_idx = ctx.strings.intern(literal);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle =
                    classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    literal,
                    TypedFeedbackContract::object_get_by_name(),
                );
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_typed_feedback_object_get_field_by_name_f64",
                    &[(I64, &site_id), (I64, &obj_handle), (I64, &key_raw)],
                ));
            }
            if is_string_expr(ctx, index) {
                // Dynamic string key: unbox both pointers and call.
                // `key_handle` routes through `unbox_str_handle` because the
                // key may be an SSO value (e.g. from JSON.parse, .slice, or
                // any short-string-producing op); the runtime fn dereferences
                // it as `*StringHeader`. Issue #214 SSO bug class.
                let preserve_class_ref_bits =
                    index_object_is_class_or_proto_ref(ctx, object.as_ref());
                let obj_box = lower_expr(ctx, object)?;
                let key_box = lower_expr(ctx, index)?;
                let blk = ctx.block();
                let obj_bits = blk.bitcast_double_to_i64(&obj_box);
                let obj_handle =
                    classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
                let key_handle = unbox_str_handle(blk, &key_box);
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    "object[index]",
                    TypedFeedbackContract::object_get_by_name(),
                );
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_typed_feedback_object_get_field_by_name_f64",
                    &[(I64, &site_id), (I64, &obj_handle), (I64, &key_handle)],
                ));
            }
            // Last-resort fallback with runtime tag checks on the index.
            // First runtime-check whether the index is a Symbol; if so,
            // dispatch to the symbol-property side table — mirrors the
            // IndexSet branch. Otherwise fall through to string/numeric.
            let preserve_class_ref_bits = index_object_is_class_or_proto_ref(ctx, object.as_ref());
            let obj_box = lower_expr(ctx, object)?;
            let idx_box = lower_expr(ctx, index)?;
            // RequireObjectCoercible(base): `null[k]` / `undefined[k]` must throw
            // a TypeError per spec, NOT silently return undefined. The dotted
            // PropertyGet path already guards nullish receivers; the computed
            // form fell through to the by-name runtime helper (masked handle
            // `2`/`1`) which returned undefined. The check fires here — after
            // both the base and the property-key *expression* are evaluated but
            // before ToPropertyKey (key coercion / `toString`) — matching the
            // ECMAScript evaluation order (test262 compound-assignment S11.13.2_A7.*,
            // prefix/postfix increment A6). A non-nullish receiver passes through
            // unchanged. (#4918 non-class language remnant.)
            let obj_box =
                ctx.block()
                    .call(DOUBLE, "js_require_object_coercible", &[(DOUBLE, &obj_box)]);
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&obj_box);
            let obj_handle = classref_preserving_handle(blk, &obj_bits, preserve_class_ref_bits);
            let is_sym_i32 = blk.call(I32, "js_is_symbol", &[(DOUBLE, &idx_box)]);
            let is_sym_bit = blk.icmp_ne(I32, &is_sym_i32, "0");
            let sym_idx = ctx.new_block("iget.sym");
            let nonsym_idx = ctx.new_block("iget.nonsym");
            let str_idx = ctx.new_block("iget.str");
            let num_idx = ctx.new_block("iget.num");
            let merge_idx = ctx.new_block("iget.merge");
            let sym_lbl = ctx.block_label(sym_idx);
            let nonsym_lbl = ctx.block_label(nonsym_idx);
            let str_lbl = ctx.block_label(str_idx);
            let num_lbl = ctx.block_label(num_idx);
            let merge_lbl = ctx.block_label(merge_idx);
            ctx.block().cond_br(&is_sym_bit, &sym_lbl, &nonsym_lbl);
            // Symbol key → side-table get.
            ctx.current_block = sym_idx;
            let v_sym = ctx.block().call(
                DOUBLE,
                "js_object_get_symbol_property",
                &[(DOUBLE, &obj_box), (DOUBLE, &idx_box)],
            );
            let sym_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Not a symbol → recompute idx_bits in this block.
            ctx.current_block = nonsym_idx;
            let blk = ctx.block();
            let idx_bits = blk.bitcast_double_to_i64(&idx_box);
            let top16 = blk.lshr(I64, &idx_bits, "48");
            // STRING_TAG (0x7FFF = 32767): heap StringHeader pointer.
            let is_str_tag_heap = blk.icmp_eq(I64, &top16, "32767");
            let lower48 = blk.and(I64, &idx_bits, POINTER_MASK_I64);
            let is_valid_ptr = blk.icmp_ugt(I64, &lower48, "4095");
            let is_str_heap = blk.and(crate::types::I1, &is_str_tag_heap, &is_valid_ptr);
            // SHORT_STRING_TAG (0x7FF9 = 32761): inline SSO from JSON.parse,
            // .slice, etc. Lower 48 encode length+bytes, NOT a pointer, so we
            // can't AND-mask to a StringHeader; route through unbox_str_handle
            // which materializes SSO to a heap StringHeader (issue #434).
            let is_str_tag_sso = blk.icmp_eq(I64, &top16, "32761");
            let is_str = blk.or(crate::types::I1, &is_str_heap, &is_str_tag_sso);
            ctx.block().cond_br(&is_str, &str_lbl, &num_lbl);
            // String key → object field access.
            ctx.current_block = str_idx;
            let key_handle = {
                let blk = ctx.block();
                unbox_str_handle(blk, &idx_box)
            };
            let site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::PropertyGet,
                "object[index]",
                TypedFeedbackContract::object_get_by_name(),
            );
            let v_str = ctx.block().call(
                DOUBLE,
                "js_typed_feedback_object_get_field_by_name_f64",
                &[(I64, &site_id), (I64, &obj_handle), (I64, &key_handle)],
            );
            let str_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Numeric key → polymorphic dispatch.
            //
            // Closes #471 (read side, paired with the IndexSet polymorphic
            // fix above): the previous fallback emitted an inline
            // `obj_handle + 8 + idx*8` load on the assumption that the
            // receiver had an ArrayHeader (8-byte header) layout. Once the
            // IndexSet path stopped writing through that layout for plain
            // objects, the read side had to follow — `constMap[i] = v;
            // constMap[i]` would otherwise set via the object setter
            // (key stringified into the keys_array) and read from
            // `obj+8+i*8` (stale ObjectHeader fields), returning garbage
            // f64 values.
            //
            // Route through the runtime which checks the receiver's GC
            // type and dispatches: arrays/lazy/buffers/typed-arrays
            // through js_array_get_f64 (handles forwarding-chain follow
            // + lazy-materialize + per-kind reads), plain objects
            // through stringify-the-index + js_object_get_field_by_name_f64.
            ctx.current_block = num_idx;
            let v_num = ctx.block().call(
                DOUBLE,
                "js_object_get_index_polymorphic",
                &[(I64, &obj_handle), (DOUBLE, &idx_box)],
            );
            let num_end_lbl = ctx.block().label.clone();
            ctx.block().br(&merge_lbl);
            // Merge.
            ctx.current_block = merge_idx;
            Ok(ctx.block().phi(
                DOUBLE,
                &[
                    (&v_sym, &sym_end_lbl),
                    (&v_str, &str_end_lbl),
                    (&v_num, &num_end_lbl),
                ],
            ))
        }

        // Phase H err: `agg.errors.length` — receiver is
        // PropertyGet(.., "errors") which resolves to a NaN-boxed
        // ArrayHeader pointer (via the dedicated "errors" arm below).
        // Inline-read length at offset 0 just like any other array.
        // Placed ahead of the generic length fast path so we don't
        // need static type analysis to recognize the shape.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
