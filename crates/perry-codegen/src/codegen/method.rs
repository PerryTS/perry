//! Class-method and static-method compilation. Split out of
//! `codegen.rs` (now `codegen/mod.rs`).

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use perry_hir::Function;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};

use super::helpers::scoped_static_method_name;
use super::opts::CrossModuleCtx;
use super::typed_abi::{
    emit_typed_arg_guard, emit_typed_arg_to_raw, generic_method_body_name, lower_typed_f64_body,
    lower_typed_f64_receiver_body, lower_typed_i1_body, lower_typed_i32_body,
    lower_typed_string_body, typed_f64_method_name, typed_f64_receiver_method_name,
    typed_i1_method_name, typed_i32_method_name, typed_param_reps_for_params,
    typed_string_method_name, TypedFunctionTrampolineKind, TypedParamRep, TypedReceiverMethodInfo,
};

fn emit_typed_method_trampoline_fast_value(
    blk: &mut crate::block::LlBlock,
    kind: TypedFunctionTrampolineKind,
    typed_name: &str,
    arg_names: &[String],
    arg_reps: &[TypedParamRep],
) -> String {
    match kind {
        TypedFunctionTrampolineKind::F64 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            blk.call(DOUBLE, typed_name, &typed_args)
        }
        TypedFunctionTrampolineKind::I32 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let raw_i32 = blk.call(I32, typed_name, &typed_args);
            crate::expr::i32_to_nanbox(blk, &raw_i32)
        }
        TypedFunctionTrampolineKind::I1 => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let typed_i1 = blk.call(I1, typed_name, &typed_args);
            let typed_i32 = blk.zext(I1, &typed_i1, I32);
            crate::expr::i32_bool_to_nanbox(blk, &typed_i32)
        }
        TypedFunctionTrampolineKind::StringRef => {
            let raw_args: Vec<String> = arg_names
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| emit_typed_arg_to_raw(blk, *rep, arg))
                .collect();
            let typed_args: Vec<(LlvmType, &str)> = raw_args
                .iter()
                .zip(arg_reps.iter())
                .map(|(arg, rep)| (rep.llvm_ty(), arg.as_str()))
                .collect();
            let raw_string = blk.call(I64, typed_name, &typed_args);
            blk.call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)])
        }
    }
}

fn emit_public_typed_method_trampoline(
    llmod: &mut LlModule,
    method: &Function,
    public_name: &str,
    generic_body_name: &str,
    kind: TypedFunctionTrampolineKind,
) {
    let typed_name = match kind {
        TypedFunctionTrampolineKind::F64 => typed_f64_method_name(public_name),
        TypedFunctionTrampolineKind::I32 => typed_i32_method_name(public_name),
        TypedFunctionTrampolineKind::I1 => typed_i1_method_name(public_name),
        TypedFunctionTrampolineKind::StringRef => typed_string_method_name(public_name),
    };
    let arg_reps = match kind {
        TypedFunctionTrampolineKind::F64 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::F64; method.params.len()]),
        TypedFunctionTrampolineKind::I32 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::I32; method.params.len()]),
        TypedFunctionTrampolineKind::I1 => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::I1; method.params.len()]),
        TypedFunctionTrampolineKind::StringRef => typed_param_reps_for_params(&method.params)
            .unwrap_or_else(|| vec![TypedParamRep::StringRef; method.params.len()]),
    };
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let arg_names: Vec<String> = method
        .params
        .iter()
        .map(|p| format!("%arg{}", p.id))
        .collect();
    let wf = llmod.define_function(public_name, DOUBLE, params);
    let _ = wf.create_block("entry");

    let mut guard: Option<String> = None;
    {
        let blk = wf.block_mut(0).unwrap();
        for (arg, rep) in arg_names.iter().zip(arg_reps.iter()) {
            let ok = emit_typed_arg_guard(blk, *rep, arg);
            guard = Some(match guard {
                Some(prev) => blk.and(I1, &prev, &ok),
                None => ok,
            });
        }
    }

    let Some(guard) = guard else {
        let value = emit_typed_method_trampoline_fast_value(
            wf.block_mut(0).unwrap(),
            kind,
            &typed_name,
            &arg_names,
            &arg_reps,
        );
        wf.block_mut(0).unwrap().ret(DOUBLE, &value);
        return;
    };

    let fast_idx = wf.num_blocks();
    let fast_label = wf.create_block("typed_method_public.fast").label.clone();
    let fallback_idx = wf.num_blocks();
    let fallback_label = wf
        .create_block("typed_method_public.fallback")
        .label
        .clone();
    wf.block_mut(0)
        .unwrap()
        .cond_br(&guard, &fast_label, &fallback_label);

    let fast_value = emit_typed_method_trampoline_fast_value(
        wf.block_mut(fast_idx).unwrap(),
        kind,
        &typed_name,
        &arg_names,
        &arg_reps,
    );
    wf.block_mut(fast_idx).unwrap().ret(DOUBLE, &fast_value);

    let mut call_args: Vec<(LlvmType, &str)> = Vec::with_capacity(arg_names.len() + 1);
    call_args.push((DOUBLE, "%this_arg"));
    for arg in &arg_names {
        call_args.push((DOUBLE, arg.as_str()));
    }
    let fallback_value =
        wf.block_mut(fallback_idx)
            .unwrap()
            .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(fallback_idx)
        .unwrap()
        .ret(DOUBLE, &fallback_value);
}

