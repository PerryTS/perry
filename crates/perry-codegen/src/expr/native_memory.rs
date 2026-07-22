use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use crate::native_value::{
    AliasState, BoundsProof, BoundsState, BufferAccessMode, BufferElem, BufferIndexUnit,
    BufferViewSlot, ExpectedNativeRep, LoweredValue, MaterializationReason,
};
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

use super::{
    attach_native_owned_view_fact, buffer_access_materialization_reason, buffer_view_lowered_value,
    effective_alias_state_for_access, lower_expr, lower_expr_native, unbox_to_i64, FnCtx,
};

#[derive(Clone)]
struct ProvenView {
    local_id: u32,
    slot: BufferViewSlot,
}

struct ViewBytes {
    data_ptr: String,
    len_i32: String,
    byte_len_i64: String,
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::NativeMemoryFillU32 { view, value } => lower_fill_u32(ctx, view, value),
        Expr::NativeMemoryCopy { dst, src } => lower_copy(ctx, dst, src),
        _ => unreachable!("native_memory::lower only accepts NativeMemory expressions"),
    }
}

fn lower_fill_u32(ctx: &mut FnCtx<'_>, view: &Expr, value: &Expr) -> Result<String> {
    let Some(proven) = proven_view(ctx, view, Some(BufferElem::U32)) else {
        return lower_fill_u32_fallback(ctx, view, value);
    };
    if !fill_value_has_no_observable_side_effects(value) {
        return lower_fill_u32_fallback(ctx, view, value);
    }

    let bytes = load_view_bytes(ctx, &proven.slot);
    if is_zero_literal(value) {
        ctx.block().call_void(
            "llvm.memset.p0.i64",
            &[
                (PTR, &bytes.data_ptr),
                (I8, "0"),
                (I64, &bytes.byte_len_i64),
                (I1, "false"),
            ],
        );
        record_bulk_view(
            ctx,
            "NativeMemoryFillU32",
            proven.local_id,
            "NativeMemoryFillU32.memset_zero",
            &proven.slot,
            &bytes.data_ptr,
            &bytes.len_i32,
        );
    } else {
        let stored = lower_expr_native(ctx, value, ExpectedNativeRep::U32)?;
        emit_u32_fill_loop(ctx, &bytes.data_ptr, &bytes.len_i32, &stored.value);
        record_bulk_view(
            ctx,
            "NativeMemoryFillU32",
            proven.local_id,
            "NativeMemoryFillU32.store_loop",
            &proven.slot,
            &bytes.data_ptr,
            &bytes.len_i32,
        );
    }

    Ok(undefined_value())
}

fn lower_copy(ctx: &mut FnCtx<'_>, dst: &Expr, src: &Expr) -> Result<String> {
    let Some(dst_proven) = proven_view(ctx, dst, None) else {
        return lower_copy_fallback(ctx, dst, src);
    };
    let Some(src_proven) = proven_view(ctx, src, None) else {
        return lower_copy_fallback(ctx, dst, src);
    };

    let dst_bytes = load_view_bytes(ctx, &dst_proven.slot);
    let src_bytes = load_view_bytes(ctx, &src_proven.slot);
    let copy_len = {
        let blk = ctx.block();
        let use_dst = blk.icmp_ult(I64, &dst_bytes.byte_len_i64, &src_bytes.byte_len_i64);
        blk.select(
            I1,
            &use_dst,
            I64,
            &dst_bytes.byte_len_i64,
            &src_bytes.byte_len_i64,
        )
    };
    ctx.block().call_void(
        "llvm.memmove.p0.p0.i64",
        &[
            (PTR, &dst_bytes.data_ptr),
            (PTR, &src_bytes.data_ptr),
            (I64, &copy_len),
            (I1, "false"),
        ],
    );
    record_bulk_view(
        ctx,
        "NativeMemoryCopy",
        dst_proven.local_id,
        "NativeMemoryCopy.dst.memmove",
        &dst_proven.slot,
        &dst_bytes.data_ptr,
        &dst_bytes.len_i32,
    );
    record_bulk_view(
        ctx,
        "NativeMemoryCopy",
        src_proven.local_id,
        "NativeMemoryCopy.src.memmove",
        &src_proven.slot,
        &src_bytes.data_ptr,
        &src_bytes.len_i32,
    );

    Ok(undefined_value())
}

fn lower_fill_u32_fallback(ctx: &mut FnCtx<'_>, view: &Expr, value: &Expr) -> Result<String> {
    let view_value = lower_expr(ctx, view)?;
    let value = lower_expr(ctx, value)?;
    let handle = unbox_to_i64(ctx.block(), &view_value);
    ctx.block().call_void(
        "js_native_memory_fill_u32",
        &[(I64, &handle), (DOUBLE, &value)],
    );
    record_runtime_fallback(
        ctx,
        "NativeMemoryFillU32",
        "NativeMemoryFillU32.runtime_fallback",
        buffer_access_materialization_reason(ctx, view),
    );
    Ok(undefined_value())
}

fn lower_copy_fallback(ctx: &mut FnCtx<'_>, dst: &Expr, src: &Expr) -> Result<String> {
    let dst_value = lower_expr(ctx, dst)?;
    let src_value = lower_expr(ctx, src)?;
    let dst_handle = unbox_to_i64(ctx.block(), &dst_value);
    let src_handle = unbox_to_i64(ctx.block(), &src_value);
    ctx.block().call_void(
        "js_native_memory_copy",
        &[(I64, &dst_handle), (I64, &src_handle)],
    );
    record_runtime_fallback(
        ctx,
        "NativeMemoryCopy",
        "NativeMemoryCopy.runtime_fallback",
        buffer_access_materialization_reason(ctx, dst),
    );
    Ok(undefined_value())
}

