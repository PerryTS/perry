use serde::Serialize;

use crate::expr::FnCtx;
use crate::nanbox::{BIGINT_TAG_I64, POINTER_TAG_I64};
use crate::types::{DOUBLE, F32, I1, I128, I32, I64, I8};

use super::artifact::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
use super::rep::{LoweredValue, NativeRep, SemanticKind};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaterializationReason {
    FunctionAbi,
    ReturnAbi,
    // #854: materialization-reason variants not yet emitted by any
    // materialization site; kept as part of the serialized reason taxonomy.
    #[allow(dead_code)]
    GenericCall,
    #[allow(dead_code)]
    DynamicPropertyAccess,
    #[allow(dead_code)]
    ExceptionPath,
    RuntimeApi,
    #[allow(dead_code)]
    DebugLogging,
    UnknownAlias,
    UnknownBounds,
    ClosureCapture,
    Reassignment,
    UnknownCallEscape,
    UseAfterDispose,
    EscapingUnownedPointer,
    StaleViewLength,
    MutableAlias,
    MissingOwnerRoot,
    PodMaterialization,
    PodUnsupported,
    PodDynamicMutation,
}

fn transition_lossy(rep: &NativeRep, op: &NativeAbiTransitionOp) -> bool {
    match op {
        NativeAbiTransitionOp::SignedIntToFloat => matches!(rep, NativeRep::I64),
        NativeAbiTransitionOp::UnsignedIntToFloat => {
            matches!(rep, NativeRep::U64 | NativeRep::USize | NativeRep::HandleId)
        }
        NativeAbiTransitionOp::JsValueToBits
        | NativeAbiTransitionOp::BitsToJsValue
        | NativeAbiTransitionOp::None
        | NativeAbiTransitionOp::FloatExtend
        | NativeAbiTransitionOp::PointerBox
        | NativeAbiTransitionOp::NativeHandleBox
        | NativeAbiTransitionOp::PromiseBox
        | NativeAbiTransitionOp::BoolToJsValue
        | NativeAbiTransitionOp::BigIntBox => false,
    }
}

fn record_transition(
    ctx: &mut FnCtx<'_>,
    expr_kind: &'static str,
    consumer: &'static str,
    materialized: &LoweredValue,
    from_native_rep: String,
    to_native_rep: String,
    op: NativeAbiTransitionOp,
    reason: MaterializationReason,
    lossy: bool,
) {
    let transition = NativeAbiTransitionRecord {
        from_native_rep,
        to_native_rep,
        op,
        reason: reason.clone(),
        lossy,
    };
    ctx.record_lowered_value_with_access_mode_and_conversion(
        expr_kind,
        None,
        consumer,
        materialized,
        None,
        None,
        None,
        Some(reason),
        Some(transition),
        None,
        false,
        false,
        Vec::new(),
    );
}

fn record_materialized_transition(
    ctx: &mut FnCtx<'_>,
    expr_kind: &'static str,
    consumer: &'static str,
    materialized: &LoweredValue,
    from_native_rep: String,
    op: NativeAbiTransitionOp,
    reason: MaterializationReason,
    lossy: bool,
) {
    record_transition(
        ctx,
        expr_kind,
        consumer,
        materialized,
        from_native_rep,
        NativeRep::JsValue.name().to_string(),
        op,
        reason,
        lossy,
    );
}

pub(crate) fn record_runtime_native_handle_box_transition(
    ctx: &mut FnCtx<'_>,
    value: &str,
    reason: MaterializationReason,
) {
    let materialized = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.to_string(),
    };
    record_materialized_transition(
        ctx,
        "materialize_js_value",
        "materialize_native_handle_runtime",
        &materialized,
        NativeRep::NativeHandle.name().to_string(),
        NativeAbiTransitionOp::NativeHandleBox,
        reason,
        false,
    );
}

fn box_raw_i64_as_js_pointer(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
    op: NativeAbiTransitionOp,
    consumer: &'static str,
) -> String {
    let from_native_rep = lowered.rep.name().to_string();
    let tagged = ctx.block().or(I64, &lowered.value, POINTER_TAG_I64);
    let value = ctx.block().bitcast_i64_to_double(&tagged);
    let materialized = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    record_materialized_transition(
        ctx,
        "materialize_js_value",
        consumer,
        &materialized,
        from_native_rep,
        op,
        reason,
        false,
    );
    value
}

