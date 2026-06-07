use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    monomorphize_module, BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module,
    ModuleInitKind, Param, Stmt, UpdateOp,
};
use perry_types::{ObjectType, PropertyInfo, Type, TypeParam};

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
            is_strict: false,
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
        closure_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
    }
}

fn compile_ir(name: &str, body: Vec<Stmt>) -> String {
    compile_ir_with_opts(name, body, empty_opts())
}

fn compile_ir_with_opts(name: &str, body: Vec<Stmt>, opts: CompileOptions) -> String {
    String::from_utf8(compile_module(&module(name, body), opts).unwrap()).unwrap()
}

fn compile_ir_for_module_with_opts(module: Module, opts: CompileOptions) -> anyhow::Result<String> {
    Ok(String::from_utf8(compile_module(&module, opts)?)?)
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
        arguments_object: None,
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
        static_accessor_names: Vec::new(),
        computed_members: Vec::new(),
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

fn pod_view_type(record_ty: Type) -> Type {
    Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![record_ty],
    }
}

fn manifest_pod_abi(
    name: Option<&str>,
    fields: Vec<(&str, perry_api_manifest::NativeAbiType)>,
) -> perry_api_manifest::NativeAbiType {
    perry_api_manifest::NativeAbiType::Pod(perry_api_manifest::NativePodAbi {
        name: name.map(str::to_string),
        fields: fields
            .into_iter()
            .map(|(name, ty)| perry_api_manifest::NativePodFieldAbi {
                name: name.to_string(),
                ty,
            })
            .collect(),
    })
}

fn manifest_pod_view_abi(
    name: Option<&str>,
    fields: Vec<(&str, perry_api_manifest::NativeAbiType)>,
) -> perry_api_manifest::NativeAbiType {
    match manifest_pod_abi(name, fields) {
        perry_api_manifest::NativeAbiType::Pod(pod) => {
            perry_api_manifest::NativeAbiType::PodAndCount(pod)
        }
        other => unreachable!("manifest_pod_abi must return pod, got {other:?}"),
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

fn typed_array_let(id: u32, name: &str, class_name: &str, kind: u8, length: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Named(class_name.to_string()),
        mutable: false,
        init: Some(Expr::TypedArrayNew {
            kind,
            arg: Some(Box::new(length)),
        }),
    }
}

fn native_arena_owner_let(id: u32, name: &str, byte_length: Expr, mutable: bool) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable,
        init: Some(Expr::NativeArenaAlloc(Box::new(byte_length))),
    }
}

fn native_pod_view_let(
    id: u32,
    name: &str,
    ty: Type,
    owner_id: u32,
    byte_offset: Expr,
    count: Expr,
) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: false,
        init: Some(Expr::NativePodView {
            owner: Box::new(local(owner_id)),
            byte_offset: Box::new(byte_offset),
            count: Box::new(count),
            view_type: None,
        }),
    }
}

fn native_arena_view_let(
    id: u32,
    name: &str,
    owner_id: u32,
    class_name: &str,
    kind: u8,
    byte_offset: Expr,
    length: Expr,
) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Named(class_name.to_string()),
        mutable: false,
        init: Some(Expr::NativeArenaView {
            owner: Box::new(local(owner_id)),
            kind,
            byte_offset: Box::new(byte_offset),
            length: Box::new(length),
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

fn buffer_read(buffer_id: u32, method: &str, index: Expr) -> Expr {
    call(
        Expr::PropertyGet {
            object: Box::new(local(buffer_id)),
            property: method.to_string(),
        },
        vec![index],
    )
}

fn index_get(object_id: u32, index: Expr) -> Expr {
    Expr::IndexGet {
        object: Box::new(local(object_id)),
        index: Box::new(index),
    }
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
                params
                    .into_iter()
                    .map(|param| perry_api_manifest::NativeAbiType::parse_str(param).unwrap())
                    .collect(),
                perry_api_manifest::NativeAbiType::parse_str(ret).unwrap(),
            )
        })
        .collect();
    opts
}

