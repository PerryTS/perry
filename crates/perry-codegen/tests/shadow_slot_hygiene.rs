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
        namespace_member_vars: std::collections::HashSet::new(),
        namespace_member_namespace_prefixes: std::collections::HashMap::new(),
        namespace_import_prefixes: std::collections::HashMap::new(),
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
        nextjs_path_init_modules: Vec::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn entry_opts() -> CompileOptions {
    CompileOptions {
        is_entry_module: true,
        ..empty_opts()
    }
}

fn shadow_hygiene_module() -> Module {
    Module {
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
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

fn top_level_shadow_module(name: &str) -> Module {
    Module {
        name: name.to_string(),
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
        init: vec![
            Stmt::Let {
                id: 10,
                name: "dead".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::MapNew),
            },
            Stmt::Let {
                id: 11,
                name: "numeric".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Number(42.0)),
            },
            Stmt::Let {
                id: 12,
                name: "live".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Array(Vec::new())),
            },
            Stmt::Expr(Expr::LocalGet(12)),
        ],
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

fn top_level_loop_shadow_module() -> Module {
    let mut module = top_level_shadow_module("entry_loop_shadow.ts");
    module.init = vec![Stmt::For {
        init: None,
        condition: Some(Expr::Bool(false)),
        update: None,
        body: vec![
            Stmt::Let {
                id: 20,
                name: "loop_value".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::MapNew),
            },
            Stmt::Expr(Expr::LocalGet(20)),
        ],
    }];
    module
}

fn flat_const_row_alias_shadow_module() -> Module {
    Module {
        name: "entry_flat_const_shadow.ts".to_string(),
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
        init: vec![
            Stmt::Let {
                id: 30,
                name: "kernel".to_string(),
                ty: Type::Array(Box::new(Type::Array(Box::new(Type::Number)))),
                mutable: false,
                init: Some(Expr::Array(vec![
                    Expr::Array(vec![Expr::Integer(1), Expr::Integer(2)]),
                    Expr::Array(vec![Expr::Integer(3), Expr::Integer(4)]),
                ])),
            },
            Stmt::Let {
                id: 31,
                name: "krow".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(30)),
                    index: Box::new(Expr::Integer(0)),
                }),
            },
            Stmt::Let {
                id: 32,
                name: "k".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(31)),
                    index: Box::new(Expr::Integer(1)),
                }),
            },
            Stmt::Expr(Expr::LocalGet(32)),
        ],
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

fn reassigned_any_shadow_module() -> Module {
    Module {
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        name: "reassigned_any_shadow.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 2,
            name: "probe_reassign".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 40,
                    name: "value".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                },
                Stmt::Expr(Expr::LocalSet(40, Box::new(Expr::Array(Vec::new())))),
                Stmt::Return(Some(Expr::LocalGet(40))),
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

fn mixed_any_alias_shadow_module() -> Module {
    Module {
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        name: "mixed_any_alias_shadow.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 4,
            name: "probe_mixed_any_alias".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 60,
                    name: "source".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Number(1.0)),
                },
                Stmt::Expr(Expr::LocalSet(60, Box::new(Expr::Array(Vec::new())))),
                Stmt::Let {
                    id: 61,
                    name: "alias".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::LocalGet(60)),
                },
                Stmt::Expr(Expr::LocalSet(60, Box::new(Expr::Number(2.0)))),
                Stmt::Let {
                    id: 62,
                    name: "later".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Array(Vec::new())),
                },
                Stmt::Return(Some(Expr::LocalGet(61))),
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

fn closure_captured_write_shadow_module() -> Module {
    Module {
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        name: "closure_captured_write_shadow.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 3,
            name: "probe_closure_write".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 50,
                    name: "value".to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Number(0.0)),
                },
                Stmt::Let {
                    id: 51,
                    name: "writer".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::Closure {
                        func_id: 30,
                        params: Vec::new(),
                        return_type: Type::Any,
                        body: vec![
                            Stmt::Expr(Expr::LocalSet(50, Box::new(Expr::Array(Vec::new())))),
                            Stmt::Return(Some(Expr::LocalGet(50))),
                        ],
                        captures: vec![50],
                        mutable_captures: vec![50],
                        captures_this: false,
                        captures_new_target: false,
                        enclosing_class: None,
                        is_arrow: false,
                        is_async: false,
                        is_generator: false,
                        is_strict: false,
                    }),
                },
                Stmt::Return(Some(Expr::LocalGet(51))),
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

