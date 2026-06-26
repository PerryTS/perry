use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};
use perry_types::{FunctionType, ObjectType, PropertyInfo, Type};

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

fn assert_typed_array_get_fallback_reason(artifact: &serde_json::Value, reason: &str) {
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["consumer"] == "TypedArrayGet.slow_path"
                && record["access_mode"] == "dynamic_fallback"
                && record["materialization_reason"] == reason
                && record["fallback_reason"] == reason
        }),
        "expected typed-array get fallback reason {reason}:\n{artifact:#}"
    );
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet" && record["access_mode"] == "unchecked_native"
        }),
        "final typed-array get must not use unchecked native access:\n{artifact:#}"
    );
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

fn native_arena_owner_alias_let(id: u32, name: &str, owner_id: u32, mutable: bool) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Any,
        mutable,
        init: Some(local(owner_id)),
    }
}

fn runtime_condition(cond_id: u32) -> Expr {
    Expr::Compare {
        op: CompareOp::Ne,
        left: Box::new(local(cond_id)),
        right: Box::new(int(0)),
    }
}

fn closure_type(return_type: Type) -> Type {
    Type::Function(FunctionType {
        params: Vec::new(),
        return_type: Box::new(return_type),
        is_async: false,
        is_generator: false,
    })
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

fn extern_func_ref(name: &str, return_type: Type) -> Expr {
    Expr::ExternFuncRef {
        name: name.to_string(),
        param_types: Vec::new(),
        return_type,
    }
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
            byte_offset: 0,
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
            byte_offset: 0,
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
fn artifact_records_width_aware_buffer_numeric_read_facts() {
    let reads = [
        ("readUInt16BE", 2, "big", false, false, "u32"),
        ("readUInt16LE", 2, "little", false, false, "u32"),
        ("readUInt32BE", 4, "big", false, false, "u32"),
        ("readUInt32LE", 4, "little", false, false, "u32"),
        ("readInt16BE", 2, "big", true, false, "i32"),
        ("readInt16LE", 2, "little", true, false, "i32"),
        ("readInt32BE", 4, "big", true, false, "i32"),
        ("readInt32LE", 4, "little", true, false, "i32"),
        ("readFloatBE", 4, "big", false, true, "f32"),
        ("readFloatLE", 4, "little", false, true, "f32"),
        ("readDoubleBE", 8, "big", false, true, "f64"),
        ("readDoubleLE", 8, "little", false, true, "f64"),
    ];
    let mut body = vec![buffer_let(1, "buf", int(16))];
    for (method, ..) in reads {
        body.push(Stmt::Expr(buffer_read(1, method, int(0))));
    }
    body.push(Stmt::Return(Some(int(0))));

    let artifact = compile_artifact_json("artifact_width_aware_buffer_reads.ts", body);
    let records = artifact["records"].as_array().unwrap();
    for (method, width, endian, signed, floating, native_rep) in reads {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "BufferNumericRead"
                    && record["native_rep_name"] == native_rep
                    && record["buffer_access"]["access_width_bytes"] == width
                    && record["buffer_access"]["bounds_width_units"] == width
                    && record["buffer_access"]["index_unit"] == "byte"
                    && record["buffer_access"]["element_width_bytes"] == 1
                    && record["buffer_access"]["endian"] == endian
                    && record["buffer_access"]["signed"] == signed
                    && record["buffer_access"]["floating"] == floating
                    && record["notes"].as_array().is_some_and(|notes| {
                        notes.iter().any(|note| {
                            note.as_str()
                                .is_some_and(|note| note == format!("method={method}"))
                        })
                    })
            }),
            "expected structured buffer_access facts for {method}:\n{artifact:#}"
        );
    }
}

#[test]
fn loop_bound_by_buffer_length_only_does_not_prove_wide_read() {
    let body = vec![
        buffer_let(1, "buf", int(16)),
        for_loop(
            2,
            length(1),
            vec![Stmt::Expr(buffer_read(1, "readUInt32BE", local(2)))],
        ),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_wide_read_needs_width_guard.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_u32"
                && record["buffer_access"]["access_width_bytes"] == 4
        }),
        "i < buf.length must not prove a 4-byte Buffer read:\n{artifact:#}"
    );
}

