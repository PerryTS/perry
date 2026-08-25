//! Class-method and static-method compilation. Split out of
//! `codegen.rs` (now `codegen/mod.rs`).

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use perry_hir::Function;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64, PTR};

use super::helpers::{node_stream_parent_kind, scoped_static_method_name};
use super::method_trampolines::{
    emit_guarded_undefined, emit_public_generic, emit_public_typed, guarded_undefined_name,
};
use super::opts::CrossModuleCtx;
use super::typed_abi::{
    generic_method_body_name, lower_typed_f64_body, lower_typed_f64_receiver_body,
    lower_typed_i1_body, lower_typed_i32_body, lower_typed_string_body, typed_f64_method_name,
    typed_f64_receiver_method_name, typed_i1_method_name, typed_i32_method_name,
    typed_param_reps_for_params, typed_string_method_name, TypedFunctionTrampolineKind,
    TypedReceiverMethodInfo,
};

/// Compile a class instance method as a top-level LLVM function with the
/// signature `perry_method_<class>_<name>(this_box: double, args: double…)
/// -> double`. The first parameter (`this`) is stored in a slot whose
/// pointer is pushed onto `this_stack`, then `class_stack` is set so
/// inner `Expr::This` and `super` work correctly.
pub(super) fn compile_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
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
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
    typed_public_trampoline: Option<TypedFunctionTrampolineKind>,
    force_generic_body: bool,
    proven_this: Option<crate::collectors::PtrShapeLocal>,
    nonnegative_index_params: Option<&[u32]>,
    fast_array_handle_clone: bool,
    ptr_array_cache_clone: bool,
    guarded_undefined_clone: bool,
    pshape_arg_clone: bool,
) -> Result<()> {
    let public_llvm_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    // Representation-selection Phase 5a: the proven-`this` clone is a SECOND,
    // additive body compiled from the same HIR through the same statement
    // lowerer. It never replaces the public symbol and never participates in
    // the typed-trampoline / generic-body split — those are emitted by the
    // primary (`proven_this: None`) invocation for this same method.
    let is_pshape_clone = proven_this.is_some() && !pshape_arg_clone;
    let is_index_clone = nonnegative_index_params.is_some();
    let pshape_arg_plan = pshape_arg_clone
        .then(|| {
            cross_module
                .pshape_arg_methods
                .get(&(class.name.clone(), method.name.clone()))
        })
        .flatten();
    let guarded_undefined_param = (!is_index_clone && !ptr_array_cache_clone && !pshape_arg_clone)
        .then(|| {
            cross_module
                .guarded_undefined_method_params
                .get(&(class.name.clone(), method.name.clone()))
                .copied()
        })
        .flatten();
    let fast_array_param_ids = if fast_array_handle_clone {
        crate::codegen::typed_abi::nonnegative_index_fast_array_params(
            method,
            nonnegative_index_params.expect("fast-array clone has index parameters"),
        )
    } else {
        Vec::new()
    };
    debug_assert!(!(is_pshape_clone && is_index_clone));
    debug_assert!(!fast_array_handle_clone || is_index_clone);
    debug_assert!(!fast_array_handle_clone || !fast_array_param_ids.is_empty());
    debug_assert!(!ptr_array_cache_clone || is_pshape_clone);
    debug_assert!(!guarded_undefined_clone || guarded_undefined_param.is_some());
    debug_assert!(!guarded_undefined_clone || !is_index_clone);
    debug_assert!(!guarded_undefined_clone || !ptr_array_cache_clone);
    debug_assert!(!pshape_arg_clone || pshape_arg_plan.is_some());
    debug_assert!(!pshape_arg_clone || !is_index_clone);
    debug_assert!(!pshape_arg_clone || !ptr_array_cache_clone);
    debug_assert!(!pshape_arg_clone || typed_public_trampoline.is_none());
    debug_assert!(!pshape_arg_clone || !force_generic_body);
    let family_name = if pshape_arg_clone {
        crate::collectors::pshape_args_method_name(&public_llvm_name)
    } else if ptr_array_cache_clone {
        crate::collectors::ptr_array_cache_method_name(&public_llvm_name)
    } else if is_pshape_clone {
        crate::collectors::pshape_method_name(&public_llvm_name)
    } else {
        public_llvm_name.clone()
    };
    let llvm_name = if fast_array_handle_clone {
        crate::codegen::nonnegative_index_fast_array_method_name(
            &public_llvm_name,
            nonnegative_index_params.expect("fast-array clone has index parameters"),
        )
    } else if let Some(params) = nonnegative_index_params {
        crate::codegen::nonnegative_index_method_name(&public_llvm_name, params)
    } else if guarded_undefined_clone {
        guarded_undefined_name(
            &family_name,
            guarded_undefined_param.expect("undefined clone parameter"),
        )
    } else if guarded_undefined_param.is_some() {
        generic_method_body_name(&family_name)
    } else if ptr_array_cache_clone || is_pshape_clone || pshape_arg_clone {
        family_name.clone()
    } else if typed_public_trampoline.is_some() || force_generic_body {
        generic_method_body_name(&public_llvm_name)
    } else {
        public_llvm_name.clone()
    };

    // Build the param list: (this, arg0, arg1, ...). All are doubles.
    let mut params: Vec<(LlvmType, String)> =
        Vec::with_capacity(method.params.len() + 1 + fast_array_param_ids.len());
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    for id in &fast_array_param_ids {
        params.push((I64, format!("%fast_array_handle{id}")));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    // Plain `$pshape` clones are producer-published capabilities and need
    // external linkage for guarded calls from importing modules. The stricter
    // array-cache clone remains module-local: only containment-proven locals
    // in this module may select it. An exact-undefined candidate names this
    // body `$pshape$generic` (or `$undefN`) and publishes a separate guarded
    // `$pshape` wrapper, so its implementation bodies also remain private.
    if ptr_array_cache_clone
        || is_index_clone
        || typed_public_trampoline.is_some()
        || force_generic_body
        || guarded_undefined_param.is_some()
        || pshape_arg_clone
    {
        lf.linkage = "internal".to_string();
    }
    super::helpers::apply_pshape_inline_policy(lf, method, is_pshape_clone || pshape_arg_clone);
    if is_index_clone {
        lf.pre_statepoint_inline = true;
    }

    // gh #6206 / #6081: methods were compiled WITHOUT a shadow frame — same
    // exact-roots liveness hole as closures (see compile_closure). One extra
    // slot roots the receiver (`this` is a pointer value reachable from
    // nothing else when the caller holds it only in a register temp).
    let shadow_slot_map = if super::helpers::precise_root_analysis_enabled() {
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let m = crate::collectors::collect_pointer_typed_locals(
            &method.params,
            &method.body,
            &flat_const_ids,
        );
        crate::codegen::helpers::maybe_spill_roots_to_shadow_frame(
            lf,
            &llvm_name,
            m.len() + 1,
            &method.body,
        );
        lf.enable_shadow_frame(m.len() as u32 + 1);
        m
    } else {
        std::collections::HashMap::new()
    };
    let this_shadow_slot_idx = shadow_slot_map.len() as u32;
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(&method.body, &shadow_slot_map);

    let _ = lf.create_block("entry");

    let mut method_boxed_vars = module_boxed_vars.clone();
    super::arguments::add_arguments_mapped_boxes(&method.params, &mut method_boxed_vars);

    // Allocate slots for `this` and each parameter; pre-populate with
    // the incoming values.
    let index_param_ids: HashSet<u32> = nonnegative_index_params
        .unwrap_or_default()
        .iter()
        .copied()
        .collect();
    let mut index_i32_param_slots: HashMap<u32, String> = HashMap::new();
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "%this_arg", &this_slot);
        if super::helpers::precise_root_analysis_enabled() {
            blk.call_void(
                "js_shadow_slot_bind",
                &[(I32, &this_shadow_slot_idx.to_string()), (PTR, &this_slot)],
            );
        }
        let mut map = HashMap::new();
        for p in &method.params {
            let arg_name = format!("%arg{}", p.id);
            let slot = super::arguments::store_param_slot(blk, p, &method_boxed_vars, &arg_name);
            if let Some(slot_idx) = shadow_slot_map.get(&p.id).copied() {
                blk.call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &slot_idx.to_string()), (PTR, &slot)],
                );
            }
            map.insert(p.id, slot);
            if index_param_ids.contains(&p.id) {
                // The clone is reachable only from a call site that proved
                // this argument is in [0, i32::MAX]. The ordinary boxed ABI
                // materializes such loop indices as plain doubles, so one
                // entry conversion supplies the canonical raw index slot.
                let raw_i32 = blk.fptosi(DOUBLE, &arg_name, I32);
                let i32_slot = blk.alloca(I32);
                blk.store(I32, &raw_i32, &i32_slot);
                index_i32_param_slots.insert(p.id, i32_slot);
            }
        }
        (this_slot, map)
    };

    let mut local_types: HashMap<u32, perry_hir::types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &method.params {
        local_types.insert(p.id, p.ty.clone());
    }
    if let Some(index) = guarded_undefined_param.filter(|_| guarded_undefined_clone) {
        local_types.insert(method.params[index].id, perry_hir::types::Type::Void);
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
    let _opt_report_scope = crate::opt_report::enter_method_region(
        &format!("{}.{}", class.name, method.name),
        cross_module
            .module_dispatch
            .return_shape_method_class(&class.name, &method.name, method.id)
            .is_some(),
    );
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        &method.body,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &method_boxed_vars,
        module_globals,
        // #6369: declared types of module-scope bindings this body reads through.
        &local_types,
        classes,
        &cross_module.compile_time_constants,
        &cross_module.module_dispatch,
    );
    let mut index_clone_integer_locals = native_facts.integer_locals().clone();
    index_clone_integer_locals.extend(index_param_ids.iter().copied());
    let index_param_proofs: HashMap<u32, perry_hir::types::Type> = index_param_ids
        .iter()
        .copied()
        .map(|id| (id, perry_hir::types::Type::Int32))
        .collect();

    // Representation-selection context gates (see codegen/function.rs).
    let repsel_flags = crate::expr::RepselContextFlags::for_body(
        method.is_async,
        method.is_generator,
        method.was_plain_async,
    );
    let repsel_allows = repsel_flags.allows_canonical_i32;
    let repsel_str_allows = repsel_flags.allows_canonical_str;
    // #7106: report the structural context exclusion at the `Stmt::Let` site.
    let repsel_context_denial = repsel_flags.canonical_denial;
    let report_denial = repsel_flags.report_denial();
    let repsel_closure_refs = if repsel_allows || repsel_str_allows || report_denial {
        crate::expr::collect_closure_referenced_locals(&method.body)
    } else {
        std::collections::HashSet::new()
    };
    let repsel_str_ineligible = if repsel_str_allows || report_denial {
        crate::expr::collect_canonical_str_ineligible_locals(&method.body)
    } else {
        std::collections::HashSet::new()
    };

    let mut guarded_param_proofs = index_param_proofs;
    if let Some(index) = guarded_undefined_param.filter(|_| guarded_undefined_clone) {
        guarded_param_proofs.insert(method.params[index].id, perry_hir::types::Type::Void);
    }
    if let Some(plan) = pshape_arg_plan {
        // Unlike a source annotation, this overlay is backed by the exact
        // class+shape guard that exclusively routes into `$pshape_args`.
        // Supplying it to property dispatch lets an unannotated (`Any`) JS
        // parameter resolve the declared field before the Ptr<Shape> overlay
        // removes that field access's ordinary IC diamond.
        guarded_param_proofs.extend(plan.args.iter().map(|arg| {
            (
                arg.param_id,
                perry_hir::types::Type::Named(arg.fact.class_name.clone()),
            )
        }));
    }
    let mut reassigned_locals = crate::collectors::reassigned_locals(&method.body);
    if let Some(index) = guarded_undefined_param.filter(|_| guarded_undefined_clone) {
        // Candidate discovery already rejected every user-authored write and
        // closure capture.  The remaining assignment is TypeScript's lowered
        // optional-parameter prologue (`undefined = undefined`), which cannot
        // invalidate the wrapper's exact-value proof.
        reassigned_locals.remove(&method.params[index].id);
    }
    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("{}.{}", class.name, method.name),
        source_function_slug: crate::expr::native_region_slug(&format!(
            "{}.{}",
            class.name, method.name
        )),
        active_region_id: None,
        native_facts: &native_facts,
        locals,
        local_types,
        proven_local_types: guarded_param_proofs,
        guarded_discriminant_aliases: std::collections::HashMap::new(),
        module_global_proven_types: &cross_module.module_global_proven_types,
        reassigned_locals,
        const_string_locals: std::collections::HashMap::new(),
        const_number_locals: std::collections::HashMap::new(),
        current_block: 0,
        discard_expr_value: false,
        discard_this_expr: false,
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
        class_stack: vec![class.name.clone()],
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
        is_async_fn: method.is_async,
        is_strict_fn: true,
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
        boxed_vars: method_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        tdz_boxes: std::collections::HashSet::new(),
        compiler_private_async_i32_control_locals: &cross_module
            .compiler_private_async_i32_control_locals,
        compiler_private_async_i1_control_locals: &cross_module
            .compiler_private_async_i1_control_locals,
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        local_closure_param_counts: HashMap::new(),
        resolved_arrow_callback_targets: HashMap::new(),
        trusted_box_captures: false,
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
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        ffi_aliases: &cross_module.ffi_aliases,
        imported_class_sources: &cross_module.imported_class_sources,
        imported_class_original_names: &cross_module.imported_class_original_names,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &index_clone_integer_locals,
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
        class_keys_slots: HashMap::new(),
        class_shape_slots: HashMap::new(),
        class_header_images: HashMap::new(),
        cached_lengths: HashMap::new(),
        array_length_snapshots: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        masked_window_array_facts: Vec::new(),
        masked_region_scalar_locals: std::collections::HashSet::new(),
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        element_shape_loop_facts: Vec::new(),
        i32_counter_slots: index_i32_param_slots,
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
        spec_i32_params: index_param_ids.clone(),
        i1_local_slots: HashMap::new(),
        index_used_locals: native_facts.index_used_locals(),
        strictly_i32_bounded_locals: native_facts.strictly_i32_bounded_locals(),
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        local_value_aliases: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
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
        trusted_array_param_handles: fast_array_param_ids
            .iter()
            .copied()
            .map(|id| (id, format!("%fast_array_handle{id}")))
            .collect(),
        versioned_indexed_loop_facts: Vec::new(),
        stable_packed_loop_facts: Vec::new(),
        pshape_tower_routable: &cross_module.pshape_tower_routable,
        proven_this,
        proven_shape_params: pshape_arg_plan
            .map(|plan| {
                plan.args
                    .iter()
                    .map(|arg| (arg.param_id, arg.fact.clone()))
                    .collect()
            })
            .unwrap_or_default(),
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
        was_unrolled: method.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
        program_shadows_buffer_read_method: cross_module.program_shadows_buffer_read_method,
        min_length_bounds: HashMap::new(),
        bounded_buffer_index_pairs: Vec::new(),
        guarded_buffer_index_pairs: Vec::new(),
        buffer_hazard_reasons: HashMap::new(),
        native_i32_aliases: HashMap::new(),
        int_range_aliases: HashMap::new(),
        int_range_facts: Vec::new(),
        next_loop_proof_scope_id: 0,
        nonnegative_integer_locals: index_param_ids,
        native_rep_records: Vec::new(),
        known_noalias_buffer_locals: native_facts.known_noalias_buffer_locals(),
        buffer_alias_base,
    };

    // Resolve each immutable callback parameter/arity once, before the method
    // body. The runtime answers null unless the actual value is a plain arrow
    // whose declared/rest shape can use this exact call ABI. Exact immutable
    // aliases (`const cb = callback`) reuse the same answer: every successful
    // read has that identity by construction, while a pre-initialisation read
    // throws during `LocalGet` lowering before dispatch is reached.
    let hoisted_callback_calls = crate::collectors::collect_hoisted_callback_calls(method);
    let callback_keys: std::collections::BTreeSet<(u32, usize)> = hoisted_callback_calls
        .iter()
        .map(|call| (call.source_param, call.arity))
        .collect();
    let mut resolved_callback_ptrs = HashMap::new();
    for (source_param, arity) in callback_keys {
        let Some(source_slot) = ctx.locals.get(&source_param).cloned() else {
            continue;
        };
        let source_box = ctx.block().load(DOUBLE, &source_slot);
        let source_handle = crate::expr::unbox_to_i64(ctx.block(), &source_box);
        let fn_ptr = ctx.block().call(
            PTR,
            "js_closure_resolve_arrow_direct_call",
            &[(I64, &source_handle), (I32, &arity.to_string())],
        );
        resolved_callback_ptrs.insert((source_param, arity), fn_ptr);
    }
    for call in hoisted_callback_calls {
        let Some(fn_ptr) = resolved_callback_ptrs
            .get(&(call.source_param, call.arity))
            .cloned()
        else {
            continue;
        };
        ctx.resolved_arrow_callback_targets
            .insert((call.callee_local, call.arity), fn_ptr);
    }

    super::arguments::materialize_arguments_object(
        &mut ctx,
        &method.params,
        super::arguments::ArgumentsCallee::Undefined,
    );

    // Constructors emitted as standalone cross-module LLVM functions (named
    // `<prefix>__<class>_constructor`) must bake the field initializers into
    // their body. At the `new ImportedClass(...)` call site, `lower_new`
    // applies initializers against the imported class stub — which has none
    // — so without this, imported classes construct with all fields left
    // as uninitialized register values (read as NaN-boxed undefined).
    let is_constructor_method = method.name == format!("{}_constructor", class.name);
    if is_constructor_method {
        if class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some()
        {
            // #8648: only a closure in this body can need the RUNTIME cell.
            // An arrow compiles as its own LLVM function and reaches the
            // binding through `js_derived_super_bind_current` /
            // `js_derived_this_check_current`, which read the thread-local
            // stack this push maintains. With no closure here, nothing can
            // perform that lookup, and `bind_derived_this_after_super` uses
            // the local alloca directly -- so the push/pop pair is a
            // thread-local round trip per construction for a cell no one
            // reads. Measured: 1.89x on a two-class `new B(x, y)` loop,
            // 3.14x on `shapes.ts`.
            if crate::collectors::body_contains_closure(&method.body) {
                crate::expr::this_super_call::push_shared_super_called_slot(&mut ctx);
                ctx.shared_super_scope_active = true;
            } else {
                crate::expr::this_super_call::push_super_called_slot(&mut ctx);
            }
        }
        // Stage field initializers around the parent body chain so leaf
        // fields can read state set by parent body (Refs #420):
        //   - has extends: apply only ancestors here; self-fields apply
        //     later (after super() in own-body case, after explicit parent
        //     ctor call in no-own-body case).
        //   - no extends: apply all (= just self) here.
        // A no-own-ctor class with a PURELY dynamic parent (`extends_expr`,
        // no `extends_name`) now emits a synthesized dynamic super below —
        // stage its self fields AFTER that call (tail SelfOnly), like any
        // other heritage class, instead of applying them twice.
        let no_ctor_dynamic_parent = class.constructor.is_none()
            && class.extends_name.is_none()
            && class.extends_expr.is_some();
        let init_mode = if class.extends_name.is_some() || no_ctor_dynamic_parent {
            crate::lower_call::FieldInitMode::AncestorsOnly
        } else {
            crate::lower_call::FieldInitMode::All
        };
        crate::lower_call::apply_field_initializers_recursive(&mut ctx, &class.name, init_mode)
            .with_context(|| {
                format!(
                    "applying field initializers for '{}' constructor",
                    class.name
                )
            })?;
        // Refs #420: when a class has no own constructor but extends a parent
        // that DOES have a body, JS spec requires a default ctor that calls
        // `super(...args)` — implicit forward. perry's standalone ctor for
        // such a class previously emitted only field initializers, so the
        // parent's ctor body (e.g. ColumnBuilder's `this.config = {...}`)
        // never ran when called via the cross-module dispatch path. Inject a
        // call to the parent's standalone ctor symbol here, forwarding all
        // args. The walk skips empty-bodied parents (matching the JS spec
        // chain semantics).
        if class.constructor.is_none()
            && (class.extends_name.is_some() || class.extends_expr.is_some())
        {
            let builtin_parent_runtime = match class.extends_name.as_deref() {
                Some("Writable") => Some("js_node_stream_writable_subclass_init"),
                Some("Duplex") => Some("js_node_stream_duplex_subclass_init"),
                Some("Transform") => Some("js_node_stream_transform_subclass_init"),
                _ => None,
            };
            let mut effective_parent: Option<&str> = if builtin_parent_runtime.is_some() {
                None
            } else {
                class.extends_name.as_deref()
            };
            while let Some(pname) = effective_parent {
                let Some(pc) = ctx.classes.get(pname).copied() else {
                    break;
                };
                let has_local_body = pc.constructor.is_some();
                let has_imported_ctor = ctx
                    .imported_class_ctors
                    .get(pname)
                    .map(|ctor| ctor.stops_constructor_walk())
                    .unwrap_or(false);
                if has_local_body || has_imported_ctor {
                    break;
                }
                effective_parent = pc.extends_name.as_deref();
            }
            // Wall 51: a class with a DYNAMIC parent (`extends_expr`, e.g.
            // `class X extends _mod.Parent {}`) must route its synthesized
            // super through the runtime dynamic-parent dispatcher below
            // (`js_fetch_or_value_super` keyed on the decl-time-registered parent
            // value), NOT this inline static-symbol call — the parent's
            // standalone ctor symbol lives under a different module prefix and
            // the static call would target the wrong/empty symbol, so the parent
            // ctor never ran and inherited fields stayed undefined. Skip the
            // inline path for dynamic-parent classes.
            if let Some(pname) = effective_parent.filter(|_| class.extends_expr.is_none()) {
                let pname_owned = pname.to_string();
                let node_stream_kind = if pname_owned == "Readable" {
                    node_stream_parent_kind(ctx.classes, class)
                } else {
                    None
                };
                if let Some(kind) = node_stream_kind {
                    let undef_lit =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let opts_box = method
                        .params
                        .first()
                        .and_then(|param| ctx.locals.get(&param.id).cloned())
                        .map(|slot| ctx.block().load(DOUBLE, &slot))
                        .unwrap_or_else(|| undef_lit.clone());
                    let this_box = match ctx.this_stack.last().cloned() {
                        Some(slot) => ctx.block().load(DOUBLE, &slot),
                        None => undef_lit.clone(),
                    };
                    let runtime_fn = match kind {
                        "readable" => "js_node_stream_readable_subclass_init",
                        "duplex" => "js_node_stream_duplex_subclass_init",
                        "transform" => "js_node_stream_transform_subclass_init",
                        _ => unreachable!("node stream parent kind {}", kind),
                    };
                    ctx.block().call(
                        DOUBLE,
                        runtime_fn,
                        &[(DOUBLE, &this_box), (DOUBLE, &opts_box)],
                    );
                } else {
                    // Resolve the standalone-ctor symbol name. Prefer the
                    // local class table (same module) for an inline call;
                    // fall back to imported_class_ctors for cross-module.
                    let (ctor_sym, param_count) = if let Some(pclass) =
                        ctx.classes.get(&pname_owned).copied()
                    {
                        if pclass.constructor.is_some() {
                            // Local class with own ctor — use the per-module-prefix
                            // standalone symbol, same one compile_method emits.
                            let module_prefix = ctx.strings.module_prefix().to_string();
                            let sym = format!("{}__{}_constructor", module_prefix, pname_owned);
                            let pcount = pclass
                                .constructor
                                .as_ref()
                                .map(|c| c.params.len())
                                .unwrap_or(0);
                            (sym, pcount)
                        } else if let Some(ctor) =
                            ctx.imported_class_ctors.get(&pname_owned).cloned()
                        {
                            (ctor.symbol, ctor.param_count)
                        } else {
                            // No callable ctor symbol — bail.
                            stmt::lower_stmts(&mut ctx, &method.body).with_context(|| {
                                format!("lowering body of method '{}::{}'", class.name, method.name)
                            })?;
                            // Fall through to the default ret at end.
                            if !ctx.block().is_terminated() {
                                let undef = crate::nanbox::double_literal(f64::from_bits(
                                    crate::nanbox::TAG_UNDEFINED,
                                ));
                                ctx.block().ret(DOUBLE, &undef);
                            }
                            let _ = std::mem::take(&mut ctx.ic_globals);
                            let _ = std::mem::take(&mut ctx.typed_parse_rodata);
                            let _ = std::mem::take(&mut ctx.pending_declares);
                            return Ok(());
                        }
                    } else if let Some(ctor) = ctx.imported_class_ctors.get(&pname_owned).cloned() {
                        (ctor.symbol, ctor.param_count)
                    } else {
                        // #6469: the walk terminated at a native Error-family
                        // base with no callable ctor symbol — `class X extends
                        // Error {}` with no own ctor anywhere (effect's
                        // `makeException` shape). The static-`new` path bakes
                        // the spec default Error-init at the call site (#573,
                        // `lower_call/new.rs`); this standalone ctor is the
                        // only body the DYNAMIC construct replay runs, so
                        // without the same init `new <classValue>("msg")`
                        // produced a message-less instance and every effect
                        // error printed "An error has occurred". Delegate to a
                        // runtime helper (rather than open-coding like the
                        // static arm) because the forwarding params here are
                        // undefined-padded — the helper applies the spec's
                        // "If message is not undefined" guard so an absent arg
                        // doesn't shadow `Error.prototype.message` with an own
                        // undefined.
                        if matches!(
                            pname_owned.as_str(),
                            "Error"
                                | "TypeError"
                                | "RangeError"
                                | "ReferenceError"
                                | "SyntaxError"
                                | "URIError"
                                | "EvalError"
                                | "AggregateError"
                        ) {
                            let undef_lit = crate::nanbox::double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            ));
                            let msg_box = method
                                .params
                                .first()
                                .and_then(|p| ctx.locals.get(&p.id).cloned())
                                .map(|slot| ctx.block().load(DOUBLE, &slot))
                                .unwrap_or_else(|| undef_lit.clone());
                            let this_box = ctx
                                .this_stack
                                .last()
                                .cloned()
                                .map(|slot| ctx.block().load(DOUBLE, &slot))
                                .unwrap_or_else(|| undef_lit.clone());
                            let kind_idx = ctx.strings.intern(&pname_owned);
                            let kind_handle_global =
                                format!("@{}", ctx.strings.entry(kind_idx).handle_global);
                            let blk = ctx.block();
                            let kind_box = blk.load(DOUBLE, &kind_handle_global);
                            let kind_bits = blk.bitcast_double_to_i64(&kind_box);
                            let kind_raw =
                                blk.and(I64, &kind_bits, crate::nanbox::POINTER_MASK_I64);
                            blk.call_void(
                                "js_error_subclass_default_init",
                                &[(DOUBLE, &this_box), (DOUBLE, &msg_box), (I64, &kind_raw)],
                            );
                        }
                        ("".to_string(), 0)
                    };
                    if !ctor_sym.is_empty() {
                        let undef_lit = crate::nanbox::double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ));
                        // Forward this method's params, padding with undefined if
                        // the parent expects more.
                        let mut forwarded: Vec<String> = Vec::with_capacity(param_count);
                        for (i, p) in method.params.iter().enumerate() {
                            if i >= param_count {
                                break;
                            }
                            let slot = ctx.locals.get(&p.id).cloned();
                            if let Some(slot) = slot {
                                forwarded.push(ctx.block().load(DOUBLE, &slot));
                            } else {
                                forwarded.push(undef_lit.clone());
                            }
                        }
                        while forwarded.len() < param_count {
                            forwarded.push(undef_lit.clone());
                        }
                        // Load `this` from the this_stack.
                        let this_slot = ctx.this_stack.last().cloned();
                        let this_box = if let Some(ref slot) = this_slot {
                            ctx.block().load(DOUBLE, &slot)
                        } else {
                            undef_lit.clone()
                        };
                        let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                            .chain(forwarded.iter().map(|_| DOUBLE))
                            .collect();
                        let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                            Vec::with_capacity(1 + forwarded.len());
                        ctor_args.push((DOUBLE, &this_box));
                        for la in &forwarded {
                            ctor_args.push((DOUBLE, la.as_str()));
                        }
                        // Synthesized default-ctor forwarding to an imported parent
                        // ctor: discard the return (parent override does not
                        // replace `this`). Declared DOUBLE to match the symbol's
                        // real signature (see codegen/mod.rs).
                        ctx.pending_declares
                            .push((ctor_sym.clone(), DOUBLE, ctor_param_types));
                        let parent_result = ctx.block().call(DOUBLE, &ctor_sym, &ctor_args);
                        if let Some(this_slot) = this_slot {
                            let current_this = ctx.block().load(DOUBLE, &this_slot);
                            let bound_this = ctx.block().call(
                                DOUBLE,
                                "js_ctor_return_override",
                                &[
                                    (DOUBLE, &current_this),
                                    (DOUBLE, &parent_result),
                                    (crate::types::I32, "0"),
                                ],
                            );
                            ctx.block().store(DOUBLE, &bound_this, &this_slot);
                        }
                    }
                }
            }
            if let Some(runtime_fn) = builtin_parent_runtime {
                let undef_lit =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let opts = method
                    .params
                    .first()
                    .and_then(|param| ctx.locals.get(&param.id).cloned())
                    .map(|slot| ctx.block().load(DOUBLE, &slot))
                    .unwrap_or_else(|| undef_lit.clone());
                let this_box = ctx
                    .this_stack
                    .last()
                    .cloned()
                    .map(|slot| ctx.block().load(DOUBLE, &slot))
                    .unwrap_or_else(|| undef_lit.clone());
                ctx.block()
                    .call(DOUBLE, runtime_fn, &[(DOUBLE, &this_box), (DOUBLE, &opts)]);
            }

            // Wall 51: a no-own-ctor class with a DYNAMIC / cross-module parent
            // (`class X extends _mod.Parent {}`, captured as `extends_expr`) that
            // the inline walk above could NOT resolve to a local/imported ctor
            // symbol (the auto-optimize / standalone build compiles each nested
            // module with the parent absent from `ctx.classes` /
            // `imported_class_ctors`, resolving it purely as a runtime dynamic
            // parent). Without an emitted super-call the parent ctor never runs
            // and inherited `this.<field> = …` writes are lost — Next.js route
            // matchers (`class PagesRouteMatcher extends _mod.RouteMatcher {}`)
            // left every `this.definition` undefined, so `matcher.definition
            // .pathname` threw. Forward this synthesized ctor's params to the
            // runtime dynamic-parent super dispatcher, mirroring the explicit
            // `Expr::SuperCall` dynamic-parent path in `expr/this_super_call.rs`.
            let parent_is_uncallable_builtin = class
                .extends_name
                .as_deref()
                .map(crate::expr::is_other_builtin_constructor_name)
                .unwrap_or(false)
                && class.extends_name.as_deref() != Some("SharedArrayBuffer");
            if builtin_parent_runtime.is_none()
                && class.extends_expr.is_some()
                && !parent_is_uncallable_builtin
            {
                if let Some(cid) = ctx.class_ids.get(&class.name).copied().filter(|c| *c != 0) {
                    let undef_lit =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let mut lowered_args: Vec<String> = Vec::with_capacity(method.params.len());
                    for p in &method.params {
                        if let Some(slot) = ctx.locals.get(&p.id).cloned() {
                            lowered_args.push(ctx.block().load(DOUBLE, &slot));
                        } else {
                            lowered_args.push(undef_lit.clone());
                        }
                    }
                    let parent_val = ctx.block().call(
                        DOUBLE,
                        "js_get_dynamic_parent_value",
                        &[(crate::types::I32, &cid.to_string())],
                    );
                    let (args_ptr, args_len) = if lowered_args.is_empty() {
                        ("null".to_string(), "0".to_string())
                    } else {
                        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, lowered_args.len());
                        for (i, a_val) in lowered_args.iter().enumerate() {
                            let slot =
                                ctx.block()
                                    .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                            ctx.block().store(DOUBLE, a_val, &slot);
                        }
                        let ptr_reg = ctx.block().next_reg();
                        ctx.block().emit_raw(format!(
                            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                            ptr_reg,
                            lowered_args.len(),
                            buf_reg
                        ));
                        (ptr_reg, lowered_args.len().to_string())
                    };
                    let this_box = match ctx.this_stack.last().cloned() {
                        Some(slot) => ctx.block().load(DOUBLE, &slot),
                        None => undef_lit.clone(),
                    };
                    let parent_result = ctx.block().call(
                        DOUBLE,
                        "js_fetch_or_value_super",
                        &[
                            (DOUBLE, &parent_val),
                            (DOUBLE, &this_box),
                            (crate::types::PTR, &args_ptr),
                            (I64, &args_len),
                        ],
                    );
                    if let Some(this_slot) = ctx.this_stack.last().cloned() {
                        let current_this = ctx.block().load(DOUBLE, &this_slot);
                        let bound_this = ctx.block().call(
                            DOUBLE,
                            "js_ctor_return_override",
                            &[
                                (DOUBLE, &current_this),
                                (DOUBLE, &parent_result),
                                (crate::types::I32, "0"),
                            ],
                        );
                        ctx.block().store(DOUBLE, &bound_this, &this_slot);
                    }
                }
            }

            // The synthesized default derived constructor has now completed
            // its implicit `super(...arguments)` path.  Publish that fact to
            // both this standalone function and any arrow closures before
            // evaluating the class's own instance fields.
            crate::expr::this_super_call::bind_derived_this_after_super(&mut ctx);

            // Apply self field initializers AFTER the parent body chain has
            // run, so they can read state set by the parent body (e.g. drizzle's
            // PgText.enumValues = this.config.enumValues — this.config is set
            // in Column body via super-chain). Refs #420.
            crate::lower_call::apply_field_initializers_recursive(
                &mut ctx,
                &class.name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )
            .with_context(|| {
                format!(
                    "applying self field initializers for '{}' constructor",
                    class.name
                )
            })?;
        }
    }

    // ECMAScript TDZ-on-`this`: a DERIVED constructor whose body never calls
    // `super()` leaves `this` uninitialized, so the implicit `return this`
    // throws ReferenceError. The inline `new` path enforces this in
    // `lower_new`; mirror it here for the standalone constructor-symbol path
    // — the DEFAULT when `force_ctor_call` routes `new C(...)` through the
    // shared `<class>_constructor` symbol instead of inlining. Without this,
    // `class A extends Array { constructor() {} }; new A()` constructs
    // silently instead of throwing. The predicate combination matches the
    // inline path verbatim (closure-`super()` without a direct `this` use
    // suppresses; a value-bearing `return` takes the return-override path).
    // Refs class/subclass/builtin-objects/*/super-must-be-called.
    let ctor_no_super_throw = is_constructor_method
        && (class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some())
        && class.constructor.as_ref().is_some_and(|ctor| {
            !crate::lower_call::ctor_body_calls_super(&ctor.body)
                && !(crate::lower_call::ctor_body_closure_calls_super(&ctor.body)
                    && !crate::lower_call::ctor_body_uses_this(&ctor.body))
                && !crate::lower_call::ctor_body_has_value_return(&ctor.body)
        });
    // Standalone constructor symbols use the same internal completion slot as
    // an inlined `new`: every explicit/bare return funnels to one block, where
    // constructor return-override semantics are applied against the CURRENT
    // `this` binding. This matters for a derived `super()` whose base returns
    // a replacement object — an implicit/bare return must publish that object
    // to the caller, not `undefined` (which would make the caller retain its
    // original pre-super allocation).
    let standalone_ctor_return = if is_constructor_method && !ctor_no_super_throw {
        let result_slot = ctx.func.alloca_entry(DOUBLE);
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.block().store(DOUBLE, &undef, &result_slot);
        let after_idx = ctx.new_block("standalone.ctor.return.after");
        let target = crate::expr::InlineCtorReturn {
            result_slot,
            after_label: ctx.block_label(after_idx),
            is_derived: class.extends.is_some()
                || class.extends_name.is_some()
                || class.native_extends.is_some()
                || class.extends_expr.is_some(),
        };
        ctx.inline_ctor_return.push(target.clone());
        Some((target, after_idx))
    } else {
        None
    };
    if ctor_no_super_throw {
        ctx.block()
            .call(DOUBLE, "js_throw_reference_error_this_before_super", &[]);
        ctx.block().unreachable();
    } else if method.is_async {
        stmt::lower_async_rejecting_stmts(&mut ctx, &method.body).with_context(|| {
            format!(
                "lowering async body of method '{}::{}'",
                class.name, method.name
            )
        })?;
    } else {
        stmt::lower_stmts(&mut ctx, &method.body).with_context(|| {
            format!("lowering body of method '{}::{}'", class.name, method.name)
        })?;
    }

    // #8648: pre-#8630 this symbol ended in `ret undefined`, and every caller
    // maps `undefined` onto its own receiver. Returning `this` instead flipped
    // the callers' `js_ctor_return_override` check from never-taken to
    // always-taken — a cross-crate call that runs the typed-array, buffer,
    // callable, Proxy, arguments and array probes before answering "yes, an
    // object" and handing back the value the caller already held. Publish only
    // when a replacement `this` can actually exist.
    let publishes_this = standalone_ctor_return.is_some()
        && crate::lower_call::ctor_chain_can_replace_this(ctx.classes, &class.name);
    if let Some((target, after_idx)) = standalone_ctor_return.as_ref() {
        let _ = ctx
            .inline_ctor_return
            .pop()
            .expect("standalone constructor return target");
        if !ctx.block().is_terminated() {
            ctx.block().br(&target.after_label);
        }
        ctx.current_block = *after_idx;
    }

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let return_value =
            if let Some((target, _)) = standalone_ctor_return.as_ref().filter(|_| publishes_this) {
                let raw = ctx.block().load(DOUBLE, &target.result_slot);
                let this_value = ctx
                    .this_stack
                    .last()
                    .cloned()
                    .map(|slot| ctx.block().load(DOUBLE, &slot))
                    .unwrap_or_else(|| undef.clone());
                crate::lower_call::emit_ctor_return_override(
                    &mut ctx,
                    &this_value,
                    &raw,
                    target.is_derived,
                )
            } else {
                undef.clone()
            };
        if ctx.shared_super_scope_active {
            ctx.block().call_void("js_derived_super_scope_pop", &[]);
        }
        if method.is_async {
            let handle = ctx
                .block()
                .call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
            ctx.block().ret(DOUBLE, &boxed);
        } else {
            ctx.block().ret(DOUBLE, &return_value);
        }
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
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [{} x i64] zeroinitializer",
            ic_name,
            crate::expr::property_get::generic_dispatch::PIC_CACHE_WORDS
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    // The Phase 5a and nonnegative-index clones are purely additive: the
    // public symbol (and its trampoline/forwarder, if any) belongs to the
    // primary invocation. Emitting it again here would define that symbol
    // twice.
    if let Some(param_index) = guarded_undefined_param.filter(|_| !guarded_undefined_clone) {
        emit_guarded_undefined(llmod, method, &family_name, &llvm_name, param_index);
    } else if !is_pshape_clone && !is_index_clone && !guarded_undefined_clone {
        if let Some(kind) = typed_public_trampoline {
            emit_public_typed(llmod, method, &public_llvm_name, &llvm_name, kind);
        } else if force_generic_body {
            emit_public_generic(llmod, method, &public_llvm_name, &llvm_name);
        }
    }
    Ok(())
}

