use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    monomorphize_module, ArgumentsObjectMeta, BinaryOp, CallArg, Class, ClassComputedMember,
    ClassComputedMemberKind, ClassField, CompareOp, Expr, Function, LogicalOp, Module,
    ModuleInitKind, Param, Stmt, UnaryOp, UpdateOp,
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
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
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
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
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
    compile_artifact_json_for_module_with_opts_and_clone_rejections(module, opts, false)
}

fn compile_artifact_json_for_module_with_opts_and_clone_rejections(
    module: Module,
    opts: CompileOptions,
    all_typed_clone_rejections: bool,
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
    let old_all_typed_clone_rejections =
        std::env::var_os("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS");
    std::env::set_var("PERRY_NATIVE_REPS", "1");
    std::env::set_var("PERRY_NATIVE_REPS_DIR", &dir);
    if all_typed_clone_rejections {
        std::env::set_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS", "1");
    } else {
        std::env::remove_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS");
    }

    let compile_result = compile_module(&module, opts);

    match old_reps {
        Some(value) => std::env::set_var("PERRY_NATIVE_REPS", value),
        None => std::env::remove_var("PERRY_NATIVE_REPS"),
    }
    match old_reps_dir {
        Some(value) => std::env::set_var("PERRY_NATIVE_REPS_DIR", value),
        None => std::env::remove_var("PERRY_NATIVE_REPS_DIR"),
    }
    match old_all_typed_clone_rejections {
        Some(value) => std::env::set_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS", value),
        None => std::env::remove_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS"),
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

fn class_with_computed_member(id: u32, name: &str, fields: Vec<ClassField>) -> Class {
    let mut class = class(id, name, fields);
    class.computed_members.push(ClassComputedMember {
        key_expr: Expr::String("dynamicKey".to_string()),
        function: Function {
            id: id + 10_000,
            name: "__computed_dummy".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Number,
            body: vec![Stmt::Return(Some(int(0)))],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        },
        is_static: false,
        kind: ClassComputedMemberKind::Method,
    });
    class
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

fn map_type(key: Type, value: Type) -> Type {
    Type::Generic {
        base: "Map".to_string(),
        type_args: vec![key, value],
    }
}

fn set_type(value: Type) -> Type {
    Type::Generic {
        base: "Set".to_string(),
        type_args: vec![value],
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

fn int32_array_let(id: u32, name: &str, values: Vec<i64>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Array(Box::new(Type::Int32)),
        mutable: true,
        init: Some(Expr::Array(values.into_iter().map(int).collect())),
    }
}

fn u32_array_let(id: u32, name: &str, values: Vec<i64>) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Array(Box::new(Type::Named("PerryU32".to_string()))),
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

fn ushr_zero(value: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::UShr,
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
        byte_offset: 0,
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
            byte_offset: 0,
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
    assert_eq!(artifact["schema_version"], 15);
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
    assert_eq!(artifact["schema_version"], 15);
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
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "gain".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_record.ts", body);
    assert_eq!(artifact["schema_version"], 15);
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

fn defined_function_ir_section<'a>(ir: &'a str, symbol: &str) -> &'a str {
    let needle = format!("@{}(", symbol);
    let mut search_start = 0;
    let start = loop {
        let Some(rel_pos) = ir[search_start..].find(&needle) else {
            panic!("function `{}` definition not found in IR:\n{}", symbol, ir);
        };
        let symbol_pos = search_start + rel_pos;
        let line_start = ir[..symbol_pos].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        if ir[line_start..symbol_pos]
            .trim_start()
            .starts_with("define ")
        {
            break line_start;
        }
        search_start = symbol_pos + needle.len();
    };
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
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_dynamic_write.ts", body);
    assert_eq!(artifact["schema_version"], 15);
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
fn pod_field_read_after_dynamic_materialization_uses_dynamic_numeric_sub() {
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
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "tag".to_string(),
            }),
            right: Box::new(int(1)),
        })),
    ];

    let ir = compile_ir("pod_dynamic_materialized_read_coerce.ts", body);
    assert!(
        ir.contains("call double @js_object_get_field_by_name_f64"),
        "materialized POD field reads must preserve boxed JSValue bits:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_dynamic_sub"),
        "materialized POD field reads must use coercing dynamic arithmetic:\n{ir}"
    );
    assert!(
        !ir.contains("fsub double"),
        "materialized POD field reads must not feed boxed JSValue bits into raw arithmetic:\n{ir}"
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
                byte_offset: 0,
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
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_init_reject.ts", body);
    assert_eq!(artifact["schema_version"], 15);
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
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "tag".to_string(),
        })),
    ];

    let artifact = compile_artifact_json("artifact_c_layout_pod_reject.ts", body);
    assert_eq!(artifact["schema_version"], 15);
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

#[test]
fn representation_first_numeric_locals_stay_f64_until_abi() {
    let add_total = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(local(1)),
        right: Box::new(number(2.25)),
    };
    let scaled = Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(local(1)),
        right: Box::new(number(3.0)),
    };
    let returned = Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(local(2)),
        right: Box::new(number(0.75)),
    };
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "total".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(number(1.5)),
        },
        Stmt::Expr(Expr::LocalSet(1, Box::new(add_total))),
        Stmt::Let {
            id: 2,
            name: "scaled".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(scaled),
        },
        Stmt::Return(Some(returned)),
    ];

    let artifact = compile_artifact_json("representation_first_numeric_locals.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Let"
                && record["consumer"] == "ordinary_expr_value.let_init_f64"
                && record["local_id"] == 1
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
        }),
        "expected numeric let init to stay region-local f64:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "LocalSet"
                && record["consumer"] == "ordinary_expr_value.local_set_f64"
                && record["local_id"] == 1
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
        }),
        "expected numeric local assignment to stay region-local f64:\n{artifact:#}"
    );
    let binary_f64_count = records
        .iter()
        .filter(|record| {
            record["expr_kind"] == "Binary"
                && record["consumer"] == "ordinary_expr_value.numeric_binary_f64"
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
        })
        .count();
    assert!(
        binary_f64_count >= 3,
        "expected binary ops to stay region-local f64:\n{artifact:#}"
    );
    let materialized: Vec<_> = records
        .iter()
        .filter(|record| record["native_value_state"] == "materialized")
        .collect();
    assert_eq!(
        materialized.len(),
        1,
        "numeric locals should materialize only at the return ABI boundary:\n{artifact:#}"
    );
    let return_materialization = materialized[0];
    assert_eq!(return_materialization["consumer"], "materialize_js_value");
    assert_eq!(
        return_materialization["materialization_reason"],
        "return_abi"
    );
    assert_eq!(
        return_materialization["native_abi_transition"]["from_native_rep"],
        "f64"
    );
    assert_eq!(
        return_materialization["native_abi_transition"]["to_native_rep"],
        "js_value"
    );
}

#[test]
fn representation_first_boolean_locals_stay_i1_until_abi() {
    let not_flag = Expr::Unary {
        op: UnaryOp::Not,
        operand: Box::new(local(1)),
    };
    let numeric_cmp = Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(number(1.0)),
        right: Box::new(number(2.0)),
    };
    let bool_cmp = Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(local(1)),
        right: Box::new(Expr::Bool(false)),
    };
    let returned = Expr::Unary {
        op: UnaryOp::Not,
        operand: Box::new(local(3)),
    };
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "flag".to_string(),
            ty: Type::Boolean,
            mutable: true,
            init: Some(Expr::Bool(true)),
        },
        Stmt::Expr(Expr::LocalSet(1, Box::new(not_flag))),
        Stmt::Let {
            id: 2,
            name: "cmp".to_string(),
            ty: Type::Boolean,
            mutable: false,
            init: Some(numeric_cmp),
        },
        Stmt::Let {
            id: 3,
            name: "same".to_string(),
            ty: Type::Boolean,
            mutable: false,
            init: Some(bool_cmp),
        },
        Stmt::Return(Some(returned)),
    ];

    let artifact = compile_artifact_json_for_module(module_with_classes_and_params(
        "representation_first_boolean_locals.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        body,
    ));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Let"
                && record["consumer"] == "ordinary_expr_value.let_init_i1"
                && record["local_id"] == 1
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record["native_value_state"] == "region_local"
        }),
        "expected boolean let init to stay region-local i1:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "LocalSet"
                && record["consumer"] == "ordinary_expr_value.local_set_i1"
                && record["local_id"] == 1
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record["native_value_state"] == "region_local"
        }),
        "expected boolean local assignment to stay region-local i1:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Compare"
                && record["consumer"] == "ordinary_expr_value.numeric_compare_i1"
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record["native_value_state"] == "region_local"
        }),
        "expected numeric comparison to produce region-local i1:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Compare"
                && record["consumer"] == "ordinary_expr_value.boolean_compare_i1"
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record["native_value_state"] == "region_local"
        }),
        "expected boolean comparison to consume and produce region-local i1:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Unary"
                && record["consumer"] == "ordinary_expr_value.boolean_not_i1"
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record["native_value_state"] == "region_local"
        }),
        "expected boolean not to stay region-local i1:\n{artifact:#}"
    );
    let materialized: Vec<_> = records
        .iter()
        .filter(|record| record["native_value_state"] == "materialized")
        .collect();
    assert_eq!(
        materialized.len(),
        1,
        "boolean locals should materialize only at the return ABI boundary:\n{artifact:#}"
    );
    let return_materialization = materialized[0];
    assert_eq!(return_materialization["consumer"], "materialize_js_value");
    assert_eq!(
        return_materialization["materialization_reason"],
        "return_abi"
    );
    assert_eq!(
        return_materialization["native_abi_transition"]["from_native_rep"],
        "i1"
    );
    assert_eq!(
        return_materialization["native_abi_transition"]["op"],
        "bool_to_js_value"
    );
}

#[test]
fn artifact_records_uint8array_buffer_alloc_length_as_native_buffer_len() {
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "bytes".to_string(),
            ty: Type::Named("Uint8Array".to_string()),
            mutable: false,
            init: Some(Expr::BufferAlloc {
                size: Box::new(int(8)),
                fill: None,
                encoding: None,
            }),
        },
        Stmt::Return(Some(length(1))),
    ];

    let ir = compile_ir("artifact_uint8array_buffer_alloc_length.ts", body.clone());
    assert!(
        !ir.contains("call double @js_value_length_f64"),
        "native buffer-view length should not use the typed-array runtime length helper:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_uint8array_buffer_alloc_length.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Buffer.length"
                && record["consumer"] == "Buffer.length.native_buffer_len"
                && record["native_rep_name"] == "buffer_len"
                && record["llvm_ty"] == "i32"
        }),
        "expected Uint8Array-typed BufferAlloc length to use native BufferLen record:\n{artifact:#}"
    );
}

fn record_has_raw_f64_layout_fact(record: &serde_json::Value, list: &str, state: &str) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts
            .iter()
            .any(|fact| fact["kind"] == "raw_f64_layout" && fact["state"] == state)
    })
}

fn record_has_array_kind_fact(
    record: &serde_json::Value,
    list: &str,
    state: &str,
    detail: &str,
) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts.iter().any(|fact| {
            fact["kind"] == "array_kind"
                && fact["state"] == state
                && fact["fact_id"]
                    .as_str()
                    .is_some_and(|fact_id| fact_id.ends_with(detail))
        })
    })
}

fn record_has_scalar_method_summary_fact(
    record: &serde_json::Value,
    list: &str,
    state: &str,
) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts
            .iter()
            .any(|fact| fact["kind"] == "scalar_method_summary" && fact["state"] == state)
    })
}

fn record_has_scalar_method_summary_detail(
    record: &serde_json::Value,
    list: &str,
    state: &str,
    detail: &str,
) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts.iter().any(|fact| {
            fact["kind"] == "scalar_method_summary"
                && fact["state"] == state
                && fact["detail"] == detail
        })
    })
}

fn record_has_type_fact(
    record: &serde_json::Value,
    list: &str,
    fact_id: &str,
    state: &str,
) -> bool {
    record[list].as_array().is_some_and(|facts| {
        facts.iter().any(|fact| {
            fact["kind"] == "type_fact" && fact["fact_id"] == fact_id && fact["state"] == state
        })
    })
}

fn record_has_note(record: &serde_json::Value, expected: &str) -> bool {
    record["notes"]
        .as_array()
        .is_some_and(|notes| notes.iter().any(|note| note.as_str() == Some(expected)))
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

#[test]
fn small_bigint_literal_stays_i128_until_js_boundary() {
    let body = vec![Stmt::Return(Some(Expr::BigInt(
        "0x7fff_ffff_ffff_ffffn".to_string(),
    )))];
    let module = module_with_classes_and_params(
        "artifact_small_bigint_literal.ts",
        Vec::new(),
        Vec::new(),
        Type::BigInt,
        body,
    );
    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i64 @js_bigint_from_i128_parts")
            && !ir.contains("call i64 @js_bigint_from_string"),
        "small BigInt literals should allocate from native i128 parts, not parse strings:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BigInt"
                && record["consumer"] == "ordinary_expr_value.small_bigint_literal_i128"
                && record["native_rep_name"] == "small_bigint"
                && record["llvm_ty"] == "i128"
                && record["native_value_state"] == "region_local"
                && record_has_note(record, "proof=bigint_literal_fits_i128")
        }),
        "expected small BigInt literal to be recorded as region-local i128:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_small_bigint"
                && record["native_value_state"] == "materialized"
                && record["native_abi_transition"]["from_native_rep"] == "small_bigint"
                && record["native_abi_transition"]["to_native_rep"] == "js_value"
                && record["native_abi_transition"]["op"] == "bigint_box"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected small BigInt literal to box only at JS boundary:\n{artifact:#}"
    );
}

#[test]
fn oversized_bigint_literal_records_small_bigint_rejection_and_falls_back() {
    let too_wide = format!("0x1{}n", "0".repeat(32));
    let body = vec![Stmt::Return(Some(Expr::BigInt(too_wide)))];
    let module = module_with_classes_and_params(
        "artifact_oversized_bigint_literal.ts",
        Vec::new(),
        Vec::new(),
        Type::BigInt,
        body,
    );
    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i64 @js_bigint_from_string"),
        "oversized BigInt literals must keep the arbitrary-precision parser fallback:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BigInt"
                && record["consumer"] == "ordinary_expr_value.small_bigint_literal_rejected"
                && record["access_mode"] == "dynamic_fallback"
                && record_has_note(
                    record,
                    "small_bigint_rejected=literal_outside_i128_or_invalid",
                )
                && record_has_note(record, "fallback=js_bigint_from_string")
        }),
        "expected oversized BigInt literal rejection evidence before fallback:\n{artifact:#}"
    );
}

#[test]
fn packed_f64_loop_store_update_versions_with_side_exit() {
    let module = module_with_classes_and_params(
        "packed_f64_store_update_side_exit.ts",
        Vec::new(),
        vec![param(2, "delta", Type::Number)],
        Type::Number,
        vec![
            number_array_let(1, "values", vec![1, 2, 3]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    add(index_get(1, local(4)), local(2)),
                )],
            ),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "safe store-update loop should get a packed-f64 loop guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_f64_fast") && ir.contains("for.packed_f64_slow"),
        "safe store-update loop should emit fast and slow clones:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"),
        "fast store should keep a runtime numeric/layout store guard:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_array_numeric_value_to_raw_f64"),
        "fast store should canonicalize numeric values before raw f64 storage:\n{ir}"
    );

    let fallback_start = ir
        .find("\npacked_f64_loop_store.fallback.")
        .map(|pos| pos + 1)
        .expect("expected packed-f64 store fallback block");
    let fallback_tail = &ir[fallback_start..];
    let fallback_end = fallback_tail
        .find("\n\n")
        .map(|offset| fallback_start + offset)
        .unwrap_or(ir.len());
    let fallback_block = &ir[fallback_start..fallback_end];
    assert!(
        fallback_block.contains("br label %packed_f64.loop.slow.preheader."),
        "packed store guard failure must side-exit to the slow clone preheader:\n{fallback_block}\n\n{ir}"
    );
    assert!(
        !fallback_block.contains("js_typed_feedback_array_index_set_fallback_boxed"),
        "packed fast clone must not perform a boxed fallback before side-exiting:\n{fallback_block}\n\n{ir}"
    );
    let slow_start = ir
        .find("for.packed_f64_slow")
        .expect("expected packed-f64 slow clone");
    assert!(
        ir[slow_start..].contains("call double @js_typed_feedback_array_index_set_fallback_boxed"),
        "packed store side exit must preserve the generic boxed fallback in the slow clone:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopLoad"
                && record["consumer"] == "packed_f64_loop_load"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "RHS arr[i] should use a packed raw-f64 loop load:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopStore"
                && record["consumer"] == "packed_f64_loop_store"
                && record["access_mode"] == "checked_native"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "store_guard_failure=side_exit_slow_restart")
                })
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected checked packed raw-f64 loop store record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopStore"
                && record["consumer"] == "packed_f64_loop_store_side_exit"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "store_guard_failure=side_exit_slow_restart")
                })
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "rejected")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected packed store side-exit fallback evidence:\n{artifact:#}"
    );
}

#[test]
fn packed_i32_loop_read_materializes_integer_native_load_with_fallback() {
    let module = module_with_classes_and_params(
        "packed_i32_loop_read.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            int32_array_let(1, "values", vec![1, 2, 3]),
            number_let(3, "sum", true, int(0)),
            for_loop(
                4,
                length(1),
                vec![Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(bit_or_zero(add(local(3), index_get(1, local(4))))),
                ))],
            ),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard"),
        "packed-i32 loop should use the i32-specific raw numeric layout guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_i32_fast") && ir.contains("for.packed_i32_slow"),
        "packed-i32 loop should emit fast and slow clones:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_f64_fast"),
        "Int32[] read loop should be tagged as packed-i32, not packed-f64:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedI32LoopGuard"
                && record["consumer"] == "packed_i32_loop_guard"
                && record["access_mode"] == "checked_native"
                && record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_i32")
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected packed-i32 guard proof record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedI32LoopGuard"
                && record["consumer"] == "packed_i32_loop_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_array_kind_fact(record, "rejected_facts", "rejected", "packed_i32")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected packed-i32 generic fallback evidence:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedI32LoopLoad"
                && record["consumer"] == "packed_i32_loop_load"
                && record["native_rep_name"] == "i32"
                && record["llvm_ty"] == "i32"
                && record["access_mode"] == "checked_native"
                && record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_i32")
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                && record_has_note(record, "integer_materialization=fptosi_guarded_packed_i32")
        }),
        "expected packed-i32 loop load to materialize an i32 native value:\n{artifact:#}"
    );
}

#[test]
fn packed_u32_loop_read_materializes_unsigned_native_load_with_fallback() {
    let module = module_with_classes_and_params(
        "packed_u32_loop_read.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            u32_array_let(1, "values", vec![0, 4_000_000_000]),
            number_let(3, "word", true, ushr_zero(int(0))),
            for_loop(
                4,
                length(1),
                vec![Stmt::Expr(Expr::LocalSet(
                    3,
                    Box::new(ushr_zero(index_get(1, local(4)))),
                ))],
            ),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_u32_array_loop_guard"),
        "packed-u32 loop should use the u32-specific raw numeric layout guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_u32_fast") && ir.contains("for.packed_u32_slow"),
        "packed-u32 loop should emit fast and slow clones:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard")
            && !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "PerryU32[] read loop should not reuse signed-i32 or f64 loop guards:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedU32LoopGuard"
                && record["consumer"] == "packed_u32_loop_guard"
                && record["access_mode"] == "checked_native"
                && record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_u32")
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected packed-u32 guard proof record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedU32LoopGuard"
                && record["consumer"] == "packed_u32_loop_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_array_kind_fact(record, "rejected_facts", "rejected", "packed_u32")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected packed-u32 generic fallback evidence:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedU32LoopLoad"
                && record["consumer"] == "packed_u32_loop_load"
                && record["native_rep_name"] == "u32"
                && record["llvm_ty"] == "i32"
                && record["access_mode"] == "checked_native"
                && record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_u32")
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                && record_has_note(record, "integer_materialization=fptoui_guarded_packed_u32")
        }),
        "expected packed-u32 loop load to materialize a u32 native value:\n{artifact:#}"
    );
}

#[test]
fn packed_i32_loop_store_update_versions_with_side_exit() {
    let module = module_with_classes_and_params(
        "packed_i32_store_update_side_exit.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            int32_array_let(1, "values", vec![1, 2, 3]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    bit_or_zero(add(index_get(1, local(4)), int(1))),
                )],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard"),
        "safe Int32[] store-update loop should get a packed-i32 loop guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_i32_fast") && ir.contains("for.packed_i32_slow"),
        "safe Int32[] store-update loop should emit fast and slow clones:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "Int32[] store-update loop should not use the packed-f64 loop guard:\n{ir}"
    );
    let fast_start = ir
        .find("for.packed_i32_fast.body")
        .expect("expected packed-i32 fast body");
    let fast_tail = &ir[fast_start..];
    let fast_end = fast_tail
        .find("for.packed_i32_fast.update")
        .map(|offset| fast_start + offset)
        .unwrap_or(ir.len());
    let fast_body = &ir[fast_start..fast_end];
    assert!(
        fast_body.contains("fptosi double") && fast_body.contains("add i32"),
        "packed-i32 store RHS should stay in the i32 lane before the store guard:\n{fast_body}\n\n{ir}"
    );
    assert!(
        !fast_body.contains("js_array_numeric_value_to_raw_f64"),
        "packed-i32 loop body should not canonicalize through the f64 numeric store helper:\n{fast_body}"
    );
    let store_fast_start = ir
        .find("\npacked_i32_loop_store.fast.")
        .map(|pos| pos + 1)
        .expect("expected packed-i32 store fast block");
    let store_fast_tail = &ir[store_fast_start..];
    let store_fast_end = store_fast_tail
        .find("\npacked_i32_loop_store.fallback.")
        .map(|offset| store_fast_start + offset)
        .unwrap_or(ir.len());
    let store_fast = &ir[store_fast_start..store_fast_end];
    assert!(
        store_fast.contains("store double") && !store_fast.contains("js_array_numeric_value_to_raw_f64"),
        "packed-i32 fast store should write the exact f64 slot without f64 canonicalization:\n{store_fast}"
    );

    let fallback_start = ir
        .find("\npacked_i32_loop_store.fallback.")
        .map(|pos| pos + 1)
        .expect("expected packed-i32 store fallback block");
    let fallback_tail = &ir[fallback_start..];
    let fallback_end = fallback_tail
        .find("\n\n")
        .map(|offset| fallback_start + offset)
        .unwrap_or(ir.len());
    let fallback_block = &ir[fallback_start..fallback_end];
    assert!(
        fallback_block.contains("br label %packed_i32.loop.slow.preheader."),
        "packed-i32 store guard failure must side-exit to the slow clone preheader:\n{fallback_block}\n\n{ir}"
    );
    assert!(
        !fallback_block.contains("js_typed_feedback_array_index_set_fallback_boxed"),
        "packed-i32 fast clone must not perform boxed fallback before side-exiting:\n{fallback_block}\n\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedI32LoopStore"
                && record["consumer"] == "packed_i32_loop_store"
                && record["native_rep_name"] == "i32"
                && record["llvm_ty"] == "i32"
                && record["access_mode"] == "checked_native"
                && record_has_array_kind_fact(record, "consumed_facts", "consumed", "packed_i32")
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
                && record_has_note(record, "rhs_i32_store=sitofp_i32_to_raw_f64_slot")
                && record_has_note(record, "store_guard_failure=side_exit_slow_restart")
        }),
        "expected checked packed-i32 loop store record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedI32LoopStore"
                && record["consumer"] == "packed_i32_loop_store_side_exit"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_array_kind_fact(record, "rejected_facts", "rejected", "packed_i32")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected packed-i32 store side-exit fallback evidence:\n{artifact:#}"
    );
}

#[test]
fn packed_i32_loop_store_rejects_fractional_number_rhs() {
    let module = module_with_classes_and_params(
        "packed_i32_store_fractional_rhs_rejected.ts",
        Vec::new(),
        vec![param(2, "delta", Type::Number)],
        Type::Number,
        vec![
            int32_array_let(1, "values", vec![1, 2, 3]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    add(index_get(1, local(4)), local(2)),
                )],
            ),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard"),
        "fractional-capable number RHS must not get a packed-i32 store clone:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "fractional-capable Int32[] store RHS must not fall back to the packed-f64 store clone:\n{ir}"
    );
    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["expr_kind"].as_str(),
                Some(
                    "PackedI32LoopStore"
                        | "PackedI32LoopGuard"
                        | "PackedF64LoopStore"
                        | "PackedF64LoopGuard"
                )
            )
        }),
        "fractional-capable Int32[] store should not record packed loop store facts:\n{artifact:#}"
    );
}

/// #5464 follow-up: `PerryU32[]` stores have NO packed-u32 store fast path —
/// the IndexSet lowering routes U32 facts to the generic array-store path
/// (full-value F64 store; every uint32 is exactly representable in f64), and
/// the defensive U32 arm in `lower_packed_numeric_loop_store_value` degrades
/// to the same full-value store instead of `bail!`ing. This pins the fast/slow
/// round-trip equivalence for the unsigned lane: whichever loop clone runs,
/// the stored element is the exact ToUint32 value (`(v + 1) >>> 0`), so a
/// wrap-range update (`4_000_000_000 + 1`) reads back identically — it must
/// never detour through the SIGNED packed-i32 lane, whose `fptosi` would flip
/// values above `i32::MAX` negative.
#[test]
fn packed_u32_loop_store_routes_to_generic_full_value_store() {
    let module = module_with_classes_and_params(
        "packed_u32_store_generic_routing.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            u32_array_let(1, "values", vec![0, 4_000_000_000]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    ushr_zero(add(index_get(1, local(4)), int(1))),
                )],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        !ir.contains("packed_u32_loop_store."),
        "PerryU32[] store must not emit a packed-u32 store block (no such fast path):\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_i32_array_loop_guard"),
        "PerryU32[] store loop must not claim the signed packed-i32 guard:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "PackedU32LoopStore"
                || record["consumer"] == "packed_u32_loop_store"
        }),
        "PerryU32[] stores must route to the generic array-store path, not a packed-u32 store:\n{artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("PackedI32Loop"))
        }),
        "PerryU32[] store loop must not record signed packed-i32 loop facts:\n{artifact:#}"
    );
}

#[test]
fn packed_f64_loop_unary_math_store_versions_with_side_exit() {
    let module = module_with_classes_and_params(
        "packed_f64_unary_math_store_side_exit.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            number_array_let(1, "values", vec![-1, 2, -3]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    Expr::MathAbs(Box::new(index_get(1, local(4)))),
                )],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "unary numeric math store loop should get a packed-f64 loop guard:\n{ir}"
    );
    assert!(
        ir.contains("for.packed_f64_fast") && ir.contains("for.packed_f64_slow"),
        "unary numeric math store loop should emit fast and slow clones:\n{ir}"
    );
    assert!(
        ir.contains("call double @llvm.fabs.f64"),
        "fast RHS should lower Math.abs over arr[i] as native f64 math:\n{ir}"
    );
    let fast_body_start = ir
        .find("for.packed_f64_fast.body")
        .expect("expected packed-f64 fast body");
    let fast_body_tail = &ir[fast_body_start..];
    let fast_body_end = fast_body_tail
        .find("for.packed_f64_fast.update")
        .map(|offset| fast_body_start + offset)
        .unwrap_or(ir.len());
    let fast_body = &ir[fast_body_start..fast_body_end];
    assert!(
        !fast_body.contains("js_math_to_number"),
        "packed fast body must not route Math.abs(arr[i]) through JSValue ToNumber:\n{fast_body}\n\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"),
        "fast unary math store should keep a runtime numeric/layout store guard:\n{ir}"
    );

    let fallback_start = ir
        .find("\npacked_f64_loop_store.fallback.")
        .map(|pos| pos + 1)
        .expect("expected packed-f64 store fallback block");
    let fallback_tail = &ir[fallback_start..];
    let fallback_end = fallback_tail
        .find("\n\n")
        .map(|offset| fallback_start + offset)
        .unwrap_or(ir.len());
    let fallback_block = &ir[fallback_start..fallback_end];
    assert!(
        fallback_block.contains("br label %packed_f64.loop.slow.preheader."),
        "unary math packed store guard failure must side-exit to the slow clone preheader:\n{fallback_block}\n\n{ir}"
    );
    assert!(
        !fallback_block.contains("js_typed_feedback_array_index_set_fallback_boxed"),
        "unary math packed fast clone must not perform a boxed fallback before side-exiting:\n{fallback_block}\n\n{ir}"
    );
    let slow_start = ir
        .find("for.packed_f64_slow")
        .expect("expected packed-f64 slow clone");
    assert!(
        ir[slow_start..].contains("call double @js_math_to_number"),
        "unary math packed store side exit must restart in the slow clone that preserves ToNumber semantics:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopLoad"
                && record["consumer"] == "packed_f64_loop_load"
                && record["access_mode"] == "checked_native"
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "Math.abs operand arr[i] should use a packed raw-f64 loop load:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopStore"
                && record["consumer"] == "packed_f64_loop_store"
                && record["access_mode"] == "checked_native"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "store_guard_failure=side_exit_slow_restart")
                        && notes
                            .iter()
                            .any(|note| note == "rhs_unary_math=llvm.fabs.f64")
                })
                && record_has_raw_f64_layout_fact(record, "consumed_facts", "consumed")
        }),
        "expected checked packed raw-f64 loop store record for unary math RHS:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PackedF64LoopStore"
                && record["consumer"] == "packed_f64_loop_store_side_exit"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record["fallback_reason"] == "runtime_api"
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "rejected")
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "expected unary math packed store side-exit fallback evidence:\n{artifact:#}"
    );
}

