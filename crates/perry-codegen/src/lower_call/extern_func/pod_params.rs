//! Manifest POD-record and POD-view parameter lowering for native extern calls:
//! marshal a JS object (or view) into a stack temp laid out per the manifest's
//! `PodLayout`, with a dynamic fallback when the value is not a plain object.
//! Split out of `extern_func.rs` to keep it under the 2,000-line cap (#10750);
//! pure relocation.

use super::*;

fn record_native_abi_pod_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    local_id: Option<u32>,
    js_argument_index: usize,
    abi_slot_index: usize,
    data_ptr: &str,
    layout: &PodLayoutManifest,
    runtime_guard: Option<(&'static str, &'static str)>,
    access_mode: Option<BufferAccessMode>,
    materialization_reason: Option<MaterializationReason>,
    notes: Vec<String>,
) {
    let mut abi_record = NativeAbiTypeRecord::new(
        descriptor,
        NativeAbiDirection::Param,
        Some(js_argument_index),
        abi_slot_index,
    );
    if let Some((helper, requirement)) = runtime_guard {
        abi_record = abi_record.with_runtime_guard(helper, requirement);
    }
    let lowered = LoweredValue {
        semantic: SemanticKind::PodRecord,
        rep: NativeRep::PodRecord {
            layout_id: layout.layout_id.clone(),
            size: layout.size,
            alignment: layout.alignment,
        },
        llvm_ty: PTR,
        value: data_ptr.to_string(),
    };
    ctx.record_lowered_value_with_native_abi_and_pod_layout(
        "NativeLibraryParam",
        local_id,
        "native_library.param.pod",
        &lowered,
        abi_record,
        Some(layout.clone()),
        access_mode,
        materialization_reason,
        notes,
    );
}

fn record_native_abi_pod_dynamic_fallback(
    ctx: &mut FnCtx<'_>,
    local_id: Option<u32>,
    value: &str,
    layout: &PodLayoutManifest,
    notes: Vec<String>,
) {
    let lowered = LoweredValue::js_value(value.to_string());
    let mut all_notes = vec![format!("layout_id={}", layout.layout_id)];
    all_notes.extend(notes);
    ctx.record_lowered_value_with_access_mode(
        "NativeLibraryParamPodFallback",
        local_id,
        "native_library.param.pod_materialized_object",
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::PodMaterialization),
        false,
        false,
        all_notes,
    );
}

fn pod_field_ptr(ctx: &mut FnCtx<'_>, data_slot: &str, offset: u32) -> String {
    if offset == 0 {
        data_slot.to_string()
    } else {
        ctx.block()
            .gep(I8, data_slot, &[(I32, &offset.to_string())])
    }
}

fn interned_pod_key_handle(ctx: &mut FnCtx<'_>, property: &str) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    ctx.block().and(I64, &key_bits, POINTER_MASK_I64)
}