/// Compile the internal typed-f64 clone for a conservatively eligible instance
/// method. The public/generic method body keeps the usual
/// `double(this, args...) -> double` ABI and remains the only symbol registered
/// in runtime vtables.
pub(super) fn compile_typed_f64_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_f64_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-f64 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_f64_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

/// Compile the internal typed-f64 receiver clone for an exact own instance
/// method. The clone takes a raw receiver handle (`i64`) plus raw numeric
/// method arguments; callers must compose the method-direct guard with raw-f64
/// class-field guards for every receiver field before entering it.
pub(super) fn compile_typed_f64_receiver_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
    receiver: &TypedReceiverMethodInfo,
    header_skip: u64,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_f64_receiver_method_name(&generic_name);
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((I64, "%this_obj".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_f64_receiver_body(blk, &method.params, &method.body, receiver, header_skip)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

/// Compile the internal typed-i1 clone for a conservatively eligible instance
/// method. Runtime vtables still register only the generic method symbol; this
/// clone is only called from guarded exact own-method sites.
pub(super) fn compile_typed_i1_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_i1_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-i1 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I1, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i1_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I1, &value);
    Ok(())
}

/// Compile the internal typed-i32 clone for a conservatively eligible instance
/// method. The public method symbol remains a JSValue trampoline registered in
/// the vtable; this clone is reached only after exact method and Int32 guards.
pub(super) fn compile_typed_i32_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_i32_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-i32 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I32, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i32_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I32, &value);
    Ok(())
}