fn box_raw_i64_as_js_pointer_bits(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
    op: NativeAbiTransitionOp,
    consumer: &'static str,
) -> String {
    let from_native_rep = lowered.rep.name().to_string();
    let bits = ctx.block().or(I64, &lowered.value, POINTER_TAG_I64);
    let materialized = LoweredValue::js_value_bits(bits.clone());
    record_transition(
        ctx,
        "materialize_js_value_bits",
        consumer,
        &materialized,
        from_native_rep,
        NativeRep::JsValueBits.name().to_string(),
        op,
        reason,
        false,
    );
    bits
}

pub(crate) fn materialize_native_handle_to_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    debug_assert!(matches!(lowered.rep, NativeRep::NativeHandle));
    box_raw_i64_as_js_pointer(
        ctx,
        lowered,
        reason,
        NativeAbiTransitionOp::PointerBox,
        "materialize_native_handle",
    )
}

pub(crate) fn materialize_promise_boundary_to_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    debug_assert!(matches!(lowered.rep, NativeRep::PromiseBoundary));
    box_raw_i64_as_js_pointer(
        ctx,
        lowered,
        reason,
        NativeAbiTransitionOp::PromiseBox,
        "materialize_promise_boundary",
    )
}

pub(crate) fn materialize_small_bigint_pointer_to_js_value(
    ctx: &mut FnCtx<'_>,
    ptr_i64: &str,
    reason: MaterializationReason,
) -> String {
    let tagged = ctx.block().or(I64, ptr_i64, BIGINT_TAG_I64);
    let value = ctx.block().bitcast_i64_to_double(&tagged);
    let materialized = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    record_materialized_transition(
        ctx,
        "materialize_js_value",
        "materialize_small_bigint",
        &materialized,
        NativeRep::SmallBigInt.name().to_string(),
        NativeAbiTransitionOp::BigIntBox,
        reason,
        false,
    );
    value
}

fn box_small_bigint_i128_to_js_value(ctx: &mut FnCtx<'_>, value_i128: &str) -> String {
    let lo = ctx.block().trunc(I128, value_i128, I64);
    let hi_wide = ctx.block().ashr(I128, value_i128, "64");
    let hi = ctx.block().trunc(I128, &hi_wide, I64);
    let ptr = ctx
        .block()
        .call(I64, "js_bigint_from_i128_parts", &[(I64, &lo), (I64, &hi)]);
    let tagged = ctx.block().or(I64, &ptr, BIGINT_TAG_I64);
    ctx.block().bitcast_i64_to_double(&tagged)
}

