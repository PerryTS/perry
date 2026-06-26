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
        import_function_ffi_aliases: std::collections::HashMap::new(),
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
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
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
        script_global_functions: Vec::new(),
        references_global_this: false,
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
        gen_param_prologue_len: std::collections::HashMap::new(),
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
        heritage_lexically_shadowed: false,
        fields,
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
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
        byte_offset: 0,
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
    assert_eq!(artifact["schema_version"], 12);
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
    assert_eq!(artifact["schema_version"], 12);
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
    assert_eq!(artifact["schema_version"], 12);
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
        byte_offset: 0,
    }));
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![],
        type_args: vec![Type::Named("Wide".to_string())],
        byte_offset: 0,
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
        byte_offset: 0,
    }));
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::NativeArenaAlloc(Box::new(int(4096)))],
        type_args: vec![Type::Named("Wide".to_string())],
        byte_offset: 0,
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
    let tiny_ir = function_ir_section(
        &ir,
        "perry_fn_pod_layout_specialization_ts__u_layout_24_Tiny",
    );
    let wide_ir = function_ir_section(
        &ir,
        "perry_fn_pod_layout_specialization_ts__u_layout_24_Wide",
    );

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
    let tiny_ir = function_ir_section(
        &ir,
        "perry_fn_native_pod_view_specialization_ts__u_view_24_Tiny",
    );
    let wide_ir = function_ir_section(
        &ir,
        "perry_fn_native_pod_view_specialization_ts__u_view_24_Wide",
    );

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
    assert_eq!(artifact["schema_version"], 12);
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
fn pod_field_read_after_dynamic_materialization_uses_number_coerce() {
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
        Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Sub,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(local(1)),
                property: "tag".to_string(),
            }),
            right: Box::new(int(1)),
        })),
    ];

    let ir = compile_ir("pod_dynamic_materialized_read_coerce.ts", body);
    assert!(
        ir.contains("call double @js_number_coerce"),
        "POD field reads after dynamic materialization must not feed boxed JSValue fallbacks into raw numeric arithmetic:\n{ir}"
    );
}

#[test]
fn number_coerce_of_proven_numeric_loop_expression_skips_runtime_call() {
    let body = vec![
        number_let(1, "sum", true, int(0)),
        Stmt::For {
            init: Some(Box::new(number_let(2, "i", true, int(0)))),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(local(2)),
                right: Box::new(int(64)),
            }),
            update: Some(increment(2)),
            body: vec![Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(add(
                    local(1),
                    Expr::NumberCoerce(Box::new(add(local(2), number(0.5)))),
                )),
            ))],
        },
        Stmt::Return(Some(local(1))),
    ];

    let ir = compile_ir("number_coerce_numeric_loop_no_runtime_call.ts", body);
    assert!(
        !ir.contains("call double @js_number_coerce"),
        "Number(i + 0.5) with a proven integer loop counter is already a primitive number:\n{ir}"
    );
}

#[test]
fn number_coerce_of_numeric_array_fallback_keeps_runtime_call() {
    let module = module_with_classes_and_params(
        "number_coerce_numeric_array_fallback.ts",
        Vec::new(),
        vec![param(1, "values", Type::Array(Box::new(Type::Number)))],
        Type::Number,
        vec![Stmt::Return(Some(Expr::NumberCoerce(Box::new(
            Expr::IndexGet {
                object: Box::new(local(1)),
                index: Box::new(int(0)),
            },
        ))))],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_number_coerce"),
        "Number(values[0]) must still coerce boxed numeric-array fallback values:\n{ir}"
    );
}

