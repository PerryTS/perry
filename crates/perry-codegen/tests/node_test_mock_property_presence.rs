//! `node:test` property mocks preserve omitted-vs-explicit-undefined at the
//! native ABI boundary.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
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
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
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
    Module {
        name: "node_test_mock_property_presence.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: Vec::new(),
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        init: vec![
            mock_property_call(vec![target(), Expr::String("value".to_string())]),
            mock_property_call(vec![
                target(),
                Expr::String("value".to_string()),
                Expr::Undefined,
            ]),
        ],
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
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
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
        calls[0].contains("i32 0"),
        "omitted value must carry presence=false:\n{}",
        calls[0]
    );
    assert!(
        calls[1].contains("i32 1"),
        "explicit undefined must carry presence=true:\n{}",
        calls[1]
    );
}
