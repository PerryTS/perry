use anyhow::Result;
use perry_hir::Expr;

use crate::native_value::{
    BufferAccessFacts, BufferAccessMode, BufferAccessProof, BufferElem, BufferEndian,
    BufferIndexUnit, ExpectedNativeRep, LoweredValue, MaterializationReason,
};
use crate::types::{DOUBLE, F32, I16, I32, I8, PTR};

use super::{
    attach_native_owned_view_fact, bounds_for_buffer_access_width, buffer_alias_metadata_suffix,
    buffer_view_lowered_value, can_lower_expr_as_i32, effective_alias_state_for_access,
    int_range_expr, is_numeric_expr, lower_expr, lower_expr_native, FnCtx,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct BufferAccessSpec {
    pub expr_kind: &'static str,
    pub buffer_expr_kind: &'static str,
    pub buffer_consumer: &'static str,
    pub access_consumer: &'static str,
    pub result_consumer: Option<&'static str>,
    pub width_bytes: u32,
    pub index_unit: BufferIndexUnit,
    pub element_width_bytes: u32,
    pub endian: BufferEndian,
    pub signed: bool,
    pub floating: bool,
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
            index_unit: BufferIndexUnit::Element,
            element_width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            floating: false,
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
            index_unit: BufferIndexUnit::Element,
            element_width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            floating: false,
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
            index_unit: BufferIndexUnit::Byte,
            element_width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            floating: false,
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
            index_unit: BufferIndexUnit::Byte,
            element_width_bytes: 1,
            endian: BufferEndian::Native,
            signed: false,
            floating: false,
        }
    }

    pub(crate) fn buffer_numeric_read(
        width_bytes: u32,
        endian: BufferEndian,
        signed: bool,
        floating: bool,
    ) -> Self {
        Self {
            expr_kind: "BufferNumericRead",
            buffer_expr_kind: "BufferNumericRead",
            buffer_consumer: "BufferNumericRead.BufferView",
            access_consumer: "BufferNumericRead.raw_load",
            result_consumer: None,
            width_bytes,
            index_unit: BufferIndexUnit::Byte,
            element_width_bytes: 1,
            endian,
            signed,
            floating,
        }
    }

    pub(crate) fn typed_array_get(
        expr_kind: &'static str,
        access_consumer: &'static str,
        result_consumer: &'static str,
        elem: BufferElem,
        element_width_bytes: u32,
        signed: bool,
        floating: bool,
    ) -> Self {
        let _ = elem;
        Self {
            expr_kind,
            buffer_expr_kind: "TypedArrayGet.array",
            buffer_consumer: "TypedArrayGet.BufferView",
            access_consumer,
            result_consumer: Some(result_consumer),
            width_bytes: element_width_bytes,
            index_unit: BufferIndexUnit::Element,
            element_width_bytes,
            endian: BufferEndian::Native,
            signed,
            floating,
        }
    }

    pub(crate) fn typed_array_set(
        expr_kind: &'static str,
        access_consumer: &'static str,
        elem: BufferElem,
        element_width_bytes: u32,
        signed: bool,
        floating: bool,
    ) -> Self {
        let _ = elem;
        Self {
            expr_kind,
            buffer_expr_kind: "TypedArraySet.array",
            buffer_consumer: "TypedArraySet.BufferView",
            access_consumer,
            result_consumer: None,
            width_bytes: element_width_bytes,
            index_unit: BufferIndexUnit::Element,
            element_width_bytes,
            endian: BufferEndian::Native,
            signed,
            floating,
        }
    }

    pub(crate) fn bounds_width_units(&self) -> u32 {
        match self.index_unit {
            BufferIndexUnit::Byte => self.width_bytes.max(1),
            BufferIndexUnit::Element => 1,
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

pub(crate) fn access_facts_for_spec(
    spec: BufferAccessSpec,
    view: &crate::native_value::BufferViewSlot,
    len_i32: Option<&str>,
) -> BufferAccessFacts {
    BufferAccessFacts {
        access_width_bytes: spec.width_bytes.max(1),
        index_unit: spec.index_unit,
        element_width_bytes: spec.element_width_bytes.max(1),
        endian: spec.endian,
        signed: spec.signed,
        floating: spec.floating,
        bounds_width_units: spec.bounds_width_units(),
        view_length: len_i32.map(str::to_string),
        view_byte_offset: view.view_byte_offset,
    }
}

fn lower_index_i32_value(ctx: &mut FnCtx<'_>, index: &Expr) -> Result<LoweredValue> {
    let value = if can_lower_expr_as_i32(
        index,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.native_facts.integer_locals(),
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        lower_expr_native(ctx, index, crate::native_value::ExpectedNativeRep::I32)?.value
    } else {
        lower_expr_native(ctx, index, crate::native_value::ExpectedNativeRep::I32)?.value
    };
    Ok(LoweredValue::i32(value))
}

fn lower_value_i32(ctx: &mut FnCtx<'_>, value: &Expr) -> Result<String> {
    if can_lower_expr_as_i32(
        value,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.native_facts.integer_locals(),
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) {
        Ok(lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?.value)
    } else {
        Ok(lower_expr_native(ctx, value, crate::native_value::ExpectedNativeRep::I32)?.value)
    }
}

fn can_lower_integer_typed_array_store_value(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    can_lower_expr_as_i32(
        value,
        &ctx.i32_counter_slots,
        ctx.flat_const_arrays,
        &ctx.array_row_aliases,
        ctx.native_facts.integer_locals(),
        ctx.clamp3_functions,
        ctx.clamp_u8_functions,
        ctx.integer_returning_functions,
        ctx.i32_identity_functions,
    ) || int_range_expr(ctx, value)
        .is_some_and(|range| range.min >= i32::MIN as i64 && range.max <= i32::MAX as i64)
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

    // A closure-captured buffer local is hazardous even before any escape
    // walk stamped `buffer_hazard_reasons` — the closure may mutate/realloc
    // the buffer between the proof and the access. Consult the capture map
    // directly (mirroring `buffer_access_materialization_reason`) and persist
    // the decision so later gates agree.
    if ctx.closure_captures.contains_key(&buffer_local_id)
        || matches!(
            ctx.buffer_hazard_reasons.get(&buffer_local_id),
            Some(MaterializationReason::ClosureCapture)
        )
    {
        ctx.buffer_hazard_reasons
            .entry(buffer_local_id)
            .or_insert(MaterializationReason::ClosureCapture);
        return Ok(None);
    }

    let bounds =
        bounds_for_buffer_access_width(ctx, buffer_local_id, index_expr, spec.bounds_width_units());
    if !bounds.allows_inbounds() {
        return Ok(None);
    }

    let alias = effective_alias_state_for_access(ctx, &view);
    if view.native_owned.is_some() && !alias.allows_noalias() {
        ctx.buffer_hazard_reasons
            .entry(buffer_local_id)
            .or_insert(MaterializationReason::MutableAlias);
        return Ok(None);
    }
    let index = lower_index_i32_value(ctx, index_expr)?;
    let access_mode = BufferAccessMode::UncheckedNative;
    let may_emit_inbounds =
        matches!(access_mode, BufferAccessMode::UncheckedNative) && bounds.allows_inbounds();
    let may_emit_noalias = matches!(access_mode, BufferAccessMode::UncheckedNative)
        && alias.allows_noalias()
        && view.scope_idx.is_some();
    Ok(Some(BufferAccessProof {
        buffer_local_id,
        view: view.clone(),
        index,
        access_mode,
        bounds,
        facts: access_facts_for_spec(spec, &view, None),
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
    let len_i32 = if let Some(length_slot) = proof.view.length_slot.as_ref() {
        blk.load(I32, length_slot)
    } else {
        let header_ptr = blk.gep(
            I8,
            &data_ptr,
            &[(I32, &proof.view.length_offset_from_data.to_string())],
        );
        blk.load_invariant(I32, &header_ptr)
    };
    if proof.may_emit_inbounds {
        let bounds_width_units = spec.bounds_width_units();
        let in_bounds = if bounds_width_units == 1 {
            blk.icmp_ult(I32, &proof.index.value, &len_i32)
        } else {
            let end_i32 = blk.add(I32, &proof.index.value, &bounds_width_units.to_string());
            blk.icmp_ule(I32, &end_i32, &len_i32)
        };
        blk.emit_raw(format!("call void @llvm.assume(i1 {})", in_bounds));
    }
    let byte_index = if spec.element_width_bytes > 1 {
        blk.mul(
            I32,
            &proof.index.value,
            &spec.element_width_bytes.to_string(),
        )
    } else {
        proof.index.value.clone()
    };
    let elem_ptr = if proof.may_emit_inbounds {
        blk.gep_inbounds(I8, &data_ptr, &[(I32, &byte_index)])
    } else {
        blk.gep(I8, &data_ptr, &[(I32, &byte_index)])
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
        proof.view.elem.clone(),
        proof.view.element_width_bytes,
        proof.view.index_unit,
        proof.view.view_byte_offset,
        proof.view.length_offset_from_data,
        proof.bounds.clone(),
        proof.alias.clone(),
    );
    let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        spec.buffer_expr_kind,
        Some(proof.buffer_local_id),
        spec.buffer_consumer,
        &buffer_value,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("elem={:?}", proof.view.elem)],
    );
    attach_native_owned_view_fact(ctx, &proof.view);
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
    let u8_value = LoweredValue::u8(byte_val);
    let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.access_consumer,
        &u8_value,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("zext_to={}", result_i32)],
    );
    attach_native_owned_view_fact(ctx, &proof.view);
    let result = LoweredValue::i32(result_i32);
    if let Some(consumer) = spec.result_consumer {
        let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
        ctx.record_lowered_value_with_access_mode_and_conversion(
            spec.expr_kind,
            Some(proof.buffer_local_id),
            consumer,
            &result,
            Some(proof.bounds.clone()),
            Some(proof.alias.clone()),
            Some(proof.access_mode),
            None,
            None,
            Some(facts),
            false,
            false,
            Vec::new(),
        );
        attach_native_owned_view_fact(ctx, &proof.view);
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
    let stored = LoweredValue::u8(byte_val);
    let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.access_consumer,
        &stored,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("source_i32={}", val_i32)],
    );
    attach_native_owned_view_fact(ctx, &proof.view);
    let result = LoweredValue::i32(val_i32.clone());
    Ok(Some(StoreResult { result }))
}

