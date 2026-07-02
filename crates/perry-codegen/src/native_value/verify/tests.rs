use super::{NativeAbiTransitionOp, NativeAbiTransitionRecord};
use crate::native_value::{
    verify_native_rep_records, AliasState, BoundsProof, BoundsState, BufferAccessMode,
    BufferViewRep, LoweredValue, MaterializationReason, NativeAbiDirection, NativeAbiTypeRecord,
    NativeFactUse, NativeRep, NativeRepRecord, NativeValueState, SemanticKind,
};
use crate::types::{DOUBLE, F32, I32, I64, PTR};

fn record() -> NativeRepRecord {
    let lowered = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::I32,
        llvm_ty: I32,
        value: "%r1".to_string(),
    };
    NativeRepRecord {
        function: "f".to_string(),
        block_label: "entry".to_string(),
        region_id: None,
        source_function: "f".to_string(),
        lowering_block: "entry".to_string(),
        local_id: None,
        expr_kind: "test".to_string(),
        source_key: None,
        semantic: lowered.semantic,
        native_rep_name: lowered.rep.name().to_string(),
        native_rep: lowered.rep,
        llvm_ty: lowered.llvm_ty,
        llvm_value: lowered.value,
        consumer: "test".to_string(),
        bounds_state: None,
        alias_state: None,
        access_mode: None,
        buffer_access: None,
        native_owned_view: None,
        materialization_reason: None,
        fallback_reason: None,
        native_value_state: NativeValueState::RegionLocal,
        native_abi_transition: None,
        scalar_conversion: None,
        native_abi_type: None,
        pod_layout: None,
        pod_record_view: None,
        consumed_facts: Vec::new(),
        rejected_facts: Vec::new(),
        emitted_inbounds: false,
        emitted_noalias: false,
        notes: Vec::new(),
    }
}

fn raw_f64_layout_fact(state: &str, reason: Option<MaterializationReason>) -> NativeFactUse {
    NativeFactUse {
        fact_id: format!("test.raw_f64_layout.{state}"),
        kind: "raw_f64_layout".to_string(),
        local_id: None,
        state: state.to_string(),
        detail: state.to_string(),
        reason,
    }
}

fn type_fact(state: &str, detail: &str, reason: Option<MaterializationReason>) -> NativeFactUse {
    NativeFactUse {
        fact_id: format!("test.type_fact.{state}.{detail}"),
        kind: "type_fact".to_string(),
        local_id: Some(1),
        state: state.to_string(),
        detail: detail.to_string(),
        reason,
    }
}

