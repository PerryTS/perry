mod artifact;
mod buffer;
mod materialize;
mod rep;
mod verify;

pub(crate) use artifact::{write_native_rep_artifact_if_enabled, NativeRepRecord};
pub(crate) use buffer::{
    AliasState, BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessMode, BufferAccessProof,
    BufferElem, BufferViewRep, BufferViewSlot, LengthSource,
};
pub(crate) use materialize::{materialize_js_value, MaterializationReason};
pub(crate) use rep::{ExpectedNativeRep, LoweredValue, NativeRep, SemanticKind};
pub(crate) use verify::verify_native_rep_records;