pub(crate) fn materialize_js_value_bits(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    if matches!(lowered.rep, NativeRep::JsValueBits) {
        return lowered.value;
    }
    if matches!(lowered.rep, NativeRep::NativeHandle) {
        return box_raw_i64_as_js_pointer_bits(
            ctx,
            lowered,
            reason,
            NativeAbiTransitionOp::PointerBox,
            "materialize_native_handle_bits",
        );
    }
    if matches!(lowered.rep, NativeRep::PromiseBoundary) {
        return box_raw_i64_as_js_pointer_bits(
            ctx,
            lowered,
            reason,
            NativeAbiTransitionOp::PromiseBox,
            "materialize_promise_boundary_bits",
        );
    }
    if matches!(lowered.rep, NativeRep::SmallBigInt) {
        let from_native_rep = lowered.rep.name().to_string();
        let value = box_small_bigint_i128_to_js_value(ctx, &lowered.value);
        let bits = ctx.block().bitcast_double_to_i64(&value);
        let materialized = LoweredValue::js_value_bits(bits.clone());
        record_transition(
            ctx,
            "materialize_js_value_bits",
            "materialize_small_bigint_bits",
            &materialized,
            from_native_rep,
            NativeRep::JsValueBits.name().to_string(),
            NativeAbiTransitionOp::BigIntBox,
            reason,
            false,
        );
        return bits;
    }
    if matches!(
        lowered.rep,
        NativeRep::StringRef
            | NativeRep::BufferView(_)
            | NativeRep::PodRecord { .. }
            | NativeRep::PodRecordView { .. }
    ) {
        let js_value = materialize_js_value(ctx, lowered, reason.clone());
        let bits = ctx.block().bitcast_double_to_i64(&js_value);
        let materialized = LoweredValue::js_value_bits(bits.clone());
        record_transition(
            ctx,
            "materialize_js_value_bits",
            "materialize_js_value_bits",
            &materialized,
            NativeRep::JsValue.name().to_string(),
            NativeRep::JsValueBits.name().to_string(),
            NativeAbiTransitionOp::JsValueToBits,
            reason,
            false,
        );
        return bits;
    }
    let from_native_rep = lowered.rep.name().to_string();
    let conversion_op = match &lowered.rep {
        NativeRep::JsValue => NativeAbiTransitionOp::JsValueToBits,
        NativeRep::I32 | NativeRep::I64 => NativeAbiTransitionOp::SignedIntToFloat,
        NativeRep::U8
        | NativeRep::U32
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::HandleId
        | NativeRep::BufferLen => NativeAbiTransitionOp::UnsignedIntToFloat,
        NativeRep::I1 => NativeAbiTransitionOp::BoolToJsValue,
        NativeRep::F32 => NativeAbiTransitionOp::FloatExtend,
        NativeRep::F64 => NativeAbiTransitionOp::None,
        NativeRep::BufferView(_)
        | NativeRep::PodRecord { .. }
        | NativeRep::PodRecordView { .. }
        | NativeRep::StringRef
        | NativeRep::JsValueBits
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary
        | NativeRep::SmallBigInt => NativeAbiTransitionOp::None,
    };
    let lossy = transition_lossy(&lowered.rep, &conversion_op);
    let bits = match &lowered.rep {
        NativeRep::JsValue => ctx.block().bitcast_double_to_i64(&lowered.value),
        NativeRep::I1 => ctx.block().select(
            I1,
            &lowered.value,
            I64,
            crate::nanbox::TAG_TRUE_I64,
            crate::nanbox::TAG_FALSE_I64,
        ),
        NativeRep::I32 => {
            let value = ctx.block().sitofp(I32, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::I64 => {
            let value = ctx.block().sitofp(I64, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            let value = ctx.block().uitofp(I32, &widened, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::U32 => {
            let value = ctx.block().uitofp(I32, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => {
            let value = ctx.block().uitofp(I64, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::BufferLen => {
            let value = ctx.block().uitofp(I32, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::F32 => {
            let value = ctx.block().fpext(F32, &lowered.value, DOUBLE);
            ctx.block().bitcast_double_to_i64(&value)
        }
        NativeRep::F64 => ctx.block().bitcast_double_to_i64(&lowered.value),
        NativeRep::BufferView(_)
        | NativeRep::PodRecord { .. }
        | NativeRep::PodRecordView { .. }
        | NativeRep::StringRef
        | NativeRep::JsValueBits
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary
        | NativeRep::SmallBigInt => {
            unreachable!("handled before direct js_value_bits materialization")
        }
    };
    let materialized = LoweredValue::js_value_bits(bits.clone());
    record_transition(
        ctx,
        "materialize_js_value_bits",
        "materialize_js_value_bits",
        &materialized,
        from_native_rep,
        NativeRep::JsValueBits.name().to_string(),
        conversion_op,
        reason,
        lossy,
    );
    bits
}

fn materialize_js_value_bits_to_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    let from_native_rep = lowered.rep.name().to_string();
    let value = ctx.block().bitcast_i64_to_double(&lowered.value);
    let materialized = LoweredValue::js_value(value.clone());
    record_materialized_transition(
        ctx,
        "materialize_js_value",
        "materialize_js_value_bits",
        &materialized,
        from_native_rep,
        NativeAbiTransitionOp::BitsToJsValue,
        reason,
        false,
    );
    value
}

pub(crate) fn materialize_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    if matches!(&lowered.rep, NativeRep::JsValue) {
        return lowered.value;
    }
    if matches!(&lowered.rep, NativeRep::JsValueBits) {
        return materialize_js_value_bits_to_js_value(ctx, lowered, reason);
    }
    if matches!(&lowered.rep, NativeRep::NativeHandle) {
        return materialize_native_handle_to_js_value(ctx, lowered, reason);
    }
    if matches!(&lowered.rep, NativeRep::PromiseBoundary) {
        return materialize_promise_boundary_to_js_value(ctx, lowered, reason);
    }
    if matches!(&lowered.rep, NativeRep::SmallBigInt) {
        let from_native_rep = lowered.rep.name().to_string();
        let value = box_small_bigint_i128_to_js_value(ctx, &lowered.value);
        let materialized = LoweredValue::js_value(value.clone());
        record_materialized_transition(
            ctx,
            "materialize_js_value",
            "materialize_small_bigint",
            &materialized,
            from_native_rep,
            NativeAbiTransitionOp::BigIntBox,
            reason,
            false,
        );
        return value;
    }
    let from_native_rep = lowered.rep.name().to_string();
    let conversion_op = match &lowered.rep {
        NativeRep::I32 | NativeRep::I64 => NativeAbiTransitionOp::SignedIntToFloat,
        NativeRep::U8
        | NativeRep::U32
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::HandleId
        | NativeRep::BufferLen => NativeAbiTransitionOp::UnsignedIntToFloat,
        NativeRep::I1 => NativeAbiTransitionOp::BoolToJsValue,
        NativeRep::F32 => NativeAbiTransitionOp::FloatExtend,
        NativeRep::F64 => NativeAbiTransitionOp::None,
        NativeRep::StringRef => NativeAbiTransitionOp::PointerBox,
        NativeRep::BufferView(_)
        | NativeRep::PodRecord { .. }
        | NativeRep::PodRecordView { .. }
        | NativeRep::JsValueBits
        | NativeRep::JsValue
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary
        | NativeRep::SmallBigInt => NativeAbiTransitionOp::None,
    };
    let lossy = transition_lossy(&lowered.rep, &conversion_op);
    let value = match &lowered.rep {
        NativeRep::I1 => {
            let bits = ctx.block().select(
                I1,
                &lowered.value,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            ctx.block().bitcast_i64_to_double(&bits)
        }
        NativeRep::I32 => ctx.block().sitofp(I32, &lowered.value, DOUBLE),
        NativeRep::I64 => ctx.block().sitofp(I64, &lowered.value, DOUBLE),
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            ctx.block().uitofp(I32, &widened, DOUBLE)
        }
        NativeRep::U32 => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => {
            ctx.block().uitofp(I64, &lowered.value, DOUBLE)
        }
        NativeRep::BufferLen => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::F32 => ctx.block().fpext(F32, &lowered.value, DOUBLE),
        NativeRep::StringRef => {
            ctx.block()
                .call(DOUBLE, "js_nanbox_string", &[(I64, &lowered.value)])
        }
        NativeRep::BufferView(_) => lowered.value.clone(),
        NativeRep::PodRecord { .. } => lowered.value.clone(),
        NativeRep::PodRecordView { .. } => lowered.value.clone(),
        NativeRep::JsValueBits => lowered.value.clone(),
        NativeRep::JsValue
        | NativeRep::F64
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary
        | NativeRep::SmallBigInt => lowered.value.clone(),
    };
    let materialized = LoweredValue {
        semantic: lowered.semantic,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    record_materialized_transition(
        ctx,
        "materialize_js_value",
        "materialize_js_value",
        &materialized,
        from_native_rep,
        conversion_op,
        reason,
        lossy,
    );
    value
}

pub(crate) fn materialize_js_value_without_record(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
) -> String {
    match &lowered.rep {
        NativeRep::JsValue | NativeRep::F64 => lowered.value.clone(),
        NativeRep::JsValueBits => ctx.block().bitcast_i64_to_double(&lowered.value),
        NativeRep::NativeHandle | NativeRep::PromiseBoundary => {
            let tagged = ctx.block().or(I64, &lowered.value, POINTER_TAG_I64);
            ctx.block().bitcast_i64_to_double(&tagged)
        }
        NativeRep::StringRef => {
            ctx.block()
                .call(DOUBLE, "js_nanbox_string", &[(I64, &lowered.value)])
        }
        NativeRep::I1 => {
            let bits = ctx.block().select(
                I1,
                &lowered.value,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            ctx.block().bitcast_i64_to_double(&bits)
        }
        NativeRep::I32 => ctx.block().sitofp(I32, &lowered.value, DOUBLE),
        NativeRep::I64 => ctx.block().sitofp(I64, &lowered.value, DOUBLE),
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            ctx.block().uitofp(I32, &widened, DOUBLE)
        }
        NativeRep::U32 => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => {
            ctx.block().uitofp(I64, &lowered.value, DOUBLE)
        }
        NativeRep::BufferLen => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::F32 => ctx.block().fpext(F32, &lowered.value, DOUBLE),
        NativeRep::BufferView(_)
        | NativeRep::PodRecord { .. }
        | NativeRep::PodRecordView { .. } => lowered.value.clone(),
        NativeRep::SmallBigInt => box_small_bigint_i128_to_js_value(ctx, &lowered.value),
    }
}
