use super::*;

#[test]
fn pod_i64_and_u64_materialization_uses_safe_integer_guards() {
    let packet_ty = pod_type(&[
        ("signed", Type::Named("PerryI64".to_string())),
        ("unsigned", Type::Named("PerryU64".to_string())),
    ]);
    let module = module(
        "pod_safe_integer_materialization.ts",
        vec![
            pod_let(
                1,
                "packet",
                packet_ty,
                vec![("signed", int(-7)), ("unsigned", int(9))],
            ),
            Stmt::Return(Some(local(1))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_abi_materialize_i64"),
        "signed POD fields must reject imprecise managed materialization:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_abi_materialize_u64"),
        "unsigned POD fields must reject imprecise managed materialization:\n{ir}"
    );
}

#[test]
fn nested_pod_initializers_and_local_assignments_preserve_value_semantics() {
    let nested_ty = pod_type(&[
        ("code", Type::Named("PerryU16".to_string())),
        ("delta", Type::Named("PerryI8".to_string())),
    ]);
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU8".to_string())),
        ("nested", nested_ty),
    ]);
    let module = module(
        "nested_pod_copy.ts",
        vec![
            pod_let(
                1,
                "original",
                packet_ty.clone(),
                vec![
                    ("tag", int(7)),
                    (
                        "nested",
                        Expr::Object(vec![
                            ("code".to_string(), int(513)),
                            ("delta".to_string(), int(-8)),
                        ]),
                    ),
                ],
            ),
            Stmt::Let {
                id: 2,
                name: "copy".to_string(),
                ty: packet_ty,
                mutable: true,
                init: Some(local(1)),
            },
            Stmt::Return(Some(local(1))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PodRecordLiteralInit"
                && record["consumer"] == "pod_record_stack_alloc"
                && record["pod_layout"]["fields"]
                    .as_array()
                    .is_some_and(|fields| fields.len() == 3)
        }),
        "nested POD literal should flatten into one C layout:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PodRecordCopyInit"
                && record["consumer"] == "pod_record_value_copy"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| note == "assignment_semantics=copy")
                })
        }),
        "POD local assignment must snapshot into independent storage:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "pod_record_materialized_value_copy"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "value_semantics=copy_at_managed_boundary")
                })
        }),
        "a managed POD boundary must receive a copied object:\n{artifact:#}"
    );
}

#[test]
fn pod_local_assigned_to_any_stays_at_the_managed_boundary() {
    let packet_ty = pod_type(&[("tag", Type::Named("PerryU8".to_string()))]);
    let module = module(
        "pod_to_any.ts",
        vec![
            pod_let(1, "packet", packet_ty, vec![("tag", int(7))]),
            Stmt::Let {
                id: 2,
                name: "managed".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(local(1)),
            },
            Stmt::Return(Some(local(2))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["local_id"] == 2 && record["consumer"] == "pod_record_value_copy"
        }),
        "an `any` destination must not be pulled back into native POD storage:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "pod_record_materialized_value_copy"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "value_semantics=copy_at_managed_boundary")
                })
        }),
        "a POD-to-any assignment must cross the managed value-copy boundary:\n{artifact:#}"
    );
}