#[test]
fn packed_f64_loop_rejects_coercive_unary_math_store_rhs() {
    let module = module_with_classes_and_params(
        "packed_f64_unary_math_store_coercion_rejected.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            number_array_let(1, "values", vec![1, 2, 3]),
            for_loop(
                4,
                length(1),
                vec![array_set(
                    1,
                    local(4),
                    Expr::MathAbs(Box::new(Expr::String("2".to_string()))),
                )],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "Math.abs over a coercive JSValue operand must not get a packed-f64 fast clone:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_f64_fast"),
        "coercive unary math store body must stay out of the packed-f64 fast clone:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_math_to_number"),
        "negative case must exercise the generic ToNumber-preserving math path:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["expr_kind"].as_str(),
                Some("PackedF64LoopGuard" | "PackedF64LoopStore" | "PackedF64LoopLoad")
            )
        }),
        "coercive unary math store loop should not record packed-f64 loop facts:\n{artifact:#}"
    );
}

#[test]
fn map_string_number_set_has_use_string_key_specialization() {
    let module = module_with_classes_and_params(
        "map_string_number_specialization.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Number),
        ],
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Number),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::If {
                condition: Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::MapGet {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }))],
                else_branch: Some(vec![Stmt::Return(Some(Expr::Number(0.0)))]),
            },
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_string_number_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_number"),
        "Map<string, number>.set should lower through the string-key/f64 helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_i32"),
        "Map<string, number>.set should not use the narrower int32 value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_u32"),
        "Map<string, number>.set should not use the narrower uint32 value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_f32"),
        "Map<string, number>.set should not use the narrower float32 value helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_map_has_string_key"),
        "Map<string, number>.has should lower through the string-key helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call double @js_map_get_string_key"),
        "Map<string, number>.get should lower through the string-key helper while keeping boxed miss semantics:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "specialized map.set path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call double @js_map_get("),
        "specialized map.get path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_map_has("),
        "specialized map.has path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn map_number_key_set_get_has_delete_use_guarded_number_key_specialization() {
    let module = module_with_classes_and_params(
        "map_number_key_specialization.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::Number),
            param(3, "value", Type::Boolean),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapGet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_number_key_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i32 @js_typed_f64_arg_guard")
            && probe_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "Map<number, V> specialization should guard then unbox the key to raw f64:\n{probe_ir}"
    );
    for helper in [
        "call i64 @js_map_set_number_key",
        "call i32 @js_map_has_number_key",
        "call double @js_map_get_number_key",
        "call i32 @js_map_delete_number_key",
    ] {
        assert!(
            probe_ir.contains(helper),
            "Map<number, V> should use guarded numeric-key helper {helper}:\n{probe_ir}"
        );
    }
    for fallback in [
        "call i64 @js_map_set(",
        "call i32 @js_map_has(",
        "call double @js_map_get(",
        "call i32 @js_map_delete(",
    ] {
        assert!(
            probe_ir.contains(fallback),
            "numeric-key guard failure must preserve generic fallback {fallback}:\n{probe_ir}"
        );
    }
    for string_helper in [
        "@js_map_set_string_key",
        "@js_map_set_string_bool",
        "@js_map_has_string_key",
        "@js_map_delete_string_key",
    ] {
        assert!(
            !probe_ir.contains(string_helper),
            "numeric-key map lowering must not use string-key helper {string_helper}:\n{probe_ir}"
        );
    }
}

#[test]
fn map_number_key_string_value_set_uses_string_ref_until_slot() {
    let module = module_with_classes_and_params(
        "map_number_string_value_specialization.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::Number),
            param(3, "value", Type::String),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::String),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_number_string_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i32 @js_typed_f64_arg_guard")
            && probe_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "Map<number, string>.set should keep the existing guarded numeric-key path:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i64 @js_get_string_pointer_unified"),
        "proven string values should be unboxed to a raw string handle before the map slot boundary:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_number_key"),
        "Map<number, string>.set should still use the numeric-key helper at the slot boundary:\n{probe_ir}"
    );
    for string_key_helper in [
        "@js_map_set_string_key",
        "@js_map_set_string_string",
        "@js_map_has_string_key",
    ] {
        assert!(
            !probe_ir.contains(string_key_helper),
            "numeric-key string-value lowering must not use string-key helper {string_key_helper}:\n{probe_ir}"
        );
    }

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_typed_value.map_set_number_string"
                && record["native_rep_name"] == "string_ref"
                && record["llvm_ty"] == "i64"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_number_key")
                && record_has_note(record, "value_rep=string_ref")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected Map<number, string>.set typed string-value selection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_number_key.map_set"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.number_key_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_number_key")
        }),
        "expected Map<number, string>.set numeric-key selection record:\n{artifact:#}"
    );
}

#[test]
fn map_number_key_string_value_rejects_unproven_value() {
    let module = module_with_classes_and_params(
        "map_number_string_value_rejection.ts",
        Vec::new(),
        vec![param(2, "key", Type::Number), param(3, "value", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::String),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_number_string_value_rejection_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_number_key"),
        "unproven string values should preserve the guarded numeric-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_get_string_pointer_unified"),
        "unproven values must not be unboxed as string refs:\n{probe_ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_typed_value.map_set_number_string_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.string_value_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_set_number_key")
                && record_has_note(
                    record,
                    "typed_collection_rejected=value_expr_not_definitely_string",
                )
                && record_has_note(record, "value_rep=js_value")
        }),
        "expected Map<number, string>.set unproven string-value rejection record:\n{artifact:#}"
    );
}

#[test]
fn map_string_key_has_delete_specialize_independent_of_value_type() {
    let module = module_with_classes_and_params(
        "map_string_boolean_delete_specialization.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_boolean_delete_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_bool"),
        "Map<string, boolean>.set should lower through the typed boolean string-key helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_map_has_string_key"),
        "Map<string, boolean>.has should lower through the string-key helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_map_delete_string_key"),
        "Map<string, boolean>.delete should lower through the string-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "specialized string-key map.set path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "typed boolean string-key map.set path should not call the generic-value string-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_map_has("),
        "specialized string-key map.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_map_delete("),
        "specialized string-key map.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_boolean_param_without_native_i1_proof_uses_generic_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_boolean_param_fallback.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Boolean),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_string_boolean_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_key"),
        "annotation-only boolean map values should keep the generic-value string-key helper until a native-i1 proof exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_bool"),
        "annotation-only boolean map values must not use the raw bool helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "static string-key map.set should still avoid the fully generic helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_int32_set_uses_typed_i32_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_int32_value_specialization.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Int32),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Integer(42)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_int32_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_i32"),
        "Map<string, Int32>.set should lower through the typed int32-value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_number"),
        "typed int32-value map.set should avoid the f64 number helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "typed int32-value map.set should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "typed int32-value map.set should not call the generic-value string-key helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_int32_param_without_native_i32_proof_uses_f64_helper() {
    let module = module_with_classes_and_params(
        "map_string_int32_param_fallback.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Int32),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Int32),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_string_int32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_number"),
        "annotation-only Int32 values should keep the f64 helper until a native-i32 proof or guard exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_i32"),
        "annotation-only Int32 values must not use the raw i32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn map_string_u32_set_uses_typed_u32_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_u32_value_specialization.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_u32_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_u32"),
        "Map<string, PerryU32>.set should lower through the typed uint32-value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_number"),
        "typed uint32-value map.set should avoid the f64 number helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "typed uint32-value map.set should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "typed uint32-value map.set should not call the generic-value string-key helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_u32_param_without_native_u32_proof_uses_generic_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_u32_param_fallback.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Named("PerryU32".to_string())),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_string_u32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_key"),
        "annotation-only PerryU32 values should keep the generic-value string-key helper until a native-u32 proof or guard exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_u32"),
        "annotation-only PerryU32 values must not use the raw u32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn map_string_f32_set_uses_typed_f32_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_f32_value_specialization.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_f32_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_f32"),
        "Map<string, PerryF32>.set should lower through the typed float32-value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_number"),
        "typed float32-value map.set should avoid the f64 number helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "typed float32-value map.set should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "typed float32-value map.set should not call the generic-value string-key helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_f32_param_without_native_f32_proof_uses_generic_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_f32_param_fallback.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Named("PerryF32".to_string())),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_string_f32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_key"),
        "annotation-only PerryF32 values should keep the generic-value string-key helper until a native-f32 proof or guard exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_f32"),
        "annotation-only PerryF32 values must not use the raw f32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn map_string_string_set_uses_typed_string_value_helper() {
    let module = module_with_classes_and_params(
        "map_string_string_value_specialization.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::String),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::String),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_string_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_string"),
        "Map<string, string>.set should lower through the typed string-value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set("),
        "specialized string-value map.set path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "typed string-value map.set path should not call the generic-value string-key helper:\n{probe_ir}"
    );
}

#[test]
fn map_string_any_set_uses_generic_value_string_key_helper() {
    let module = module_with_classes_and_params(
        "map_string_any_value_specialization.ts",
        Vec::new(),
        vec![param(2, "key", Type::String), param(3, "value", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Any),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(
        &ir,
        "perry_fn_map_string_any_value_specialization_ts__probe",
    );
    assert!(
        probe_ir.contains("call i64 @js_map_set_string_key"),
        "Map<string, any>.set should keep the generic-value string-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_string"),
        "unproven string values must not use the typed string-value helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_bool"),
        "unproven values must not use the typed boolean helper:\n{probe_ir}"
    );
}

#[test]
fn map_unproven_number_key_keeps_generic_fallback() {
    let module = module_with_classes_and_params(
        "map_number_unproven_key_generic.ts",
        Vec::new(),
        vec![param(2, "key", Type::Any), param(3, "value", Type::Boolean)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_map_number_unproven_key_generic_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_map_set("),
        "Map<number, boolean>.set with an unproven key should keep the generic helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_map_has("),
        "Map<number, boolean>.has with an unproven key should keep the generic helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_map_delete("),
        "Map<number, boolean>.delete with an unproven key should keep the generic helper:\n{probe_ir}"
    );
    for number_helper in [
        "@js_map_set_number_key",
        "@js_map_has_number_key",
        "@js_map_get_number_key",
        "@js_map_delete_number_key",
    ] {
        assert!(
            !probe_ir.contains(number_helper),
            "unproven numeric map keys must not use helper {number_helper}:\n{probe_ir}"
        );
    }
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_bool"),
        "non-string map.set must not use the string-key boolean helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_map_set_string_key"),
        "non-string map.set must not use the string-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_map_has_string_key"),
        "non-string map.has must not use the string-key helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_map_delete_string_key"),
        "non-string map.delete must not use the string-key helper:\n{probe_ir}"
    );
}

#[test]
fn artifact_records_map_string_key_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_map_string_key_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapHas"
                && record["consumer"] == "collection_string_key.map_has"
                && record["native_rep_name"] == "string_ref"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_has_string_key")
                && record_has_note(record, "boxed_key_avoided=true")
        }),
        "expected map.has string-key helper selection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapDelete"
                && record["consumer"] == "collection_string_key.map_delete"
                && record["native_rep_name"] == "string_ref"
                && record["llvm_ty"] == "i64"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_delete_string_key")
        }),
        "expected map.delete string-key helper selection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_bool"
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.boolean_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_string_bool")
                && record_has_note(record, "value_rep=i1")
                && record_has_note(record, "boxed_key_avoided=true")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected map.set typed-boolean string-key helper selection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_bool_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_bool")
        }),
        "expected map.set typed-boolean string-key helper key record:\n{artifact:#}"
    );

    let boolean_fallback_module = module_with_classes_and_params(
        "artifact_map_string_boolean_value_rejection.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Boolean),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let boolean_fallback_artifact = compile_artifact_json_for_module(boolean_fallback_module);
    let boolean_fallback_records = boolean_fallback_artifact["records"].as_array().unwrap();
    assert!(
        boolean_fallback_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_key")
                && record_has_note(record, "boxed_key_avoided=true")
        }),
        "expected annotation-only boolean map.set to use generic-value string-key helper:\n{boolean_fallback_artifact:#}"
    );
    assert!(
        boolean_fallback_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_typed_value.map_set_string_bool_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.boolean_value_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_set_string_key")
                && record_has_note(record, "typed_collection_rejected=value_expr_not_native_i1")
                && record_has_note(record, "value_rep=js_value")
        }),
        "expected annotation-only boolean map.set typed-value rejection record:\n{boolean_fallback_artifact:#}"
    );

    let selected_i32_value_module = module_with_classes_and_params(
        "artifact_map_string_i32_value_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Int32),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Integer(42)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let i32_value_artifact = compile_artifact_json_for_module(selected_i32_value_module);
    let i32_value_records = i32_value_artifact["records"].as_array().unwrap();
    assert!(
        i32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_i32"
                && record["native_rep_name"] == "i32"
                && record["llvm_ty"] == "i32"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.int32_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_string_i32")
                && record_has_note(record, "value_rep=i32")
                && record_has_note(record, "boxed_key_avoided=true")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected map.set typed-int32 string-key helper value record:\n{i32_value_artifact:#}"
    );
    assert!(
        i32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_i32_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_i32")
        }),
        "expected map.set typed-int32 string-key helper key record:\n{i32_value_artifact:#}"
    );

    let selected_u32_value_module = module_with_classes_and_params(
        "artifact_map_string_u32_value_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let u32_value_artifact = compile_artifact_json_for_module(selected_u32_value_module);
    let u32_value_records = u32_value_artifact["records"].as_array().unwrap();
    assert!(
        u32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_u32"
                && record["native_rep_name"] == "u32"
                && record["llvm_ty"] == "i32"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.uint32_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_string_u32")
                && record_has_note(record, "value_rep=u32")
                && record_has_note(record, "boxed_key_avoided=true")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected map.set typed-uint32 string-key helper value record:\n{u32_value_artifact:#}"
    );
    assert!(
        u32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_u32_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_u32")
        }),
        "expected map.set typed-uint32 string-key helper key record:\n{u32_value_artifact:#}"
    );

    let selected_f32_value_module = module_with_classes_and_params(
        "artifact_map_string_f32_value_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let f32_value_artifact = compile_artifact_json_for_module(selected_f32_value_module);
    let f32_value_records = f32_value_artifact["records"].as_array().unwrap();
    assert!(
        f32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_f32"
                && record["native_rep_name"] == "f32"
                && record["llvm_ty"] == "float"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.float32_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_string_f32")
                && record_has_note(record, "value_rep=f32")
                && record_has_note(record, "boxed_key_avoided=true")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected map.set typed-float32 string-key helper value record:\n{f32_value_artifact:#}"
    );
    assert!(
        f32_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_f32_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_f32")
        }),
        "expected map.set typed-float32 string-key helper key record:\n{f32_value_artifact:#}"
    );

    let selected_string_value_module = module_with_classes_and_params(
        "artifact_map_string_value_selection.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::String),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::String),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let string_value_artifact = compile_artifact_json_for_module(selected_string_value_module);
    let string_value_records = string_value_artifact["records"].as_array().unwrap();
    assert!(
        string_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_string"
                && record["native_rep_name"] == "string_ref"
                && record["llvm_ty"] == "i64"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_value_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_set_string_string")
                && record_has_note(record, "value_rep=string_ref")
                && record_has_note(record, "boxed_key_avoided=true")
                && record_has_note(record, "boxed_value_avoided_until_map_slot=true")
        }),
        "expected map.set typed-string string-key helper value record:\n{string_value_artifact:#}"
    );
    assert!(
        string_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_string_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_string")
        }),
        "expected map.set typed-string string-key helper key record:\n{string_value_artifact:#}"
    );

    let generic_value_module = module_with_classes_and_params(
        "artifact_map_string_any_value_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::String), param(3, "value", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Any),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let generic_value_artifact = compile_artifact_json_for_module(generic_value_module);
    let generic_value_records = generic_value_artifact["records"].as_array().unwrap();
    assert!(
        generic_value_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_string_key"
                && record["native_rep_name"] == "string_ref"
                && record_has_note(record, "selected_helper=js_map_set_string_key")
                && record_has_note(record, "boxed_key_avoided=true")
        }),
        "expected map.set generic-value string-key helper record:\n{generic_value_artifact:#}"
    );

    let selected_get_module = module_with_classes_and_params(
        "artifact_map_string_get_selection.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::String),
            param(3, "value", Type::Number),
        ],
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::String, Type::Number),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::If {
                condition: Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::MapGet {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }))],
                else_branch: Some(vec![Stmt::Return(Some(Expr::Number(0.0)))]),
            },
        ],
    );
    let get_artifact = compile_artifact_json_for_module(selected_get_module);
    let get_records = get_artifact["records"].as_array().unwrap();
    assert!(
        get_records.iter().any(|record| {
            record["expr_kind"] == "MapGet"
                && record["consumer"] == "collection_string_key.map_get"
                && record["native_rep_name"] == "string_ref"
                && record["llvm_ty"] == "i64"
                && record_has_type_fact(
                    record,
                    "consumed_facts",
                    "map.string_key_helper",
                    "consumed",
                )
                && record_has_note(record, "selected_helper=js_map_get_string_key")
                && record_has_note(record, "boxed_key_avoided=true")
        }),
        "expected map.get string-key helper selection record:\n{get_artifact:#}"
    );

    let fallback_module = module_with_classes_and_params(
        "artifact_map_non_string_non_number_key_rejection.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::Boolean),
            param(3, "value", Type::Boolean),
        ],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Boolean, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );
    let artifact = compile_artifact_json_for_module(fallback_module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_set")
                && record_has_note(
                    record,
                    "typed_collection_rejected=receiver_or_key_not_static_string",
                )
        }),
        "expected map.set non-string-key rejection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapHas"
                && record["consumer"] == "collection_string_key.map_has_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_has")
                && record_has_note(
                    record,
                    "typed_collection_rejected=receiver_or_key_not_static_string",
                )
        }),
        "expected map.has non-string-key rejection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MapDelete"
                && record["consumer"] == "collection_string_key.map_delete_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_delete")
        }),
        "expected map.delete non-string-key rejection record:\n{artifact:#}"
    );

    let fallback_get_module = module_with_classes_and_params(
        "artifact_map_non_string_non_number_get_rejection.ts",
        Vec::new(),
        vec![
            param(2, "key", Type::Boolean),
            param(3, "value", Type::Number),
        ],
        Type::Number,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Boolean, Type::Number),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(local(3)),
            }),
            Stmt::If {
                condition: Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                },
                then_branch: vec![Stmt::Return(Some(Expr::MapGet {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }))],
                else_branch: Some(vec![Stmt::Return(Some(Expr::Number(0.0)))]),
            },
        ],
    );
    let get_artifact = compile_artifact_json_for_module(fallback_get_module);
    let get_records = get_artifact["records"].as_array().unwrap();
    assert!(
        get_records.iter().any(|record| {
            record["expr_kind"] == "MapGet"
                && record["consumer"] == "collection_string_key.map_get_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "map.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_map_get")
                && record_has_note(
                    record,
                    "typed_collection_rejected=receiver_or_key_not_static_string",
                )
        }),
        "expected map.get non-string-key rejection record:\n{get_artifact:#}"
    );
}

#[test]
fn artifact_records_map_number_key_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_map_number_key_selection.ts",
        Vec::new(),
        vec![param(2, "key", Type::Number)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Let {
                id: 4,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::MapHas {
                    map: Box::new(local(1)),
                    key: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::MapDelete {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(4))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "MapSet",
            "collection_number_key.map_set",
            "js_map_set_number_key",
        ),
        (
            "MapHas",
            "collection_number_key.map_has",
            "js_map_has_number_key",
        ),
        (
            "MapDelete",
            "collection_number_key.map_delete",
            "js_map_delete_number_key",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "f64"
                    && record["llvm_ty"] == "double"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "map.number_key_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "key_rep=raw_f64")
                    && record_has_note(record, "key_guard=js_typed_f64_arg_guard")
            }),
            "expected map numeric-key helper selection record {consumer}:\n{artifact:#}"
        );
    }
    for (expr_kind, consumer, helper) in [
        (
            "MapSet",
            "collection_number_key.map_set_generic",
            "js_map_set",
        ),
        (
            "MapHas",
            "collection_number_key.map_has_generic",
            "js_map_has",
        ),
        (
            "MapDelete",
            "collection_number_key.map_delete_generic",
            "js_map_delete",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "map.number_key_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(record, "typed_collection_rejected=runtime_key_guard_failed")
                    && record_has_note(record, "key_rep=js_value")
            }),
            "expected map numeric-key guarded fallback record {consumer}:\n{artifact:#}"
        );
    }

    let rejected_module = module_with_classes_and_params(
        "artifact_map_number_key_rejection.ts",
        Vec::new(),
        vec![param(2, "key", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "m".to_string(),
                ty: map_type(Type::Number, Type::Boolean),
                mutable: true,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::MapSet {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Return(Some(Expr::MapHas {
                map: Box::new(local(1)),
                key: Box::new(local(2)),
            })),
        ],
    );
    let rejected_artifact = compile_artifact_json_for_module(rejected_module);
    let rejected_records = rejected_artifact["records"].as_array().unwrap();
    assert!(
        rejected_records.iter().any(|record| {
            record["expr_kind"] == "MapSet"
                && record["consumer"] == "collection_string_key.map_set_generic"
                && record_has_note(record, "generic_helper=js_map_set")
                && record_has_note(record, "typed_collection_rejected=receiver_or_key_not_static_string")
        }),
        "unproven numeric-key map path should still record generic fallback evidence:\n{rejected_artifact:#}"
    );
}

#[test]
fn set_string_add_has_delete_use_string_specialization() {
    let module = module_with_classes_and_params(
        "set_string_specialization.ts",
        Vec::new(),
        vec![param(2, "value", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::String),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_string_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add_string"),
        "Set<string>.add should lower through the string helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has_string"),
        "Set<string>.has should lower through the string helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete_string"),
        "Set<string>.delete should lower through the string helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i64 @js_get_string_pointer_unified"),
        "Set<string> selected path should lower proven values to raw StringRef handles before helper calls:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add("),
        "specialized set.add path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has("),
        "specialized set.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete("),
        "specialized set.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn set_number_add_has_delete_use_guarded_number_specialization() {
    let module = module_with_classes_and_params(
        "set_number_specialization.ts",
        Vec::new(),
        vec![param(2, "value", Type::Number)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Number),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_number_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i32 @js_typed_f64_arg_guard")
            && probe_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "Set<number> specialization should guard then unbox the value to raw f64:\n{probe_ir}"
    );
    for helper in [
        "call i64 @js_set_add_number",
        "call i32 @js_set_has_number",
        "call i32 @js_set_delete_number",
    ] {
        assert!(
            probe_ir.contains(helper),
            "Set<number> should use guarded numeric helper {helper}:\n{probe_ir}"
        );
    }
    for fallback in [
        "call i64 @js_set_add(",
        "call i32 @js_set_has(",
        "call i32 @js_set_delete(",
    ] {
        assert!(
            probe_ir.contains(fallback),
            "numeric Set guard failure must preserve generic fallback {fallback}:\n{probe_ir}"
        );
    }
}

#[test]
fn set_number_specialization_rejects_unproven_value() {
    let module = module_with_classes_and_params(
        "set_number_unproven.ts",
        Vec::new(),
        vec![param(2, "value", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Number),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(Expr::SetHas {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_number_unproven_ts__probe");
    for helper in [
        "@js_set_add_number",
        "@js_set_has_number",
        "@js_set_delete_number",
    ] {
        assert!(
            !probe_ir.contains(helper),
            "unproven Set<number> values must not use helper {helper}:\n{probe_ir}"
        );
    }
    assert!(
        probe_ir.contains("call i64 @js_set_add(") && probe_ir.contains("call i32 @js_set_has("),
        "unproven value path should call generic Set helpers:\n{probe_ir}"
    );
}

#[test]
fn set_int32_add_has_delete_use_i32_specialization() {
    let module = module_with_classes_and_params(
        "set_int32_specialization.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Int32),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Integer(42)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Integer(42)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Integer(42)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_int32_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add_i32"),
        "Set<Int32>.add should lower through the raw int32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has_i32"),
        "Set<Int32>.has should lower through the raw int32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete_i32"),
        "Set<Int32>.delete should lower through the raw int32 helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add("),
        "specialized int32 set.add path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has("),
        "specialized int32 set.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete("),
        "specialized int32 set.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn set_int32_param_without_native_i32_proof_uses_generic_helpers() {
    let module = module_with_classes_and_params(
        "set_int32_param_fallback.ts",
        Vec::new(),
        vec![param(2, "value", Type::Int32)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Int32),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_int32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add("),
        "annotation-only Int32 Set.add should keep the generic helper until a native-i32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has("),
        "annotation-only Int32 Set.has should keep the generic helper until a native-i32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete("),
        "annotation-only Int32 Set.delete should keep the generic helper until a native-i32 proof exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add_i32"),
        "annotation-only Int32 Set.add must not use the raw int32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has_i32"),
        "annotation-only Int32 Set.has must not use the raw int32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete_i32"),
        "annotation-only Int32 Set.delete must not use the raw int32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn set_u32_add_has_delete_use_u32_specialization() {
    let module = module_with_classes_and_params(
        "set_u32_specialization.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Integer(4_000_000_000)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_u32_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add_u32"),
        "Set<PerryU32>.add should lower through the raw uint32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has_u32"),
        "Set<PerryU32>.has should lower through the raw uint32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete_u32"),
        "Set<PerryU32>.delete should lower through the raw uint32 helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add("),
        "specialized uint32 set.add path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has("),
        "specialized uint32 set.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete("),
        "specialized uint32 set.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn set_u32_param_without_native_u32_proof_uses_generic_helpers() {
    let module = module_with_classes_and_params(
        "set_u32_param_fallback.ts",
        Vec::new(),
        vec![param(2, "value", Type::Named("PerryU32".to_string()))],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_u32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add("),
        "annotation-only PerryU32 Set.add should keep the generic helper until a native-u32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has("),
        "annotation-only PerryU32 Set.has should keep the generic helper until a native-u32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete("),
        "annotation-only PerryU32 Set.delete should keep the generic helper until a native-u32 proof exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add_u32"),
        "annotation-only PerryU32 Set.add must not use the raw u32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has_u32"),
        "annotation-only PerryU32 Set.has must not use the raw u32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete_u32"),
        "annotation-only PerryU32 Set.delete must not use the raw u32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn set_f32_add_has_delete_use_f32_specialization() {
    let module = module_with_classes_and_params(
        "set_f32_specialization.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Number(1.5)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_f32_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add_f32"),
        "Set<PerryF32>.add should lower through the raw float32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has_f32"),
        "Set<PerryF32>.has should lower through the raw float32 helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete_f32"),
        "Set<PerryF32>.delete should lower through the raw float32 helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add("),
        "specialized float32 set.add path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has("),
        "specialized float32 set.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete("),
        "specialized float32 set.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn set_f32_param_without_native_f32_proof_uses_generic_helpers() {
    let module = module_with_classes_and_params(
        "set_f32_param_fallback.ts",
        Vec::new(),
        vec![param(2, "value", Type::Named("PerryF32".to_string()))],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_f32_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add("),
        "annotation-only PerryF32 Set.add should keep the generic helper until a native-f32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has("),
        "annotation-only PerryF32 Set.has should keep the generic helper until a native-f32 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete("),
        "annotation-only PerryF32 Set.delete should keep the generic helper until a native-f32 proof exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add_f32"),
        "annotation-only PerryF32 Set.add must not use the raw f32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has_f32"),
        "annotation-only PerryF32 Set.has must not use the raw f32 helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete_f32"),
        "annotation-only PerryF32 Set.delete must not use the raw f32 helper without proof:\n{probe_ir}"
    );
}

#[test]
fn set_boolean_add_has_delete_use_bool_specialization() {
    let module = module_with_classes_and_params(
        "set_boolean_specialization.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Boolean),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Bool(true)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_boolean_specialization_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add_bool"),
        "Set<boolean>.add should lower through the raw boolean helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has_bool"),
        "Set<boolean>.has should lower through the raw boolean helper:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete_bool"),
        "Set<boolean>.delete should lower through the raw boolean helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add("),
        "specialized boolean set.add path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has("),
        "specialized boolean set.has path should not call the generic helper:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete("),
        "specialized boolean set.delete path should not call the generic helper:\n{probe_ir}"
    );
}

#[test]
fn set_boolean_param_without_native_i1_proof_uses_generic_helpers() {
    let module = module_with_classes_and_params(
        "set_boolean_param_fallback.ts",
        Vec::new(),
        vec![param(2, "value", Type::Boolean)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Boolean),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    let probe_ir = function_ir_section(&ir, "perry_fn_set_boolean_param_fallback_ts__probe");
    assert!(
        probe_ir.contains("call i64 @js_set_add("),
        "annotation-only boolean Set.add should keep the generic helper until a native-i1 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_has("),
        "annotation-only boolean Set.has should keep the generic helper until a native-i1 proof exists:\n{probe_ir}"
    );
    assert!(
        probe_ir.contains("call i32 @js_set_delete("),
        "annotation-only boolean Set.delete should keep the generic helper until a native-i1 proof exists:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i64 @js_set_add_bool"),
        "annotation-only boolean Set.add must not use the raw bool helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_has_bool"),
        "annotation-only boolean Set.has must not use the raw bool helper without proof:\n{probe_ir}"
    );
    assert!(
        !probe_ir.contains("call i32 @js_set_delete_bool"),
        "annotation-only boolean Set.delete must not use the raw bool helper without proof:\n{probe_ir}"
    );
}