fn lower_pod_field_from_js_value(
    ctx: &mut FnCtx<'_>,
    local_id: Option<u32>,
    field: &crate::native_value::PodLayoutField,
    value: &str,
) -> LoweredValue {
    let (helper, lowered) = match &field.native_rep {
        NativeRep::I8 => {
            let raw = ctx
                .block()
                .call(I8, "js_native_abi_check_i8", &[(DOUBLE, value)]);
            ("js_native_abi_check_i8", LoweredValue::i8(raw))
        }
        NativeRep::I16 => {
            let raw = ctx
                .block()
                .call(I16, "js_native_abi_check_i16", &[(DOUBLE, value)]);
            ("js_native_abi_check_i16", LoweredValue::i16(raw))
        }
        NativeRep::I32 => {
            let raw = ctx
                .block()
                .call(I32, "js_native_abi_check_i32", &[(DOUBLE, value)]);
            ("js_native_abi_check_i32", LoweredValue::i32(raw))
        }
        NativeRep::I64 => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_i64", &[(DOUBLE, value)]);
            ("js_native_abi_check_i64", LoweredValue::i64(raw))
        }
        NativeRep::U8 => {
            let raw = ctx
                .block()
                .call(I8, "js_native_abi_check_u8", &[(DOUBLE, value)]);
            ("js_native_abi_check_u8", LoweredValue::u8(raw))
        }
        NativeRep::U16 => {
            let raw = ctx
                .block()
                .call(I16, "js_native_abi_check_u16", &[(DOUBLE, value)]);
            ("js_native_abi_check_u16", LoweredValue::u16(raw))
        }
        NativeRep::U32 => {
            let raw = ctx
                .block()
                .call(I32, "js_native_abi_check_u32", &[(DOUBLE, value)]);
            ("js_native_abi_check_u32", LoweredValue::u32(raw))
        }
        NativeRep::U64 => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_u64", &[(DOUBLE, value)]);
            ("js_native_abi_check_u64", LoweredValue::u64(raw))
        }
        NativeRep::USize => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_usize", &[(DOUBLE, value)]);
            ("js_native_abi_check_usize", LoweredValue::usize(raw))
        }
        NativeRep::ISize => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_isize", &[(DOUBLE, value)]);
            ("js_native_abi_check_isize", LoweredValue::isize(raw))
        }
        NativeRep::F32 => {
            let raw = ctx
                .block()
                .call(F32, "js_native_abi_check_f32", &[(DOUBLE, value)]);
            ("js_native_abi_check_f32", LoweredValue::f32(raw))
        }
        NativeRep::F64 => {
            let raw = ctx
                .block()
                .call(DOUBLE, "js_native_abi_check_f64", &[(DOUBLE, value)]);
            ("js_native_abi_check_f64", LoweredValue::f64(raw))
        }
        NativeRep::BufferLen => {
            let raw = ctx
                .block()
                .call(I32, "js_native_abi_check_u32", &[(DOUBLE, value)]);
            ("js_native_abi_check_u32", LoweredValue::buffer_len(raw))
        }
        NativeRep::HandleId => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_u64", &[(DOUBLE, value)]);
            ("js_native_abi_check_u64", LoweredValue::handle_id(raw))
        }
        other => unreachable!("manifest POD layout contained non-scalar field {other:?}"),
    };
    ctx.record_lowered_value(
        "NativeLibraryParamPodField",
        local_id,
        "native_library.param.pod_field",
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("field={}", field.name),
            format!("native_rep={}", field.native_rep.name()),
            format!("guard={helper}"),
        ],
    );
    lowered
}

fn load_pod_field_path_from_js_object(
    ctx: &mut FnCtx<'_>,
    object_handle: &str,
    path: &[String],
) -> String {
    let mut current_object = object_handle.to_string();
    let mut current_value = None;
    for (idx, part) in path.iter().enumerate() {
        let key_handle = interned_pod_key_handle(ctx, part);
        let value = ctx.block().call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &current_object), (I64, &key_handle)],
        );
        if idx + 1 == path.len() {
            current_value = Some(value);
        } else {
            current_object =
                ctx.block()
                    .call(I64, "js_native_abi_check_pod_object", &[(DOUBLE, &value)]);
        }
    }
    current_value.unwrap_or_else(|| crate::nanbox::double_literal(f64::NAN))
}

fn build_pod_temp_from_object_value(
    ctx: &mut FnCtx<'_>,
    local_id: Option<u32>,
    object_value: &str,
    layout: &PodLayoutManifest,
    fallback_notes: Vec<String>,
) -> String {
    record_native_abi_pod_dynamic_fallback(ctx, local_id, object_value, layout, fallback_notes);
    let data_slot = ctx
        .func
        .alloca_entry_bytes_aligned(layout.size, layout.alignment);
    let object_handle = ctx.block().call(
        I64,
        "js_native_abi_check_pod_object",
        &[(DOUBLE, object_value)],
    );
    for field in &layout.fields {
        let field_value = load_pod_field_path_from_js_object(ctx, &object_handle, &field.path);
        let lowered = lower_pod_field_from_js_value(ctx, local_id, field, &field_value);
        let ptr = pod_field_ptr(ctx, &data_slot, field.offset);
        let llvm_ty = llvm_type_for_native_rep(&field.native_rep)
            .expect("manifest POD field reps have scalar LLVM types");
        // GC_STORE_AUDIT(POINTER_FREE): POD record fields are native scalars, never heap edges.
        ctx.block()
            .store_aligned(llvm_ty, &lowered.value, &ptr, field.alignment);
    }
    data_slot
}

