use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};
use perry_types::{ObjectType, PropertyInfo, Type};

static ARTIFACT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_reexport_named_imports: std::collections::HashSet::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: perry_codegen::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
    }
}

fn module(name: &str, body: Vec<Stmt>) -> Module {
    module_with_classes_and_params(name, Vec::new(), Vec::new(), Type::Number, body)
}

fn module_with_classes_and_params(
    name: &str,
    classes: Vec<Class>,
    params: Vec<Param>,
    return_type: Type,
    body: Vec<Stmt>,
) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes,
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params,
            return_type,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
    }
}

fn compile_ir(name: &str, body: Vec<Stmt>) -> String {
    compile_ir_with_opts(name, body, empty_opts())
}

fn compile_ir_with_opts(name: &str, body: Vec<Stmt>, opts: CompileOptions) -> String {
    String::from_utf8(compile_module(&module(name, body), opts).unwrap()).unwrap()
}

fn compile_artifact_json(name: &str, body: Vec<Stmt>) -> serde_json::Value {
    compile_artifact_json_for_module(module(name, body))
}

fn compile_artifact_json_for_module(module: Module) -> serde_json::Value {
    compile_artifact_json_for_module_with_opts(module, empty_opts())
}

fn compile_artifact_json_for_module_with_opts(
    module: Module,
    opts: CompileOptions,
) -> serde_json::Value {
    let name = module.name.clone();
    let _guard = ARTIFACT_ENV_LOCK.lock().unwrap();
    let dir = std::env::temp_dir().join(format!(
        "perry_native_reps_test_{}_{}",
        std::process::id(),
        name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let old_reps = std::env::var_os("PERRY_NATIVE_REPS");
    let old_reps_dir = std::env::var_os("PERRY_NATIVE_REPS_DIR");
    std::env::set_var("PERRY_NATIVE_REPS", "1");
    std::env::set_var("PERRY_NATIVE_REPS_DIR", &dir);

    let compile_result = compile_module(&module, opts);

    match old_reps {
        Some(value) => std::env::set_var("PERRY_NATIVE_REPS", value),
        None => std::env::remove_var("PERRY_NATIVE_REPS"),
    }
    match old_reps_dir {
        Some(value) => std::env::set_var("PERRY_NATIVE_REPS_DIR", value),
        None => std::env::remove_var("PERRY_NATIVE_REPS_DIR"),
    }

    compile_result.unwrap();
    let paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let mut parsed = Vec::new();
    for path in paths {
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        if value["module"] == name {
            return value;
        }
        parsed.push(value["module"].clone());
    }
    panic!("native reps artifact for {name} not found in {dir:?}; saw modules {parsed:?}");
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
    }
}

fn class_field(name: &str, ty: Type) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn class(id: u32, name: &str, fields: Vec<ClassField>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        fields,
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
    }
}

fn local(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn int(value: i64) -> Expr {
    Expr::Integer(value)
}

fn number(value: f64) -> Expr {
    Expr::Number(value)
}

fn prop(ty: Type) -> PropertyInfo {
    PropertyInfo {
        ty,
        optional: false,
        readonly: false,
    }
}

fn pod_type(fields: &[(&str, Type)]) -> Type {
    let mut properties = std::collections::HashMap::new();
    let mut property_order = Vec::new();
    for (name, ty) in fields {
        properties.insert((*name).to_string(), prop(ty.clone()));
        property_order.push((*name).to_string());
    }
    Type::Generic {
        base: "PerryPod".to_string(),
        type_args: vec![Type::Object(ObjectType {
            name: None,
            properties,
            property_order: Some(property_order),
            index_signature: None,
        })],
    }
}

fn pod_let(id: u32, name: &str, ty: Type, fields: Vec<(&str, Expr)>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: true,
        init: Some(Expr::Object(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )),
    }
}

fn number_let(id: u32, name: &str, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable,
        init: Some(init),
    }
}

