use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, ModuleInitKind, Stmt};

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
        namespace_member_nested: Vec::new(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
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
        local_source_spans: std::collections::HashMap::new(),
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
                    field_writeback: None,
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
        local_source_spans: std::collections::HashMap::new(),
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

    // #7708: the store->note->barrier ordering is asserted over the CFG region
    // reachable from `apush.inbounds`, NOT over the text between two block
    // labels. The barrier has already migrated once into a dedicated
    // `apush.barrier.*` block downstream of `apush.realloc` -- a legal
    // block-structure change that a text slice bounded by those labels is
    // structurally unable to see (this test sat red for two days over exactly
    // that; the #7698 fix documents the same failure shape on the
    // class-field-store side). A census that cannot distinguish "the subject
    // vanished" from "the subject moved" is a gate that cannot fail honestly.
    let region = apush_region_from_inbounds(&ir);
    assert!(
        region
            .first_visit_order
            .first()
            .is_some_and(|b| b.starts_with("apush.inbounds.")),
        "optimized local push should emit an in-bounds fast block"
    );
    let store_at = region
        .position_of("store double")
        .expect("optimized push should emit a direct element store");
    let layout_at = region
        .position_of("call void @js_gc_note_slot_layout")
        .expect("optimized push store should keep slot layout notes");
    let barrier_at = region
        .position_of("call void @js_write_barrier_slot")
        .expect("optimized push direct store must remember old-born parent slots");

    assert!(
        store_at.0 == 0,
        "the direct element store must live in the in-bounds block itself, \
         not a successor; found it in {}",
        region.first_visit_order[store_at.0]
    );
    assert!(
        store_at < layout_at,
        "slot-layout note must come at or after the direct store on the \
         in-bounds path (store in {}, note in {})",
        region.first_visit_order[store_at.0],
        region.first_visit_order[layout_at.0]
    );
    assert!(
        layout_at < barrier_at,
        "the write barrier must come after the layout note on the in-bounds \
         path (note in {}, barrier in {})",
        region.first_visit_order[layout_at.0],
        region.first_visit_order[barrier_at.0]
    );
}

/// The `apush.*` region reachable from the push's `apush.inbounds` block, in
/// first-visit (breadth-first) order, with block-relative marker positions.
///
/// Successors are read from `label %name` operands, and the walk stays inside
/// `apush.*`-prefixed blocks: the region ends where the push's own control
/// flow rejoins the surrounding function, so a marker found here is on the
/// push path and nowhere else.
struct ApushRegion {
    first_visit_order: Vec<String>,
    block_text: Vec<String>,
}

impl ApushRegion {
    /// `(block_index_in_first_visit_order, byte_offset_in_block)` of the first
    /// occurrence of `needle`, or `None`. The tuple ordering makes "earlier
    /// block, then earlier within the block" the comparison the asserts use.
    fn position_of(&self, needle: &str) -> Option<(usize, usize)> {
        self.block_text
            .iter()
            .enumerate()
            .find_map(|(i, text)| text.find(needle).map(|off| (i, off)))
    }
}

fn apush_region_from_inbounds(ir: &str) -> ApushRegion {
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in ir.lines() {
        if let Some(label) = line.strip_suffix(':') {
            if !label.is_empty()
                && !label.contains(' ')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
            {
                if let Some(done) = current.take() {
                    blocks.push(done);
                }
                current = Some((label.to_string(), String::new()));
                continue;
            }
        }
        if let Some((_, text)) = current.as_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    if let Some(done) = current.take() {
        blocks.push(done);
    }

    let entry = blocks
        .iter()
        .map(|(name, _)| name.clone())
        .find(|name| name.starts_with("apush.inbounds."))
        .expect("optimized local push should emit an in-bounds fast block");

    let successors = |name: &str| -> Vec<String> {
        blocks
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, text)| {
                let mut out = Vec::new();
                let mut rest = text.as_str();
                while let Some(pos) = rest.find("label %") {
                    let tail = &rest[pos + "label %".len()..];
                    let target: String = tail
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                        .collect();
                    if !target.is_empty() {
                        out.push(target);
                    }
                    rest = tail;
                }
                out
            })
            .unwrap_or_default()
    };

    let mut order: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    queue.push_back(entry);
    while let Some(name) = queue.pop_front() {
        if order.contains(&name) || !name.starts_with("apush.") {
            continue;
        }
        order.push(name.clone());
        for succ in successors(&name) {
            queue.push_back(succ);
        }
    }
    let block_text = order
        .iter()
        .map(|name| {
            blocks
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone())
                .unwrap_or_default()
        })
        .collect();
    ApushRegion {
        first_visit_order: order,
        block_text,
    }
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