pub(in crate::lower_call) fn lower_manifest_pod_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    pod: &NativePodAbi,
    js_argument_index: usize,
    abi_slot_index: usize,
    arg: &Expr,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<crate::types::LlvmType>,
) -> Result<()> {
    let layout = layout_for_manifest_pod(pod).map_err(|reason| {
        anyhow!(
            "native ABI pod descriptor {} has invalid layout: {}",
            descriptor,
            reason
        )
    })?;

    if let Expr::LocalGet(local_id) = arg {
        if let Some(local) = ctx.pod_records.get(local_id).cloned() {
            if local.layout.layout_id != layout.layout_id
                || local.layout.size != layout.size
                || local.layout.alignment != layout.alignment
            {
                return Err(anyhow!(
                    "native ABI pod parameter {} expected layout {} (size {}, align {}) but local {} has layout {} (size {}, align {})",
                    descriptor,
                    layout.layout_id,
                    layout.size,
                    layout.alignment,
                    local_id,
                    local.layout.layout_id,
                    local.layout.size,
                    local.layout.alignment
                ));
            }

            let current = ctx.block().load(DOUBLE, &local.materialized_slot);
            let current_bits = ctx.block().bitcast_double_to_i64(&current);
            let is_unmaterialized =
                ctx.block()
                    .icmp_eq(I64, &current_bits, crate::nanbox::TAG_UNDEFINED_I64);
            let native_idx = ctx.new_block("native.pod.param.raw");
            let fallback_idx = ctx.new_block("native.pod.param.materialized");
            let merge_idx = ctx.new_block("native.pod.param.merge");
            let native_label = ctx.block_label(native_idx);
            let fallback_label = ctx.block_label(fallback_idx);
            let merge_label = ctx.block_label(merge_idx);
            ctx.block()
                .cond_br(&is_unmaterialized, &native_label, &fallback_label);

            ctx.current_block = native_idx;
            record_native_abi_pod_param(
                ctx,
                descriptor,
                Some(*local_id),
                js_argument_index,
                abi_slot_index,
                &local.data_slot,
                &layout,
                None,
                None,
                None,
                vec![
                    format!("layout_id={}", layout.layout_id),
                    "source=region_local_pod".to_string(),
                ],
            );
            let native_end = ctx.current_block_label();
            ctx.block().br(&merge_label);

            ctx.current_block = fallback_idx;
            let fallback_slot = build_pod_temp_from_object_value(
                ctx,
                Some(*local_id),
                &current,
                &layout,
                vec!["source=materialized_pod_object".to_string()],
            );
            record_native_abi_pod_param(
                ctx,
                descriptor,
                Some(*local_id),
                js_argument_index,
                abi_slot_index,
                &fallback_slot,
                &layout,
                Some((
                    "js_native_abi_check_pod_object",
                    "object_with_manifest_fields",
                )),
                None,
                None,
                vec![
                    format!("layout_id={}", layout.layout_id),
                    "source=guarded_materialized_object_copy".to_string(),
                ],
            );
            let fallback_end = ctx.current_block_label();
            ctx.block().br(&merge_label);

            ctx.current_block = merge_idx;
            let ptr = ctx.block().phi(
                PTR,
                &[
                    (&local.data_slot, &native_end),
                    (&fallback_slot, &fallback_end),
                ],
            );
            lowered.push(ptr);
            arg_types.push(PTR);
            return Ok(());
        }
    }

    let object_value = lower_expr(ctx, arg)?;
    let data_slot = build_pod_temp_from_object_value(
        ctx,
        None,
        &object_value,
        &layout,
        vec!["source=dynamic_js_value".to_string()],
    );
    record_native_abi_pod_param(
        ctx,
        descriptor,
        None,
        js_argument_index,
        abi_slot_index,
        &data_slot,
        &layout,
        Some((
            "js_native_abi_check_pod_object",
            "object_with_manifest_fields",
        )),
        None,
        None,
        vec![
            format!("layout_id={}", layout.layout_id),
            "source=guarded_dynamic_object_copy".to_string(),
        ],
    );
    lowered.push(data_slot);
    arg_types.push(PTR);
    Ok(())
}

