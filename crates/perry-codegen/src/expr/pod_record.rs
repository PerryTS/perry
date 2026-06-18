use anyhow::Result;

use perry_hir::Expr;

use crate::nanbox::{double_literal, POINTER_MASK_I64, TAG_UNDEFINED_I64};
use crate::native_value::{
    field_expected_rep, llvm_type_for_native_rep, materialize_js_value, BufferAccessMode,
    LoweredValue, MaterializationReason, NativeRep, NativeValueState, PodLayoutField,
    PodLayoutManifest, SemanticKind,
};
use crate::type_analysis::expr_may_return_boxed_value_from_raw_f64_fallback;
use crate::types::{DOUBLE, F32, I32, I64, I8};

use super::{
    emit_root_nanbox_store_on_block, lower_expr, lower_expr_native, nanbox_pointer_inline, FnCtx,
};

pub(crate) fn materialize_pod_local(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    reason: MaterializationReason,
) -> Result<String> {
    let Some(local) = ctx.pod_records.get(&local_id).cloned() else {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    };
    Ok(materialize_pod_parts(
        ctx,
        local_id,
        &local.layout,
        &local.data_slot,
        &local.materialized_slot,
        reason,
    ))
}

pub(crate) fn try_lower_pod_field_get(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    property: &str,
) -> Result<Option<String>> {
    let Some(local) = ctx.pod_records.get(&local_id).cloned() else {
        return Ok(None);
    };
    let Some(field) = local
        .layout
        .fields
        .iter()
        .find(|field| field.name == property)
        .cloned()
    else {
        return Ok(None);
    };

    let current = ctx.block().load(DOUBLE, &local.materialized_slot);
    let current_bits = ctx.block().bitcast_double_to_i64(&current);
    let is_unmaterialized = ctx.block().icmp_eq(I64, &current_bits, TAG_UNDEFINED_I64);
    let native_idx = ctx.new_block("pod.get.native");
    let fallback_idx = ctx.new_block("pod.get.materialized");
    let merge_idx = ctx.new_block("pod.get.merge");
    let native_label = ctx.block_label(native_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&is_unmaterialized, &native_label, &fallback_label);

    ctx.current_block = native_idx;
    let native_js = load_pod_field_as_js(
        ctx,
        local_id,
        &local.data_slot,
        &field,
        MaterializationReason::PodMaterialization,
        "pod_record_field_get",
    );
    let native_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = fallback_idx;
    let key_handle = interned_key_handle(ctx, property);
    let obj_handle = unbox_object_handle(ctx, &current);
    let fallback_js = ctx.block().call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_handle), (I64, &key_handle)],
    );
    record_pod_dynamic_fallback(
        ctx,
        "PodRecordFieldGet",
        Some(local_id),
        "pod_record_field_get_materialized_object",
        &fallback_js,
        MaterializationReason::PodMaterialization,
        vec![
            format!("layout_id={}", local.layout.layout_id),
            format!("field={}", property),
        ],
    );
    let fallback_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&native_js, &native_end), (&fallback_js, &fallback_end)],
    )))
}

