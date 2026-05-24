use serde::Serialize;

use crate::types::LlvmType;

use super::buffer::BufferViewRep;

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
    U32,
    F64,
    U8,
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LoweredValue {
    pub semantic: SemanticKind,
    pub rep: NativeRep,
    pub llvm_ty: LlvmType,
    pub value: String,
}
