use serde::Serialize;

use crate::types::{LlvmType, DOUBLE, I32, I8, PTR};

use super::buffer::{AliasState, BoundsState, BufferElem, BufferViewRep};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticKind {
    JsNumber,
    JsValue,
    TypedArrayElement,
    BufferObject,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub(crate) enum NativeRep {
    JsValue,
    I32,
    /// Unsigned 32-bit scalar. LLVM carries this as `i32`; consumers must
    /// preserve unsigned semantics explicitly, e.g. `uitofp` at JS-number
    /// materialization boundaries.
    U32,
    F64,
    U8,
    /// Region-local view over buffer bytes. This is not a JS pointer contract:
    /// it may be consumed only inside the native region that proved its bounds
    /// and alias facts.
    BufferView(BufferViewRep),
}

impl NativeRep {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::JsValue => "js_value",
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::F64 => "f64",
            Self::U8 => "u8",
            Self::BufferView(_) => "buffer_view",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedNativeRep {
    I32,
    U32,
    F64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LoweredValue {
    pub semantic: SemanticKind,
    pub rep: NativeRep,
    pub llvm_ty: LlvmType,
    pub value: String,
}

impl LoweredValue {
    pub(crate) fn new(
        semantic: SemanticKind,
        rep: NativeRep,
        llvm_ty: LlvmType,
        value: impl Into<String>,
    ) -> Self {
        Self {
            semantic,
            rep,
            llvm_ty,
            value: value.into(),
        }
    }

    pub(crate) fn i32(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::I32, I32, value)
    }

    pub(crate) fn u32(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::U32, I32, value)
    }

    pub(crate) fn u8(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::TypedArrayElement, NativeRep::U8, I8, value)
    }

    pub(crate) fn f64(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::F64, DOUBLE, value)
    }

    pub(crate) fn js_value(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsValue, NativeRep::JsValue, DOUBLE, value)
    }

    pub(crate) fn buffer_view(
        data_ptr: impl Into<String>,
        length: impl Into<String>,
        bounds: BoundsState,
        alias: AliasState,
    ) -> Self {
        let data_ptr = data_ptr.into();
        Self::new(
            SemanticKind::BufferObject,
            NativeRep::BufferView(BufferViewRep {
                data_ptr: data_ptr.clone(),
                length: length.into(),
                elem: BufferElem::U8,
                bounds,
                alias,
            }),
            PTR,
            data_ptr,
        )
    }

    pub(crate) fn is_rep(&self, expected: ExpectedNativeRep) -> bool {
        matches!(
            (expected, &self.rep),
            (ExpectedNativeRep::I32, NativeRep::I32)
                | (ExpectedNativeRep::U32, NativeRep::U32)
                | (ExpectedNativeRep::F64, NativeRep::F64)
        )
    }
}

// Deliberately absent for now:
// - `I64`: JS-visible integer precision and BigInt allocation need an explicit
//   boxing/materialization contract before 64-bit scalars may escape a region.
// - Native pointer reps: raw pointers must stay region-local until ownership,
//   lifetime, aliasing, and NaN-boxing/materialization rules are specified.
