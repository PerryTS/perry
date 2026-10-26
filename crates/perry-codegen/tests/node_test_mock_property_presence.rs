//! `node:test` property mocks preserve omitted-vs-explicit-undefined at the
//! native ABI boundary.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_layout_ordinals: Default::default(),
        program_class_shape_ids: Default::default(),
        program_facts: Default::default(),
        program_layouts: Default::default(),
        gc_effects: Default::default(),
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_source_prefixes: Default::default(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        cjs_interop_namespaces: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: Default::default(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        program_has_worker: false,
        program_has_thread_agents: false,
        needs_stdlib: false,
        program_is_synchronous: false,
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
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        namespace_adopts_cjs_exports: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn mock_property_call(args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "test".to_string(),
        class_name: None,
        object: None,
        method: "property".to_string(),
        args,
    })
}

fn fixture_module() -> Module {
    let target = || Expr::Object(vec![("value".to_string(), Expr::Integer(5))]);
    let mut module = Module::new("node_test_mock_property_presence.ts");
    module.init = vec![
        mock_property_call(vec![target(), Expr::String("value".to_string())]),
        mock_property_call(vec![
            target(),
            Expr::String("value".to_string()),
            Expr::Undefined,
        ]),
    ];
    module
}

#[test]
fn property_mock_abi_carries_source_argument_presence() {
    let ir = String::from_utf8(compile_module(&fixture_module(), ir_opts()).unwrap()).unwrap();
    let calls = ir
        .lines()
        .filter(|line| line.contains("call double @js_node_test_mock_property_with_presence"))
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2, "expected two property-mock calls:\n{ir}");
    assert!(
        calls[0].contains("i32 0)"),
        "omitted value must carry presence=false:\n{}",
        calls[0]
    );
    assert!(
        calls[1].contains("i32 1)"),
        "explicit undefined must carry presence=true:\n{}",
        calls[1]
    );
}
