use serde::Serialize;

use super::rep::LoweredValue;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferElem {
    U8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundsProof {
    LoopGuard,
    MinLength,
    ExplicitGuard,
    ExplicitAssume,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundsState {
    Unknown,
    Proven { proof: BoundsProof },
    Guarded { guard_id: String },
}

impl BoundsState {
    pub(crate) fn allows_inbounds(&self) -> bool {
        matches!(self, Self::Proven { .. } | Self::Guarded { .. })
    }

    pub(crate) fn uses_unsound_explicit_assume_guard(&self) -> bool {
        match self {
            Self::Proven {
                proof: BoundsProof::ExplicitAssume,
            } => true,
            Self::Guarded { guard_id } => guard_id == "explicit_assume",
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AliasState {
    Unknown,
    MayAlias,
    NoAliasProven,
    NoAliasGuarded { guard_id: String },
}

impl AliasState {
    pub(crate) fn allows_noalias(&self) -> bool {
        matches!(self, Self::NoAliasProven | Self::NoAliasGuarded { .. })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferAccessMode {
    UncheckedNative,
    CheckedNative,
    DynamicFallback,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) struct BufferViewRep {
    pub data_ptr: String,
    pub length: String,
    pub elem: BufferElem,
    pub bounds: BoundsState,
    pub alias: AliasState,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferViewSlot {
    pub data_slot: String,
    pub scope_idx: Option<u32>,
    pub elem: BufferElem,
    pub alias: AliasState,
    pub length_source: Option<LengthSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LengthSource {
    Local { id: u32, addend: i64 },
    Constant(i64),
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundedBufferIndex {
    pub index_local_id: u32,
    pub buffer_local_id: u32,
    pub scope_id: u32,
    pub bounds: BoundsState,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferAccessProof {
    pub buffer_local_id: u32,
    pub view: BufferViewSlot,
    pub index: LoweredValue,
    pub access_mode: BufferAccessMode,
    pub bounds: BoundsState,
    pub alias: AliasState,
    pub may_emit_inbounds: bool,
    pub may_emit_noalias: bool,
}
