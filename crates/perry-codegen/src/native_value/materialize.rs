use serde::Serialize;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32, I8};

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
    if matches!(lowered.rep, NativeRep::JsValue | NativeRep::F64) {
        return lowered.value;
    }
    let value = match lowered.rep {
        NativeRep::I32 => ctx.block().sitofp(I32, &lowered.value, DOUBLE),
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            ctx.block().sitofp(I32, &widened, DOUBLE)
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
    ctx.record_lowered_value(
        "materialize_js_value",
        None,
        "materialize_js_value",
        &materialized,
        None,
        None,
        Some(reason),
        false,
        false,
        Vec::new(),
    );
    value
}
