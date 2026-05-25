use serde::Serialize;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I8};

use super::artifact::{ScalarConversionOp, ScalarConversionRecord};
use super::rep::{LoweredValue, NativeRep};

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
}

pub(crate) fn materialize_js_value(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
    reason: MaterializationReason,
) -> String {
    if matches!(&lowered.rep, NativeRep::JsValue) {
        return lowered.value;
    }
    let from_native_rep = lowered.rep.name().to_string();
    let conversion_op = match &lowered.rep {
        NativeRep::I32 => ScalarConversionOp::SignedIntToFloat,
        NativeRep::U8 | NativeRep::U32 => ScalarConversionOp::UnsignedIntToFloat,
        NativeRep::F64 => ScalarConversionOp::None,
        NativeRep::BufferView(_) | NativeRep::JsValue => ScalarConversionOp::None,
    };
    let value = match &lowered.rep {
        NativeRep::I32 => ctx.block().sitofp(I32, &lowered.value, DOUBLE),
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            ctx.block().uitofp(I32, &widened, DOUBLE)
        }
        NativeRep::U32 => ctx.block().uitofp(I32, &lowered.value, DOUBLE),
        NativeRep::BufferView(_) => lowered.value.clone(),
        NativeRep::JsValue | NativeRep::F64 => lowered.value.clone(),
    };
    let materialized = LoweredValue {
        semantic: lowered.semantic,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_conversion(
        "materialize_js_value",
        None,
        "materialize_js_value",
        &materialized,
        None,
        None,
        None,
        Some(reason.clone()),
        Some(ScalarConversionRecord {
            from_native_rep,
            to_native_rep: NativeRep::JsValue.name().to_string(),
            op: conversion_op,
            reason,
        }),
        false,
        false,
        Vec::new(),
    );
    value
}
