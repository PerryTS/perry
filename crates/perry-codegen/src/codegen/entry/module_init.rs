//! Guarded module initialization, shared by dependencies and worker program entries.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_module_init(
    llmod: &mut LlModule,
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    is_entry: bool,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    output_type: &str,
    // Issue #100: parallel-to-`cross_module.namespace_entries` list of
    // `(string_constant_global_name, byte_len)` for each export-key.
    // The populator emits one `getelementptr` per key into the stack
    // keys array — `byte_len` becomes the corresponding entry in the
    // key-lengths array passed to `js_create_namespace`. Empty when
    // this module is not a dynamic-import target.
    namespace_key_globals: &[(String, usize)],
) -> Result<()> {
    let strings_init_name = format!("__perry_init_strings_{}", module_prefix);
    let is_dylib = output_type == "dylib" || output_type == "staticlib";
    // Issue #753: idempotent init guard. Every module initializer gets
    // a one-byte `@__perry_init_done_<prefix>` flag and a thin
    // wrapper `<prefix>__init` that returns immediately when the
    // flag is set or stores 1 + dispatches to `<prefix>__init_body`
    // when it isn't. The wrapper is what the entry main calls
    // eagerly (for Eager modules) and what every
    // `Expr::DynamicImport` dispatch site calls (for any module
    // that's a dynamic-import target — possibly multiple sites in
    // the same program). The 2-state guard matches ESM's
    // partial-cycle semantics: re-entry during init returns without
    // re-running the body, leaving the namespace populator's work
    // partially observable. The wrapper sets `done = 1` BEFORE
    // calling the body so the re-entry path returns immediately.
    let done_global = format!("__perry_init_done_{}", module_prefix);
    // #10399: with a Worker in the program this guard MUST be per-thread.
    // A process-wide flag means a worker reaching `<mod>__init` finds the
    // 1 the main thread stored, skips the body, and then reads module
    // globals that point into the main thread's arena — where
    // `classify_heap_generation` is `Unknown`, so the object reads back
    // with no keys. Node and bun evaluate the module graph once per
    // worker; a thread-local flag is what makes that happen here.
    if crate::codegen::program_has_worker() {
        llmod.add_internal_thread_local_global(&done_global, I8, "0");
    } else {
        llmod.add_internal_global(&done_global, I8, "0");
    }
    let init_name = format!("{}__init", module_prefix);
    let init_body_name = format!("{}__init_body", module_prefix);
    {
        let wrap_fn = llmod.define_function(&init_name, VOID, vec![]);
        let _ = wrap_fn.create_block("entry");
        let _ = wrap_fn.create_block("guard.ret");
        let _ = wrap_fn.create_block("guard.do");
        let ret_label = wrap_fn.block_mut(1).unwrap().label.clone();
        let do_label = wrap_fn.block_mut(2).unwrap().label.clone();
        {
            let blk = wrap_fn.block_mut(0).unwrap();
            let done = blk.load(I8, &format!("@{}", done_global));
            let already = blk.icmp_ne(I8, &done, "0");
            blk.cond_br(&already, &ret_label, &do_label);
        }
        {
            let blk = wrap_fn.block_mut(1).unwrap();
            blk.ret_void();
        }
        {
            let blk = wrap_fn.block_mut(2).unwrap();
            blk.store(I8, "1", &format!("@{}", done_global));
            // A CommonJS program entry claims this thread's main-module
            // record in its wrapper preamble. Main publishes it before eager
            // imports; a worker also needs it before evaluating dependencies.
            if is_entry && crate::collectors::is_cjs_wrapped_module(hir) {
                blk.call_void("js_bootstrap_cjs_main_module_placeholder", &[]);
            }
            // Cyclic dependencies may call our hoisted functions before
            // our body. Prepare literal infrastructure without evaluating
            // declared classes or any user statement ahead of dependencies.
            let prepare_addr = format!(
                "ptrtoint (ptr @__perry_prepare_literals_{} to i64)",
                module_prefix
            );
            blk.call_void("js_run_module_init_catching", &[(I64, &prepare_addr)]);
            // Trigger init of static-dep + re-export source modules
            // before the body runs. Each `<dep>__init` is itself
            // wrapped by the same guard pattern, so this short-
            // circuits when the dep was already initialized
            // (Eager-via-main path) and fires the body when the
            // dep is Deferred and this is the first reach. The driver
            // excludes evaluation back-edges to the program entry.
            for dep_prefix in &cross_module.module_init_deps {
                if dep_prefix == module_prefix {
                    continue;
                }
                blk.call_void(&format!("{}__init", dep_prefix), &[]);
            }
            // Run each module body behind a native exception boundary.
            // A CommonJS wrapper publishes partial exports at the top of
            // this body; if an exception escapes before final publication,
            // the runtime caches that exact failure and wakes path-module
            // waiters before rethrowing. Keeping the boundary here avoids
            // adding a JavaScript `try` block that would change top-level
            // `let`/`const`/`class` scope in flat CJS emission.
            let init_body_addr = format!("ptrtoint (ptr @{} to i64)", init_body_name);
            blk.call_void(
                "js_run_module_init_catching",
                &[(I64, init_body_addr.as_str())],
            );
            blk.ret_void();
        }
    }
    // Declare every dep's `__init` symbol so the wrapper's calls
    // resolve at link time. Most overlap with `non_entry_module_prefixes`
    // (whose declarations live in the entry module's compilation),
    // but a non-entry module compiled standalone has no entry-side
    // declaration list — emit them here too. `declare_function`
    // dedupes by name.
    for dep_prefix in &cross_module.module_init_deps {
        if dep_prefix == module_prefix {
            continue;
        }
        llmod.declare_function(&format!("{}__init", dep_prefix), VOID, &[]);
    }
    // The body retains every existing semantic of `<prefix>__init`
    // (strings init, globals/GC registration, top-level statements,
    // namespace populator at the tail). Calls normally go through the
    // wrapper above, both within this module and across modules.
    let init_name = init_body_name;
    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let init_fn = llmod.define_function(&init_name, VOID, vec![]);
    // Keep the body symbol available to the legacy direct-body worker path.
    // Worker programs use the guarded wrapper, whose state is per thread.
    init_fn.linkage = "external".to_string();
    if is_dylib && !is_entry {
        init_fn.add_pre_return_void_call("js_typed_feedback_maybe_dump_trace");
    }
    let _ = init_fn.create_block("entry");
    {
        let blk = init_fn.block_mut(0).unwrap();
        if write_barriers_enabled() {
            blk.call_void("js_gc_write_barriers_emitted", &[(I32, "1")]);
        }
        // Each non-entry module runs its own string pool init at
        // the start of its module init function. The entry main
        // calls each module init in order (after running its own
        // strings init), so by the time user code in any module
        // executes, every module's strings are alive.
        blk.call_void(&strings_init_name, &[]);
    }
    // Same boundary as the entry-module main: hoisted post-init
    // setup must run AFTER the strings init populates module
    // globals like `@perry_class_keys_*`.
    init_fn.mark_entry_init_boundary();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let (mut init_shadow_slot_map, _) =
        enable_module_init_shadow_frame(init_fn, &hir.init, &flat_const_ids);
    // #10663: module init is the textbook run-once body.
    crate::codegen::helpers::decide_straight_line_store_outline(init_fn, &hir.init);

    let init_boxed_vars = module_boxed_vars.clone();
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    // `--opt-report` (#6952) attribution scope; no-op when off.
    let _opt_report_scope =
        crate::opt_report::enter_region("module_init", crate::opt_report::RegionKind::ModuleInit);
    let init_native_facts = crate::collectors::collect_native_region_fact_graph(
        &hir.init,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &init_boxed_vars,
        module_globals,
        // Module scope IS this body — see the `main` fact graph above (#6369).
        &HashMap::new(),
        classes,
        &cross_module.compile_time_constants,
        &cross_module.module_dispatch,
        // #9363: module-scope views need their construction proofs here
        // too — passing an empty map kept top-level accumulator loops on
        // the rooted/guarded path while in-function ones were clean.
        &cross_module.module_global_proven_types,
    );
    // #9363: the same redundant-shadow-slot pruning `codegen/function.rs`
    // does, which module init never got. The shadow map is built above
    // from the CONSERVATIVE pointer-typed-locals scan, before the fact
    // graph exists; a local the whole-write proof later shows can only
    // hold a Number keeps a root slot it can never need. That slot is not
    // just wasted stores: `local_is_inert_primitive` refuses any local
    // with one, so the accumulator of `sum += buf[i]` was never inert,
    // `loop_may_allocate` stayed true, and the inner loop kept a
    // per-iteration volatile GC poll that blocks vectorization. In a
    // function body the same loop was already clean — this was the whole
    // top-level/in-function asymmetry.
    //
    // `enable_post_init_shadow_frame` sized the frame from the unpruned
    // map, so the retained slot indices stay valid with holes, exactly as
    // in the function-body twin.
    crate::codegen::helpers::drop_number_local_root_slots(
        &mut init_shadow_slot_map,
        init_native_facts.number_by_construction_locals(),
    );
    let init_shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(&hir.init, &init_shadow_slot_map);

    // #7109: the module-init body participates in canonical (i32/u32/Str)
    // selection on exactly the per-value rules a function body uses. There
    // is no structural context reason to deny — see
    // `expr::MODULE_INIT_CONTEXT` for the audit — so the only remaining
    // gates are the two bisection env knobs.
    // #7128: one derivation, each flag reading its own knob. `Entry`
    // pins `allows_ptr_shape` off structurally (see below).
    let repsel_flags = crate::expr::RepselContextFlags::for_entry();
    let repsel_allows = repsel_flags.allows_canonical_i32;
    let repsel_str_allows = repsel_flags.allows_canonical_str;
    // The two value-level screens the `Stmt::Let` site consults (#7106
    // collected them for the report only; now they are load-bearing).
    let repsel_closure_refs = if repsel_allows || repsel_str_allows {
        crate::expr::collect_closure_referenced_locals(&hir.init)
    } else {
        std::collections::HashSet::new()
    };
    let repsel_str_ineligible = if repsel_str_allows {
        crate::expr::collect_canonical_str_ineligible_locals(&hir.init)
    } else {
        std::collections::HashSet::new()
    };
    let mut init_local_types = HashMap::new();
    if is_entry {
        crate::boxed_vars::collect_let_types_in_stmts(&hir.init, &mut init_local_types);
    }
    let mut ctx = FnCtx {
        func: init_fn,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: "module_init".to_string(),
        source_function_slug: crate::expr::native_region_slug("module_init"),
        active_region_id: None,
        native_facts: &init_native_facts,
        locals: HashMap::new(),
        local_types: init_local_types,
        proven_local_types: HashMap::new(),
        guarded_discriminant_aliases: HashMap::new(),
        module_global_proven_types: &cross_module.module_global_proven_types,
        module_global_transfers: &cross_module.module_global_transfers,
        reassigned_locals: crate::collectors::reassigned_locals(&hir.init),
        const_string_locals: HashMap::new(),
        const_number_locals: HashMap::new(),
        current_block: 0,
        discard_expr_value: false,
        discard_this_expr: false,
        truthy_call_result_requested: false,
        pending_truthy_call_result: None,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_labels: Vec::new(),
        classes,
        this_stack: Vec::new(),
        super_called_stack: Vec::new(),
        shared_super_scope_active: false,
        lexical_this_uses_derived_binding: false,
        inline_ctor_return: Vec::new(),
        new_target_stack: Vec::new(),
        class_stack: Vec::new(),
        in_static_member: false,
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures: HashMap::new(),
        current_closure_ptr: None,
        current_closure_slot: None,
        enums,
        is_async_fn: false,
        // #9423: an ESM is strict code (ES2024 SS11.2.2) and so is a Script
        // with a `"use strict"` prologue. This was hardcoded `false`, so
        // every codegen lane keyed on the CONTEXT rather than on a
        // node-carried flag ran module top-level code sloppy.
        is_strict_fn: hir.init_is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        anon_key_adds: &cross_module.anon_key_adds,
        class_init_chains: &cross_module.class_init_chains,
        class_header_image_globals: &cross_module.class_header_images,
        class_birth_reps: &cross_module.class_birth_reps,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: init_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        tdz_boxes: std::collections::HashSet::new(),
        compiler_private_async_i32_control_locals: &cross_module
            .compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals: &cross_module
            .compiler_private_async_i1_control_locals,
        scope_map: &cross_module.scope_map,
        string_accumulator_locals: &cross_module.string_accumulator_locals,
        string_length_read_of: None,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        guard_free_closure_bindings: std::collections::HashSet::new(),
        local_closure_param_counts: HashMap::new(),
        resolved_plain_callback_targets: HashMap::new(),
        resolved_versioned_loop_callback_targets: HashMap::new(),
        trusted_box_captures: false,
        versioned_loop_deopt_context: None,
        trusted_box_capture_ptrs: HashMap::new(),
        local_func_ref_ids: HashMap::new(),
        option_object_locals: HashMap::new(),
        object_literal_locals: HashSet::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        namespace_member_nested: &cross_module.namespace_member_nested,
        namespace_member_origin_names: &cross_module.namespace_member_origin_names,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        local_generator_funcs: &cross_module.local_generator_funcs,
        async_step_closures: &cross_module.async_step_closures,
        funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        method_has_synthetic_arguments: &cross_module.method_has_synthetic_arguments,
        method_arguments_length_only: &cross_module.method_arguments_length_only,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        ffi_aliases: &cross_module.ffi_aliases,
        imported_class_sources: &cross_module.imported_class_sources,
        imported_class_original_names: &cross_module.imported_class_original_names,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: init_native_facts.integer_locals(),
        int_valued_i64_locals: init_native_facts.int_valued_i64_locals(),
        not_bigint_locals: init_native_facts.not_bigint_locals(),
        number_by_construction_locals: init_native_facts.number_by_construction_locals(),
        unsigned_i32_locals: init_native_facts.unsigned_i32_locals(),
        shadow_slots_bound: init_shadow_slot_map.values().copied().collect(),
        temp_roots: crate::rooting::TempRootPool::default(),
        scoped_temp_roots: Vec::new(),
        shadow_slot_map: init_shadow_slot_map,
        persistent_shadow_slots: std::collections::HashSet::new(),
        declared_only_numeric_locals: std::collections::HashSet::new(),
        shadow_slot_clears_after_stmt: init_shadow_slot_clears_after_stmt,
        arena_state_slot: None,
        arena_state_lazy: false,
        class_keys_slots: HashMap::new(),
        class_shape_slots: HashMap::new(),
        class_header_images: HashMap::new(),
        array_length_snapshots: HashMap::new(),
        string_window_array_facts: Vec::new(),
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        region_loops: Vec::new(),
        region_loop_facts: Vec::new(),
        element_shape_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
        numeric_accumulator_f64_slots: HashMap::new(),
        transition_cache_base_slot: None,
        receiver_descriptors: Default::default(),
        poll_stride_counter_slot: None,
        deferred_integer_update_accumulators: HashSet::new(),
        local_slot_reps: HashMap::new(),
        // #7109: this entry body selects canonical i32/u32/Str on the same
        // per-value rules as a function body. Phase 1 (#6903) excluded it
        // on the premise that "the win lives in function bodies"; 9 of the
        // 17 suite benchmarks put their entire hot loop at module top
        // level, so it does not. `expr::MODULE_INIT_CONTEXT` carries the
        // audit of every entry-body property that made the exclusion look
        // load-bearing.
        repsel_context_allows_canonical_i32: repsel_allows,
        repsel_context_denial: None,
        // Ptr<Shape> stays off here, on its own flag now: Phase 5a reused
        // the canonical-i32 gate, and #6991 is a live rooting bug for a
        // compiled receiver held across the globalThis-population
        // collection — which runs around module init.
        repsel_context_allows_ptr_shape: repsel_flags.allows_ptr_shape,
        repsel_ptr_shape_context_denial: repsel_flags.ptr_shape_denial,
        repsel_closure_ref_locals: repsel_closure_refs,
        repsel_context_allows_canonical_str: repsel_str_allows,
        repsel_str_ineligible_locals: repsel_str_ineligible,
        spec_abi_functions: &cross_module.spec_abi_functions,
        spec_return_proofs: &cross_module.spec_return_proofs,
        spec_ta_bindings: &cross_module.spec_ta_bindings,
        spec_ta_ready: std::collections::HashSet::new(),
        spec_i32_params: std::collections::HashSet::new(),
        spec_bool_params: std::collections::HashSet::new(),
        i1_local_slots: HashMap::new(),
        index_used_locals: init_native_facts.index_used_locals(),
        strictly_i32_bounded_locals: init_native_facts.strictly_i32_bounded_locals(),
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        local_value_aliases: HashMap::new(),
        local_imported_object_aliases: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        imported_object_literals: &cross_module.imported_object_literals,
        short_spread_method_candidates: &cross_module.short_spread_method_candidates,
        program_class_accessor_names: cross_module.program_class_accessor_names.as_deref(),
        object_literal_method_candidates: &cross_module.object_literal_method_candidates,
        compile_time_constants: init_native_facts.compile_time_constants(),
        target_triple: &cross_module.target_triple,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        pod_records: std::collections::HashMap::new(),
        pod_views: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_replaced_split_part_lengths: std::collections::HashMap::new(),
        scalar_replaced_uppercase_sources: std::collections::HashMap::new(),
        scalar_slot_shadow_slots: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news: init_native_facts.non_escaping_news().clone(),
        non_escaping_new_used_fields: init_native_facts.non_escaping_new_used_fields().clone(),
        non_escaping_arrays: init_native_facts.non_escaping_arrays().clone(),
        non_escaping_array_used_indices: init_native_facts
            .non_escaping_array_used_indices()
            .clone(),
        non_escaping_array_length_only_indices: init_native_facts
            .non_escaping_array_length_only_indices()
            .clone(),
        fusible_uppercase_locals: init_native_facts.fusible_uppercase_locals().clone(),
        suffix_cursor_locals: init_native_facts.suffix_cursor_locals().clone(),
        suffix_cursors: std::collections::HashMap::new(),
        non_escaping_object_literals: init_native_facts.non_escaping_object_literals().clone(),
        non_escaping_object_literal_used_fields: init_native_facts
            .non_escaping_object_literal_used_fields()
            .clone(),
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        integer_returning_functions: &cross_module.returns_int_functions,
        i32_identity_functions: &cross_module.i32_identity_functions,
        param_int_ranges: &cross_module.param_int_ranges,
        typed_f64_functions: &cross_module.typed_f64_functions,
        typed_i32_functions: &cross_module.typed_i32_functions,
        typed_string_functions: &cross_module.typed_string_functions,
        typed_i1_functions: &cross_module.typed_i1_functions,
        typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
        typed_f64_methods: &cross_module.typed_f64_methods,
        pshape_methods: &cross_module.pshape_methods,
        pshape_arg_methods: &cross_module.pshape_arg_methods,
        nonnegative_index_methods: &cross_module.nonnegative_index_methods,
        trusted_array_param_handles: HashMap::new(),
        versioned_indexed_loop_facts: Vec::new(),
        stable_packed_loop_facts: Vec::new(),
        pshape_tower_routable: &cross_module.pshape_tower_routable,
        proven_this: None,
        guarded_this_class: None,
        proven_shape_params: std::collections::HashMap::new(),
        typed_i32_methods: &cross_module.typed_i32_methods,
        typed_i1_methods: &cross_module.typed_i1_methods,
        typed_string_methods: &cross_module.typed_string_methods,
        typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
        typed_f64_closures: &cross_module.typed_f64_closures,
        typed_i32_closures: &cross_module.typed_i32_closures,
        typed_i1_closures: &cross_module.typed_i1_closures,
        typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
        typed_string_closures: &cross_module.typed_string_closures,
        typed_closure_capture_reps: &cross_module.typed_closure_capture_reps,
        was_unrolled: hir.init_was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        property_get_ic_override: None,
        typed_parse_rodata: Vec::new(),
        buffer_data_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
        program_shadows_buffer_read_method: cross_module.program_shadows_buffer_read_method,
        module_has_shape_barrier_sites: cross_module.module_dispatch.has_shape_barrier_sites(),
        min_length_bounds: HashMap::new(),
        bounded_buffer_index_pairs: Vec::new(),
        guarded_buffer_index_pairs: Vec::new(),
        buffer_hazard_reasons: HashMap::new(),
        native_i32_aliases: HashMap::new(),
        int_range_aliases: HashMap::new(),
        int_range_facts: Vec::new(),
        next_loop_proof_scope_id: 0,
        nonnegative_integer_locals: HashSet::new(),
        elided_arguments: HashMap::new(),
        native_rep_records: Vec::new(),
        known_noalias_buffer_locals: init_native_facts.known_noalias_buffer_locals(),
        sealed_buffer_locals: init_native_facts.sealed_buffer_locals(),
        late_exposed_buffer_locals: init_native_facts.late_exposed_buffer_locals(),
        buffer_alias_base,
    };
    // Register every module-level global's ADDRESS as a GC root —
    // same reason as the entry-module branch above (issue #36). For
    // non-entry modules the registration runs inside their __init
    // function, which the entry main calls in topological order
    // right after js_gc_init, so by the time any user code executes
    // every module's globals are already GC-rooted.
    register_module_globals_as_gc_roots(&mut ctx, module_globals);
    // Issue #894: split into early/late around top-level lowering so a
    // computed-Symbol-key static field whose key/init reference
    // top-level module lets (e.g. effect's `make()` factory:
    // `static [TypeId] = variance`) sees populated globals.
    if is_entry {
        // ESM entry (import/export syntax or top-level await — Node's module
        // detection): mark the pending module-evaluation checkpoint so the
        // first microtask drain finishes promise/queueMicrotask jobs before
        // the nextTick queue, matching Node's job-within-checkpoint ordering
        // for ESM evaluation (#788). CJS-style entries keep ticks-first.
        //
        // #9412: "has imports or exports" is not the same question for a
        // CommonJS entry, because `cjs_wrap` gives every CommonJS file BOTH —
        // a synthetic `import { createRequire as __perry_cjs_create_require }
        // from 'node:module'` and an `export default _cjs`. So any entry
        // containing a bare `require(` answered "ESM" here and ran its
        // `process.nextTick` callbacks AFTER the promise queue, where Node
        // runs a CommonJS program's ticks first. Measured against Node 26:
        // an entry as `.cjs` prints ["tick","promise","await"], the same file
        // as `.mjs` prints ["promise","await","tick"] — the deferral is right,
        // it was just being applied to the wrong module kind. Every real
        // bundle requires a builtin and every minimal fixture doesn't, so the
        // ordering was correct in exactly the programs a test suite contains.
        //
        // Only this checkpoint is re-gated. `is_esm_entry` below keeps its
        // original meaning for GlobalDeclarationInstantiation: a CommonJS
        // module's top-level `function` declarations live inside the module
        // wrapper and are NOT global-object properties either, so "not a
        // Script" is the right answer there for a wrapped entry too — and
        // that predicate is mirrored in `perry-hir`'s `lower_module_fn`,
        // which runs before the wrap flag is knowable here.
        let cjs_wrapped_entry = crate::collectors::is_cjs_wrapped_module(hir);
        if (!hir.imports.is_empty() || !hir.exports.is_empty() || hir.has_top_level_await)
            && !cjs_wrapped_entry
        {
            ctx.block().call_void("js_mark_entry_module_esm", &[]);
        }
        // #5579: GlobalDeclarationInstantiation for a Script. A non-ESM entry
        // program runs as a *Script*, so its bare top-level `function`
        // declarations become own properties of the global object (observable
        // via `Object.prototype.hasOwnProperty.call(globalThis, name)` — the
        // check the Test262 async harness uses for `$DONE`). ESM modules
        // (import/export syntax or top-level await) instead bind in the
        // module record and do NOT reflect. #11591: HIR only fills these lists
        // under the global-script opt-in (`PERRY_GLOBAL_SCRIPT_THIS`); a `.ts`
        // entry is a module under Node, so by default both are empty.
        //
        // Gated additionally on the program actually referencing `globalThis`:
        // if it never reads the global object the reflection is unobservable,
        // so skipping it avoids adding dynamic-property-helper calls (and their
        // startup cost) to every pure program's module init. Emitted before
        // user init so the functions are visible to top-level code (hoisting).
        let is_esm_entry =
            !hir.imports.is_empty() || !hir.exports.is_empty() || hir.has_top_level_await;
        if !is_esm_entry && hir.references_global_this {
            emit_script_global_function_decls(&mut ctx, hir);
        }
        if !is_esm_entry {
            emit_annexb_global_undefined_decls(&mut ctx, hir);
        }
    }
    init_static_fields_early(&mut ctx, hir)?;
    stmt::lower_top_level_stmts(&mut ctx, &hir.init)
        .with_context(|| format!("lowering init statements of module '{}'", hir.name))?;
    init_static_fields_late(&mut ctx, hir)?;

    // Issue #100: populate `@__perry_ns_<module_prefix>` from the
    // namespace_entries list at the tail of the non-entry __init.
    // The entry main has already called this module's __init AFTER
    // every static-import dependency's __init (topo sort) — so
    // re-export sources have populated their getters. Local
    // exports' bindings are also set because top-level lowering ran
    // above. The dispatcher in `Expr::DynamicImport` loads
    // `@__perry_ns_<prefix>` and wraps it in `js_promise_resolved`.
    // Issue #842: also run the populator for side-effect-only
    // dynamic-import targets (`namespace_entries` empty but module
    // is a target). The populator emits `js_create_namespace(0, ...)`
    // → an empty NaN-boxed object → stored into `@__perry_ns_<prefix>`,
    // satisfying the consumer-side extern reference.
    if (!cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target)
        && !ctx.block().is_terminated()
    {
        emit_namespace_populator(
            &mut ctx,
            &cross_module.namespace_entries,
            namespace_key_globals,
            module_prefix,
        );
    }

    if !ctx.block().is_terminated() {
        ctx.block().ret_void();
    }
    let ic_globals = std::mem::take(&mut ctx.ic_globals);
    let typed_parse_rodata = std::mem::take(&mut ctx.typed_parse_rodata);
    let ic_end = ctx.ic_site_counter;
    let pending = std::mem::take(&mut ctx.pending_declares);
    let buffer_alias_used = ctx.buffer_data_slots.len() as u32;
    let native_rep_records = std::mem::take(&mut ctx.native_rep_records);
    drop(ctx);
    llmod.ic_counter = ic_end;
    llmod.buffer_alias_counter += buffer_alias_used;
    llmod.native_rep_records.extend(native_rep_records);
    for (name, ret, params) in pending {
        llmod.declare_function(&name, ret, &params);
    }
    // NB: the plugin ABI shim (`perry_plugin_abi_version` /
    // `plugin_activate` / `plugin_deactivate`) is emitted from the entry
    // branch above, NOT here — see `emit_plugin_abi_shim` and issue #5273.
    // A dylib's top-level plugin exports live in its entry module, and the
    // three symbols must be defined exactly once per shared library.
    for ic_name in &ic_globals {
        llmod.add_raw_global(crate::expr::inline_cache_global_definition(ic_name));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}