fn buffer_let(id: u32, name: &str, size: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Named("Buffer".to_string()),
        mutable: false,
        init: Some(Expr::BufferAlloc {
            size: Box::new(size),
            fill: None,
            encoding: None,
        }),
    }
}

fn number_array_let(id: u32, name: &str, values: Vec<i64>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Array(Box::new(Type::Number)),
        mutable: true,
        init: Some(Expr::Array(values.into_iter().map(int).collect())),
    }
}

fn bit_or_zero(value: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::BitOr,
        left: Box::new(value),
        right: Box::new(int(0)),
    }
}

fn div(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Div,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn length(local_id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(local(local_id)),
        property: "length".to_string(),
    }
}

fn buffer_set(buffer_id: u32, index: Expr) -> Stmt {
    Stmt::Expr(Expr::BufferIndexSet {
        buffer: Box::new(local(buffer_id)),
        index: Box::new(index),
        value: Box::new(int(1)),
    })
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        type_args: Vec::new(),
    }
}

fn native_module_call(module: &str, method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: module.to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

fn extern_call(name: &str, args: Vec<Expr>, return_type: Type) -> Expr {
    let param_types = args.iter().map(|_| Type::Number).collect();
    call(
        Expr::ExternFuncRef {
            name: name.to_string(),
            param_types,
            return_type,
        },
        args,
    )
}

fn native_library_opts(functions: Vec<(&str, Vec<&str>, &str)>) -> CompileOptions {
    let mut opts = empty_opts();
    opts.native_library_functions = functions
        .into_iter()
        .map(|(name, params, ret)| {
            (
                name.to_string(),
                params.into_iter().map(str::to_string).collect(),
                ret.to_string(),
            )
        })
        .collect();
    opts
}

fn array_set(array_id: u32, index: Expr, value: Expr) -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(local(array_id)),
        index: Box::new(index),
        value: Box::new(value),
    })
}

fn increment(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Increment,
        prefix: false,
    }
}

fn decrement(id: u32) -> Expr {
    Expr::Update {
        id,
        op: UpdateOp::Decrement,
        prefix: false,
    }
}

fn for_loop_with_start_and_update(
    counter_id: u32,
    start: Expr,
    bound: Expr,
    update: Option<Expr>,
    body: Vec<Stmt>,
) -> Stmt {
    for_loop_with_op_start_and_update(counter_id, start, CompareOp::Lt, bound, update, body)
}

fn for_loop_with_op_start_and_update(
    counter_id: u32,
    start: Expr,
    op: CompareOp,
    bound: Expr,
    update: Option<Expr>,
    body: Vec<Stmt>,
) -> Stmt {
    Stmt::For {
        init: Some(Box::new(number_let(counter_id, "i", true, start))),
        condition: Some(Expr::Compare {
            op,
            left: Box::new(local(counter_id)),
            right: Box::new(bound),
        }),
        update,
        body,
    }
}

fn for_loop(counter_id: u32, bound: Expr, body: Vec<Stmt>) -> Stmt {
    for_loop_with_start_and_update(counter_id, int(0), bound, Some(increment(counter_id)), body)
}

fn assert_buffer_store_uses_dynamic_fallback(ir: &str) {
    assert!(
        ir.contains("call void @js_buffer_set"),
        "stale-proof case should keep the checked Buffer store fallback:\n{ir}"
    );
    assert!(
        !ir.contains("getelementptr inbounds i8"),
        "stale-proof case must not emit an inbounds native buffer GEP:\n{ir}"
    );
}

#[test]
fn artifact_schema_v6_records_consumed_native_facts_for_buffer_region() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        for_loop(3, length(2), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_positive_buffer_region.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "unchecked_native"
                && record["consumed_facts"]
                    .as_array()
                    .is_some_and(|facts| facts.iter().any(|fact| fact["kind"] == "bounds"))
                && record["consumed_facts"]
                    .as_array()
                    .is_some_and(|facts| facts.iter().any(|fact| fact["kind"] == "alias_noalias"))
        }),
        "expected native buffer record with consumed bounds and noalias facts:\n{artifact:#}"
    );
}

