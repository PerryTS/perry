//! Static class methods compiled as top-level LLVM functions.
//!
//! Child module of `method.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private helpers reachable.

use super::*;

/// Compile a static class method as a top-level LLVM function with
/// no `this` parameter. Mostly identical to `compile_function` but
/// the LLVM symbol name is scoped by module, class id, class name, and
/// method name instead of `perry_fn_<modprefix>__<name>`.
#[allow(clippy::too_many_arguments)]
pub(in crate::codegen) fn compile_static_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    f: &Function,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    module_global_types: &HashMap<u32, perry_hir::types::Type>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = scoped_static_method_name(module_prefix, class.id, &class.name, &f.name);

    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);

    // gh #6206 / #6081: same shadow-frame emission as compile_method — static
    // method bodies were equally invisible to the exact-roots copying minor.
    // One extra slot roots the resolved receiver: static `this` is usually
    // the non-pointer INT32 class-ref, but `js_static_this_resolve` returns a
    // REAL heap receiver for `C.m.call(x)` / `.apply(x)` / inherited `D.m()`
    // dynamic dispatch, and that object may be reachable only from this slot.
    let shadow_slot_map = if crate::codegen::helpers::precise_root_analysis_enabled() {
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let m =
            crate::collectors::collect_pointer_typed_locals(&f.params, &f.body, &flat_const_ids);
        crate::codegen::helpers::maybe_spill_roots_to_shadow_frame(
            lf,
            &llvm_name,
            m.len() + 1,
            &f.body,
        );
        lf.enable_shadow_frame(m.len() as u32 + 1);
        m
    } else {
        std::collections::HashMap::new()
    };
    let this_shadow_slot_idx = shadow_slot_map.len() as u32;
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(&f.body, &shadow_slot_map);

    let _ = lf.create_block("entry");

    let mut static_boxed_vars = module_boxed_vars.clone();
    crate::codegen::arguments::add_arguments_mapped_boxes(&f.params, &mut static_boxed_vars);

    // A static method invoked as `C.m()` binds `this` to the class
    // constructor `C`. Represent that as the class-ref NaN-box (the same
    // INT32-tagged class-id value `Expr::ClassRef` lowers to) stored in a
    // `this` slot so `this.x` / `this.#x()` / `this[k]` inside the body
    // resolve against the class object via the normal dynamic-dispatch
    // path. (Previously `this` fell through to `js_implicit_this_get` and
    // read back `undefined`.)
    let class_ref_cid = class_ids.get(&class.name).copied().unwrap_or(class.id);
    let class_ref_lit = {
        let bits = crate::nanbox::INT32_TAG | (class_ref_cid as u64 & 0xFFFF_FFFF);
        crate::nanbox::double_literal(f64::from_bits(bits))
    };
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        // Receiver-sensitive `this`: dynamic dispatch paths (inherited
        // `D.m()`, `C.m.call(x)` / `.apply(x)`) arm a one-shot override that
        // this prologue call consumes; direct calls fall back to the lexical
        // class-ref, preserving the prior `this === C` behavior. Needed so
        // static private brand checks (`this.#x` in a static method) see the
        // real receiver (test262 class/elements static-private-*).
        let resolved_this = blk.call(
            DOUBLE,
            "js_static_this_resolve",
            &[(DOUBLE, &class_ref_lit)],
        );
        blk.store(DOUBLE, &resolved_this, &this_slot);
        if crate::codegen::helpers::precise_root_analysis_enabled() {
            blk.call_void(
                "js_shadow_slot_bind",
                &[(I32, &this_shadow_slot_idx.to_string()), (PTR, &this_slot)],
            );
        }
        let mut map = HashMap::new();
        for p in &f.params {
            let arg_name = format!("%arg{}", p.id);
            let slot =
                crate::codegen::arguments::store_param_slot(blk, p, &static_boxed_vars, &arg_name);
            if let Some(slot_idx) = shadow_slot_map.get(&p.id).copied() {
                blk.call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &slot_idx.to_string()), (PTR, &slot)],
                );
            }
            map.insert(p.id, slot);
        }
        (this_slot, map)
    };
    crate::codegen::arguments::release_boxed_param_slots_at_exit(
        lf,
        &f.params,
        &static_boxed_vars,
        &locals,
    );

    // Seed with module-global declared types (mirrors compile_method /
    // compile_function): static-method bodies read module globals through
    // `@perry_global_*` slots too, and without the types here both the
    // type-aware dispatch sites and the #6185 perry/thread worker-closure
    // check (`hazardous_module_global_ids`) were blind inside static
    // methods. Param types override on collision.
    let mut local_types: HashMap<u32, perry_hir::types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &f.params {
        local_types.insert(p.id, p.ty.clone());
    }

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    // `--opt-report` (#6952) attribution scope; no-op when off.
    let _opt_report_scope = crate::opt_report::enter_region(
        &format!("{}.{} (static)", class.name, f.name),
        crate::opt_report::RegionKind::Method,
    );
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        &f.body,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &static_boxed_vars,
        module_globals,
        // #6369: declared types of module-scope bindings this body reads through.
        &local_types,
        classes,
        &cross_module.compile_time_constants,
        &cross_module.module_dispatch,
        // #9363: a method body reads the same module-scope views.
        &cross_module.module_global_proven_types,
    );

    // Representation-selection context gates (see codegen/function.rs).
    let repsel_flags =
        crate::expr::RepselContextFlags::for_body(f.is_async, f.is_generator, f.was_plain_async);
    let repsel_allows = repsel_flags.allows_canonical_i32;
    let repsel_str_allows = repsel_flags.allows_canonical_str;
    // #7106: report the structural context exclusion at the `Stmt::Let` site.
    let repsel_context_denial = repsel_flags.canonical_denial;
    let report_denial = repsel_flags.report_denial();
    let repsel_closure_refs = if repsel_allows || repsel_str_allows || report_denial {
        crate::expr::collect_closure_referenced_locals(&f.body)
    } else {
        std::collections::HashSet::new()
    };
    let repsel_str_ineligible = if repsel_str_allows || report_denial {
        crate::expr::collect_canonical_str_ineligible_locals(&f.body)
    } else {
        std::collections::HashSet::new()
    };

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("{}.{}", class.name, f.name),
        source_function_slug: crate::expr::native_region_slug(&format!(
            "{}.{}",
            class.name, f.name
        )),
        regex_factory_identity: None,
        active_region_id: None,
        native_facts: &native_facts,
        locals,
        local_types,
        proven_local_types: std::collections::HashMap::new(),
        guarded_discriminant_aliases: std::collections::HashMap::new(),
        module_global_proven_types: &cross_module.module_global_proven_types,
        reassigned_locals: crate::collectors::reassigned_locals(&f.body),
        const_string_locals: std::collections::HashMap::new(),
        const_number_locals: std::collections::HashMap::new(),
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
        this_stack: vec![this_slot],
        super_called_stack: Vec::new(),
        shared_super_scope_active: false,
        lexical_this_uses_derived_binding: false,
        inline_ctor_return: Vec::new(),
        new_target_stack: Vec::new(),
        // A static method's `this` is the class constructor (bound above to
        // the class-ref slot). `class_stack` carries the class name so
        // `super.x` in a static method resolves against the parent's static
        // side, mirroring instance-method setup.
        class_stack: vec![class.name.clone()],
        in_static_member: true,
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
        is_async_fn: f.is_async,
        is_strict_fn: f.is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
        class_header_image_globals: &cross_module.class_header_images,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: static_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        tdz_boxes: std::collections::HashSet::new(),
        compiler_private_async_i32_control_locals: &cross_module
            .compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals: &cross_module
            .compiler_private_async_i1_control_locals,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        guard_free_closure_bindings: std::collections::HashSet::new(),
        local_closure_param_counts: HashMap::new(),
        resolved_arrow_callback_targets: HashMap::new(),
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
        integer_locals: native_facts.integer_locals(),
        int_valued_i64_locals: native_facts.int_valued_i64_locals(),
        not_bigint_locals: native_facts.not_bigint_locals(),
        number_by_construction_locals: native_facts.number_by_construction_locals(),
        unsigned_i32_locals: native_facts.unsigned_i32_locals(),
        // Conservative: treat every slot as possibly-bound (param binds are
        // emitted before FnCtx exists here), so clears never get skipped.
        shadow_slots_bound: shadow_slot_map.values().copied().collect(),
        temp_roots: crate::rooting::TempRootPool::default(),
        shadow_slot_map,
        persistent_shadow_slots: std::collections::HashSet::new(),
        declared_only_numeric_locals: std::collections::HashSet::new(),
        shadow_slot_clears_after_stmt,
        arena_state_slot: None,
        arena_state_lazy: false,
        class_keys_slots: HashMap::new(),
        class_shape_slots: HashMap::new(),
        class_header_images: HashMap::new(),
        array_length_snapshots: HashMap::new(),
        string_window_array_facts: Vec::new(),
        masked_region_scalar_locals: std::collections::HashSet::new(),
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        element_shape_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
        numeric_accumulator_f64_slots: HashMap::new(),
        transition_cache_base_slot: None,
        receiver_descriptors: Default::default(),
        poll_stride_counter_slot: None,
        deferred_integer_update_accumulators: HashSet::new(),
        local_slot_reps: HashMap::new(),
        repsel_context_allows_canonical_i32: repsel_allows,
        // #7109 split the FIELD out of `repsel_context_allows_canonical_i32`;
        // #7128 split the VALUE, which is what the knob actually reads. Until
        // then this was still `repsel_allows`, so `PERRY_CANONICAL_I32_LOCALS=0`
        // disabled every Ptr<Shape> consumption in the program.
        repsel_context_allows_ptr_shape: repsel_flags.allows_ptr_shape,
        repsel_ptr_shape_context_denial: repsel_flags.ptr_shape_denial,
        repsel_context_denial,
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
        index_used_locals: native_facts.index_used_locals(),
        strictly_i32_bounded_locals: native_facts.strictly_i32_bounded_locals(),
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
        object_literal_method_candidates: &cross_module.object_literal_method_candidates,
        compile_time_constants: native_facts.compile_time_constants(),
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
        non_escaping_news: native_facts.non_escaping_news().clone(),
        non_escaping_new_used_fields: native_facts.non_escaping_new_used_fields().clone(),
        non_escaping_arrays: native_facts.non_escaping_arrays().clone(),
        non_escaping_array_used_indices: native_facts.non_escaping_array_used_indices().clone(),
        non_escaping_array_length_only_indices: native_facts
            .non_escaping_array_length_only_indices()
            .clone(),
        fusible_uppercase_locals: native_facts.fusible_uppercase_locals().clone(),
        suffix_cursor_locals: native_facts.suffix_cursor_locals().clone(),
        suffix_cursors: std::collections::HashMap::new(),
        non_escaping_object_literals: native_facts.non_escaping_object_literals().clone(),
        non_escaping_object_literal_used_fields: native_facts
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
        was_unrolled: f.was_unrolled,
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
        native_rep_records: Vec::new(),
        known_noalias_buffer_locals: native_facts.known_noalias_buffer_locals(),
        buffer_alias_base,
    };
    crate::codegen::arguments::materialize_arguments_object(
        &mut ctx,
        &f.params,
        Some(&f.body),
        crate::codegen::arguments::ArgumentsCallee::Undefined,
    );
    if f.is_async {
        stmt::lower_async_rejecting_stmts(&mut ctx, &f.body).with_context(|| {
            format!("lowering async body of static '{}::{}'", class.name, f.name)
        })?;
    } else {
        stmt::lower_stmts(&mut ctx, &f.body)
            .with_context(|| format!("lowering body of static '{}::{}'", class.name, f.name))?;
    }

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if f.is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &undef);
        }
    }
    let artifacts = take_lowered_fn_artifacts(&mut ctx);
    drop(ctx);
    publish_lowered_fn_artifacts(llmod, artifacts);
    Ok(())
}
