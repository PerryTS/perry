use anyhow::Result;
use perry_hir::Expr;

use crate::native_value::{
    BoundsState, BufferAccessMode, BufferAccessProof, LoweredValue, NativeRep, SemanticKind,
};
use crate::types::{DOUBLE, I32, I8, PTR};

use super::{
    bounds_for_buffer_access, buffer_alias_metadata_suffix, buffer_view_lowered_value,
    can_lower_expr_as_i32, effective_alias_state_for_access, lower_expr, lower_expr_native, FnCtx,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct BufferAccessSpec {
    pub expr_kind: &'static str,
    pub buffer_expr_kind: &'static str,
    pub buffer_consumer: &'static str,
    pub access_consumer: &'static str,
    pub result_consumer: Option<&'static str>,
    pub width_bytes: u32,
}

impl BufferAccessSpec {
    pub(crate) fn uint8array_get() -> Self {
        Self {
            expr_kind: "Uint8ArrayGet",
            buffer_expr_kind: "Uint8ArrayGet.array",
            buffer_consumer: "Uint8ArrayGet.BufferView",
            access_consumer: "u8_load_zext_i32",
            result_consumer: Some("Uint8ArrayGet.native_i32"),
            width_bytes: 1,
        }
    }

    pub(crate) fn uint8array_set() -> Self {
        Self {
            expr_kind: "Uint8ArraySet",
            buffer_expr_kind: "Uint8ArraySet.array",
            buffer_consumer: "Uint8ArraySet.BufferView",
            access_consumer: "u8_store_trunc_i32",
            result_consumer: None,
            width_bytes: 1,
        }
    }

    pub(crate) fn buffer_index_get() -> Self {
        Self {
            expr_kind: "BufferIndexGet",
            buffer_expr_kind: "BufferIndexGet.buffer",
            buffer_consumer: "BufferIndexGet.BufferView",
            access_consumer: "u8_load_zext_i32",
            result_consumer: Some("BufferIndexGet.native_i32"),
            width_bytes: 1,
        }
    }

    pub(crate) fn buffer_index_set() -> Self {
        Self {
            expr_kind: "BufferIndexSet",
            buffer_expr_kind: "BufferIndexSet.buffer",
            buffer_consumer: "BufferIndexSet.BufferView",
            access_consumer: "u8_store_trunc_i32",
            result_consumer: None,
            width_bytes: 1,
        }
    }

    pub(crate) fn buffer_numeric_read(width_bytes: u32) -> Self {
        Self {
            expr_kind: "BufferNumericRead",
            buffer_expr_kind: "BufferNumericRead",
            buffer_consumer: "BufferNumericRead.BufferView",
            access_consumer: "BufferNumericRead.raw_load",
            result_consumer: None,
            width_bytes,
        }
    }
}

pub(crate) struct BufferAccessEmission {
    pub data_ptr: String,
    pub len_i32: String,
    pub elem_ptr: String,
    pub alias_metadata: String,
}

pub(crate) struct StoreResult {
    pub result: LoweredValue,
}

fn lower_index_i32_value(ctx: &mut FnCtx<'_>, index: &Expr) -> Result<LoweredValue> {
    let value = if can_lower_expr_as_i32(
        index,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        lower_expr_native(ctx, index, crate::native_value::ExpectedNativeRep::I32)?.value
    } else {
        let d = lower_expr(ctx, index)?;
        ctx.block().fptosi(DOUBLE, &d, I32)
    };
    Ok(LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value,
    })
}

fn lower_value_i32(ctx: &mut FnCtx<'_>, value: &Expr) -> Result<String> {
    if can_lower_expr_as_i32(
        value,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.integer_locals,
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        Ok(lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?.value)
    } else {
        let v = lower_expr(ctx, value)?;
        Ok(ctx.block().fptosi(DOUBLE, &v, I32))
    }
}

pub(crate) fn lower_buffer_access_proof(
    ctx: &mut FnCtx<'_>,
    buffer_expr: &Expr,
    index_expr: &Expr,
    spec: BufferAccessSpec,
) -> Result<Option<BufferAccessProof>> {
    if ctx.disable_buffer_fast_path {
        return Ok(None);
    }

    let (buffer_local_id, view) = match buffer_expr {
        Expr::LocalGet(id) => match ctx.buffer_view_slots.get(id).cloned() {
            Some(view) => (*id, view),
            None => return Ok(None),
        },
        _ => return Ok(None),
    };

    let mut bounds = bounds_for_buffer_access(ctx, buffer_local_id, index_expr);
    if spec.width_bytes != 1 && bounds.allows_inbounds() {
        bounds = BoundsState::Unknown;
    }
    if !bounds.allows_inbounds() {
        return Ok(None);
    }

    let index = lower_index_i32_value(ctx, index_expr)?;
    let alias = effective_alias_state_for_access(ctx, &view);
    let access_mode = BufferAccessMode::UncheckedNative;
    let may_emit_inbounds =
        matches!(access_mode, BufferAccessMode::UncheckedNative) && bounds.allows_inbounds();
    let may_emit_noalias = matches!(access_mode, BufferAccessMode::UncheckedNative)
        && alias.allows_noalias()
        && view.scope_idx.is_some();
    Ok(Some(BufferAccessProof {
        buffer_local_id,
        view,
        index,
        access_mode,
        bounds,
        alias,
        may_emit_inbounds,
        may_emit_noalias,
    }))
}