fn typed_array_get_spec(view: &crate::native_value::BufferViewSlot) -> Option<BufferAccessSpec> {
    let (access_consumer, result_consumer, signed, floating) = match view.elem {
        BufferElem::I8 => ("i8_load_sext_i32", "TypedArrayGet.native_i32", true, false),
        BufferElem::U8 | BufferElem::U8Clamped => {
            ("u8_load_zext_i32", "TypedArrayGet.native_i32", false, false)
        }
        BufferElem::I16 => ("i16_load_sext_i32", "TypedArrayGet.native_i32", true, false),
        BufferElem::U16 => (
            "u16_load_zext_u32",
            "TypedArrayGet.native_u32",
            false,
            false,
        ),
        BufferElem::I32 => ("i32_load", "TypedArrayGet.native_i32", true, false),
        BufferElem::U32 => ("u32_load", "TypedArrayGet.native_u32", false, false),
        BufferElem::F32 => ("f32_load", "TypedArrayGet.native_f32", false, true),
        BufferElem::F64 => ("f64_load", "TypedArrayGet.native_f64", false, true),
    };
    Some(BufferAccessSpec::typed_array_get(
        "TypedArrayGet",
        access_consumer,
        result_consumer,
        view.elem.clone(),
        view.element_width_bytes,
        signed,
        floating,
    ))
}

