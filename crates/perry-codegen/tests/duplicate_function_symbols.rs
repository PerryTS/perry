use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Function, Module, ModuleInitKind, Stmt};
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