fn function_slice<'a>(ir: &'a str, name: &str) -> &'a str {
    let define_marker = format!("@{}(", name);
    let define_start = ir
        .match_indices("define ")
        .find_map(|(idx, _)| {
            let line_end = ir[idx..].find('\n').map(|offset| idx + offset)?;
            ir[idx..line_end].contains(&define_marker).then_some(idx)
        })
        .unwrap_or_else(|| panic!("expected function '{}' in IR", name));
    let body_end = ir[define_start..]
        .find("\n}\n")
        .map(|offset| define_start + offset + 3)
        .expect("function body should be closed");
    &ir[define_start..body_end]
}

fn init_body_function_name(ir: &str) -> String {
    // `__init_body` is emitted with `external` linkage (worker_threads must
    // call it directly, bypassing the once-guard on the `__init` wrapper — see
    // codegen/entry.rs), so locate it by the `void @<prefix>__init_body()`
    // shape rather than hard-coding the `internal` linkage word.
    for line in ir.lines() {
        if !line.trim_start().starts_with("define ") {
            continue;
        }
        if let Some(at) = line.find(" void @") {
            let after_at = at + " void @".len();
            if let Some(end) = line[after_at..].find("__init_body()") {
                let prefix = &line[after_at..after_at + end];
                return format!("{}__init_body", prefix);
            }
        }
    }
    panic!("expected non-entry init body function in IR");
}

#[test]
fn function_shadow_slots_clear_dead_values_and_skip_numeric_roots() {
    let ir = String::from_utf8(compile_module(&shadow_hygiene_module(), empty_opts()).unwrap())
        .expect("LLVM IR should be UTF-8");

    assert!(
        ir.contains("call i64 @js_shadow_frame_push(i32 2)"),
        "known numeric Any local must not reserve a shadow slot"
    );

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
        !ir.contains("call void @js_shadow_slot_set(i32 2"),
        "known numeric Any local must not shift later pointer roots into a third slot"
    );
}