#[test]
fn artifact_records_set_string_key_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_string_key_selection.ts",
        Vec::new(),
        vec![param(2, "value", Type::String)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::String),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_string_key.set_add",
            "js_set_add_string",
        ),
        (
            "SetHas",
            "collection_string_key.set_has",
            "js_set_has_string",
        ),
        (
            "SetDelete",
            "collection_string_key.set_delete",
            "js_set_delete_string",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "string_ref"
                    && record["llvm_ty"] == "i64"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.string_key_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "boxed_key_avoided=true")
            }),
            "expected {consumer} string-key helper selection record:\n{artifact:#}"
        );
    }
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_string",
            "js_set_add_string",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_string",
            "js_set_has_string",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_string",
            "js_set_delete_string",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "string_ref"
                    && record["llvm_ty"] == "i64"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.string_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=string_ref")
                    && record_has_note(record, "boxed_value_avoided_until_set_slot=true")
            }),
            "expected {consumer} string-value helper selection record:\n{artifact:#}"
        );
    }

    let fallback_module = module_with_classes_and_params(
        "artifact_set_non_string_key_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Number)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Any),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let artifact = compile_artifact_json_for_module(fallback_module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "SetHas"
                && record["consumer"] == "collection_string_key.set_has_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "set.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_set_has")
                && record_has_note(
                    record,
                    "typed_collection_rejected=receiver_or_value_not_static_string",
                )
        }),
        "expected set.has non-string rejection record:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "SetDelete"
                && record["consumer"] == "collection_string_key.set_delete_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "set.string_key_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_set_delete")
        }),
        "expected set.delete non-string rejection record:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_set_number_value_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_number_value_selection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Number)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Number),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_number_value.set_add",
            "js_set_add_number",
        ),
        (
            "SetHas",
            "collection_number_value.set_has",
            "js_set_has_number",
        ),
        (
            "SetDelete",
            "collection_number_value.set_delete",
            "js_set_delete_number",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "f64"
                    && record["llvm_ty"] == "double"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.number_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=raw_f64")
                    && record_has_note(record, "value_guard=js_typed_f64_arg_guard")
            }),
            "expected Set<number> helper selection record {consumer}:\n{artifact:#}"
        );
    }
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_number_value.set_add_generic",
            "js_set_add",
        ),
        (
            "SetHas",
            "collection_number_value.set_has_generic",
            "js_set_has",
        ),
        (
            "SetDelete",
            "collection_number_value.set_delete_generic",
            "js_set_delete",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "set.number_value_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(
                        record,
                        "typed_collection_rejected=runtime_value_guard_failed",
                    )
                    && record_has_note(record, "value_rep=js_value")
            }),
            "expected Set<number> guarded fallback record {consumer}:\n{artifact:#}"
        );
    }

    let rejected_module = module_with_classes_and_params(
        "artifact_set_number_value_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Any)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Number),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(Expr::SetHas {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            })),
        ],
    );
    let rejected_artifact = compile_artifact_json_for_module(rejected_module);
    let rejected_records = rejected_artifact["records"].as_array().unwrap();
    assert!(
        rejected_records.iter().any(|record| {
            record["expr_kind"] == "SetAdd"
                && record["consumer"] == "collection_number_value.set_add_generic"
                && record["native_rep_name"] == "js_value"
                && record_has_type_fact(
                    record,
                    "rejected_facts",
                    "set.number_value_helper",
                    "rejected",
                )
                && record_has_note(record, "generic_helper=js_set_add")
                && record_has_note(record, "typed_collection_rejected=value_expr_not_numeric")
        }),
        "expected unproven Set<number> value rejection record:\n{rejected_artifact:#}"
    );
}

#[test]
fn artifact_records_set_int32_value_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_int32_value_selection.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Int32),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Integer(7)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Integer(7)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Integer(7)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_i32",
            "js_set_add_i32",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_i32",
            "js_set_has_i32",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_i32",
            "js_set_delete_i32",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "i32"
                    && record["llvm_ty"] == "i32"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.int32_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=i32")
                    && record_has_note(record, "boxed_value_avoided_until_set_slot=true")
            }),
            "expected {consumer} int32-value helper selection record:\n{artifact:#}"
        );
    }

    let fallback_module = module_with_classes_and_params(
        "artifact_set_int32_value_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Int32)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Int32),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let fallback_artifact = compile_artifact_json_for_module(fallback_module);
    let fallback_records = fallback_artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_generic",
            "js_set_add",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_generic",
            "js_set_has",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_generic",
            "js_set_delete",
        ),
    ] {
        assert!(
            fallback_records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "set.int32_value_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(
                        record,
                        "typed_collection_rejected=value_expr_not_native_i32",
                    )
                    && record_has_note(record, "value_rep=js_value")
            }),
            "expected {consumer} int32-value helper rejection record:\n{fallback_artifact:#}"
        );
    }
}

#[test]
fn artifact_records_set_u32_value_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_u32_value_selection.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Integer(4_000_000_000)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Integer(4_000_000_000)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_u32",
            "js_set_add_u32",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_u32",
            "js_set_has_u32",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_u32",
            "js_set_delete_u32",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "u32"
                    && record["llvm_ty"] == "i32"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.uint32_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=u32")
                    && record_has_note(record, "boxed_value_avoided_until_set_slot=true")
            }),
            "expected {consumer} uint32-value helper selection record:\n{artifact:#}"
        );
    }

    let fallback_module = module_with_classes_and_params(
        "artifact_set_u32_value_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Named("PerryU32".to_string()))],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryU32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let fallback_artifact = compile_artifact_json_for_module(fallback_module);
    let fallback_records = fallback_artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_generic",
            "js_set_add",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_generic",
            "js_set_has",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_generic",
            "js_set_delete",
        ),
    ] {
        assert!(
            fallback_records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "set.uint32_value_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(
                        record,
                        "typed_collection_rejected=value_expr_not_native_u32",
                    )
                    && record_has_note(record, "value_rep=js_value")
            }),
            "expected {consumer} uint32-value helper rejection record:\n{fallback_artifact:#}"
        );
    }
}

#[test]
fn artifact_records_set_f32_value_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_f32_value_selection.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Number(1.5)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Number(1.5)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_f32",
            "js_set_add_f32",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_f32",
            "js_set_has_f32",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_f32",
            "js_set_delete_f32",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "f32"
                    && record["llvm_ty"] == "float"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.float32_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=f32")
                    && record_has_note(record, "boxed_value_avoided_until_set_slot=true")
            }),
            "expected {consumer} float32-value helper selection record:\n{artifact:#}"
        );
    }

    let fallback_module = module_with_classes_and_params(
        "artifact_set_f32_value_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Named("PerryF32".to_string()))],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Named("PerryF32".to_string())),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let fallback_artifact = compile_artifact_json_for_module(fallback_module);
    let fallback_records = fallback_artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_generic",
            "js_set_add",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_generic",
            "js_set_has",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_generic",
            "js_set_delete",
        ),
    ] {
        assert!(
            fallback_records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "set.float32_value_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(
                        record,
                        "typed_collection_rejected=value_expr_not_native_f32",
                    )
                    && record_has_note(record, "value_rep=js_value")
            }),
            "expected {consumer} float32-value helper rejection record:\n{fallback_artifact:#}"
        );
    }
}

#[test]
fn artifact_records_set_boolean_value_helper_selection_and_rejection() {
    let selected_module = module_with_classes_and_params(
        "artifact_set_boolean_value_selection.ts",
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Boolean),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Let {
                id: 2,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(Expr::Bool(true)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Return(Some(local(2))),
        ],
    );
    let artifact = compile_artifact_json_for_module(selected_module);
    let records = artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_bool",
            "js_set_add_bool",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_bool",
            "js_set_has_bool",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_bool",
            "js_set_delete_bool",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "i1"
                    && record["llvm_ty"] == "i1"
                    && record_has_type_fact(
                        record,
                        "consumed_facts",
                        "set.boolean_value_helper",
                        "consumed",
                    )
                    && record_has_note(record, &format!("selected_helper={helper}"))
                    && record_has_note(record, "value_rep=i1")
                    && record_has_note(record, "boxed_value_avoided_until_set_slot=true")
            }),
            "expected {consumer} boolean-value helper selection record:\n{artifact:#}"
        );
    }

    let fallback_module = module_with_classes_and_params(
        "artifact_set_boolean_value_rejection.ts",
        Vec::new(),
        vec![param(2, "value", Type::Boolean)],
        Type::Boolean,
        vec![
            Stmt::Let {
                id: 1,
                name: "s".to_string(),
                ty: set_type(Type::Boolean),
                mutable: true,
                init: Some(Expr::SetNew),
            },
            Stmt::Expr(Expr::SetAdd {
                set_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Let {
                id: 3,
                name: "present".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::SetHas {
                    set: Box::new(local(1)),
                    value: Box::new(local(2)),
                }),
            },
            Stmt::Expr(Expr::SetDelete {
                set: Box::new(local(1)),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(local(3))),
        ],
    );
    let fallback_artifact = compile_artifact_json_for_module(fallback_module);
    let fallback_records = fallback_artifact["records"].as_array().unwrap();
    for (expr_kind, consumer, helper) in [
        (
            "SetAdd",
            "collection_typed_value.set_add_generic",
            "js_set_add",
        ),
        (
            "SetHas",
            "collection_typed_value.set_has_generic",
            "js_set_has",
        ),
        (
            "SetDelete",
            "collection_typed_value.set_delete_generic",
            "js_set_delete",
        ),
    ] {
        assert!(
            fallback_records.iter().any(|record| {
                record["expr_kind"] == expr_kind
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == "js_value"
                    && record_has_type_fact(
                        record,
                        "rejected_facts",
                        "set.boolean_value_helper",
                        "rejected",
                    )
                    && record_has_note(record, &format!("generic_helper={helper}"))
                    && record_has_note(record, "typed_collection_rejected=value_expr_not_native_i1")
                    && record_has_note(record, "value_rep=js_value")
            }),
            "expected {consumer} boolean-value helper rejection record:\n{fallback_artifact:#}"
        );
    }
}

#[test]
fn packed_f64_loop_rejects_nonnumeric_store_then_later_read() {
    let module = module_with_classes_and_params(
        "packed_f64_nonnumeric_store_then_read.ts",
        Vec::new(),
        Vec::new(),
        Type::Number,
        vec![
            number_array_let(1, "values", vec![1, 2, 3]),
            for_loop(
                4,
                length(1),
                vec![
                    array_set(1, local(4), Expr::String("x".to_string())),
                    Stmt::Expr(index_get(1, local(4))),
                ],
            ),
            Stmt::Return(Some(int(0))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "nonnumeric store before a later read must not get a packed-f64 clone:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_f64_fast"),
        "nonnumeric store/read body must not be emitted under the packed-f64 fast clone:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_array_note_numeric_write"),
        "nonnumeric store into a numeric array must invalidate the raw-f64 layout:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
        "later numeric-array read should be guarded independently after the layout-changing store:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["expr_kind"].as_str(),
                Some("PackedF64LoopGuard" | "PackedF64LoopStore" | "PackedF64LoopLoad")
            )
        }),
        "nonnumeric store/read loop should not record packed-f64 loop facts:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexGet"
                && record["consumer"] == "js_array_numeric_get_f64_unboxed"
                && record["access_mode"] == "checked_native"
        }),
        "later read should use its own guarded numeric-array get, not a packed-loop raw load:\n{artifact:#}"
    );
}

#[test]
fn packed_f64_loop_rejects_store_then_read_invalidation_shape() {
    let module = module_with_classes_and_params(
        "packed_f64_store_fallback_then_read.ts",
        Vec::new(),
        vec![param(2, "value", Type::Number)],
        Type::Number,
        vec![
            number_array_let(1, "values", vec![1, 2, 3]),
            number_let(3, "sum", true, int(0)),
            for_loop(
                4,
                length(1),
                vec![
                    array_set(1, local(4), local(2)),
                    Stmt::Expr(Expr::LocalSet(
                        3,
                        Box::new(add(local(3), index_get(1, local(4)))),
                    )),
                ],
            ),
            Stmt::Return(Some(local(3))),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module.clone(), empty_opts()).unwrap();
    assert!(
        !ir.contains("call i32 @js_typed_feedback_packed_f64_array_loop_guard"),
        "store-then-read loops must not get a packed-f64 clone whose store fallback could invalidate later raw loads:\n{ir}"
    );
    assert!(
        !ir.contains("for.packed_f64_fast"),
        "unsafe store-then-read loop body must not be emitted under the packed-f64 fast clone:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_set_guard"),
        "test must exercise the guarded numeric array store path:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_typed_feedback_array_index_set_fallback_boxed"),
        "numeric store must retain the boxed fallback that invalidates raw-f64 layout:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_numeric_array_index_get_guard"),
        "later read should be guarded independently after the fallback-capable store:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            matches!(
                record["expr_kind"].as_str(),
                Some("PackedF64LoopGuard" | "PackedF64LoopStore" | "PackedF64LoopLoad")
            )
        }),
        "store-bearing loop should not record packed-f64 loop facts:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexSet"
                && record["consumer"] == "js_typed_feedback_array_index_set_fallback_boxed"
                && record["access_mode"] == "dynamic_fallback"
                && record_has_raw_f64_layout_fact(record, "rejected_facts", "invalidated")
        }),
        "numeric store fallback must invalidate raw-f64 layout:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "NumericArrayIndexGet"
                && record["consumer"] == "js_array_numeric_get_f64_unboxed"
                && record["access_mode"] == "checked_native"
        }),
        "later read should use its own guarded numeric-array get, not a packed-loop raw load:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_array_push_value_bits_before_slot_store() {
    let module = module_with_classes_and_params(
        "artifact_array_push_slot_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "value", Type::Any),
        ],
        Type::Number,
        vec![
            Stmt::Expr(Expr::ArrayPush {
                array_id: 1,
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ArrayPush"
                && record["consumer"] == "array_push.slot_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["access_mode"].is_null()
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str()
                            == Some("boxed_at=array_push_slot_or_runtime_helper_edge")
                    })
                })
        }),
        "expected array.push slot store to consume js_value_bits before boxing at the helper edge:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "lower_expr_native_js_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_abi_type"].is_null()
        }),
        "expected array.push value to be selected through the js_value_bits native lowering lane:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_dynamic_property_set_value_bits_before_helper() {
    let module = module_with_classes_and_params(
        "artifact_property_set_slot_js_value_bits.ts",
        Vec::new(),
        vec![param(1, "obj", Type::Any), param(2, "value", Type::Any)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(1)),
                property: "field".to_string(),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "PropertySet"
                && record["consumer"] == "property_set.dynamic_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record["access_mode"].is_null()
                && record_has_note(record, "boxed_at=dynamic_property_set_helper_edge")
        }),
        "expected dynamic property-set RHS to stay as js_value_bits before the helper edge:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_dynamic_index_set_value_bits_before_helper() {
    let module = module_with_classes_and_params(
        "artifact_index_set_slot_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "obj", Type::Any),
            param(2, "key", Type::Any),
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
            record["expr_kind"] == "IndexSet"
                && record["consumer"] == "index_set.dynamic_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record["access_mode"].is_null()
                && record_has_note(record, "boxed_at=polymorphic_index_set_helper_edge")
        }),
        "expected dynamic index-set RHS to stay as js_value_bits before the polymorphic helper edge:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_array_runtime_key_index_set_value_bits_before_helper() {
    let module = module_with_classes_and_params(
        "artifact_array_runtime_key_index_set_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "key", Type::Number),
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
            record["expr_kind"] == "IndexSet"
                && record["consumer"] == "index_set.array_runtime_key_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record_has_note(record, "boxed_at=array_runtime_key_set_helper_edge")
        }),
        "expected array runtime-key index-set RHS to stay as js_value_bits before the helper edge:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_direct_f64_to_js_value_bits_for_write_barrier() {
    let module = module_with_classes_and_params(
        "artifact_write_barrier_f64_to_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "key", Type::String),
            param(3, "value", Type::Number),
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
            record["consumer"] == "materialize_js_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["native_abi_transition"]["from_native_rep"] == "f64"
                && record["native_abi_transition"]["to_native_rep"] == "js_value_bits"
                && record["native_abi_transition"]["op"] == "none"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected direct f64 -> js_value_bits materialization for write barrier:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "write_barrier.child_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["native_value_state"] == "region_local"
        }),
        "expected write barrier to consume js_value_bits:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_direct_i1_to_js_value_bits_for_write_barrier() {
    let module = module_with_classes_and_params(
        "artifact_write_barrier_i1_to_js_value_bits.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "key", Type::String),
        ],
        Type::Number,
        vec![
            Stmt::Let {
                id: 3,
                name: "value".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::Bool(true)),
            },
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
            record["consumer"] == "materialize_js_value_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["native_abi_transition"]["from_native_rep"] == "i1"
                && record["native_abi_transition"]["to_native_rep"] == "js_value_bits"
                && record["native_abi_transition"]["op"] == "bool_to_js_value"
                && record["native_abi_transition"]["lossy"] == false
        }),
        "expected direct i1 -> js_value_bits materialization for write barrier:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "write_barrier.child_bits"
                && record["native_rep_name"] == "js_value_bits"
                && record["native_value_state"] == "region_local"
        }),
        "expected write barrier to consume js_value_bits:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_static_write_barrier_elision_for_primitive_array_store() {
    let module = module_with_classes_and_params(
        "artifact_write_barrier_elided_primitive.ts",
        Vec::new(),
        vec![
            param(1, "xs", Type::Array(Box::new(Type::Any))),
            param(2, "key", Type::String),
        ],
        Type::Number,
        vec![
            Stmt::Expr(Expr::IndexSet {
                object: Box::new(local(1)),
                index: Box::new(local(2)),
                value: Box::new(Expr::Bool(true)),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "WriteBarrierElided"
                && record["consumer"] == "write_barrier.elided_non_pointer_child"
                && record["native_rep_name"] == "js_value"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note.as_str() == Some("reason=statically_non_pointer_child"))
                })
        }),
        "expected static primitive write-barrier elision record:\n{artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "WriteBarrier"
                && record["consumer"] == "write_barrier.child_bits"
        }),
        "primitive child store should not emit a write-barrier child-bits record:\n{artifact:#}"
    );
    assert_eq!(
        artifact["summary"]["write_barrier_elided_count"]
            .as_u64()
            .unwrap_or(0),
        1,
        "expected write-barrier elision summary count:\n{artifact:#}"
    );
}

fn boxed_local_capture_module(name: &str) -> Module {
    module(
        name,
        vec![
            Stmt::Let {
                id: 10,
                name: "cell".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Let {
                id: 11,
                name: "writer".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 30,
                    params: Vec::new(),
                    return_type: Type::Any,
                    body: vec![
                        Stmt::Expr(Expr::LocalSet(10, Box::new(Expr::Array(Vec::new())))),
                        Stmt::Return(Some(local(10))),
                    ],
                    captures: vec![10],
                    mutable_captures: vec![10],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Return(Some(local(11))),
        ],
    )
}

fn boxed_local_storage_module(name: &str, init: Expr, replacement: Expr) -> Module {
    module(
        name,
        vec![
            Stmt::Let {
                id: 10,
                name: "cell".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(init),
            },
            Stmt::Let {
                id: 11,
                name: "writer".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 30,
                    params: Vec::new(),
                    return_type: Type::Any,
                    body: vec![
                        Stmt::Expr(Expr::LocalSet(10, Box::new(replacement))),
                        Stmt::Return(Some(local(10))),
                    ],
                    captures: vec![10],
                    mutable_captures: vec![10],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Return(Some(local(11))),
        ],
    )
}

fn boxed_param_capture_module(name: &str) -> Module {
    module_with_classes_and_params(
        name,
        Vec::new(),
        vec![
            param(20, "cell", Type::Any),
            Param {
                id: 22,
                name: "arguments".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: true,
                arguments_object: Some(ArgumentsObjectMeta {
                    strict: false,
                    simple_parameters: true,
                    mapped_parameter_ids: vec![(0, 20)],
                    restricted_callee: false,
                }),
            },
        ],
        Type::Any,
        vec![Stmt::Return(Some(local(22)))],
    )
}

#[test]
fn boxed_local_slot_uses_i64_js_value_bits_until_helper_edges() {
    let module = boxed_local_capture_module("boxed_local_js_value_bits_ir.ts");
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    let box_alloc = ir
        .find("call i64 @js_box_alloc_bits")
        .expect("fixture should allocate a mutable-capture box");
    let first_array_alloc = ir[box_alloc..]
        .find("call i64 @js_array_alloc")
        .map(|offset| box_alloc + offset)
        .expect("fixture should lower the initializer after storing the box pointer");
    let slot_init = &ir[box_alloc..first_array_alloc];

    assert!(
        slot_init.contains("store i64 "),
        "box pointer slot should be stored as i64 before helper edges:\n{slot_init}\n\n{ir}"
    );
    assert!(
        !slot_init.contains("store double "),
        "box pointer slot init must not materialize as a double store:\n{slot_init}\n\n{ir}"
    );
    assert!(
        !slot_init.contains("bitcast i64"),
        "box pointer slot init should not bitcast to double before storage:\n{slot_init}\n\n{ir}"
    );
    assert!(
        ir.contains(" = alloca i64"),
        "boxed local should allocate an i64 slot:\n{ir}"
    );
    assert!(
        ir.contains(" = load i64, ptr "),
        "boxed local reads should load the box pointer as i64:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_box_set_bits(i64 ")
            && ir.contains("call i64 @js_box_get_bits(i64 "),
        "runtime box helpers should use i64 JSValueBits payload edges:\n{ir}"
    );
    for old_helper in [
        "call i64 @js_box_alloc(double",
        "call void @js_box_set(i64 ",
        "call double @js_box_get(i64 ",
    ] {
        assert!(
            !ir.contains(old_helper),
            "boxed local storage should not use old f64 helper edge {old_helper}:\n{ir}"
        );
    }
    // Payloads crossing the box-bits ABI must be i64 JSValueBits, never a raw
    // `double`. A value coming from the `lower_expr` double ABI is bitcast to
    // bits before it reaches the helper; a value already in i64 form — a raw
    // pointer/handle, or a constant that lowering folds straight to bits (e.g.
    // the `undefined` slot default `0x7FFC000000000001`) — needs no bitcast.
    // Either way, no `double`-typed operand may reach a bits helper. (The
    // explicit `bitcast double ... to i64` this previously required is elided
    // when the source is already bits — main's constant/pointer lowering now
    // emits the slot default directly as i64, so we assert the invariant
    // instead of one particular instruction sequence.)
    let box_bits_payloads_are_i64 = ir
        .lines()
        .filter(|line| line.contains("@js_box_set_bits(") || line.contains("@js_box_alloc_bits("))
        .all(|line| !line.contains("double"));
    assert!(
        box_bits_payloads_are_i64,
        "box-bits ABI payloads must be i64 JSValueBits, never a raw double:\n{ir}"
    );
    assert!(
        ir.contains("bitcast i64 ") && ir.contains(" to double"),
        "boxed reads should bitcast JSValueBits back to the lower_expr double ABI:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_closure_get_capture_bits")
            && (ir.contains("call void @js_closure_set_capture_bits")
                || ir.contains("call i64 @js_closure_alloc_with_captures_singleton")),
        "generated boxed capture traffic should use exact i64 closure capture slots:\n{ir}"
    );
    for old_helper in [
        "call void @js_closure_set_capture_f64",
        "call double @js_closure_get_capture_f64",
    ] {
        assert!(
            !ir.contains(old_helper),
            "generated boxed capture traffic should not use old f64 helper edge {old_helper}:\n{ir}"
        );
    }
}

#[test]
fn boxed_param_slot_uses_i64_js_value_bits_until_helper_edges() {
    let module = boxed_param_capture_module("boxed_param_js_value_bits_ir.ts");
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    let box_alloc = ir
        .find("call i64 @js_box_alloc_bits(i64 ")
        .expect("fixture should allocate a mutable-capture param box");
    let store_i64 = ir[box_alloc..]
        .find("store i64 ")
        .map(|offset| box_alloc + offset)
        .expect("fixture should store the param box pointer as i64");
    let param_slot = &ir[box_alloc..store_i64];

    assert!(
        ir[..box_alloc].contains(" = alloca i64"),
        "boxed param should allocate an i64 slot before js_box_alloc_bits:\n{ir}"
    );
    assert!(
        !param_slot.contains("store double ") && !param_slot.contains("bitcast i64"),
        "boxed param slot setup must not materialize the box pointer as double:\n{param_slot}\n\n{ir}"
    );
    assert!(
        ir[..box_alloc].contains("bitcast double %arg20 to i64"),
        "boxed param should convert the incoming JSValue ABI double to bits before allocation:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_box_alloc(double"),
        "boxed param allocation should not use old f64 payload helper:\n{ir}"
    );
}

#[test]
fn boxed_jsvalue_storage_uses_bits_helpers_for_strings_objects_and_tags() {
    let short_string_expr = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::String("a".to_string())),
        right: Box::new(Expr::String("b".to_string())),
    };
    let cases = [
        (
            "heap_string",
            boxed_local_storage_module(
                "boxed_heap_string_bits.ts",
                Expr::String("captured".to_string()),
                Expr::String("replacement".to_string()),
            ),
            "js_string_from_bytes",
        ),
        (
            "short_string_candidate",
            boxed_local_storage_module(
                "boxed_short_string_bits.ts",
                short_string_expr.clone(),
                short_string_expr,
            ),
            "js_string_concat_box",
        ),
        (
            "object",
            boxed_local_storage_module(
                "boxed_object_bits.ts",
                Expr::Object(vec![(
                    "kind".to_string(),
                    Expr::String("object".to_string()),
                )]),
                Expr::Object(vec![("next".to_string(), Expr::Bool(true))]),
            ),
            "js_object_alloc",
        ),
        (
            "tagged_primitive",
            boxed_local_storage_module(
                "boxed_tagged_primitive_bits.ts",
                Expr::Null,
                Expr::Bool(true),
            ),
            "bitcast double 0x7FFC000000000002 to i64",
        ),
    ];

    for (label, module, marker) in cases {
        let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
        assert!(
            ir.contains(marker),
            "fixture {label} should exercise marker {marker}:\n{ir}"
        );
        for helper in [
            "call i64 @js_box_alloc_bits(i64 ",
            "call void @js_box_set_bits(i64 ",
            "call i64 @js_box_get_bits(i64 ",
        ] {
            assert!(
                ir.contains(helper),
                "boxed {label} storage should use bits helper {helper}:\n{ir}"
            );
        }
        for old_helper in [
            "call i64 @js_box_alloc(double",
            "call void @js_box_set(i64 ",
            "call double @js_box_get(i64 ",
        ] {
            assert!(
                !ir.contains(old_helper),
                "boxed {label} storage should not use old f64 helper edge {old_helper}:\n{ir}"
            );
        }
    }
}

#[test]
fn artifact_records_boxed_local_slot_as_js_value_bits() {
    let artifact = compile_artifact_json_for_module(boxed_local_capture_module(
        "artifact_boxed_local_bits.ts",
    ));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BoxedLocalSlot"
                && record["consumer"] == "boxed_let.box_ptr_slot"
                && record["local_id"] == 10
                && record["native_rep_name"] == "js_value_bits"
                && record["llvm_ty"] == "i64"
                && record["native_value_state"] == "region_local"
                && record["materialization_reason"].is_null()
                && record["native_abi_type"].is_null()
        }),
        "expected boxed local slot js_value_bits artifact record:\n{artifact:#}"
    );
    assert!(
        artifact["summary"]["js_value_bits_count"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "expected boxed local slot to contribute to js_value_bits summary:\n{artifact:#}"
    );
}

fn compiler_private_async_control_body() -> Vec<Stmt> {
    vec![
        Stmt::PreallocateBoxes(vec![10, 11, 12]),
        Stmt::Let {
            id: 10,
            name: "__gen_state".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(0.0)),
        },
        Stmt::Let {
            id: 11,
            name: "__gen_done".to_string(),
            ty: Type::Boolean,
            mutable: true,
            init: Some(Expr::Bool(false)),
        },
        Stmt::Let {
            id: 12,
            name: "__gen_executing".to_string(),
            ty: Type::Boolean,
            mutable: true,
            init: Some(Expr::Bool(false)),
        },
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(10)),
                right: Box::new(Expr::Number(0.0)),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(10, Box::new(Expr::Number(1.0)))),
                Stmt::Expr(Expr::LocalSet(11, Box::new(Expr::Bool(true)))),
            ],
            else_branch: None,
        },
        Stmt::If {
            condition: Expr::LocalGet(11),
            then_branch: vec![Stmt::Expr(Expr::LocalSet(12, Box::new(Expr::Bool(true))))],
            else_branch: None,
        },
        Stmt::Return(Some(Expr::Number(0.0))),
    ]
}

fn compiler_private_async_iter_result_f64_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(Box::new(Expr::Number(41.5)), false)),
        Stmt::Let {
            id: 20,
            name: "__step_value".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::IterResultGetValue),
        },
        Stmt::Return(Some(Expr::LocalGet(20))),
    ]
}

fn compiler_private_async_iter_result_i1_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(Box::new(Expr::Bool(true)), false)),
        Stmt::Let {
            id: 21,
            name: "__step_bool".to_string(),
            ty: Type::Boolean,
            mutable: false,
            init: Some(Expr::BooleanCoerce(Box::new(Expr::IterResultGetValue))),
        },
        Stmt::Return(Some(Expr::LocalGet(21))),
    ]
}

fn compiler_private_async_iter_result_i32_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(
            Box::new(Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(Expr::Integer(17)),
                right: Box::new(Expr::Integer(0)),
            }),
            false,
        )),
        Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(Expr::IterResultGetValue),
            right: Box::new(Expr::Integer(0)),
        })),
    ]
}

fn compiler_private_async_iter_result_generic_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(
            Box::new(Expr::String("generic".to_string())),
            false,
        )),
        Stmt::Return(Some(Expr::IterResultGetValue)),
    ]
}

fn compiler_private_async_iter_result_annotated_numeric_param_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(Box::new(Expr::LocalGet(30)), false)),
        Stmt::Return(Some(Expr::IterResultGetValue)),
    ]
}

fn compiler_private_async_iter_result_annotated_boolean_param_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(Box::new(Expr::LocalGet(31)), false)),
        Stmt::Return(Some(Expr::IterResultGetValue)),
    ]
}

fn compiler_private_async_iter_result_annotated_i32_param_body() -> Vec<Stmt> {
    vec![
        Stmt::Expr(Expr::IterResultSet(Box::new(Expr::LocalGet(32)), false)),
        Stmt::Return(Some(Expr::IterResultGetValue)),
    ]
}