fn emit_public_generic_method_forwarder(
    llmod: &mut LlModule,
    method: &Function,
    public_name: &str,
    generic_body_name: &str,
) {
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let wf = llmod.define_function(public_name, DOUBLE, params);
    let _ = wf.create_block("entry");
    let mut arg_names: Vec<String> = Vec::with_capacity(method.params.len() + 1);
    arg_names.push("%this_arg".to_string());
    for p in &method.params {
        arg_names.push(format!("%arg{}", p.id));
    }
    let call_args: Vec<(LlvmType, &str)> =
        arg_names.iter().map(|arg| (DOUBLE, arg.as_str())).collect();
    let value = wf
        .block_mut(0)
        .unwrap()
        .call(DOUBLE, generic_body_name, &call_args);
    wf.block_mut(0).unwrap().ret(DOUBLE, &value);
}

fn node_stream_parent_kind(
    classes: &HashMap<String, &perry_hir::Class>,
    class: &perry_hir::Class,
) -> Option<&'static str> {
    let mut cur = class.extends_name.as_deref();
    let mut depth = 0usize;
    while let Some(name) = cur {
        match name {
            "Readable" => return Some("readable"),
            "Duplex" => return Some("duplex"),
            "Transform" => return Some("transform"),
            _ => {}
        }
        cur = classes
            .get(name)
            .copied()
            .and_then(|parent| parent.extends_name.as_deref());
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    None
}

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
    module_global_types: &HashMap<u32, perry_types::Type>,
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
    let llvm_name = if typed_public_trampoline.is_some() || force_generic_body {
        generic_method_body_name(&public_llvm_name)
    } else {
        public_llvm_name.clone()
    };

    // Build the param list: (this, arg0, arg1, ...). All are doubles.
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    if typed_public_trampoline.is_some() || force_generic_body {
        lf.linkage = "internal".to_string();
    }
    let _ = lf.create_block("entry");

    let mut method_boxed_vars = module_boxed_vars.clone();
    super::arguments::add_arguments_mapped_boxes(&method.params, &mut method_boxed_vars);

    // Allocate slots for `this` and each parameter; pre-populate with
    // the incoming values.
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "%this_arg", &this_slot);
        let mut map = HashMap::new();
        for p in &method.params {
            let arg_name = format!("%arg{}", p.id);
            let slot = super::arguments::store_param_slot(blk, p, &method_boxed_vars, &arg_name);
            map.insert(p.id, slot);
        }
        (this_slot, map)
    };

    let mut local_types: HashMap<u32, perry_types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &method.params {
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
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        &method.body,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &method_boxed_vars,
        module_globals,
        classes,
        &cross_module.compile_time_constants,
    );

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
        current_block: 0,
        discard_expr_value: false,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_labels: Vec::new(),
        classes,
        this_stack: vec![this_slot],
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
        enums,
        is_async_fn: method.is_async,
        is_strict_fn: true,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
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
        option_object_locals: HashMap::new(),
        object_literal_locals: HashSet::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        namespace_member_origin_names: &cross_module.namespace_member_origin_names,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        local_generator_funcs: &cross_module.local_generator_funcs,
        funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        ffi_aliases: &cross_module.ffi_aliases,
        imported_class_sources: &cross_module.imported_class_sources,
        imported_class_original_names: &cross_module.imported_class_original_names,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: native_facts.integer_locals(),
        unsigned_i32_locals: native_facts.unsigned_i32_locals(),
        shadow_slot_map: std::collections::HashMap::new(),
        shadow_slot_clears_after_stmt: std::collections::HashMap::new(),
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        class_field_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
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
        scalar_ctor_target: Vec::new(),
        non_escaping_news: native_facts.non_escaping_news().clone(),
        non_escaping_new_used_fields: native_facts.non_escaping_new_used_fields().clone(),
        non_escaping_arrays: native_facts.non_escaping_arrays().clone(),
        non_escaping_array_used_indices: native_facts.non_escaping_array_used_indices().clone(),
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
        typed_f64_functions: &cross_module.typed_f64_functions,
        typed_i32_functions: &cross_module.typed_i32_functions,
        typed_string_functions: &cross_module.typed_string_functions,
        typed_i1_functions: &cross_module.typed_i1_functions,
        typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
        typed_f64_methods: &cross_module.typed_f64_methods,
        typed_i32_methods: &cross_module.typed_i32_methods,
        typed_i1_methods: &cross_module.typed_i1_methods,
        typed_string_methods: &cross_module.typed_string_methods,
        typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
        typed_f64_closures: &cross_module.typed_f64_closures,
        typed_i32_closures: &cross_module.typed_i32_closures,
        typed_i1_closures: &cross_module.typed_i1_closures,
        typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
        typed_string_closures: &cross_module.typed_string_closures,
        typed_string_closure_capture_counts: &cross_module.typed_string_closure_capture_counts,
        was_unrolled: method.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
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
        // Stage field initializers around the parent body chain so leaf
        // fields can read state set by parent body (Refs #420):
        //   - has extends: apply only ancestors here; self-fields apply
        //     later (after super() in own-body case, after explicit parent
        //     ctor call in no-own-body case).
        //   - no extends: apply all (= just self) here.
        let init_mode = if class.extends_name.is_some() {
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
        if class.constructor.is_none() && class.extends_name.is_some() {
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
                        let this_box = if let Some(slot) = this_slot {
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
                        let _ = ctx.block().call(DOUBLE, &ctor_sym, &ctor_args);
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
            if builtin_parent_runtime.is_none() && class.extends_expr.is_some() {
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
                    let _ = ctx.block().call(
                        DOUBLE,
                        "js_fetch_or_value_super",
                        &[
                            (DOUBLE, &parent_val),
                            (DOUBLE, &this_box),
                            (crate::types::PTR, &args_ptr),
                            (I64, &args_len),
                        ],
                    );
                }
            }

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

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if method.is_async {
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
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    if let Some(kind) = typed_public_trampoline {
        emit_public_typed_method_trampoline(llmod, method, &public_llvm_name, &llvm_name, kind);
    } else if force_generic_body {
        emit_public_generic_method_forwarder(llmod, method, &public_llvm_name, &llvm_name);
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
        lower_typed_f64_receiver_body(blk, &method.params, &method.body, receiver)?
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
        let mut map = HashMap::new();
        for p in &f.params {
            let arg_name = format!("%arg{}", p.id);
            let slot = super::arguments::store_param_slot(blk, p, &static_boxed_vars, &arg_name);
            map.insert(p.id, slot);
        }
        (this_slot, map)
    };

    let local_types: HashMap<u32, perry_types::Type> =
        f.params.iter().map(|p| (p.id, p.ty.clone())).collect();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        &f.body,
        &[],
        &flat_const_ids,
        &clamp_fn_ids,
        &cross_module.clamp3_functions,
        &static_boxed_vars,
        module_globals,
        classes,
        &cross_module.compile_time_constants,
    );

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
        current_block: 0,
        discard_expr_value: false,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_labels: Vec::new(),
        classes,
        this_stack: vec![this_slot],
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
        enums,
        is_async_fn: f.is_async,
        is_strict_fn: f.is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
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
        option_object_locals: HashMap::new(),
        object_literal_locals: HashSet::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        namespace_member_origin_names: &cross_module.namespace_member_origin_names,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        local_generator_funcs: &cross_module.local_generator_funcs,
        funcs_reading_dynamic_this: &cross_module.funcs_reading_dynamic_this,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        ffi_aliases: &cross_module.ffi_aliases,
        imported_class_sources: &cross_module.imported_class_sources,
        imported_class_original_names: &cross_module.imported_class_original_names,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: native_facts.integer_locals(),
        unsigned_i32_locals: native_facts.unsigned_i32_locals(),
        shadow_slot_map: std::collections::HashMap::new(),
        shadow_slot_clears_after_stmt: std::collections::HashMap::new(),
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        packed_f64_loop_facts: Vec::new(),
        class_field_loop_facts: Vec::new(),
        i32_counter_slots: HashMap::new(),
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
        scalar_ctor_target: Vec::new(),
        non_escaping_news: native_facts.non_escaping_news().clone(),
        non_escaping_new_used_fields: native_facts.non_escaping_new_used_fields().clone(),
        non_escaping_arrays: native_facts.non_escaping_arrays().clone(),
        non_escaping_array_used_indices: native_facts.non_escaping_array_used_indices().clone(),
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
        typed_f64_functions: &cross_module.typed_f64_functions,
        typed_i32_functions: &cross_module.typed_i32_functions,
        typed_string_functions: &cross_module.typed_string_functions,
        typed_i1_functions: &cross_module.typed_i1_functions,
        typed_i1_function_param_reps: &cross_module.typed_i1_function_param_reps,
        typed_f64_methods: &cross_module.typed_f64_methods,
        typed_i32_methods: &cross_module.typed_i32_methods,
        typed_i1_methods: &cross_module.typed_i1_methods,
        typed_string_methods: &cross_module.typed_string_methods,
        typed_i1_method_param_reps: &cross_module.typed_i1_method_param_reps,
        typed_f64_closures: &cross_module.typed_f64_closures,
        typed_i32_closures: &cross_module.typed_i32_closures,
        typed_i1_closures: &cross_module.typed_i1_closures,
        typed_i1_closure_param_reps: &cross_module.typed_i1_closure_param_reps,
        typed_string_closures: &cross_module.typed_string_closures,
        typed_string_closure_capture_counts: &cross_module.typed_string_closure_capture_counts,
        was_unrolled: f.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        native_arena_owner_aliases: HashMap::new(),
        native_arena_ambiguous_owner_aliases: HashSet::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
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
            "@{} = private global [2 x i64] zeroinitializer",
            ic_name
        ));
    }
    for raw in &typed_parse_rodata {
        llmod.add_raw_global(raw.clone());
    }
    Ok(())
}