pub(crate) fn try_lower_pod_field_set(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    property: &str,
    value: &Expr,
) -> Result<Option<String>> {
    let Some(local) = ctx.pod_records.get(&local_id).cloned() else {
        return Ok(None);
    };
    let Some(field) = local
        .layout
        .fields
        .iter()
        .find(|field| field.name == property)
        .cloned()
    else {
        return Ok(None);
    };

    let value_js = lower_expr(ctx, value)?;
    let current = ctx.block().load(DOUBLE, &local.materialized_slot);
    let current_bits = ctx.block().bitcast_double_to_i64(&current);
    let is_unmaterialized = ctx.block().icmp_eq(I64, &current_bits, TAG_UNDEFINED_I64);
    let native_idx = ctx.new_block("pod.set.native");
    let fallback_idx = ctx.new_block("pod.set.materialized");
    let merge_idx = ctx.new_block("pod.set.merge");
    let native_label = ctx.block_label(native_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&is_unmaterialized, &native_label, &fallback_label);

    ctx.current_block = native_idx;
    let is_compatible = pod_field_write_compatibility_guard(ctx, &value_js, &field);
    let checked_native_idx = ctx.new_block("pod.set.native.checked");
    let dynamic_idx = ctx.new_block("pod.set.native.dynamic");
    let checked_native_label = ctx.block_label(checked_native_idx);
    let dynamic_label = ctx.block_label(dynamic_idx);
    ctx.block()
        .cond_br(&is_compatible, &checked_native_label, &dynamic_label);

    ctx.current_block = checked_native_idx;
    let native_value = coerce_js_double_to_native(ctx, &value_js, &field);
    store_pod_field_native(ctx, local_id, &local.data_slot, &field, &native_value);
    let native_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = dynamic_idx;
    let materialized = materialize_pod_parts(
        ctx,
        local_id,
        &local.layout,
        &local.data_slot,
        &local.materialized_slot,
        MaterializationReason::PodDynamicMutation,
    );
    let key_handle = interned_key_handle(ctx, property);
    let obj_handle = unbox_object_handle(ctx, &materialized);
    ctx.block().call_void(
        "js_object_set_field_by_name",
        &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &value_js)],
    );
    record_pod_dynamic_fallback(
        ctx,
        "PodRecordFieldSet",
        Some(local_id),
        "pod_record_field_set_dynamic_value",
        &value_js,
        MaterializationReason::PodDynamicMutation,
        vec![
            format!("layout_id={}", local.layout.layout_id),
            format!("field={}", property),
            format!("native_rep={}", field.native_rep.name()),
            "rhs_not_scalar_compatible".to_string(),
        ],
    );
    let dynamic_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = fallback_idx;
    let key_handle = interned_key_handle(ctx, property);
    let obj_handle = unbox_object_handle(ctx, &current);
    ctx.block().call_void(
        "js_object_set_field_by_name",
        &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &value_js)],
    );
    record_pod_dynamic_fallback(
        ctx,
        "PodRecordFieldSet",
        Some(local_id),
        "pod_record_field_set_materialized_object",
        &value_js,
        MaterializationReason::PodMaterialization,
        vec![
            format!("layout_id={}", local.layout.layout_id),
            format!("field={}", property),
        ],
    );
    let fallback_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (&value_js, &native_end),
            (&value_js, &dynamic_end),
            (&value_js, &fallback_end),
        ],
    )))
}

pub(crate) fn lower_pod_local_reassignment(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    value: &Expr,
) -> Result<Option<String>> {
    if !ctx.pod_records.contains_key(&local_id) {
        return Ok(None);
    }
    let value_js = lower_expr(ctx, value)?;
    ctx.pod_records.remove(&local_id);
    if let Some(slot) = ctx.locals.get(&local_id).cloned() {
        ctx.block().store(DOUBLE, &value_js, &slot);
    }
    if let Some(global_name) = ctx.module_globals.get(&local_id).cloned() {
        let global_ref = format!("@{}", global_name);
        emit_root_nanbox_store_on_block(ctx.block(), &value_js, &global_ref);
    }
    record_pod_dynamic_fallback(
        ctx,
        "PodRecordLocalSet",
        Some(local_id),
        "pod_record_dynamic_mutation",
        &value_js,
        MaterializationReason::PodDynamicMutation,
        vec!["local_reassignment".to_string()],
    );
    Ok(Some(value_js))
}

pub(crate) fn load_pod_field_native(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    data_slot: &str,
    field: &PodLayoutField,
    consumer: &'static str,
) -> LoweredValue {
    let ptr = pod_field_ptr(ctx, data_slot, field.offset);
    let llvm_ty =
        llvm_type_for_native_rep(&field.native_rep).expect("pod field reps have scalar LLVM types");
    let value = ctx.block().load_aligned(llvm_ty, &ptr, field.alignment);
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: field.native_rep.clone(),
        llvm_ty,
        value,
    };
    ctx.record_lowered_value(
        "PodRecordFieldLoad",
        Some(local_id),
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("field={}", field.name),
            format!("offset={}", field.offset),
        ],
    );
    lowered
}

