use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Class, ClassField, Expr, Function, Module, ModuleInitKind, Stmt};
use perry_types::{ObjectType, Type};

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

fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

fn class(id: u32, name: &str, fields: Vec<ClassField>) -> Class {
    Class {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields,
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
    }
}

fn module_with_new(class: Class) -> Module {
    let class_name = class.name.clone();
    Module {
        name: "typed_shape_descriptor.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: vec![class],
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Named(class_name.clone()),
            body: vec![Stmt::Return(Some(Expr::New {
                class_name,
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
            }))],
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

fn compile_ir(module: &Module) -> String {
    String::from_utf8(compile_module(module, empty_opts()).unwrap()).unwrap()
}

#[test]
fn typed_class_emits_pointer_mask_and_descriptor_install() {
    let module = module_with_new(class(
        1,
        "TypedBox",
        vec![
            field("id", Type::Number),
            field("name", Type::String),
            field("child", Type::Named("Child".to_string())),
        ],
    ));

    let ir = compile_ir(&module);

    assert!(ir.contains("@perry_typed_shape_mask_"));
    assert!(ir.contains("@perry_typed_shape_raw_f64_mask_"));
    assert!(ir.contains("private unnamed_addr constant [1 x i64] [i64 1]"));
    assert!(ir.contains("private unnamed_addr constant [1 x i64] [i64 6]"));
    assert!(
        ir.contains("declare void @js_gc_init_typed_shape_layout(i64, i32, ptr, i32, ptr, i32)")
    );
    assert!(ir.contains("call void @js_gc_init_typed_shape_layout"));
}

#[test]
fn synthesized_closed_shape_emits_pointer_mask_and_descriptor_install() {
    let module = module_with_new(class(
        2,
        "__AnonShape_Test",
        vec![
            field("count", Type::Number),
            field("payload", Type::Object(ObjectType::default())),
        ],
    ));

    let ir = compile_ir(&module);

    assert!(ir.contains("@perry_typed_shape_mask_"));
    assert!(ir.contains("@perry_typed_shape_raw_f64_mask_"));
    assert!(ir.contains("private unnamed_addr constant [1 x i64] [i64 1]"));
    assert!(ir.contains("private unnamed_addr constant [1 x i64] [i64 2]"));
    assert!(ir.contains("call void @js_gc_init_typed_shape_layout"));
}