#[test]
fn checked_native_scalar_conversions_keep_dynamic_pod_initializers_native() {
    let packet_ty = pod_type(&[
        ("signed8", Type::Named("PerryI8".to_string())),
        ("signed16", Type::Named("PerryI16".to_string())),
        ("unsigned8", Type::Named("PerryU8".to_string())),
        ("unsigned16", Type::Named("PerryU16".to_string())),
        ("signed", Type::Named("PerryI32".to_string())),
        ("flags", Type::Named("PerryU32".to_string())),
        ("signed64", Type::Named("PerryI64".to_string())),
        ("sequence", Type::Named("PerryU64".to_string())),
        ("pointerSize", Type::Named("PerryUSize".to_string())),
        ("signedPointerSize", Type::Named("PerryISize".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
        ("ratio", Type::Named("PerryF64".to_string())),
    ]);
    let module = module(
        "checked_native_scalar_pod_init.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "input".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(number(7.0)),
            },
            pod_let(
                2,
                "packet",
                packet_ty,
                vec![
                    (
                        "signed8",
                        native_module_call("perry/native", "i8", vec![local(1)]),
                    ),
                    (
                        "signed16",
                        native_module_call("perry/native", "i16", vec![local(1)]),
                    ),
                    (
                        "unsigned8",
                        native_module_call("perry/native", "u8", vec![local(1)]),
                    ),
                    (
                        "unsigned16",
                        native_module_call("perry/native", "u16", vec![local(1)]),
                    ),
                    (
                        "signed",
                        native_module_call("perry/native", "i32", vec![local(1)]),
                    ),
                    (
                        "flags",
                        native_module_call("perry/native", "u32", vec![local(1)]),
                    ),
                    (
                        "signed64",
                        native_module_call("perry/native", "i64", vec![local(1)]),
                    ),
                    (
                        "sequence",
                        native_module_call("perry/native", "u64", vec![local(1)]),
                    ),
                    (
                        "pointerSize",
                        native_module_call("perry/native", "usize", vec![local(1)]),
                    ),
                    (
                        "signedPointerSize",
                        native_module_call("perry/native", "isize", vec![local(1)]),
                    ),
                    (
                        "gain",
                        native_module_call("perry/native", "f32", vec![local(1)]),
                    ),
                    (
                        "ratio",
                        native_module_call("perry/native", "f64", vec![local(1)]),
                    ),
                ],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    for helper in [
        "js_perry_native_i8",
        "js_perry_native_i16",
        "js_perry_native_u8",
        "js_perry_native_u16",
        "js_perry_native_i32",
        "js_perry_native_u32",
        "js_perry_native_i64",
        "js_perry_native_u64",
        "js_perry_native_usize",
        "js_perry_native_isize",
        "js_perry_native_f32",
        "js_perry_native_f64",
    ] {
        assert!(
            ir.contains(&format!("call double @{helper}")),
            "missing checked conversion {helper}:\n{ir}"
        );
    }

    let artifact = compile_artifact_json_for_module(module);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["native_rep_name"] == "pod_record"
                    && record["consumer"] == "pod_record_stack_alloc"
                    && record["pod_layout"]["size"] == 64
            }),
        "checked conversions should preserve a region-local POD record:\n{artifact:#}"
    );
    assert!(
        !artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["consumer"] == "pod_record_fallback_to_js_object"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note.contains("inexact_or_dynamic_initializer"))
                        })
                    })
            }),
        "checked conversions must not be rejected as inexact dynamic initializers:\n{artifact:#}"
    );
}

#[test]
fn mismatched_checked_native_scalar_conversion_does_not_prove_pod_field() {
    let packet_ty = pod_type(&[("signed", Type::Named("PerryI32".to_string()))]);
    let module = module(
        "mismatched_checked_native_scalar_pod_init.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "input".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(number(7.0)),
            },
            pod_let(
                2,
                "packet",
                packet_ty,
                vec![(
                    "signed",
                    native_module_call("perry/native", "u32", vec![local(1)]),
                )],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["consumer"] == "pod_record_fallback_to_js_object"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note.contains("inexact_or_dynamic_initializer"))
                        })
                    })
            }),
        "a u32 conversion must not prove an i32 POD field:\n{artifact:#}"
    );
}