pub(crate) fn store_pod_field_native(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    data_slot: &str,
    field: &PodLayoutField,
    lowered: &LoweredValue,
) {
    let ptr = pod_field_ptr(ctx, data_slot, field.offset);
    let llvm_ty =
        llvm_type_for_native_rep(&field.native_rep).expect("pod field reps have scalar LLVM types");
    // GC_STORE_AUDIT(POINTER_FREE): POD record fields are native scalars, never heap edges.
    ctx.block()
        .store_aligned(llvm_ty, &lowered.value, &ptr, field.alignment);
    ctx.record_lowered_value(
        "PodRecordFieldStore",
        Some(local_id),
        "pod_record_field_store",
        lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("field={}", field.name),
            format!("offset={}", field.offset),
        ],
    );
}

fn materialize_pod_parts(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    layout: &PodLayoutManifest,
    data_slot: &str,
    materialized_slot: &str,
    reason: MaterializationReason,
) -> String {
    let current = ctx.block().load(DOUBLE, materialized_slot);
    let current_bits = ctx.block().bitcast_double_to_i64(&current);
    let is_unmaterialized = ctx.block().icmp_eq(I64, &current_bits, TAG_UNDEFINED_I64);
    let existing_idx = ctx.new_block("pod.materialize.existing");
    let create_idx = ctx.new_block("pod.materialize.create");
    let merge_idx = ctx.new_block("pod.materialize.merge");
    let existing_label = ctx.block_label(existing_idx);
    let create_label = ctx.block_label(create_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&is_unmaterialized, &create_label, &existing_label);

    ctx.current_block = existing_idx;
    let existing_value = current.clone();
    let existing_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = create_idx;
    let field_count = layout.fields.len().to_string();
    let obj_handle = ctx
        .block()
        .call(I64, "js_object_alloc", &[(I32, "0"), (I32, &field_count)]);
    for field in &layout.fields {
        let key_handle = interned_key_handle(ctx, &field.name);
        let value_js = load_pod_field_as_js(
            ctx,
            local_id,
            data_slot,
            field,
            reason.clone(),
            "pod_record_materialize_field",
        );
        ctx.block().call_void(
            "js_object_set_field_by_name",
            &[(I64, &obj_handle), (I64, &key_handle), (DOUBLE, &value_js)],
        );
    }
    let created_value = nanbox_pointer_inline(ctx.block(), &obj_handle);
    ctx.block().store(DOUBLE, &created_value, materialized_slot);
    let materialized = LoweredValue::js_value(created_value.clone());
    ctx.record_lowered_value_with_access_mode(
        "PodRecordMaterialize",
        Some(local_id),
        "pod_record_materialize_object",
        &materialized,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(reason),
        false,
        false,
        vec![format!("layout_id={}", layout.layout_id)],
    );
    let create_end = ctx.current_block_label();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let materialized_value = ctx.block().phi(
        DOUBLE,
        &[
            (&existing_value, &existing_end),
            (&created_value, &create_end),
        ],
    );
    if let Some(global_name) = ctx.module_globals.get(&local_id).cloned() {
        let global_ref = format!("@{}", global_name);
        emit_root_nanbox_store_on_block(ctx.block(), &materialized_value, &global_ref);
    }
    materialized_value
}

fn load_pod_field_as_js(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    data_slot: &str,
    field: &PodLayoutField,
    reason: MaterializationReason,
    consumer: &'static str,
) -> String {
    let lowered = load_pod_field_native(ctx, local_id, data_slot, field, consumer);
    materialize_js_value(ctx, lowered, reason)
}

