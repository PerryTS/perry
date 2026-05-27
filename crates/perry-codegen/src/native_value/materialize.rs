use serde::Serialize;

use crate::expr::FnCtx;
use crate::nanbox::POINTER_TAG_I64;
use crate::types::{DOUBLE, F32, I32, I64, I8};

use super::artifact::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
use super::rep::{LoweredValue, NativeRep, SemanticKind};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaterializationReason {
    FunctionAbi,
    ReturnAbi,
    GenericCall,
    DynamicPropertyAccess,
    ExceptionPath,
    RuntimeApi,
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
            matches!(rep, NativeRep::U64 | NativeRep::USize)
        }
        NativeAbiTransitionOp::None
        | NativeAbiTransitionOp::FloatExtend
        | NativeAbiTransitionOp::PointerBox
        | NativeAbiTransitionOp::NativeHandleBox
        | NativeAbiTransitionOp::PromiseBox => false,
    }
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
    let transition = NativeAbiTransitionRecord {
        from_native_rep,
        to_native_rep: NativeRep::JsValue.name().to_string(),
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

pub(crate) fn materialize_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    if matches!(&lowered.rep, NativeRep::JsValue) {
        return lowered.value;
    }
    if matches!(&lowered.rep, NativeRep::NativeHandle) {
        return materialize_native_handle_to_js_value(ctx, lowered, reason);
    }
    if matches!(&lowered.rep, NativeRep::PromiseBoundary) {
        return materialize_promise_boundary_to_js_value(ctx, lowered, reason);
    }
    let from_native_rep = lowered.rep.name().to_string();
    let conversion_op = match &lowered.rep {
        NativeRep::I32 | NativeRep::I64 => NativeAbiTransitionOp::SignedIntToFloat,
        NativeRep::U8
        | NativeRep::U32
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::BufferLen => NativeAbiTransitionOp::UnsignedIntToFloat,
        NativeRep::F32 => NativeAbiTransitionOp::FloatExtend,
        NativeRep::F64 => NativeAbiTransitionOp::None,
        NativeRep::BufferView(_)
        | NativeRep::PodRecord { .. }
        | NativeRep::JsValue
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary => NativeAbiTransitionOp::None,
    };
    let lossy = transition_lossy(&lowered.rep, &conversion_op);
    let value = match &lowered.rep {
        NativeRep::I32 => ctx.block().sitofp(I32, &lowered.value, DOUBLE),
        NativeRep::I64 => ctx.block().sitofp(I64, &lowered.value, DOUBLE),
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            ctx.block().uitofp(I32, &widened, DOUBLE)
        }
        NativeRep::U32 => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::U64 | NativeRep::USize => ctx.block().uitofp(I64, &lowered.value, DOUBLE),
        NativeRep::BufferLen => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::F32 => ctx.block().fpext(F32, &lowered.value, DOUBLE),
        NativeRep::BufferView(_) => lowered.value.clone(),
        NativeRep::PodRecord { .. } => lowered.value.clone(),
        NativeRep::JsValue
        | NativeRep::F64
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary => lowered.value.clone(),
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