#[test]
fn compiler_private_async_control_cells_use_primitive_heap_boxes() {
    let ir = compile_ir(
        "compiler_private_async_control_cells.ts",
        compiler_private_async_control_body(),
    );

    for symbol in [
        "call i64 @js_i32_box_alloc",
        "call i32 @js_i32_box_get",
        "call void @js_i32_box_set",
        "call i64 @js_bool_box_alloc",
        "call i32 @js_bool_box_get",
        "call void @js_bool_box_set",
    ] {
        assert!(
            ir.contains(symbol),
            "expected compiler-private control lowering to emit {symbol}:\n{ir}"
        );
    }
    assert!(
        ir.contains("icmp eq i32"),
        "__gen_state constant comparisons should stay as i32 compares:\n{ir}"
    );
    for generic_box_call in [
        "call i64 @js_box_alloc",
        "call double @js_box_get",
        "call void @js_box_set",
    ] {
        assert!(
            !ir.contains(generic_box_call),
            "compiler-private control cells must not use generic JSValue boxes ({generic_box_call}):\n{ir}"
        );
    }
}

#[test]
fn compiler_private_async_iter_result_f64_slot_uses_typed_handoff() {
    let ir = compile_ir(
        "compiler_private_async_iter_result_f64.ts",
        compiler_private_async_iter_result_f64_body(),
    );

    assert!(
        ir.contains("call double @js_iter_result_set_f64"),
        "numeric async iter-result payload should use the raw f64 setter:\n{ir}"
    );
    // The CONSUMER reads through the representation-agnostic getter, NOT the
    // raw `js_iter_result_get_value_f64`. The typed getter was previously
    // applied speculatively (via `lower_expr_value`) to every
    // `IterResultGetValue`, but that getter coerces a non-raw-f64 slot with
    // `js_number_coerce` — so any `await`/`for await` of a non-numeric value
    // (object/string/array, or the promise threaded by `AsyncStepChain`) was
    // turned into a number. The generic getter still reads the raw-f64 slot
    // correctly (the value is unchanged), so the numeric payload stays exact
    // while non-numeric awaits are no longer corrupted.
    assert!(
        ir.contains("call double @js_iter_result_get_value("),
        "async iter-result consumer should use the representation-agnostic getter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_get_value_f64"),
        "async iter-result consumer must not speculatively use the coercing f64 getter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set("),
        "numeric async iter-result payload should avoid the generic JSValue setter:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_i1_slot_uses_typed_handoff() {
    let ir = compile_ir(
        "compiler_private_async_iter_result_i1.ts",
        compiler_private_async_iter_result_i1_body(),
    );

    assert!(
        ir.contains("call double @js_iter_result_set_i1"),
        "proven boolean async iter-result payload should use the raw i1 setter:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_iter_result_get_value_i1"),
        "proven boolean async iter-result consumer should use the raw i1 getter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set("),
        "proven boolean async iter-result payload should avoid the generic JSValue setter:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_is_truthy"),
        "raw i1 async iter-result consumers should not re-enter generic truthiness in generated IR:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_i32_slot_uses_typed_handoff() {
    let ir = compile_ir(
        "compiler_private_async_iter_result_i32.ts",
        compiler_private_async_iter_result_i32_body(),
    );

    assert!(
        ir.contains("call double @js_iter_result_set_i32"),
        "proven Int32 async iter-result payload should use the raw i32 setter:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_iter_result_get_value_i32"),
        "Int32 async iter-result consumer should use the raw i32 getter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set("),
        "proven Int32 async iter-result payload should avoid the generic JSValue setter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set_f64"),
        "proven Int32 async iter-result payload should not widen through the raw f64 setter:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_annotated_numeric_payload_is_coerced_before_raw_slot() {
    let ir = compile_ir_for_module_with_opts(
        module_with_classes_and_params(
            "compiler_private_async_iter_result_annotated_numeric_param.ts",
            Vec::new(),
            vec![param(30, "value", Type::Number)],
            Type::Number,
            compiler_private_async_iter_result_annotated_numeric_param_body(),
        ),
        empty_opts(),
    )
    .unwrap();

    assert!(
        ir.contains("call double @js_number_coerce"),
        "annotation-only numeric async payloads must be coerced before raw f64 storage:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_iter_result_set_f64"),
        "coerced numeric async payload should still use the raw f64 scratch slot:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set("),
        "coerced numeric async payload should avoid the generic JSValue setter:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_annotated_boolean_payload_stays_generic() {
    let ir = compile_ir_for_module_with_opts(
        module_with_classes_and_params(
            "compiler_private_async_iter_result_annotated_boolean_param.ts",
            Vec::new(),
            vec![param(31, "value", Type::Boolean)],
            Type::Boolean,
            compiler_private_async_iter_result_annotated_boolean_param_body(),
        ),
        empty_opts(),
    )
    .unwrap();

    assert!(
        ir.contains("call double @js_iter_result_set("),
        "annotation-only boolean async payloads must preserve the runtime JSValue:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set_i1"),
        "annotation-only boolean async payloads must not be narrowed to raw i1:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_annotated_i32_payload_stays_off_raw_i32_slot() {
    let ir = compile_ir_for_module_with_opts(
        module_with_classes_and_params(
            "compiler_private_async_iter_result_annotated_i32_param.ts",
            Vec::new(),
            vec![param(32, "value", Type::Int32)],
            Type::Int32,
            compiler_private_async_iter_result_annotated_i32_param_body(),
        ),
        empty_opts(),
    )
    .unwrap();

    assert!(
        !ir.contains("call double @js_iter_result_set_i32"),
        "annotation-only Int32 async payloads must not use the raw i32 slot without proof:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_iter_result_set_f64"),
        "annotation-only Int32 async payloads should keep the existing numeric-compatible raw f64 slot:\n{ir}"
    );
}

#[test]
fn compiler_private_async_iter_result_non_numeric_payload_stays_generic() {
    let ir = compile_ir(
        "compiler_private_async_iter_result_generic.ts",
        compiler_private_async_iter_result_generic_body(),
    );

    assert!(
        ir.contains("call double @js_iter_result_set("),
        "non-numeric async iter-result payload should use the generic JSValue setter:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_iter_result_set_f64"),
        "non-numeric async iter-result payload must not use the raw f64 setter:\n{ir}"
    );
}

#[test]
fn artifact_records_compiler_private_async_iter_result_f64_handoff() {
    let artifact = compile_artifact_json(
        "artifact_compiler_private_async_iter_result_f64.ts",
        compiler_private_async_iter_result_f64_body(),
    );
    let records = artifact["records"].as_array().unwrap();
    // Only the SETTER side records a raw-f64 handoff: a proven-numeric payload
    // is stored via `js_iter_result_set_f64`. The CONSUMER reads through the
    // representation-agnostic getter (no typed record) — the previously-recorded
    // `compiler_private_async_iter_result_get_f64` typed getter was unsound when
    // applied speculatively (it coerced non-raw-f64 slots, corrupting
    // `await`/`for await` of non-numeric values), so it is no longer emitted.
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "compiler_private_async_iter_result_set_f64"
                && record["native_rep_name"] == "f64"
                && record["llvm_ty"] == "double"
        }),
        "expected async iter-result f64 setter artifact record:\n{artifact:#}"
    );
    assert!(
        !records
            .iter()
            .any(|record| { record["consumer"] == "compiler_private_async_iter_result_get_f64" }),
        "async iter-result consumer must not record a speculative f64 getter:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_compiler_private_async_iter_result_i32_handoff() {
    let artifact = compile_artifact_json(
        "artifact_compiler_private_async_iter_result_i32.ts",
        compiler_private_async_iter_result_i32_body(),
    );
    let records = artifact["records"].as_array().unwrap();
    for consumer in [
        "compiler_private_async_iter_result_set_i32",
        "compiler_private_async_iter_result_get_i32",
    ] {
        assert!(
            records.iter().any(|record| {
                record["consumer"] == consumer
                    && record["native_rep_name"] == "i32"
                    && record["llvm_ty"] == "i32"
            }),
            "expected async iter-result i32 artifact record {consumer}:\n{artifact:#}"
        );
    }
}

#[test]
fn artifact_records_compiler_private_async_iter_result_i1_handoff() {
    let artifact = compile_artifact_json(
        "artifact_compiler_private_async_iter_result_i1.ts",
        compiler_private_async_iter_result_i1_body(),
    );
    let records = artifact["records"].as_array().unwrap();
    for consumer in [
        "compiler_private_async_iter_result_set_i1",
        "compiler_private_async_iter_result_get_i1",
    ] {
        assert!(
            records.iter().any(|record| {
                record["consumer"] == consumer
                    && record["native_rep_name"] == "i1"
                    && record["llvm_ty"] == "i1"
            }),
            "expected async iter-result i1 artifact record {consumer}:\n{artifact:#}"
        );
    }
}

#[test]
fn artifact_records_compiler_private_async_control_cells() {
    let artifact = compile_artifact_json(
        "artifact_compiler_private_async_control_cells.ts",
        compiler_private_async_control_body(),
    );
    let records = artifact["records"].as_array().unwrap();
    for (local_id, consumer, native_rep, llvm_ty) in [
        (10, "primitive_i32_control_cell", "js_value_bits", "i64"),
        (11, "primitive_i1_control_cell", "js_value_bits", "i64"),
        (12, "primitive_i1_control_cell", "js_value_bits", "i64"),
        (
            10,
            "compiler_private_async_control.local_set_i32",
            "i32",
            "i32",
        ),
        (11, "compiler_private_async_control.local_i1", "i1", "i1"),
        (
            12,
            "compiler_private_async_control.local_set_i1",
            "i1",
            "i1",
        ),
    ] {
        assert!(
            records.iter().any(|record| {
                record["local_id"] == local_id
                    && record["consumer"] == consumer
                    && record["native_rep_name"] == native_rep
                    && record["llvm_ty"] == llvm_ty
            }),
            "expected async control artifact record {consumer}/{native_rep}/{llvm_ty} for local {local_id}:\n{artifact:#}"
        );
    }
    assert!(
        records.iter().any(|record| {
            record["local_id"] == 10
                && record["consumer"] == "compiler_private_async_control.i32_compare"
                && record["native_rep_name"] == "i1"
                && record["llvm_ty"] == "i1"
        }),
        "expected __gen_state comparison artifact to stay native i1:\n{artifact:#}"
    );
}

fn typed_f64_clone_test_module(use_any_param: bool) -> Module {
    let add_param_ty = if use_any_param {
        Type::Any
    } else {
        Type::Number
    };
    Module {
        name: "typed_f64_function_abi.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "add".to_string(),
                type_params: Vec::new(),
                params: vec![
                    param(1, "a", add_param_ty.clone()),
                    param(2, "b", Type::Number),
                ],
                return_type: Type::Number,
                body: vec![
                    Stmt::Let {
                        id: 5,
                        name: "denom".to_string(),
                        ty: Type::Number,
                        mutable: false,
                        init: Some(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(local(2)),
                            right: Box::new(number(0.5)),
                        }),
                    },
                    Stmt::Return(Some(Expr::Binary {
                        op: BinaryOp::Div,
                        left: Box::new(local(1)),
                        right: Box::new(local(5)),
                    })),
                ],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(3, "x", Type::Number), param(4, "y", Type::Number)],
                return_type: Type::Number,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(3), local(4)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn typed_f64_i64_specialized_collision_module() -> Module {
    let mut module = typed_f64_clone_test_module(false);
    module.functions[0].body = vec![Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(local(1)),
        right: Box::new(local(2)),
    }))];
    module
}

fn typed_f64_rejected_signature_module(case: &str) -> Module {
    let mut module = typed_f64_clone_test_module(false);
    match case {
        "any" => module.functions[0].params[0].ty = Type::Any,
        "mixed" => module.functions[0].params[1].ty = Type::Boolean,
        other => panic!("unknown typed-f64 negative signature fixture: {other}"),
    }
    module
}

fn typed_f64_mixed_clone_test_module() -> Module {
    let mut module = typed_f64_clone_test_module(false);
    module.name = "typed_f64_mixed_function_abi.ts".to_string();
    module.functions[0].params = vec![
        param(1, "a", Type::Number),
        param(2, "b", Type::Int32),
        param(6, "flag", Type::Boolean),
    ];
    module.functions[0].body = vec![Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(local(1)),
        right: Box::new(local(2)),
    }))];
    module.functions[1].params = vec![
        param(3, "x", Type::Number),
        param(4, "y", Type::Int32),
        param(7, "flag", Type::Boolean),
    ];
    module.functions[1].body = vec![Stmt::Return(Some(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![local(3), local(4), local(7)],
        type_args: Vec::new(),
        byte_offset: 0,
    }))];
    module
}

fn typed_f64_i32_local_clone_test_module() -> Module {
    let mut module = typed_f64_clone_test_module(false);
    module.name = "typed_f64_i32_local_function_abi.ts".to_string();
    module.functions[0].params = vec![param(1, "a", Type::Number), param(2, "b", Type::Int32)];
    module.functions[0].body = vec![
        Stmt::Let {
            id: 5,
            name: "mask".to_string(),
            ty: Type::Int32,
            mutable: false,
            init: Some(Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(local(2)),
                right: Box::new(int(1)),
            }),
        },
        Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(local(1)),
            right: Box::new(local(5)),
        })),
    ];
    module.functions[1].params = vec![param(3, "x", Type::Number), param(4, "y", Type::Int32)];
    module.functions[1].body = vec![Stmt::Return(Some(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![local(3), local(4)],
        type_args: Vec::new(),
        byte_offset: 0,
    }))];
    module
}

fn typed_i1_clone_test_module() -> Module {
    typed_i1_clone_test_module_named("typed_i1_function_abi.ts")
}

fn typed_i1_clone_test_module_named(name: &str) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "both".to_string(),
                type_params: Vec::new(),
                params: vec![param(1, "a", Type::Boolean), param(2, "b", Type::Boolean)],
                return_type: Type::Boolean,
                body: vec![
                    Stmt::Let {
                        id: 5,
                        name: "not_b".to_string(),
                        ty: Type::Boolean,
                        mutable: false,
                        init: Some(Expr::Unary {
                            op: perry_hir::UnaryOp::Not,
                            operand: Box::new(local(2)),
                        }),
                    },
                    Stmt::Return(Some(Expr::Logical {
                        op: LogicalOp::And,
                        left: Box::new(local(1)),
                        right: Box::new(local(5)),
                    })),
                ],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(3, "x", Type::Boolean), param(4, "y", Type::Boolean)],
                return_type: Type::Boolean,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(3), local(4)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn typed_i1_rejected_signature_module(case: &str) -> Module {
    let mut module = typed_i1_clone_test_module();
    match case {
        "any" => module.functions[0].params[0].ty = Type::Any,
        "mixed" => module.functions[0].params[1].ty = Type::Number,
        other => panic!("unknown typed-i1 negative signature fixture: {other}"),
    }
    module
}

fn typed_string_clone_test_module(case: &str) -> Module {
    let mut module = Module {
        name: "typed_string_function_abi.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "id".to_string(),
                type_params: Vec::new(),
                params: vec![param(1, "s", Type::String)],
                return_type: Type::String,
                body: vec![
                    Stmt::Let {
                        id: 5,
                        name: "copy".to_string(),
                        ty: Type::String,
                        mutable: false,
                        init: Some(local(1)),
                    },
                    Stmt::Return(Some(local(5))),
                ],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(2, "x", Type::String)],
                return_type: Type::String,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(2)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    };
    match case {
        "positive" => {}
        "any_param" => module.functions[0].params[0].ty = Type::Any,
        "number_param" => module.functions[0].params[0].ty = Type::Number,
        "default_param" => {
            module.functions[0].params[0].default = Some(Expr::String("fallback".to_string()))
        }
        "rest_param" => module.functions[0].params[0].is_rest = true,
        "concat_body" => {
            module.functions[0].body = vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(1)),
                right: Box::new(local(1)),
            }))];
        }
        other => panic!("unknown typed-string fixture case: {other}"),
    }
    module
}

fn typed_i1_mixed_callsite_module() -> Module {
    let mut module = typed_i1_clone_test_module();
    module.functions[1].params[0].ty = Type::Any;
    module
}

fn typed_i1_numeric_predicate_module() -> Module {
    Module {
        name: "typed_i1_numeric_predicate.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "above".to_string(),
                type_params: Vec::new(),
                params: vec![param(1, "a", Type::Number), param(2, "b", Type::Number)],
                return_type: Type::Boolean,
                body: vec![
                    Stmt::Let {
                        id: 5,
                        name: "delta".to_string(),
                        ty: Type::Number,
                        mutable: false,
                        init: Some(Expr::Binary {
                            op: BinaryOp::Sub,
                            left: Box::new(local(1)),
                            right: Box::new(local(2)),
                        }),
                    },
                    Stmt::Return(Some(Expr::Compare {
                        op: CompareOp::Gt,
                        left: Box::new(local(5)),
                        right: Box::new(number(0.0)),
                    })),
                ],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(3, "x", Type::Number), param(4, "y", Type::Number)],
                return_type: Type::Boolean,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(3), local(4)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn typed_i1_i32_predicate_module() -> Module {
    Module {
        name: "typed_i1_i32_predicate.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "above_i32".to_string(),
                type_params: Vec::new(),
                params: vec![param(1, "a", Type::Int32), param(2, "b", Type::Int32)],
                return_type: Type::Boolean,
                body: vec![Stmt::Return(Some(Expr::Compare {
                    op: CompareOp::Gt,
                    left: Box::new(local(1)),
                    right: Box::new(local(2)),
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(3, "x", Type::Int32), param(4, "y", Type::Int32)],
                return_type: Type::Boolean,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(3), local(4)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn typed_i32_return_module(case: &str) -> Module {
    let (params, return_type, body) = match case {
        "positive" => (
            vec![param(1, "a", Type::Int32), param(2, "b", Type::Int32)],
            Type::Int32,
            vec![
                Stmt::Let {
                    id: 5,
                    name: "mixed".to_string(),
                    ty: Type::Int32,
                    mutable: false,
                    init: Some(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left: Box::new(local(1)),
                        right: Box::new(local(2)),
                    }),
                },
                Stmt::Return(Some(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(local(5)),
                    right: Box::new(Expr::Integer(7)),
                })),
            ],
        ),
        "number_param" => (
            vec![param(1, "a", Type::Number), param(2, "b", Type::Int32)],
            Type::Int32,
            vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(local(1)),
                right: Box::new(local(2)),
            }))],
        ),
        "number_return" => (
            vec![param(1, "a", Type::Int32), param(2, "b", Type::Int32)],
            Type::Number,
            vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(local(1)),
                right: Box::new(local(2)),
            }))],
        ),
        "unsafe_add" => (
            vec![param(1, "a", Type::Int32), param(2, "b", Type::Int32)],
            Type::Int32,
            vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(1)),
                right: Box::new(local(2)),
            }))],
        ),
        other => panic!("unknown typed-i32 return fixture: {other}"),
    };

    Module {
        name: format!("typed_i32_return_{case}.ts"),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![
            Function {
                id: 1,
                name: "mix_i32".to_string(),
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
            },
            Function {
                id: 2,
                name: "caller".to_string(),
                type_params: Vec::new(),
                params: vec![param(3, "x", Type::Int32), param(4, "y", Type::Int32)],
                return_type: Type::Int32,
                body: vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::FuncRef(1)),
                    args: vec![local(3), local(4)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            },
        ],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn typed_f64_method_clone_module() -> Module {
    let mut calc = class(201, "Calc", Vec::new());
    calc.methods.push(Function {
        id: 200,
        name: "mix".to_string(),
        type_params: Vec::new(),
        params: vec![param(21, "a", Type::Number), param(22, "b", Type::Number)],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 25,
                name: "denom".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(local(22)),
                    right: Box::new(number(0.5)),
                }),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Div,
                left: Box::new(local(21)),
                right: Box::new(local(25)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        "typed_f64_method_abi.ts",
        vec![calc],
        vec![
            param(1, "receiver", Type::Named("Calc".to_string())),
            param(2, "x", Type::Number),
            param(3, "y", Type::Number),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "mix".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_f64_i32_local_method_clone_module() -> Module {
    let mut calc = class(203, "Calc", Vec::new());
    calc.methods.push(Function {
        id: 204,
        name: "mix".to_string(),
        type_params: Vec::new(),
        params: vec![param(21, "a", Type::Number), param(22, "b", Type::Int32)],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 25,
                name: "mask".to_string(),
                ty: Type::Int32,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(local(22)),
                    right: Box::new(int(1)),
                }),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(21)),
                right: Box::new(local(25)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        "typed_f64_i32_local_method_abi.ts",
        vec![calc],
        vec![
            param(1, "receiver", Type::Named("Calc".to_string())),
            param(2, "x", Type::Number),
            param(3, "y", Type::Int32),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "mix".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_f64_method_negative_module(case: &str) -> Module {
    let mut calc = class(202, "Calc", vec![class_field("x", Type::Number)]);
    let mut params = vec![param(21, "a", Type::Number), param(22, "b", Type::Number)];
    let mut body = vec![Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(local(21)),
        right: Box::new(local(22)),
    }))];
    match case {
        "this" => {
            body = vec![Stmt::Return(Some(Expr::This))];
        }
        "default" => {
            params[0].default = Some(number(1.0));
        }
        "rest" => {
            params[1].is_rest = true;
        }
        "any" => {
            params[0].ty = Type::Any;
        }
        other => panic!("unknown negative typed-f64 method fixture: {other}"),
    }
    calc.methods.push(Function {
        id: 201,
        name: "mix".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        &format!("typed_f64_method_reject_{case}.ts"),
        vec![calc],
        vec![
            param(1, "receiver", Type::Named("Calc".to_string())),
            param(2, "x", Type::Number),
            param(3, "y", Type::Number),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "mix".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn this_field(name: &str) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::This),
        property: name.to_string(),
    }
}

fn typed_f64_receiver_method_function(id: u32, body: Vec<Stmt>) -> Function {
    Function {
        id,
        name: "score".to_string(),
        type_params: Vec::new(),
        params: vec![param(21, "scale", Type::Number)],
        return_type: Type::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn typed_f64_receiver_method_positive_module() -> Module {
    let mut point = class(
        211,
        "Point",
        vec![
            class_field("x", Type::Number),
            class_field("y", Type::Number),
        ],
    );
    point.methods.push(typed_f64_receiver_method_function(
        2110,
        vec![
            Stmt::Let {
                id: 25,
                name: "sum".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(this_field("x")),
                    right: Box::new(this_field("y")),
                }),
            },
            Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(local(25)),
                right: Box::new(local(21)),
            })),
        ],
    ));

    module_with_classes_and_params(
        "typed_f64_receiver_method.ts",
        vec![point],
        vec![
            param(1, "receiver", Type::Named("Point".to_string())),
            param(2, "scale", Type::Number),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "score".to_string(),
            }),
            args: vec![local(2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_f64_receiver_method_negative_module(case: &str) -> Module {
    let mut point = class(
        212,
        "Point",
        vec![class_field(
            "x",
            if case == "non_numeric_field" {
                Type::String
            } else {
                Type::Number
            },
        )],
    );
    let mut receiver_ty = Type::Named("Point".to_string());
    let mut method_body = vec![Stmt::Return(Some(this_field("x")))];

    match case {
        "this_escape" => {
            method_body = vec![Stmt::Return(Some(Expr::This))];
        }
        "field_mutation" => {
            method_body = vec![
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: "x".to_string(),
                    value: Box::new(number(1.0)),
                }),
                Stmt::Return(Some(this_field("x"))),
            ];
        }
        "nested_call" => {
            method_body = vec![Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::This),
                    property: "other".to_string(),
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }))];
        }
        "computed_member" => {
            point = class_with_computed_member(212, "Point", vec![class_field("x", Type::Number)]);
        }
        "accessor" => {
            point.getters.push((
                "x".to_string(),
                Function {
                    id: 2121,
                    name: "__get_x".to_string(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: Type::Number,
                    body: vec![Stmt::Return(Some(number(1.0)))],
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                    was_plain_async: false,
                    was_unrolled: false,
                },
            ));
        }
        "dynamic_receiver" => {
            receiver_ty = Type::Any;
        }
        "inherited_receiver" => {
            let mut base = class(212, "BasePoint", vec![class_field("x", Type::Number)]);
            base.methods
                .push(typed_f64_receiver_method_function(2120, method_body));
            let mut child = class(213, "Point", Vec::new());
            child.extends_name = Some("BasePoint".to_string());
            return module_with_classes_and_params(
                "typed_f64_receiver_method_reject_inherited_receiver.ts",
                vec![base, child],
                vec![
                    param(1, "receiver", Type::Named("Point".to_string())),
                    param(2, "scale", Type::Number),
                ],
                Type::Number,
                vec![Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(local(1)),
                        property: "score".to_string(),
                    }),
                    args: vec![local(2)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }))],
            );
        }
        "non_numeric_field" => {}
        other => panic!("unknown negative typed-f64 receiver method fixture: {other}"),
    }

    point
        .methods
        .push(typed_f64_receiver_method_function(2120, method_body));
    module_with_classes_and_params(
        &format!("typed_f64_receiver_method_reject_{case}.ts"),
        vec![point],
        vec![
            param(1, "receiver", receiver_ty),
            param(2, "scale", Type::Number),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "score".to_string(),
            }),
            args: vec![local(2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_f64_closure_clone_module(case: &str) -> Module {
    let mut params = vec![param(31, "a", Type::Number), param(32, "b", Type::Number)];
    let mut prefix = Vec::new();
    let mut captures = Vec::new();
    let mut mutable_captures = Vec::new();
    let mut body_expr = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(local(31)),
        right: Box::new(local(32)),
    };
    match case {
        "eligible" => {}
        "any" => {
            params[0].ty = Type::Any;
        }
        "capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "scale".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(number(1.5)),
            });
            captures.push(30);
            body_expr = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(body_expr),
                right: Box::new(local(30)),
            };
        }
        "mutable_capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "scale".to_string(),
                ty: Type::Number,
                mutable: true,
                init: Some(number(1.5)),
            });
            captures.push(30);
            mutable_captures.push(30);
            body_expr = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(body_expr),
                right: Box::new(local(30)),
            };
        }
        other => panic!("unknown typed-f64 closure fixture: {other}"),
    }

    let mut body = prefix;
    body.extend([
        Stmt::Let {
            id: 10,
            name: "adder".to_string(),
            ty: Type::Function(perry_types::FunctionType {
                params: vec![
                    ("a".to_string(), Type::Number, false),
                    ("b".to_string(), Type::Number, false),
                ],
                return_type: Box::new(Type::Number),
                is_async: false,
                is_generator: false,
            }),
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 300,
                params,
                return_type: Type::Number,
                body: vec![
                    Stmt::Let {
                        id: 33,
                        name: "sum".to_string(),
                        ty: Type::Number,
                        mutable: false,
                        init: Some(body_expr),
                    },
                    Stmt::Return(Some(Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(local(33)),
                        right: Box::new(number(2.0)),
                    })),
                ],
                captures,
                mutable_captures,
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(local(10)),
            args: vec![number(2.0), number(3.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ]);

    module("typed_f64_closure_abi.ts", body)
}

fn typed_i32_closure_clone_module(case: &str) -> Module {
    let mut params = vec![param(31, "a", Type::Int32), param(32, "b", Type::Int32)];
    let mut prefix = Vec::new();
    let mut captures = Vec::new();
    let mut mutable_captures = Vec::new();
    let mut local_ty = Type::Function(perry_types::FunctionType {
        params: vec![
            ("a".to_string(), Type::Int32, false),
            ("b".to_string(), Type::Int32, false),
        ],
        return_type: Box::new(Type::Int32),
        is_async: false,
        is_generator: false,
    });
    let mut return_type = Type::Int32;
    let mut first_let_ty = Type::Int32;
    let mut body_expr = Expr::Binary {
        op: BinaryOp::BitXor,
        left: Box::new(local(31)),
        right: Box::new(local(32)),
    };
    let mut return_expr = Expr::Binary {
        op: BinaryOp::BitOr,
        left: Box::new(local(33)),
        right: Box::new(int(7)),
    };
    match case {
        "eligible" => {}
        "capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "mask".to_string(),
                ty: Type::Int32,
                mutable: false,
                init: Some(int(3)),
            });
            captures.push(30);
            return_expr = Expr::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(return_expr),
                right: Box::new(local(30)),
            };
        }
        "number_param" => {
            params[0].ty = Type::Number;
            local_ty = Type::Function(perry_types::FunctionType {
                params: vec![
                    ("a".to_string(), Type::Number, false),
                    ("b".to_string(), Type::Int32, false),
                ],
                return_type: Box::new(Type::Int32),
                is_async: false,
                is_generator: false,
            });
        }
        "number_return" => {
            return_type = Type::Number;
            local_ty = Type::Function(perry_types::FunctionType {
                params: vec![
                    ("a".to_string(), Type::Int32, false),
                    ("b".to_string(), Type::Int32, false),
                ],
                return_type: Box::new(Type::Number),
                is_async: false,
                is_generator: false,
            });
        }
        "unsafe_add" => {
            first_let_ty = Type::Int32;
            body_expr = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(31)),
                right: Box::new(local(32)),
            };
        }
        "mutable_capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "mask".to_string(),
                ty: Type::Int32,
                mutable: true,
                init: Some(int(3)),
            });
            captures.push(30);
            mutable_captures.push(30);
            return_expr = Expr::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(return_expr),
                right: Box::new(local(30)),
            };
        }
        "dynamic" => {
            local_ty = Type::Any;
        }
        other => panic!("unknown typed-i32 closure fixture: {other}"),
    }

    let mut body = prefix;
    body.extend([
        Stmt::Let {
            id: 10,
            name: "mix_i32".to_string(),
            ty: local_ty,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 303,
                params,
                return_type: return_type.clone(),
                body: vec![
                    Stmt::Let {
                        id: 33,
                        name: "mixed".to_string(),
                        ty: first_let_ty,
                        mutable: false,
                        init: Some(body_expr),
                    },
                    Stmt::Return(Some(return_expr)),
                ],
                captures,
                mutable_captures,
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(local(10)),
            args: vec![int(11), int(5)],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ]);

    module_with_classes_and_params(
        &format!("typed_i32_closure_{case}.ts"),
        Vec::new(),
        Vec::new(),
        return_type,
        body,
    )
}

