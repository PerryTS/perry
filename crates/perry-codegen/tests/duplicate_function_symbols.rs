use perry_codegen::{compile_module, AppMetadata, CompileOptions, NamespaceEntryKind};
use perry_hir::{Class, Export, Expr, Function, Module, ModuleInitKind, Stmt};
use perry_types::Type;

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
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        namespace_reexport_named_imports: std::collections::HashSet::new(),
        imported_vars: std::collections::HashSet::new(),
        namespace_reexport_values: std::collections::HashMap::new(),
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

fn function(id: u32, name: &str) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(perry_hir::Expr::Number(id as f64)))],
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

fn module_with_duplicate_local_function_names() -> Module {
    Module {
        name: "duplicate_function_symbols.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![function(1, "iterator"), function(2, "iterator")],
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

fn class_with_static_and_instance_create() -> Class {
    Class {
        id: 1,
        name: "PackageJson".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        fields: Vec::new(),
        constructor: None,
        methods: vec![function(10, "create")],
        getters: Vec::new(),
        setters: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: vec![function(11, "create")],
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
    }
}

fn module_with_static_and_instance_method_name_collision() -> Module {
    let mut module = module_with_duplicate_local_function_names();
    module.functions = Vec::new();
    module.classes = vec![class_with_static_and_instance_create()];
    module
}

fn module_with_exported_value_alias_colliding_with_function() -> Module {
    let mut module = module_with_duplicate_local_function_names();
    module.functions = vec![function(1, "graphql")];
    module.init = vec![Stmt::Let {
        id: 10,
        name: "graphql2".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::Number(42.0)),
    }];
    module.exports = vec![Export::Named {
        local: "graphql2".to_string(),
        exported: "graphql".to_string(),
    }];
    module
}

fn module_with_exported_value_alias_colliding_with_local_value_getter() -> Module {
    let mut module = module_with_duplicate_local_function_names();
    module.functions = Vec::new();
    module.init = vec![
        Stmt::Let {
            id: 10,
            name: "s".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::Number(1.0)),
        },
        Stmt::Let {
            id: 11,
            name: "a".to_string(),
            ty: Type::Number,
            mutable: false,
            init: Some(Expr::Number(2.0)),
        },
    ];
    module.exported_objects = vec!["s".to_string(), "a".to_string()];
    module.exports = vec![
        Export::Named {
            local: "s".to_string(),
            exported: "a".to_string(),
        },
        Export::Named {
            local: "a".to_string(),
            exported: "b".to_string(),
        },
    ];
    module
}

fn module_with_native_namespace_reexport() -> Module {
    let mut module = module_with_duplicate_local_function_names();
    module.name = "NodeSocket.ts".to_string();
    module.functions = Vec::new();
    module.exports = vec![Export::NamespaceReExport {
        source: "ws".to_string(),
        name: "NodeWS".to_string(),
    }];
    module
}

#[test]
fn duplicate_local_function_names_get_unique_llvm_symbols() {
    let ir = String::from_utf8(
        compile_module(&module_with_duplicate_local_function_names(), empty_opts()).unwrap(),
    )
    .unwrap();

    assert!(!ir.contains("define double @perry_fn_duplicate_function_symbols_ts__iterator("));
    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__iterator__local_1(")
            .count(),
        1
    );
    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__iterator__local_2(")
            .count(),
        1
    );
    assert_eq!(
        ir.matches(
            "define double @__perry_wrap_perry_fn_duplicate_function_symbols_ts__iterator__local_"
        )
        .count(),
        2
    );
}

#[test]
fn static_method_name_does_not_clobber_instance_method_symbol() {
    let ir = String::from_utf8(
        compile_module(
            &module_with_static_and_instance_method_name_collision(),
            empty_opts(),
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        ir.matches(
            "define double @perry_method_duplicate_function_symbols_ts__PackageJson__create("
        )
        .count(),
        1
    );
    assert_eq!(
        ir.matches(
            "define double @perry_static_duplicate_function_symbols_ts__PackageJson__create("
        )
        .count(),
        1
    );
}

#[test]
fn exported_value_alias_does_not_clobber_local_function_symbol() {
    let ir = String::from_utf8(
        compile_module(
            &module_with_exported_value_alias_colliding_with_function(),
            empty_opts(),
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__graphql()")
            .count(),
        1
    );
    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__graphql__local_1(")
            .count(),
        1
    );
}

#[test]
fn exported_value_alias_does_not_clobber_local_value_getter_symbol() {
    let ir = String::from_utf8(
        compile_module(
            &module_with_exported_value_alias_colliding_with_local_value_getter(),
            empty_opts(),
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__s()")
            .count(),
        1
    );
    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__a()")
            .count(),
        1
    );
    assert_eq!(
        ir.matches("define double @perry_fn_duplicate_function_symbols_ts__b()")
            .count(),
        1
    );
}

#[test]
fn native_namespace_reexport_emits_static_value_getter() {
    let mut opts = empty_opts();
    opts.namespace_reexport_values.insert(
        "NodeWS".to_string(),
        NamespaceEntryKind::NativeModuleNamespace {
            module_name: "ws".to_string(),
        },
    );
    let ir =
        String::from_utf8(compile_module(&module_with_native_namespace_reexport(), opts).unwrap())
            .unwrap();

    assert_eq!(
        ir.matches("define double @perry_fn_NodeSocket_ts__NodeWS()")
            .count(),
        1
    );
    assert!(ir.contains("js_create_native_module_namespace"));
    assert!(!ir.contains("declare double @perry_fn_NodeSocket_ts__NodeWS()"));
}

#[test]
fn export_all_barrel_forwards_namespace_reexport_getter() {
    let mut opts = empty_opts();
    opts.namespace_reexport_values.insert(
        "NodeWS".to_string(),
        NamespaceEntryKind::ForeignVar {
            source_prefix: "platform_node_shared_src_NodeSocket_ts".to_string(),
            source_local: "NodeWS".to_string(),
        },
    );
    let mut module = module_with_native_namespace_reexport();
    module.name = "platform-node/src/NodeSocket.ts".to_string();
    module.exports = Vec::new();

    let ir = String::from_utf8(compile_module(&module, opts).unwrap()).unwrap();

    assert_eq!(
        ir.matches("define double @perry_fn_platform_node_src_NodeSocket_ts__NodeWS()")
            .count(),
        1
    );
    assert!(ir.contains("call double @perry_fn_platform_node_shared_src_NodeSocket_ts__NodeWS()"));
}
