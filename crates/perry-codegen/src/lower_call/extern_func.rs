//! Cross-module function call via `Expr::ExternFuncRef` — covers
//! built-in extern names (setTimeout, setInterval, gc, jsx, …),
//! perry/system + perry/updater + perry/background dispatch via the
//! `lower_perry_ui_table_call` machinery, V8-fallback bridge calls,
//! and the generic `perry_fn_<src>__<name>` consumer-prefix path.

use anyhow::{anyhow, Result};
use perry_api_manifest::{
    NativeAbiType, NativeHandleAbi, NativeHandleOwnership, NativeHandleThreadAffinity, NativePodAbi,
};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::{
    layout_for_manifest_pod, layout_runtime_id, llvm_type_for_native_rep, materialize_js_value,
    materialize_native_handle_to_js_value, materialize_promise_boundary_to_js_value,
    record_runtime_native_handle_box_transition, AliasState, BoundsState, BufferAccessMode,
    BufferElem, BufferIndexUnit, LoweredValue, MaterializationReason, NativeAbiDirection,
    NativeAbiTypeRecord, NativeRep, PodLayoutManifest, PodRecordViewManifest, SemanticKind,
};
use crate::type_analysis::{is_array_expr, is_string_expr};
use crate::types::{DOUBLE, F32, I1, I32, I64, I8, PTR, VOID};

use super::{
    lower_perry_ui_table_call, perry_background_table_lookup, perry_system_table_lookup,
    perry_updater_table_lookup, try_rewrite_perry_tui_jsx_intrinsic,
};

fn record_native_abi_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    js_argument_index: usize,
    abi_slot_index: usize,
    lowered: &LoweredValue,
    runtime_guard: Option<(&'static str, &'static str)>,
    note: impl Into<String>,
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
    ctx.record_lowered_value_with_native_abi(
        "NativeLibraryParam",
        format!("native_library.param.{}", descriptor.canonical_kind()),
        lowered,
        abi_record,
        vec![note.into()],
    );
}

fn record_native_abi_return(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    lowered: &LoweredValue,
    name: &str,
) {
    ctx.record_lowered_value_with_native_abi(
        "NativeLibraryReturn",
        format!("native_library.raw_{}", descriptor.canonical_kind()),
        lowered,
        NativeAbiTypeRecord::new(descriptor, NativeAbiDirection::Return, None, 0),
        vec![format!("runtime={}", name)],
    );
}

pub(super) fn lower_buffer_and_len_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    js_argument_index: usize,
    abi_slot_index: usize,
    val: &str,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<crate::types::LlvmType>,
) {
    let blk = ctx.block();
    let data_ptr = blk.call(PTR, "js_native_abi_check_buffer_data_ptr", &[(DOUBLE, val)]);
    let byte_len = blk.call(I64, "js_native_abi_check_buffer_byte_len", &[(DOUBLE, val)]);
    let ptr_value = LoweredValue::buffer_view(
        data_ptr.clone(),
        byte_len.clone(),
        BufferElem::U8,
        1,
        BufferIndexUnit::Byte,
        None,
        0,
        BoundsState::Unknown,
        AliasState::Unknown,
    );
    record_native_abi_param(
        ctx,
        descriptor,
        js_argument_index,
        abi_slot_index,
        &ptr_value,
        Some((
            "js_native_abi_check_buffer_data_ptr",
            "registered_buffer_span_data",
        )),
        "buffer+len.data_ptr",
    );
    let len_value = LoweredValue::usize(byte_len.clone());
    record_native_abi_param(
        ctx,
        descriptor,
        js_argument_index,
        abi_slot_index + 1,
        &len_value,
        Some((
            "js_native_abi_check_buffer_byte_len",
            "registered_buffer_span_byte_len",
        )),
        "buffer+len.byte_len",
    );
    lowered.push(data_ptr);
    arg_types.push(PTR);
    lowered.push(byte_len);
    arg_types.push(I64);
}

fn i64_literal_from_u64(value: u64) -> String {
    (value as i64).to_string()
}

fn handle_ownership_code(ownership: NativeHandleOwnership) -> &'static str {
    match ownership {
        NativeHandleOwnership::Borrowed => "1",
        NativeHandleOwnership::Owned => "2",
    }
}

fn handle_thread_code(thread: NativeHandleThreadAffinity) -> &'static str {
    match thread {
        NativeHandleThreadAffinity::Any => "0",
        NativeHandleThreadAffinity::Main => "1",
        NativeHandleThreadAffinity::Creator => "2",
    }
}

fn handle_debug_name_global(ctx: &mut FnCtx<'_>, handle: &NativeHandleAbi) -> (String, String) {
    let idx = ctx.strings.intern(&handle.debug_name);
    let entry = ctx.strings.entry(idx);
    (
        format!("@{}", entry.bytes_global),
        entry.byte_len.to_string(),
    )
}

fn lower_native_handle_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    handle: &NativeHandleAbi,
    js_argument_index: usize,
    abi_slot_index: usize,
    val: &str,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<crate::types::LlvmType>,
) {
    let type_id = i64_literal_from_u64(handle.type_id());
    let nullable = if handle.nullable { "1" } else { "0" };
    let ownership = handle_ownership_code(handle.ownership);
    let thread = handle_thread_code(handle.thread);
    let raw = ctx.block().call(
        I64,
        "js_native_handle_unwrap",
        &[
            (DOUBLE, val),
            (I64, &type_id),
            (I32, nullable),
            (I32, ownership),
            (I32, thread),
        ],
    );
    let native = LoweredValue::native_handle(raw.clone());
    record_native_abi_param(
        ctx,
        descriptor,
        js_argument_index,
        abi_slot_index,
        &native,
        Some(("js_native_handle_unwrap", "native_handle_contract")),
        "native_handle.unwrap",
    );
    lowered.push(raw);
    arg_types.push(I64);
}

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