fn native_library_opts_typed(
    functions: Vec<(
        &str,
        Vec<perry_api_manifest::NativeAbiType>,
        perry_api_manifest::NativeAbiType,
    )>,
) -> CompileOptions {
    let mut opts = empty_opts();
    opts.native_library_functions = functions
        .into_iter()
        .map(|(name, params, ret)| (name.to_string(), params, ret))
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
    assert_eq!(artifact["schema_version"], 11);
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
    assert_eq!(artifact["schema_version"], 11);
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
    assert_eq!(artifact["schema_version"], 11);
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

fn pod_layout_constant_opts() -> CompileOptions {
    let header_ty = pod_type(&[
        ("code", Type::Named("PerryU32".to_string())),
        ("flags", Type::Named("PerryU32".to_string())),
    ]);
    let packet_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("header", header_ty),
        ("total", Type::Number),
        ("count", Type::Named("PerryU32".to_string())),
    ]);
    let mut opts = empty_opts();
    opts.type_aliases.insert("Packet".to_string(), packet_ty);
    opts
}

fn compile_pod_layout_constant(expr: Expr) -> anyhow::Result<String> {
    compile_ir_for_module_with_opts(
        module("pod_layout_constants.ts", vec![Stmt::Return(Some(expr))]),
        pod_layout_constant_opts(),
    )
}

fn pod_layout_specialization_opts() -> CompileOptions {
    let tiny_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("payload", Type::Named("PerryU32".to_string())),
    ]);
    let wide_ty = pod_type(&[
        ("tag", Type::Named("PerryU32".to_string())),
        ("payload", Type::Number),
    ]);
    let mut opts = empty_opts();
    opts.type_aliases.insert("Tiny".to_string(), tiny_ty);
    opts.type_aliases.insert("Wide".to_string(), wide_ty);
    opts
}

fn pod_layout_metric_expr(ty: Type) -> Expr {
    add(
        add(
            add(
                Expr::PodLayoutSizeOf { ty: ty.clone() },
                Expr::PodLayoutAlignOf { ty: ty.clone() },
            ),
            Expr::PodLayoutOffsetOf {
                ty,
                field_path: vec!["payload".to_string()],
            },
        ),
        number(0.5),
    )
}

fn pod_layout_specialization_module() -> Module {
    let mut module = Module::new("pod_layout_specialization.ts");
    module.functions.push(Function {
        id: 1,
        name: "layout".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: Some(Box::new(Type::Generic {
                base: "PerryPod".to_string(),
                type_args: vec![Type::Any],
            })),
            default: None,
        }],
        params: vec![],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(pod_layout_metric_expr(Type::TypeVar(
            "T".to_string(),
        ))))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![],
        type_args: vec![Type::Named("Tiny".to_string())],
    }));
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![],
        type_args: vec![Type::Named("Wide".to_string())],
    }));
    module
}

fn native_pod_view_specialization_module() -> Module {
    let generic_view_ty = Type::Generic {
        base: "PerryPodView".to_string(),
        type_args: vec![Type::TypeVar("T".to_string())],
    };
    let mut module = Module::new("native_pod_view_specialization.ts");
    module.functions.push(Function {
        id: 1,
        name: "view".to_string(),
        type_params: vec![TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }],
        params: vec![param(0, "arena", Type::Named("NativeArena".to_string()))],
        return_type: generic_view_ty.clone(),
        body: vec![Stmt::Return(Some(Expr::NativePodView {
            owner: Box::new(local(0)),
            byte_offset: Box::new(int(0)),
            count: Box::new(int(4)),
            view_type: Some(generic_view_ty),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::NativeArenaAlloc(Box::new(int(4096)))],
        type_args: vec![Type::Named("Tiny".to_string())],
    }));
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::NativeArenaAlloc(Box::new(int(4096)))],
        type_args: vec![Type::Named("Wide".to_string())],
    }));
    module
}

fn function_ir_section<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let needle = format!("define double @{}(", symbol);
    let start = ir
        .find(&needle)
        .unwrap_or_else(|| panic!("function `{}` not found in IR:\n{}", symbol, ir));
    let rest = &ir[start..];
    let end = rest.find("\n}\n").map(|idx| idx + 3).unwrap_or(rest.len());
    &rest[..end]
}

