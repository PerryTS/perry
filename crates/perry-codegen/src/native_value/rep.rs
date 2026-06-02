use serde::Serialize;

use crate::types::{LlvmType, DOUBLE, F32, I32, I64, I8, PTR};

use super::buffer::{AliasState, BoundsState, BufferElem, BufferIndexUnit, BufferViewRep};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SemanticKind {
    JsNumber,
    JsValue,
    TypedArrayElement,
    BufferObject,
    PodRecord,
    PodRecordView,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub(crate) enum NativeRep {
    JsValue,
    I32,
    /// Legacy signed 64-bit scalar. Kept for existing native-library
    /// manifests that declare `"i64"` and expect a JS-number bridge.
    I64,
    /// Unsigned 32-bit scalar. LLVM carries this as `i32`; consumers must
    /// preserve unsigned semantics explicitly, e.g. `uitofp` at JS-number
    /// materialization boundaries.
    U32,
    /// Unsigned 64-bit scalar. LLVM carries this as `i64`; conversion to a
    /// JS number is explicit and may lose precision above 2^53.
    U64,
    /// Native `usize` on Perry's supported 64-bit native runtime targets.
    USize,
    F64,
    /// Native/storage-only 32-bit float. It may be region-local, but JS-visible
    /// number boundaries must materialize through an explicit `fpext`.
    F32,
    U8,
    /// BufferHeader.length. The runtime layout is `u32`, so LLVM carries this
    /// as `i32` with unsigned conversion semantics at JS boundaries.
    BufferLen,
    /// Pointer-free native handle id stored inside POD bytes. LLVM carries it
    /// as `i64`; unlike NativeHandle this is not a raw pointer contract.
    HandleId,
    /// Raw native handle/pointer-sized integer. Region-local unless boxed by a
    /// dedicated boundary transition.
    NativeHandle,
    /// Raw promise handle at an async/native boundary. Region-local unless
    /// boxed by a dedicated promise-boundary transition.
    PromiseBoundary,
    /// Region-local view over buffer bytes. This is not a JS pointer contract:
    /// it may be consumed only inside the native region that proved its bounds
    /// and alias facts.
    BufferView(BufferViewRep),
    /// Region-local native stack storage for an exact closed POD record. The
    /// artifact carries the verifier-owned C layout manifest.
    PodRecord {
        layout_id: String,
        size: u32,
        alignment: u32,
    },
    /// Region-local view over native-arena-backed packed POD records. The
    /// pointer value is the first record byte; the paired ABI slot carries the
    /// record count.
    PodRecordView {
        layout_id: String,
        stride: u32,
        alignment: u32,
    },
}

impl NativeRep {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::JsValue => "js_value",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::USize => "usize",
            Self::F64 => "f64",
            Self::F32 => "f32",
            Self::U8 => "u8",
            Self::BufferLen => "buffer_len",
            Self::HandleId => "handle_id",
            Self::NativeHandle => "native_handle",
            Self::PromiseBoundary => "promise_boundary",
            Self::BufferView(_) => "buffer_view",
            Self::PodRecord { .. } => "pod_record",
            Self::PodRecordView { .. } => "pod_record_view",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedNativeRep {
    I32,
    I64,
    U32,
    U64,
    USize,
    F64,
    F32,
    BufferLen,
    HandleId,
    // #854: expected-rep variants matched by is_rep but not yet constructed by
    // any ABI classifier; kept as part of the native-rep expectation taxonomy.
    #[allow(dead_code)]
    NativeHandle,
    #[allow(dead_code)]
    PromiseBoundary,
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

    pub(crate) fn i64(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::I64, I64, value)
    }

    pub(crate) fn u32(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::U32, I32, value)
    }

    pub(crate) fn u64(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::U64, I64, value)
    }

    pub(crate) fn usize(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::USize, I64, value)
    }

    pub(crate) fn u8(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::TypedArrayElement, NativeRep::U8, I8, value)
    }

    pub(crate) fn f64(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::F64, DOUBLE, value)
    }

    pub(crate) fn f32(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::F32, F32, value)
    }

    pub(crate) fn buffer_len(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::BufferLen, I32, value)
    }

    pub(crate) fn handle_id(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsNumber, NativeRep::HandleId, I64, value)
    }

    pub(crate) fn js_value(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsValue, NativeRep::JsValue, DOUBLE, value)
    }

    pub(crate) fn native_handle(value: impl Into<String>) -> Self {
        Self::new(SemanticKind::JsValue, NativeRep::NativeHandle, I64, value)
    }

    pub(crate) fn promise_boundary(value: impl Into<String>) -> Self {
        Self::new(
            SemanticKind::JsValue,
            NativeRep::PromiseBoundary,
            I64,
            value,
        )
    }

    pub(crate) fn buffer_view(
        data_ptr: impl Into<String>,
        length: impl Into<String>,
        elem: BufferElem,
        element_width_bytes: u32,
        index_unit: BufferIndexUnit,
        view_byte_offset: Option<i64>,
        length_offset_from_data: i32,
        bounds: BoundsState,
        alias: AliasState,
    ) -> Self {
        let data_ptr = data_ptr.into();
        Self::new(
            SemanticKind::BufferObject,
            NativeRep::BufferView(BufferViewRep {
                data_ptr: data_ptr.clone(),
                length: length.into(),
                elem,
                element_width_bytes,
                index_unit,
                view_byte_offset,
                length_offset_from_data,
                bounds,
                alias,
            }),
            PTR,
            data_ptr,
        )
    }

    // #854: near-future ABI rep-match predicate; not yet called by a codegen
    // dispatch site.
    #[allow(dead_code)]
    pub(crate) fn is_rep(&self, expected: ExpectedNativeRep) -> bool {
        matches!(
            (expected, &self.rep),
            (ExpectedNativeRep::I32, NativeRep::I32)
                | (ExpectedNativeRep::I64, NativeRep::I64)
                | (ExpectedNativeRep::U32, NativeRep::U32)
                | (ExpectedNativeRep::U64, NativeRep::U64)
                | (ExpectedNativeRep::USize, NativeRep::USize)
                | (ExpectedNativeRep::F64, NativeRep::F64)
                | (ExpectedNativeRep::F32, NativeRep::F32)
                | (ExpectedNativeRep::BufferLen, NativeRep::BufferLen)
                | (ExpectedNativeRep::HandleId, NativeRep::HandleId)
                | (ExpectedNativeRep::NativeHandle, NativeRep::NativeHandle)
                | (
                    ExpectedNativeRep::PromiseBoundary,
                    NativeRep::PromiseBoundary
                )
        )
    }
}