fn typed_i1_method_clone_module(case: &str) -> Module {
    let mut switch = class(203, "Switch", Vec::new());
    let mut params = vec![param(21, "a", Type::Boolean), param(22, "b", Type::Boolean)];
    let mut receiver_ty = Type::Named("Switch".to_string());
    match case {
        "eligible" => {}
        "any" => {
            params[0].ty = Type::Any;
        }
        "mixed" => {
            params[1].ty = Type::Number;
        }
        "dynamic" => {
            receiver_ty = Type::Any;
        }
        other => panic!("unknown typed-i1 method fixture: {other}"),
    }
    switch.methods.push(Function {
        id: 210,
        name: "check".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Boolean,
        body: vec![
            Stmt::Let {
                id: 25,
                name: "not_b".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::Unary {
                    op: perry_hir::UnaryOp::Not,
                    operand: Box::new(local(22)),
                }),
            },
            Stmt::Return(Some(Expr::Logical {
                op: LogicalOp::Or,
                left: Box::new(local(21)),
                right: Box::new(local(25)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        &format!("typed_i1_method_{case}.ts"),
        vec![switch],
        vec![
            param(1, "receiver", receiver_ty),
            param(2, "x", Type::Boolean),
            param(3, "y", Type::Boolean),
        ],
        Type::Boolean,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "check".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_i32_method_clone_module(case: &str) -> Module {
    let mut bits = class(205, "Bits", Vec::new());
    let mut params = vec![param(21, "a", Type::Int32), param(22, "b", Type::Int32)];
    let mut return_type = Type::Int32;
    let mut first_let_ty = Type::Int32;
    let mut first_expr = Expr::Binary {
        op: BinaryOp::BitXor,
        left: Box::new(local(21)),
        right: Box::new(local(22)),
    };
    let mut return_expr = Expr::Binary {
        op: BinaryOp::BitOr,
        left: Box::new(local(25)),
        right: Box::new(int(7)),
    };
    match case {
        "eligible" => {}
        "number_param" => {
            params[0].ty = Type::Number;
        }
        "number_return" => {
            return_type = Type::Number;
            first_let_ty = Type::Number;
            first_expr = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(21)),
                right: Box::new(local(22)),
            };
            return_expr = local(25);
        }
        "unsafe_add" => {
            first_expr = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(21)),
                right: Box::new(local(22)),
            };
        }
        other => panic!("unknown typed-i32 method fixture: {other}"),
    }
    bits.methods.push(Function {
        id: 230,
        name: "mix_i32".to_string(),
        type_params: Vec::new(),
        params,
        return_type,
        body: vec![
            Stmt::Let {
                id: 25,
                name: "mixed".to_string(),
                ty: first_let_ty,
                mutable: false,
                init: Some(first_expr),
            },
            Stmt::Return(Some(return_expr)),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    let (arg2_ty, arg3_ty) = if case == "number_param" || case == "number_return" {
        (Type::Number, Type::Int32)
    } else {
        (Type::Int32, Type::Int32)
    };
    module_with_classes_and_params(
        &format!("typed_i32_method_{case}.ts"),
        vec![bits],
        vec![
            param(1, "receiver", Type::Named("Bits".to_string())),
            param(2, "x", arg2_ty),
            param(3, "y", arg3_ty),
        ],
        if case == "number_return" {
            Type::Number
        } else {
            Type::Int32
        },
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "mix_i32".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_string_method_clone_module(case: &str) -> Module {
    let mut labeler = class(206, "Labeler", Vec::new());
    let mut params = vec![param(21, "s", Type::String)];
    let mut return_type = Type::String;
    let mut body = vec![
        Stmt::Let {
            id: 25,
            name: "copy".to_string(),
            ty: Type::String,
            mutable: false,
            init: Some(local(21)),
        },
        Stmt::Return(Some(local(25))),
    ];
    let mut receiver_ty = Type::Named("Labeler".to_string());
    match case {
        "eligible" => {}
        "any_param" => {
            params[0].ty = Type::Any;
        }
        "number_param" => {
            params[0].ty = Type::Number;
            return_type = Type::Number;
            body = vec![Stmt::Return(Some(number(1.0)))];
        }
        "default_param" => {
            params[0].default = Some(Expr::String("fallback".to_string()));
        }
        "rest_param" => {
            params[0].is_rest = true;
        }
        "concat_body" => {
            body = vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(21)),
                right: Box::new(local(21)),
            }))];
        }
        "dynamic_receiver" => {
            receiver_ty = Type::Any;
        }
        other => panic!("unknown typed-string method fixture: {other}"),
    }
    labeler.methods.push(Function {
        id: 240,
        name: "pick".to_string(),
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
    });

    module_with_classes_and_params(
        &format!("typed_string_method_{case}.ts"),
        vec![labeler],
        vec![
            param(1, "receiver", receiver_ty),
            param(2, "x", Type::String),
        ],
        Type::String,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "pick".to_string(),
            }),
            args: vec![local(2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_i1_numeric_predicate_method_module() -> Module {
    let mut meter = class(204, "Meter", Vec::new());
    meter.methods.push(Function {
        id: 220,
        name: "above".to_string(),
        type_params: Vec::new(),
        params: vec![param(21, "a", Type::Number), param(22, "b", Type::Number)],
        return_type: Type::Boolean,
        body: vec![
            Stmt::Let {
                id: 25,
                name: "delta".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(local(21)),
                    right: Box::new(local(22)),
                }),
            },
            Stmt::Return(Some(Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(local(25)),
                right: Box::new(number(0.0)),
            })),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        "typed_i1_numeric_method.ts",
        vec![meter],
        vec![
            param(1, "receiver", Type::Named("Meter".to_string())),
            param(2, "x", Type::Number),
            param(3, "y", Type::Number),
        ],
        Type::Boolean,
        vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "above".to_string(),
            }),
            args: vec![local(2), local(3)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
    )
}

fn typed_i1_closure_clone_module(case: &str) -> Module {
    let mut params = vec![param(31, "a", Type::Boolean), param(32, "b", Type::Boolean)];
    let mut prefix = Vec::new();
    let mut captures = Vec::new();
    let mut mutable_captures = Vec::new();
    let mut call_args = vec![Expr::Bool(true), Expr::Bool(false)];
    let mut local_ty = Type::Function(perry_types::FunctionType {
        params: vec![
            ("a".to_string(), Type::Boolean, false),
            ("b".to_string(), Type::Boolean, false),
        ],
        return_type: Box::new(Type::Boolean),
        is_async: false,
        is_generator: false,
    });
    let mut body_expr = Expr::Logical {
        op: LogicalOp::And,
        left: Box::new(local(31)),
        right: Box::new(Expr::Unary {
            op: perry_hir::UnaryOp::Not,
            operand: Box::new(local(32)),
        }),
    };
    match case {
        "eligible" => {}
        "any" => {
            params[0].ty = Type::Any;
        }
        "mixed" => {
            params[1].ty = Type::Number;
        }
        "numeric_predicate" => {
            params = vec![param(31, "a", Type::Number), param(32, "b", Type::Number)];
            local_ty = Type::Function(perry_types::FunctionType {
                params: vec![
                    ("a".to_string(), Type::Number, false),
                    ("b".to_string(), Type::Number, false),
                ],
                return_type: Box::new(Type::Boolean),
                is_async: false,
                is_generator: false,
            });
            body_expr = Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(local(31)),
                    right: Box::new(local(32)),
                }),
                right: Box::new(number(0.0)),
            };
            call_args = vec![number(7.0), number(3.0)];
        }
        "capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "enabled".to_string(),
                ty: Type::Boolean,
                mutable: false,
                init: Some(Expr::Bool(true)),
            });
            captures.push(30);
            body_expr = Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(body_expr),
                right: Box::new(local(30)),
            };
        }
        "mutable_capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "enabled".to_string(),
                ty: Type::Boolean,
                mutable: true,
                init: Some(Expr::Bool(true)),
            });
            captures.push(30);
            mutable_captures.push(30);
            body_expr = Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(body_expr),
                right: Box::new(local(30)),
            };
        }
        "dynamic" => {
            local_ty = Type::Any;
        }
        other => panic!("unknown typed-i1 closure fixture: {other}"),
    }

    let mut body = prefix;
    body.extend([
        Stmt::Let {
            id: 10,
            name: "pred".to_string(),
            ty: local_ty,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 301,
                params,
                return_type: Type::Boolean,
                body: vec![
                    Stmt::Let {
                        id: 33,
                        name: "pred_base".to_string(),
                        ty: Type::Boolean,
                        mutable: false,
                        init: Some(body_expr),
                    },
                    Stmt::Return(Some(local(33))),
                ],
                captures,
                mutable_captures,
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(local(10)),
            args: call_args,
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ]);

    module_with_classes_and_params(
        &format!("typed_i1_closure_{case}.ts"),
        Vec::new(),
        Vec::new(),
        Type::Boolean,
        body,
    )
}

fn typed_string_closure_clone_module(case: &str) -> Module {
    let mut params = vec![param(31, "s", Type::String)];
    let mut prefix = Vec::new();
    let mut captures = Vec::new();
    let mut mutable_captures = Vec::new();
    let mut local_ty = Type::Function(perry_types::FunctionType {
        params: vec![("s".to_string(), Type::String, false)],
        return_type: Box::new(Type::String),
        is_async: false,
        is_generator: false,
    });
    let mut body_expr = local(31);
    match case {
        "eligible" => {}
        "any" => {
            params[0].ty = Type::Any;
        }
        "capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "captured".to_string(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String("captured".to_string())),
            });
            captures.push(30);
            body_expr = local(30);
        }
        "mutable_capture" => {
            prefix.push(Stmt::Let {
                id: 30,
                name: "captured".to_string(),
                ty: Type::String,
                mutable: true,
                init: Some(Expr::String("captured".to_string())),
            });
            captures.push(30);
            mutable_captures.push(30);
            body_expr = local(30);
        }
        "dynamic" => {
            local_ty = Type::Any;
        }
        other => panic!("unknown typed-string closure fixture: {other}"),
    }

    let mut body = prefix;
    body.extend([
        Stmt::Let {
            id: 10,
            name: "id".to_string(),
            ty: local_ty,
            mutable: false,
            init: Some(Expr::Closure {
                func_id: 302,
                params,
                return_type: Type::String,
                body: vec![
                    Stmt::Let {
                        id: 33,
                        name: "copy".to_string(),
                        ty: Type::String,
                        mutable: false,
                        init: Some(body_expr),
                    },
                    Stmt::Return(Some(local(33))),
                ],
                captures,
                mutable_captures,
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(local(10)),
            args: vec![Expr::String("input".to_string())],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ]);

    module_with_classes_and_params(
        &format!("typed_string_closure_{case}.ts"),
        Vec::new(),
        Vec::new(),
        Type::String,
        body,
    )
}

fn scalar_method_summary_module() -> Module {
    let mut point = class(
        101,
        "Point",
        vec![
            class_field("x", Type::Number),
            class_field("y", Type::Number),
        ],
    );
    point.constructor = Some(Function {
        id: 100,
        name: "Point_constructor".to_string(),
        type_params: Vec::new(),
        params: vec![param(10, "x", Type::Number), param(11, "y", Type::Number)],
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "x".to_string(),
                value: Box::new(local(10)),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "y".to_string(),
                value: Box::new(local(11)),
            }),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    point.methods.push(Function {
        id: 101,
        name: "sum".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::This),
                property: "x".to_string(),
            }),
            right: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::This),
                property: "y".to_string(),
            }),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    module_with_classes_and_params(
        "scalar_method_summary.ts",
        vec![point],
        Vec::new(),
        Type::Number,
        vec![
            Stmt::Let {
                id: 20,
                name: "p".to_string(),
                ty: Type::Named("Point".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Point".to_string(),
                    args: vec![number(1.25), number(2.75)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(local(20)),
                    property: "sum".to_string(),
                }),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            })),
        ],
    )
}

fn scalar_method_shadowed_by_field_module() -> Module {
    let mut module = scalar_method_summary_module();
    module.name = "scalar_method_shadowed_by_field.ts".to_string();
    module.classes[0]
        .fields
        .push(class_field("sum", Type::Number));
    module
}

fn scalar_method_numeric_local_temp_module(case: &str, mutable_temp: bool) -> Module {
    let mut module = scalar_method_summary_module();
    module.name = format!("scalar_method_numeric_local_temp_{case}.ts");
    module.classes[0].methods.clear();
    module.classes[0].methods.push(Function {
        id: 103,
        name: "weighted".to_string(),
        type_params: Vec::new(),
        params: vec![param(12, "scale", Type::Number)],
        return_type: Type::Number,
        body: vec![
            Stmt::Let {
                id: 130,
                name: "shifted".to_string(),
                ty: Type::Number,
                mutable: mutable_temp,
                init: Some(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: "x".to_string(),
                    }),
                    right: Box::new(local(12)),
                }),
            },
            Stmt::Let {
                id: 131,
                name: "scaled".to_string(),
                ty: Type::Number,
                mutable: false,
                init: Some(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(local(130)),
                    right: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: "y".to_string(),
                    }),
                }),
            },
            Stmt::Return(Some(local(131))),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module.functions[0].body = vec![
        Stmt::Let {
            id: 20,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Point".to_string(),
                args: vec![number(1.25), number(2.75)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(20)),
                property: "weighted".to_string(),
            }),
            args: vec![number(3.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ];
    module
}

fn scalar_predicate_method_body(field: &str) -> Expr {
    Expr::Compare {
        op: CompareOp::Gt,
        left: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::This),
            property: field.to_string(),
        }),
        right: Box::new(local(12)),
    }
}

fn scalar_method_boolean_predicate_module() -> Module {
    let mut module = scalar_method_summary_module();
    module.name = "scalar_method_boolean_predicate.ts".to_string();
    module.functions[0].return_type = Type::Boolean;
    module.functions[0].body = vec![
        Stmt::Let {
            id: 20,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Point".to_string(),
                args: vec![number(4.0), number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(20)),
                property: "isAbove".to_string(),
            }),
            args: vec![number(3.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ];
    module.classes[0].methods.clear();
    module.classes[0].methods.push(Function {
        id: 102,
        name: "isAbove".to_string(),
        type_params: Vec::new(),
        params: vec![param(12, "limit", Type::Number)],
        return_type: Type::Boolean,
        body: vec![Stmt::Return(Some(scalar_predicate_method_body("x")))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    module
}

fn scalar_method_boolean_public_numeric_arg_module(case: &str, arg_ty: Type) -> Module {
    let mut module = scalar_method_boolean_predicate_module();
    module.name = format!("scalar_method_boolean_guarded_{case}_arg.ts");
    module.functions[0].params = vec![param(70, "limit", arg_ty)];
    module.functions[0].body = vec![
        Stmt::Let {
            id: 20,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Point".to_string(),
                args: vec![number(4.0), number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(20)),
                property: "isAbove".to_string(),
            }),
            args: vec![local(70)],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ];
    module
}

fn scalar_method_boolean_public_numeric_expr_arg_module() -> Module {
    let mut module = scalar_method_boolean_predicate_module();
    module.name = "scalar_method_boolean_guarded_expr_arg.ts".to_string();
    module.functions[0].params = vec![
        param(70, "limit", Type::Number),
        param(71, "delta", Type::Int32),
    ];
    module.functions[0].body = vec![
        Stmt::Let {
            id: 20,
            name: "p".to_string(),
            ty: Type::Named("Point".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Point".to_string(),
                args: vec![number(4.0), number(2.0)],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(20)),
                property: "isAbove".to_string(),
            }),
            args: vec![Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(local(70)),
                right: Box::new(Expr::Binary {
                    op: BinaryOp::Mul,
                    left: Box::new(local(71)),
                    right: Box::new(int(2)),
                }),
            }],
            type_args: Vec::new(),
            byte_offset: 0,
        })),
    ];
    module
}

fn scalar_method_int32_bitwise_module(case: &str, field_ty: Type, arg_ty: Type) -> Module {
    let mut flags = class(
        111,
        "Flags",
        vec![
            class_field("mask", field_ty.clone()),
            class_field("salt", field_ty),
        ],
    );
    flags.constructor = Some(Function {
        id: 110,
        name: "Flags_constructor".to_string(),
        type_params: Vec::new(),
        params: vec![
            param(10, "mask", Type::Int32),
            param(11, "salt", Type::Int32),
        ],
        return_type: Type::Any,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "mask".to_string(),
                value: Box::new(local(10)),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: "salt".to_string(),
                value: Box::new(local(11)),
            }),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    flags.methods.push(Function {
        id: 111,
        name: "mix".to_string(),
        type_params: Vec::new(),
        params: vec![param(12, "extra", arg_ty.clone())],
        return_type: Type::Int32,
        body: vec![Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::BitAnd,
            left: Box::new(Expr::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(Expr::Binary {
                    op: BinaryOp::BitXor,
                    left: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: "mask".to_string(),
                    }),
                    right: Box::new(local(12)),
                }),
                right: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::This),
                    property: "salt".to_string(),
                }),
            }),
            right: Box::new(int(255)),
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    let arg_is_any = matches!(&arg_ty, Type::Any);
    let call_arg = if arg_is_any { local(70) } else { int(12) };
    let params = if arg_is_any {
        vec![param(70, "extra", Type::Any)]
    } else {
        Vec::new()
    };
    module_with_classes_and_params(
        &format!("scalar_method_int32_bitwise_{case}.ts"),
        vec![flags],
        params,
        Type::Int32,
        vec![
            Stmt::Let {
                id: 20,
                name: "flags".to_string(),
                ty: Type::Named("Flags".to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: "Flags".to_string(),
                    args: vec![int(42), int(7)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }),
            },
            Stmt::Return(Some(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(local(20)),
                    property: "mix".to_string(),
                }),
                args: vec![call_arg],
                type_args: Vec::new(),
                byte_offset: 0,
            })),
        ],
    )
}

fn scalar_method_int32_bitwise_public_arg_module() -> Module {
    let mut module = scalar_method_int32_bitwise_module("guarded_arg", Type::Int32, Type::Int32);
    module.functions[0].params = vec![param(70, "extra", Type::Int32)];
    if let Stmt::Return(Some(Expr::Call { args, .. })) = &mut module.functions[0].body[1] {
        args[0] = local(70);
    } else {
        panic!("unexpected int32 bitwise scalar method fixture body");
    }
    module
}

fn scalar_method_int32_unsigned_shift_module() -> Module {
    let mut module = scalar_method_int32_bitwise_module("unsigned_shift", Type::Int32, Type::Int32);
    module.classes[0].methods[0].body = vec![Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::UShr,
        left: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::This),
            property: "mask".to_string(),
        }),
        right: Box::new(int(0)),
    }))];
    module
}

fn scalar_method_int32_bitwise_local_temp_module() -> Module {
    let mut module = scalar_method_int32_bitwise_module("local_temp", Type::Int32, Type::Int32);
    module.classes[0].methods[0].body = vec![
        Stmt::Let {
            id: 130,
            name: "mixed".to_string(),
            ty: Type::Int32,
            mutable: false,
            init: Some(Expr::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::This),
                    property: "mask".to_string(),
                }),
                right: Box::new(local(12)),
            }),
        },
        Stmt::Let {
            id: 131,
            name: "shifted".to_string(),
            ty: Type::Int32,
            mutable: false,
            init: Some(Expr::Binary {
                op: BinaryOp::Shl,
                left: Box::new(local(130)),
                right: Box::new(int(1)),
            }),
        },
        Stmt::Return(Some(Expr::Binary {
            op: BinaryOp::BitOr,
            left: Box::new(local(131)),
            right: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::This),
                property: "salt".to_string(),
            }),
        })),
    ];
    module
}

fn scalar_method_boolean_negative_module(case: &str) -> Module {
    let mut module = scalar_method_boolean_predicate_module();
    module.name = format!("scalar_method_boolean_reject_{case}.ts");
    let method_idx = module.classes[0]
        .methods
        .iter()
        .position(|method| method.name == "isAbove")
        .unwrap();
    match case {
        "mutation" => {
            module.classes[0].methods[method_idx].body = vec![
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::This),
                    property: "x".to_string(),
                    value: Box::new(local(12)),
                }),
                Stmt::Return(Some(scalar_predicate_method_body("x"))),
            ];
        }
        "unknown_call" => {
            module.classes[0].methods.push(Function {
                id: 103,
                name: "readX".to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: Type::Number,
                body: vec![Stmt::Return(Some(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::This),
                    property: "x".to_string(),
                }))],
                is_async: false,
                is_generator: false,
                is_strict: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
                was_plain_async: false,
                was_unrolled: false,
            });
            module.classes[0].methods[method_idx].body = vec![Stmt::Return(Some(Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: "readX".to_string(),
                    }),
                    args: Vec::new(),
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
                right: Box::new(local(12)),
            }))];
        }
        "accessor" => {
            module.classes[0].getters.push((
                "score".to_string(),
                Function {
                    id: 104,
                    name: "get_score".to_string(),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: Type::Number,
                    body: vec![Stmt::Return(Some(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: "x".to_string(),
                    }))],
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                    is_exported: false,
                    captures: Vec::new(),
                    decorators: Vec::new(),
                    was_plain_async: false,
                    was_unrolled: false,
                },
            ));
            module.classes[0].methods[method_idx].body =
                vec![Stmt::Return(Some(scalar_predicate_method_body("score")))];
        }
        "dynamic_property" => {
            module.classes[0].methods[method_idx].body = vec![Stmt::Return(Some(Expr::Compare {
                op: CompareOp::Gt,
                left: Box::new(Expr::IndexGet {
                    object: Box::new(Expr::This),
                    index: Box::new(Expr::String("x".to_string())),
                }),
                right: Box::new(local(12)),
            }))];
        }
        "computed_member_collision" => {
            module.classes[0]
                .computed_members
                .push(ClassComputedMember {
                    key_expr: Expr::String("isAbove".to_string()),
                    function: Function {
                        id: 105,
                        name: "__computed_isAbove".to_string(),
                        type_params: Vec::new(),
                        params: Vec::new(),
                        return_type: Type::Number,
                        body: vec![Stmt::Return(Some(number(1.0)))],
                        is_async: false,
                        is_generator: false,
                        is_strict: false,
                        is_exported: false,
                        captures: Vec::new(),
                        decorators: Vec::new(),
                        was_plain_async: false,
                        was_unrolled: false,
                    },
                    is_static: false,
                    kind: ClassComputedMemberKind::Method,
                });
        }
        "inherited_field_shadow" => {
            let base = class(99, "BasePoint", vec![class_field("isAbove", Type::Number)]);
            module.classes[0].extends_name = Some("BasePoint".to_string());
            module.classes.insert(0, base);
        }
        "any_arg" => {
            module.functions[0].params = vec![param(70, "limit", Type::Any)];
            module.functions[0].body = vec![
                Stmt::Let {
                    id: 20,
                    name: "p".to_string(),
                    ty: Type::Named("Point".to_string()),
                    mutable: false,
                    init: Some(Expr::New {
                        class_name: "Point".to_string(),
                        args: vec![number(4.0), number(2.0)],
                        type_args: Vec::new(),
                        byte_offset: 0,
                        cap_args_appended: 0,
                    }),
                },
                Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(local(20)),
                        property: "isAbove".to_string(),
                    }),
                    args: vec![local(70)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                })),
            ];
        }
        "any_arg_expr" => {
            module.functions[0].params = vec![param(70, "limit", Type::Any)];
            module.functions[0].body = vec![
                Stmt::Let {
                    id: 20,
                    name: "p".to_string(),
                    ty: Type::Named("Point".to_string()),
                    mutable: false,
                    init: Some(Expr::New {
                        class_name: "Point".to_string(),
                        args: vec![number(4.0), number(2.0)],
                        type_args: Vec::new(),
                        byte_offset: 0,
                        cap_args_appended: 0,
                    }),
                },
                Stmt::Return(Some(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(local(20)),
                        property: "isAbove".to_string(),
                    }),
                    args: vec![Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(local(70)),
                        right: Box::new(int(1)),
                    }],
                    type_args: Vec::new(),
                    byte_offset: 0,
                })),
            ];
        }
        other => panic!("unknown scalar method predicate negative fixture: {other}"),
    }
    module
}

fn artifact_has_scalar_method_inline(artifact: &serde_json::Value, method: &str) -> bool {
    let method_note = format!("method={method}");
    artifact["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note.as_str() == Some(method_note.as_str()))
                        && notes.iter().any(|note| note == "receiver=scalar_replaced")
                })
        })
}

#[test]
fn typed_f64_function_clone_emits_internal_clone_and_guarded_call() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_clone_test_module(false), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_f64_function_abi_ts__add";
    let typed = "perry_fn_typed_f64_function_abi_ts__add__typed_f64";
    let generic_body = "perry_fn_typed_f64_function_abi_ts__add__generic";
    assert!(
        ir.contains(&format!("define internal double @{typed}")),
        "{ir}"
    );
    assert!(ir.contains(&format!("define double @{public}")), "{ir}");
    assert!(
        ir.contains(&format!("define internal double @{generic_body}")),
        "{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_f64_arg_guard"), "{ir}");
    assert!(ir.contains("call double @js_typed_f64_arg_to_raw"), "{ir}");
    assert!(ir.contains(&format!("call double @{typed}")), "{ir}");
    assert!(
        ir.contains(&format!("call double @{generic_body}(")),
        "generic body fallback should remain present:\n{ir}"
    );
}

#[test]
fn typed_f64_public_trampoline_dispatches_before_generic_body() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_clone_test_module(false), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_f64_function_abi_ts__add";
    let typed = "perry_fn_typed_f64_function_abi_ts__add__typed_f64";
    let generic_body = "perry_fn_typed_f64_function_abi_ts__add__generic";
    let wrapper_ir = function_ir_section(&ir, public);

    assert!(
        ir.contains(&format!("define internal double @{generic_body}")),
        "typed function should keep a separate generic body:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "public wrapper should guard and unbox numeric JSValue args:\n{wrapper_ir}"
    );
    let typed_call = wrapper_ir
        .find(&format!("call double @{typed}("))
        .unwrap_or_else(|| panic!("public wrapper should call typed clone:\n{wrapper_ir}"));
    let fallback_call = wrapper_ir
        .find(&format!("call double @{generic_body}("))
        .unwrap_or_else(|| {
            panic!("public wrapper should call generic body fallback:\n{wrapper_ir}")
        });
    assert!(
        typed_call < fallback_call,
        "public wrapper should dispatch to typed clone before the generic body fallback:\n{wrapper_ir}"
    );
    assert!(
        !wrapper_ir.contains(&format!("call double @{public}(")),
        "public wrapper must not recursively call itself:\n{wrapper_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_f64_clone_test_module(false));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_f64_func_ref_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    })
                })
        }),
        "expected direct-call artifact to record generic body fallback:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_function_clone_does_not_call_unemitted_i64_specialized_clone() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_i64_specialized_collision_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        ir.contains("define i64 @perry_fn_typed_f64_function_abi_ts__add_i64"),
        "fixture should exercise the existing i64 specializer:\n{ir}"
    );
    assert!(
        !ir.contains("__typed_f64"),
        "i64-specialized functions must not select a missing typed-f64 clone:\n{ir}"
    );
}

#[test]
fn typed_string_function_clone_emits_internal_clone_and_guarded_wrapper() {
    let ir = String::from_utf8(
        compile_module(&typed_string_clone_test_module("positive"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_string_function_abi_ts__id";
    let typed = "perry_fn_typed_string_function_abi_ts__id__typed_string";
    let generic_body = "perry_fn_typed_string_function_abi_ts__id__generic";
    let caller = "perry_fn_typed_string_function_abi_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let caller_ir = function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!("define internal i64 @{typed}(i64 %arg1)")),
        "typed string clone should use raw i64 StringHeader handles:\n{ir}"
    );
    assert!(
        ir.contains(&format!("define double @{public}(double %arg1)")),
        "public JSValue ABI wrapper must remain emitted:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define internal double @{generic_body}(double %arg1)"
        )),
        "generic JSValue ABI body must remain emitted separately:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_string_arg_guard"),
        "public wrapper should guard string JSValue args:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains("call i64 @js_typed_string_arg_to_raw"),
        "public wrapper should unbox string args to raw handles:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains(&format!("call i64 @{typed}(i64 ")),
        "public wrapper should call the raw string clone:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains("call double @js_nanbox_string(i64 "),
        "typed string result should box at the public ABI boundary:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "string-guard failure should keep a generic body fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("typed_string_call.fast")
            && caller_ir.contains("typed_string_call.fallback")
            && caller_ir.contains("call i32 @js_typed_string_arg_guard")
            && caller_ir.contains("call i64 @js_typed_string_arg_to_raw")
            && caller_ir.contains(&format!("call i64 @{typed}(i64 "))
            && caller_ir.contains("call double @js_nanbox_string(i64 "),
        "same-module direct string call should guard/unbox, call the raw clone, and box at the call boundary:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(double ")),
        "direct string-call guard failure should target the internal generic body:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(double ")),
        "direct string-call guard failure must not recurse through the public wrapper:\n{caller_ir}"
    );
}

