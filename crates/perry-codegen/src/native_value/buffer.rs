use serde::Serialize;

use super::rep::LoweredValue;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferElem {
    I8,
    U8,
    U8Clamped,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferIndexUnit {
    Byte,
    Element,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BufferEndian {
    Native,
    Little,
    Big,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct BufferAccessFacts {
    pub access_width_bytes: u32,
    pub index_unit: BufferIndexUnit,
    pub element_width_bytes: u32,
    pub endian: BufferEndian,
    pub signed: bool,
    pub floating: bool,
    pub bounds_width_units: u32,
    pub view_length: Option<String>,
    pub view_byte_offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BoundsProof {
    LoopGuard,
    MinLength,
    ExplicitGuard,
    // #854: bounds-proof variant matched by uses_unsound_explicit_assume_guard
    // but not yet constructed by any proof emitter; kept as part of the
    // serialized BoundsProof contract.
    #[allow(dead_code)]
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
    // #854: alias-state variant matched by allows_noalias but not yet
    // constructed by any alias-guard emitter; kept as part of the serialized
    // AliasState contract.
    #[allow(dead_code)]
    NoAliasGuarded {
        guard_id: String,
    },
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
    pub element_width_bytes: u32,
    pub index_unit: BufferIndexUnit,
    pub view_byte_offset: Option<i64>,
    pub length_offset_from_data: i32,
    pub bounds: BoundsState,
    pub alias: AliasState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NativeOwnedViewFact {
    pub owner_local_id: u32,
    pub owner_root_state: String,
    pub disposed_state: String,
    pub byte_offset: Option<i64>,
    pub byte_length: Option<i64>,
    pub element_width_bytes: u32,
    pub alias_group: String,
    pub pointer_free_backing: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct NativeOwnedViewSlot {
    pub owner_local_id: u32,
    pub byte_offset: Option<i64>,
    pub byte_length: Option<i64>,
    pub owner_rooted: bool,
    pub disposed: bool,
    pub pointer_free_backing: bool,
}

impl NativeOwnedViewSlot {
    pub(crate) fn fact(
        &self,
        element_width_bytes: u32,
        alias_group: String,
    ) -> NativeOwnedViewFact {
        NativeOwnedViewFact {
            owner_local_id: self.owner_local_id,
            owner_root_state: if self.owner_rooted {
                "rooted"
            } else {
                "missing"
            }
            .to_string(),
            disposed_state: if self.disposed { "disposed" } else { "alive" }.to_string(),
            byte_offset: self.byte_offset,
            byte_length: self.byte_length,
            element_width_bytes,
            alias_group,
            pointer_free_backing: self.pointer_free_backing,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BufferViewSlot {
    pub data_slot: String,
    pub length_slot: Option<String>,
    pub scope_idx: Option<u32>,
    pub elem: BufferElem,
    pub element_width_bytes: u32,
    pub index_unit: BufferIndexUnit,
    pub view_byte_offset: Option<i64>,
    pub length_offset_from_data: i32,
    pub alias: AliasState,
    pub length_source: Option<LengthSource>,
    pub native_owned: Option<NativeOwnedViewSlot>,
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
    pub bounds_width_units: u32,
    pub bounds: BoundsState,
}

#[derive(Debug, Clone)]
pub(crate) struct GuardedBufferIndex {
    pub index_local_id: u32,
    pub buffer_local_id: u32,
    pub scope_id: u32,
    pub bounds_width_units: u32,
    pub guard_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct BufferAccessProof {
    pub buffer_local_id: u32,
    pub view: BufferViewSlot,
    pub index: LoweredValue,
    pub access_mode: BufferAccessMode,
    pub bounds: BoundsState,
    // #854: in-progress buffer-access fact bundle (Debug field); populated path
    // not yet wired, no consumer reads it.
    #[allow(dead_code)]
    pub facts: BufferAccessFacts,
    pub alias: AliasState,
    pub may_emit_inbounds: bool,
    pub may_emit_noalias: bool,
}