#[test]
fn artifact_schema_v6_records_rejected_facts_for_buffer_fallback() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_rejected_buffer_region.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "dynamic_fallback"
                && !record["fallback_reason"].is_null()
                && record["rejected_facts"]
                    .as_array()
                    .is_some_and(|facts| !facts.is_empty())
        }),
        "expected fallback record with rejected facts:\n{artifact:#}"
    );
}

#[test]
fn artifact_schema_v6_records_c_layout_pod_manifest() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
        ("total", Type::Number),
        ("count", Type::Named("PerryBufferLen".to_string())),
    ]);
    let body = vec![
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
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(local(1)),
            property: "tag".to_string(),
            value: Box::new(int(9)),
        }),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(local(1)),
            property: "gain".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_record.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    assert_eq!(artifact["summary"]["pod_layout_count"], 1);
    assert_eq!(artifact["summary"]["pod_record_count"], 1);
    let layouts = artifact["pod_layouts"].as_array().unwrap();
    assert_eq!(layouts.len(), 1);
    let layout = &layouts[0];
    assert_eq!(layout["endian"], "native");
    assert_eq!(layout["packing"], "c");
    assert_eq!(layout["size"], 24);
    assert_eq!(layout["alignment"], 8);
    assert_eq!(layout["tail_padding"], 4);
    let fields = layout["fields"].as_array().unwrap();
    let observed: Vec<_> = fields
        .iter()
        .map(|field| {
            (
                field["name"].as_str().unwrap(),
                field["native_rep_name"].as_str().unwrap(),
                field["offset"].as_u64().unwrap(),
                field["size"].as_u64().unwrap(),
                field["alignment"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        observed,
        vec![
            ("tag", "u32", 0, 4, 4),
            ("gain", "f32", 4, 4, 4),
            ("total", "f64", 8, 8, 8),
            ("count", "buffer_len", 16, 4, 4),
        ]
    );
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["native_rep_name"] == "pod_record"
                    && !record["pod_layout"].is_null()
                    && record["consumer"] == "pod_record_stack_alloc"
            }),
        "expected pod_record stack allocation record:\n{artifact:#}"
    );
}

#[test]
fn artifact_schema_v6_records_pod_dynamic_write_fallback() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
    ]);
    let body = vec![
        pod_let(
            1,
            "packet",
            packet_ty,
            vec![("tag", int(7)), ("gain", number(1.5))],
        ),
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(local(1)),
            property: "tag".to_string(),
            value: Box::new(Expr::String("x".to_string())),
        }),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_dynamic_write.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["consumer"] == "pod_record_field_set_dynamic_value"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "pod_dynamic_mutation"
                    && record["fallback_reason"] == "pod_dynamic_mutation"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note == "rhs_not_scalar_compatible")
                        })
                    })
            }),
        "expected explicit POD dynamic write fallback record:\n{artifact:#}"
    );
}

#[test]
fn artifact_schema_v6_rejects_inexact_pod_initializer_values() {
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("gain", Type::Named("PerryF32".to_string())),
        ("count", Type::Named("PerryBufferLen".to_string())),
    ]);
    let body = vec![
        pod_let(
            1,
            "packet",
            packet_ty,
            vec![
                ("tag", int(-1)),
                ("gain", number(1.1)),
                ("count", Expr::String("x".to_string())),
            ],
        ),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_init_reject.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    assert_eq!(artifact["summary"]["pod_layout_count"], 0);
    assert_eq!(artifact["summary"]["pod_record_count"], 0);
    assert!(artifact["pod_layouts"].as_array().unwrap().is_empty());
    assert!(
        !artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["native_rep_name"] == "pod_record"),
        "inexact POD initializer must not emit pod_record storage:\n{artifact:#}"
    );
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "PodRecordRejected"
                    && record["fallback_reason"] == "pod_unsupported"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note.contains("inexact_or_dynamic_initializer"))
                        })
                    })
            }),
        "expected explicit POD initializer rejection record:\n{artifact:#}"
    );
}

