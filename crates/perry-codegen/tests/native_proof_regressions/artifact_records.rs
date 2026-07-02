use super::*;

#[test]
fn native_library_handle_runtime_lowering_records_contracts() {
    let owned_handle = perry_api_manifest::NativeHandleAbi {
        type_name: Some("Thing".to_string()),
        ownership: perry_api_manifest::NativeHandleOwnership::Owned,
        nullable: true,
        thread: perry_api_manifest::NativeHandleThreadAffinity::Creator,
        finalizer: Some("thing_free".to_string()),
        debug_name: "ThingHandle".to_string(),
    };
    let borrowed_param = perry_api_manifest::NativeHandleAbi {
        ownership: perry_api_manifest::NativeHandleOwnership::Borrowed,
        finalizer: None,
        ..owned_handle.clone()
    };
    let opts = native_library_opts_typed(vec![
        (
            "make_thing",
            vec![],
            perry_api_manifest::NativeAbiType::Handle(owned_handle.clone()),
        ),
        (
            "use_thing",
            vec![perry_api_manifest::NativeAbiType::Handle(
                borrowed_param.clone(),
            )],
            perry_api_manifest::NativeAbiType::Void,
        ),
    ]);
    let module = module(
        "native_library_handle_runtime_lowering.ts",
        vec![
            Stmt::Expr(extern_call(
                "use_thing",
                vec![extern_call("make_thing", Vec::new(), Type::Any)],
                Type::Void,
            )),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_handle_new_owned"),
        "{ir}"
    );
    assert!(ir.contains("ptr @thing_free"), "{ir}");
    assert!(ir.contains("declare void @thing_free(ptr, ptr)"), "{ir}");
    assert!(ir.contains("call i64 @js_native_handle_unwrap"), "{ir}");
    assert!(!ir.contains("call i64 @js_nanbox_get_pointer"), "{ir}");

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            let contract = &record["native_abi_type"]["native_handle"];
            record["consumer"] == "native_library.raw_handle"
                && record["native_abi_type"]["direction"] == "return"
                && contract["type_name"] == "Thing"
                && contract["type_id"].as_u64() == Some(owned_handle.type_id())
                && contract["ownership"] == "owned"
                && contract["nullable"] == true
                && contract["thread_affinity"] == "creator"
                && contract["debug_name"] == "ThingHandle"
                && contract["finalizer_symbol"] == "thing_free"
                && contract["has_finalizer"] == true
        }),
        "expected owned native-handle return contract:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            let contract = &record["native_abi_type"]["native_handle"];
            record["expr_kind"] == "NativeLibraryParam"
                && record["native_abi_type"]["direction"] == "param"
                && record["native_abi_type"]["abi_slot_index"] == 0
                && record["native_abi_type"]["runtime_guard"]["helper"] == "js_native_handle_unwrap"
                && contract["ownership"] == "borrowed"
                && contract["js_argument_index"] == 0
                && contract["has_finalizer"] == false
        }),
        "expected borrowed native-handle param contract:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_native_handle_runtime"
                && record["native_abi_transition"]["op"] == "native_handle_box"
        }),
        "expected native-handle runtime boxing transition:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_numeric_array_f64_fast_paths_and_fallback_reasons() {
    let array_ty = Type::Array(Box::new(Type::Number));
    let module = module_with_classes_and_params(
        "artifact_numeric_array_f64.ts",
        Vec::new(),
        vec![param(1, "xs", array_ty)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(local(1)),
                index: Box::new(int(0)),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(local(1)),
                index: Box::new(int(0)),
            })),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexSet"
                && record["consumer"] == "js_array_numeric_set_f64_unboxed"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected numeric array f64 set fast-path record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexGet"
                && record["consumer"] == "js_array_numeric_get_f64_unboxed"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected numeric array f64 get fast-path record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "rejected")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected boxed runtime fallback reason records:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["raw_f64_layout_fact_counts"]["consumed"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected raw-f64 layout consumed summary:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["raw_f64_layout_fact_counts"]["rejected"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "expected raw-f64 layout rejection summary:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["raw_f64_layout_fact_counts"]["invalidated"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "expected raw-f64 layout invalidation summary:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_write_barrier_child_js_value_bits() {
    let module = module_with_classes_and_params(
        "artifact_write_barrier_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "key", Type::String),
            param(3, "value", Type::Any),
        ],
        Type::Number,
        vec![
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(local(1)),
                index: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "WriteBarrier"
                && record["consumer"] == "write_barrier.child_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["native_value_state"] == "region_local"
                && record["access_mode"].is_null()
                && record["native_abi_type"].is_null()
        }),
        "expected production write-barrier js_value_bits record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "lower_expr_native_js_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_abi_type"].is_null()
        }),
        "expected production js_value_bits selector record:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["js_value_bits_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "expected js_value_bits summary count:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_raw_numeric_class_field_f64_fast_paths_and_fallback_reasons() {
    let point = class(101, "Point", vec![class_field("x", Type::Number)]);
    let module = module_with_classes_and_params(
        "artifact_raw_numeric_class_field.ts",
        vec![point],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(1)),
                property: "x".to_string(),
                value: Box::new(Expr::Number(7.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(local(1)),
                property: "x".to_string(),
            })),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClassFieldSet"
                && record["consumer"] == "class_field_set.raw_f64_store"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                && record_has_note(
                    record,
                    "receiver_proof=declared_named_receiver_guarded_exact_class"
                )
                && record_has_note(record, "field_layout=raw_f64_slot_array")
                && record_has_note(record, "pointer_bitmap=non_pointer")
        }),
        "expected raw numeric class field f64 store record with exact receiver proof:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClassFieldGet"
                && record["consumer"] == "class_field_get.raw_f64_load"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                && record_has_note(
                    record,
                    "receiver_proof=declared_named_receiver_guarded_exact_class",
                )
                && record_has_note(record, "field_layout=raw_f64_slot_array")
                && record_has_note(record, "pointer_bitmap=non_pointer")
        }),
        "expected raw numeric class field f64 load record with exact receiver proof:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "WriteBarrierElided"
                && record["consumer"] == "write_barrier.elided_raw_f64_class_field"
                && record["native_rep_name"] == "f64"
                && record_has_note(record, "reason=raw_f64_class_field_pointer_free")
                && record_has_note(record, "pointer_bitmap=non_pointer")
        }),
        "expected pointer-free raw numeric class field store to record barrier elision:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "rejected")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected boxed raw-field fallback reason records:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["raw_f64_layout_fact_counts"]["consumed"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected raw-f64 layout consumed summary:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["write_barrier_elided_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "expected raw numeric class-field barrier elision summary:\n{artifact:#}"
    );
}