#[test]
fn native_library_manifest_pod_param_lowers_region_local_record_to_ptr() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
        ("total", Type::Number),
        ("count", Type::Named("PerryBufferLen".to_string())),
    ]);
    let packet_abi = manifest_pod_abi(
        Some("Packet"),
        vec![
            ("tag", perry_api_manifest::NativeAbiType::U32),
            ("gain", perry_api_manifest::NativeAbiType::F32),
            ("total", perry_api_manifest::NativeAbiType::F64),
            ("count", perry_api_manifest::NativeAbiType::BufferLen),
        ],
    );
    let opts = native_library_opts_typed(vec![(
        "native_use_packet",
        vec![packet_abi],
        perry_api_manifest::NativeAbiType::Void,
    )]);
    let module = module(
        "native_library_pod_param.ts",
        vec![
            pod_let(
                1,
                "packet",
                packet_ty,
                vec![
                    ("tag", int(7)),
                    ("gain", number(1.5)),
                    ("total", number(2.25)),
                    ("count", int(4)),
                ],
            ),
            Stmt::Expr(extern_call("native_use_packet", vec![local(1)], Type::Void)),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(ir.contains("declare void @native_use_packet(ptr)"), "{ir}");
    assert!(ir.contains("call void @native_use_packet(ptr"), "{ir}");
    assert!(
        ir.contains("call i64 @js_native_abi_check_pod_object"),
        "materialized POD fallback must validate object shape:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeLibraryParam"
                && record["consumer"] == "native_library.param.pod"
                && record["native_rep_name"] == "pod_record"
                && record["native_abi_type"]["canonical_kind"] == "pod"
                && record["native_abi_type"]["display"] == "pod<Packet>"
                && record["native_abi_type"]["runtime_guard"].is_null()
                && !record["pod_layout"].is_null()
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note.as_str() == Some("source=region_local_pod"))
                })
        }),
        "expected raw POD native-library param record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "native_library.param.pod_materialized_object"
                && record["native_value_state"] == "dynamic_fallback"
                && record["materialization_reason"] == "pod_materialization"
        }),
        "expected materialized-object POD fallback proof:\n{artifact:#}"
    );
}

#[test]
fn native_library_manifest_pod_param_rejects_layout_mismatch() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let mismatched_abi = manifest_pod_abi(
        Some("OtherPacket"),
        vec![
            ("tag", perry_api_manifest::NativeAbiType::U32),
            ("gain", perry_api_manifest::NativeAbiType::F64),
        ],
    );
    let opts = native_library_opts_typed(vec![(
        "native_use_packet",
        vec![mismatched_abi],
        perry_api_manifest::NativeAbiType::Void,
    )]);
    let module = module(
        "native_library_pod_mismatch.ts",
        vec![
            pod_let(
                1,
                "packet",
                packet_ty,
                vec![("tag", int(7)), ("gain", number(1.5))],
            ),
            Stmt::Expr(extern_call("native_use_packet", vec![local(1)], Type::Void)),
            Stmt::Return(Some(int(0))),
        ],
    );

    let err = compile_module(&module, opts).expect_err("POD layout mismatch must reject");
    let err = format!("{err:?}");
    assert!(err.contains("native ABI pod parameter"), "{err}");
    assert!(err.contains("expected layout"), "{err}");
    assert!(err.contains("local 1"), "{err}");
}