fn pod_view_manifest(
    layout: &PodLayoutManifest,
    count_source: impl Into<String>,
) -> PodRecordViewManifest {
    PodRecordViewManifest {
        layout_id: layout.layout_id.clone(),
        stride: layout.size,
        alignment: layout.alignment,
        count_source: count_source.into(),
        pointer_free_backing: true,
        endian: layout.endian.clone(),
        packing: layout.packing.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
fn record_native_abi_pod_view_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    local_id: Option<u32>,
    js_argument_index: usize,
    abi_slot_index: usize,
    data_ptr: &str,
    count: &str,
    layout: &PodLayoutManifest,
    count_source: &str,
    notes: Vec<String>,
) {
    let runtime_view = pod_view_manifest(layout, count_source);
    let mut data_abi = NativeAbiTypeRecord::new(
        descriptor,
        NativeAbiDirection::Param,
        Some(js_argument_index),
        abi_slot_index,
    )
    .with_runtime_guard(
        "js_native_abi_check_pod_view_data_ptr",
        "registered_pod_record_view_data",
    );
    data_abi.abi_slot_count = descriptor.abi_slot_count();
    let data = LoweredValue {
        semantic: SemanticKind::PodRecordView,
        rep: NativeRep::PodRecordView {
            layout_id: layout.layout_id.clone(),
            stride: layout.size,
            alignment: layout.alignment,
        },
        llvm_ty: PTR,
        value: data_ptr.to_string(),
    };
    ctx.record_lowered_value_with_native_abi_and_pod_view(
        "NativeLibraryParam",
        local_id,
        "native_library.param.pod+count.data_ptr",
        &data,
        data_abi,
        Some(layout.clone()),
        runtime_view.clone(),
        None,
        None,
        notes.clone(),
    );

    let count_abi = NativeAbiTypeRecord::new(
        descriptor,
        NativeAbiDirection::Param,
        Some(js_argument_index),
        abi_slot_index + 1,
    )
    .with_runtime_guard(
        "js_native_abi_check_pod_view_record_count",
        "registered_pod_record_view_count",
    );
    let count_value = LoweredValue::usize(count.to_string());
    ctx.record_lowered_value_with_native_abi_and_pod_view(
        "NativeLibraryParam",
        local_id,
        "native_library.param.pod+count.record_count",
        &count_value,
        count_abi,
        Some(layout.clone()),
        runtime_view,
        None,
        None,
        notes,
    );
}

pub(in crate::lower_call) fn lower_manifest_pod_view_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    pod: &NativePodAbi,
    js_argument_index: usize,
    abi_slot_index: usize,
    arg: &Expr,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<crate::types::LlvmType>,
) -> Result<()> {
    let layout = layout_for_manifest_pod(pod).map_err(|reason| {
        anyhow!(
            "native ABI pod+count descriptor {} has invalid layout: {}",
            descriptor,
            reason
        )
    })?;
    let layout_id = (layout_runtime_id(&layout.layout_id) as i64).to_string();
    let mut local_id = None;
    let (value, count_source, source_note) = if let Expr::LocalGet(id) = arg {
        if let Some(view) = ctx.pod_views.get(id).cloned() {
            if view.layout.layout_id != layout.layout_id
                || view.layout.size != layout.size
                || view.layout.alignment != layout.alignment
            {
                return Err(anyhow!(
                    "native ABI pod+count parameter {} expected layout {} (size {}, align {}) but local {} has layout {} (size {}, align {})",
                    descriptor,
                    layout.layout_id,
                    layout.size,
                    layout.alignment,
                    id,
                    view.layout.layout_id,
                    view.layout.size,
                    view.layout.alignment
                ));
            }
            local_id = Some(*id);
            (
                ctx.block().load(DOUBLE, &view.view_slot),
                view.count_source,
                "source=local_pod_view".to_string(),
            )
        } else {
            (
                lower_expr(ctx, arg)?,
                "dynamic_js_value".to_string(),
                "source=dynamic_js_value".to_string(),
            )
        }
    } else {
        (
            lower_expr(ctx, arg)?,
            "dynamic_js_value".to_string(),
            "source=dynamic_js_value".to_string(),
        )
    };
    let data_ptr = ctx.block().call(
        PTR,
        "js_native_abi_check_pod_view_data_ptr",
        &[(DOUBLE, &value), (I64, &layout_id)],
    );
    let count = ctx.block().call(
        I64,
        "js_native_abi_check_pod_view_record_count",
        &[(DOUBLE, &value), (I64, &layout_id)],
    );
    record_native_abi_pod_view_param(
        ctx,
        descriptor,
        local_id,
        js_argument_index,
        abi_slot_index,
        &data_ptr,
        &count,
        &layout,
        &count_source,
        vec![format!("layout_id={}", layout.layout_id), source_note],
    );
    lowered.push(data_ptr);
    arg_types.push(PTR);
    lowered.push(count);
    arg_types.push(I64);
    Ok(())
}