#[test]
fn explicit_width_guard_proves_wide_buffer_read() {
    let body = vec![
        buffer_let(1, "buf", int(16)),
        Stmt::For {
            init: Some(Box::new(number_let(2, "i", true, int(0)))),
            condition: Some(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(add(local(2), int(4))),
                right: Box::new(length(1)),
            }),
            update: Some(increment(2)),
            body: vec![Stmt::Expr(buffer_read(1, "readUInt32BE", local(2)))],
        },
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_width_guard_buffer_read.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "BufferNumericRead"
                && record["consumer"] == "BufferNumericRead.native_u32"
                && record["bounds_state"]["proven"]["proof"] == "loop_guard"
                && record["buffer_access"]["access_width_bytes"] == 4
                && record["buffer_access"]["bounds_width_units"] == 4
        }),
        "expected i + 4 <= buf.length to prove a 4-byte native read:\n{artifact:#}"
    );
}

#[test]
fn proven_buffer_and_typed_array_reads_are_numeric_operands() {
    let body = vec![
        buffer_let(1, "buf", int(16)),
        number_let(2, "sum", true, int(0)),
        Stmt::For {
            init: Some(Box::new(number_let(3, "i", true, int(0)))),
            condition: Some(Expr::Compare {
                op: CompareOp::Le,
                left: Box::new(add(local(3), int(4))),
                right: Box::new(length(1)),
            }),
            update: Some(increment(3)),
            body: vec![Stmt::Expr(Expr::LocalSet(
                2,
                Box::new(add(local(2), buffer_read(1, "readUInt32BE", local(3)))),
            ))],
        },
        typed_array_let(
            4,
            "u16",
            "Uint16Array",
            perry_hir::TYPED_ARRAY_KIND_UINT16,
            int(8),
        ),
        for_loop(
            5,
            int(8),
            vec![Stmt::Expr(Expr::LocalSet(
                2,
                Box::new(add(local(2), index_get(4, local(5)))),
            ))],
        ),
        Stmt::Return(Some(local(2))),
    ];

    let ir = compile_ir("numeric_buffer_typed_array_operands.ts", body);
    assert!(
        !ir.contains("call double @js_number_coerce"),
        "proven native numeric reads should not force JS number coercion:\n{ir}"
    );
    assert!(
        ir.contains("load i32, ptr") && ir.contains("align 1"),
        "byte-offset Buffer numeric reads must use unaligned-safe loads:\n{ir}"
    );
}