#[test]
fn typed_string_function_clone_rejects_unsupported_string_shapes() {
    for case in [
        "any_param",
        "number_param",
        "default_param",
        "rest_param",
        "concat_body",
    ] {
        let ir = String::from_utf8(
            compile_module(&typed_string_clone_test_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_string") && !ir.contains("__generic"),
            "{case} must stay on the ordinary JSValue ABI:\n{ir}"
        );
    }
}

#[test]
fn artifact_records_typed_string_direct_call_selection() {
    let artifact = compile_artifact_json_for_module(typed_string_clone_test_module("positive"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_string_func_ref_call"
                && record["native_rep_name"] == "js_value"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_fn_typed_string_function_abi_ts__id__typed_string",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "generic_body=perry_fn_typed_string_function_abi_ts__id__generic",
                            )
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=string(i64, ...)->string")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-string direct-call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_function_clone_accepts_mixed_raw_signature_and_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_mixed_clone_test_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_f64_mixed_function_abi_ts__add";
    let typed = "perry_fn_typed_f64_mixed_function_abi_ts__add__typed_f64";
    let generic_body = "perry_fn_typed_f64_mixed_function_abi_ts__add__generic";
    let caller = "perry_fn_typed_f64_mixed_function_abi_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(double %arg1, i32 %arg2, i1 %arg6)"
        )),
        "typed f64 clone should carry mixed raw params internally:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %arg1, double %arg2, double %arg6)"
        )),
        "public wrapper must preserve the JSValue ABI:\n{ir}"
    );
    assert!(
        typed_ir.contains("sitofp i32 %arg2 to double")
            && typed_ir.contains("fadd double")
            && !typed_ir.contains("js_typed_f64_arg_to_raw")
            && !typed_ir.contains("js_nanbox"),
        "typed clone body should avoid JSValue traffic on the hot path:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i1_arg_guard")
            && wrapper_ir.contains(&format!("call double @{typed}(double %"))
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public wrapper should guard mixed JSValue args and keep generic fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("typed_f64_call.fast")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains("call i32 @js_typed_i1_arg_to_raw")
            && caller_ir.contains(&format!("call double @{typed}(double "))
            && caller_ir.contains(&format!("call double @{generic_body}("))
            && !caller_ir.contains(&format!("call double @{public}(")),
        "same-module direct call should use the mixed raw clone plus generic body fallback, not the public wrapper:\n{caller_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_f64_mixed_clone_test_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_f64_func_ref_call"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=f64(f64, ...)->f64")
                })
        }),
        "expected mixed typed-f64 direct call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_function_clone_keeps_i32_locals_raw_until_f64_use() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_i32_local_clone_test_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_f64_i32_local_function_abi_ts__add";
    let typed = "perry_fn_typed_f64_i32_local_function_abi_ts__add__typed_f64";
    let generic_body = "perry_fn_typed_f64_i32_local_function_abi_ts__add__generic";
    let caller = "perry_fn_typed_f64_i32_local_function_abi_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(double %arg1, i32 %arg2)"
        )),
        "typed f64 clone should accept the raw i32 parameter:\n{ir}"
    );
    assert!(
        typed_ir.contains(" or i32 %arg2, 1")
            && typed_ir.contains("sitofp i32 ")
            && typed_ir.contains(" fadd double")
            && !typed_ir.contains("js_typed_i32_arg_to_raw")
            && !typed_ir.contains("js_nanbox"),
        "typed f64 clone should keep the Int32 local raw until it flows into f64 arithmetic:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call double @{typed}(double "))
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public wrapper should guard/unbox the Int32 ABI arg and keep the generic fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("typed_f64_call.fast")
            && caller_ir.contains("call i32 @js_typed_i32_arg_guard")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains(&format!("call double @{typed}(double "))
            && caller_ir.contains(&format!("call double @{generic_body}("))
            && !caller_ir.contains(&format!("call double @{public}(")),
        "same-module direct call should target the mixed raw clone with generic-body fallback:\n{caller_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_f64_i32_local_clone_test_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_f64_func_ref_call"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=f64(f64, ...)->f64")
                })
        }),
        "expected typed-f64 direct-call artifact for raw i32 local clone:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_function_clone_rejects_any_and_unsafe_mixed_parameter_signatures() {
    for case in ["any", "mixed"] {
        let ir = String::from_utf8(
            compile_module(&typed_f64_rejected_signature_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_f64") && !ir.contains("__generic"),
            "{case} unsafe ABI surface must stay generic:\n{ir}"
        );
    }
}

#[test]
fn artifact_records_typed_clone_rejection_reasons() {
    let artifact = compile_artifact_json_for_module(typed_f64_rejected_signature_module("any"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_f64_function_clone_decision"
                && record["native_rep_name"] == "js_value"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=param_not_f64")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_f64_function")
                        && notes.iter().any(|note| note == "function_id=1")
                })
        }),
        "expected typed-f64 function rejection artifact:\n{artifact:#}"
    );

    let artifact = compile_artifact_json_for_module(typed_i1_method_clone_module("any"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_i1_method_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=param_not_i1")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_i1_method")
                        && notes.iter().any(|note| note == "method=check")
                })
        }),
        "expected typed-i1 method rejection artifact:\n{artifact:#}"
    );

    let artifact = compile_artifact_json_for_module(typed_string_clone_test_module("any_param"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_string_function_clone_decision"
                && record["native_rep_name"] == "js_value"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=param_not_string")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_string_function")
                        && notes.iter().any(|note| note == "function_id=1")
                })
        }),
        "expected typed-string function rejection artifact:\n{artifact:#}"
    );

    let artifact =
        compile_artifact_json_for_module(typed_f64_closure_clone_module("mutable_capture"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_f64_closure_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=captures")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_f64_closure")
                        && notes.iter().any(|note| note == "closure_func_id=300")
                })
        }),
        "expected typed-f64 mutable-capture rejection artifact:\n{artifact:#}"
    );

    let artifact =
        compile_artifact_json_for_module(typed_string_closure_clone_module("mutable_capture"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_string_closure_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=captures")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_string_closure")
                        && notes.iter().any(|note| note == "closure_func_id=302")
                })
        }),
        "expected typed-string mutable-capture rejection artifact:\n{artifact:#}"
    );
}

#[test]
fn explain_lowering_mode_records_broad_typed_clone_rejection_reasons() {
    let default_artifact = compile_artifact_json_for_module(typed_i1_clone_test_module());
    let default_records = default_artifact["records"].as_array().unwrap();
    assert!(
        !default_records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_f64_function_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=return_type_not_f64")
                })
        }),
        "default artifact mode should keep broad clone-family mismatch noise suppressed:\n{default_artifact:#}"
    );

    let explain_artifact = compile_artifact_json_for_module_with_opts_and_clone_rejections(
        typed_i1_clone_test_module_named("typed_i1_explain_rejections.ts"),
        empty_opts(),
        true,
    );
    let explain_records = explain_artifact["records"].as_array().unwrap();
    assert!(
        explain_records.iter().any(|record| {
            record["expr_kind"] == "TypedCloneDecision"
                && record["consumer"] == "typed_f64_function_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=return_type_not_f64")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_f64_function")
                })
        }),
        "explain-lowering artifact mode should record broad clone rejection reasons:\n{explain_artifact:#}"
    );
}

#[test]
fn artifact_records_typed_f64_function_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_f64_clone_test_module(false));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_f64_func_ref_call"
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_fn_typed_f64_function_abi_ts__add__typed_f64",
                            )
                        })
                    })
                })
        }),
        "expected typed-f64 clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_function_clone_emits_internal_clone_and_guarded_call() {
    let ir =
        String::from_utf8(compile_module(&typed_i1_clone_test_module(), empty_opts()).unwrap())
            .unwrap();
    let generic = "perry_fn_typed_i1_function_abi_ts__both";
    let typed = "perry_fn_typed_i1_function_abi_ts__both__typed_i1";
    let generic_body = "perry_fn_typed_i1_function_abi_ts__both__generic";
    assert!(
        ir.contains(&format!("define internal i1 @{typed}(i1 %arg1, i1 %arg2)")),
        "typed bool clone should use i1 formal params and i1 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{generic}(double %arg1, double %arg2)"
        )),
        "public JSValue ABI wrapper must remain emitted:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define internal double @{generic_body}(double %arg1, double %arg2)"
        )),
        "generic JSValue ABI body must remain emitted separately:\n{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_i1_arg_guard"), "{ir}");
    assert!(ir.contains("call i32 @js_typed_i1_arg_to_raw"), "{ir}");
    assert!(
        ir.contains(&format!("call i1 @{typed}(i1 ")),
        "direct bool call should target the typed-i1 clone:\n{ir}"
    );
    assert!(
        ir.contains("zext i1"),
        "typed-i1 result should be converted for JSValue boxing at the call boundary:\n{ir}"
    );
    assert!(
        ir.contains("9222246136947933188") && ir.contains("9222246136947933187"),
        "typed-i1 result should box back to TAG_TRUE/TAG_FALSE:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(")),
        "boolean-guard failure should keep a generic body fallback:\n{ir}"
    );
}

#[test]
fn typed_i1_public_trampoline_dispatches_before_generic_body() {
    let ir =
        String::from_utf8(compile_module(&typed_i1_clone_test_module(), empty_opts()).unwrap())
            .unwrap();
    let public = "perry_fn_typed_i1_function_abi_ts__both";
    let typed = "perry_fn_typed_i1_function_abi_ts__both__typed_i1";
    let generic_body = "perry_fn_typed_i1_function_abi_ts__both__generic";
    let wrapper_ir = function_ir_section(&ir, public);

    assert!(
        ir.contains(&format!("define internal double @{generic_body}")),
        "typed-i1 function should keep a separate generic body:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i1_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i1_arg_to_raw"),
        "public wrapper should guard and unbox boolean JSValue args:\n{wrapper_ir}"
    );
    let typed_call = wrapper_ir
        .find(&format!("call i1 @{typed}("))
        .unwrap_or_else(|| panic!("public wrapper should call typed-i1 clone:\n{wrapper_ir}"));
    let fallback_call = wrapper_ir
        .find(&format!("call double @{generic_body}("))
        .unwrap_or_else(|| {
            panic!("public wrapper should call generic body fallback:\n{wrapper_ir}")
        });
    assert!(
        typed_call < fallback_call,
        "public wrapper should dispatch to typed clone before the generic body fallback:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains("zext i1")
            && wrapper_ir.contains("9222246136947933188")
            && wrapper_ir.contains("9222246136947933187"),
        "public wrapper should box the typed-i1 result at the ABI edge:\n{wrapper_ir}"
    );
    assert!(
        !wrapper_ir.contains(&format!("call double @{public}(")),
        "public wrapper must not recursively call itself:\n{wrapper_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_i1_clone_test_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_i1_func_ref_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected direct-call artifact to record generic body fallback:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_typed_i1_function_clone_selection() {
    let artifact =
        compile_artifact_json_for_module(typed_i1_clone_test_module_named("typed_i1_artifact.ts"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_i1_func_ref_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_fn_typed_i1_artifact_ts__both__typed_i1",
                            )
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i1(i1, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i1 clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_function_clone_rejects_any_and_mixed_parameter_signatures() {
    for case in ["any", "mixed"] {
        let ir = String::from_utf8(
            compile_module(&typed_i1_rejected_signature_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i1") && !ir.contains("__generic"),
            "{case} boolean ABI surface must stay generic:\n{ir}"
        );
    }
}

#[test]
fn typed_i1_function_clone_rejects_mixed_direct_call_inputs() {
    let ir =
        String::from_utf8(compile_module(&typed_i1_mixed_callsite_module(), empty_opts()).unwrap())
            .unwrap();
    let generic = "perry_fn_typed_i1_function_abi_ts__both";
    let typed = "perry_fn_typed_i1_function_abi_ts__both__typed_i1";
    let caller = "perry_fn_typed_i1_function_abi_ts__caller";
    let caller_ir = defined_function_ir_section(&ir, caller);
    assert!(
        ir.contains(&format!("define internal i1 @{typed}")),
        "callee should still have an eligible typed-i1 clone:\n{ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call i1 @{typed}(")),
        "call site with any/mixed inputs must not use the typed-i1 clone:\n{ir}"
    );
    assert!(
        !caller_ir.contains("call i32 @js_typed_i1_arg_guard"),
        "call site with any/mixed inputs should stay on the generic call path:\n{ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic}(")),
        "mixed direct call input should retain generic fallback call:\n{ir}"
    );
}

#[test]
fn typed_i1_numeric_predicate_function_uses_f64_params_and_public_wrapper() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_numeric_predicate_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_fn_typed_i1_numeric_predicate_ts__above";
    let typed = "perry_fn_typed_i1_numeric_predicate_ts__above__typed_i1";
    let generic_body = "perry_fn_typed_i1_numeric_predicate_ts__above__generic";
    let caller = "perry_fn_typed_i1_numeric_predicate_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(double %arg1, double %arg2)"
        )),
        "numeric predicate clone should use f64 params and i1 return:\n{ir}"
    );
    assert!(
        typed_ir.contains(" fsub ") && typed_ir.contains("fcmp ogt double"),
        "numeric predicate body should stay in native f64/i1 SSA:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw")
            && wrapper_ir.contains(&format!("call i1 @{typed}(double ")),
        "public wrapper should guard/unbox f64 args before the i1 clone:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public wrapper should retain a generic JSValue fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call i32 @js_typed_f64_arg_guard")
            && caller_ir.contains("call double @js_typed_f64_arg_to_raw")
            && caller_ir.contains(&format!("call i1 @{typed}(double ")),
        "direct FuncRef lowering should use the mixed-signature typed-i1 clone after f64 guards:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct caller should retain the generic body fallback on guard failure:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(")),
        "same-module direct caller should not bounce through the public JSValue wrapper once mixed direct-call metadata exists:\n{caller_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_i1_numeric_predicate_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_i1_func_ref_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i1(f64, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected numeric-predicate direct call artifact to record f64 typed signature:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_i32_predicate_function_uses_i32_params_and_public_wrapper() {
    let ir =
        String::from_utf8(compile_module(&typed_i1_i32_predicate_module(), empty_opts()).unwrap())
            .unwrap();
    let public = "perry_fn_typed_i1_i32_predicate_ts__above_i32";
    let typed = "perry_fn_typed_i1_i32_predicate_ts__above_i32__typed_i1";
    let generic_body = "perry_fn_typed_i1_i32_predicate_ts__above_i32__generic";
    let caller = "perry_fn_typed_i1_i32_predicate_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(i32 %arg1, i32 %arg2)"
        )),
        "Int32 predicate clone should use raw i32 params and i1 return:\n{ir}"
    );
    assert!(
        typed_ir.contains("icmp sgt i32 %arg1, %arg2") && !typed_ir.contains("fcmp "),
        "Int32 predicate body should stay in native i32/i1 SSA:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call i1 @{typed}(i32 ")),
        "public wrapper should guard/unbox Int32 args before the i1 clone:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public wrapper should retain a generic JSValue fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call i32 @js_typed_i32_arg_guard")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains(&format!("call i1 @{typed}(i32 ")),
        "direct FuncRef lowering should use the i32 typed-i1 clone after Int32 guards:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct caller should retain the generic body fallback on guard failure:\n{caller_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_i1_i32_predicate_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_i1_func_ref_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                                && text.contains(&format!("generic_body={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i1(i32, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected Int32 predicate direct call artifact to record i32 typed signature:\n{artifact:#}"
    );
}

#[test]
fn typed_i32_return_function_uses_i32_params_return_and_public_wrapper() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_return_module("positive"), empty_opts()).unwrap(),
    )
    .unwrap();
    const INT32_TAG_I64: &str = "9222809086901354496";
    let public = "perry_fn_typed_i32_return_positive_ts__mix_i32";
    let typed = "perry_fn_typed_i32_return_positive_ts__mix_i32__typed_i32";
    let generic_body = "perry_fn_typed_i32_return_positive_ts__mix_i32__generic";
    let caller = "perry_fn_typed_i32_return_positive_ts__caller";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal i32 @{typed}(i32 %arg1, i32 %arg2)"
        )),
        "typed-i32 clone should use raw i32 params and i32 return:\n{ir}"
    );
    assert!(
        typed_ir.contains(" xor i32 %arg1, %arg2")
            && typed_ir.contains(" or i32 ")
            && !typed_ir.contains(" fadd ")
            && !typed_ir.contains(" sitofp "),
        "typed-i32 body should stay in native i32 SSA:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call i32 @{typed}(i32 "))
            && wrapper_ir.contains(INT32_TAG_I64),
        "public wrapper should guard/unbox Int32 args and box raw i32 at the ABI edge:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public wrapper should retain a generic JSValue fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("typed_i32_call.fast")
            && caller_ir.contains("typed_i32_call.fallback")
            && caller_ir.contains("call i32 @js_typed_i32_arg_guard")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains(&format!("call i32 @{typed}(i32 "))
            && caller_ir.contains(INT32_TAG_I64),
        "direct FuncRef lowering should use the raw i32 clone after guards and box at the call boundary:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct caller should retain the generic body fallback on guard failure:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(")),
        "same-module direct caller should not bounce through the public JSValue wrapper:\n{caller_ir}"
    );
}

#[test]
fn artifact_records_typed_i32_function_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_i32_return_module("positive"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "Call"
                && record["consumer"] == "typed_i32_func_ref_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_fn_typed_i32_return_positive_ts__mix_i32__typed_i32",
                            )
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i32(i32, ...)->i32")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i32 direct-call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i32_return_function_rejects_annotation_only_or_unsafe_shapes() {
    for case in ["number_param", "number_return", "unsafe_add"] {
        let ir = String::from_utf8(
            compile_module(&typed_i32_return_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i32") && !ir.contains("__generic"),
            "{case} must stay on the ordinary JSValue ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_i32_method_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_method_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    const INT32_TAG_I64: &str = "9222809086901354496";
    let public = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32";
    let typed = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__typed_i32";
    let generic_body = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__generic";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir =
        defined_function_ir_section(&ir, "perry_fn_typed_i32_method_eligible_ts__probe");

    assert!(
        ir.contains(&format!(
            "define internal i32 @{typed}(i32 %arg21, i32 %arg22)"
        )),
        "typed-i32 method clone should use raw i32 params and i32 return:\n{ir}"
    );
    assert!(
        typed_ir.contains(" xor i32 %arg21, %arg22")
            && typed_ir.contains(" or i32 ")
            && !typed_ir.contains(" fadd ")
            && !typed_ir.contains(" sitofp "),
        "typed-i32 method body should stay in native i32 SSA:\n{typed_ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %this_arg, double %arg21, double %arg22)"
        )) && ir.contains(&format!(
            "define internal double @{generic_body}(double %this_arg, double %arg21, double %arg22)"
        )),
        "typed-i32 method should expose a public JSValue wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call i32 @{typed}(i32 "))
            && wrapper_ir.contains(INT32_TAG_I64),
        "public method wrapper should guard/unbox Int32 args and box raw i32 at the ABI edge:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call i32 @js_method_direct_shape_guard")
            && caller_ir.contains("typed_i32_method.fast")
            && caller_ir.contains("typed_i32_method.generic")
            && caller_ir.contains("call i32 @js_typed_i32_arg_guard")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains(&format!("call i32 @{typed}(i32 "))
            && caller_ir.contains(INT32_TAG_I64),
        "exact direct method call should guard receiver/method identity, then guard/unbox Int32 args and call the clone:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct typed-i32 guard failure should target the internal generic method body:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(")),
        "direct typed-i32 guard failure must not recurse through the public wrapper:\n{caller_ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint (ptr @{generic_body}"))
            && !ir.contains(&format!("ptrtoint ptr @{generic_body}")),
        "runtime vtable must register the public wrapper, not internal typed/generic bodies:\n{ir}"
    );
}

#[test]
fn typed_i32_method_public_trampoline_dispatches_before_generic_body() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_method_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32";
    let typed = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__typed_i32";
    let generic_body = "perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__generic";
    let wrapper_ir = function_ir_section(&ir, public);

    let typed_call = wrapper_ir
        .find(&format!("call i32 @{typed}("))
        .unwrap_or_else(|| {
            panic!("public method wrapper should call typed-i32 clone:\n{wrapper_ir}")
        });
    let fallback_call = wrapper_ir
        .find(&format!("call double @{generic_body}("))
        .unwrap_or_else(|| {
            panic!("public method wrapper should call generic body fallback:\n{wrapper_ir}")
        });
    assert!(
        typed_call < fallback_call,
        "public method wrapper should dispatch to typed clone before generic fallback:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw"),
        "public method wrapper should guard and unbox Int32 JSValue args:\n{wrapper_ir}"
    );
    assert!(
        !wrapper_ir.contains(&format!("call double @{public}(")),
        "public method wrapper must not recursively call itself:\n{wrapper_ir}"
    );
}

#[test]
fn artifact_records_typed_i32_method_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_i32_method_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_i32_method_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__typed_i32",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note
                            == "generic_method=perry_method_typed_i32_method_eligible_ts__Bits__mix_i32__generic"
                    }) && notes.iter().any(|note| note == "receiver_class=Bits")
                        && notes.iter().any(|note| note == "method=mix_i32")
                        && notes
                            .iter()
                            .any(|note| note == "typed_signature=i32(i32, ...)->i32")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i32 method direct-call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i32_method_clone_rejects_number_param_number_return_and_unsafe_add() {
    for case in ["number_param", "number_return", "unsafe_add"] {
        let ir = String::from_utf8(
            compile_module(&typed_i32_method_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i32"),
            "{case} method must stay off the typed-i32 method ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_f64_method_clone_keeps_i32_locals_raw_until_f64_use() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_i32_local_method_clone_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_f64_i32_local_method_abi_ts__Calc__mix";
    let typed = "perry_method_typed_f64_i32_local_method_abi_ts__Calc__mix__typed_f64";
    let generic_body = "perry_method_typed_f64_i32_local_method_abi_ts__Calc__mix__generic";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir =
        defined_function_ir_section(&ir, "perry_fn_typed_f64_i32_local_method_abi_ts__probe");

    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(double %arg21, i32 %arg22)"
        )),
        "typed f64 method clone should accept the raw i32 parameter:\n{ir}"
    );
    assert!(
        typed_ir.contains(" or i32 %arg22, 1")
            && typed_ir.contains("sitofp i32 ")
            && typed_ir.contains(" fadd double")
            && !typed_ir.contains("js_typed_i32_arg_to_raw")
            && !typed_ir.contains("js_nanbox"),
        "typed f64 method clone should keep the Int32 local raw until f64 arithmetic:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call double @{typed}(double "))
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public method wrapper should guard/unbox the Int32 ABI arg and keep fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("typed_f64_method.fast")
            && caller_ir.contains("typed_f64_method.generic")
            && caller_ir.contains("call i32 @js_typed_i32_arg_guard")
            && caller_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && caller_ir.contains(&format!("call double @{typed}(double "))
            && caller_ir.contains(&format!("call double @{generic_body}(")),
        "exact direct method call should use the raw clone with generic-body fallback:\n{caller_ir}"
    );
}

#[test]
fn typed_string_method_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_string_method_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_string_method_eligible_ts__Labeler__pick";
    let typed = "perry_method_typed_string_method_eligible_ts__Labeler__pick__typed_string";
    let generic_body = "perry_method_typed_string_method_eligible_ts__Labeler__pick__generic";
    let caller = "perry_fn_typed_string_method_eligible_ts__probe";
    let wrapper_ir = function_ir_section(&ir, public);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!("define internal i64 @{typed}(i64 %arg21)")),
        "typed-string method clone should use raw i64 StringHeader handles:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %this_arg, double %arg21)"
        )) && ir.contains(&format!(
            "define internal double @{generic_body}(double %this_arg, double %arg21)"
        )),
        "typed-string method should expose a public JSValue wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_string_arg_guard")
            && wrapper_ir.contains("call i64 @js_typed_string_arg_to_raw")
            && wrapper_ir.contains(&format!("call i64 @{typed}(i64 "))
            && wrapper_ir.contains("call double @js_nanbox_string(i64 ")
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "public method wrapper should guard/unbox string args, call the raw clone, box the result, and keep generic fallback:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call i32 @js_method_direct_shape_guard")
            && caller_ir.contains("typed_string_method.fast")
            && caller_ir.contains("typed_string_method.generic")
            && caller_ir.contains("call i32 @js_typed_string_arg_guard")
            && caller_ir.contains("call i64 @js_typed_string_arg_to_raw")
            && caller_ir.contains(&format!("call i64 @{typed}(i64 "))
            && caller_ir.contains("call double @js_nanbox_string(i64 "),
        "exact direct method call should guard receiver/method identity, then guard/unbox string args and call the raw clone:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct typed-string guard failure should target the internal generic method body:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(")),
        "direct typed-string guard failure must not recurse through the public wrapper:\n{caller_ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint (ptr @{generic_body}"))
            && !ir.contains(&format!("ptrtoint ptr @{generic_body}")),
        "runtime vtable must register the public wrapper, not internal typed/generic bodies:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_string_method_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_string_method_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_string_method_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_method_typed_string_method_eligible_ts__Labeler__pick__typed_string",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note
                            == "generic_method=perry_method_typed_string_method_eligible_ts__Labeler__pick__generic"
                    }) && notes.iter().any(|note| note == "receiver_class=Labeler")
                        && notes.iter().any(|note| note == "method=pick")
                        && notes
                            .iter()
                            .any(|note| note == "typed_signature=string(string)->string")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-string method direct-call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_string_method_clone_rejects_unsupported_string_shapes() {
    for case in [
        "any_param",
        "number_param",
        "default_param",
        "rest_param",
        "concat_body",
    ] {
        let ir = String::from_utf8(
            compile_module(&typed_string_method_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_string") && !ir.contains("typed_string_method.fast"),
            "{case} method must stay off the typed-string method ABI:\n{ir}"
        );
    }
}

#[test]
fn artifact_records_typed_string_method_clone_rejection_reason() {
    let artifact = compile_artifact_json_for_module(typed_string_method_clone_module("any_param"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "typed_string_method_clone_decision"
                && record["expr_kind"] == "TypedCloneDecision"
                && record["native_rep_name"] == "js_value"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|note| note == "typed_clone_rejected=param_not_string")
                        && notes
                            .iter()
                            .any(|note| note == "typed_clone_kind=typed_string_method")
                        && notes.iter().any(|note| note == "class=Labeler")
                        && notes.iter().any(|note| note == "method=pick")
                })
        }),
        "expected typed-string method rejection artifact for unsupported param:\n{artifact:#}"
    );
}

#[test]
fn typed_string_method_clone_rejects_dynamic_receiver_direct_call_site() {
    let ir = String::from_utf8(
        compile_module(
            &typed_string_method_clone_module("dynamic_receiver"),
            empty_opts(),
        )
        .unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_string_method_dynamic_receiver_ts__Labeler__pick";
    let typed = "perry_method_typed_string_method_dynamic_receiver_ts__Labeler__pick__typed_string";
    let caller_ir = defined_function_ir_section(
        &ir,
        "perry_fn_typed_string_method_dynamic_receiver_ts__probe",
    );

    assert!(
        ir.contains(&format!("define internal i64 @{typed}(i64 %arg21)"))
            && ir.contains(&format!("define double @{public}(")),
        "eligible method should still expose its public wrapper even when this call site is dynamic:\n{ir}"
    );
    assert!(
        !caller_ir.contains("typed_string_method.fast")
            && !caller_ir.contains(&format!("call i64 @{typed}(")),
        "dynamic receiver call site must not route directly to the typed-string method clone:\n{caller_ir}"
    );
}

#[test]
fn typed_i1_method_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_method_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_i1_method_eligible_ts__Switch__check";
    let generic_body = "perry_method_typed_i1_method_eligible_ts__Switch__check__generic";
    let typed = "perry_method_typed_i1_method_eligible_ts__Switch__check__typed_i1";
    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(i1 %arg21, i1 %arg22)"
        )),
        "typed method clone should use i1 formal params and i1 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %this_arg, double %arg21, double %arg22)"
        )),
        "public method ABI wrapper must remain emitted:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define internal double @{generic_body}(double %this_arg, double %arg21, double %arg22)"
        )),
        "generic method ABI body must remain emitted separately:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_method_direct_shape_guard"),
        "{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_i1_arg_guard"), "{ir}");
    assert!(ir.contains("call i32 @js_typed_i1_arg_to_raw"), "{ir}");
    assert!(
        ir.contains(&format!("call i1 @{typed}(i1 ")),
        "typed direct call should target the clone:\n{ir}"
    );
    assert!(
        ir.contains("zext i1"),
        "typed-i1 method result should be converted for JSValue boxing:\n{ir}"
    );
    assert!(
        ir.contains("9222246136947933188") && ir.contains("9222246136947933187"),
        "typed-i1 method result should box back to TAG_TRUE/TAG_FALSE:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(")),
        "boolean-guard failure should keep a generic method fallback:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method"),
        "receiver/method guard failure should keep the dynamic generic fallback:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint ptr @{typed}")),
        "typed clone must not be registered in the runtime vtable:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{generic_body}"))
            && !ir.contains(&format!("ptrtoint ptr @{generic_body}")),
        "generic body must not be registered in the runtime vtable:\n{ir}"
    );
}

#[test]
fn typed_i1_method_public_trampoline_dispatches_before_generic_body() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_method_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_i1_method_eligible_ts__Switch__check";
    let typed = "perry_method_typed_i1_method_eligible_ts__Switch__check__typed_i1";
    let generic_body = "perry_method_typed_i1_method_eligible_ts__Switch__check__generic";
    let wrapper_ir = function_ir_section(&ir, public);

    assert!(
        wrapper_ir.contains("call i32 @js_typed_i1_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i1_arg_to_raw"),
        "public method wrapper should guard and unbox boolean JSValue args:\n{wrapper_ir}"
    );
    let typed_call = wrapper_ir
        .find(&format!("call i1 @{typed}("))
        .unwrap_or_else(|| panic!("public method wrapper should call typed clone:\n{wrapper_ir}"));
    let fallback_call = wrapper_ir
        .find(&format!("call double @{generic_body}("))
        .unwrap_or_else(|| {
            panic!("public method wrapper should call generic body fallback:\n{wrapper_ir}")
        });
    assert!(
        typed_call < fallback_call,
        "public method wrapper should dispatch to typed clone before generic fallback:\n{wrapper_ir}"
    );
    assert!(
        !wrapper_ir.contains(&format!("call double @{public}(")),
        "public method wrapper must not recursively call itself:\n{wrapper_ir}"
    );
    assert!(
        wrapper_ir.contains("zext i1")
            && wrapper_ir.contains("9222246136947933188")
            && wrapper_ir.contains("9222246136947933187"),
        "public typed-i1 method wrapper should box the i1 result at the ABI boundary:\n{wrapper_ir}"
    );
}