fn error_chain(err: &anyhow::Error) -> String {
    err.chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn pod_layout_constants_emit_layout_numbers() {
    let ty = Type::Named("Packet".to_string());

    let size_ir = compile_pod_layout_constant(Expr::PodLayoutSizeOf { ty: ty.clone() }).unwrap();
    assert!(
        size_ir.contains("ret double 32.0"),
        "sizeof<Packet>() should emit the POD size constant:\n{size_ir}"
    );

    let align_ir = compile_pod_layout_constant(Expr::PodLayoutAlignOf { ty: ty.clone() }).unwrap();
    assert!(
        align_ir.contains("ret double 8.0"),
        "alignof<Packet>() should emit the POD alignment constant:\n{align_ir}"
    );

    let offset_ir = compile_pod_layout_constant(Expr::PodLayoutOffsetOf {
        ty,
        field_path: vec!["header".to_string(), "flags".to_string()],
    })
    .unwrap();
    assert!(
        offset_ir.contains("ret double 8.0"),
        "offsetof<Packet>(\"header.flags\") should emit the flattened field offset:\n{offset_ir}"
    );
}

#[test]
fn pod_layout_constants_specialize_generic_layout_type_params() {
    let mut module = pod_layout_specialization_module();
    monomorphize_module(&mut module);

    assert!(
        module.functions.iter().any(|f| f.name == "layout$Tiny"),
        "expected Tiny specialization: {:?}",
        module
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        module.functions.iter().any(|f| f.name == "layout$Wide"),
        "expected Wide specialization: {:?}",
        module
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );

    module.functions.retain(|func| func.type_params.is_empty());
    module.init.clear();
    let ir = compile_ir_for_module_with_opts(module, pod_layout_specialization_opts()).unwrap();
    let tiny_ir = function_ir_section(&ir, "perry_fn_pod_layout_specialization_ts__layout_Tiny");
    let wide_ir = function_ir_section(&ir, "perry_fn_pod_layout_specialization_ts__layout_Wide");

    assert!(
        tiny_ir.contains("8.0") && tiny_ir.contains("4.0") && !tiny_ir.contains("16.0"),
        "Tiny specialization should use size 8, align 4, offset 4:\n{tiny_ir}"
    );
    assert!(
        wide_ir.contains("16.0") && wide_ir.contains("8.0") && !wide_ir.contains("4.0"),
        "Wide specialization should use size 16, align 8, offset 8:\n{wide_ir}"
    );
}

#[test]
fn native_pod_view_specializes_generic_layout_type_params() {
    let mut module = native_pod_view_specialization_module();
    monomorphize_module(&mut module);

    assert!(
        module.functions.iter().any(|f| f.name == "view$Tiny"),
        "expected Tiny specialization: {:?}",
        module
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        module.functions.iter().any(|f| f.name == "view$Wide"),
        "expected Wide specialization: {:?}",
        module
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
    );

    module.functions.retain(|func| func.type_params.is_empty());
    module.init.clear();
    let ir = compile_ir_for_module_with_opts(module, pod_layout_specialization_opts()).unwrap();
    let tiny_ir = function_ir_section(&ir, "perry_fn_native_pod_view_specialization_ts__view_Tiny");
    let wide_ir = function_ir_section(&ir, "perry_fn_native_pod_view_specialization_ts__view_Wide");

    assert!(
        tiny_ir.contains("call i64 @js_native_pod_view") && tiny_ir.contains("i64 8, i64 4"),
        "Tiny specialization should use stride 8 and alignment 4:\n{tiny_ir}"
    );
    assert!(
        wide_ir.contains("call i64 @js_native_pod_view") && wide_ir.contains("i64 16, i64 8"),
        "Wide specialization should use stride 16 and alignment 8:\n{wide_ir}"
    );
}

#[test]
fn pod_layout_constants_reject_non_pod_type() {
    let err = compile_pod_layout_constant(Expr::PodLayoutSizeOf { ty: Type::Number })
        .expect_err("non-POD type should fail codegen");
    let chain = error_chain(&err);

    assert!(
        chain.contains("sizeof<T>() requires T to resolve to PerryPod<...>"),
        "unexpected error: {chain}"
    );
}

#[test]
fn pod_layout_constants_reject_missing_field_path() {
    let err = compile_pod_layout_constant(Expr::PodLayoutOffsetOf {
        ty: Type::Named("Packet".to_string()),
        field_path: vec!["header".to_string(), "missing".to_string()],
    })
    .expect_err("unknown offsetof path should fail codegen");
    let chain = error_chain(&err);

    assert!(
        chain.contains("offsetof<T>(\"header.missing\") could not find that field path"),
        "unexpected error: {chain}"
    );
}

#[test]
fn native_memory_fill_u32_zero_uses_memset_fast_path() {
    let body = vec![
        native_arena_owner_let(1, "arena", int(64), false),
        native_arena_view_let(
            2,
            "words",
            1,
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(0),
            int(16),
        ),
        Stmt::Expr(Expr::NativeMemoryFillU32 {
            view: Box::new(local(2)),
            value: Box::new(int(0)),
        }),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_memory_fill_u32_zero.ts", body.clone());
    assert!(
        ir.contains("call void @llvm.memset.p0.i64"),
        "NativeMemory.fillU32(words, 0) should lower to llvm.memset:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_native_memory_fill_u32"),
        "proven local Uint32Array view should not use runtime fallback:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_native_memory_fill_u32_zero.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeMemoryFillU32"
                && record["consumer"] == "NativeMemoryFillU32.memset_zero"
                && record["native_rep_name"] == "buffer_view"
                && record["access_mode"] == "checked_native"
        }),
        "expected NativeMemoryFillU32 buffer_view record:\n{artifact:#}"
    );
}