#[test]
fn artifact_records_tracked_typed_array_native_reads() {
    let arrays = [
        (
            1,
            "u16",
            "Uint16Array",
            perry_hir::TYPED_ARRAY_KIND_UINT16,
            "u32",
            2,
            false,
            false,
        ),
        (
            3,
            "i32s",
            "Int32Array",
            perry_hir::TYPED_ARRAY_KIND_INT32,
            "i32",
            4,
            true,
            false,
        ),
        (
            5,
            "u32s",
            "Uint32Array",
            perry_hir::TYPED_ARRAY_KIND_UINT32,
            "u32",
            4,
            false,
            false,
        ),
        (
            7,
            "f32s",
            "Float32Array",
            perry_hir::TYPED_ARRAY_KIND_FLOAT32,
            "f32",
            4,
            false,
            true,
        ),
        (
            9,
            "f64s",
            "Float64Array",
            perry_hir::TYPED_ARRAY_KIND_FLOAT64,
            "f64",
            8,
            false,
            true,
        ),
    ];
    let mut body = Vec::new();
    for (array_id, name, class_name, kind, ..) in arrays {
        body.push(typed_array_let(array_id, name, class_name, kind, int(8)));
        body.push(for_loop(
            array_id + 1,
            int(8),
            vec![Stmt::Expr(index_get(array_id, local(array_id + 1)))],
        ));
    }
    body.push(Stmt::Return(Some(int(0))));

    let artifact = compile_artifact_json("artifact_tracked_typed_array_reads.ts", body);
    let records = artifact["records"].as_array().unwrap();
    for (_, name, _, _, native_rep, width, signed, floating) in arrays {
        assert!(
            records.iter().any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["native_rep_name"] == native_rep
                    && record["access_mode"] == "unchecked_native"
                    && record["buffer_access"]["index_unit"] == "element"
                    && record["buffer_access"]["access_width_bytes"] == width
                    && record["buffer_access"]["element_width_bytes"] == width
                    && record["buffer_access"]["bounds_width_units"] == 1
                    && record["buffer_access"]["signed"] == signed
                    && record["buffer_access"]["floating"] == floating
            }),
            "expected native typed-array read record for {name}:\n{artifact:#}"
        );
    }
    assert!(
        records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["materialization_reason"] == "runtime_api"
        }) && !records.iter().any(|record| {
            record["consumer"] == "materialize_js_value"
                && record["materialization_reason"] == "unknown_bounds"
        }),
        "proven typed-array loads should materialize at a JSValue boundary, not as an unknown-bounds hazard:\n{artifact:#}"
    );
}

#[test]
fn artifact_records_native_owned_typed_array_facts() {
    let body = vec![
        native_arena_owner_let(1, "owner", int(64), false),
        native_arena_view_let(
            2,
            "view",
            1,
            "Float64Array",
            perry_hir::TYPED_ARRAY_KIND_FLOAT64,
            int(0),
            int(8),
        ),
        for_loop(
            3,
            int(8),
            vec![array_set(2, local(3), add(local(3), number(0.5)))],
        ),
        for_loop(4, int(8), vec![Stmt::Expr(index_get(2, local(4)))]),
        Stmt::Return(Some(int(0))),
    ];

    let ir = compile_ir("native_owned_typed_array_fast_path.ts", body.clone());
    assert!(
        !ir.contains("call double @js_typed_array_get")
            && !ir.contains("call void @js_typed_array_set"),
        "native-owned typed-array hot loops should avoid typed-array runtime get/set:\n{ir}"
    );

    let artifact = compile_artifact_json("artifact_native_owned_typed_array.ts", body);
    let records = artifact["records"].as_array().unwrap();
    let native_record = records.iter().find(|record| {
        record["expr_kind"] == "TypedArrayGet"
            && record["consumer"] == "TypedArrayGet.native_f64"
            && record["access_mode"] == "unchecked_native"
            && !record["native_owned_view"].is_null()
    });
    let Some(record) = native_record else {
        panic!("expected native-owned f64 typed-array get record:\n{artifact:#}");
    };
    assert_eq!(record["native_owned_view"]["owner_local_id"], 1);
    assert_eq!(record["native_owned_view"]["owner_root_state"], "rooted");
    assert_eq!(record["native_owned_view"]["disposed_state"], "alive");
    assert_eq!(record["native_owned_view"]["byte_offset"], 0);
    assert_eq!(record["native_owned_view"]["byte_length"], 64);
    assert_eq!(record["native_owned_view"]["element_width_bytes"], 8);
    assert_eq!(record["native_owned_view"]["pointer_free_backing"], true);
    assert_eq!(record["alias_state"], "no_alias_proven");
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["consumer"] == "f64_store"
                && record["access_mode"] == "unchecked_native"
                && record["alias_state"] == "no_alias_proven"
                && !record["native_owned_view"].is_null()
        }),
        "expected native-owned f64 typed-array store record:\n{artifact:#}"
    );
    assert_eq!(artifact["summary"]["native_owned_view_count"], 4);
}

