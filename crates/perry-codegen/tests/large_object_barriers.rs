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

fn compile_ir(module: &Module, opts: CompileOptions) -> String {
    String::from_utf8(compile_module(module, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

fn entry_opts() -> CompileOptions {
    CompileOptions {
        is_entry_module: true,
        ..empty_opts()
    }
}

fn assert_default_barrier_env_not_disabled() {
    assert!(
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        ),
        "default barrier emission tests require PERRY_WRITE_BARRIERS unset or enabled"
    );
}

fn assert_runtime_barrier_metadata_emitted(ir: &str) {
    assert!(
        ir.contains("call void @js_gc_write_barriers_emitted(i32 1)"),
        "barrier-enabled modules must notify the runtime that generated store barriers exist"
    );
}

fn module_with_large_pointer_array_literal(element_count: usize) -> Module {
    Module {
        name: "large_object_barriers.ts".to_string(),
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
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 1,
                    name: "child".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Array(Vec::new())),
                },
                Stmt::Return(Some(Expr::Array(vec![Expr::LocalGet(1); element_count]))),
            ],
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

fn module_with_large_local_array_push(element_count: usize) -> Module {
    Module {
        name: "large_object_push_barriers.ts".to_string(),
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
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 1,
                    name: "child".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Object(Vec::new())),
                },
                Stmt::Let {
                    id: 2,
                    name: "arr".to_string(),
                    ty: Type::Array(Box::new(Type::Any)),
                    mutable: true,
                    init: Some(Expr::Array(vec![Expr::Number(0.0); element_count])),
                },
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 2,
                    value: Box::new(Expr::LocalGet(1)),
                }),
                Stmt::Return(Some(Expr::LocalGet(2))),
            ],
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

#[test]
fn large_array_literal_direct_stores_emit_precise_slot_barriers() {
    const LARGE_LITERAL_ELEMENTS: usize = 2050;
    assert_default_barrier_env_not_disabled();

    let ir = compile_ir(
        &module_with_large_pointer_array_literal(LARGE_LITERAL_ELEMENTS),
        empty_opts(),
    );
    assert_runtime_barrier_metadata_emitted(&ir);

    let alloc_marker = format!(
        "call i64 @js_array_alloc_literal(i32 {})",
        LARGE_LITERAL_ELEMENTS
    );
    let alloc_pos = ir
        .find(&alloc_marker)
        .expect("large literal should use js_array_alloc_literal");
    let literal_ir = &ir[alloc_pos..];
    let store_pos = literal_ir
        .find("store double")
        .expect("large literal should emit direct element stores");
    let layout_pos = literal_ir
        .find("call void @js_gc_note_slot_layout")
        .expect("large literal should keep slot layout notes");
    let barrier_pos = literal_ir
        .find("call void @js_write_barrier_slot")
        .expect("large literal stores must remember old-born parent slots");

    assert!(store_pos < layout_pos);
    assert!(layout_pos < barrier_pos);
    assert!(
        literal_ir
            .matches("call void @js_write_barrier_slot")
            .count()
            >= LARGE_LITERAL_ELEMENTS,
        "every direct literal store needs a slot barrier"
    );
}

#[test]
fn large_local_array_push_inbounds_store_emits_precise_slot_barrier() {
    const LARGE_LITERAL_ELEMENTS: usize = 2050;
    assert_default_barrier_env_not_disabled();

    let ir = compile_ir(
        &module_with_large_local_array_push(LARGE_LITERAL_ELEMENTS),
        empty_opts(),
    );
    assert_runtime_barrier_metadata_emitted(&ir);

    let alloc_marker = format!(
        "call i64 @js_array_alloc_literal(i32 {})",
        LARGE_LITERAL_ELEMENTS
    );
    assert!(
        ir.contains(&alloc_marker),
        "fixture should allocate a large local array outside the inline small-literal path"
    );

    let inbounds_pos = ir
        .find("\napush.inbounds.")
        .expect("optimized local push should emit an in-bounds fast block");
    let push_ir = &ir[inbounds_pos + 1..];
    let inbounds_end = push_ir
        .find("\napush.realloc.")
        .expect("in-bounds push block should precede the realloc block");
    let inbounds_ir = &push_ir[..inbounds_end];
    let store_pos = inbounds_ir
        .find("store double")
        .expect("optimized push should emit a direct element store");
    let layout_pos = inbounds_ir
        .find("call void @js_gc_note_slot_layout")
        .expect("optimized push store should keep slot layout notes");
    let barrier_pos = inbounds_ir
        .find("call void @js_write_barrier_slot")
        .expect("optimized push direct store must remember old-born parent slots");

    assert!(store_pos < layout_pos);
    assert!(layout_pos < barrier_pos);
}

#[test]
fn default_write_barriers_emit_runtime_metadata_for_entry_and_module_init() {
    assert_default_barrier_env_not_disabled();

    let module = module_with_large_pointer_array_literal(1);
    let module_init_ir = compile_ir(&module, empty_opts());
    assert_runtime_barrier_metadata_emitted(&module_init_ir);

    let entry_ir = compile_ir(&module, entry_opts());
    assert_runtime_barrier_metadata_emitted(&entry_ir);
}
