use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Module, ModuleInitKind};

fn entry_opts(output_type: &str) -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: true,
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
        output_type: output_type.to_string(),
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
        fp_contract_mode: crate::FpContractMode::Off,
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

fn empty_module() -> Module {
    Module {
        name: "gc_exit_teardown.ts".to_string(),
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

fn emitted_ir(output_type: &str) -> String {
    String::from_utf8(compile_module(&empty_module(), entry_opts(output_type)).unwrap())
        .expect("LLVM IR should be UTF-8")
}

#[test]
fn executable_exit_releases_collection_side_allocations_last() {
    let ir = emitted_ir("executable");
    let exit_start = ir
        .find("\nevent_loop.exit.")
        .map(|offset| offset + 1)
        .unwrap_or_else(|| panic!("missing event-loop exit block in emitted IR:\n{ir}"));
    let exit_block = &ir[exit_start..];

    let finalization = exit_block
        .find("call void @js_process_run_finalization_exit()")
        .expect("exit finalization call should be emitted");
    let rejections = exit_block
        .find("call void @js_promise_report_unhandled_rejections()")
        .expect("unhandled-rejection report should be emitted");
    let release = exit_block
        .find("call void @js_gc_release_current_thread_collection_side_allocations()")
        .expect("collection side-allocation release should be emitted");
    // The exit code is now the process's pending exit code (#6671), so the
    // return operand is an SSA value (`ret i32 %N`), not the literal
    // `ret i32 0`. Match the return generically — this assertion only pins the
    // *ordering* (release before return), not the exit-code value.
    let ret = exit_block
        .find("ret i32 ")
        .expect("exit return should be emitted");

    assert!(finalization < rejections);
    assert!(rejections < release);
    assert!(release < ret);

    let host_start = ir
        .find("\nevent_loop.host_return.")
        .map(|offset| offset + 1)
        .expect("missing host-return block");
    let host_end = ir[host_start..]
        .find("\nevent_loop.body.")
        .map(|offset| host_start + offset)
        .expect("missing event-loop body block");
    assert!(
        !ir[host_start..host_end]
            .contains("js_gc_release_current_thread_collection_side_allocations"),
        "host-driven return must not run process-exit cleanup"
    );
}

#[test]
fn dylib_entry_does_not_release_process_owned_collection_storage() {
    let ir = emitted_ir("dylib");
    assert!(
        !ir.contains("call void @js_gc_release_current_thread_collection_side_allocations()"),
        "a library return is not a process-exit boundary"
    );
}