#[test]
fn native_owned_typed_array_owner_alias_dispose_invalidates_views() {
    let dispose_through_alias = compile_artifact_json(
        "artifact_native_owned_dispose_through_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, false),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(3)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        dispose_through_alias["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "use_after_dispose"
                    && record["fallback_reason"] == "use_after_dispose"
            }),
        "expected dispose-through-alias to invalidate the native-owned view:\n{dispose_through_alias:#}"
    );

    let view_through_alias = compile_artifact_json(
        "artifact_native_owned_view_through_alias_dispose_owner.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_owner_alias_let(3, "alias", 1, false),
            native_arena_view_let(
                2,
                "view",
                3,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(1)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        view_through_alias["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "use_after_dispose"
                    && record["fallback_reason"] == "use_after_dispose"
            }),
        "expected aliased owner view to share dispose invalidation with the owner:\n{view_through_alias:#}"
    );

    let reassigned_alias = compile_artifact_json(
        "artifact_native_owned_reassigned_alias_not_stale.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, true),
            Stmt::Expr(Expr::LocalSet(3, Box::new(int(0)))),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(3)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    let records = reassigned_alias["records"].as_array().unwrap();
    assert!(
        !records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["materialization_reason"] == "use_after_dispose"
        }),
        "reassigned owner alias should not keep a stale dispose link to the old owner:\n{reassigned_alias:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["consumer"] == "TypedArrayGet.native_f64"
                && record["access_mode"] == "unchecked_native"
        }),
        "live owner view should remain eligible for the unchecked native path:\n{reassigned_alias:#}"
    );
}

#[test]
fn native_owned_conditional_owner_alias_reassignment_invalidates_views() {
    let artifact = compile_artifact_json_for_module(module_with_classes_and_params(
        "artifact_native_owned_conditional_alias_reassignment.ts",
        Vec::new(),
        vec![param(10, "cond", Type::Number)],
        Type::Number,
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, true),
            native_arena_owner_let(4, "other", int(64), false),
            Stmt::If {
                condition: runtime_condition(10),
                then_branch: vec![Stmt::Expr(Expr::LocalSet(3, Box::new(local(4))))],
                else_branch: None,
            },
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(3)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    ));
    assert_typed_array_get_fallback_reason(&artifact, "use_after_dispose");
}

#[test]
fn native_owned_branch_local_owner_alias_removal_invalidates_views() {
    let artifact = compile_artifact_json_for_module(module_with_classes_and_params(
        "artifact_native_owned_branch_local_alias_removal.ts",
        Vec::new(),
        vec![param(10, "cond", Type::Number)],
        Type::Number,
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, true),
            Stmt::If {
                condition: runtime_condition(10),
                then_branch: vec![Stmt::Expr(Expr::LocalSet(3, Box::new(int(0))))],
                else_branch: None,
            },
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(3)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    ));
    assert_typed_array_get_fallback_reason(&artifact, "use_after_dispose");
}

#[test]
fn native_owned_unknown_call_escape_through_owner_alias_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_unknown_escape_through_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, false),
            Stmt::Expr(extern_call(
                "unknown_owner_escape",
                vec![local(3)],
                Type::Number,
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "missing_owner_root");
}

