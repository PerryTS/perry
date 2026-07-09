//! Free helper functions extracted from `property_get.rs`.
//!
//! Pure mechanical move — bodies are verbatim. Visibility widened to
//! `pub(crate)` so both the trunk's guarded arms and the sibling general
//! dispatch can reach them.

use super::*;

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
    is_numeric_expr, is_numeric_typed_array_class, is_set_expr, is_string_expr,
    is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

pub(crate) fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

pub(crate) fn lower_runtime_property_get_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    // The helper takes a raw `*const ObjectHeader`, so strip the NaN-box
    // POINTER_TAG to a canonical pointer (mirrors the property_id masking).
    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let property_id = blk.and(I64, &key_bits, POINTER_MASK_I64);
    Ok(blk.call(
        DOUBLE,
        "js_object_get_field_by_property_id_f64",
        &[(I64, &obj_handle), (I64, &property_id)],
    ))
}

pub(crate) fn lower_class_method_bind(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method_name: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(method_name);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let method_id = blk.and(I64, &key_bits, POINTER_MASK_I64);
    Ok(blk.call(
        DOUBLE,
        "js_class_method_bind_by_id",
        &[(DOUBLE, &recv_box), (I64, &method_id)],
    ))
}

pub(crate) fn is_primitive_builtin_proto_method(builtin_name: &str, method_name: &str) -> bool {
    match builtin_name {
        "Number" => matches!(
            method_name,
            "toExponential" | "toFixed" | "toLocaleString" | "toPrecision" | "toString" | "valueOf"
        ),
        "Boolean" | "Symbol" => matches!(method_name, "toString" | "valueOf"),
        "BigInt" => matches!(method_name, "toString" | "valueOf"),
        _ => false,
    }
}

pub(crate) fn builtin_prototype_method_read<'a>(
    object: &'a Expr,
    property: &'a str,
) -> Option<(&'a str, &'a str)> {
    let Expr::PropertyGet {
        object: ctor_object,
        property: proto_property,
    } = object
    else {
        return None;
    };
    if proto_property != "prototype" {
        return None;
    }
    let Expr::PropertyGet {
        object: global_object,
        property: builtin_name,
    } = ctor_object.as_ref()
    else {
        return None;
    };
    if !matches!(global_object.as_ref(), Expr::GlobalGet(_)) {
        return None;
    }
    is_primitive_builtin_proto_method(builtin_name, property)
        .then_some((builtin_name.as_str(), property))
}

pub(crate) fn is_global_builtin_value_expr(expr: &Expr, name: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property }
            if property == name && matches!(object.as_ref(), Expr::GlobalGet(_))
    )
}

pub(crate) fn promise_static_function_length_expr(expr: &Expr) -> Option<u32> {
    let Expr::PropertyGet { object, property } = expr else {
        return None;
    };
    let is_promise_receiver = matches!(object.as_ref(), Expr::GlobalGet(_))
        || is_global_builtin_value_expr(object, "Promise");
    if !is_promise_receiver {
        return None;
    }
    match property.as_str() {
        "withResolvers" => Some(0),
        "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "try" => Some(1),
        _ => None,
    }
}

pub(crate) fn lower_global_builtin_static_value(
    ctx: &mut FnCtx<'_>,
    builtin: &str,
    property: &str,
) -> String {
    if builtin == "Promise" {
        let key_idx = ctx.strings.intern(property);
        let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
        let key_len = property.len().to_string();
        return ctx.block().call(
            DOUBLE,
            "js_promise_static_function_value",
            &[(PTR, &key_bytes_global), (I64, &key_len)],
        );
    }

    let builtin_idx = ctx.strings.intern(builtin);
    let builtin_bytes_global = format!("@{}", ctx.strings.entry(builtin_idx).bytes_global);
    let builtin_len = builtin.len().to_string();
    let builtin_value = ctx.block().call(
        DOUBLE,
        "js_get_global_this_builtin_value",
        &[(PTR, &builtin_bytes_global), (I64, &builtin_len)],
    );
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let builtin_handle = unbox_to_i64(blk, &builtin_value);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &builtin_handle), (I64, &key_raw)],
    )
}

