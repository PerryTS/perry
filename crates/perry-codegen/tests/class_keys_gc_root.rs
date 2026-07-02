//! Regression test for #5042: the per-class keys-array global
//! (`@perry_class_keys_<module>__<class>`) must be registered as a GC root
//! at module init.
//!
//! The global holds a *raw* I64 pointer to the keys array, which is built
//! once at module init into the longlived (old-gen) arena and read by the
//! inline allocator at every `new ClassName()` site. The array is anchored
//! by the shape-cache scanner (so it stays live) but is non-pinned, so
//! old-page defrag (C4b) can relocate it. Every other codegen-emitted
//! global that holds a movable pointer (string handles, module-var data
//! tables) is registered via `js_gc_register_global_root` so the evacuation
//! rewrite pass fixes it up after a move; before #5042 the class-keys global
//! was not, leaving it dangling after a relocation — a later `new
//! ClassName()` then built an instance over a forwarded/freed keys array.
//!
//! A faithful end-to-end *corruption* repro is impractical on a clean tree:
//! the keys array is allocated at module init and lands on a permanently
//! live page that old-page defrag never selects (zero reclaimable bytes),
//! so it does not move in ordinary workloads. This test therefore asserts
//! the codegen contract directly on the emitted LLVM IR (in-process, via
//! `compile_module` with `emit_ir_only`): the module-init function takes the
//! address of the class-keys global and hands it to
//! `js_gc_register_global_root`.

use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Class, ClassField, Module, ModuleInitKind};
use perry_types::Type;

fn entry_opts() -> CompileOptions {
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
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn field(name: &str) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty: Type::Number,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

/// A class with statically declared fields => a non-empty keys array, which
/// is addressed by the `@perry_class_keys_*` codegen global.
fn module_with_declared_field_class() -> Module {
    Module {
        name: "class_keys_gc_root.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: vec![Class {
            id: 1,
            name: "Point".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: vec![field("x"), field("y"), field("z")],
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
            is_nested: false,
            aliases: Vec::new(),
        }],
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

#[test]
fn class_keys_global_is_registered_as_gc_root() {
    let ir = String::from_utf8(
        compile_module(&module_with_declared_field_class(), entry_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");

    // A declared-field class emits a per-class keys global.
    let global = "@perry_class_keys_class_keys_gc_root_ts__Point";
    assert!(
        ir.contains(&format!("{global} = internal global i64")),
        "expected class-keys global {global} in emitted IR:\n{ir}"
    );

    // Find the SSA value produced by taking the global's address, then assert
    // that exact value is passed to js_gc_register_global_root. This is the
    // #5042 fix; without it the global is only ever `load`ed and never
    // registered, so the evacuation rewrite pass can't fix it up.
    let ptrtoint_marker = format!("= ptrtoint ptr {global} to i64");
    let reg = ir
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let idx = line.find(&ptrtoint_marker)?;
            // Line looks like: `%r14 = ptrtoint ptr @... to i64`
            Some(line[..idx].trim().to_string())
        })
        .unwrap_or_else(|| {
            panic!(
                "class-keys global {global} address is never taken for GC-root \
                 registration (missing #5042 ptrtoint + js_gc_register_global_root)\n\
                 emitted IR:\n{ir}"
            )
        });

    let expected_call = format!("call void @js_gc_register_global_root(i64 {reg})");
    assert!(
        ir.contains(&expected_call),
        "class-keys global {global} address ({reg}) is not registered as a GC \
         root (expected `{expected_call}`)\nemitted IR:\n{ir}"
    );
}