#[test]
fn artifact_records_typed_i1_method_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_i1_method_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_i1_method_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_method_typed_i1_method_eligible_ts__Switch__check__typed_i1",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "generic_method=perry_method_typed_i1_method_eligible_ts__Switch__check__generic",
                            )
                        })
                    }) && notes.iter().any(|note| note == "method=check")
                        && notes
                            .iter()
                        .any(|note| note == "typed_signature=i1(i1, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i1 method clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_numeric_predicate_method_uses_f64_params_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_numeric_predicate_method_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_i1_numeric_method_ts__Meter__above";
    let generic_body = "perry_method_typed_i1_numeric_method_ts__Meter__above__generic";
    let typed = "perry_method_typed_i1_numeric_method_ts__Meter__above__typed_i1";
    let caller = "perry_fn_typed_i1_numeric_method_ts__probe";
    let wrapper_ir = function_ir_section(&ir, public);
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(double %arg21, double %arg22)"
        )),
        "numeric predicate method clone should use f64 params and i1 return:\n{ir}"
    );
    assert!(
        typed_ir.contains(" fsub ") && typed_ir.contains("fcmp ogt double"),
        "numeric predicate method body should stay in native f64/i1 SSA:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw")
            && wrapper_ir.contains(&format!("call i1 @{typed}(double ")),
        "public method wrapper should guard/unbox f64 args before the i1 clone:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call i32 @js_method_direct_shape_guard")
            && caller_ir.contains("call i32 @js_typed_f64_arg_guard")
            && caller_ir.contains("call double @js_typed_f64_arg_to_raw")
            && caller_ir.contains(&format!("call i1 @{typed}(double ")),
        "exact direct method call should use the mixed-signature typed-i1 clone after f64 guards:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "direct method call should retain generic body fallback on typed guard failure:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_native_call_method"),
        "receiver/method guard failure should keep the dynamic generic fallback:\n{caller_ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call double @{public}(")),
        "exact same-module direct method call should not bounce through the public JSValue wrapper:\n{caller_ir}"
    );

    let artifact = compile_artifact_json_for_module(typed_i1_numeric_predicate_method_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_i1_method_direct_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("typed_clone={typed}"))
                        })
                    }) && notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(&format!("generic_method={generic_body}"))
                        })
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i1(f64, ...)->i1")
                        && notes.iter().any(|note| note == "method=above")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected numeric-predicate method direct call artifact to record f64 typed signature:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_method_clone_rejects_any_and_mixed_parameter_signatures() {
    for case in ["any", "mixed"] {
        let ir = String::from_utf8(
            compile_module(&typed_i1_method_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i1"),
            "{case} method must stay on the generic method ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_i1_method_clone_rejects_dynamic_receiver_call_site() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_method_clone_module("dynamic"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_i1_method_dynamic_ts__Switch__check";
    let typed = "perry_method_typed_i1_method_dynamic_ts__Switch__check__typed_i1";
    assert!(
        ir.contains(&format!("define internal i1 @{typed}")),
        "eligible method should still have an internal typed-i1 clone:\n{ir}"
    );
    let wrapper_ir = function_ir_section(&ir, public);
    let non_wrapper_ir = ir.replace(wrapper_ir, "");
    assert!(
        wrapper_ir.contains(&format!("call i1 @{typed}(")),
        "dynamic dispatch should be able to reach the public typed method wrapper:\n{wrapper_ir}"
    );
    assert!(
        !non_wrapper_ir.contains(&format!("call i1 @{typed}(")),
        "dynamic receiver call must not use the direct typed-i1 method clone path:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method"),
        "dynamic receiver call should dispatch through the generic method fallback:\n{ir}"
    );
}

#[test]
fn typed_f64_method_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir =
        String::from_utf8(compile_module(&typed_f64_method_clone_module(), empty_opts()).unwrap())
            .unwrap();
    let public = "perry_method_typed_f64_method_abi_ts__Calc__mix";
    let generic_body = "perry_method_typed_f64_method_abi_ts__Calc__mix__generic";
    let typed = "perry_method_typed_f64_method_abi_ts__Calc__mix__typed_f64";
    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(double %arg21, double %arg22)"
        )),
        "typed method clone should use only f64 formal params and f64 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %this_arg, double %arg21, double %arg22)"
        )),
        "public method ABI wrapper must remain emitted:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define internal double @{generic_body}(double %this_arg, double %arg21, double %arg22)"
        )),
        "generic method ABI body must remain emitted separately:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_method_direct_shape_guard"),
        "{ir}"
    );
    assert!(ir.contains("call i32 @js_typed_f64_arg_guard"), "{ir}");
    assert!(ir.contains("call double @js_typed_f64_arg_to_raw"), "{ir}");
    assert!(
        ir.contains(&format!("call double @{typed}(double ")),
        "typed direct call should target the clone:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(")),
        "numeric-guard failure should keep a generic method fallback:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method"),
        "receiver/method guard failure should keep the dynamic generic fallback:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{typed}"))
            && !ir.contains(&format!("ptrtoint ptr @{typed}")),
        "typed clone must not be registered in the runtime vtable:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("ptrtoint (ptr @{generic_body}"))
            && !ir.contains(&format!("ptrtoint ptr @{generic_body}")),
        "generic body must not be registered in the runtime vtable:\n{ir}"
    );
}

#[test]
fn typed_f64_method_public_trampoline_dispatches_before_generic_body() {
    let ir =
        String::from_utf8(compile_module(&typed_f64_method_clone_module(), empty_opts()).unwrap())
            .unwrap();
    let public = "perry_method_typed_f64_method_abi_ts__Calc__mix";
    let typed = "perry_method_typed_f64_method_abi_ts__Calc__mix__typed_f64";
    let generic_body = "perry_method_typed_f64_method_abi_ts__Calc__mix__generic";
    let wrapper_ir = function_ir_section(&ir, public);

    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "public method wrapper should guard and unbox numeric JSValue args:\n{wrapper_ir}"
    );
    let typed_call = wrapper_ir
        .find(&format!("call double @{typed}("))
        .unwrap_or_else(|| panic!("public method wrapper should call typed clone:\n{wrapper_ir}"));
    let fallback_call = wrapper_ir
        .find(&format!("call double @{generic_body}("))
        .unwrap_or_else(|| {
            panic!("public method wrapper should call generic body fallback:\n{wrapper_ir}")
        });
    assert!(
        typed_call < fallback_call,
        "public method wrapper should dispatch to typed clone before generic fallback:\n{wrapper_ir}"
    );
    assert!(
        !wrapper_ir.contains(&format!("call double @{public}(")),
        "public method wrapper must not recursively call itself:\n{wrapper_ir}"
    );
}

#[test]
fn artifact_records_typed_f64_method_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_f64_method_clone_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_f64_method_direct_call"
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_method_typed_f64_method_abi_ts__Calc__mix__typed_f64",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "generic_method=perry_method_typed_f64_method_abi_ts__Calc__mix__generic",
                            )
                        })
                    }) && notes.iter().any(|note| note == "method=mix")
                })
        }),
        "expected typed-f64 method clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_method_clone_rejects_this_default_rest_and_any() {
    for case in ["this", "default", "rest", "any"] {
        let ir = String::from_utf8(
            compile_module(&typed_f64_method_negative_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_f64"),
            "{case} method must stay on the generic method ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_f64_receiver_method_clone_raw_loads_after_composed_guards() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_receiver_method_positive_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_method_typed_f64_receiver_method_ts__Point__score";
    let generic_body = "perry_method_typed_f64_receiver_method_ts__Point__score__generic";
    let typed = "perry_method_typed_f64_receiver_method_ts__Point__score__typed_f64_recv";
    let caller = "perry_fn_typed_f64_receiver_method_ts__probe";
    let typed_ir = defined_function_ir_section(&ir, typed);
    let caller_ir = defined_function_ir_section(&ir, caller);

    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(i64 %this_obj, double %arg21)"
        )),
        "receiver method clone should take a raw receiver handle plus raw f64 args:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "define double @{public}(double %this_arg, double %arg21)"
        )),
        "public method ABI must stay boxed:\n{ir}"
    );
    assert!(
        typed_ir.contains("inttoptr i64 %this_obj to ptr")
            && typed_ir.contains("getelementptr i8, ptr")
            && typed_ir.matches("load double").count() >= 2
            && typed_ir.contains(" fadd ")
            && typed_ir.contains(" fmul "),
        "typed receiver clone should raw-load receiver fields and stay in f64 SSA:\n{typed_ir}"
    );
    let method_guard = caller_ir
        .find("call i32 @js_typed_feedback_method_direct_call_guard")
        .unwrap_or_else(|| panic!("caller should use the full method-direct guard:\n{caller_ir}"));
    let field_guard = caller_ir
        .find("call i32 @js_typed_feedback_class_field_get_guard")
        .unwrap_or_else(|| panic!("caller should guard raw-f64 receiver fields:\n{caller_ir}"));
    let typed_call = caller_ir
        .find(&format!("call double @{typed}(i64 "))
        .unwrap_or_else(|| panic!("caller should call the receiver clone:\n{caller_ir}"));
    assert!(
        method_guard < field_guard && field_guard < typed_call,
        "receiver clone must run only after method-direct and raw-f64 field guards:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains(&format!("call double @{generic_body}(")),
        "receiver field or numeric arg guard failure should call the generic method body:\n{caller_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_native_call_method_by_id"),
        "method-direct guard failure should retain dynamic method fallback:\n{caller_ir}"
    );
    assert!(
        !ir.contains(&format!("define internal double @{}__typed_f64(", public)),
        "field-reading receiver methods should not use the receiver-less typed method ABI:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_f64_receiver_method_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_f64_receiver_method_positive_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "MethodCall"
                && record["consumer"] == "typed_f64_receiver_method_direct_call"
                && record["native_rep_name"] == "f64"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_method_typed_f64_receiver_method_ts__Point__score__typed_f64_recv",
                            )
                        })
                    }) && notes.iter().any(|note| note == "receiver_arg=i64")
                        && notes
                            .iter()
                            .any(|note| note == "raw_f64_field_guard=required")
                })
        }),
        "expected typed-f64 receiver method clone artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_receiver_method_clone_rejects_unsafe_cases() {
    for case in [
        "this_escape",
        "field_mutation",
        "nested_call",
        "non_numeric_field",
        "computed_member",
        "accessor",
    ] {
        let ir = String::from_utf8(
            compile_module(
                &typed_f64_receiver_method_negative_module(case),
                empty_opts(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_f64_recv"),
            "{case} receiver method must not get a raw receiver clone:\n{ir}"
        );
    }
}

#[test]
fn typed_f64_receiver_method_clone_rejects_inherited_and_dynamic_call_sites() {
    for case in ["inherited_receiver", "dynamic_receiver"] {
        let ir = String::from_utf8(
            compile_module(
                &typed_f64_receiver_method_negative_module(case),
                empty_opts(),
            )
            .unwrap(),
        )
        .unwrap();
        let caller = format!("perry_fn_typed_f64_receiver_method_reject_{case}_ts__probe");
        let caller_ir = defined_function_ir_section(&ir, &caller);
        assert!(
            !caller_ir.contains("__typed_f64_recv"),
            "{case} call site must not use the raw receiver clone:\n{caller_ir}"
        );
    }
}

#[test]
fn typed_f64_closure_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_closure_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_f64_closure_abi_ts__300";
    let generic_body = "perry_closure_typed_f64_closure_abi_ts__300__generic";
    let typed = "perry_closure_typed_f64_closure_abi_ts__300__typed_f64";
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!(
            "define internal double @{typed}(i64 %this_closure, double %arg31, double %arg32)"
        )),
        "typed closure clone should carry the closure handle plus f64 formal params and f64 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!("define double @{public}(i64 %this_closure"))
            && ir.contains(&format!(
                "define internal double @{generic_body}(i64 %this_closure"
            )),
        "typed closure should expose a public wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "call i64 @js_closure_alloc_singleton(ptr @{public}"
        )),
        "closure allocation must keep storing the public wrapper pointer:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"),
        "{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "public closure wrapper should guard and unbox numeric JSValue args:\n{wrapper_ir}"
    );
    assert!(
        ir.contains(&format!("call double @{typed}(i64 ")),
        "typed direct closure call should target the clone:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(i64 ")),
        "numeric-guard failure should target the internal generic closure body:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("call double @{public}(i64 ")),
        "typed guard failure must not recursively call the public closure wrapper:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_closure_call2"),
        "closure identity/arity guard failure should keep runtime dispatch fallback:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_f64_closure_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_f64_closure_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClosureCall"
                && record["consumer"] == "typed_f64_closure_direct_call"
                && record["native_rep_name"] == "f64"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_closure_typed_f64_closure_abi_ts__300__typed_f64",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note == "generic_closure=perry_closure_typed_f64_closure_abi_ts__300__generic"
                    }) && notes.iter().any(|note| note == "closure_func_id=300")
                })
        }),
        "expected typed-f64 closure clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_f64_closure_clone_accepts_immutable_numeric_capture() {
    let ir = String::from_utf8(
        compile_module(&typed_f64_closure_clone_module("capture"), empty_opts()).unwrap(),
    )
    .unwrap();
    let typed = "perry_closure_typed_f64_closure_abi_ts__300__typed_f64";
    let typed_ir = defined_function_ir_section(&ir, typed);
    assert!(
        typed_ir.contains("call i64 @js_closure_get_capture_bits(i64 %this_closure, i32 0)")
            && typed_ir.contains("bitcast i64")
            && typed_ir.contains("call double @js_typed_f64_arg_to_raw"),
        "typed-f64 captured closure should load immutable numeric capture as JSValue bits through the closure handle:\n{typed_ir}"
    );
    assert!(
        ir.contains(&format!("call double @{typed}(i64 ")),
        "typed direct call should pass the closure handle to the captured clone:\n{ir}"
    );
}

#[test]
fn typed_f64_closure_clone_rejects_any_parameter_and_mutable_capture() {
    for case in ["any", "mutable_capture"] {
        let ir = String::from_utf8(
            compile_module(&typed_f64_closure_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_f64"),
            "{case} closure must stay on the generic closure ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_i32_closure_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_closure_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_i32_closure_eligible_ts__303";
    let generic_body = "perry_closure_typed_i32_closure_eligible_ts__303__generic";
    let typed = "perry_closure_typed_i32_closure_eligible_ts__303__typed_i32";
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!(
            "define internal i32 @{typed}(i64 %this_closure, i32 %arg31, i32 %arg32)"
        )),
        "typed-i32 closure clone should carry the closure handle plus i32 params and i32 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!("define double @{public}(i64 %this_closure"))
            && ir.contains(&format!(
                "define internal double @{generic_body}(i64 %this_closure"
            )),
        "typed-i32 closure should expose a public wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "call i64 @js_closure_alloc_singleton(ptr @{public}"
        )),
        "closure allocation must keep storing the public wrapper pointer:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && wrapper_ir.contains(&format!("call i32 @{typed}(i64 %this_closure")),
        "public closure wrapper should guard/unbox Int32 JSValue args and call the typed clone:\n{wrapper_ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("closure_direct.typed_i32")
            && ir.contains("call i32 @js_typed_i32_arg_guard")
            && ir.contains("call i32 @js_typed_i32_arg_to_raw")
            && ir.contains(&format!("call i32 @{typed}(i64 ")),
        "direct local closure call should guard/unbox Int32 args and call the raw clone:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(i64 ")),
        "Int32-guard failure should target the internal generic closure body:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("call double @{public}(i64 ")),
        "typed guard failure must not recursively call the public closure wrapper:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_closure_call2"),
        "closure identity/arity guard failure should keep runtime dispatch fallback:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_i32_closure_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_i32_closure_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClosureCall"
                && record["consumer"] == "typed_i32_closure_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_closure_typed_i32_closure_eligible_ts__303__typed_i32",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note == "generic_closure=perry_closure_typed_i32_closure_eligible_ts__303__generic"
                    }) && notes.iter().any(|note| note == "closure_func_id=303")
                        && notes
                            .iter()
                            .any(|note| note == "typed_signature=i32(i64 closure, i32, ...)->i32")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i32 closure clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i32_closure_clone_accepts_immutable_i32_capture() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_closure_clone_module("capture"), empty_opts()).unwrap(),
    )
    .unwrap();
    let typed = "perry_closure_typed_i32_closure_capture_ts__303__typed_i32";
    let typed_ir = defined_function_ir_section(&ir, typed);
    assert!(
        typed_ir.contains("call i64 @js_closure_get_capture_bits(i64 %this_closure, i32 0)")
            && typed_ir.contains("bitcast i64")
            && typed_ir.contains("call i32 @js_typed_i32_arg_to_raw"),
        "typed-i32 captured closure should load immutable Int32 capture through the closure handle:\n{typed_ir}"
    );
    assert!(
        ir.contains(&format!("call i32 @{typed}(i64 ")),
        "typed direct call should pass the closure handle to the captured clone:\n{ir}"
    );
}

#[test]
fn typed_i32_closure_clone_rejects_annotation_unsafe_and_mutable_capture() {
    for case in [
        "number_param",
        "number_return",
        "unsafe_add",
        "mutable_capture",
    ] {
        let ir = String::from_utf8(
            compile_module(&typed_i32_closure_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i32"),
            "{case} closure must stay on the generic closure ABI:\n{ir}"
        );
    }

    let artifact = compile_artifact_json_for_module(typed_i32_closure_clone_module("unsafe_add"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "typed_i32_closure_clone_decision"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note == "typed_clone_rejected=return_expr_not_typed_i32_safe"
                            || note == "typed_clone_rejected=body_not_straight_line_typed"
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_clone_kind=typed_i32_closure")
                })
        }),
        "expected typed-i32 closure rejection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i32_closure_clone_rejects_dynamic_callee_call_site() {
    let ir = String::from_utf8(
        compile_module(&typed_i32_closure_clone_module("dynamic"), empty_opts()).unwrap(),
    )
    .unwrap();
    let caller = "perry_fn_typed_i32_closure_dynamic_ts__probe";
    let public = "perry_closure_typed_i32_closure_dynamic_ts__303";
    let generic_body = "perry_closure_typed_i32_closure_dynamic_ts__303__generic";
    let typed = "perry_closure_typed_i32_closure_dynamic_ts__303__typed_i32";
    let caller_ir = function_ir_section(&ir, caller);
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!("define internal i32 @{typed}(i64 %this_closure")),
        "eligible closure should still have an internal typed-i32 clone:\n{ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call i32 @{typed}("))
            && !caller_ir.contains("call i32 @js_typed_i32_arg_guard"),
        "dynamic closure callee must not direct-call the typed-i32 clone:\n{caller_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i32_arg_guard")
            && wrapper_ir.contains(&format!("call i32 @{typed}("))
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "dynamic runtime dispatch should enter the public closure wrapper, which owns typed-i32 guards:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_closure_call2"),
        "dynamic closure callee should dispatch through the generic closure fallback:\n{ir}"
    );
}

#[test]
fn typed_i1_closure_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_closure_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_i1_closure_eligible_ts__301";
    let generic_body = "perry_closure_typed_i1_closure_eligible_ts__301__generic";
    let typed = "perry_closure_typed_i1_closure_eligible_ts__301__typed_i1";
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(i64 %this_closure, i1 %arg31, i1 %arg32)"
        )),
        "typed closure clone should carry the closure handle plus i1 formal params and i1 return:\n{ir}"
    );
    assert!(
        ir.contains(&format!("define double @{public}(i64 %this_closure"))
            && ir.contains(&format!(
                "define internal double @{generic_body}(i64 %this_closure"
            )),
        "typed closure should expose a public wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "call i64 @js_closure_alloc_singleton(ptr @{public}"
        )),
        "closure allocation must keep storing the public wrapper pointer:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"),
        "{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i1_arg_guard")
            && wrapper_ir.contains("call i32 @js_typed_i1_arg_to_raw"),
        "public closure wrapper should guard and unbox boolean JSValue args:\n{wrapper_ir}"
    );
    assert!(
        ir.contains(&format!("call i1 @{typed}(i64 ")),
        "typed direct closure call should target the clone:\n{ir}"
    );
    assert!(
        ir.contains("zext i1"),
        "typed-i1 closure result should be converted for JSValue boxing:\n{ir}"
    );
    assert!(
        ir.contains("9222246136947933188") && ir.contains("9222246136947933187"),
        "typed-i1 closure result should box back to TAG_TRUE/TAG_FALSE:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(i64 ")),
        "boolean-guard failure should target the internal generic closure body:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("call double @{public}(i64 ")),
        "typed guard failure must not recursively call the public closure wrapper:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_closure_call2"),
        "closure identity/arity guard failure should keep runtime dispatch fallback:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_i1_closure_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_i1_closure_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClosureCall"
                && record["consumer"] == "typed_i1_closure_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_closure_typed_i1_closure_eligible_ts__301__typed_i1",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note == "generic_closure=perry_closure_typed_i1_closure_eligible_ts__301__generic"
                    }) && notes.iter().any(|note| note == "closure_func_id=301")
                        && notes
                            .iter()
                            .any(|note| note == "typed_signature=i1(i64 closure, i1, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-i1 closure clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_numeric_predicate_closure_uses_f64_params_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(
            &typed_i1_closure_clone_module("numeric_predicate"),
            empty_opts(),
        )
        .unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_i1_closure_numeric_predicate_ts__301";
    let generic_body = "perry_closure_typed_i1_closure_numeric_predicate_ts__301__generic";
    let typed = "perry_closure_typed_i1_closure_numeric_predicate_ts__301__typed_i1";
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!(
            "define internal i1 @{typed}(i64 %this_closure, double %arg31, double %arg32)"
        )),
        "numeric-predicate typed closure clone should use f64 params and i1 return:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_f64_arg_guard")
            && wrapper_ir.contains("call double @js_typed_f64_arg_to_raw")
            && wrapper_ir.contains(&format!("call i1 @{typed}(i64 %this_closure")),
        "public closure wrapper should guard/unbox numeric JSValue args and call the typed clone:\n{wrapper_ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"),
        "{ir}"
    );
    assert!(
        ir.contains(&format!("call i1 @{typed}(i64 "))
            && ir.contains("call i32 @js_typed_f64_arg_guard")
            && ir.contains("call double @js_typed_f64_arg_to_raw"),
        "direct local closure call should guard/unbox numeric args and call the typed clone:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(i64 ")),
        "numeric-guard failure should target the internal generic closure body:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("call double @{public}(i64 ")),
        "typed guard failure must not recursively call the public closure wrapper:\n{ir}"
    );

    let artifact =
        compile_artifact_json_for_module(typed_i1_closure_clone_module("numeric_predicate"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClosureCall"
                && record["consumer"] == "typed_i1_closure_direct_call"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_closure_typed_i1_closure_numeric_predicate_ts__301__typed_i1",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note == "generic_closure=perry_closure_typed_i1_closure_numeric_predicate_ts__301__generic"
                    }) && notes
                        .iter()
                        .any(|note| note == "typed_signature=i1(i64 closure, f64, ...)->i1")
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected numeric-predicate typed-i1 closure direct-call artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_i1_closure_clone_accepts_immutable_boolean_capture() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_closure_clone_module("capture"), empty_opts()).unwrap(),
    )
    .unwrap();
    let typed = "perry_closure_typed_i1_closure_capture_ts__301__typed_i1";
    let typed_ir = defined_function_ir_section(&ir, typed);
    assert!(
        typed_ir.contains("call i64 @js_closure_get_capture_bits(i64 %this_closure, i32 0)")
            && typed_ir.contains("bitcast i64")
            && typed_ir.contains("call i32 @js_typed_i1_arg_to_raw"),
        "typed-i1 captured closure should load immutable boolean capture as JSValue bits through the closure handle:\n{typed_ir}"
    );
    assert!(
        ir.contains(&format!("call i1 @{typed}(i64 ")),
        "typed direct call should pass the closure handle to the captured clone:\n{ir}"
    );
}

#[test]
fn typed_i1_closure_clone_rejects_any_mixed_and_mutable_capture() {
    for case in ["any", "mixed", "mutable_capture"] {
        let ir = String::from_utf8(
            compile_module(&typed_i1_closure_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_i1"),
            "{case} closure must stay on the generic closure ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_i1_closure_clone_rejects_dynamic_callee_call_site() {
    let ir = String::from_utf8(
        compile_module(&typed_i1_closure_clone_module("dynamic"), empty_opts()).unwrap(),
    )
    .unwrap();
    let caller = "perry_fn_typed_i1_closure_dynamic_ts__probe";
    let public = "perry_closure_typed_i1_closure_dynamic_ts__301";
    let generic_body = "perry_closure_typed_i1_closure_dynamic_ts__301__generic";
    let typed = "perry_closure_typed_i1_closure_dynamic_ts__301__typed_i1";
    let caller_ir = function_ir_section(&ir, caller);
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!("define internal i1 @{typed}(i64 %this_closure")),
        "eligible closure should still have an internal typed-i1 clone:\n{ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call i1 @{typed}("))
            && !caller_ir.contains("call i32 @js_typed_i1_arg_guard"),
        "dynamic closure callee must not direct-call the typed-i1 clone:\n{caller_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_i1_arg_guard")
            && wrapper_ir.contains(&format!("call i1 @{typed}("))
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "dynamic runtime dispatch should enter the public closure wrapper, which owns typed guards:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_closure_call2"),
        "dynamic closure callee should dispatch through the generic closure fallback:\n{ir}"
    );
}

#[test]
fn typed_string_closure_clone_emits_internal_clone_and_guarded_direct_call() {
    let ir = String::from_utf8(
        compile_module(&typed_string_closure_clone_module("eligible"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_string_closure_eligible_ts__302";
    let generic_body = "perry_closure_typed_string_closure_eligible_ts__302__generic";
    let typed = "perry_closure_typed_string_closure_eligible_ts__302__typed_string";
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!(
            "define internal i64 @{typed}(i64 %this_closure, i64 %arg31)"
        )),
        "typed string closure clone should carry the closure handle plus raw string handles:\n{ir}"
    );
    assert!(
        ir.contains(&format!("define double @{public}(i64 %this_closure"))
            && ir.contains(&format!(
                "define internal double @{generic_body}(i64 %this_closure"
            )),
        "typed string closure should expose a public wrapper and keep an internal generic body:\n{ir}"
    );
    assert!(
        ir.contains(&format!(
            "call i64 @js_closure_alloc_singleton(ptr @{public}"
        )),
        "closure allocation must keep storing the public wrapper pointer:\n{ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_string_arg_guard")
            && wrapper_ir.contains("call i64 @js_typed_string_arg_to_raw")
            && wrapper_ir.contains(&format!("call i64 @{typed}(i64 %this_closure"))
            && wrapper_ir.contains("call double @js_nanbox_string"),
        "public closure wrapper should guard/unbox string JSValue args, call the raw clone, and box at the boundary:\n{wrapper_ir}"
    );
    assert!(
        ir.contains("call i32 @js_typed_feedback_closure_direct_call_guard"),
        "{ir}"
    );
    assert!(
        ir.contains("closure_direct.typed_string")
            && ir.contains("call i32 @js_typed_string_arg_guard")
            && ir.contains("call i64 @js_typed_string_arg_to_raw")
            && ir.contains(&format!("call i64 @{typed}(i64 "))
            && ir.contains("call double @js_nanbox_string"),
        "direct local closure call should guard/unbox string args and call the raw clone:\n{ir}"
    );
    assert!(
        ir.contains(&format!("call double @{generic_body}(i64 ")),
        "string-guard failure should target the internal generic closure body:\n{ir}"
    );
    assert!(
        !ir.contains(&format!("call double @{public}(i64 ")),
        "typed guard failure must not recursively call the public closure wrapper:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_closure_call1"),
        "closure identity/arity guard failure should keep runtime dispatch fallback:\n{ir}"
    );
}

#[test]
fn typed_string_closure_clone_accepts_immutable_string_capture() {
    let ir = String::from_utf8(
        compile_module(&typed_string_closure_clone_module("capture"), empty_opts()).unwrap(),
    )
    .unwrap();
    let public = "perry_closure_typed_string_closure_capture_ts__302";
    let generic_body = "perry_closure_typed_string_closure_capture_ts__302__generic";
    let typed = "perry_closure_typed_string_closure_capture_ts__302__typed_string";
    let typed_ir = defined_function_ir_section(&ir, typed);
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        typed_ir.contains("call i64 @js_closure_get_capture_bits(i64 %this_closure, i32 0)")
            && typed_ir.contains("bitcast i64")
            && typed_ir.contains("call i64 @js_typed_string_arg_to_raw"),
        "typed-string captured closure should load immutable string capture as guarded JSValue bits through the closure handle:\n{typed_ir}"
    );
    assert!(
        wrapper_ir.contains("call i64 @js_closure_get_capture_bits(i64 %this_closure, i32 0)")
            && wrapper_ir.contains("call i32 @js_typed_string_arg_guard"),
        "public typed-string closure wrapper should guard immutable string captures before entering the raw clone:\n{wrapper_ir}"
    );
    assert!(
        ir.contains("closure_direct.typed_string")
            && ir.contains("call i64 @js_closure_get_capture_bits")
            && ir.contains("call i32 @js_typed_string_arg_guard")
            && ir.contains(&format!("call i64 @{typed}(i64 "))
            && ir.contains(&format!("call double @{generic_body}(i64 ")),
        "direct local call should guard string captures, call the raw clone on success, and keep a generic fallback:\n{ir}"
    );
}

#[test]
fn artifact_records_typed_string_closure_clone_selection() {
    let artifact = compile_artifact_json_for_module(typed_string_closure_clone_module("eligible"));
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ClosureCall"
                && record["consumer"] == "typed_string_closure_direct_call"
                && record["native_rep_name"] == "js_value"
                && record["llvm_ty"] == "double"
                && record["native_value_state"] == "region_local"
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| {
                        note.as_str().is_some_and(|text| {
                            text.contains(
                                "typed_clone=perry_closure_typed_string_closure_eligible_ts__302__typed_string",
                            )
                        })
                    }) && notes.iter().any(|note| {
                        note == "generic_closure=perry_closure_typed_string_closure_eligible_ts__302__generic"
                    }) && notes.iter().any(|note| note == "closure_func_id=302")
                        && notes.iter().any(|note| {
                            note == "typed_signature=string(i64 closure, string)->string"
                        })
                        && notes
                            .iter()
                            .any(|note| note == "boxed_result_at=direct_call_boundary")
                })
        }),
        "expected typed-string closure clone selection artifact:\n{artifact:#}"
    );
}