#[test]
fn verifier_accepts_structured_consumed_and_rejected_facts() {
    let mut r = record();
    r.consumed_facts
        .push(type_fact("consumed", "packed_i32", None));
    r.rejected_facts.push(type_fact(
        "rejected",
        "unknown_call_escape",
        Some(MaterializationReason::UnknownCallEscape),
    ));

    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn verifier_rejects_malformed_fact_uses() {
    let mut r = record();
    r.consumed_facts.push(NativeFactUse {
        fact_id: String::new(),
        kind: "type_fact".to_string(),
        local_id: Some(1),
        state: "consumed".to_string(),
        detail: "packed_i32".to_string(),
        reason: None,
    });
    r.rejected_facts.push(NativeFactUse {
        fact_id: "test.type_fact.rejected".to_string(),
        kind: "type_fact".to_string(),
        local_id: Some(1),
        state: "guard_failed".to_string(),
        detail: String::new(),
        reason: None,
    });

    let err = verify_native_rep_records(&[r]).expect_err("malformed facts should fail");
    let text = err.to_string();
    assert!(text.contains("empty fact_id"));
    assert!(text.contains("lacks reason/detail"));
}

fn packed_f64_loop_store_record() -> NativeRepRecord {
    let mut r = record();
    r.expr_kind = "PackedF64LoopStore".to_string();
    r.consumer = "packed_f64_loop_store".to_string();
    r.native_rep = NativeRep::F64;
    r.native_rep_name = "f64".to_string();
    r.llvm_ty = DOUBLE;
    r.access_mode = Some(BufferAccessMode::CheckedNative);
    r.bounds_state = Some(BoundsState::Guarded {
        guard_id: "packed_f64_array_loop_guard".to_string(),
    });
    r.consumed_facts.push(raw_f64_layout_fact("consumed", None));
    r
}

#[test]
fn verifier_accepts_packed_f64_loop_store_with_runtime_safety_notes() {
    let mut r = packed_f64_loop_store_record();
    r.notes = vec![
        "rhs_numeric_guard=js_typed_feedback_numeric_array_index_set_guard".to_string(),
        "raw_f64_canonicalized=js_array_numeric_value_to_raw_f64".to_string(),
        "array_reloaded_after_rhs=1".to_string(),
        "array_reloaded_after_store_guard=1".to_string(),
        "array_reloaded_after_canonicalization=1".to_string(),
        "index_range=nonnegative_i32".to_string(),
        "length_range=guarded_i32".to_string(),
    ];
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn verifier_rejects_packed_f64_loop_store_without_canonicalization_notes() {
    let mut r = packed_f64_loop_store_record();
    r.notes = vec![
        "index_range=nonnegative_i32".to_string(),
        "length_range=guarded_i32".to_string(),
    ];
    let err = verify_native_rep_records(&[r]).expect_err("missing packed store notes");
    assert!(
        err.to_string()
            .contains("packed-f64 loop store missing raw_f64_canonicalized"),
        "{err}"
    );
}

fn pod_layout() -> crate::native_value::PodLayoutManifest {
    super::recompute_layout_from_fields(
        "pod_test".to_string(),
        &[
            ("tag".to_string(), NativeRep::U32),
            ("gain".to_string(), NativeRep::F32),
            ("total".to_string(), NativeRep::F64),
            ("count".to_string(), NativeRep::BufferLen),
        ],
    )
    .unwrap()
}

fn pod_record(layout: crate::native_value::PodLayoutManifest) -> NativeRepRecord {
    let mut r = record();
    r.semantic = SemanticKind::PodRecord;
    r.native_rep = NativeRep::PodRecord {
        layout_id: layout.layout_id.clone(),
        size: layout.size,
        alignment: layout.alignment,
    };
    r.native_rep_name = "pod_record".to_string();
    r.llvm_ty = PTR;
    r.llvm_value = "%pod".to_string();
    r.pod_layout = Some(layout);
    r
}

fn pod_record_view(layout: crate::native_value::PodLayoutManifest) -> NativeRepRecord {
    let mut r = record();
    r.semantic = SemanticKind::PodRecordView;
    r.native_rep = NativeRep::PodRecordView {
        layout_id: layout.layout_id.clone(),
        stride: layout.size,
        alignment: layout.alignment,
    };
    r.native_rep_name = "pod_record_view".to_string();
    r.llvm_ty = PTR;
    r.llvm_value = "%data".to_string();
    r.pod_layout = Some(layout.clone());
    r.pod_record_view = Some(crate::native_value::PodRecordViewManifest {
        layout_id: layout.layout_id.clone(),
        stride: layout.size,
        alignment: layout.alignment,
        count_source: "constant:4".to_string(),
        pointer_free_backing: true,
        endian: "native".to_string(),
        packing: "c".to_string(),
    });
    r
}

fn abi_type(
    descriptor: &str,
    direction: NativeAbiDirection,
    js_argument_index: Option<usize>,
    abi_slot_index: usize,
) -> NativeAbiTypeRecord {
    let descriptor = perry_api_manifest::NativeAbiType::parse_str(descriptor).unwrap();
    NativeAbiTypeRecord::new(&descriptor, direction, js_argument_index, abi_slot_index)
}

fn guarded_abi_type(
    descriptor: &str,
    direction: NativeAbiDirection,
    js_argument_index: Option<usize>,
    abi_slot_index: usize,
    helper: &str,
) -> NativeAbiTypeRecord {
    abi_type(descriptor, direction, js_argument_index, abi_slot_index)
        .with_runtime_guard(helper, "test_requirement")
}

fn pod_abi_type(
    direction: NativeAbiDirection,
    js_argument_index: Option<usize>,
    abi_slot_index: usize,
) -> NativeAbiTypeRecord {
    let descriptor = perry_api_manifest::NativeAbiType::Pod(perry_api_manifest::NativePodAbi {
        name: Some("Packet".to_string()),
        fields: vec![
            perry_api_manifest::NativePodFieldAbi {
                name: "tag".to_string(),
                ty: perry_api_manifest::NativeAbiType::U32,
            },
            perry_api_manifest::NativePodFieldAbi {
                name: "gain".to_string(),
                ty: perry_api_manifest::NativeAbiType::F32,
            },
            perry_api_manifest::NativePodFieldAbi {
                name: "total".to_string(),
                ty: perry_api_manifest::NativeAbiType::F64,
            },
            perry_api_manifest::NativePodFieldAbi {
                name: "count".to_string(),
                ty: perry_api_manifest::NativeAbiType::BufferLen,
            },
        ],
    });
    NativeAbiTypeRecord::new(&descriptor, direction, js_argument_index, abi_slot_index)
}

fn pod_count_abi_type(
    direction: NativeAbiDirection,
    js_argument_index: Option<usize>,
    abi_slot_index: usize,
    helper: &str,
) -> NativeAbiTypeRecord {
    let descriptor =
        perry_api_manifest::NativeAbiType::PodAndCount(perry_api_manifest::NativePodAbi {
            name: Some("PacketBatch".to_string()),
            fields: vec![
                perry_api_manifest::NativePodFieldAbi {
                    name: "tag".to_string(),
                    ty: perry_api_manifest::NativeAbiType::U32,
                },
                perry_api_manifest::NativePodFieldAbi {
                    name: "gain".to_string(),
                    ty: perry_api_manifest::NativeAbiType::F32,
                },
                perry_api_manifest::NativePodFieldAbi {
                    name: "total".to_string(),
                    ty: perry_api_manifest::NativeAbiType::F64,
                },
                perry_api_manifest::NativePodFieldAbi {
                    name: "count".to_string(),
                    ty: perry_api_manifest::NativeAbiType::BufferLen,
                },
            ],
        });
    NativeAbiTypeRecord::new(&descriptor, direction, js_argument_index, abi_slot_index)
        .with_runtime_guard(helper, "test_requirement")
}

#[test]
fn fails_unsafe_inbounds_without_artifact_output() {
    let mut r = record();
    r.emitted_inbounds = true;
    r.bounds_state = Some(BoundsState::Unknown);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn fails_unsafe_noalias_without_artifact_output() {
    let mut r = record();
    r.emitted_noalias = true;
    r.alias_state = Some(AliasState::MayAlias);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn fails_explicit_assume_guard_without_artifact_output() {
    let mut r = record();
    r.bounds_state = Some(BoundsState::Proven {
        proof: BoundsProof::ExplicitAssume,
    });
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_proven_bounds_and_noalias() {
    let mut r = record();
    r.emitted_inbounds = true;
    r.emitted_noalias = true;
    r.bounds_state = Some(BoundsState::Proven {
        proof: BoundsProof::MinLength,
    });
    r.alias_state = Some(AliasState::NoAliasProven);
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn fails_unchecked_native_unknown_bounds_without_artifact_output() {
    let mut r = record();
    r.access_mode = Some(BufferAccessMode::UncheckedNative);
    r.bounds_state = Some(BoundsState::Unknown);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_dynamic_fallback_unknown_bounds() {
    let mut r = record();
    r.access_mode = Some(BufferAccessMode::DynamicFallback);
    r.bounds_state = Some(BoundsState::Unknown);
    r.materialization_reason = Some(crate::native_value::MaterializationReason::UnknownBounds);
    r.fallback_reason = Some(crate::native_value::MaterializationReason::UnknownBounds);
    r.native_value_state = NativeValueState::DynamicFallback;
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn accepts_unchecked_native_proven_and_guarded_bounds() {
    let mut proven = record();
    proven.access_mode = Some(BufferAccessMode::UncheckedNative);
    proven.bounds_state = Some(BoundsState::Proven {
        proof: BoundsProof::MinLength,
    });
    let mut guarded = record();
    guarded.access_mode = Some(BufferAccessMode::UncheckedNative);
    guarded.bounds_state = Some(BoundsState::Guarded {
        guard_id: "loop_guard".to_string(),
    });
    assert!(verify_native_rep_records(&[proven, guarded]).is_ok());
}

#[test]
fn rejects_checked_native_without_real_bounds() {
    let mut r = record();
    r.access_mode = Some(BufferAccessMode::CheckedNative);
    r.bounds_state = Some(BoundsState::Unknown);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_raw_f64_checked_native_without_consumed_layout_fact() {
    for (expr_kind, consumer) in [
        ("NumericArrayIndexGet", "js_array_numeric_get_f64_unboxed"),
        ("NumericArrayIndexSet", "js_array_numeric_set_f64_unboxed"),
        ("NumericArrayPush", "js_array_numeric_push_f64_unboxed"),
        ("ClassFieldGet", "class_field_get.raw_f64_load"),
        ("ClassFieldSet", "class_field_set.raw_f64_store"),
    ] {
        let mut r = record();
        r.expr_kind = expr_kind.to_string();
        r.consumer = consumer.to_string();
        r.semantic = SemanticKind::JsNumber;
        r.native_rep = NativeRep::F64;
        r.native_rep_name = "f64".to_string();
        r.llvm_ty = DOUBLE;
        r.access_mode = Some(BufferAccessMode::CheckedNative);
        r.bounds_state = Some(BoundsState::Guarded {
            guard_id: "raw_f64_guard".to_string(),
        });

        assert!(
            verify_native_rep_records(&[r.clone()]).is_err(),
            "{consumer} should require a consumed raw_f64_layout fact"
        );

        r.consumed_facts.push(raw_f64_layout_fact("consumed", None));
        assert!(
            verify_native_rep_records(&[r]).is_ok(),
            "{consumer} should verify once the consumed layout fact is present"
        );
    }
}

#[test]
fn rejects_raw_f64_dynamic_fallback_without_rejected_and_invalidated_layout_facts() {
    for (expr_kind, consumer) in [
        ("NumericArrayPush", "js_array_push_f64"),
        (
            "NumericArrayIndexGet",
            "js_typed_feedback_array_index_get_fallback_boxed",
        ),
        (
            "NumericArrayIndexSet",
            "js_typed_feedback_array_index_set_fallback_boxed",
        ),
        ("ClassFieldGet", "js_object_get_field_by_name_f64"),
        ("ClassFieldSet", "js_object_set_field_by_name"),
    ] {
        let mut r = record();
        r.expr_kind = expr_kind.to_string();
        r.consumer = consumer.to_string();
        r.semantic = SemanticKind::JsValue;
        r.native_rep = NativeRep::JsValue;
        r.native_rep_name = "js_value".to_string();
        r.llvm_ty = DOUBLE;
        r.access_mode = Some(BufferAccessMode::DynamicFallback);
        r.materialization_reason = Some(MaterializationReason::RuntimeApi);
        r.fallback_reason = Some(MaterializationReason::RuntimeApi);
        r.native_value_state = NativeValueState::DynamicFallback;

        assert!(
            verify_native_rep_records(&[r.clone()]).is_err(),
            "{consumer} should require rejected and invalidated raw_f64_layout facts"
        );

        r.rejected_facts.push(raw_f64_layout_fact(
            "rejected",
            Some(MaterializationReason::RuntimeApi),
        ));
        assert!(
            verify_native_rep_records(&[r.clone()]).is_err(),
            "{consumer} should still require invalidated raw_f64_layout fact"
        );

        r.rejected_facts.push(raw_f64_layout_fact(
            "invalidated",
            Some(MaterializationReason::RuntimeApi),
        ));
        assert!(
            verify_native_rep_records(&[r]).is_ok(),
            "{consumer} should verify once rejection and invalidation are recorded"
        );
    }
}

#[test]
fn accepts_new_region_local_native_abi_records() {
    let mut f64_record = record();
    f64_record.native_rep = NativeRep::F64;
    f64_record.native_rep_name = "f64".to_string();
    f64_record.llvm_ty = DOUBLE;
    f64_record.llvm_value = "%f".to_string();
    f64_record.native_abi_type = Some(abi_type("f64", NativeAbiDirection::Return, None, 0));

    let mut u32_record = record();
    u32_record.native_rep = NativeRep::U32;
    u32_record.native_rep_name = "u32".to_string();
    u32_record.llvm_ty = I32;
    u32_record.llvm_value = "%u".to_string();
    u32_record.native_abi_type = Some(guarded_abi_type(
        "u32",
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_u32",
    ));

    let mut u64_record = record();
    u64_record.native_rep = NativeRep::U64;
    u64_record.native_rep_name = "u64".to_string();
    u64_record.llvm_ty = I64;
    u64_record.llvm_value = "%u64".to_string();
    u64_record.native_abi_type = Some(guarded_abi_type(
        "u64",
        NativeAbiDirection::Param,
        Some(1),
        1,
        "js_native_abi_check_u64",
    ));

    let mut usize_record = record();
    usize_record.native_rep = NativeRep::USize;
    usize_record.native_rep_name = "usize".to_string();
    usize_record.llvm_ty = I64;
    usize_record.llvm_value = "%usize".to_string();
    usize_record.native_abi_type = Some(guarded_abi_type(
        "usize",
        NativeAbiDirection::Param,
        Some(2),
        2,
        "js_native_abi_check_usize",
    ));

    let mut f32_record = record();
    f32_record.native_rep = NativeRep::F32;
    f32_record.native_rep_name = "f32".to_string();
    f32_record.llvm_ty = F32;
    f32_record.llvm_value = "%f32".to_string();
    f32_record.native_abi_type = Some(guarded_abi_type(
        "f32",
        NativeAbiDirection::Param,
        Some(3),
        3,
        "js_native_abi_check_f32",
    ));

    let mut buffer_len_record = record();
    buffer_len_record.native_rep = NativeRep::BufferLen;
    buffer_len_record.native_rep_name = "buffer_len".to_string();
    buffer_len_record.llvm_ty = I32;
    buffer_len_record.llvm_value = "%len".to_string();
    buffer_len_record.native_abi_type = Some(guarded_abi_type(
        "buffer_len",
        NativeAbiDirection::Param,
        Some(4),
        4,
        "js_native_abi_check_u32",
    ));

    let mut handle_record = record();
    handle_record.native_rep = NativeRep::NativeHandle;
    handle_record.native_rep_name = "native_handle".to_string();
    handle_record.llvm_ty = I64;
    handle_record.llvm_value = "%handle".to_string();
    handle_record.native_abi_type = Some(guarded_abi_type(
        "handle<MyThing>",
        NativeAbiDirection::Param,
        Some(5),
        5,
        "js_native_handle_unwrap",
    ));

    let mut promise_record = record();
    promise_record.native_rep = NativeRep::PromiseBoundary;
    promise_record.native_rep_name = "promise_boundary".to_string();
    promise_record.llvm_ty = I64;
    promise_record.llvm_value = "%promise".to_string();
    promise_record.native_abi_type = Some(abi_type(
        "promise<f64>",
        NativeAbiDirection::Return,
        None,
        0,
    ));

    assert!(verify_native_rep_records(&[
        f64_record,
        u32_record,
        u64_record,
        usize_record,
        f32_record,
        buffer_len_record,
        handle_record,
        promise_record
    ])
    .is_ok());
}

#[test]
fn rejects_native_abi_descriptor_rep_mismatch() {
    let mut r = record();
    r.native_abi_type = Some(abi_type("f32", NativeAbiDirection::Param, Some(0), 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_native_abi_param_without_js_argument_index() {
    let mut r = record();
    r.native_abi_type = Some(abi_type("i32", NativeAbiDirection::Param, None, 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_manifest_param_missing_runtime_guard() {
    let mut r = record();
    r.native_abi_type = Some(abi_type("i32", NativeAbiDirection::Param, Some(0), 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_manifest_param_wrong_runtime_guard() {
    let mut r = record();
    r.native_abi_type = Some(guarded_abi_type(
        "i32",
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_u32",
    ));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_region_local_manifest_pod_param_without_runtime_guard() {
    let layout = pod_layout();
    let mut r = pod_record(layout);
    r.native_abi_type = Some(pod_abi_type(NativeAbiDirection::Param, Some(0), 0));
    r.notes.push("source=region_local_pod".to_string());
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn rejects_dynamic_manifest_pod_param_without_runtime_guard() {
    let layout = pod_layout();
    let mut r = pod_record(layout);
    r.native_abi_type = Some(pod_abi_type(NativeAbiDirection::Param, Some(0), 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_native_abi_return_with_js_argument_index() {
    let mut r = record();
    r.native_abi_type = Some(abi_type("i32", NativeAbiDirection::Return, Some(0), 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_unpaired_buffer_span_descriptor() {
    let mut r = record();
    r.native_rep = NativeRep::BufferView(BufferViewRep {
        data_ptr: "%ptr".to_string(),
        length: "%len".to_string(),
        elem: crate::native_value::BufferElem::U8,
        element_width_bytes: 1,
        index_unit: crate::native_value::BufferIndexUnit::Byte,
        view_byte_offset: Some(0),
        length_offset_from_data: 0,
        bounds: BoundsState::Unknown,
        alias: AliasState::Unknown,
    });
    r.native_rep_name = "buffer_view".to_string();
    r.llvm_ty = PTR;
    r.native_abi_type = Some(guarded_abi_type(
        "buffer+len",
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_buffer_data_ptr",
    ));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_paired_buffer_span_descriptor() {
    let mut ptr_record = record();
    ptr_record.native_rep = NativeRep::BufferView(BufferViewRep {
        data_ptr: "%ptr".to_string(),
        length: "%len".to_string(),
        elem: crate::native_value::BufferElem::U8,
        element_width_bytes: 1,
        index_unit: crate::native_value::BufferIndexUnit::Byte,
        view_byte_offset: Some(0),
        length_offset_from_data: 0,
        bounds: BoundsState::Unknown,
        alias: AliasState::Unknown,
    });
    ptr_record.native_rep_name = "buffer_view".to_string();
    ptr_record.llvm_ty = PTR;
    ptr_record.native_abi_type = Some(guarded_abi_type(
        "buffer+len",
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_buffer_data_ptr",
    ));

    let mut len_record = record();
    len_record.native_rep = NativeRep::USize;
    len_record.native_rep_name = "usize".to_string();
    len_record.llvm_ty = I64;
    len_record.llvm_value = "%len".to_string();
    len_record.native_abi_type = Some(guarded_abi_type(
        "buffer+len",
        NativeAbiDirection::Param,
        Some(0),
        1,
        "js_native_abi_check_buffer_byte_len",
    ));

    assert!(verify_native_rep_records(&[ptr_record, len_record]).is_ok());
}

#[test]
fn rejects_unpaired_pod_count_span_descriptor() {
    let layout = pod_layout();
    let mut r = pod_record_view(layout);
    r.native_abi_type = Some(pod_count_abi_type(
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_pod_view_data_ptr",
    ));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_paired_pod_count_span_descriptor() {
    let layout = pod_layout();
    let mut data_record = pod_record_view(layout.clone());
    data_record.native_abi_type = Some(pod_count_abi_type(
        NativeAbiDirection::Param,
        Some(0),
        0,
        "js_native_abi_check_pod_view_data_ptr",
    ));

    let mut count_record = record();
    count_record.native_rep = NativeRep::USize;
    count_record.native_rep_name = "usize".to_string();
    count_record.llvm_ty = I64;
    count_record.llvm_value = "%count".to_string();
    count_record.pod_layout = Some(layout.clone());
    count_record.pod_record_view = Some(crate::native_value::PodRecordViewManifest {
        layout_id: layout.layout_id.clone(),
        stride: layout.size,
        alignment: layout.alignment,
        count_source: "constant:4".to_string(),
        pointer_free_backing: true,
        endian: "native".to_string(),
        packing: "c".to_string(),
    });
    count_record.native_abi_type = Some(pod_count_abi_type(
        NativeAbiDirection::Param,
        Some(0),
        1,
        "js_native_abi_check_pod_view_record_count",
    ));

    assert!(verify_native_rep_records(&[data_record, count_record]).is_ok());
}

#[test]
fn rejects_pod_count_return_descriptor() {
    let layout = pod_layout();
    let mut r = pod_record_view(layout);
    r.native_abi_type = Some(pod_count_abi_type(
        NativeAbiDirection::Return,
        None,
        0,
        "js_native_abi_check_pod_view_data_ptr",
    ));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_buffer_and_len_return_descriptor() {
    let mut r = record();
    r.native_rep = NativeRep::BufferView(BufferViewRep {
        data_ptr: "%ptr".to_string(),
        length: "%len".to_string(),
        elem: crate::native_value::BufferElem::U8,
        element_width_bytes: 1,
        index_unit: crate::native_value::BufferIndexUnit::Byte,
        view_byte_offset: Some(0),
        length_offset_from_data: -8,
        bounds: BoundsState::Unknown,
        alias: AliasState::Unknown,
    });
    r.native_rep_name = "buffer_view".to_string();
    r.llvm_ty = PTR;
    r.native_abi_type = Some(abi_type("buffer+len", NativeAbiDirection::Return, None, 0));
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_pod_return_descriptor() {
    let layout = pod_layout();
    let mut r = pod_record(layout);
    r.native_abi_type = Some(pod_abi_type(NativeAbiDirection::Return, None, 0));

    let err = verify_native_rep_records(&[r]).expect_err("pod returns must reject");
    assert!(
        err.to_string().contains("pod cannot be a return type"),
        "{err}"
    );
}

#[test]
fn rejects_handle_abi_missing_native_handle_contract() {
    let mut r = record();
    r.native_rep = NativeRep::NativeHandle;
    r.native_rep_name = "native_handle".to_string();
    r.llvm_ty = I64;
    r.llvm_value = "%handle".to_string();
    r.native_abi_type = Some(abi_type(
        "handle<MyThing>",
        NativeAbiDirection::Param,
        Some(0),
        0,
    ));
    r.native_abi_type.as_mut().unwrap().native_handle = None;

    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_invalid_native_handle_contract_fields() {
    let mut r = record();
    r.native_rep = NativeRep::NativeHandle;
    r.native_rep_name = "native_handle".to_string();
    r.llvm_ty = I64;
    r.llvm_value = "%handle".to_string();
    r.native_abi_type = Some(abi_type(
        "handle<MyThing>",
        NativeAbiDirection::Param,
        Some(0),
        0,
    ));
    let handle = r
        .native_abi_type
        .as_mut()
        .unwrap()
        .native_handle
        .as_mut()
        .unwrap();
    handle.type_id = 0;
    handle.ownership = "leased".to_string();
    handle.thread_affinity = "worker".to_string();
    handle.debug_name.clear();
    handle.has_finalizer = true;
    handle.finalizer_symbol = Some("my_thing_free".to_string());

    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_verifier_backed_pod_layout() {
    let layout = pod_layout();
    let r = pod_record(layout);
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn rejects_pod_layout_offset_mismatch() {
    let mut layout = pod_layout();
    layout.fields[2].offset = 12;
    let r = pod_record(layout);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_pod_pointer_mask_without_metadata() {
    let mut layout = pod_layout();
    layout.pointer_mask = vec![1];
    layout.explicit_pointer_metadata = false;
    let r = pod_record(layout);
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_escaping_buffer_view() {
    let mut r = record();
    r.native_rep = NativeRep::BufferView(BufferViewRep {
        data_ptr: "%ptr".to_string(),
        length: "%len".to_string(),
        elem: crate::native_value::BufferElem::U8,
        element_width_bytes: 1,
        index_unit: crate::native_value::BufferIndexUnit::Byte,
        view_byte_offset: Some(0),
        length_offset_from_data: -8,
        bounds: BoundsState::Unknown,
        alias: AliasState::Unknown,
    });
    r.native_rep_name = "buffer_view".to_string();
    r.llvm_ty = crate::types::PTR;
    r.materialization_reason = Some(crate::native_value::MaterializationReason::RuntimeApi);
    r.native_value_state = NativeValueState::Materialized;
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_rep_llvm_type_mismatch() {
    let mut r = record();
    r.native_rep = NativeRep::U32;
    r.native_rep_name = "u32".to_string();
    r.llvm_ty = DOUBLE;
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_dynamic_fallback_without_reason() {
    let mut r = record();
    r.access_mode = Some(BufferAccessMode::DynamicFallback);
    r.native_value_state = NativeValueState::DynamicFallback;
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_invalid_scalar_conversion() {
    let mut r = record();
    r.native_rep = NativeRep::JsValue;
    r.native_rep_name = "js_value".to_string();
    r.llvm_ty = DOUBLE;
    r.native_value_state = NativeValueState::Materialized;
    r.materialization_reason = Some(crate::native_value::MaterializationReason::FunctionAbi);
    r.native_abi_transition = Some(NativeAbiTransitionRecord {
        from_native_rep: "u32".to_string(),
        to_native_rep: "js_value".to_string(),
        op: NativeAbiTransitionOp::SignedIntToFloat,
        reason: crate::native_value::MaterializationReason::FunctionAbi,
        lossy: false,
    });
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn accepts_region_local_js_value_bits() {
    let mut r = record();
    r.semantic = SemanticKind::JsValue;
    r.native_rep = NativeRep::JsValueBits;
    r.native_rep_name = "js_value_bits".to_string();
    r.llvm_ty = I64;
    r.llvm_value = "%bits".to_string();
    assert!(verify_native_rep_records(&[r]).is_ok());
}

#[test]
fn accepts_js_value_bits_materialization_transitions() {
    fn bits_transition(from: &str, op: NativeAbiTransitionOp, lossy: bool) -> NativeRepRecord {
        let mut to_bits = record();
        to_bits.semantic = SemanticKind::JsValue;
        to_bits.native_rep = NativeRep::JsValueBits;
        to_bits.native_rep_name = "js_value_bits".to_string();
        to_bits.llvm_ty = I64;
        to_bits.llvm_value = "%bits".to_string();
        to_bits.native_value_state = NativeValueState::Materialized;
        to_bits.materialization_reason = Some(MaterializationReason::FunctionAbi);
        to_bits.native_abi_transition = Some(NativeAbiTransitionRecord {
            from_native_rep: from.to_string(),
            to_native_rep: "js_value_bits".to_string(),
            op,
            reason: MaterializationReason::FunctionAbi,
            lossy,
        });
        to_bits
    }

    let to_bits = bits_transition("js_value", NativeAbiTransitionOp::JsValueToBits, false);
    let f64_to_bits = bits_transition("f64", NativeAbiTransitionOp::None, false);
    let i1_to_bits = bits_transition("i1", NativeAbiTransitionOp::BoolToJsValue, false);
    let i32_to_bits = bits_transition("i32", NativeAbiTransitionOp::SignedIntToFloat, false);
    let i64_to_bits = bits_transition("i64", NativeAbiTransitionOp::SignedIntToFloat, true);
    let native_handle_to_bits =
        bits_transition("native_handle", NativeAbiTransitionOp::PointerBox, false);

    let mut to_js_value = record();
    to_js_value.semantic = SemanticKind::JsValue;
    to_js_value.native_rep = NativeRep::JsValue;
    to_js_value.native_rep_name = "js_value".to_string();
    to_js_value.llvm_ty = DOUBLE;
    to_js_value.llvm_value = "%boxed".to_string();
    to_js_value.native_value_state = NativeValueState::Materialized;
    to_js_value.materialization_reason = Some(MaterializationReason::ReturnAbi);
    to_js_value.native_abi_transition = Some(NativeAbiTransitionRecord {
        from_native_rep: "js_value_bits".to_string(),
        to_native_rep: "js_value".to_string(),
        op: NativeAbiTransitionOp::BitsToJsValue,
        reason: MaterializationReason::ReturnAbi,
        lossy: false,
    });

    assert!(verify_native_rep_records(&[
        to_bits,
        f64_to_bits,
        i1_to_bits,
        i32_to_bits,
        i64_to_bits,
        native_handle_to_bits,
        to_js_value,
    ])
    .is_ok());
}

#[test]
fn rejects_materialized_js_value_bits_without_transition() {
    let mut r = record();
    r.semantic = SemanticKind::JsValue;
    r.native_rep = NativeRep::JsValueBits;
    r.native_rep_name = "js_value_bits".to_string();
    r.llvm_ty = I64;
    r.llvm_value = "%bits".to_string();
    r.native_value_state = NativeValueState::Materialized;
    r.materialization_reason = None;
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_js_value_bits_as_abi_or_fallback() {
    let mut abi = record();
    abi.semantic = SemanticKind::JsValue;
    abi.native_rep = NativeRep::JsValueBits;
    abi.native_rep_name = "js_value_bits".to_string();
    abi.llvm_ty = I64;
    abi.llvm_value = "%bits".to_string();
    abi.native_abi_type = Some(abi_type("jsvalue", NativeAbiDirection::Param, Some(0), 0));
    assert!(verify_native_rep_records(&[abi]).is_err());

    let mut fallback = record();
    fallback.semantic = SemanticKind::JsValue;
    fallback.native_rep = NativeRep::JsValueBits;
    fallback.native_rep_name = "js_value_bits".to_string();
    fallback.llvm_ty = I64;
    fallback.llvm_value = "%bits".to_string();
    fallback.access_mode = Some(BufferAccessMode::DynamicFallback);
    fallback.native_value_state = NativeValueState::DynamicFallback;
    fallback.materialization_reason = Some(MaterializationReason::RuntimeApi);
    fallback.fallback_reason = Some(MaterializationReason::RuntimeApi);
    assert!(verify_native_rep_records(&[fallback]).is_err());
}

#[test]
fn rejects_materialized_f32_record() {
    let mut r = record();
    r.native_rep = NativeRep::F32;
    r.native_rep_name = "f32".to_string();
    r.llvm_ty = F32;
    r.materialization_reason = Some(crate::native_value::MaterializationReason::FunctionAbi);
    r.native_value_state = NativeValueState::Materialized;
    assert!(verify_native_rep_records(&[r]).is_err());
}

#[test]
fn rejects_escaping_raw_handle_and_promise() {
    let mut handle = record();
    handle.native_rep = NativeRep::NativeHandle;
    handle.native_rep_name = "native_handle".to_string();
    handle.llvm_ty = I64;
    handle.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
    handle.native_value_state = NativeValueState::Materialized;

    let mut promise = record();
    promise.native_rep = NativeRep::PromiseBoundary;
    promise.native_rep_name = "promise_boundary".to_string();
    promise.llvm_ty = I64;
    promise.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
    promise.native_value_state = NativeValueState::Materialized;

    assert!(verify_native_rep_records(&[handle, promise]).is_err());
}

#[test]
fn accepts_handle_and_promise_boxing_transitions() {
    let mut handle = record();
    handle.native_rep = NativeRep::JsValue;
    handle.native_rep_name = "js_value".to_string();
    handle.llvm_ty = DOUBLE;
    handle.native_value_state = NativeValueState::Materialized;
    handle.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
    handle.native_abi_transition = Some(NativeAbiTransitionRecord {
        from_native_rep: "native_handle".to_string(),
        to_native_rep: "js_value".to_string(),
        op: NativeAbiTransitionOp::PointerBox,
        reason: crate::native_value::MaterializationReason::ReturnAbi,
        lossy: false,
    });

    let mut promise = record();
    promise.native_rep = NativeRep::JsValue;
    promise.native_rep_name = "js_value".to_string();
    promise.llvm_ty = DOUBLE;
    promise.native_value_state = NativeValueState::Materialized;
    promise.materialization_reason = Some(crate::native_value::MaterializationReason::ReturnAbi);
    promise.native_abi_transition = Some(NativeAbiTransitionRecord {
        from_native_rep: "promise_boundary".to_string(),
        to_native_rep: "js_value".to_string(),
        op: NativeAbiTransitionOp::PromiseBox,
        reason: crate::native_value::MaterializationReason::ReturnAbi,
        lossy: false,
    });

    assert!(verify_native_rep_records(&[handle, promise]).is_ok());
}