fn typed_array_set_spec(view: &crate::native_value::BufferViewSlot) -> Option<BufferAccessSpec> {
    let (access_consumer, signed, floating) = match view.elem {
        BufferElem::I8 => ("i8_store_trunc_i32", true, false),
        BufferElem::U8 => ("u8_store_trunc_i32", false, false),
        BufferElem::U8Clamped => return None,
        BufferElem::I16 => ("i16_store_trunc_i32", true, false),
        BufferElem::U16 => ("u16_store_trunc_i32", false, false),
        BufferElem::I32 => ("i32_store", true, false),
        BufferElem::U32 => ("u32_store", false, false),
        BufferElem::F32 => ("f32_store", false, true),
        BufferElem::F64 => ("f64_store", false, true),
    };
    Some(BufferAccessSpec::typed_array_set(
        "TypedArraySet",
        access_consumer,
        view.elem.clone(),
        view.element_width_bytes,
        signed,
        floating,
    ))
}

pub(crate) fn lower_typed_array_load(
    ctx: &mut FnCtx<'_>,
    array_expr: &Expr,
    index_expr: &Expr,
) -> Result<Option<LoweredValue>> {
    let view = match array_expr {
        Expr::LocalGet(id) => match ctx.buffer_view_slots.get(id).cloned() {
            Some(view) if view.index_unit == BufferIndexUnit::Element => view,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    if !view.alias.allows_noalias() || view.scope_idx.is_none() {
        return Ok(None);
    }
    let Some(spec) = typed_array_get_spec(&view) else {
        return Ok(None);
    };
    let Some(proof) = lower_buffer_access_proof(ctx, array_expr, index_expr, spec)? else {
        return Ok(None);
    };
    let emission = emit_buffer_access_pointer(ctx, &proof, spec);
    let result = {
        let blk = ctx.block();
        macro_rules! load_raw {
            ($ty:expr) => {{
                let raw = blk.fresh_reg();
                blk.emit_raw(format!(
                    "{} = load {}, ptr {}{}",
                    raw, $ty, emission.elem_ptr, emission.alias_metadata
                ));
                raw
            }};
        }
        match proof.view.elem {
            BufferElem::I8 => {
                let raw = load_raw!(I8);
                LoweredValue::i32(blk.sext(I8, &raw, I32))
            }
            BufferElem::U8 | BufferElem::U8Clamped => {
                let raw = load_raw!(I8);
                LoweredValue::i32(blk.zext(I8, &raw, I32))
            }
            BufferElem::I16 => {
                let raw = load_raw!(I16);
                LoweredValue::i32(blk.sext(I16, &raw, I32))
            }
            BufferElem::U16 => {
                let raw = load_raw!(I16);
                LoweredValue::u32(blk.zext(I16, &raw, I32))
            }
            BufferElem::I32 => LoweredValue::i32(load_raw!(I32)),
            BufferElem::U32 => LoweredValue::u32(load_raw!(I32)),
            BufferElem::F32 => LoweredValue::f32(load_raw!(F32)),
            BufferElem::F64 => LoweredValue::f64(load_raw!(DOUBLE)),
        }
    };
    record_buffer_view(ctx, &proof, &emission, spec);
    let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.result_consumer.unwrap_or("TypedArrayGet.native_value"),
        &result,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("elem={:?}", proof.view.elem)],
    );
    attach_native_owned_view_fact(ctx, &proof.view);
    Ok(Some(result))
}

