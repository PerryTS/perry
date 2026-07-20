//! Regression test: `App({...})` launcher-style window config keys.
//!
//! `frameless`, `level`, `transparent`, `vibrancy`, and `activationPolicy`
//! were wired under the Cranelift backend (v0.4.11) but silently dropped in
//! the Phase K Cranelift→LLVM cutover — the appshell branch only forwarded
//! title/width/height/body/icon/windowState and lowered everything else for
//! side effects only (2026-07-16 docs audit finding). These tests pin the
//! restored wiring at the IR level: each present key must emit a call to its
//! `perry_ui_app_set_*` FFI setter, and omitted keys must emit nothing.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};
use perry_types::Type;

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
    Module {
        name: name.to_string(),
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

fn app_call(config: Vec<(&str, Expr)>) -> Stmt {
    Stmt::Expr(Expr::NativeMethodCall {
        module: "perry/ui".to_string(),
        class_name: None,
        object: None,
        method: "App".to_string(),
        args: vec![Expr::Object(
            config
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )],
    })
}

fn compile_ir(name: &str, body: Vec<Stmt>) -> String {
    String::from_utf8(compile_module(&module(name, body), empty_opts()).unwrap()).unwrap()
}

const WINDOW_OPTION_SETTERS: [&str; 5] = [
    "call void @perry_ui_app_set_frameless",
    "call void @perry_ui_app_set_level",
    "call void @perry_ui_app_set_transparent",
    "call void @perry_ui_app_set_vibrancy",
    "call void @perry_ui_app_set_activation_policy",
];

#[test]
fn app_config_window_options_emit_ffi_calls() {
    let ir = compile_ir(
        "app_window_opts_all",
        vec![app_call(vec![
            ("title", Expr::String("Launcher".to_string())),
            ("width", Expr::Number(600.0)),
            ("height", Expr::Number(80.0)),
            ("body", Expr::Number(0.0)),
            ("frameless", Expr::Bool(true)),
            ("level", Expr::String("floating".to_string())),
            ("transparent", Expr::Bool(true)),
            ("vibrancy", Expr::String("sidebar".to_string())),
            ("activationPolicy", Expr::String("accessory".to_string())),
        ])],
    );
    for setter in WINDOW_OPTION_SETTERS {
        assert!(
            ir.contains(setter),
            "App() config key must lower to `{setter}` — the key was silently \
             dropped (Cranelift→LLVM migration regression). IR:\n{ir}"
        );
    }
    // Window options must be applied before the body is attached so
    // vibrancy/frameless reconfigure the window ahead of Auto Layout.
    let vibrancy_at = ir.find("call void @perry_ui_app_set_vibrancy").unwrap();
    let body_at = ir.find("call void @perry_ui_app_set_body").unwrap();
    assert!(
        vibrancy_at < body_at,
        "perry_ui_app_set_vibrancy must be emitted before perry_ui_app_set_body"
    );
}

#[test]
fn app_config_without_window_options_emits_no_setter_calls() {
    let ir = compile_ir(
        "app_window_opts_none",
        vec![app_call(vec![
            ("title", Expr::String("Plain".to_string())),
            ("width", Expr::Number(1024.0)),
            ("height", Expr::Number(768.0)),
            ("body", Expr::Number(0.0)),
        ])],
    );
    for setter in WINDOW_OPTION_SETTERS {
        assert!(
            !ir.contains(setter),
            "omitted App() config key must not emit `{setter}`. IR:\n{ir}"
        );
    }
}
