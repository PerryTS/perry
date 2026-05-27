mod artifact;
mod buffer;
mod materialize;
mod pod;
mod rep;
mod verify;

pub(crate) use artifact::{
    write_native_rep_artifact_if_enabled, NativeAbiDirection, NativeAbiTypeRecord, NativeFactUse,
    NativeRepRecord, NativeValueState, PodLayoutField, PodLayoutManifest, ScalarConversionRecord,
};
pub(crate) use buffer::{
    AliasState, BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessFacts, BufferAccessMode,
    BufferAccessProof, BufferElem, BufferEndian, BufferIndexUnit, BufferViewRep, BufferViewSlot,
    GuardedBufferIndex, LengthSource, NativeOwnedViewFact, NativeOwnedViewSlot,
};
pub(crate) use materialize::{
    materialize_js_value, materialize_native_handle_to_js_value,
    materialize_promise_boundary_to_js_value, record_runtime_native_handle_box_transition,
    MaterializationReason,
};
pub(crate) use pod::{
    collect_pod_init_fields, field_expected_rep, layout_decision_for_type,
    llvm_type_for_native_rep, validate_exact_init, PodLayoutDecision, PodLocal,
};
pub(crate) use rep::{ExpectedNativeRep, LoweredValue, NativeRep, SemanticKind};
pub(crate) use verify::verify_native_rep_records;