#[test]
fn native_owned_unknown_call_escape_inside_aggregate_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_unknown_escape_inside_aggregate.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(extern_call(
                "unknown_nested_escape",
                vec![Expr::Array(vec![local(2)])],
                Type::Number,
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_call_spread_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_call_spread_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::CallSpread {
                callee: Box::new(Expr::ExternFuncRef {
                    name: "unknown_spread_escape".to_string(),
                    param_types: Vec::new(),
                    return_type: Type::Number,
                }),
                args: vec![perry_hir::CallArg::Spread(Expr::Array(vec![local(2)]))],
                type_args: Vec::new(),
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_proxy_apply_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_proxy_apply_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::ProxyApply {
                proxy: Box::new(extern_func_ref("unknown_proxy_apply_escape", Type::Any)),
                args: vec![Expr::Array(vec![local(2)])],
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_proxy_construct_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_proxy_construct_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::ProxyConstruct {
                proxy: Box::new(extern_func_ref("unknown_proxy_construct_escape", Type::Any)),
                args: vec![Expr::Array(vec![local(2)])],
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_reflect_apply_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_reflect_apply_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::ReflectApply {
                func: Box::new(extern_func_ref("unknown_reflect_apply_escape", Type::Any)),
                this_arg: Box::new(Expr::Undefined),
                args: Box::new(Expr::Array(vec![local(2)])),
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_reflect_construct_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_reflect_construct_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::ReflectConstruct {
                target: Box::new(extern_func_ref(
                    "unknown_reflect_construct_escape",
                    Type::Any,
                )),
                args: Box::new(Expr::Array(vec![local(2)])),
                new_target: Box::new(Expr::Undefined),
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_js_call_value_escape_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_js_call_value_escape.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::JsCallValue {
                callee: Box::new(extern_func_ref("unknown_js_call_value_escape", Type::Any)),
                args: vec![Expr::Array(vec![local(2)])],
            }),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_static_method_v8_escape_invalidates_views() {
    let mut opts = empty_opts();
    opts.namespace_imports.push("RemoteNs".to_string());
    opts.namespace_v8_specifiers
        .insert("RemoteNs".to_string(), "remote:v8".to_string());
    let artifact = compile_artifact_json_for_module_with_opts(
        module(
            "artifact_native_owned_static_method_v8_escape.ts",
            vec![
                native_arena_owner_let(1, "owner", int(64), false),
                native_arena_view_let(
                    2,
                    "view",
                    1,
                    "Float64Array",
                    perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                    int(0),
                    int(8),
                ),
                Stmt::Expr(Expr::StaticMethodCall {
                    class_name: "RemoteNs".to_string(),
                    method_name: "invoke".to_string(),
                    args: vec![Expr::Array(vec![local(2)])],
                }),
                Stmt::Return(Some(index_get(2, int(0)))),
            ],
        ),
        opts,
    );
    assert_typed_array_get_fallback_reason(&artifact, "escaping_unowned_pointer");
}

#[test]
fn native_owned_closure_capture_through_owner_alias_invalidates_views() {
    let artifact = compile_artifact_json(
        "artifact_native_owned_closure_capture_through_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            native_arena_owner_alias_let(3, "alias", 1, false),
            Stmt::Let {
                id: 4,
                name: "f".to_string(),
                ty: closure_type(Type::Number),
                mutable: false,
                init: Some(Expr::Closure {
                    func_id: 90,
                    params: Vec::new(),
                    return_type: Type::Number,
                    body: vec![
                        Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(3)))),
                        Stmt::Return(Some(int(0))),
                    ],
                    captures: vec![3],
                    mutable_captures: Vec::new(),
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    is_strict: false,
                }),
            },
            Stmt::Expr(call(local(4), Vec::new())),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert_typed_array_get_fallback_reason(&artifact, "closure_capture");
}

#[test]
fn native_owned_uint8array_get_fallback_uses_uint8array_helper() {
    let ir = compile_ir(
        "native_owned_uint8array_get_disposed_fallback.ts",
        vec![
            native_arena_owner_let(1, "owner", int(16), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Uint8Array",
                perry_hir::TYPED_ARRAY_KIND_UINT8,
                int(0),
                int(16),
            ),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(1)))),
            Stmt::Return(Some(Expr::Uint8ArrayGet {
                array: Box::new(local(2)),
                index: Box::new(int(0)),
            })),
        ],
    );
    assert!(
        ir.contains("call i32 @js_uint8array_get"),
        "disposed native Uint8Array fallback should call js_uint8array_get:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_buffer_get"),
        "native Uint8Array fallback must not use BufferHeader layout:\n{ir}"
    );
}