#[test]
fn artifact_schema_v6_records_pod_pointerful_field_rejection() {
    let invalid_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("name", Type::String),
    ]);
    let body = vec![
        pod_let(
            1,
            "packet",
            invalid_ty,
            vec![("tag", int(7)), ("name", Expr::String("x".to_string()))],
        ),
        Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_reject.ts", body);
    assert_eq!(artifact["schema_version"], 6);
    assert_eq!(artifact["summary"]["pod_layout_count"], 0);
    assert!(artifact["pod_layouts"].as_array().unwrap().is_empty());
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "PodRecordRejected"
                    && record["fallback_reason"] == "pod_unsupported"
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note.contains("field:name"))
                        })
                    })
            }),
        "expected explicit pointerful POD rejection record:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_buffer_read_u32_and_unsigned_materialization() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(local(1)),
                property: "readUInt32BE".to_string(),
            }),
            args: vec![int(0)],
            type_args: Vec::new(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_buffer_read_u32.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_u32"
                && record["native_rep_name"] == "u32"
                && record["llvm_ty"] == "i32"
                && record["native_value_state"] == "region_local"
        }),
        "expected region-local u32 buffer numeric read record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "u32"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "unsigned_int_to_float"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected unsigned u32 JS materialization record:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["native_abi_transition_count"]
            .as_u64()
            .is_some_and(|count| count >= 1)
            && artifact["summary"]["native_abi_transition_op_counts"]["unsigned_int_to_float"]
                .as_u64()
                .is_some_and(|count| count >= 1),
        "expected transition summary counts for unsigned materialization:\n{artifact:#}"
    );
}

#[test]
fn loop_length_bound_does_not_prove_multibyte_buffer_read_inbounds() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![Stmt::Expr(call(
                Expr::PropertyGet {
                    object: Box::new(local(1)),
                    property: "readUInt32BE".to_string(),
                },
                vec![local(2)],
            ))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("loop_bound_multibyte_buffer_read.ts", body.clone());
    assert!(
        !ir.contains("getelementptr inbounds i8"),
        "`i < buf.length` only proves one-byte Buffer access; multi-byte reads must not emit an inbounds GEP:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_loop_bound_multibyte_buffer_read.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_u32"
        }),
        "multi-byte Buffer read must not consume a one-byte loop proof:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_buffer_read_double_as_f64() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(local(1)),
                property: "readDoubleLE".to_string(),
            }),
            args: vec![int(0)],
            type_args: Vec::new(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_buffer_read_double.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_f64"
                && record["native_rep_name"] == "f64"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
        }),
        "expected region-local f64 buffer numeric read record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["native_abi_transition"]["from_native_rep"] == "f64"
                && record["native_abi_transition"]["op"] == "none"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected no-op f64 JS materialization record:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_buffer_read_float_as_f32_and_float_extend_materialization() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        Stmt::Return(Some(call(
            Expr::PropertyGet {
                object: Box::new(local(1)),
                property: "readFloatLE".to_string(),
            },
            vec![int(0)],
        ))),
    ];

    let artifact = compile_artifact_json("artifact_buffer_read_f32.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_f32"
                && record["native_rep_name"] == "f32"
                && record["llvm_ty"] == "float"
                && record["native_value_state"] == "region_local"
        }),
        "expected region-local f32 buffer numeric read record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "f32"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "float_extend"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected explicit f32->double materialization record:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_buffer_length_as_buffer_len_and_unsigned_materialization() {
    let body = vec![buffer_let(1, "buf", int(8)), Stmt::Return(Some(length(1)))];

    let artifact = compile_artifact_json("artifact_buffer_length.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Buffer.length"
                && record["consumer"] == "Buffer.length.native_buffer_len"
                && record["native_rep_name"] == "buffer_len"
                && record["llvm_ty"] == "i32"
                && record["native_value_state"] == "region_local"
        }),
        "expected region-local BufferLen record for Buffer.length:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["native_abi_transition"]["from_native_rep"] == "buffer_len"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "unsigned_int_to_float"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected unsigned BufferLen JS materialization record:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_native_module_handle_and_promise_boundary_boxing() {
    let body = vec![
        Stmt::Expr(native_module_call("net", "Socket", Vec::new())),
        Stmt::Return(Some(native_module_call(
            "perry/ads",
            "js_ads_interstitial_show",
            Vec::new(),
        ))),
    ];

    let artifact = compile_artifact_json("artifact_native_module_abi_boundaries.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeModuleReturn"
                && record["consumer"] == "native_module.raw_handle"
                && record["native_rep_name"] == "native_handle"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
        }),
        "expected raw native-module handle record before boxing:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_native_handle"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "native_handle"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "pointer_box"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected native-module handle pointer-box transition:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeModuleReturn"
                && record["consumer"] == "native_module.raw_promise"
                && record["native_rep_name"] == "promise_boundary"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
        }),
        "expected raw native-module promise-boundary record before boxing:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_promise_boundary"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "promise_boundary"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "promise_box"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected native-module promise-boundary box transition:\n{artifact:#}"
    );
}