#[test]
fn native_library_manifest_pod_view_param_lowers_to_ptr_and_count_with_proof() {
    let meta_ty = pod_type(&[
        ("seq", Type::Named("PerryU32".to_string())),
        ("owner", Type::Named("PerryHandleId".to_string())),
    ]);
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("meta", meta_ty.clone()),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let view_ty = pod_view_type(packet_ty);
    let meta_abi = match manifest_pod_abi(
        Some("PacketMeta"),
        vec![
            ("seq", perry_api_manifest::NativeAbiType::U32),
            ("owner", perry_api_manifest::NativeAbiType::HandleId),
        ],
    ) {
        perry_api_manifest::NativeAbiType::Pod(pod) => perry_api_manifest::NativeAbiType::Pod(pod),
        other => unreachable!("expected nested pod ABI, got {other:?}"),
    };
    let packet_abi = manifest_pod_view_abi(
        Some("PacketBatch"),
        vec![
            ("tag", perry_api_manifest::NativeAbiType::U32),
            ("meta", meta_abi),
            ("gain", perry_api_manifest::NativeAbiType::F32),
        ],
    );
    let mut opts = native_library_opts_typed(vec![(
        "native_batch",
        vec![packet_abi],
        perry_api_manifest::NativeAbiType::Void,
    )]);
    opts.verify_native_regions = true;
    let module = module(
        "native_library_pod_view_param.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            native_pod_view_let(2, "view", view_ty, 1, int(0), int(128)),
            Stmt::Expr(extern_call("native_batch", vec![local(2)], Type::Void)),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(ir.contains("declare void @native_batch(ptr, i64)"), "{ir}");
    assert!(ir.contains("call void @native_batch(ptr"), "{ir}");
    assert!(
        ir.contains("call i64 @js_native_pod_view"),
        "view intrinsic must allocate one native POD view wrapper:\n{ir}"
    );
    assert!(
        ir.contains("call ptr @js_native_abi_check_pod_view_data_ptr")
            && ir.contains("call i64 @js_native_abi_check_pod_view_record_count"),
        "pod+count lowering must guard and emit data/count ABI slots:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_native_abi_check_pod_object"),
        "pod+count view lowering must not materialize per-record JS objects:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    assert_eq!(artifact["summary"]["pod_record_view_count"], 2);
    let records = artifact["records"].as_array().unwrap();
    let data_record = records
        .iter()
        .find(|record| record["consumer"] == "native_library.param.pod+count.data_ptr")
        .unwrap_or_else(|| panic!("missing pod+count data record:\n{artifact:#}"));
    let count_record = records
        .iter()
        .find(|record| record["consumer"] == "native_library.param.pod+count.record_count")
        .unwrap_or_else(|| panic!("missing pod+count count record:\n{artifact:#}"));
    assert_eq!(data_record["native_rep_name"], "pod_record_view");
    assert_eq!(
        data_record["native_abi_type"]["canonical_kind"],
        "pod+count"
    );
    assert_eq!(
        data_record["native_abi_type"]["display"],
        "pod+count<PacketBatch>"
    );
    assert_eq!(data_record["native_abi_type"]["abi_slot_index"], 0);
    assert_eq!(data_record["native_abi_type"]["abi_slot_count"], 2);
    assert_eq!(
        data_record["native_abi_type"]["runtime_guard"]["helper"],
        "js_native_abi_check_pod_view_data_ptr"
    );
    assert_eq!(count_record["native_rep_name"], "usize");
    assert_eq!(count_record["native_abi_type"]["abi_slot_index"], 1);
    assert_eq!(
        count_record["native_abi_type"]["runtime_guard"]["helper"],
        "js_native_abi_check_pod_view_record_count"
    );
    assert_eq!(data_record["pod_record_view"]["stride"], 32);
    assert_eq!(data_record["pod_record_view"]["alignment"], 8);
    assert_eq!(
        data_record["pod_record_view"]["count_source"],
        "constant:128"
    );
    assert_eq!(data_record["pod_record_view"]["pointer_free_backing"], true);
    assert_eq!(data_record["pod_record_view"]["endian"], "native");
    assert_eq!(data_record["pod_record_view"]["packing"], "c");
    let layout = &data_record["pod_layout"];
    assert_eq!(layout["size"], 32);
    assert_eq!(layout["alignment"], 8);
    assert_eq!(layout["tail_padding"], 4);
    let fields = layout["fields"].as_array().unwrap();
    let observed: Vec<_> = fields
        .iter()
        .map(|field| {
            (
                field["name"].as_str().unwrap(),
                field["path"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|part| part.as_str().unwrap())
                    .collect::<Vec<_>>(),
                field["native_rep_name"].as_str().unwrap(),
                field["offset"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            ("tag", vec!["tag"], "u32", 0),
            ("meta.seq", vec!["meta", "seq"], "u32", 8),
            ("meta.owner", vec!["meta", "owner"], "handle_id", 16),
            ("gain", vec!["gain"], "f32", 24),
        ]
    );
    assert!(
        records.iter().all(|record| {
            record["consumer"] != "native_library.param.pod_materialized_object"
                && record["materialization_reason"] != "pod_materialization"
        }),
        "pod+count lowering must not use POD object materialization:\n{artifact:#}"
    );
}

#[test]
fn native_pod_view_explicit_public_type_lowers_without_left_hand_annotation() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let view_ty = pod_view_type(packet_ty);
    let module = module(
        "native_public_pod_view_explicit_type.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            Stmt::Let {
                id: 2,
                name: "view".to_string(),
                ty: view_ty.clone(),
                mutable: false,
                init: Some(Expr::NativePodView {
                    owner: Box::new(local(1)),
                    byte_offset: Box::new(int(0)),
                    count: Box::new(int(8)),
                    view_type: Some(view_ty),
                }),
            },
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call i64 @js_native_pod_view"),
        "explicit public podView<T> must lower without a left-hand annotation:\n{ir}"
    );
}

#[test]
fn native_pod_view_length_survives_immutable_local_alias() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let view_ty = pod_view_type(packet_ty);
    let module = module(
        "native_public_pod_view_length_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            native_pod_view_let(2, "direct", view_ty.clone(), 1, int(0), int(8)),
            Stmt::Let {
                id: 3,
                name: "alias".to_string(),
                ty: view_ty,
                mutable: false,
                init: Some(local(2)),
            },
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(local(3)),
                property: "length".to_string(),
                byte_offset: 0,
            })),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_pod_view_length"),
        "an immutable PodView alias must use the validating length helper:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_object_get_field_ic_miss"),
        "a PodView alias must not enter the ordinary object-property PIC:\n{ir}"
    );
}