#[test]
fn native_owned_uint8array_set_fallback_uses_uint8array_helper() {
    let ir = compile_ir(
        "native_owned_uint8array_set_disposed_fallback.ts",
        vec![
            native_arena_owner_let(1, "owner", int(16), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Uint8Array",
                perry_hir::TYPED_ARRAY_KIND_UINT8,
                int(0),
                int(16),
            ),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(1)))),
            Stmt::Expr(Expr::Uint8ArraySet {
                array: Box::new(local(2)),
                index: Box::new(int(0)),
                value: Box::new(int(7)),
            }),
            Stmt::Return(Some(int(0))),
        ],
    );
    assert!(
        ir.contains("call void @js_uint8array_set"),
        "disposed native Uint8Array fallback should call js_uint8array_set:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_buffer_set"),
        "native Uint8Array fallback must not use BufferHeader layout:\n{ir}"
    );
}

#[test]
fn uint8array_const_local_length_uses_inline_byte_get_set() {
    let ir = compile_ir(
        "uint8array_const_local_length_inline_byte_access.ts",
        vec![
            number_let(1, "size", false, int(16)),
            Stmt::Let {
                id: 2,
                name: "buf".to_string(),
                ty: Type::Named("Uint8Array".to_string()),
                mutable: false,
                init: Some(Expr::Uint8ArrayNew(Some(Box::new(local(1))))),
            },
            for_loop(
                3,
                local(1),
                vec![Stmt::Expr(Expr::Uint8ArraySet {
                    array: Box::new(local(2)),
                    index: Box::new(local(3)),
                    value: Box::new(local(3)),
                })],
            ),
            Stmt::Return(Some(Expr::Uint8ArrayGet {
                array: Box::new(local(2)),
                index: Box::new(int(0)),
            })),
        ],
    );
    assert!(
        ir.contains("store i8"),
        "bounded Uint8Array set should lower to an inline byte store:\n{ir}"
    );
    assert!(
        ir.contains("load i8"),
        "bounded Uint8Array get should lower to an inline byte load:\n{ir}"
    );
    assert!(
        !ir.contains("call void @js_uint8array_set"),
        "inline Uint8Array set should not call the runtime helper:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @js_uint8array_get"),
        "inline Uint8Array get should not call the runtime helper:\n{ir}"
    );
}

#[test]
fn native_owned_typed_array_fallback_reasons_are_explicit() {
    let disposed = compile_artifact_json(
        "artifact_native_owned_disposed.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::NativeArenaDispose(Box::new(local(1)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        disposed["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "use_after_dispose"
                    && record["fallback_reason"] == "use_after_dispose"
            }),
        "expected disposed native-owned view fallback reason:\n{disposed:#}"
    );

    let stale_length = compile_artifact_json(
        "artifact_native_owned_stale_length.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            number_let(3, "len", true, int(8)),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                local(3),
            ),
            Stmt::Expr(Expr::LocalSet(3, Box::new(int(4)))),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        stale_length["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "stale_view_length"
                    && record["fallback_reason"] == "stale_view_length"
            }),
        "expected stale native-owned view length fallback reason:\n{stale_length:#}"
    );

    let mutable_alias = compile_artifact_json(
        "artifact_native_owned_mutable_alias.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Let {
                id: 3,
                name: "alias".to_string(),
                ty: Type::Named("Float64Array".to_string()),
                mutable: false,
                init: Some(local(2)),
            },
            Stmt::Return(Some(index_get(3, int(0)))),
        ],
    );
    assert!(
        mutable_alias["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "mutable_alias"
                    && record["fallback_reason"] == "mutable_alias"
            }),
        "expected native-owned mutable alias fallback reason:\n{mutable_alias:#}"
    );

    let missing_owner = compile_artifact_json(
        "artifact_native_owned_missing_owner_root.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), true),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(Expr::LocalSet(
                1,
                Box::new(Expr::NativeArenaAlloc(Box::new(int(64)))),
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        missing_owner["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "missing_owner_root"
                    && record["fallback_reason"] == "missing_owner_root"
            }),
        "expected missing owner-root fallback reason:\n{missing_owner:#}"
    );

    let escaping = compile_artifact_json(
        "artifact_native_owned_escaping_pointer.ts",
        vec![
            native_arena_owner_let(1, "owner", int(64), false),
            native_arena_view_let(
                2,
                "view",
                1,
                "Float64Array",
                perry_hir::TYPED_ARRAY_KIND_FLOAT64,
                int(0),
                int(8),
            ),
            Stmt::Expr(extern_call(
                "escape_native_view",
                vec![local(2)],
                Type::Number,
            )),
            Stmt::Return(Some(index_get(2, int(0)))),
        ],
    );
    assert!(
        escaping["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && record["materialization_reason"] == "escaping_unowned_pointer"
                    && record["fallback_reason"] == "escaping_unowned_pointer"
            }),
        "expected escaping unowned pointer fallback reason:\n{escaping:#}"
    );
}