#[test]
fn native_library_manifest_lowercase_abi_returns_emit_signatures_and_artifacts() {
    let opts = native_library_opts(vec![
        ("native_ret_u64", vec![], "u64"),
        ("native_ret_usize", vec![], "usize"),
        ("native_ret_f32", vec![], "f32"),
        ("native_ret_handle", vec![], "handle"),
        ("native_ret_promise", vec![], "promise"),
    ]);
    let module = module(
        "artifact_native_library_lowercase_returns.ts",
        vec![
            Stmt::Expr(extern_call("native_ret_u64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_usize", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_f32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_handle", Vec::new(), Type::Number)),
            Stmt::Return(Some(extern_call(
                "native_ret_promise",
                Vec::new(),
                Type::Number,
            ))),
        ],
    );
    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(
        ir.contains("declare i64 @native_ret_u64()")
            && ir.contains("declare i64 @native_ret_usize()")
            && ir.contains("declare float @native_ret_f32()")
            && ir.contains("declare i64 @native_ret_handle()")
            && ir.contains("declare i64 @native_ret_promise()"),
        "expected lowercase manifest return kinds to drive LLVM declarations:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    for (consumer, rep, llvm_ty) in [
        ("native_library.raw_u64", "u64", "i64"),
        ("native_library.raw_usize", "usize", "i64"),
        ("native_library.raw_f32", "f32", "float"),
        ("native_library.raw_handle", "native_handle", "i64"),
        ("native_library.raw_promise", "promise_boundary", "i64"),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryReturn"
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == rep
                    && record["llvm_ty"] == llvm_ty
                    && record["native_value_state"] == "region_local"
            }),
            "expected raw native-library return record {consumer}/{rep}:\n{artifact:#}"
        );
    }
    for (consumer, from_rep, op, lossy) in [
        ("materialize_js_value", "u64", "unsigned_int_to_float", true),
        (
            "materialize_js_value",
            "usize",
            "unsigned_int_to_float",
            true,
        ),
        ("materialize_js_value", "f32", "float_extend", false),
        (
            "materialize_native_handle",
            "native_handle",
            "pointer_box",
            false,
        ),
        (
            "materialize_promise_boundary",
            "promise_boundary",
            "promise_box",
            false,
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["consumer"] == consumer
                    && record["native_value_state"] == "materialized"
                    && record["native_abi_transition"]["from_native_rep"] == from_rep
                    && record["native_abi_transition"]["to_native_rep"] == "js_value"
                    && record["native_abi_transition"]["op"] == op
                    && record["native_abi_transition"]["lossy"] == lossy
            }),
            "expected native-library transition {from_rep}->{op}:\n{artifact:#}"
        );
    }
}

