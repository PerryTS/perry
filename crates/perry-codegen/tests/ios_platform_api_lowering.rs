//! Regression coverage for the iOS-only `perry/ios` table (#5536).

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};

fn options(target: Option<&str>) -> CompileOptions {
    CompileOptions {
        target: target.map(str::to_string),
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        import_function_prefixes: Default::default(),
        import_function_ffi_aliases: Default::default(),
        import_function_origin_names: Default::default(),
        import_function_v8_specifiers: Default::default(),
        import_function_node_submodule: Default::default(),
        namespace_node_submodules: Default::default(),
        namespace_v8_specifiers: Default::default(),
        namespace_member_prefixes: Default::default(),
        namespace_member_origin_names: Default::default(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: Default::default(),
        type_aliases: Default::default(),
        imported_func_param_counts: Default::default(),
        imported_func_has_rest: Default::default(),
        imported_func_synthetic_arguments: Default::default(),
        imported_func_return_types: Default::default(),
        imported_vars: Default::default(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: true,
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
        dynamic_import_path_to_prefix: Default::default(),
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn call(method: &str, args: Vec<Expr>) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ios".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    })
}

fn module(body: Vec<Stmt>) -> Module {
    Module {
        name: "ios_platform_api_probe".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
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
        }],
        init_is_strict: false,
        init: Vec::new(),
        classic_for_lexical_bindings: std::collections::HashSet::new(),
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
        async_step_closures: Default::default(),
        closure_display_names: Default::default(),
        class_display_names: Default::default(),
        closure_source_text: Default::default(),
        async_generator_funcs: Default::default(),
        local_source_spans: Default::default(),
        gen_param_prologue_len: Default::default(),
    }
}

#[test]
fn ios_layout_and_foundation_model_calls_emit_runtime_symbols() {
    let hir = module(vec![
        call("getLayoutEnvironment", vec![]),
        call("onLayoutChange", vec![Expr::Number(0.0)]),
        call("offLayoutChange", vec![Expr::Number(1.0)]),
        call("foundationModelAvailability", vec![]),
        // The optional instructions argument must pad to an empty runtime string.
        call("createLanguageModelSession", vec![]),
        call(
            "respond",
            vec![Expr::Number(1.0), Expr::String("Hello".to_string())],
        ),
        call("destroyLanguageModelSession", vec![Expr::Number(1.0)]),
    ]);
    let ir =
        String::from_utf8(compile_module(&hir, options(Some("aarch64-apple-ios17.0"))).unwrap())
            .unwrap();

    for symbol in [
        "@perry_ios_get_layout_environment",
        "@perry_ios_on_layout_change",
        "@perry_ios_off_layout_change",
        "@perry_ios_foundation_model_availability",
        "@perry_ios_foundation_model_session_create",
        "@perry_ios_foundation_model_respond",
        "@perry_ios_foundation_model_session_destroy",
    ] {
        assert!(ir.contains(symbol), "missing {symbol} in IR:\n{ir}");
    }
}

#[test]
fn ios_module_is_rejected_for_non_ios_targets() {
    let error = compile_module(
        &module(vec![call("getLayoutEnvironment", vec![])]),
        options(Some("aarch64-apple-darwin")),
    )
    .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("perry/ios is only available"),
        "unexpected diagnostic: {error}"
    );
}