pub(super) fn lower_manifest_pod_param(
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

pub(super) fn lower_manifest_pod_view_param(
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

fn materialize_native_handle_return(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    handle: &NativeHandleAbi,
) -> String {
    let type_id = i64_literal_from_u64(handle.type_id());
    let nullable = if handle.nullable { "1" } else { "0" };
    let thread = handle_thread_code(handle.thread);
    let (debug_name_global, debug_name_len) = handle_debug_name_global(ctx, handle);
    let boxed = match handle.ownership {
        NativeHandleOwnership::Owned => {
            let finalizer = handle
                .finalizer
                .as_ref()
                .map(|symbol| {
                    ctx.pending_declares
                        .push((symbol.clone(), VOID, vec![PTR, PTR]));
                    format!("@{symbol}")
                })
                .unwrap_or_else(|| "null".to_string());
            ctx.block().call(
                DOUBLE,
                "js_native_handle_new_owned",
                &[
                    (I64, raw),
                    (I64, &type_id),
                    (I32, nullable),
                    (I32, thread),
                    (PTR, &finalizer),
                    (PTR, &debug_name_global),
                    (I64, &debug_name_len),
                ],
            )
        }
        NativeHandleOwnership::Borrowed => ctx.block().call(
            DOUBLE,
            "js_native_handle_new_borrowed",
            &[
                (I64, raw),
                (I64, &type_id),
                (I32, nullable),
                (I32, thread),
                (PTR, &debug_name_global),
                (I64, &debug_name_len),
            ],
        ),
    };
    record_runtime_native_handle_box_transition(ctx, &boxed, MaterializationReason::ReturnAbi);
    boxed
}

pub(super) fn lower_manifest_param(
    ctx: &mut FnCtx<'_>,
    descriptor: &NativeAbiType,
    js_argument_index: usize,
    abi_slot_index: usize,
    val: &str,
    lowered: &mut Vec<String>,
    arg_types: &mut Vec<crate::types::LlvmType>,
) {
    match descriptor {
        NativeAbiType::JsValue | NativeAbiType::F64 => {
            let (raw, native, guard, note) = match descriptor {
                NativeAbiType::JsValue => (
                    val.to_string(),
                    LoweredValue::js_value(val.to_string()),
                    None,
                    "direct.double",
                ),
                _ => {
                    let raw = ctx
                        .block()
                        .call(DOUBLE, "js_native_abi_check_f64", &[(DOUBLE, val)]);
                    (
                        raw.clone(),
                        LoweredValue::f64(raw),
                        Some(("js_native_abi_check_f64", "number")),
                        "f64.checked_number",
                    )
                }
            };
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                guard,
                note,
            );
            lowered.push(raw);
            arg_types.push(DOUBLE);
        }
        NativeAbiType::String => {
            let blk = ctx.block();
            let raw_ptr = blk.call(I64, "js_native_abi_check_string_ptr", &[(DOUBLE, val)]);
            let ptr_val = blk.inttoptr(I64, &raw_ptr);
            let native = LoweredValue::native_handle(raw_ptr);
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_string_ptr", "string")),
                "string.checked_pointer",
            );
            lowered.push(ptr_val);
            arg_types.push(PTR);
        }
        NativeAbiType::Json => {
            // #5626: the JS argument (object/array/primitive) is serialized to
            // a JSON string at the call site, then passed through the same
            // single `string` ABI slot as `NativeAbiType::String`. The native
            // side receives a `*const StringHeader` and `serde_json`-decodes it.
            // `type_hint` 0 == TYPE_UNKNOWN (let the runtime classify the value).
            let blk = ctx.block();
            let raw_ptr = blk.call(I64, "js_json_stringify", &[(DOUBLE, val), (I32, "0")]);
            let ptr_val = blk.inttoptr(I64, &raw_ptr);
            let native = LoweredValue::native_handle(raw_ptr);
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_json_stringify", "json_serialized_string")),
                "json.stringified_pointer",
            );
            lowered.push(ptr_val);
            arg_types.push(PTR);
        }
        NativeAbiType::Bool => {
            let raw = ctx.block().call(I32, "js_is_truthy", &[(DOUBLE, val)]);
            let native = LoweredValue::i32(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_is_truthy", "truthiness_coercion")),
                "bool.truthy_i32",
            );
            lowered.push(raw);
            arg_types.push(I32);
        }
        NativeAbiType::I32 => {
            let raw = ctx
                .block()
                .call(I32, "js_native_abi_check_i32", &[(DOUBLE, val)]);
            let native = LoweredValue::i32(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_i32", "int32_range")),
                "i32.checked",
            );
            lowered.push(raw);
            arg_types.push(I32);
        }
        NativeAbiType::I64 | NativeAbiType::I64String => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_i64", &[(DOUBLE, val)]);
            let native = LoweredValue::i64(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_i64", "safe_integer_i64")),
                "i64.checked",
            );
            lowered.push(raw);
            arg_types.push(I64);
        }
        NativeAbiType::U32 | NativeAbiType::BufferLen => {
            let raw = ctx
                .block()
                .call(I32, "js_native_abi_check_u32", &[(DOUBLE, val)]);
            let native = if matches!(descriptor, NativeAbiType::BufferLen) {
                LoweredValue::buffer_len(raw.clone())
            } else {
                LoweredValue::u32(raw.clone())
            };
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_u32", "uint32_range")),
                "u32.checked",
            );
            lowered.push(raw);
            arg_types.push(I32);
        }
        NativeAbiType::U64 | NativeAbiType::USize => {
            let helper = if matches!(descriptor, NativeAbiType::USize) {
                "js_native_abi_check_usize"
            } else {
                "js_native_abi_check_u64"
            };
            let raw = ctx.block().call(I64, helper, &[(DOUBLE, val)]);
            let native = if matches!(descriptor, NativeAbiType::USize) {
                LoweredValue::usize(raw.clone())
            } else {
                LoweredValue::u64(raw.clone())
            };
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some((helper, "safe_unsigned_integer")),
                "u64.checked",
            );
            lowered.push(raw);
            arg_types.push(I64);
        }
        NativeAbiType::F32 => {
            let raw = ctx
                .block()
                .call(F32, "js_native_abi_check_f32", &[(DOUBLE, val)]);
            let native = LoweredValue::f32(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_f32", "number_f32_range")),
                "f32.checked",
            );
            lowered.push(raw);
            arg_types.push(F32);
        }
        NativeAbiType::Ptr => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_ptr", &[(DOUBLE, val)]);
            let native = LoweredValue::native_handle(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_ptr", "pointer_payload")),
                "ptr.checked_unbox",
            );
            lowered.push(raw);
            arg_types.push(I64);
        }
        NativeAbiType::Handle(handle) => {
            lower_native_handle_param(
                ctx,
                descriptor,
                handle,
                js_argument_index,
                abi_slot_index,
                val,
                lowered,
                arg_types,
            );
        }
        NativeAbiType::Promise(_) => {
            let raw = ctx
                .block()
                .call(I64, "js_native_abi_check_promise", &[(DOUBLE, val)]);
            let native = LoweredValue::promise_boundary(raw.clone());
            record_native_abi_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                &native,
                Some(("js_native_abi_check_promise", "promise_object")),
                "promise_boundary.checked_unbox",
            );
            lowered.push(raw);
            arg_types.push(I64);
        }
        NativeAbiType::BufferAndLen => {
            lower_buffer_and_len_param(
                ctx,
                descriptor,
                js_argument_index,
                abi_slot_index,
                val,
                lowered,
                arg_types,
            );
        }
        NativeAbiType::Pod(_) | NativeAbiType::PodAndCount(_) | NativeAbiType::HandleId => {
            unreachable!("POD ABI params are lowered before JSValue materialization")
        }
        NativeAbiType::Void => {
            lowered.push(val.to_string());
            arg_types.push(DOUBLE);
        }
    }
}

