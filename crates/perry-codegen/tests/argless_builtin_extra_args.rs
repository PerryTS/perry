//! Regression: argless builtin methods (`String.trim`, `String.toLowerCase`,
//! `Array.pop`, …) must accept and ignore extra arguments rather than bailing.
//!
//! JS ignores surplus args to argless methods (`"  x ".trim(1)` is legal and
//! returns the trimmed string), so codegen must lower these without erroring.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Module, ModuleInitKind, Stmt};

fn empty_opts() -> CompileOptions {
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

fn module_with_init(init: Vec<Stmt>) -> Module {
    Module {
        name: "argless_extra_args.ts".to_string(),
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
        init,
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

/// `"  x  ".trim(1)` — String.trim is argless but JS ignores the extra arg.
/// Codegen must lower this without bailing (`String.trim takes no args`).
#[test]
fn string_trim_with_extra_arg_compiles() {
    let stmt = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::String("  x  ".to_string())),
            property: "trim".to_string(),
        }),
        args: vec![Expr::Number(1.0)],
        type_args: Vec::new(),
        byte_offset: 0,
    });
    let ir = compile_module(&module_with_init(vec![stmt]), empty_opts())
        .expect("\"  x  \".trim(1) must compile (extra arg ignored, not an error)");
    let ir = String::from_utf8(ir).unwrap();
    // The runtime trim helper must still be emitted — the arg is dropped, not
    // routed into a different code path.
    assert!(
        ir.contains("js_string_trim"),
        "expected trim lowering to emit js_string_trim"
    );
}

/// `[1].pop(99)` — Array.pop is argless; the extra arg must be ignored.
#[test]
fn array_pop_with_extra_arg_compiles() {
    let stmt = Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::Array(vec![Expr::Number(1.0)])),
            property: "pop".to_string(),
        }),
        args: vec![Expr::Number(99.0)],
        type_args: Vec::new(),
        byte_offset: 0,
    });
    compile_module(&module_with_init(vec![stmt]), empty_opts())
        .expect("[1].pop(99) must compile (extra arg ignored, not an error)");
}
