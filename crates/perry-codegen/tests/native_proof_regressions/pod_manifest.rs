use super::*;

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