/// Compile the internal typed-string clone for a conservatively eligible
/// instance method. The clone passes raw `StringHeader*` handles as i64; the
/// public method symbol remains a JSValue trampoline registered in vtables.
pub(super) fn compile_typed_string_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_string_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-string method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I64, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_string_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I64, &value);
    Ok(())
}

/// Compile a static class method as a top-level LLVM function with
/// no `this` parameter. Mostly identical to `compile_function` but
/// the LLVM symbol name is scoped by module, class id, class name, and
/// method name instead of `perry_fn_<modprefix>__<name>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_static_method(
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
    let shadow_slot_map = if super::helpers::precise_root_analysis_enabled() {
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
    super::arguments::add_arguments_mapped_boxes(&f.params, &mut static_boxed_vars);

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
        if super::helpers::precise_root_analysis_enabled() {
            blk.call_void(
                "js_shadow_slot_bind",
                &[(I32, &this_shadow_slot_idx.to_string()), (PTR, &this_slot)],
            );
        }
        let mut map = HashMap::new();
        for p in &f.params {
            let arg_name = format!("%arg{}", p.id);
            let slot = super::arguments::store_param_slot(blk, p, &static_boxed_vars, &arg_name);
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
        local_closure_param_counts: HashMap::new(),
        resolved_arrow_callback_targets: HashMap::new(),
        trusted_box_captures: false,
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
        class_keys_slots: HashMap::new(),
        class_shape_slots: HashMap::new(),
        class_header_images: HashMap::new(),
        cached_lengths: HashMap::new(),
        array_length_snapshots: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        masked_window_array_facts: Vec::new(),
        masked_region_scalar_locals: std::collections::HashSet::new(),
        suppressed_cleared_shadow_slots: std::collections::HashSet::new(),
        class_field_loop_facts: Vec::new(),
        element_shape_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
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
        i1_local_slots: HashMap::new(),
        index_used_locals: native_facts.index_used_locals(),
        strictly_i32_bounded_locals: native_facts.strictly_i32_bounded_locals(),
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        local_value_aliases: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
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
        typed_parse_rodata: Vec::new(),
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
        program_shadows_buffer_read_method: cross_module.program_shadows_buffer_read_method,
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
    super::arguments::materialize_arguments_object(
        &mut ctx,
        &f.params,
        super::arguments::ArgumentsCallee::Undefined,
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
    for ic_name in &ic_globals {
        llmod.add_raw_global(format!(
            "@{} = private global [{} x i64] zeroinitializer",
            ic_name,
            crate::expr::property_get::generic_dispatch::PIC_CACHE_WORDS
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}