pub(crate) fn lower_raw_f64_class_field_get_for_number_context(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::PropertyGet { object, property } = expr else {
        return Ok(None);
    };

    // Scalar-replaced objects do not have a valid heap receiver. The general
    // property-get lowering handles this, but native-f64 numeric contexts query
    // raw class-field lowering first. Keep allocation-elided objects on their
    // scalar slots rather than feeding a dummy/uninitialized receiver into the
    // class-field guard path.
    if let Expr::LocalGet(id) = object.as_ref() {
        if let Some(slot) = ctx
            .scalar_replaced
            .get(id)
            .and_then(|fs| fs.get(property.as_str()))
            .cloned()
        {
            let declared_raw_f64 = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                ctx,
                object.as_ref(),
                property,
            );
            let raw_f64_field = crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                ctx,
                Some(*id),
                property,
                declared_raw_f64,
            );
            if !raw_f64_field {
                return Ok(None);
            }
            let value = ctx.block().load(DOUBLE, &slot);
            let lowered_js = LoweredValue {
                semantic: SemanticKind::JsValue,
                rep: NativeRep::JsValue,
                llvm_ty: DOUBLE,
                value: value.clone(),
            };
            ctx.record_lowered_value_with_access_mode(
                "ScalarObjectFieldGet",
                Some(*id),
                "scalar_object_field_load",
                &lowered_js,
                None,
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("field={}", property),
                    format!("raw_f64_field={}", raw_f64_field as u8),
                    "number_context=true".to_string(),
                ],
            );
            let lowered_f64 = LoweredValue::f64(value.clone());
            ctx.record_lowered_value_with_access_mode(
                "ScalarObjectFieldGet",
                Some(*id),
                "scalar_object_field_load.raw_f64",
                &lowered_f64,
                None,
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("field={}", property),
                    "raw_f64_field=1".to_string(),
                    "number_context=true".to_string(),
                ],
            );
            return Ok(Some(value));
        }
    }

    if let Expr::This = object.as_ref() {
        if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
            if let Some(slot) = ctx
                .scalar_replaced
                .get(&target_id)
                .and_then(|fs| fs.get(property.as_str()))
                .cloned()
            {
                let declared_raw_f64 = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                    ctx,
                    object.as_ref(),
                    property,
                );
                let raw_f64_field = crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                    ctx,
                    Some(target_id),
                    property,
                    declared_raw_f64,
                );
                if !raw_f64_field {
                    return Ok(None);
                }
                let value = ctx.block().load(DOUBLE, &slot);
                let lowered_js = LoweredValue {
                    semantic: SemanticKind::JsValue,
                    rep: NativeRep::JsValue,
                    llvm_ty: DOUBLE,
                    value: value.clone(),
                };
                ctx.record_lowered_value_with_access_mode(
                    "ScalarThisFieldGet",
                    Some(target_id),
                    "scalar_object_field_load",
                    &lowered_js,
                    None,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![
                        format!("field={}", property),
                        format!("raw_f64_field={}", raw_f64_field as u8),
                        "number_context=true".to_string(),
                    ],
                );
                let lowered_f64 = LoweredValue::f64(value.clone());
                ctx.record_lowered_value_with_access_mode(
                    "ScalarThisFieldGet",
                    Some(target_id),
                    "scalar_object_field_load.raw_f64",
                    &lowered_f64,
                    None,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![
                        format!("field={}", property),
                        "raw_f64_field=1".to_string(),
                        "number_context=true".to_string(),
                    ],
                );
                return Ok(Some(value));
            }
        }
    }

    let Some(class_name) = receiver_class_name(ctx, object) else {
        return Ok(None);
    };
    if class_has_computed_runtime_members(ctx, &class_name) {
        return Ok(None);
    }

    let is_static_accessor = ctx
        .classes
        .get(&class_name)
        .map(|c| c.static_accessor_names.iter().any(|n| n == property))
        .unwrap_or(false);
    let getter_key = (class_name.clone(), format!("__get_{}", property));
    if is_static_accessor || ctx.methods.contains_key(&getter_key) {
        return Ok(None);
    }

    let Some(declared_type) =
        crate::type_analysis::class_field_declared_type(ctx, &class_name, property)
    else {
        return Ok(None);
    };
    if !crate::typed_shape::type_is_raw_f64_candidate(&declared_type) {
        return Ok(None);
    }
    let Some(field_index) =
        crate::type_analysis::class_field_global_index(ctx, &class_name, property)
    else {
        return Ok(None);
    };
    let (Some(&expected_class_id), Some(keys_global_name)) = (
        ctx.class_ids.get(&class_name),
        ctx.class_keys_globals.get(&class_name).cloned(),
    ) else {
        return Ok(None);
    };

    // #5093 loop versioning: inside the fast clone of a class-field versioned
    // loop, a tracked number-context field read on the proven receiver lowers
    // to a bare slot load on the preheader-cached object pointer — no shape
    // check, no guard call, no fallback (see stmt/loops.rs). Mirrors the hook
    // in the generic class-field GET diamond (property_get.rs).
    let loop_fact_ptr = match object.as_ref() {
        Expr::LocalGet(recv_id) => crate::expr::class_field_loop_fact_lookup(
            &ctx.class_field_loop_facts,
            *recv_id,
            &class_name,
            property,
        )
        .filter(|(_, loop_idx)| *loop_idx == field_index)
        .map(|(fact, _)| fact.obj_ptr.clone()),
        _ => None,
    };
    if let Some(obj_ptr) = loop_fact_ptr {
        let field_idx_str = field_index.to_string();
        let blk = ctx.block();
        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, "24")]);
        let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
        let val = blk.load(DOUBLE, &field_ptr);
        let fast = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "ClassFieldGet",
            None,
            "class_field_get_number.loop_raw_f64_load",
            &fast,
            Some(BoundsState::Guarded {
                guard_id: "class_field_loop_preheader_check".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "class_field_loop_preheader_check",
                None,
            )],
            Vec::new(),
            false,
            false,
            vec![
                format!("class={}", class_name),
                format!("field={}", property),
                format!("field_index={}", field_idx_str),
                "receiver_proof=loop_preheader_shape_check".to_string(),
                "field_layout=raw_f64_slot_array".to_string(),
                "loop_versioning=class_field_fast_clone".to_string(),
            ],
        );
        return Ok(Some(val));
    }

    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::PropertyGet,
        property,
        TypedFeedbackContract::class_field_get(),
    );
    let field_idx_str = field_index.to_string();
    let expected_class_id_str = expected_class_id.to_string();
    let (obj_bits, obj_handle, key_raw, expected_keys) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        let expected_keys = blk.load(I64, &format!("@{}", keys_global_name));
        (obj_bits, obj_handle, key_raw, expected_keys)
    };

    let fast_idx = ctx.new_block("class_field_get_number.fast");
    let fallback_idx = ctx.new_block("class_field_get_number.fallback");
    let merge_idx = ctx.new_block("class_field_get_number.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    let _guardcall_label = crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
        ctx,
        &obj_bits,
        &obj_handle,
        &expected_class_id_str,
        &expected_keys,
        field_index,
        true,
        None,
        &fast_label,
    );
    let guard_ok = ctx.block().call(
        I32,
        "js_typed_feedback_class_field_get_guard",
        &[
            (I64, &site_id),
            (DOUBLE, &recv_box),
            (I32, &expected_class_id_str),
            (I64, &expected_keys),
            (I64, &key_raw),
            (I32, &field_idx_str),
            (I32, "1"),
        ],
    );
    let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
    ctx.block()
        .cond_br(&guard_pass, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let blk = ctx.block();
    let obj_ptr = blk.inttoptr(I64, &obj_handle);
    let header_skip = "24".to_string();
    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
    let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
    let val_fast = blk.load(DOUBLE, &field_ptr);
    let fast_end_label = blk.label.clone();
    blk.br(&merge_label);
    let fast = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: val_fast.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ClassFieldGet",
        None,
        "class_field_get.raw_f64_number_context",
        &fast,
        Some(BoundsState::Guarded {
            guard_id: "class_field_get_guard".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![raw_f64_layout_fact(
            None,
            "consumed",
            "class_field_get_guard",
            None,
        )],
        Vec::new(),
        false,
        false,
        vec![
            format!("class={}", class_name),
            format!("class_id={}", expected_class_id_str),
            format!("field={}", property),
            format!("field_index={}", field_idx_str),
            "receiver_proof=declared_named_receiver_guarded_exact_class".to_string(),
            "field_layout=raw_f64_slot_array".to_string(),
            "pointer_bitmap=non_pointer".to_string(),
            "number_context=true".to_string(),
        ],
    );

    ctx.current_block = fallback_idx;
    let blk = ctx.block();
    blk.call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
    let val_fallback_js = blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &key_raw)],
    );
    let val_fallback = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &val_fallback_js)]);
    let fallback_end_label = blk.label.clone();
    blk.br(&merge_label);
    let fallback = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: val_fallback_js.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ClassFieldGet",
        None,
        "js_object_get_field_by_name_f64.number_context_fallback",
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
                "class_field_get_guard",
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
            "number_context=true".to_string(),
        ],
    );

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (&val_fast, &fast_end_label),
            (&val_fallback, &fallback_end_label),
        ],
    )))
}
