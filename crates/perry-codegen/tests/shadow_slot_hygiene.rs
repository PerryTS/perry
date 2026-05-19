use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};
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
        namespace_imports: Vec::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        namespace_reexport_named_imports: std::collections::HashSet::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        needs_js_runtime: false,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
    }
}

fn shadow_hygiene_module() -> Module {
    Module {
        name: "shadow_hygiene.ts".to_string(),
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
                    name: "dead".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::MapNew),
                },
                Stmt::Let {
                    id: 2,
                    name: "numeric".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Number(42.0)),
                },
                Stmt::Let {
                    id: 3,
                    name: "live".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Array(Vec::new())),
                },
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
            is_async: false,
            is_generator: false,
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
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
    }
}

#[test]
fn function_shadow_slots_clear_dead_values_and_skip_numeric_roots() {
    let ir = String::from_utf8(compile_module(&shadow_hygiene_module(), empty_opts()).unwrap())
        .expect("LLVM IR should be UTF-8");

    let dead_write = ir
        .find("call void @js_shadow_slot_set(i32 0, i64 %")
        .expect("dead array let should write its pointer to shadow slot 0");
    let dead_clear = ir[dead_write..]
        .find("call void @js_shadow_slot_set(i32 0, i64 0)")
        .map(|offset| dead_write + offset)
        .expect("dead shadow slot should be cleared after its last top-level statement");
    let live_alloc = ir[dead_clear..]
        .find("call i64 @js_array_alloc")
        .map(|offset| dead_clear + offset)
        .expect("later allocation should remain after dead slot clear");

    assert!(dead_write < dead_clear);
    assert!(dead_clear < live_alloc);
    assert!(
        !ir.contains("call void @js_shadow_slot_set(i32 1, i64 %"),
        "known numeric Any local must not be mirrored as a shadow root"
    );
    assert!(
        ir.contains("call void @js_shadow_slot_set(i32 1, i64 0)"),
        "numeric Any local's shadow slot should stay clear"
    );
}