#[test]
fn native_library_manifest_lowercase_abi_params_emit_c_abi_signature() {
    let opts = native_library_opts(vec![(
        "native_abi_args",
        vec![
            "u32",
            "u64",
            "usize",
            "f32",
            "buffer_len",
            "ptr",
            "handle",
            "promise",
        ],
        "void",
    )]);
    let ir = compile_ir_with_opts(
        "native_library_lowercase_params.ts",
        vec![
            Stmt::Expr(extern_call(
                "native_abi_args",
                vec![
                    Expr::Number(1.0),
                    Expr::Number(2.0),
                    Expr::Number(3.0),
                    Expr::Number(4.0),
                    Expr::Number(5.0),
                    Expr::Number(6.0),
                    Expr::Number(7.0),
                    Expr::Number(8.0),
                ],
                Type::Void,
            )),
            Stmt::Return(Some(int(0))),
        ],
        opts,
    );

    assert!(
        ir.contains("call void @native_abi_args(i32")
            && ir.contains(
                "declare void @native_abi_args(i32, i64, i64, float, i32, i64, i64, i64)"
            )
            && !ir.contains("call i64 @js_get_string_pointer_unified"),
        "expected lowercase manifest param kinds to drive LLVM call/declaration ABI:\n{ir}"
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
        }),
        "expected numeric array f64 set fast-path record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexGet"
                && record["consumer"] == "js_array_numeric_get_f64_unboxed"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
        }),
        "expected numeric array f64 get fast-path record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
        }),
        "expected boxed runtime fallback reason records:\n{artifact:#}"
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
        }),
        "expected raw numeric class field f64 store record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClassFieldGet"
                && record["consumer"] == "class_field_get.raw_f64_load"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
        }),
        "expected raw numeric class field f64 load record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
        }),
        "expected boxed raw-field fallback reason records:\n{artifact:#}"
    );
}

fn block_between<'a>(ir: &'a str, start: &str, end: &str) -> &'a str {
    let start_pos = ir.find(start).unwrap_or_else(|| {
        panic!("missing block start marker {start:?} in IR:\n{ir}");
    });
    let after_start = &ir[start_pos + 1..];
    let end_pos = after_start.find(end).unwrap_or_else(|| {
        panic!("missing block end marker {end:?} after {start:?} in IR:\n{ir}");
    });
    &after_start[..end_pos]
}

