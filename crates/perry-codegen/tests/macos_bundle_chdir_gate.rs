//! #4856 — `perry_macos_bundle_chdir` must only be emitted into `main` for
//! macOS targets. The runtime fn is a documented no-op everywhere else, and
//! referencing it unconditionally made every iOS/tvOS executable link depend
//! on the Apple cross runtime archive carrying a macOS-only symbol — a stale
//! cross runtime then failed the link with
//! `undefined symbol: _perry_macos_bundle_chdir`.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Module, ModuleInitKind};

fn entry_opts(target: Option<&str>) -> CompileOptions {
    CompileOptions {
        target: target.map(str::to_string),
        is_entry_module: true,
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

fn empty_entry_module() -> Module {
    Module {
        name: "chdir_gate.ts".to_string(),
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
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

fn ir_for_target(target: &str) -> String {
    String::from_utf8(compile_module(&empty_entry_module(), entry_opts(Some(target))).unwrap())
        .unwrap()
}

const CHDIR_CALL: &str = "call void @perry_macos_bundle_chdir()";

#[test]
fn macos_targets_emit_bundle_chdir_in_main() {
    for triple in [
        "arm64-apple-macosx15.0.0",
        "x86_64-apple-macosx15.0.0",
        "aarch64-apple-darwin",
    ] {
        let ir = ir_for_target(triple);
        assert!(
            ir.contains(CHDIR_CALL),
            "expected {} in main for {}",
            CHDIR_CALL,
            triple
        );
    }
}

#[test]
fn non_macos_targets_do_not_reference_bundle_chdir() {
    for triple in [
        "aarch64-apple-ios",
        "arm64-apple-ios17.0-simulator",
        "aarch64-apple-tvos",
        "arm64-apple-tvos17.0-simulator",
        "arm64-apple-xros1.0",
        "arm64_32-apple-watchos",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-musl",
        "aarch64-unknown-linux-android",
        "x86_64-pc-windows-msvc",
    ] {
        let ir = ir_for_target(triple);
        assert!(
            !ir.contains(CHDIR_CALL),
            "{} must not call perry_macos_bundle_chdir from main (#4856)",
            triple
        );
    }
}
