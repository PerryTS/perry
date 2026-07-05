mod artifact;
mod buffer;
mod materialize;
mod pod;
mod rep;
mod verify;

pub(crate) use artifact::{
    typed_clone_rejection_record, write_native_rep_artifact_if_enabled, NativeAbiDirection,
    NativeAbiTypeRecord, NativeFactUse, NativeRepRecord, NativeValueState, PodLayoutField,
    PodLayoutManifest, PodRecordViewManifest, ScalarConversionRecord,
};
pub(crate) use buffer::{
    AliasState, BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessFacts, BufferAccessMode,
    BufferAccessProof, BufferElem, BufferEndian, BufferIndexUnit, BufferViewSlot,
    GuardedBufferIndex, LengthSource, NativeOwnedViewFact, NativeOwnedViewSlot,
};
pub(crate) use materialize::{
    materialize_js_value, materialize_js_value_bits, materialize_js_value_without_record,
    materialize_native_handle_to_js_value, materialize_promise_boundary_to_js_value,
    materialize_small_bigint_pointer_to_js_value, record_runtime_native_handle_box_transition,
    MaterializationReason,
};
pub(crate) use pod::{
    collect_pod_init_fields, field_expected_rep, layout_decision_for_type, layout_for_manifest_pod,
    layout_for_pod_view_type, layout_runtime_id, llvm_type_for_native_rep, validate_exact_init,
    PodLayoutDecision, PodLocal, PodViewLocal,
};
pub(crate) use rep::{ExpectedNativeRep, LoweredValue, NativeRep, SemanticKind};
pub(crate) use verify::verify_native_rep_records;
// Re-exported for the `verify` test module, which constructs `NativeRep::BufferView`
// records via the `crate::native_value::BufferViewRep` path. Non-test code reaches the
// type through `super::buffer::BufferViewRep` directly, so gate this to the test build.
#[cfg(test)]
pub(crate) use buffer::BufferViewRep;