fn proven_view(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    expected_elem: Option<BufferElem>,
) -> Option<ProvenView> {
    if ctx.disable_buffer_fast_path {
        return None;
    }
    let Expr::LocalGet(local_id) = expr else {
        return None;
    };
    if !is_native_memory_typed_view(ctx.local_types.get(local_id)) {
        return None;
    }
    let slot = ctx.buffer_view_slots.get(local_id)?.clone();
    if slot.index_unit != BufferIndexUnit::Element {
        return None;
    }
    if expected_elem.is_some_and(|expected| slot.elem != expected) {
        return None;
    }
    if let Some(native) = slot.native_owned.as_ref() {
        if !native.owner_rooted || native.disposed {
            return None;
        }
        if matches!(
            ctx.buffer_hazard_reasons.get(local_id),
            Some(
                MaterializationReason::UseAfterDispose
                    | MaterializationReason::MissingOwnerRoot
                    | MaterializationReason::EscapingUnownedPointer
            )
        ) {
            return None;
        }
    }
    Some(ProvenView {
        local_id: *local_id,
        slot,
    })
}

fn is_native_memory_typed_view(ty: Option<&HirType>) -> bool {
    matches!(
        ty,
        Some(HirType::Named(name))
            if matches!(
                name.as_str(),
                "Int8Array"
                    | "Uint8Array"
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

fn load_view_bytes(ctx: &mut FnCtx<'_>, view: &BufferViewSlot) -> ViewBytes {
    let blk = ctx.block();
    let data_ptr = blk.load(PTR, &view.data_slot);
    let len_i32 = if let Some(length_slot) = view.length_slot.as_ref() {
        blk.load(I32, length_slot)
    } else {
        let header_ptr = blk.gep(
            I8,
            &data_ptr,
            &[(I32, &view.length_offset_from_data.to_string())],
        );
        blk.load_invariant(I32, &header_ptr)
    };
    let len_i64 = blk.zext(I32, &len_i32, I64);
    let byte_len_i64 = if view.element_width_bytes == 1 {
        len_i64
    } else {
        blk.mul(I64, &len_i64, &view.element_width_bytes.to_string())
    };
    ViewBytes {
        data_ptr,
        len_i32,
        byte_len_i64,
    }
}

fn record_bulk_view(
    ctx: &mut FnCtx<'_>,
    expr_kind: &'static str,
    local_id: u32,
    consumer: &'static str,
    view: &BufferViewSlot,
    data_ptr: &str,
    len_i32: &str,
) {
    let bounds = BoundsState::Proven {
        proof: BoundsProof::ExplicitGuard,
    };
    let alias = effective_alias_state_for_access(ctx, view);
    let lowered = buffer_view_lowered_value(
        data_ptr,
        len_i32,
        view.elem.clone(),
        view.element_width_bytes,
        view.index_unit,
        view.view_byte_offset,
        view.length_offset_from_data,
        bounds.clone(),
        alias.clone(),
    );
    ctx.record_lowered_value_with_access_mode(
        expr_kind,
        Some(local_id),
        consumer,
        &lowered,
        Some(bounds),
        Some(alias),
        Some(BufferAccessMode::CheckedNative),
        None,
        false,
        false,
        Vec::new(),
    );
    attach_native_owned_view_fact(ctx, view);
}

fn record_runtime_fallback(
    ctx: &mut FnCtx<'_>,
    expr_kind: &'static str,
    consumer: &'static str,
    reason: MaterializationReason,
) {
    let lowered = LoweredValue::js_value(undefined_value());
    ctx.record_lowered_value_with_access_mode(
        expr_kind,
        None,
        consumer,
        &lowered,
        Some(BoundsState::Unknown),
        Some(AliasState::Unknown),
        Some(BufferAccessMode::DynamicFallback),
        Some(reason),
        false,
        false,
        Vec::new(),
    );
}

fn emit_u32_fill_loop(ctx: &mut FnCtx<'_>, data_ptr: &str, len_i32: &str, value_i32: &str) {
    let loop_id = ctx.next_loop_proof_scope_id();
    let index_slot = ctx.func.alloca_entry(I64);
    ctx.block().store(I64, "0", &index_slot);

    let cond_idx = ctx.new_block(&format!("native_memory_fill_cond_{}", loop_id));
    let body_idx = ctx.new_block(&format!("native_memory_fill_body_{}", loop_id));
    let done_idx = ctx.new_block(&format!("native_memory_fill_done_{}", loop_id));
    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().br(&cond_label);

    ctx.current_block = cond_idx;
    let len_i64 = ctx.block().zext(I32, len_i32, I64);
    let i = ctx.block().load(I64, &index_slot);
    let keep_going = ctx.block().icmp_ult(I64, &i, &len_i64);
    ctx.block().cond_br(&keep_going, &body_label, &done_label);

    ctx.current_block = body_idx;
    let elem_ptr = ctx.block().gep(I32, data_ptr, &[(I64, &i)]);
    // GC_STORE_AUDIT(POINTER_FREE): i32 fill of a native-memory buffer, never a heap edge.
    ctx.block().store(I32, value_i32, &elem_ptr);
    let next = ctx.block().add(I64, &i, "1");
    ctx.block().store(I64, &next, &index_slot);
    ctx.block().br(&cond_label);

    ctx.current_block = done_idx;
}

fn fill_value_has_no_observable_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Binary { left, right, .. } => {
            fill_value_has_no_observable_side_effects(left)
                && fill_value_has_no_observable_side_effects(right)
        }
        _ => false,
    }
}

fn is_zero_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(0) => true,
        Expr::Number(n) => *n == 0.0,
        _ => false,
    }
}

fn undefined_value() -> String {
    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
}