pub(crate) fn lower_typed_array_store(
    ctx: &mut FnCtx<'_>,
    array_expr: &Expr,
    index_expr: &Expr,
    value_expr: &Expr,
) -> Result<Option<StoreResult>> {
    let view = match array_expr {
        Expr::LocalGet(id) => match ctx.buffer_view_slots.get(id).cloned() {
            Some(view) if view.index_unit == BufferIndexUnit::Element => view,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    if !view.alias.allows_noalias() || view.scope_idx.is_none() {
        return Ok(None);
    }
    let Some(spec) = typed_array_set_spec(&view) else {
        return Ok(None);
    };
    if matches!(
        view.elem,
        BufferElem::I8
            | BufferElem::U8
            | BufferElem::I16
            | BufferElem::U16
            | BufferElem::I32
            | BufferElem::U32
    ) && !can_lower_integer_typed_array_store_value(ctx, value_expr)
    {
        return Ok(None);
    }
    if matches!(view.elem, BufferElem::F32 | BufferElem::F64) && !is_numeric_expr(ctx, value_expr) {
        return Ok(None);
    }

    let Some(proof) = lower_buffer_access_proof(ctx, array_expr, index_expr, spec)? else {
        return Ok(None);
    };
    let emission = emit_buffer_access_pointer(ctx, &proof, spec);
    let (stored, result) = match proof.view.elem {
        BufferElem::I8 | BufferElem::U8 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::I32)?;
            let byte = ctx.block().trunc(I32, &value.value, I8);
            ctx.block().emit_raw(format!(
                "store i8 {}, ptr {}{}",
                byte, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::u8(byte), value)
        }
        BufferElem::I16 | BufferElem::U16 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::I32)?;
            let half = ctx.block().trunc(I32, &value.value, I16);
            ctx.block().emit_raw(format!(
                "store i16 {}, ptr {}{}",
                half, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::i32(value.value.clone()), value)
        }
        BufferElem::I32 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::I32)?;
            ctx.block().emit_raw(format!(
                "store i32 {}, ptr {}{}",
                value.value, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::i32(value.value.clone()), value)
        }
        BufferElem::U32 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::U32)?;
            ctx.block().emit_raw(format!(
                "store i32 {}, ptr {}{}",
                value.value, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::u32(value.value.clone()), value)
        }
        BufferElem::F32 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::F64)?;
            let narrow = ctx.block().fptrunc(DOUBLE, &value.value, F32);
            ctx.block().emit_raw(format!(
                "store float {}, ptr {}{}",
                narrow, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::f32(narrow), value)
        }
        BufferElem::F64 => {
            let value = lower_expr_native(ctx, value_expr, ExpectedNativeRep::F64)?;
            ctx.block().emit_raw(format!(
                "store double {}, ptr {}{}",
                value.value, emission.elem_ptr, emission.alias_metadata
            ));
            (LoweredValue::f64(value.value.clone()), value)
        }
        BufferElem::U8Clamped => return Ok(None),
    };
    record_buffer_view(ctx, &proof, &emission, spec);
    let facts = access_facts_for_spec(spec, &proof.view, Some(&emission.len_i32));
    ctx.record_lowered_value_with_access_mode_and_conversion(
        spec.expr_kind,
        Some(proof.buffer_local_id),
        spec.access_consumer,
        &stored,
        Some(proof.bounds.clone()),
        Some(proof.alias.clone()),
        Some(proof.access_mode),
        None,
        None,
        Some(facts),
        proof.may_emit_inbounds,
        proof.may_emit_noalias,
        vec![format!("elem={:?}", proof.view.elem)],
    );
    attach_native_owned_view_fact(ctx, &proof.view);
    Ok(Some(StoreResult { result }))
}