pub fn try_lower_extern_func_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Cross-module function call via ExternFuncRef. The HIR carries the
    // function name; we look up the source module's prefix in
    // `import_function_prefixes` (built by the CLI from hir.imports) and
    // generate `perry_fn_<source_prefix>__<name>`. The function is
    // declared in the OTHER module's compilation; here we just emit a
    // direct LLVM call to its scoped name and the system linker
    // resolves the symbol when the .o files are linked together.
    let Expr::ExternFuncRef {
        name,
        return_type: ext_return_type,
        ..
    } = callee
    else {
        return Ok(None);
    };
    // Issue #5621: ergonomic camelCase native-library exports. When a
    // `perry.nativeLibrary` package exposes a spec-faithful camelCase
    // binding (`requestAdapter`) over a snake_case `js_<pkg>_*` FFI symbol
    // (`js_webgpu_request_adapter`), the CLI driver records the
    // binding → symbol mapping in `ffi_aliases`. Rewrite `name` to the
    // manifest symbol up front so every `ffi_signatures` lookup below hits
    // and codegen emits the call (and `declare`) against the real FFI
    // symbol — instead of routing through the package's throwing TS
    // wrapper body or a bare extern to the alias. Exact-match raw exports
    // (`@perryts/storekit`) carry no alias and pass through unchanged.
    let aliased_symbol = ctx.ffi_aliases.get(name).cloned();
    let name: &String = aliased_symbol.as_ref().unwrap_or(name);
    // Issue #1317: when `name` is bound to a named import from a Node
    // submodule Perry recognizes but doesn't yet back with a real impl
    // (`node:timers/promises`, `node:readline/promises`,
    // `node:stream/promises`, `node:stream/consumers`, `node:sys`), the
    // shadowing import collides with global names like `setTimeout`/
    // `setInterval`/`setImmediate`. Without this guard, e.g.
    // `import { setTimeout } from "node:timers/promises";
    //  await setTimeout(1, { a: 1 }, { ref: false })`
    // would lower to the global `setTimeout(fn, delay, ...args)` fast
    // path below — handing `1` to `js_set_timeout_callback_args` as if
    // it were a callback handle and segfaulting on the next dispatch.
    // Skip the fast-path table so the call falls through to the regular
    // closure-dispatch lowering, which invokes the named-import
    // singleton's thunk (raising the "not yet implemented" error).
    if ctx.import_function_node_submodule.contains_key(name) {
        return Ok(None);
    }
    match name.as_str() {
        // #1671: `setTimeout(fn)` with no explicit delay. Node treats a
        // missing/undefined delay as 0 (fires on the next timer tick).
        // Without this arm a 1-arg `setTimeout` falls through to the
        // catch-all below, which emits a bare LLVM call to `@setTimeout`
        // and the linker fails with `Undefined symbols: _setTimeout`
        // (hit by hono/jsx's `hooks/index.js`, which schedules a re-render
        // via `setTimeout(() => { … })`). Route it to the same runtime
        // entry as the 2-arg form with a zero delay.
        "setTimeout" if args.len() == 1 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            // #2013 — validate the callback type before unboxing the
            // pointer. `js_timer_validate_callback` throws
            // ERR_INVALID_ARG_TYPE for any non-callable value and
            // returns the raw closure pointer otherwise; the second
            // arg `0` is the type-name index for "setTimeout".
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let zero = double_literal(0.0);
            let id = blk.call(
                I64,
                "js_set_timeout_callback",
                &[(I64, &cb_handle), (DOUBLE, &zero)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setTimeout" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_timeout_callback",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setImmediate" if !args.is_empty() => {
            let cb_box = lower_expr(ctx, &args[0])?;
            if args.len() == 1 {
                let blk = ctx.block();
                let two_idx = "2";
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, two_idx)],
                );
                let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }

            let n = args.len() - 1;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().skip(1).enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let two_idx = "2";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, two_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_immediate_callback_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        // Refs #665: `setTimeout(fn, delay, ...args)` — JS spec forwards
        // the trailing args to `fn` when the timer fires. Pack them into
        // a stack buffer of doubles and hand off to the varargs runtime
        // entry. Used by Promise-executor patterns like
        // `setTimeout(resolve, delay, res)` (rate-limiter-flexible's
        // `RateLimiterMemory.consume` is the discovering call site).
        "setTimeout" if args.len() >= 3 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let n = args.len() - 2;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().skip(2).enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_timeout_callback_args",
                &[
                    (I64, &cb_handle),
                    (DOUBLE, &delay_box),
                    (crate::types::PTR, &ptr_reg),
                    (I32, &n.to_string()),
                ],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setInterval" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let one_idx = "1";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, one_idx)],
            );
            let id = blk.call(
                I64,
                "setInterval",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setInterval" if args.len() >= 3 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let n = args.len() - 2;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().skip(2).enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let one_idx = "1";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, one_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_interval_callback_args",
                &[
                    (I64, &cb_handle),
                    (DOUBLE, &delay_box),
                    (crate::types::PTR, &ptr_reg),
                    (I32, &n.to_string()),
                ],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "clearTimeout" if args.len() == 1 => {
            // Pass the raw NaN-boxed arg so the runtime accepts both the
            // handle and its primitive numeric id (`clearTimeout(+t)`, #1213).
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_timeout_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "clearInterval" if args.len() == 1 => {
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_interval_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "clearImmediate" if args.len() == 1 => {
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_immediate_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "gc" => {
            ctx.block().call_void("js_gc_collect", &[]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "getAppVersion" if args.is_empty() => {
            let version = ctx.app_metadata.version.clone();
            let idx = ctx.strings.intern(&version);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            return Ok(Some(ctx.block().load(DOUBLE, &handle_global)));
        }
        "getAppBuildNumber" if args.is_empty() => {
            return Ok(Some(double_literal(ctx.app_metadata.build_number as f64)));
        }
        "getBundleId" if args.is_empty() => {
            let bundle_id = ctx.app_metadata.bundle_id.clone();
            let idx = ctx.strings.intern(&bundle_id);
            let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
            return Ok(Some(ctx.block().load(DOUBLE, &handle_global)));
        }
        // JSX runtime calls: `jsx(type, props)` and `jsxs(type, props)`.
        // The HIR lowers <div>…</div> to ExternFuncRef { name: "jsx" } and
        // <div><a/><b/></div> (multiple children) to "jsxs".  The first arg
        // is the element type (a string literal for HTML tags, or a NaN-boxed
        // function/class reference for components); the second arg is a
        // NaN-boxed props object (or TAG_NULL).  Both are passed as DOUBLE so
        // the ABI is uniform regardless of whether the type arg is a string or
        // a component reference — avoiding the PTR vs DOUBLE divergence that
        // the generic ExternFuncRef path would otherwise produce for string
        // literals.  The runtime adapter `js_jsx`/`js_jsxs` handles Perry's
        // built-in TSX path: intrinsic HTML-style tags render to boxed HTML,
        // fragments render their children, and function components are called
        // with the props object.
        //
        // perry/tui JSX intrinsic rewriter (#689). When the first arg
        // is `ExternFuncRef { name: "__perry_jsx_intrinsic::<mod>::<method>__" }`
        // (the HIR's marker for `<Box>` / `<Text>` resolved against a
        // native module — see `crates/perry-hir/src/jsx.rs`), bypass
        // the runtime `js_jsx` adapter entirely and route the call
        // through `lower_native_method_call` so the JSX form lowers
        // to the same widget builder the function-call form would.
        // Today this covers Box + Text from `perry/tui`; other
        // intrinsics (Spacer / Input / Spinner / List / Select /
        // ProgressBar / Table / Tabs / TextArea) are listed as follow-up
        // scope in #689 and continue to fall through to `js_jsx`; the runtime
        // returns `undefined` for those unrecognised intrinsic sentinels until
        // the rewriter is extended.
        "jsx" | "jsxs" => {
            if let Some(call) = try_rewrite_perry_tui_jsx_intrinsic(ctx, name == "jsxs", args)? {
                return Ok(Some(call));
            }
            let runtime_fn = if name == "jsx" { "js_jsx" } else { "js_jsxs" };
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            return Ok(Some(ctx.block().call(DOUBLE, runtime_fn, &arg_slices)));
        }
        _ => {}
    }
    // Issue #841: direct call against a named import from one of the
    // five recognized Node submodules (`import { pipeline } from
    // "node:stream/promises"; pipeline()`). The HIR registers
    // `pipeline` as an imported func; without this routing the
    // catch-all below tries to emit a bare LLVM call to `@pipeline`
    // and the linker errors with `Undefined symbols: _pipeline`.
    //
    // Route to the value-form singleton getter and then dispatch
    // through the closure-call machinery — the singleton's thunk
    // throws an "is not yet implemented" Error. Real impls are
    // tracked separately under #793.
    if let Some((submod_key, exported_name)) = ctx.import_function_node_submodule.get(name).cloned()
    {
        let mut lowered_args = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(crate::expr::lower_expr(ctx, a)?);
        }
        let submod_label = crate::expr::emit_string_literal_global(ctx, &submod_key);
        let name_label = crate::expr::emit_string_literal_global(ctx, &exported_name);
        let submod_len = submod_key.len();
        let name_len = exported_name.len();
        ctx.pending_declares.push((
            "js_node_submodule_export_as_function".to_string(),
            DOUBLE,
            vec![PTR, I32, PTR, I32],
        ));
        let blk = ctx.block();
        let closure_value = blk.call(
            DOUBLE,
            "js_node_submodule_export_as_function",
            &[
                (PTR, &submod_label),
                (I32, &submod_len.to_string()),
                (PTR, &name_label),
                (I32, &name_len.to_string()),
            ],
        );
        // Drive through the closure-call machinery and preserve the user's
        // arguments. The original #841 surface-only fix discarded args because
        // all known submodule thunks threw/no-op'd. `node:diagnostics_channel`
        // now implements real `channel(name)`, `subscribe(name, cb)`, etc., so
        // named imports must receive their actual argument list.
        let blk = ctx.block();
        let closure_bits = blk.bitcast_double_to_i64(&closure_value);
        let closure_handle = blk.and(I64, &closure_bits, POINTER_MASK_I64);
        let call_name = format!("js_closure_call{}", lowered_args.len().min(16));
        let mut decl_types = vec![I64];
        decl_types.extend(std::iter::repeat_n(DOUBLE, lowered_args.len().min(16)));
        ctx.pending_declares
            .push((call_name.clone(), DOUBLE, decl_types));
        let mut call_args: Vec<(crate::types::LlvmType, String)> = vec![(I64, closure_handle)];
        for arg in lowered_args.into_iter().take(16) {
            call_args.push((DOUBLE, arg));
        }
        let arg_refs: Vec<(crate::types::LlvmType, &str)> =
            call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
        return Ok(Some(ctx.block().call(DOUBLE, &call_name, &arg_refs)));
    }
    // perry/system dispatch: map JS names (isDarkMode, getDeviceIdiom,
    // keychainSave, etc.) to their perry_system_* / perry_* C symbols.
    // These arrive as ExternFuncRef because perry/system imports aren't
    // lowered to NativeMethodCall in the HIR.
    if let Some(sig) = perry_system_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // perry/updater dispatch: same shape as perry/system. Imports from
    // `perry/updater` arrive as ExternFuncRef; route by name to the
    // perry_updater_* runtime symbols in `perry-updater`.
    if let Some(sig) = perry_updater_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // perry/background dispatch (issue #538): registerTask / schedule /
    // cancel from `perry/background`. Backed by perry_background_* in
    // libperry_ui_*.a (real impls on iOS + Android, no-op stubs
    // elsewhere). Same calling convention as perry/system.
    if let Some(sig) = perry_background_table_lookup(name) {
        return Ok(Some(lower_perry_ui_table_call(ctx, sig, args)?));
    }
    // Built-in runtime extern functions (`js_weakmap_set`,
    // `js_regexp_exec`, etc.) that start with `js_` are resolved
    // directly against the runtime library — bypass the import-
    // map lookup and emit a direct LLVM call with an f64/f64 ABI.
    // (The declarations are added centrally in runtime_decls.rs.)
    //
    // External `perry.nativeLibrary` packages commonly export their
    // symbols with the same `js_*` prefix. If the manifest declares
    // this name, let the native-library path below emit the call and
    // declaration from `ffi_signatures` instead of treating it as a
    // runtime builtin.
    if name.starts_with("js_") && !ctx.ffi_signatures.contains_key(name) {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        return Ok(Some(ctx.block().call(DOUBLE, name, &arg_slices)));
    }
    // Issue #692: default-import call against an unresolved module.
    // `import sanitizeHtml from "sanitize-html"` (when sanitize-html
    // didn't resolve to a NativeCompiled module / perry-stdlib
    // binding) lowers `sanitizeHtml(x)` to `Call { callee:
    // ExternFuncRef { name: "default" } }` — the HIR's
    // register_imported_func uses the literal `"default"` as the
    // exported-name marker for default imports (lower.rs:3727).
    // Without a source_prefix, the catch-all below emitted a direct
    // LLVM call to the bare symbol `default`, and the system linker
    // failed with `undefined reference to 'default'`. Route to the
    // runtime stub instead: lower args for side effects (so closure
    // collection / string interning still happens), then call
    // `js_unresolved_default_call` which returns NaN-boxed undefined
    // and prints a one-shot diagnostic at runtime. The program now
    // links; the user gets a clear runtime signal rather than a
    // cryptic linker error.
    if name == "default" && !ctx.import_function_prefixes.contains_key(name) {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_unresolved_default_call",
            &[],
        )));
    }
    // Native library functions (bloom_draw_rect, bloom_init_window,
    // etc.) that aren't in the import map — emit a direct call so
    // the linker resolves them against the linked native .a library.
    // Previously these were silently dropped (returned 0.0), which
    // caused Bloom Engine games to render blank windows.
    //
    // #1110 (follow-up to #1085): a symbol declared in the source
    // package's `perry.nativeLibrary.functions` manifest is always
    // resolved against the linked static library, never via the
    // `perry_fn_<src>__<name>` wrapper (the source `.ts` is ambient
    // and emits no wrapper). Force the FFI-manifest path whenever
    // `ffi_signatures` knows the name, even if some other code path
    // accidentally registered an entry in `import_function_prefixes`
    // (re-export chains, namespace re-exports, etc. — anything that
    // doesn't go through the #1085 per-specifier skip ends up there).
    let force_ffi_path = ctx.ffi_signatures.contains_key(name);
    let prefix_lookup = if force_ffi_path {
        None
    } else {
        ctx.import_function_prefixes.get(name).cloned()
    };
    let Some(source_prefix) = prefix_lookup else {
        // Determine per-arg types: string args need to be unboxed
        // to raw `*const u8` pointers and passed as `ptr` so the
        // ARM64 ABI puts them in x-registers (not d-registers).
        // Without this, bloom_draw_text(text, x, y, ...) passes
        // the NaN-boxed string in d0 but the native function reads
        // x0 as a *const u8 → SIGSEGV.
        // Extern C functions use the platform C ABI. Perry stores
        // all values as `double`, but native C/Rust functions may
        // take a mix of i64 (pointers/handles) and f64 (floats).
        //
        // The LLVM IR declaration type determines ARM64 register
        // placement: i64 → x-register, double → d-register.
        //
        // When the FFI manifest (`ffi_signatures`) declares a param
        // as `"i64"`, lower it via `fptosi` to put the value in an
        // x-register. This is required for handle-typed params like
        // `view: *mut EditorView` — without it the C ABI reads a
        // garbage value out of x0/x1 since Perry put the handle in
        // d-registers.
        let manifest_sig = ctx.ffi_signatures.get(name).cloned();
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        let mut arg_types: Vec<crate::types::LlvmType> = Vec::with_capacity(args.len());
        let mut abi_slot_index = 0usize;
        for (idx, a) in args.iter().enumerate() {
            let manifest_kind: Option<&NativeAbiType> =
                manifest_sig.as_ref().and_then(|(p, _)| p.get(idx));
            if let Some(descriptor @ NativeAbiType::Pod(pod)) = manifest_kind {
                lower_manifest_pod_param(
                    ctx,
                    descriptor,
                    pod,
                    idx,
                    abi_slot_index,
                    a,
                    &mut lowered,
                    &mut arg_types,
                )?;
                abi_slot_index += descriptor.abi_slot_count();
                continue;
            }
            if let Some(descriptor @ NativeAbiType::PodAndCount(pod)) = manifest_kind {
                lower_manifest_pod_view_param(
                    ctx,
                    descriptor,
                    pod,
                    idx,
                    abi_slot_index,
                    a,
                    &mut lowered,
                    &mut arg_types,
                )?;
                abi_slot_index += descriptor.abi_slot_count();
                continue;
            }
            let val = lower_expr(ctx, a)?;
            if let Some(descriptor) = manifest_kind {
                lower_manifest_param(
                    ctx,
                    descriptor,
                    idx,
                    abi_slot_index,
                    &val,
                    &mut lowered,
                    &mut arg_types,
                );
                abi_slot_index += descriptor.abi_slot_count();
            } else if is_string_expr(ctx, a) {
                let blk = ctx.block();
                let raw_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &val)]);
                let ptr_val = blk.inttoptr(I64, &raw_ptr);
                lowered.push(ptr_val);
                arg_types.push(PTR);
                abi_slot_index += 1;
            } else if is_array_expr(ctx, a) {
                let blk = ctx.block();
                let bits = blk.bitcast_double_to_i64(&val);
                let header_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                let header_ptr = blk.inttoptr(I64, &header_handle);
                // Skip 8-byte ArrayHeader (u32 length + u32 capacity)
                // to reach the inline f64 data.
                let eight = "8".to_string();
                let data_ptr = blk.gep(I8, &header_ptr, &[(I64, &eight)]);
                lowered.push(data_ptr);
                arg_types.push(PTR);
                abi_slot_index += 1;
            } else {
                lowered.push(val);
                arg_types.push(DOUBLE);
                abi_slot_index += 1;
            }
        }

        // Issue #5812 item 4 — pad omitted trailing manifest params with
        // defined null/zero sentinels so the emitted call/declaration has
        // the same ABI slot count the native function reads (otherwise it
        // reads an uninitialized register — the win64 `read_string` crash).
        // See `omitted_native_params::pad_omitted_native_params`.
        if let Some((manifest_params, _)) = manifest_sig.as_ref() {
            super::omitted_native_params::pad_omitted_native_params(
                ctx,
                manifest_params,
                args.len(),
                abi_slot_index,
                &mut lowered,
                &mut arg_types,
            )?;
        }

        let arg_slices: Vec<(crate::types::LlvmType, &str)> = arg_types
            .iter()
            .zip(lowered.iter())
            .map(|(t, v)| (*t, v.as_str()))
            .collect();
        // Determine return type.
        //
        // Manifest `returns` field takes precedence over HIR heuristics:
        //
        //   "string" / "ptr"  → PTR return (*const u8 / *const StringHeader);
        //                       ptrtoint + NaN-box STRING_TAG. Use when the
        //                       Rust function is declared `-> *const u8`.
        //   "i64_str"         → I64 return (raw integer that IS a *StringHeader
        //                       address). NaN-box directly with STRING_TAG; no
        //                       sitofp. Use when the Rust function is declared
        //                       `-> i64` but the value is a string pointer.
        //   "i64"             → I64 return; sitofp → JS number. Use for opaque
        //                       handles / integers (`*mut View`, counts, etc.).
        //   "u32" / "u64" /
        //   "usize" / "f32"  → native scalar ABI return; explicitly
        //                       materialized to a JS number at this boundary.
        //   "buffer_len"      → u32 BufferHeader.length return.
        //   "handle"          → I64 opaque handle; NaN-box via POINTER_TAG.
        //   "promise"         → I64 Promise handle; NaN-box via POINTER_TAG
        //                       with a promise-boundary transition record.
        //   "void"            → no return value.
        //   (absent)          → fall back to HIR ExternFuncRef.return_type and
        //                       the name-pattern heuristic below.
        let has_string_args = arg_types.contains(&PTR);
        let manifest_ret: Option<&NativeAbiType> = manifest_sig.as_ref().map(|(_, r)| r);
        // "i64_str": explicit opt-in for FFI functions that return a raw i64
        // which is actually a *StringHeader pointer — distinct from "string"
        // (which declares the function as returning `ptr` in LLVM IR) and
        // from "i64" (which sitofp-converts the integer to a JS number).
        let returns_i64_str = matches!(manifest_ret, Some(NativeAbiType::I64String));
        let returns_string = matches!(
            manifest_ret,
            Some(NativeAbiType::String | NativeAbiType::Ptr)
        ) || matches!(ext_return_type, HirType::String)
            || (manifest_ret.is_none()
                && has_string_args
                && (name.contains("read_file")
                    || name.contains("clipboard_text")
                    || name.contains("file_dialog")));
        let returns_void = matches!(manifest_ret, Some(NativeAbiType::Void))
            || (manifest_ret.is_none() && matches!(ext_return_type, HirType::Void));
        let returns_i32 = matches!(manifest_ret, Some(NativeAbiType::I32 | NativeAbiType::Bool));
        let returns_i64 = matches!(manifest_ret, Some(NativeAbiType::I64));
        let returns_u32 = matches!(manifest_ret, Some(NativeAbiType::U32));
        let returns_u64 = matches!(manifest_ret, Some(NativeAbiType::U64));
        let returns_usize = matches!(manifest_ret, Some(NativeAbiType::USize));
        let returns_f32 = matches!(manifest_ret, Some(NativeAbiType::F32));
        let returns_buffer_len = matches!(manifest_ret, Some(NativeAbiType::BufferLen));
        let returns_handle = matches!(manifest_ret, Some(NativeAbiType::Handle(_)));
        let returns_promise = matches!(manifest_ret, Some(NativeAbiType::Promise(_)));
        if returns_void {
            ctx.pending_declares
                .push((name.clone(), crate::types::VOID, arg_types));
            ctx.block().call_void(name, &arg_slices);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        } else if returns_i64_str {
            // C function returns a raw i64 that is a *StringHeader address.
            // Declare as I64 (matching the C ABI — x0 on ARM64, rax on
            // x86_64), call it, and NaN-box the result directly with
            // STRING_TAG. No sitofp (which would corrupt the pointer
            // bits) and no ptrtoint (already an integer, not a ptr).
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            if let Some(descriptor) = manifest_ret {
                let lowered = LoweredValue::native_handle(raw.clone());
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            let blk = ctx.block();
            return Ok(Some(nanbox_string_inline(blk, &raw)));
        } else if returns_string {
            ctx.pending_declares.push((name.clone(), PTR, arg_types));
            let raw_ptr = ctx.block().call(PTR, name, &arg_slices);
            // Convert raw *const u8 back to a NaN-boxed string.
            let blk = ctx.block();
            let ptr_i64 = blk.ptrtoint(&raw_ptr, I64);
            if let Some(descriptor) = manifest_ret {
                let lowered = LoweredValue::native_handle(ptr_i64.clone());
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            let boxed = nanbox_string_inline(ctx.block(), &ptr_i64);
            return Ok(Some(boxed));
        } else if returns_i32 {
            ctx.pending_declares.push((name.clone(), I32, arg_types));
            let raw = ctx.block().call(I32, name, &arg_slices);
            let lowered = LoweredValue::i32(raw.clone());
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            if matches!(manifest_ret, Some(NativeAbiType::Bool)) {
                let is_true = ctx.block().icmp_ne(I32, &raw, "0");
                let bits = ctx.block().select(
                    I1,
                    &is_true,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                return Ok(Some(ctx.block().bitcast_i64_to_double(&bits)));
            }
            return Ok(Some(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_i64 {
            // C function returns i64 in x0 (e.g. `*mut View`
            // handles). Declare as I64; the value comes back as a
            // raw integer. Convert via `sitofp` so callers see a
            // normal JS number; subsequent FFI calls that pass it
            // back as an i64 param will truncate via `fptosi`.
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let lowered = LoweredValue::i64(raw.clone());
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_u32 || returns_buffer_len {
            ctx.pending_declares.push((name.clone(), I32, arg_types));
            let raw = ctx.block().call(I32, name, &arg_slices);
            let lowered = if returns_buffer_len {
                LoweredValue::buffer_len(raw.clone())
            } else {
                LoweredValue::u32(raw.clone())
            };
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_u64 || returns_usize {
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let lowered = if returns_usize {
                LoweredValue::usize(raw.clone())
            } else {
                LoweredValue::u64(raw.clone())
            };
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_f32 {
            ctx.pending_declares.push((name.clone(), F32, arg_types));
            let raw = ctx.block().call(F32, name, &arg_slices);
            let lowered = LoweredValue::f32(raw.clone());
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(materialize_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_handle {
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let lowered = LoweredValue::native_handle(raw.clone());
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            if let Some(NativeAbiType::Handle(handle)) = manifest_ret {
                return Ok(Some(materialize_native_handle_return(ctx, &raw, handle)));
            }
            return Ok(Some(materialize_native_handle_to_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else if returns_promise {
            ctx.pending_declares.push((name.clone(), I64, arg_types));
            let raw = ctx.block().call(I64, name, &arg_slices);
            let lowered = LoweredValue::promise_boundary(raw.clone());
            if let Some(descriptor) = manifest_ret {
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(materialize_promise_boundary_to_js_value(
                ctx,
                lowered,
                MaterializationReason::ReturnAbi,
            )));
        } else {
            // Native library functions (Bloom, etc.) return f64 in
            // the d0 register — they use the Perry double-based ABI,
            // not a C integer ABI. Declare as DOUBLE and use the
            // return value directly (no sitofp needed).
            ctx.pending_declares.push((name.clone(), DOUBLE, arg_types));
            let raw = ctx.block().call(DOUBLE, name, &arg_slices);
            if let Some(descriptor) = manifest_ret {
                let lowered = if matches!(descriptor, NativeAbiType::JsValue) {
                    LoweredValue::js_value(raw.clone())
                } else {
                    LoweredValue::f64(raw.clone())
                };
                record_native_abi_return(ctx, descriptor, &lowered, name);
            }
            return Ok(Some(raw));
        }
    };
    // Issue #678 followup: if the consumer-visible name resolves to a
    // V8-fallback module, there is no `perry_fn_<src>__<name>` symbol
    // (the origin was demoted to V8 and never emitted a native one).
    // Route the call through the runtime V8 bridge.
    if let Some(specifier) = ctx.import_function_v8_specifiers.get(name).cloned() {
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, name, &lowered,
        )));
    }
    // Issue #678: re-export rename (`export { default as render } from
    // './render.js'`) means the origin module emits the symbol under
    // the *origin* name (`default`), not the consumer-visible name
    // (`render`). Look up the actual origin suffix before forming the
    // extern.
    let origin_suffix = crate::expr::import_origin_suffix(ctx.import_function_origin_names, name);
    let fname = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
    // Issue #493 followup: when the imported binding is a VARIABLE
    // holding a closure value (e.g. `var mergePath = (b, s, ...r) => …`
    // exported from another module), `perry_fn_<src>__<name>` is the
    // ZERO-arg GETTER that returns the closure pointer (set up at
    // crates/perry/src/commands/compile.rs's `imported_vars` registration
    // and emitted by the source module's value-getter loop). Calling
    // the getter with N args puts garbage in the registers and discards
    // the actual call — `mergePath('/', '/foo')` returned the closure
    // itself instead of the merged path. The fix is to call the getter
    // first, treating its return as a closure value, then dispatch
    // through `js_closure_callN`. The runtime's closure-rest registry
    // (issue #493) bundles trailing args correctly when the closure
    // has `...rest`. Before this branch, ExternFuncRef-as-call for
    // imported-VAR bindings silently broke any code path that imports
    // an arrow-bound exported value (hono's `mergePath` from utils/url.js,
    // any `export const foo = () => …` cross-module use).
    if ctx.imported_vars.contains(name) {
        ctx.pending_declares.push((fname.clone(), DOUBLE, vec![]));
        let closure_box = ctx.block().call(DOUBLE, &fname, &[]);
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        if lowered_args.len() > 16 {
            anyhow::bail!(
                "perry-codegen Phase D.1: closure call with {} args (max 16)",
                lowered_args.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &closure_box);
        let runtime_fn = format!("js_closure_call{}", lowered_args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
    }
    // Record the cross-module call so the caller can add a `declare`
    // line for it after the &mut LlFunction borrow is released. The
    // module dedupes by name, so duplicates are harmless. Without
    // this, clang errors with `use of undefined value @perry_fn_*`
    // for any cross-module call hidden inside a closure body, try
    // block, switch, etc. — the old pre-walker missed those shapes.
    //
    // Determine the actual param count from the imported function
    // signature. Calls that pass fewer args than the function declares
    // (because the trailing params have defaults) need to be padded
    // with `undefined` so the function body sees defined values for
    // the missing args (and can apply its defaults). Without this,
    // the d-registers for the missing params hold stale data and
    // the function reads garbage (e.g. alpha = -3e-5 instead of 1).
    let declared_count = ctx
        .imported_func_param_counts
        .get(name)
        .copied()
        .unwrap_or(args.len());
    let has_rest = ctx.imported_func_has_rest.contains(name);
    // #1816: the trailing rest is the HIR-synthesized `arguments` param (the
    // callee's body references `arguments`). Unlike a real `...rest` (which
    // binds only the trailing args), `arguments` must reflect ALL passed args,
    // so bundle every arg — matching the same-module path in
    // `func_ref.rs` and JS `arguments.length` semantics. effect's `pipe`/`dual`
    // (used everywhere) hit this; without it cross-package calls returned
    // undefined.
    let has_synthetic_args = ctx.imported_func_synthetic_arguments.contains(name);
    // Issue #608: when the imported callee declares a trailing
    // `...rest` parameter, the LLVM signature has exactly
    // `declared_count` doubles (rest counts as one slot — a
    // NaN-boxed array pointer). Bundle every arg at and beyond the
    // rest position into a single `js_array_alloc` array; that
    // array is what the callee's rest binding sees. Without this
    // bundling, `tag\`hello ${x}\`` lowers to `tag([…], x)` and
    // the cross-module callee reads `params` as `x` directly
    // (`undefined` when no interp args, or the raw arg value
    // when one).
    let target_arity = if has_rest {
        declared_count.max(1)
    } else {
        declared_count.max(args.len())
    };
    let param_types: Vec<crate::types::LlvmType> =
        std::iter::repeat_n(DOUBLE, target_arity).collect();
    ctx.pending_declares
        .push((fname.clone(), DOUBLE, param_types));
    let mut lowered: Vec<String> = Vec::with_capacity(target_arity);
    if has_rest {
        // Fixed (non-rest) params: pass through.
        let fixed_count = declared_count.saturating_sub(1);
        for a in args.iter().take(fixed_count) {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad fixed params if the caller passed too few.
        let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < fixed_count {
            lowered.push(undefined_lit.clone());
        }
        // Materialize the rest array (always — even when zero
        // trailing args, the callee's rest binding must be `[]`).
        // #1816: for a synthetic `arguments` param, bundle ALL args (from 0),
        // not just the trailing ones, so `arguments.length` is correct.
        let bundle_from = if has_synthetic_args { 0 } else { fixed_count };
        let rest_count = args.len().saturating_sub(bundle_from);
        let cap = (rest_count as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for a in args.iter().skip(bundle_from) {
            let v = lower_expr(ctx, a)?;
            let blk = ctx.block();
            current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
        }
        if has_synthetic_args {
            current = ctx
                .block()
                .call(I64, "js_array_mark_arguments_object", &[(I64, &current)]);
        }
        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(rest_box);
    } else {
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad with TAG_UNDEFINED for the missing trailing args.
        let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < target_arity {
            lowered.push(undefined_lit.clone());
        }
    }
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
    Ok(Some(ctx.block().call(DOUBLE, &fname, &arg_slices)))
}