#[test]
fn native_memory_copy_uses_memmove_fast_path() {
    let body = vec![
        native_arena_owner_let(1, "arena", int(128), false),
        native_arena_view_let(
            2,
            "src",
            1,
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(0),
            int(16),
        ),
        native_arena_view_let(
            3,
            "dst",
            1,
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            int(64),
            int(16),
        ),
        Stmt::Expr(Expr::NativeMemoryCopy {
            dst: Box::new(local(3)),
            src: Box::new(local(2)),
        }),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_memory_copy.ts", body.clone());
    assert!(
        ir.contains("call void @llvm.memmove.p0.p0.i64"),
        "NativeMemory.copy(dst, src) should lower to llvm.memmove:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_native_memory_copy"),
        "proven local typed views should not use runtime fallback:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_native_memory_copy.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeMemoryCopy"
                && record["consumer"] == "NativeMemoryCopy.dst.memmove"
                && record["native_rep_name"] == "buffer_view"
        }) && records.iter().any(|record| {
            record["expr_kind"] == "NativeMemoryCopy"
                && record["consumer"] == "NativeMemoryCopy.src.memmove"
                && record["native_rep_name"] == "buffer_view"
        }),
        "expected NativeMemoryCopy dst/src buffer_view records:\n{artifact:#}"
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
    assert_eq!(artifact["schema_version"], 11);
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
fn artifact_schema_v8_rejects_inexact_pod_initializer_values() {
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
    assert_eq!(artifact["schema_version"], 11);
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
    assert_eq!(artifact["schema_version"], 11);
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

fn record_has_raw_f64_layout_fact(record: &serde_json::Value, list: &str, state: &str) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts
            .iter()
            .any(|fact| fact["kind"] == "raw_f64_layout" && fact["state"] == state)
    })
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
        ("native_ret_jsvalue", vec![], "jsvalue"),
        ("native_ret_string", vec![], "string"),
        ("native_ret_bool", vec![], "bool"),
        ("native_ret_i32", vec![], "i32"),
        ("native_ret_i64", vec![], "i64"),
        ("native_ret_u32", vec![], "u32"),
        ("native_ret_u64", vec![], "u64"),
        ("native_ret_usize", vec![], "usize"),
        ("native_ret_f32", vec![], "f32"),
        ("native_ret_f64", vec![], "f64"),
        ("native_ret_ptr", vec![], "ptr"),
        ("native_ret_buffer_len", vec![], "buffer_len"),
        ("native_ret_handle", vec![], "handle"),
        ("native_ret_promise", vec![], "promise"),
    ]);
    let module = module(
        "artifact_native_library_lowercase_returns.ts",
        vec![
            Stmt::Expr(extern_call("native_ret_jsvalue", Vec::new(), Type::Any)),
            Stmt::Expr(extern_call("native_ret_string", Vec::new(), Type::String)),
            Stmt::Expr(extern_call("native_ret_bool", Vec::new(), Type::Boolean)),
            Stmt::Expr(extern_call("native_ret_i32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_i64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_u32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_u64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_usize", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_f32", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_f64", Vec::new(), Type::Number)),
            Stmt::Expr(extern_call("native_ret_ptr", Vec::new(), Type::Any)),
            Stmt::Expr(extern_call(
                "native_ret_buffer_len",
                Vec::new(),
                Type::Number,
            )),
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
        ir.contains("declare double @native_ret_jsvalue()")
            && ir.contains("declare ptr @native_ret_string()")
            && ir.contains("declare i32 @native_ret_bool()")
            && ir.contains("declare i32 @native_ret_i32()")
            && ir.contains("declare i64 @native_ret_i64()")
            && ir.contains("declare i32 @native_ret_u32()")
            && ir.contains("declare i64 @native_ret_u64()")
            && ir.contains("declare i64 @native_ret_usize()")
            && ir.contains("declare float @native_ret_f32()")
            && ir.contains("declare double @native_ret_f64()")
            && ir.contains("declare ptr @native_ret_ptr()")
            && ir.contains("declare i32 @native_ret_buffer_len()")
            && ir.contains("declare i64 @native_ret_handle()")
            && ir.contains("declare i64 @native_ret_promise()")
            && ir.contains("call double @js_native_handle_new_borrowed"),
        "expected lowercase manifest return kinds to drive LLVM declarations:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    for (consumer, rep, llvm_ty, abi_kind) in [
        (
            "native_library.raw_jsvalue",
            "js_value",
            "double",
            "jsvalue",
        ),
        (
            "native_library.raw_string",
            "native_handle",
            "i64",
            "string",
        ),
        ("native_library.raw_bool", "i32", "i32", "bool"),
        ("native_library.raw_i32", "i32", "i32", "i32"),
        ("native_library.raw_i64", "i64", "i64", "i64"),
        ("native_library.raw_u32", "u32", "i32", "u32"),
        ("native_library.raw_u64", "u64", "i64", "u64"),
        ("native_library.raw_usize", "usize", "i64", "usize"),
        ("native_library.raw_f32", "f32", "float", "f32"),
        ("native_library.raw_f64", "f64", "double", "f64"),
        ("native_library.raw_ptr", "native_handle", "i64", "ptr"),
        (
            "native_library.raw_buffer_len",
            "buffer_len",
            "i32",
            "buffer_len",
        ),
        (
            "native_library.raw_handle",
            "native_handle",
            "i64",
            "handle",
        ),
        (
            "native_library.raw_promise",
            "promise_boundary",
            "i64",
            "promise",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryReturn"
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == rep
                    && record["llvm_ty"] == llvm_ty
                    && record["native_value_state"] == "region_local"
                    && record["native_abi_type"]["canonical_kind"] == abi_kind
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
            "materialize_native_handle_runtime",
            "native_handle",
            "native_handle_box",
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
fn native_library_manifest_native_async_promise_artifact_records_metadata() {
    let ret = perry_api_manifest::NativeAbiType::Promise(perry_api_manifest::NativePromiseAbi {
        result: Box::new(perry_api_manifest::NativeAbiType::F64),
        completion: perry_api_manifest::NativePromiseCompletion::NativeAsync,
        thread: perry_api_manifest::NativePromiseThread::Main,
    });
    let opts = native_library_opts_typed(vec![("native_ret_native_async", vec![], ret)]);
    let module = module(
        "artifact_native_async_promise_return.ts",
        vec![Stmt::Return(Some(extern_call(
            "native_ret_native_async",
            Vec::new(),
            Type::Number,
        )))],
    );

    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();
    assert!(
        ir.contains("declare i64 @native_ret_native_async()"),
        "native async promise lowering should keep the JS Promise boundary ABI:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NativeLibraryReturn"
                && record["consumer"] == "native_library.raw_promise"
                && record["native_rep_name"] == "promise_boundary"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record["native_abi_type"]["canonical_kind"] == "promise"
                && record["native_abi_type"]["promise_result"] == "f64"
                && record["native_abi_type"]["promise_completion"] == "native_async"
                && record["native_abi_type"]["promise_thread"] == "main"
        }),
        "expected native async promise ABI metadata in artifact:\n{artifact:#}"
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
        "expected native async promise return to use existing promise boxing:\n{artifact:#}"
    );
}

#[test]
fn native_library_manifest_lowercase_abi_params_emit_c_abi_signature() {
    let opts = native_library_opts(vec![(
        "native_abi_args",
        vec![
            "jsvalue",
            "string",
            "bool",
            "i32",
            "i64",
            "u32",
            "u64",
            "usize",
            "f32",
            "f64",
            "buffer_len",
            "buffer+len",
            "ptr",
            "handle",
            "promise",
        ],
        "void",
    )]);
    let module = module(
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
                    Expr::Number(9.0),
                    Expr::Number(10.0),
                    Expr::Number(11.0),
                    Expr::Number(12.0),
                    Expr::Number(13.0),
                    Expr::Number(14.0),
                    Expr::Number(15.0),
                ],
                Type::Void,
            )),
            Stmt::Return(Some(int(0))),
        ],
    );
    let ir = String::from_utf8(compile_module(&module, opts.clone()).unwrap()).unwrap();

    assert!(
        ir.contains("call i64 @js_native_abi_check_string_ptr")
            && ir.contains("call i32 @js_native_abi_check_i32")
            && ir.contains("call i64 @js_native_abi_check_i64")
            && ir.contains("call i32 @js_native_abi_check_u32")
            && ir.contains("call i64 @js_native_abi_check_u64")
            && ir.contains("call i64 @js_native_abi_check_usize")
            && ir.contains("call float @js_native_abi_check_f32")
            && ir.contains("call double @js_native_abi_check_f64")
            && ir.contains("call ptr @js_native_abi_check_buffer_data_ptr")
            && ir.contains("call i64 @js_native_abi_check_buffer_byte_len")
            && ir.contains("call i64 @js_native_abi_check_ptr")
            && ir.contains("call i64 @js_native_abi_check_promise")
            && ir.contains("call i64 @js_native_handle_unwrap")
            && ir.contains("call void @native_abi_args(double")
            && ir.contains(
                "declare void @native_abi_args(double, ptr, i32, i32, i64, i32, i64, i64, float, double, i32, ptr, i64, i64, i64, i64)"
            ),
        "expected lowercase manifest param kinds to drive LLVM call/declaration ABI:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module_with_opts(module, opts);
    let records = artifact["records"].as_array().unwrap();
    for (display, abi_slot_index, abi_slot_count) in [
        ("jsvalue", 0, 1),
        ("string", 1, 1),
        ("bool", 2, 1),
        ("i32", 3, 1),
        ("i64", 4, 1),
        ("u32", 5, 1),
        ("u64", 6, 1),
        ("usize", 7, 1),
        ("f32", 8, 1),
        ("f64", 9, 1),
        ("buffer_len", 10, 1),
        ("buffer+len", 11, 2),
        ("buffer+len", 12, 2),
        ("ptr", 13, 1),
        ("handle", 14, 1),
        ("promise<jsvalue>", 15, 1),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryParam"
                    && record["native_abi_type"]["display"] == display
                    && record["native_abi_type"]["direction"] == "param"
                    && record["native_abi_type"]["abi_slot_index"] == abi_slot_index
                    && record["native_abi_type"]["abi_slot_count"] == abi_slot_count
            }),
            "expected native-library param ABI record {display}@{abi_slot_index}:\n{artifact:#}"
        );
    }
    for (display, abi_slot_index, helper) in [
        ("string", 1, "js_native_abi_check_string_ptr"),
        ("bool", 2, "js_is_truthy"),
        ("i32", 3, "js_native_abi_check_i32"),
        ("i64", 4, "js_native_abi_check_i64"),
        ("u32", 5, "js_native_abi_check_u32"),
        ("u64", 6, "js_native_abi_check_u64"),
        ("usize", 7, "js_native_abi_check_usize"),
        ("f32", 8, "js_native_abi_check_f32"),
        ("f64", 9, "js_native_abi_check_f64"),
        ("buffer_len", 10, "js_native_abi_check_u32"),
        ("buffer+len", 11, "js_native_abi_check_buffer_data_ptr"),
        ("buffer+len", 12, "js_native_abi_check_buffer_byte_len"),
        ("ptr", 13, "js_native_abi_check_ptr"),
        ("handle", 14, "js_native_handle_unwrap"),
        ("promise<jsvalue>", 15, "js_native_abi_check_promise"),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "NativeLibraryParam"
                    && record["native_abi_type"]["display"] == display
                    && record["native_abi_type"]["abi_slot_index"] == abi_slot_index
                    && record["native_abi_type"]["runtime_guard"]["helper"] == helper
                    && record["materialization_reason"].is_null()
                    && record["native_value_state"] == "region_local"
            }),
            "expected native-library param runtime guard {display}@{abi_slot_index}/{helper}:\n{artifact:#}"
        );
    }
}

#[path = "native_proof_regressions/pod_manifest.rs"]
mod pod_manifest;

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
        }),
        "expected raw numeric class field f64 store record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClassFieldGet"
                && record["consumer"] == "class_field_get.raw_f64_load"
                && record["native_rep_name"] == "f64"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected raw numeric class field f64 load record:\n{artifact:#}"
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
}

#[path = "native_proof_regressions/invalidation.rs"]
mod invalidation;