#[test]
fn typed_array_f64_store_coerces_raw_numeric_array_fallback_value() {
    let module = module_with_classes_and_params(
        "typed_array_f64_store_coerces_numeric_array_fallback.ts",
        Vec::new(),
        vec![param(3, "values", Type::Array(Box::new(Type::Number)))],
        Type::Number,
        vec![
            native_arena_owner_let(1, "arena", int(64), false),
            native_arena_view_let(
                2,
                "out",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(local(2)),
                index: Box::new(int(0)),
                value: Box::new(Expr::IndexGet {
                    object: Box::new(local(3)),
                    index: Box::new(int(0)),
                }),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );
    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_number_coerce"),
        "Float64Array native stores must coerce guarded numeric-array fallback values before raw storage:\n{ir}"
    );
    assert!(
        ir.contains("store double"),
        "test must exercise the raw Float64Array store path:\n{ir}"
    );
}

#[test]
fn scalar_replaced_raw_f64_field_store_keeps_numeric_array_fallback_boxed() {
    let mut properties = std::collections::HashMap::new();
    properties.insert("gain".to_string(), prop(Type::Number));
    let packet_ty = Type::Object(ObjectType {
        name: None,
        properties,
        property_order: Some(vec!["gain".to_string()]),
        index_signature: None,
    });
    let module = module_with_classes_and_params(
        "scalar_field_store_keeps_numeric_array_fallback_boxed.ts",
        Vec::new(),
        vec![param(3, "values", Type::Array(Box::new(Type::Number)))],
        Type::Number,
        vec![
            Stmt::Let {
                id: 2,
                name: "packet".to_string(),
                ty: packet_ty,
                mutable: true,
                init: Some(Expr::Object(
                    vec![("gain".to_string(), number(0.0))]
                        .into_iter()
                        .collect(),
                )),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(2)),
                property: "gain".to_string(),
                value: Box::new(Expr::IndexGet {
                    object: Box::new(local(3)),
                    index: Box::new(int(0)),
                }),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(local(2)),
                property: "gain".to_string(),
            })),
        ],
    );
    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_typed_feedback_array_index_get_fallback_boxed"),
        "test must exercise a numeric-array get with a boxed fallback arm:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_array_numeric_value_to_raw_f64"),
        "scalar raw-f64 fields must not canonicalize a possibly boxed fallback value into raw storage:\n{ir}"
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
    assert_eq!(artifact["schema_version"], 12);
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
    assert_eq!(artifact["schema_version"], 12);
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

#[path = "native_proof_regressions/native_library.rs"]
mod native_library;

#[path = "native_proof_regressions/artifact_records.rs"]
mod artifact_records;
#[path = "native_proof_regressions/pod_manifest.rs"]
mod pod_manifest;

/// Regression: a named/value-form import of a node-core native-module
/// function (`import { realpathSync } from "fs"; realpathSync(p)`) reaches
/// codegen as a receiver-less `NativeMethodCall { module: "fs", object: None,
/// method: "realpathSync" }` with no static `NATIVE_MODULE_TABLE` row.
/// Pre-fix this hit the receiver-less fall-through and emitted a TAG_UNDEFINED
/// sentinel, so the call returned `undefined` even though the member form
/// (`fs.realpathSync(p)`) works. The fix bridges value-form calls of modules
/// that own a runtime dispatch bucket onto the same runtime by-name dispatcher
/// the member form uses (`js_native_call_method` on the module namespace). This
/// asserts the bridge fires (real dispatch call emitted) for an fs method that
/// has no dedicated table row / HIR fast-path.
#[test]
fn named_import_native_fn_routes_to_runtime_dispatch() {
    let body = vec![
        Stmt::Expr(native_module_call(
            "fs",
            "realpathSync",
            vec![Expr::String("/tmp".to_string())],
        )),
        Stmt::Return(Some(int(0))),
    ];
    let ir = compile_ir("named_import_native_fn_dispatch.ts", body);
    // The value-form call must reach the runtime by-name dispatcher (the same
    // path the member form `fs.realpathSync(...)` uses), not the dead-end
    // TAG_UNDEFINED sentinel.
    assert!(
        ir.contains("@js_native_call_method"),
        "named-import native fn must route through js_native_call_method:\n{ir}"
    );
    // It must also install the fs dispatch bucket via the module-namespace
    // receiver synthesis so the method actually resolves at runtime.
    assert!(
        ir.contains("@js_nm_install_fs") || ir.contains("@js_create_native_module_namespace"),
        "fs module namespace receiver must be synthesized:\n{ir}"
    );
}

#[path = "native_proof_regressions/invalidation.rs"]
mod invalidation;