#[test]
fn localset_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_native_i32_alias_facts() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![
                number_let(3, "j", true, bit_or_zero(local(2))),
                Stmt::Expr(increment(3)),
                buffer_set(1, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_i32_alias_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_min_length_facts() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", true, Expr::MathMin(vec![length(1), length(2)])),
        Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
        for_loop(4, local(3), vec![buffer_set(2, local(4))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("min_length_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_active_bounded_buffer_index_facts() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop(
            3,
            local(1),
            vec![
                Stmt::Expr(Expr::LocalSet(3, Box::new(int(16)))),
                buffer_set(2, local(3)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("bounded_buffer_index_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inner_loop_bounded_buffer_fact_is_removed_after_outer_fact_invalidation() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "a", local(1)),
        buffer_let(3, "b", int(8)),
        for_loop(
            4,
            local(1),
            vec![
                for_loop(
                    5,
                    length(3),
                    vec![Stmt::Expr(Expr::LocalSet(4, Box::new(int(16))))],
                ),
                buffer_set(3, local(5)),
            ],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("nested_loop_scope_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn localset_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(Expr::LocalSet(1, Box::new(int(16)))),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn update_invalidates_buffer_view_local_length_sources() {
    let body = vec![
        number_let(1, "n", true, int(8)),
        buffer_let(2, "buf", local(1)),
        Stmt::Expr(increment(1)),
        for_loop(3, local(1), vec![buffer_set(2, local(3))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("buffer_length_source_update_invalidation.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(-1),
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_loop_counter_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn decrementing_loop_update_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_start_and_update(
            2,
            int(0),
            length(1),
            Some(decrement(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("decrementing_loop_update_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn body_counter_mutation_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop(
            2,
            length(1),
            vec![Stmt::Expr(decrement(2)), buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("body_counter_mutation_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn inclusive_length_loop_does_not_emit_inbounds_buffer_gep() {
    let body = vec![
        buffer_let(1, "buf", int(8)),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![buffer_set(1, local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_length_loop_buffer_bounds.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
    let cond_ir = block_between(&ir, "\nfor.cond.", "\nfor.body.");
    assert!(
        cond_ir.contains("icmp sle i32"),
        "`i <= buf.length` with hoisted i32 length must lower as signed <=:\n{cond_ir}"
    );
    assert!(
        !cond_ir.contains("icmp slt i32"),
        "`i <= buf.length` must not be narrowed to signed <:\n{cond_ir}"
    );
}

#[test]
fn inclusive_array_length_write_uses_extension_capable_index_set_path() {
    let body = vec![
        number_array_let(1, "arr", vec![0, 0, 0]),
        for_loop_with_op_start_and_update(
            2,
            int(0),
            CompareOp::Le,
            length(1),
            Some(increment(2)),
            vec![array_set(1, local(2), local(2))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_array_length_write.ts", body);
    assert!(
        ir.contains("\nidxset.check_cap."),
        "`arr[i]` under `i <= arr.length` must keep the capacity check path:\n{ir}"
    );
    assert!(
        ir.contains("\nidxset.extend_inline."),
        "`arr[i]` under `i <= arr.length` must keep the inline length-extension path:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_array_set_f64_extend"),
        "`arr[i]` under `i <= arr.length` must keep the realloc-capable fallback:\n{ir}"
    );
}

#[test]
fn inclusive_local_length_bound_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_op_start_and_update(
            3,
            int(0),
            CompareOp::Le,
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("inclusive_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_local_length_bound_fact() {
    let body = vec![
        number_let(1, "n", false, int(8)),
        buffer_let(2, "buf", local(1)),
        for_loop_with_start_and_update(
            3,
            int(-1),
            local(1),
            Some(increment(3)),
            vec![buffer_set(2, local(3))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_local_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn negative_loop_counter_does_not_use_min_length_bound_fact() {
    let body = vec![
        buffer_let(1, "src", int(8)),
        buffer_let(2, "dst", int(8)),
        number_let(3, "n", false, Expr::MathMin(vec![length(1), length(2)])),
        for_loop_with_start_and_update(
            4,
            int(-1),
            local(3),
            Some(increment(4)),
            vec![buffer_set(2, local(4))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("negative_counter_min_length_bound.ts", body);
    assert_buffer_store_uses_dynamic_fallback(&ir);
}

#[test]
fn bitwise_truncated_division_does_not_emit_sdiv_i32() {
    let quotient = bit_or_zero(div(local(1), local(2)));
    let divide_by_zero = bit_or_zero(div(local(1), int(0)));
    let overflow = bit_or_zero(div(int(i32::MIN as i64), int(-1)));
    let body = vec![
        number_let(1, "x", false, int(8)),
        number_let(2, "y", false, int(2)),
        Stmt::Return(Some(add(add(quotient, divide_by_zero), overflow))),
    ];

    let ir = compile_ir("i32_division_regression.ts", body);
    assert!(
        !ir.contains("sdiv i32"),
        "`(a / b) | 0` must not lower to LLVM signed integer division:\n{ir}"
    );
    assert!(
        ir.contains("fdiv double"),
        "`(a / b) | 0` should lower through JS double division:\n{ir}"
    );
    assert!(
        ir.contains("@llvm.fabs.f64"),
        "ToInt32 after division should keep the NaN/Infinity guard:\n{ir}"
    );
}
