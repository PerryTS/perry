//! Closure-body compilation. Split out of `codegen.rs` (now
//! `codegen/mod.rs`). Only contains `compile_closure`.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};

use crate::collectors::{collect_let_ids, collect_ref_ids_in_stmts};
use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I32, I64};

use super::opts::CrossModuleCtx;

/// Compile a closure body as a top-level LLVM function.
///
/// Signature: `double perry_closure_<modprefix>__<func_id>(i64 this_closure,
/// double arg0, double arg1, …)`. The first parameter is the closure
/// pointer (raw i64); the remaining params are the closure's own
/// declared parameters.
///
/// Inside the body, captured variables (`closure.captures`) are mapped
/// to capture indices and accessed via the runtime
/// `js_closure_get/set_capture_f64(this_closure, idx)` calls. The
/// `closure_captures` field on `FnCtx` carries the LocalId → capture
/// index map; `current_closure_ptr` carries the closure pointer SSA
/// value name.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_closure(
    llmod: &mut LlModule,
    func_id: perry_types::FuncId,
    closure_expr: &perry_hir::Expr,
    func_names: &HashMap<u32, String>,
    strings: &mut StringPool,
    classes: &HashMap<String, &perry_hir::Class>,
    methods: &HashMap<(String, String), String>,
    module_globals: &HashMap<u32, String>,
    import_function_prefixes: &HashMap<String, String>,
    enums: &HashMap<(String, String), perry_hir::EnumValue>,
    static_field_globals: &HashMap<(String, String), String>,
    class_ids: &HashMap<String, u32>,
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    module_local_types: &HashMap<u32, perry_types::Type>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    // Destructure the closure expression. We trust that the caller
    // passes only `Expr::Closure` here (from `collect_closures_*`).
    let (
        params,
        body,
        captures,
        captures_this,
        captures_new_target,
        enclosing_class,
        is_async,
        is_strict,
    ) = match closure_expr {
        perry_hir::Expr::Closure {
            params,
            body,
            captures,
            captures_this,
            captures_new_target,
            enclosing_class,
            is_async,
            is_strict,
            ..
        } => (
            params,
            body,
            captures,
            *captures_this,
            *captures_new_target,
            enclosing_class.clone(),
            *is_async,
            *is_strict,
        ),
        _ => return Err(anyhow!("compile_closure: expected Expr::Closure")),
    };

    let llvm_name = format!("perry_closure_{}__{}", module_prefix, func_id);

    // Param list: i64 this_closure, then each param as double.
    let mut llvm_params: Vec<(LlvmType, String)> = Vec::with_capacity(params.len() + 1);
    llvm_params.push((I64, "%this_closure".to_string()));
    for p in params {
        llvm_params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, llvm_params);
    let _ = lf.create_block("entry");

    // Allocate slots for the closure's own params (captures don't get
    // alloca slots — they're accessed via the runtime).
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
    };

    // Start with the closure's own params as local_types, then
    // merge in the module-wide map so captured-from-outer ids have
    // their types available inside the body. Without this, closures
    // that capture an array `items` and do `items.length` miss the
    // typed fast path and return undefined.
    let mut local_types: HashMap<u32, perry_types::Type> =
        params.iter().map(|p| (p.id, p.ty.clone())).collect();
    for (id, ty) in module_local_types.iter() {
        local_types.entry(*id).or_insert_with(|| ty.clone());
    }

    // Build the capture map: each captured LocalId gets the index it
    // occupies in the closure's capture array. Identical logic to the
    // `compute_auto_captures` helper used by the closure creation site
    // — they MUST agree on the slot indices, otherwise the body reads
    // captures from the wrong slots. Sorting the auto-detected ids
    // gives deterministic indexing across both call sites.
    //
    // Filter module globals out of the explicit captures list — same
    // reason as in `compute_auto_captures` (closures auto-load module
    // globals through `@perry_global_*`). Without this, the body and
    // creation sites disagree on capture indices and a globalized
    // block-scoped let captured by a closure ends up with a
    // value-instead-of-box-pointer in its capture slot.
    let mut auto_captures: Vec<u32> = captures
        .iter()
        .copied()
        .filter(|id| !module_globals.contains_key(id))
        .collect();
    {
        let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_ref_ids_in_stmts(body, &mut referenced);
        let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
        collect_let_ids(body, &mut inner_lets);
        let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
        let already: std::collections::HashSet<u32> = auto_captures.iter().copied().collect();
        let mut sorted: Vec<u32> = referenced.into_iter().collect();
        sorted.sort();
        for id in sorted {
            if !param_ids.contains(&id)
                && !inner_lets.contains(&id)
                && !already.contains(&id)
                && !module_globals.contains_key(&id)
            {
                auto_captures.push(id);
            }
        }
    }
    let closure_captures: HashMap<u32, u32> = auto_captures
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i as u32))
        .collect();

    // `this` capture. Object-literal methods get `captures_this=true`
    // AND the creation site (lower_object_literal) patches a reserved
    // capture slot at index `auto_captures.len()` with the containing
    // object pointer. At function entry we read that slot and store it
    // into the `this` alloca so `Expr::This` loads the real receiver.
    //
    // Arrow-in-class leftover path (`enclosing_class.is_some()` without
    // the object-literal patch) keeps the old 0.0 sentinel — reads
    // return a bogus value but don't crash.
    let new_target_stack = if captures_new_target {
        let new_target_cap_idx = auto_captures.len() as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        let idx_str = new_target_cap_idx.to_string();
        let v = blk.call(
            DOUBLE,
            "js_closure_get_capture_f64",
            &[(I64, "%this_closure"), (I32, &idx_str)],
        );
        blk.store(DOUBLE, &v, &slot);
        vec![slot]
    } else {
        Vec::new()
    };

    let this_stack = if captures_this || enclosing_class.is_some() {
        let this_cap_idx = (auto_captures.len() + usize::from(captures_new_target)) as u32;
        let blk = lf.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        if captures_this {
            let idx_str = this_cap_idx.to_string();
            let v = blk.call(
                DOUBLE,
                "js_closure_get_capture_f64",
                &[(I64, "%this_closure"), (I32, &idx_str)],
            );
            blk.store(DOUBLE, &v, &slot);
        } else {
            blk.store(DOUBLE, "0.0", &slot);
        }
        vec![slot]
    } else {
        Vec::new()
    };
    let class_stack = match enclosing_class.clone() {
        Some(c) => vec![c],
        None => Vec::new(),
    };

    // Boxed vars inside the closure body: mutable captures from the
    // closure's own let-bindings. We don't add the captured-from-outer
    // ids here because those are already boxed in the outer function;
    // the closure body just sees them via the capture mechanism.
    let closure_boxed_vars = module_boxed_vars.clone();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let native_facts = crate::collectors::collect_native_region_fact_graph(
        body,
        &flat_const_ids,
        &clamp_fn_ids,
        &closure_boxed_vars,
        module_globals,
        classes,
        &cross_module.compile_time_constants,
    );

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("closure_{}", func_id),
        source_function_slug: crate::expr::native_region_slug(&format!("closure_{}", func_id)),
        active_region_id: None,
        locals,
        local_types,
        current_block: 0,
        discard_expr_value: false,
        func_names,
        strings,
        loop_targets: Vec::new(),
        label_targets: HashMap::new(),
        pending_label: None,
        classes,
        this_stack,
        new_target_stack,
        class_stack,
        methods,
        module_globals,
        import_function_prefixes,
        import_function_origin_names: &cross_module.import_function_origin_names,
        import_function_v8_specifiers: &cross_module.import_function_v8_specifiers,
        // Issue #841: node:submodule named-import + namespace registries.
        import_function_node_submodule: &cross_module.import_function_node_submodule,
        namespace_node_submodules: &cross_module.namespace_node_submodules,
        namespace_v8_specifiers: &cross_module.namespace_v8_specifiers,
        closure_captures,
        current_closure_ptr: Some("%this_closure".to_string()),
        enums,
        // Async closures (arrow functions declared `async () => ...`)
        // must wrap their return values in `js_promise_resolved` so the
        // call site sees a NaN-boxed Promise pointer — same contract as
        // regular async functions. Consumers like the Fastify server
        // runtime inspect the returned value with `js_is_promise` and
        // break if a raw object pointer (or any non-Promise) is handed
        // back. Issue #125.
        is_async_fn: is_async,
        is_strict_fn: is_strict,
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        class_field_counts: &cross_module.class_field_counts,
        class_init_chains: &cross_module.class_init_chains,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: closure_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        local_closure_param_counts: HashMap::new(),
        option_object_locals: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        imported_func_synthetic_arguments: &cross_module.imported_func_synthetic_arguments,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
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
        i32_counter_slots: HashMap::new(),
        index_used_locals: native_facts.index_used_locals(),
        strictly_i32_bounded_locals: native_facts.strictly_i32_bounded_locals(),
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
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
        non_escaping_object_literals: native_facts.non_escaping_object_literals().clone(),
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        integer_returning_functions: &cross_module.returns_int_functions,
        i32_identity_functions: &cross_module.i32_identity_functions,
        was_unrolled: false,
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

    if is_async {
        stmt::lower_async_rejecting_stmts(&mut ctx, body)
            .with_context(|| format!("lowering async closure body func_id={}", func_id))?;
    } else {
        stmt::lower_stmts(&mut ctx, body)
            .with_context(|| format!("lowering closure body func_id={}", func_id))?;
    }

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        if is_async {
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