pub(crate) fn emit_buffer_access_pointer(
    ctx: &mut FnCtx<'_>,
    proof: &BufferAccessProof,
    spec: BufferAccessSpec,
) -> BufferAccessEmission {
    let blk = ctx.block();
    let data_ptr = blk.load(PTR, &proof.view.data_slot);
    let header_ptr = blk.gep(I8, &data_ptr, &[(I32, "-8")]);
    let len_i32 = blk.load_invariant(I32, &header_ptr);
    if proof.may_emit_inbounds {
        let in_bounds = if spec.width_bytes == 1 {
            blk.icmp_ult(I32, &proof.index.value, &len_i32)
        } else {
            let end_i32 = blk.add(I32, &proof.index.value, &spec.width_bytes.to_string());
            blk.icmp_ule(I32, &end_i32, &len_i32)
        };
        blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
    }
    let elem_ptr = if proof.may_emit_inbounds {
        blk.gep_inbounds(I8, &data_ptr, &[(I32, &proof.index.value)])
    } else {
        blk.gep(I8, &data_ptr, &[(I32, &proof.index.value)])
    };
    let alias_metadata = if proof.may_emit_noalias {
        buffer_alias_metadata_suffix(proof.view.scope_idx.expect("scope for noalias proof"))
    } else {
        String::new()
    };
    BufferAccessEmission {
        data_ptr,
        len_i32,
        elem_ptr,
        alias_metadata,
    }
}

fn record_buffer_view(
    ctx: &mut FnCtx<'_>,
    proof: &BufferAccessProof,
    emission: &BufferAccessEmission,
    spec: BufferAccessSpec,
) {
    let buffer_value = buffer_view_lowered_value(
        &emission.data_ptr,
        &emission.len_i32,
        proof.bounds.clone(),
        proof.alias.clone(),
    );
    ctx.record_lowered_value_with_access_mode(
        spec.buffer_expr_kind,
        Some(proof.buffer_local_id),
        spec.buffer_consumer,
        &buffer_value,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode.clone()),
        None,
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("elem={:?}", proof.view.elem)],
    );
}

pub(crate) fn lower_buffer_load(
    ctx: &mut FnCtx<'_>,
    buffer_expr: &Expr,
    index_expr: &Expr,
    spec: BufferAccessSpec,
) -> Result<Option<LoweredValue>> {
    let Some(proof) = lower_buffer_access_proof(ctx, buffer_expr, index_expr, spec)? else {
        return Ok(None);
    };
    let emission = emit_buffer_access_pointer(ctx, &proof, spec);
    let byte_val = ctx.block().fresh_reg();
    ctx.block().emit_raw(format!(
        "{} = load i8, ptr {}{}",
        byte_val, emission.elem_ptr, emission.alias_metadata
    ));
    let result_i32 = ctx.block().zext(I8, &byte_val, I32);
    record_buffer_view(ctx, &proof, &emission, spec);
    let u8_value = LoweredValue {
        semantic: SemanticKind::TypedArrayElement,
        rep: NativeRep::U8,
        llvm_ty: I8,
        value: byte_val,
    };
    ctx.record_lowered_value_with_access_mode(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.access_consumer,
        &u8_value,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode.clone()),
        None,
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("zext_to={}", result_i32)],
    );
    let result = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: result_i32,
    };
    if let Some(consumer) = spec.result_consumer {
        ctx.record_lowered_value_with_access_mode(
            spec.expr_kind,
            Some(proof.buffer_local_id),
            consumer,
            &result,
            Some(proof.bounds),
            Some(proof.alias),
            Some(proof.access_mode),
            None,
            false,
            false,
            Vec::new(),
        );
    }
    Ok(Some(result))
}

pub(crate) fn lower_buffer_store(
    ctx: &mut FnCtx<'_>,
    buffer_expr: &Expr,
    index_expr: &Expr,
    value_expr: &Expr,
    spec: BufferAccessSpec,
) -> Result<Option<StoreResult>> {
    let Some(proof) = lower_buffer_access_proof(ctx, buffer_expr, index_expr, spec)? else {
        return Ok(None);
    };
    let val_i32 = lower_value_i32(ctx, value_expr)?;
    let emission = emit_buffer_access_pointer(ctx, &proof, spec);
    let byte_val = ctx.block().trunc(I32, &val_i32, I8);
    ctx.block().emit_raw(format!(
        "store i8 {}, ptr {}{}",
        byte_val, emission.elem_ptr, emission.alias_metadata
    ));
    record_buffer_view(ctx, &proof, &emission, spec);
    let stored = LoweredValue {
        semantic: SemanticKind::TypedArrayElement,
        rep: NativeRep::U8,
        llvm_ty: I8,
        value: byte_val,
    };
    ctx.record_lowered_value_with_access_mode(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.access_consumer,
        &stored,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode.clone()),
        None,
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("source_i32={}", val_i32)],
    );
    let result = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: val_i32.clone(),
    };
    Ok(Some(StoreResult { result }))
}