#[test]
fn uint8_clamped_typed_array_store_records_runtime_fallback() {
    let body = vec![
        typed_array_let(
            1,
            "clamped",
            "Uint8ClampedArray",
            perry_hir::TYPED_ARRAY_KIND_UINT8_CLAMPED,
            int(8),
        ),
        Stmt::Expr(Expr::IndexSet {
            object: Box::new(local(1)),
            index: Box::new(int(0)),
            value: Box::new(number(300.5)),
        }),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_uint8_clamped_store_fallback.ts", body);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArraySet"
                    && record["consumer"] == "TypedArraySet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && !record["fallback_reason"].is_null()
            }),
        "expected Uint8ClampedArray store to stay on runtime fallback:\n{artifact:#}"
    );
}

#[test]
fn typed_array_alias_read_records_runtime_fallback() {
    let body = vec![
        typed_array_let(
            1,
            "array",
            "Uint16Array",
            perry_hir::TYPED_ARRAY_KIND_UINT16,
            int(8),
        ),
        Stmt::Let {
            id: 2,
            name: "alias".to_string(),
            ty: Type::Named("Uint16Array".to_string()),
            mutable: false,
            init: Some(local(1)),
        },
        for_loop(3, int(8), vec![Stmt::Expr(index_get(2, local(3)))]),
        Stmt::Return(Some(int(0))),
    ];

    let artifact = compile_artifact_json("artifact_typed_array_alias_fallback.ts", body);
    assert!(
        artifact["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["expr_kind"] == "TypedArrayGet"
                    && record["consumer"] == "TypedArrayGet.slow_path"
                    && record["access_mode"] == "dynamic_fallback"
                    && !record["fallback_reason"].is_null()
            }),
        "expected aliased typed-array read to record runtime fallback:\n{artifact:#}"
    );
}

#[test]
fn reassigned_typed_array_store_records_runtime_fallback() {
    let body = vec![
        Stmt::Let {
            id: 1,
            name: "array".to_string(),
            ty: Type::Named("Int32Array".to_string()),
            mutable: true,
            init: Some(Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(int(8))),
            }),
        },
        Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::TypedArrayNew {
                kind: perry_hir::TYPED_ARRAY_KIND_INT32,
                arg: Some(Box::new(int(8))),
            }),
        )),
        array_set(1, int(0), int(42)),
        Stmt::Return(Some(index_get(1, int(0)))),
    ];

    let artifact = compile_artifact_json("artifact_typed_array_reassign_fallback.ts", body);
    let records = artifact["records"].as_array().unwrap();
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArraySet"
                && record["consumer"] == "TypedArraySet.slow_path"
                && record["access_mode"] == "dynamic_fallback"
                && !record["fallback_reason"].is_null()
        }),
        "expected reassigned typed-array store to record runtime fallback:\n{artifact:#}"
    );
    assert!(
        records.iter().any(|record| {
            record["expr_kind"] == "TypedArrayGet"
                && record["consumer"] == "TypedArrayGet.slow_path"
                && record["access_mode"] == "dynamic_fallback"
                && !record["fallback_reason"].is_null()
        }),
        "expected reassigned typed-array read to record runtime fallback:\n{artifact:#}"
    );
}