#[test]
fn native_pod_view_embedded_type_survives_any_expected_type() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let view_ty = pod_view_type(packet_ty);
    let module = module(
        "native_public_pod_view_any_expected_type.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            Stmt::Let {
                id: 2,
                name: "view".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::NativePodView {
                    owner: Box::new(local(1)),
                    byte_offset: Box::new(int(0)),
                    count: Box::new(int(8)),
                    view_type: Some(view_ty),
                }),
            },
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call i64 @js_native_pod_view"),
        "embedded public podView<T> type should lower when the expected type is any:\n{ir}"
    );
}

#[test]
fn native_pod_view_embedded_type_lowers_without_expected_context() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let view_ty = pod_view_type(packet_ty);
    let module = module(
        "native_public_pod_view_no_expected_context.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            Stmt::Expr(Expr::NativePodView {
                owner: Box::new(local(1)),
                byte_offset: Box::new(int(0)),
                count: Box::new(int(8)),
                view_type: Some(view_ty),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call i64 @js_native_pod_view"),
        "embedded public podView<T> type should lower with no expected context:\n{ir}"
    );
}

#[test]
fn native_pod_view_hidden_intrinsic_without_expected_type_still_errors() {
    let module = module(
        "native_hidden_pod_view_untyped.ts",
        vec![
            native_arena_owner_let(1, "owner", int(4096), false),
            Stmt::Expr(Expr::NativePodView {
                owner: Box::new(local(1)),
                byte_offset: Box::new(int(0)),
                count: Box::new(int(8)),
                view_type: None,
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let err = compile_module(&module, empty_opts()).expect_err("hidden intrinsic must reject");
    let err = format!("{err:?}");
    assert!(
        err.contains(
            "__perry_native_pod_view requires an explicit PerryPodView<T> type annotation"
        ),
        "{err}"
    );
}

#[test]
fn native_library_manifest_pod_view_param_rejects_layout_mismatch() {
    let view_ty = pod_view_type(pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]));
    let mismatched_abi = manifest_pod_view_abi(
        Some("OtherPacketBatch"),
        vec![
            ("tag", perry_api_manifest::NativeAbiType::U32),
            ("gain", perry_api_manifest::NativeAbiType::F64),
        ],
    );
    let opts = native_library_opts_typed(vec![(
        "native_batch",
        vec![mismatched_abi],
        perry_api_manifest::NativeAbiType::Void,
    )]);
    let module = module(
        "native_library_pod_view_mismatch.ts",
        vec![
            native_arena_owner_let(1, "owner", int(1024), false),
            native_pod_view_let(2, "view", view_ty, 1, int(0), int(4)),
            Stmt::Expr(extern_call("native_batch", vec![local(2)], Type::Void)),
            Stmt::Return(Some(int(0))),
        ],
    );

    let err = compile_module(&module, opts).expect_err("POD view layout mismatch must reject");
    let err = format!("{err:?}");
    assert!(err.contains("native ABI pod+count parameter"), "{err}");
    assert!(err.contains("expected layout"), "{err}");
    assert!(err.contains("local 2"), "{err}");
}