pub(crate) fn lower_and_store_initial_pod_field(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    data_slot: &str,
    field: &PodLayoutField,
    value: &Expr,
) -> Result<()> {
    let needs_raw_f64_fallback_coercion =
        expr_may_return_boxed_value_from_raw_f64_fallback(ctx, value)
            || matches!(value, Expr::IndexGet { .. } | Expr::PropertyGet { .. });
    let lowered = if matches!(field.native_rep, NativeRep::F64 | NativeRep::F32)
        && needs_raw_f64_fallback_coercion
    {
        let raw = lower_expr(ctx, value)?;
        let coerced = ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &raw)]);
        let (rep, llvm_ty, value) = match field.native_rep {
            NativeRep::F64 => (NativeRep::F64, DOUBLE, coerced),
            NativeRep::F32 => {
                let value = ctx.block().fptrunc(DOUBLE, &coerced, F32);
                (NativeRep::F32, F32, value)
            }
            _ => unreachable!(),
        };
        LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep,
            llvm_ty,
            value,
        }
    } else {
        let expected = field_expected_rep(field);
        lower_expr_native(ctx, value, expected)?
    };
    store_pod_field_native(ctx, local_id, data_slot, field, &lowered);
    Ok(())
}

fn coerce_js_double_to_native(
    ctx: &mut FnCtx<'_>,
    value_js: &str,
    field: &PodLayoutField,
) -> LoweredValue {
    let value = match field.native_rep {
        NativeRep::I32 => ctx.block().fptosi(DOUBLE, value_js, I32),
        NativeRep::I64 => ctx.block().fptosi(DOUBLE, value_js, I64),
        NativeRep::U32 | NativeRep::BufferLen => ctx.block().toint32(value_js),
        NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => {
            ctx.block().fptoui(DOUBLE, value_js, I64)
        }
        NativeRep::F64 => value_js.to_string(),
        NativeRep::F32 => ctx.block().fptrunc(DOUBLE, value_js, F32),
        _ => value_js.to_string(),
    };
    let llvm_ty =
        llvm_type_for_native_rep(&field.native_rep).expect("pod field reps have scalar LLVM types");
    LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: field.native_rep.clone(),
        llvm_ty,
        value,
    }
}

fn pod_field_write_compatibility_guard(
    ctx: &mut FnCtx<'_>,
    value_js: &str,
    field: &PodLayoutField,
) -> String {
    let rep_id = pod_scalar_guard_rep_id(&field.native_rep);
    let compatible = ctx.block().call(
        I32,
        "js_pod_scalar_write_compatible",
        &[(DOUBLE, value_js), (I32, &rep_id.to_string())],
    );
    ctx.block().icmp_ne(I32, &compatible, "0")
}

fn pod_scalar_guard_rep_id(rep: &NativeRep) -> i32 {
    match rep {
        NativeRep::I32 => 1,
        NativeRep::I64 => 2,
        NativeRep::U32 => 3,
        NativeRep::U64 => 4,
        NativeRep::USize => 5,
        NativeRep::F64 => 6,
        NativeRep::F32 => 7,
        NativeRep::BufferLen => 8,
        NativeRep::HandleId => 9,
        _ => 0,
    }
}

fn pod_field_ptr(ctx: &mut FnCtx<'_>, data_slot: &str, offset: u32) -> String {
    ctx.block()
        .gep(I8, data_slot, &[(I32, &offset.to_string())])
}

fn interned_key_handle(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    ctx.block().and(I64, &key_bits, POINTER_MASK_I64)
}

fn unbox_object_handle(ctx: &mut FnCtx<'_>, object_value: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(object_value);
    ctx.block().and(I64, &bits, POINTER_MASK_I64)
}

fn record_pod_dynamic_fallback(
    ctx: &mut FnCtx<'_>,
    expr_kind: &'static str,
    local_id: Option<u32>,
    consumer: &'static str,
    value: &str,
    reason: MaterializationReason,
    notes: Vec<String>,
) {
    let lowered = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.to_string(),
    };
    ctx.record_lowered_value_with_access_mode(
        expr_kind,
        local_id,
        consumer,
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(reason),
        false,
        false,
        notes,
    );
    if let Some(record) = ctx.native_rep_records.last_mut() {
        record.native_value_state = NativeValueState::DynamicFallback;
    }
}
