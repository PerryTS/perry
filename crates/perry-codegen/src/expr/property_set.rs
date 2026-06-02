//! PropertySet (obj.prop = v).
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
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_jsvalue_slot_store_on_block,
    emit_layout_note_slot_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_typed_feedback_register_site, emit_v8_export_call,
    emit_v8_member_method_call, emit_write_barrier, emit_write_barrier_slot_on_block,
    expr_is_known_non_pointer_shadow_value, extract_array_of_object_shape, i32_bool_to_nanbox,
    import_origin_suffix, is_global_this_builtin_function_name, is_global_this_builtin_name,
    is_known_finite, lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, raw_f64_layout_fact,
    try_flat_const_2d_int, try_lower_flat_const_index_get, try_lower_pod_field_set,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx, TypedFeedbackContract,
    TypedFeedbackKind,
};

fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

fn lower_runtime_property_set_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let val_double = lower_expr(ctx, value)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call_void(
        "js_object_set_field_by_name",
        &[(I64, &obj_bits), (I64, &key_raw), (DOUBLE, &val_double)],
    );
    Ok(val_double)
}

pub(crate) fn emit_nullish_write_guard(
    ctx: &mut FnCtx<'_>,
    obj_bits: &str,
    property: &str,
    label_prefix: &str,
) {
    let is_undef = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
    let is_null = ctx
        .block()
        .icmp_eq(I64, obj_bits, crate::nanbox::TAG_NULL_I64);
    let is_nullish = ctx.block().or(I1, &is_undef, &is_null);
    let throw_idx = ctx.new_block(&format!("{}.throw_nullish", label_prefix));
    let ok_idx = ctx.new_block(&format!("{}.recv_ok", label_prefix));
    let throw_label = ctx.block_label(throw_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&is_nullish, &throw_label, &ok_label);

    ctx.current_block = throw_idx;
    let key_idx = ctx.strings.intern(property);
    let prop_entry = ctx.strings.entry(key_idx);
    let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
    let prop_len_str = prop_entry.byte_len.to_string();
    let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
    ctx.block().call_void(
        "js_throw_type_error_property_access",
        &[
            (I32, &is_null_i32),
            (PTR, &prop_bytes_global),
            (I64, &prop_len_str),
        ],
    );
    ctx.block().unreachable();

    ctx.current_block = ok_idx;
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if ctx.pod_records.get(id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                }) {
                    if let Some(value) = try_lower_pod_field_set(ctx, *id, property, value)? {
                        return Ok(value);
                    }
                }
            }
            // Closes #304: `arr.length = N` must mutate the ArrayHeader, not
            // set a "length" field in the object dispatch. Pre-fix the generic
            // `js_object_set_field_by_name(arr, "length", N)` path silently
            // recorded a property on the array's hidden dispatch object but
            // never touched the real ArrayHeader.length, so subsequent reads
            // of `arr.length` returned the stale original count and the
            // elements stayed live. Statically Array-typed receivers route to
            // `js_array_set_length` which truncates / extends the header.
            // Open question: dynamic `Any`-typed receivers that happen to be
            // arrays at runtime still hit the generic path and miss the fix —
            // they'd need a runtime-side check inside js_object_set_field_by_name
            // (route to js_array_set_length when the target is registered as
            // an array). Deliberately out of scope here; the static-typed
            // case covers the issue's repro.
            if property == "length" && crate::type_analysis::is_array_expr(ctx, object) {
                let arr_box = lower_expr(ctx, object)?;
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let arr_bits = blk.bitcast_double_to_i64(&arr_box);
                let arr_handle = blk.and(I64, &arr_bits, POINTER_MASK_I64);
                blk.call_void(
                    "js_array_set_length",
                    &[(I64, &arr_handle), (DOUBLE, &val_double)],
                );
                return Ok(val_double);
            }
            // #1344: `process.env.X = v` must persist to the real OS
            // environment, not just a cached ProcessEnv object backing.
            // Pre-fix the generic `js_object_set_field_by_name` path
            // stored on the cached dict but `process.env.X` (`EnvGet`)
            // reads from `std::env::var` directly, so the value never
            // round-tripped and child processes inherited the
            // unmodified parent env.
            //
            // Route the store through `js_setenv(key, value)` (writes
            // via `std::env::set_var`, coerces non-string values to
            // strings via `js_jsvalue_to_string`). Reads still go
            // through `js_getenv_value`, so the round-trip works.
            if matches!(object.as_ref(), Expr::ProcessEnv) {
                let key_idx = ctx.strings.intern(property);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let val_double = lower_expr(ctx, value)?;
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_handle = unbox_to_i64(blk, &key_box);
                blk.call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
                return Ok(val_double);
            }
            // Scalar replacement fast path: store to the field's alloca.
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_replaced
                    .get(id)
                    .and_then(|fs| fs.get(property.as_str()))
                    .cloned()
                {
                    let val_double = lower_expr(ctx, value)?;
                    ctx.block().store(DOUBLE, &val_double, &slot);
                    let lowered = LoweredValue {
                        semantic: SemanticKind::JsValue,
                        rep: NativeRep::JsValue,
                        llvm_ty: DOUBLE,
                        value: val_double.clone(),
                    };
                    ctx.record_lowered_value_with_access_mode(
                        "ScalarObjectFieldSet",
                        Some(*id),
                        "scalar_object_field_store",
                        &lowered,
                        None,
                        None,
                        None,
                        None,
                        false,
                        false,
                        vec![format!("field={}", property)],
                    );
                    return Ok(val_double);
                }
            }
            // Handle `this` during scalar-replaced constructor inlining:
            if let Expr::This = object.as_ref() {
                if let Some(slot) = ctx
                    .scalar_ctor_target
                    .last()
                    .and_then(|tid| ctx.scalar_replaced.get(tid))
                {
                    let maybe_slot = slot.get(property.as_str()).cloned();
                    let val_double = lower_expr(ctx, value)?;
                    if let Some(slot) = maybe_slot {
                        ctx.block().store(DOUBLE, &val_double, &slot);
                        let lowered = LoweredValue {
                            semantic: SemanticKind::JsValue,
                            rep: NativeRep::JsValue,
                            llvm_ty: DOUBLE,
                            value: val_double.clone(),
                        };
                        ctx.record_lowered_value_with_access_mode(
                            "ScalarThisFieldSet",
                            None,
                            "scalar_object_field_store",
                            &lowered,
                            None,
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![format!("field={}", property)],
                        );
                    }
                    return Ok(val_double);
                }
            }
            // Setter dispatch: if the receiver is a known class and the
            // property is registered as a setter, call the synthesized
            // __set_<property> method instead of doing a raw field
            // store. The setter takes (this, value) and returns
            // undefined; we forward `value` as the expression result.
            if let Some(class_name) = receiver_class_name(ctx, object) {
                if class_has_computed_runtime_members(ctx, &class_name) {
                    return lower_runtime_property_set_by_name(ctx, object, property, value);
                }
                let setter_key = (class_name.clone(), format!("__set_{}", property));
                if let Some(fn_name) = ctx.methods.get(&setter_key).cloned() {
                    let recv_box = lower_expr(ctx, object)?;
                    let val_double = lower_expr(ctx, value)?;
                    let _ = ctx.block().call(
                        DOUBLE,
                        &fn_name,
                        &[(DOUBLE, &recv_box), (DOUBLE, &val_double)],
                    );
                    return Ok(val_double);
                }
                // Fast path: known class instance + plain instance field.
                // The runtime guard checks the receiver's class/shape and
                // descriptor state before this block touches the raw slot.
                if let Some(field_index) =
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                {
                    if let (Some(&expected_class_id), Some(keys_global_name)) = (
                        ctx.class_ids.get(&class_name),
                        ctx.class_keys_globals.get(&class_name).cloned(),
                    ) {
                        let recv_box = lower_expr(ctx, object)?;
                        let val_double = lower_expr(ctx, value)?;
                        let key_idx = ctx.strings.intern(property);
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let site_id = emit_typed_feedback_register_site(
                            ctx,
                            TypedFeedbackKind::PropertySet,
                            property,
                            TypedFeedbackContract::class_field_set(),
                        );
                        let field_idx_str = field_index.to_string();
                        let expected_class_id_str = expected_class_id.to_string();
                        let requires_raw_f64 = crate::type_analysis::class_field_declared_type(
                            ctx,
                            &class_name,
                            property,
                        )
                        .as_ref()
                        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate);
                        let requires_raw_f64_str = if requires_raw_f64 { "1" } else { "0" };
                        let (key_raw, guard_ok) = {
                            let blk = ctx.block();
                            let key_box = blk.load(DOUBLE, &key_handle_global);
                            let key_bits = blk.bitcast_double_to_i64(&key_box);
                            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                            let expected_keys = blk.load(I64, &format!("@{}", keys_global_name));
                            let guard_ok = blk.call(
                                I32,
                                "js_typed_feedback_class_field_set_guard",
                                &[
                                    (I64, &site_id),
                                    (DOUBLE, &recv_box),
                                    (I32, &expected_class_id_str),
                                    (I64, &expected_keys),
                                    (I64, &key_raw),
                                    (I32, &field_idx_str),
                                    (DOUBLE, &val_double),
                                    (I32, requires_raw_f64_str),
                                ],
                            );
                            (key_raw, guard_ok)
                        };
                        let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                        let fast_idx = ctx.new_block("class_field_set.fast");
                        let fallback_idx = ctx.new_block("class_field_set.fallback");
                        let merge_idx = ctx.new_block("class_field_set.merge");
                        let fast_label = ctx.block_label(fast_idx);
                        let fallback_label = ctx.block_label(fallback_idx);
                        let merge_label = ctx.block_label(merge_idx);
                        ctx.block()
                            .cond_br(&guard_pass, &fast_label, &fallback_label);

                        ctx.current_block = fast_idx;
                        let blk = ctx.block();
                        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
                        let obj_ptr = blk.inttoptr(I64, &obj_handle);
                        let header_skip = "24".to_string();
                        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
                        let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
                        if requires_raw_f64 {
                            // Guarded raw-f64 slots are pointer-free by typed
                            // shape descriptor; non-number writes miss the
                            // guard and use the boxed setter fallback.
                            // GC_STORE_AUDIT(POINTER_FREE): typed raw-f64 class
                            // slots contain numbers only.
                            blk.store(DOUBLE, &val_double, &field_ptr);
                        } else {
                            let field_addr = blk.ptrtoint(&field_ptr, I64);
                            emit_jsvalue_slot_store_on_block(
                                blk,
                                &field_ptr,
                                &val_double,
                                &obj_handle,
                                &field_idx_str,
                                true,
                                &obj_bits,
                                &field_addr,
                                true,
                            );
                        }
                        blk.br(&merge_label);
                        if requires_raw_f64 {
                            let stored = LoweredValue {
                                semantic: SemanticKind::JsNumber,
                                rep: NativeRep::F64,
                                llvm_ty: DOUBLE,
                                value: val_double.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "ClassFieldSet",
                                None,
                                "class_field_set.raw_f64_store",
                                &stored,
                                Some(BoundsState::Guarded {
                                    guard_id: "class_field_set_guard".to_string(),
                                }),
                                None,
                                Some(BufferAccessMode::CheckedNative),
                                None,
                                None,
                                None,
                                vec![raw_f64_layout_fact(
                                    None,
                                    "consumed",
                                    "class_field_set_guard",
                                    None,
                                )],
                                Vec::new(),
                                false,
                                false,
                                vec![
                                    format!("class={}", class_name),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                ],
                            );
                        }

                        ctx.current_block = fallback_idx;
                        let blk = ctx.block();
                        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                        blk.call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
                        blk.call_void(
                            "js_object_set_field_by_name",
                            &[(I64, &obj_bits), (I64, &key_raw), (DOUBLE, &val_double)],
                        );
                        blk.br(&merge_label);
                        if requires_raw_f64 {
                            let fallback = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: val_double.clone(),
                            };
                            ctx.record_lowered_value_with_access_mode_and_facts(
                                "ClassFieldSet",
                                None,
                                "js_object_set_field_by_name",
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
                                        "class_field_set_guard",
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
                                vec![
                                    format!("class={}", class_name),
                                    format!("field={}", property),
                                    format!("field_index={}", field_idx_str),
                                ],
                            );
                        }

                        ctx.current_block = merge_idx;
                        return Ok(val_double);
                    }
                }
            }
            let obj_box = lower_expr(ctx, object)?;
            let val_double = lower_expr(ctx, value)?;
            // Intern the field name in the StringPool (same one the
            // matching getter uses, so they share the global string).
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let obj_bits = ctx.block().bitcast_double_to_i64(&obj_box);
            emit_nullish_write_guard(ctx, &obj_bits, property, "pset");
            // Issue #618-followup: pass the FULL bits (including NaN-box
            // tag) so the runtime can detect INT32-tagged class refs
            // (`SQL.Aliased = Aliased` IIFE-static-property pattern from
            // drizzle-orm). Pre-fix the AND-with-POINTER_MASK_I64 stripped
            // the 0x7FFE tag, leaving the runtime with a small integer
            // (the class id) — which fell into the small-handle dispatch
            // path and silently dropped the assignment. The runtime now
            // checks for top16 == 0x7FFE and routes to CLASS_DYNAMIC_PROPS.
            let key_box = ctx.block().load(DOUBLE, &key_handle_global);
            let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
            let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
            let site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::PropertySet,
                property,
                TypedFeedbackContract::object_set_by_name(),
            );
            ctx.block().call_void(
                "js_typed_feedback_object_set_field_by_name_fast",
                &[
                    (I64, &site_id),
                    (I64, &obj_bits),
                    (I64, &key_raw),
                    (DOUBLE, &val_double),
                ],
            );
            Ok(val_double)
        }

        // `obj.field` — generic object field read. We get the key string
        // handle from the StringPool (interned, so the same key across
        // multiple sites shares one allocation), unbox both the object
        // pointer and the key handle, then call
        // `js_object_get_field_by_name_f64`. The result is a raw f64
        // (which IS the NaN-boxed value for non-number fields — same bit
        // pattern, runtime callers re-interpret based on context).
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