#[test]
fn entry_module_top_level_shadow_frame_starts_after_init_prelude() {
    let ir = String::from_utf8(
        compile_module(&top_level_shadow_module("entry_shadow.ts"), entry_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let main_ir = function_slice(&ir, "main");

    let gc_init = main_ir
        .find("call void @js_gc_init()")
        .expect("entry main should initialize GC before user code");
    let strings_init = main_ir
        .find("__perry_init_strings_")
        .expect("entry main should initialize module strings before user code");
    let frame_push = main_ir
        .find("call i64 @js_shadow_frame_push(i32 2)")
        .expect("entry main should push a top-level shadow frame");
    let user_alloc = main_ir
        .find("call i64 @js_map_alloc")
        .expect("top-level allocation should be present after init");

    assert!(gc_init < frame_push);
    assert!(strings_init < frame_push);
    assert!(frame_push < user_alloc);
    assert!(
        main_ir.contains("call void @js_shadow_frame_pop"),
        "entry main returns should pop the top-level shadow frame"
    );
}

#[test]
fn entry_module_top_level_shadow_slots_update_and_clear() {
    let ir = String::from_utf8(
        compile_module(
            &top_level_shadow_module("entry_shadow_slots.ts"),
            entry_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let main_ir = function_slice(&ir, "main");

    assert!(
        main_ir.contains("call i64 @js_shadow_frame_push(i32 2)"),
        "known numeric top-level Any local must not reserve a shadow slot"
    );

    let dead_write = main_ir
        .find("call void @js_shadow_slot_set(i32 0, i64 %")
        .expect("top-level pointer let should write its pointer to shadow slot 0");
    let dead_clear = main_ir[dead_write..]
        .find("call void @js_shadow_slot_set(i32 0, i64 0)")
        .map(|offset| dead_write + offset)
        .expect("top-level dead shadow slot should be cleared after last use");
    let later_alloc = main_ir[dead_clear..]
        .find("call i64 @js_array_alloc")
        .map(|offset| dead_clear + offset)
        .expect("later allocation should remain after dead slot clear");

    assert!(dead_write < dead_clear);
    assert!(dead_clear < later_alloc);
    assert!(
        !main_ir.contains("call void @js_shadow_slot_set(i32 2"),
        "known numeric top-level Any local must not shift later pointer roots into a third slot"
    );
}

#[test]
fn non_entry_module_init_body_gets_post_init_shadow_frame() {
    let ir = String::from_utf8(
        compile_module(
            &top_level_shadow_module("non_entry_shadow.ts"),
            empty_opts(),
        )
        .unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let init_body_name = init_body_function_name(&ir);
    let init_ir = function_slice(&ir, &init_body_name);

    let strings_init = init_ir
        .find("__perry_init_strings_")
        .expect("non-entry init body should initialize strings before user code");
    let frame_push = init_ir
        .find("call i64 @js_shadow_frame_push(i32 2)")
        .expect("non-entry init body should push a top-level shadow frame");
    let user_alloc = init_ir
        .find("call i64 @js_map_alloc")
        .expect("top-level allocation should be present after init");

    assert!(strings_init < frame_push);
    assert!(frame_push < user_alloc);
    assert!(
        init_ir.contains("call void @js_shadow_slot_set(i32 0, i64 %"),
        "non-entry top-level pointer local should update its shadow slot"
    );
    assert!(
        init_ir.contains("call void @js_shadow_frame_pop"),
        "non-entry init returns should pop the top-level shadow frame"
    );
}

#[test]
fn top_level_loop_body_shadow_slots_clear_each_iteration() {
    let ir =
        String::from_utf8(compile_module(&top_level_loop_shadow_module(), entry_opts()).unwrap())
            .expect("LLVM IR should be UTF-8");
    let main_ir = function_slice(&ir, "main");

    let body_write = main_ir
        .find("call void @js_shadow_slot_set(i32 0, i64 %")
        .expect("loop-body pointer local should write its shadow slot");
    let body_clear = main_ir[body_write..]
        .find("call void @js_shadow_slot_set(i32 0, i64 0)")
        .map(|offset| body_write + offset)
        .expect("loop-body shadow slot should be cleared before the next iteration");
    let loop_backedge = main_ir[body_clear..]
        .find("br label %for.update")
        .map(|offset| body_clear + offset)
        .expect("for body should branch to update after clearing loop-body slots");

    assert!(body_write < body_clear);
    assert!(body_clear < loop_backedge);
}

#[test]
fn flat_const_row_aliases_do_not_reserve_shadow_slots() {
    let ir = String::from_utf8(
        compile_module(&flat_const_row_alias_shadow_module(), entry_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let main_ir = function_slice(&ir, "main");

    assert!(
        main_ir.contains("call i64 @js_shadow_frame_push(i32 1)"),
        "only the flat-const table root should reserve a shadow slot"
    );
    assert!(
        !main_ir.contains("call void @js_shadow_slot_set(i32 1"),
        "row aliases of flat-const tables must not touch shadow slots"
    );
}

#[test]
fn reassigned_any_from_number_to_pointer_reserves_and_updates_shadow_slot() {
    let ir =
        String::from_utf8(compile_module(&reassigned_any_shadow_module(), empty_opts()).unwrap())
            .expect("LLVM IR should be UTF-8");
    let fn_ir = function_slice(&ir, "perry_fn_reassigned_any_shadow_ts__probe_reassign");

    assert!(
        fn_ir.contains("call i64 @js_shadow_frame_push(i32 1)"),
        "Any local with a later pointer write must reserve a shadow slot"
    );
    let array_alloc = fn_ir
        .find("call i64 @js_array_alloc")
        .expect("pointer reassignment should allocate an array");
    let slot_update = fn_ir[array_alloc..]
        .find("call void @js_shadow_slot_set(i32 0, i64 %")
        .map(|offset| array_alloc + offset)
        .expect("pointer reassignment should update the reserved shadow slot");
    assert!(array_alloc < slot_update);
}

#[test]
fn mixed_any_writes_keep_alias_shadow_slots_precise() {
    let ir =
        String::from_utf8(compile_module(&mixed_any_alias_shadow_module(), empty_opts()).unwrap())
            .expect("LLVM IR should be UTF-8");
    let fn_ir = function_slice(
        &ir,
        "perry_fn_mixed_any_alias_shadow_ts__probe_mixed_any_alias",
    );

    assert!(
        fn_ir.contains("call i64 @js_shadow_frame_push(i32 3)"),
        "mixed Any writes must keep source, alias, and later reserved as shadow slots"
    );
    for slot_idx in 0..3 {
        assert!(
            fn_ir.contains(&format!("call void @js_shadow_slot_set(i32 {slot_idx}")),
            "expected writes or clears for shadow slot {slot_idx}:\n{fn_ir}"
        );
    }
}

#[test]
fn closure_body_write_to_captured_outer_local_is_visible_to_shadow_analysis() {
    let ir = String::from_utf8(
        compile_module(&closure_captured_write_shadow_module(), empty_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");
    let fn_ir = function_slice(
        &ir,
        "perry_fn_closure_captured_write_shadow_ts__probe_closure_write",
    );

    assert!(
        fn_ir.contains("call i64 @js_shadow_frame_push(i32 2)"),
        "captured Any local written to a pointer inside a closure must keep its outer slot"
    );
    assert!(
        fn_ir.contains("call void @js_shadow_slot_bind(i32 0, ptr %"),
        "boxed captured local should bind its outer shadow slot to the box slot"
    );
}