#[test]
fn typed_string_closure_clone_rejects_any_and_mutable_capture() {
    for case in ["any", "mutable_capture"] {
        let ir = String::from_utf8(
            compile_module(&typed_string_closure_clone_module(case), empty_opts()).unwrap(),
        )
        .unwrap();
        assert!(
            !ir.contains("__typed_string"),
            "{case} closure must stay on the generic closure ABI:\n{ir}"
        );
    }
}

#[test]
fn typed_string_closure_clone_rejects_dynamic_callee_call_site() {
    let ir = String::from_utf8(
        compile_module(&typed_string_closure_clone_module("dynamic"), empty_opts()).unwrap(),
    )
    .unwrap();
    let caller = "perry_fn_typed_string_closure_dynamic_ts__probe";
    let public = "perry_closure_typed_string_closure_dynamic_ts__302";
    let generic_body = "perry_closure_typed_string_closure_dynamic_ts__302__generic";
    let typed = "perry_closure_typed_string_closure_dynamic_ts__302__typed_string";
    let caller_ir = function_ir_section(&ir, caller);
    let wrapper_ir = function_ir_section(&ir, public);
    assert!(
        ir.contains(&format!("define internal i64 @{typed}(i64 %this_closure")),
        "eligible closure should still have an internal typed-string clone:\n{ir}"
    );
    assert!(
        !caller_ir.contains(&format!("call i64 @{typed}("))
            && !caller_ir.contains("call i32 @js_typed_string_arg_guard"),
        "dynamic closure callee must not direct-call the typed-string clone:\n{caller_ir}"
    );
    assert!(
        wrapper_ir.contains("call i32 @js_typed_string_arg_guard")
            && wrapper_ir.contains(&format!("call i64 @{typed}("))
            && wrapper_ir.contains("call double @js_nanbox_string")
            && wrapper_ir.contains(&format!("call double @{generic_body}(")),
        "dynamic runtime dispatch should enter the public closure wrapper, which owns typed string guards:\n{wrapper_ir}"
    );
    assert!(
        caller_ir.contains("call double @js_closure_call1"),
        "dynamic closure callee should dispatch through the generic closure fallback:\n{ir}"
    );
}

#[test]
fn scalar_replaced_simple_method_call_inlines_summary_without_dispatch() {
    let ir =
        String::from_utf8(compile_module(&scalar_method_summary_module(), empty_opts()).unwrap())
            .unwrap();
    assert!(
        !ir.contains("call double @js_native_call_method"),
        "scalar-replaced summarized method call should not dispatch dynamically:\n{ir}"
    );
    assert!(
        !ir.contains("call double @perry_method_scalar_method_summary_ts__Point_sum"),
        "scalar-replaced summarized method call should inline the method body:\n{ir}"
    );
}

#[test]
fn artifact_records_scalar_replaced_method_summary_inline() {
    let artifact = compile_artifact_json_for_module(scalar_method_summary_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record["local_id"] == 20
                && record["native_value_state"] == "region_local"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "exact_receiver_summary",
                )
                && record["notes"].as_array().is_some_and(|notes| {
                    notes.iter().any(|note| note == "class=Point")
                        && notes.iter().any(|note| note == "method=sum")
                        && notes.iter().any(|note| note == "receiver=scalar_replaced")
                })
        }),
        "expected scalar method summary inline artifact:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_summary_rejects_own_property_shadow() {
    let artifact = compile_artifact_json_for_module(scalar_method_shadowed_by_field_module());
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
        }),
        "own data property shadowing the method must block scalar method inlining:\n{artifact:#}"
    );
}

#[test]
fn scalar_replaced_numeric_method_with_local_temps_inlines_without_dispatch_or_allocation() {
    let module = scalar_method_numeric_local_temp_module("inline", false);
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        !ir.contains("call double @js_native_call_method"),
        "scalar-replaced numeric method with local temps should not dispatch dynamically:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_object_alloc"),
        "scalar-replaced numeric method with local temps should not materialize the receiver:\n{ir}"
    );
    assert!(
        ir.contains("fadd double") && ir.contains("fmul double"),
        "numeric local temp summary should rebuild native arithmetic in the inlined body:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "exact_receiver_summary",
                )
                && record_has_note(record, "method=weighted")
                && record_has_note(record, "summary_return=number")
        }),
        "expected scalar numeric local-temp summary inline artifact:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_local_temp_rejects_mutable_binding() {
    let module = scalar_method_numeric_local_temp_module("mutable", true);
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_call_method"),
        "mutable local temp must keep dynamic method dispatch fallback:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_object_alloc"),
        "mutable local temp must materialize the scalar receiver for fallback:\n{ir}"
    );
    let artifact = compile_artifact_json_for_module(module);
    assert!(
        !artifact_has_scalar_method_inline(&artifact, "weighted"),
        "mutable local temp must not record a scalar method summary inline:\n{artifact:#}"
    );
}

#[test]
fn scalar_replaced_boolean_method_predicate_inlines_without_dispatch_or_allocation() {
    let ir = String::from_utf8(
        compile_module(&scalar_method_boolean_predicate_module(), empty_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        !ir.contains("call double @js_native_call_method"),
        "scalar-replaced boolean predicate should not dispatch dynamically:\n{ir}"
    );
    assert!(
        !ir.contains("call double @perry_method_scalar_method_boolean_predicate_ts__Point_isAbove"),
        "scalar-replaced boolean predicate should inline the method body:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_object_alloc"),
        "scalar-replaced boolean predicate receiver should not heap-allocate:\n{ir}"
    );
    assert!(
        !ir.contains("call ptr @js_inline_arena_slow_alloc"),
        "scalar-replaced boolean predicate receiver should not use inline heap allocation:\n{ir}"
    );
}

#[test]
fn artifact_records_scalar_replaced_boolean_method_predicate_inline() {
    let artifact = compile_artifact_json_for_module(scalar_method_boolean_predicate_module());
    assert!(
        artifact_has_scalar_method_inline(&artifact, "isAbove"),
        "expected scalar boolean method predicate summary inline artifact:\n{artifact:#}"
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "exact_receiver_summary",
                )
                && record_has_note(record, "method=isAbove")
        }),
        "expected scalar boolean method predicate inline record to consume the scalar method summary fact:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_boolean_predicate_rejects_mutation_call_accessor_and_dynamic_property() {
    for case in [
        "mutation",
        "unknown_call",
        "accessor",
        "dynamic_property",
        "computed_member_collision",
        "inherited_field_shadow",
    ] {
        let module = scalar_method_boolean_negative_module(case);
        let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
        assert!(
            ir.contains("call double @js_native_call_method"),
            "{case} must keep dynamic method dispatch fallback:\n{ir}"
        );
        assert!(
            ir.contains("call i64 @js_object_alloc"),
            "{case} must keep heap allocation fallback for the receiver:\n{ir}"
        );

        let artifact = compile_artifact_json_for_module(module);
        assert!(
            !artifact_has_scalar_method_inline(&artifact, "isAbove"),
            "{case} must not record a scalar method summary inline:\n{artifact:#}"
        );
    }
}

#[test]
fn scalar_method_boolean_predicate_rejects_unproven_numeric_arguments() {
    let module = scalar_method_boolean_negative_module("any_arg");
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_call_method_by_id"),
        "any arg must keep generic method dispatch:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_object_alloc_class_inline_keys"),
        "any arg fallback must materialize the scalar receiver with stable class keys before dispatch:\n{ir}"
    );
    assert!(
        ir.contains("call void @js_gc_init_typed_shape_layout"),
        "any arg fallback materialization must install typed shape pointer/raw-f64 bitmap evidence:\n{ir}"
    );
    let fallback_block = {
        let start = ir
            .find("call i64 @js_object_alloc_class_inline_keys")
            .unwrap_or_else(|| panic!("missing scalar receiver materialization call:\n{ir}"));
        let end = ir[start..]
            .find("call double @js_native_call_method_by_id")
            .map(|offset| start + offset)
            .unwrap_or_else(|| {
                panic!("missing scalar method by-id dispatch after fallback:\n{ir}")
            });
        &ir[start..end]
    };
    assert!(
        !fallback_block.contains("call void @js_object_set_field_by_name"),
        "stable scalar receiver materialization should restore known fields with direct slots, not named dynamic stores:\n{fallback_block}"
    );
    assert!(
        !ir.contains("scalar_method_arg_guard.fast"),
        "any arg must not use the guarded scalar inline path:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    assert!(
        !artifact_has_scalar_method_inline(&artifact, "isAbove"),
        "any arg must not record a scalar method summary inline:\n{artifact:#}"
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().filter(|record| {
            record["expr_kind"] == "ScalarReceiverMaterializeField"
                && record["consumer"] == "scalar_receiver_materialize.direct_field_store"
                && record["local_id"] == 20
                && record["access_mode"] == "checked_native"
                && record["materialization_reason"] == "runtime_api"
                && record_has_note(record, "receiver_materialization=direct_slot")
                && record_has_note(record, "field_layout=fixed_slot_array")
                && record_has_note(record, "raw_f64_field=1")
                && record_has_note(record, "pointer_bitmap=non_pointer")
        }).count() == 2,
        "fallback materialization should restore both scalar numeric fields through direct fixed slots:\n{artifact:#}"
    );
    assert!(
        records.iter().filter(|record| {
            record["expr_kind"] == "WriteBarrierElided"
                && record["consumer"] == "write_barrier.elided_scalar_receiver_materialize_raw_f64"
                && record["local_id"] == 20
                && record_has_note(record, "reason=scalar_receiver_raw_f64_field_pointer_free")
                && record_has_note(record, "pointer_bitmap=non_pointer")
        }).count() == 2,
        "fallback materialization should record raw-f64 pointer-free barrier elision for both fields:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_materialized_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_scalar_method_summary_fact(record, "rejected_facts", "generic_arg")
                && record_has_scalar_method_summary_detail(
                    record,
                    "rejected_facts",
                    "generic_arg",
                    "generic_argument",
                )
                && record_has_note(record, "scalar_method_fallback=generic_arg")
                && record_has_note(record, "method=isAbove")
        }),
        "any arg fallback should record rejected scalar method summary evidence:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_boolean_predicate_rejects_unproven_numeric_argument_expressions() {
    let module = scalar_method_boolean_negative_module("any_arg_expr");
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("call double @js_native_call_method_by_id"),
        "any arg expression must keep generic method dispatch:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_object_alloc"),
        "any arg expression fallback must materialize the scalar receiver before dispatch:\n{ir}"
    );
    assert!(
        !ir.contains("scalar_method_arg_guard.fast"),
        "any arg expression must not use the guarded scalar inline path:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    assert!(
        !artifact_has_scalar_method_inline(&artifact, "isAbove"),
        "any arg expression must not record a scalar method summary inline:\n{artifact:#}"
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_materialized_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_scalar_method_summary_fact(record, "rejected_facts", "generic_arg")
                && record_has_scalar_method_summary_detail(
                    record,
                    "rejected_facts",
                    "generic_arg",
                    "generic_argument",
                )
                && record_has_note(record, "scalar_method_fallback=generic_arg")
                && record_has_note(record, "method=isAbove")
        }),
        "any arg expression fallback should record rejected scalar method summary evidence:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_boolean_predicate_guards_public_numeric_arguments() {
    for (case, arg_ty) in [("number", Type::Number), ("int32", Type::Int32)] {
        let module = scalar_method_boolean_public_numeric_arg_module(case, arg_ty);
        let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
        assert!(
            ir.contains("scalar_method_arg_guard.fast")
                && ir.contains("scalar_method_arg_guard.fallback")
                && ir.contains("call i32 @js_typed_f64_arg_guard")
                && ir.contains("call double @js_typed_f64_arg_to_raw"),
            "{case} public numeric arg should guard/unbox before scalar inline:\n{ir}"
        );
        assert!(
            ir.contains("call double @js_native_call_method_by_id"),
            "{case} public numeric arg should keep a generic fallback:\n{ir}"
        );
        let materialize = ir
            .find("call i64 @js_object_alloc")
            .unwrap_or_else(|| panic!("{case} fallback should materialize receiver:\n{ir}"));
        let dispatch = ir
            .find("call double @js_native_call_method_by_id")
            .unwrap_or_else(|| panic!("{case} fallback should dispatch generically:\n{ir}"));
        assert!(
            materialize < dispatch,
            "{case} fallback must materialize before generic dispatch:\n{ir}"
        );

        let artifact = compile_artifact_json_for_module(module);
        assert!(
            artifact_has_scalar_method_inline(&artifact, "isAbove"),
            "{case} public numeric arg should still record scalar inline fast path:\n{artifact:#}"
        );
        let records = artifact["records"].as_array().unwrap();
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "ScalarMethodCall"
                    && record["consumer"] == "scalar_method_summary_inline"
                    && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                    && record_has_scalar_method_summary_detail(
                        record,
                        "consumed_facts",
                        "consumed",
                        "guarded_numeric_args_fast_path",
                    )
                    && record_has_note(record, "arg_guard=js_typed_f64_arg_guard")
                    && record_has_note(record, "method=isAbove")
            }),
            "{case} public numeric arg should record guarded scalar inline summary evidence:\n{artifact:#}"
        );
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "ScalarMethodCall"
                    && record["consumer"] == "scalar_method_summary_materialized_fallback"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "runtime_api"
                    && record_has_scalar_method_summary_fact(
                        record,
                        "rejected_facts",
                        "arg_guard_failed",
                    )
                    && record_has_scalar_method_summary_detail(
                        record,
                        "rejected_facts",
                        "arg_guard_failed",
                        "guarded_numeric_args_fallback",
                    )
                    && record_has_note(record, "scalar_method_fallback=arg_guard_failed")
                    && record_has_note(record, "arg_guard=js_typed_f64_arg_guard")
                    && record_has_note(record, "method=isAbove")
            }),
            "{case} public numeric arg should record guarded scalar fallback summary evidence:\n{artifact:#}"
        );
    }
}

#[test]
fn scalar_method_boolean_predicate_guards_public_numeric_argument_expressions() {
    let module = scalar_method_boolean_public_numeric_expr_arg_module();
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("scalar_method_arg_guard.fast")
            && ir.contains("scalar_method_arg_guard.fallback")
            && ir.matches("call i32 @js_typed_f64_arg_guard").count() >= 2
            && ir.matches("call double @js_typed_f64_arg_to_raw").count() >= 2
            && ir.contains("fmul double")
            && ir.contains("fadd double"),
        "public numeric arg expression should guard/unbox locals and rebuild arithmetic as raw f64 before scalar inline:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method_by_id"),
        "public numeric arg expression should keep a generic fallback:\n{ir}"
    );
    let fast = ir
        .find("scalar_method_arg_guard.fast")
        .unwrap_or_else(|| panic!("missing guarded fast block:\n{ir}"));
    let fallback = ir
        .find("scalar_method_arg_guard.fallback")
        .unwrap_or_else(|| panic!("missing guarded fallback block:\n{ir}"));
    let materialize = ir
        .find("call i64 @js_object_alloc")
        .unwrap_or_else(|| panic!("fallback should materialize receiver:\n{ir}"));
    let dispatch = ir
        .find("call double @js_native_call_method_by_id")
        .unwrap_or_else(|| panic!("fallback should dispatch generically:\n{ir}"));
    assert!(
        fast < fallback && fallback < materialize && materialize < dispatch,
        "guarded expression fast path must precede materialized generic fallback:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    assert!(
        artifact_has_scalar_method_inline(&artifact, "isAbove"),
        "public numeric arg expression should record scalar inline fast path:\n{artifact:#}"
    );
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "guarded_numeric_args_fast_path",
                )
                && record_has_note(record, "method=isAbove")
                && record_has_note(record, "receiver=scalar_replaced")
                && record_has_note(record, "arg_guard=public_numeric_expr")
                && record_has_note(record, "guarded_arg_count=1")
        }),
        "public numeric arg expression should record guarded scalar inline summary evidence:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_materialized_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_scalar_method_summary_fact(record, "rejected_facts", "arg_guard_failed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "rejected_facts",
                    "arg_guard_failed",
                    "guarded_numeric_args_fallback",
                )
                && record_has_note(record, "scalar_method_fallback=arg_guard_failed")
                && record_has_note(record, "arg_guard=public_numeric_expr")
                && record_has_note(record, "method=isAbove")
        }),
        "public numeric arg expression should record guarded scalar fallback summary evidence:\n{artifact:#}"
    );
}

#[test]
fn scalar_replaced_int32_bitwise_method_inlines_without_dispatch_or_allocation() {
    let module = scalar_method_int32_bitwise_module("inline", Type::Int32, Type::Int32);
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        !ir.contains("call double @js_native_call_method"),
        "scalar-replaced Int32 bitwise method should not dispatch dynamically:\n{ir}"
    );
    assert!(
        !ir.contains("call double @perry_method_scalar_method_int32_bitwise_inline_ts__Flags_mix"),
        "scalar-replaced Int32 bitwise method should inline the method body:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_object_alloc"),
        "scalar-replaced Int32 bitwise receiver should not heap-allocate:\n{ir}"
    );
    assert!(
        ir.contains("xor i32") && ir.contains("or i32") && ir.contains("and i32"),
        "Int32 bitwise summary should lower to native i32 operators in the inlined body:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record["local_id"] == 20
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "exact_receiver_summary",
                )
                && record_has_note(record, "class=Flags")
                && record_has_note(record, "method=mix")
                && record_has_note(record, "receiver=scalar_replaced")
                && record_has_note(record, "summary_return=int32")
        }),
        "expected Int32 scalar method summary inline artifact:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_int32_bitwise_guards_public_int32_argument_and_preserves_fallback() {
    let module = scalar_method_int32_bitwise_public_arg_module();
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        ir.contains("scalar_method_arg_guard.fast")
            && ir.contains("scalar_method_arg_guard.fallback")
            && ir.contains("call i32 @js_typed_i32_arg_guard")
            && ir.contains("call i32 @js_typed_i32_arg_to_raw"),
        "public Int32 arg should guard/unbox before scalar Int32 summary inline:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_native_call_method_by_id"),
        "public Int32 arg should keep a generic by-ID fallback:\n{ir}"
    );
    let materialize = ir
        .find("call i64 @js_object_alloc")
        .unwrap_or_else(|| panic!("fallback should materialize receiver:\n{ir}"));
    let dispatch = ir
        .find("call double @js_native_call_method_by_id")
        .unwrap_or_else(|| panic!("fallback should dispatch generically:\n{ir}"));
    assert!(
        materialize < dispatch,
        "fallback must materialize before generic dispatch:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "guarded_numeric_args_fast_path",
                )
                && record_has_note(record, "method=mix")
                && record_has_note(record, "summary_return=int32")
                && record_has_note(record, "arg_guard=js_typed_i32_arg_guard")
                && record_has_note(record, "guarded_arg_count=1")
        }),
        "guarded public Int32 arg should record scalar inline fast path:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_materialized_fallback"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == "runtime_api"
                && record_has_scalar_method_summary_fact(
                    record,
                    "rejected_facts",
                    "arg_guard_failed",
                )
                && record_has_scalar_method_summary_detail(
                    record,
                    "rejected_facts",
                    "arg_guard_failed",
                    "guarded_numeric_args_fallback",
                )
                && record_has_note(record, "scalar_method_fallback=arg_guard_failed")
                && record_has_note(record, "arg_guard=js_typed_i32_arg_guard")
                && record_has_note(record, "method=mix")
        }),
        "guarded public Int32 arg should record scalar fallback evidence:\n{artifact:#}"
    );
}

#[test]
fn scalar_replaced_int32_bitwise_method_with_local_temps_inlines_without_dispatch() {
    let module = scalar_method_int32_bitwise_local_temp_module();
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    assert!(
        !ir.contains("call double @js_native_call_method"),
        "scalar-replaced Int32 local-temp method should not dispatch dynamically:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_object_alloc"),
        "scalar-replaced Int32 local-temp method should not materialize the receiver:\n{ir}"
    );
    assert!(
        ir.contains("xor i32") && ir.contains("shl i32") && ir.contains("or i32"),
        "Int32 local temp summary should keep bitwise temps in native i32:\n{ir}"
    );

    let artifact = compile_artifact_json_for_module(module);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "ScalarMethodCall"
                && record["consumer"] == "scalar_method_summary_inline"
                && record_has_scalar_method_summary_fact(record, "consumed_facts", "consumed")
                && record_has_scalar_method_summary_detail(
                    record,
                    "consumed_facts",
                    "consumed",
                    "exact_receiver_summary",
                )
                && record_has_note(record, "method=mix")
                && record_has_note(record, "summary_return=int32")
        }),
        "expected Int32 local-temp scalar method summary inline artifact:\n{artifact:#}"
    );
}

#[test]
fn scalar_method_int32_bitwise_rejects_unproven_or_unsigned_shapes() {
    for (case, module) in [
        (
            "number_field",
            scalar_method_int32_bitwise_module("number_field", Type::Number, Type::Int32),
        ),
        (
            "unsigned_shift",
            scalar_method_int32_unsigned_shift_module(),
        ),
        (
            "any_arg",
            scalar_method_int32_bitwise_module("any_arg", Type::Int32, Type::Any),
        ),
    ] {
        let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
        assert!(
            ir.contains("call double @js_native_call_method"),
            "{case} must keep dynamic method dispatch fallback:\n{ir}"
        );
        assert!(
            ir.contains("call i64 @js_object_alloc"),
            "{case} must keep heap allocation fallback for the receiver:\n{ir}"
        );

        let artifact = compile_artifact_json_for_module(module);
        assert!(
            !artifact_has_scalar_method_inline(&artifact, "mix"),
            "{case} must not record a scalar Int32 method summary inline:\n{artifact:#}"
        );
    }
}

#[test]
fn static_property_access_on_computed_class_uses_property_id_wrappers() {
    let dynamic = class_with_computed_member(141, "DynamicShape", vec![]);
    let module = module_with_classes_and_params(
        "property_id_static_access.ts",
        vec![dynamic],
        vec![
            param(1, "obj", Type::Named("DynamicShape".to_string())),
            param(2, "value", Type::Number),
        ],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(1)),
                property: "score".to_string(),
                value: Box::new(local(2)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "score".to_string(),
            })),
        ],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call void @js_object_set_field_by_property_id"),
        "computed-member class static property stores should use property-id ABI:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_object_get_field_by_property_id_f64"),
        "computed-member class static property reads should use property-id ABI:\n{ir}"
    );
}

#[test]
fn static_name_method_fallback_uses_method_id_wrapper() {
    let module = module_with_classes_and_params(
        "method_id_static_name_fallback.ts",
        Vec::new(),
        vec![param(1, "obj", Type::Any), param(2, "arg", Type::Number)],
        Type::Number,
        vec![Stmt::Return(Some(call(
            Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "score".to_string(),
            },
            vec![local(2)],
        )))],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_typed_feedback_native_call_method_by_id"),
        "static-name dynamic method fallback should use typed-feedback method-id ABI:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_typed_feedback_native_call_method(i64"),
        "static-name dynamic method fallback should not pass raw name bytes:\n{ir}"
    );
}

#[test]
fn static_name_spread_method_fallback_uses_method_id_wrapper() {
    let module = module_with_classes_and_params(
        "method_id_spread_static_name_fallback.ts",
        Vec::new(),
        vec![
            param(1, "obj", Type::Any),
            param(2, "args", Type::Array(Box::new(Type::Any))),
        ],
        Type::Number,
        vec![Stmt::Return(Some(Expr::CallSpread {
            callee: Box::new(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "score".to_string(),
            }),
            args: vec![CallArg::Spread(local(2))],
            type_args: Vec::new(),
        }))],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_native_call_method_apply_by_id"),
        "static-name spread fallback should use method-id apply ABI:\n{ir}"
    );
}

#[test]
fn static_name_class_method_value_uses_method_id_bind_wrapper() {
    let mut calc = class(209, "Calc", Vec::new());
    calc.methods.push(Function {
        id: 2090,
        name: "score".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(number(1.0)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let module = module_with_classes_and_params(
        "method_id_class_method_value.ts",
        vec![calc],
        vec![param(1, "obj", Type::Named("Calc".to_string()))],
        Type::Any,
        vec![Stmt::Return(Some(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(local(1)),
            property: "score".to_string(),
        }))],
    );

    let ir = compile_ir_for_module_with_opts(module, empty_opts()).unwrap();
    assert!(
        ir.contains("call double @js_class_method_bind_by_id"),
        "static-name class method value reads should use method-id bind ABI:\n{ir}"
    );
    assert!(
        !ir.contains("call double @js_class_method_bind(double"),
        "static-name class method value reads should not pass raw name bytes:\n{ir}"
    );
}

#[test]
fn raw_numeric_class_field_rejects_unknown_or_dynamic_shape_receiver() {
    let dynamic_receiver_module = module_with_classes_and_params(
        "artifact_raw_numeric_class_field_unknown_receiver.ts",
        vec![class(102, "Point", vec![class_field("x", Type::Number)])],
        vec![param(1, "p", Type::Any)],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(1)),
                property: "x".to_string(),
                value: Box::new(number(7.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "x".to_string(),
            })),
        ],
    );
    let computed_shape_module = module_with_classes_and_params(
        "artifact_raw_numeric_class_field_computed_shape.ts",
        vec![class_with_computed_member(
            103,
            "Point",
            vec![class_field("x", Type::Number)],
        )],
        vec![param(1, "p", Type::Named("Point".to_string()))],
        Type::Number,
        vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(local(1)),
                property: "x".to_string(),
                value: Box::new(number(7.0)),
            }),
            Stmt::Return(Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(local(1)),
                property: "x".to_string(),
            })),
        ],
    );

    for module in [dynamic_receiver_module, computed_shape_module] {
        let artifact = compile_artifact_json_for_module(module);
        let records = artifact["records"].as_array().unwrap();
        assert!(
            !records.iter().any(|record| {
                record["source_function"] == "probe"
                    && (record["consumer"] == "class_field_set.raw_f64_store"
                        || record["consumer"] == "class_field_get.raw_f64_load"
                        || record["consumer"] == "class_field_get.raw_f64_number_context"
                        || record["consumer"] == "write_barrier.elided_raw_f64_class_field")
            }),
            "unknown/dynamic-shape receivers must not claim raw class-field access or pointer-free barrier elision:\n{artifact:#}"
        );
    }
}

/// #6299: a numeric array that arrives as a call's return value must still get
/// the guarded numeric array-index fast path.
///
/// `arr[i] = arr[i] + 1` only lowers to `Expr::IndexSet` when the receiver is a
/// statically-known local array; for a call-returned array the lowerer emits the
/// spec-compliant `Expr::PutValueSet` instead. `collect_index_used_locals` had no
/// arm for that variant (it fell into a `_ => {}` catch-all), so the loop counter
/// never joined `index_used_locals`, lost its i32 shadow slot, and every `arr[i]`
/// in the loop fell back from `js_typed_feedback_numeric_array_index_{get,set}_guard`
/// to the generic `js_array_get_index_or_string` path — a 6.8x cliff.
///
/// Asserting on the emitted helper (rather than wall-clock time) pins the actual
/// codegen decision and stays meaningful under any optimization level.
#[test]
fn put_value_set_index_keeps_the_numeric_array_fast_path() {
    // for (let i = 0; i < arr.length; i++) arr[i] = arr[i] + 1;
    // with `arr: number[]` written through the generic PutValue form.
    let arr = 1u32;
    let i = 2u32;
    let read_arr_i = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(arr)),
        index: Box::new(Expr::LocalGet(i)),
    };
    let body = vec![
        Stmt::Let {
            id: arr,
            name: "arr".to_string(),
            ty: Type::Array(Box::new(Type::Number)),
            mutable: false,
            init: Some(Expr::Array(vec![])),
        },
        // The array escapes into a call — this is what `const arr = build()`
        // lowers to (the callee fills the array through a return slot). It is
        // load-bearing for the repro: an array that stays local is still
        // hoistable, so `stmt/loops.rs` hands the counter an i32 slot from the
        // length-hoist path and the fast path survives even with the collector
        // blind spot. Once the array escapes, that path bails and the counter's
        // only route to an i32 slot is `index_used_locals` — the set this fix
        // repairs.
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(1)),
            args: vec![Expr::LocalGet(arr)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
        Stmt::For {
            init: Some(Box::new(Stmt::Let {
                id: i,
                name: "i".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(0)),
            })),
            condition: Some(Expr::Compare {
                op: CompareOp::Lt,
                left: Box::new(Expr::LocalGet(i)),
                right: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(arr)),
                    property: "length".to_string(),
                }),
            }),
            update: Some(Expr::Update {
                id: i,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            body: vec![Stmt::Expr(Expr::PutValueSet {
                target: Box::new(Expr::LocalGet(arr)),
                key: Box::new(Expr::LocalGet(i)),
                value: Box::new(Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(read_arr_i),
                    right: Box::new(Expr::Integer(1)),
                }),
                receiver: Box::new(Expr::LocalGet(arr)),
                strict: false,
            })],
        },
    ];

    let ir = compile_ir("put_value_set_fast_path", body);

    // The loop counter must keep its i32 shadow slot: that is what lets the
    // guarded helpers take a native i32 index.
    assert!(
        ir.contains("alloca i32"),
        "the `arr[i]` loop counter lost its i32 shadow slot — `index_used_locals` \
         no longer sees the PutValueSet key (#6299):\n{ir}"
    );
    assert!(
        ir.contains("@js_typed_feedback_numeric_array_index_get_guard"),
        "`arr[i]` read through PutValueSet's value subtree must keep the guarded \
         numeric fast path, not fall back to js_array_get_index_or_string (#6299):\n{ir}"
    );
    assert!(
        ir.contains("@js_typed_feedback_numeric_array_index_set_guard"),
        "`arr[i] = ...` through PutValueSet must keep the guarded numeric store \
         fast path (#6299):\n{ir}"
    );
}

#[path = "native_proof_regressions/invalidation.rs"]
mod invalidation;

#[path = "native_proof_regressions/integer_modulo.rs"]
mod integer_modulo;

#[path = "native_proof_regressions/math_mul_fastpath.rs"]
mod math_mul_fastpath;
