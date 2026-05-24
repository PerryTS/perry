//! User-function compilation. Split out of `codegen.rs` (now
//! `codegen/mod.rs`). Behavior is unchanged — this only contains
//! `compile_function`.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use perry_hir::Function;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::native_value::{AliasState, BufferElem, BufferViewSlot, LengthSource};
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I32, I64, I8, PTR};

use super::helpers::shadow_stack_enabled;
use super::opts::CrossModuleCtx;

/// Compile a single user function into the module.
pub(super) fn compile_function(
    llmod: &mut LlModule,
    f: &Function,
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
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = func_names
        .get(&f.id)
        .cloned()
        .ok_or_else(|| anyhow!("function name not resolved for {}", f.name))?;

    // Phase A assumes all user-function params are `double`. Parameter
    // registers are named `%arg{LocalId}` so the body can store them into
    // alloca slots keyed by the same HIR LocalId.
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);

    // Gen-GC Phase A sub-phase 3a: opt-in shadow-frame emission
    // for user functions. Pointer-typed param + local slots are
    // assigned pre-lowering via `collect_pointer_typed_locals`;
    // the frame is sized to hold all of them. Sub-phase 3b emits
    // the slot-set calls at Let/LocalSet sites to actually
    // populate the frame with live values; today the slots stay
    // zero (the tracer doesn't consume them yet — Phase A ship
    // criterion is "shadow stack is built but not yet consumed").
    let shadow_slot_map = if shadow_stack_enabled() {
        let flat_const_ids: std::collections::HashSet<u32> =
            cross_module.flat_const_arrays.keys().copied().collect();
        let m =
            crate::collectors::collect_pointer_typed_locals(&f.params, &f.body, &flat_const_ids);
        lf.enable_shadow_frame(m.len() as u32);
        m
    } else {
        std::collections::HashMap::new()
    };
    let shadow_slot_clears_after_stmt =
        crate::collectors::collect_shadow_slot_clear_points(&f.body, &shadow_slot_map);

    // Small leaf functions (≤ 8 statements) get alwaysinline so LLVM
    // exposes their operations to the caller's optimizer context — critical
    // for vectorizing clamp helpers and similar patterns. Excluded:
    // async/generator functions, AND functions rewritten by the
    // async-to-generator pre-pass (was_plain_async=true). Inlining the
    // rewritten wrapper into its caller breaks GC-root coverage of the
    // step closure's iter capture, hanging async chains (issue #447).
    if f.body.len() <= 8 && !f.is_async && !f.is_generator && !f.was_plain_async {
        lf.force_inline = true;
    }
    let _ = lf.create_block("entry");

    // Store each param into an alloca slot, collecting LocalId → slot
    // mappings. We release the &mut LlBlock at scope end before handing
    // the function over to the FnCtx lowering pass.
    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            if let Some(slot_idx) = shadow_slot_map.get(&p.id).copied() {
                blk.call_void(
                    "js_shadow_slot_bind",
                    &[(I32, &slot_idx.to_string()), (PTR, &slot)],
                );
            }
            map.insert(p.id, slot);
        }
        map
    };

    // Param types feed local_types so type-aware dispatch (e.g. string
    // concat detection on a `: string` parameter) works inside the body.
    // Also seed with module global types so functions that access module
    // globals see the correct declared types (e.g., Named("Editor")).
    let mut local_types: HashMap<u32, perry_types::Type> = module_global_types
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for p in &f.params {
        local_types.insert(p.id, p.ty.clone());
    }

    // Pre-walk: which locals need to be boxed? A local is boxed when
    // it's captured by a closure AND written by someone (either the
    // enclosing function or inside a closure). Box-backing lets multiple
    // closures share the same mutable cell — critical for the common
    // `let x = 0; return { get: () => x, set: (n) => x = n }` pattern.
    let boxed_vars = module_boxed_vars.clone();

    // Pre-walk: which locals are provably integer-valued? Used by
    // `BinaryOp::Mod` to emit integer modulo instead of libm `fmod()`.
    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let hir_facts = crate::collectors::collect_hir_facts(&f.body, &flat_const_ids, &clamp_fn_ids);

    // Pre-walk: which `let x = new Class(...)` locals never escape?
    let non_escaping_news =
        crate::collectors::collect_non_escaping_news(&f.body, &boxed_vars, module_globals, classes);
    let non_escaping_new_used_fields =
        crate::collectors::collect_non_escaping_new_used_fields(&f.body, &non_escaping_news);
    let non_escaping_arrays =
        crate::collectors::collect_non_escaping_arrays(&f.body, &boxed_vars, module_globals);
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body,
        &boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: f.name.clone(),
        source_function_slug: crate::expr::native_region_slug(&f.name),
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
        this_stack: Vec::new(),
        class_stack: Vec::new(),
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
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        namespace_imports: &cross_module.namespace_imports,
        namespace_reexport_named_imports: &cross_module.namespace_reexport_named_imports,
        namespace_member_prefixes: &cross_module.namespace_member_prefixes,
        imported_async_funcs: &cross_module.imported_async_funcs,
        local_async_funcs: &cross_module.local_async_funcs,
        type_aliases: &cross_module.type_aliases,
        imported_func_param_counts: &cross_module.imported_func_param_counts,
        imported_func_has_rest: &cross_module.imported_func_has_rest,
        method_param_counts: &cross_module.method_param_counts,
        method_has_rest: &cross_module.method_has_rest,
        imported_func_return_types: &cross_module.imported_func_return_types,
        ffi_signatures: &cross_module.ffi_signatures,
        imported_class_sources: &cross_module.imported_class_sources,
        interfaces: &cross_module.interfaces,
        try_depth: 0,
        pending_declares: Vec::new(),
        integer_locals: &hir_facts.integer_locals,
        unsigned_i32_locals: &hir_facts.unsigned_i32_locals,
        shadow_slot_map,
        shadow_slot_clears_after_stmt,
        arena_state_slot: None,
        class_keys_slots: HashMap::new(),
        cached_lengths: HashMap::new(),
        bounded_index_pairs: Vec::new(),
        i32_counter_slots: HashMap::new(),
        index_used_locals: &hir_facts.index_used_locals,
        strictly_i32_bounded_locals: &hir_facts.strictly_i32_bounded_locals,
        i18n: &cross_module.i18n,
        dynamic_import_path_to_prefix: &cross_module.dynamic_import_path_to_prefix,
        local_class_aliases: HashMap::new(),
        local_class_field_aliases: HashMap::new(),
        local_id_to_name: HashMap::new(),
        imported_vars: &cross_module.imported_vars,
        compile_time_constants: &cross_module.compile_time_constants,
        app_metadata: &cross_module.app_metadata,
        scalar_replaced: std::collections::HashMap::new(),
        scalar_replaced_arrays: std::collections::HashMap::new(),
        scalar_ctor_target: Vec::new(),
        non_escaping_news,
        non_escaping_new_used_fields,
        non_escaping_arrays,
        non_escaping_object_literals,
        flat_const_arrays: &cross_module.flat_const_arrays,
        array_row_aliases: HashMap::new(),
        clamp3_functions: &cross_module.clamp3_functions,
        clamp_u8_functions: &cross_module.clamp_u8_functions,
        integer_returning_functions: &cross_module.returns_int_functions,
        i32_identity_functions: &cross_module.i32_identity_functions,
        was_unrolled: f.was_unrolled,
        ic_site_counter: ic_base,
        ic_globals: Vec::new(),
        typed_parse_rodata: Vec::new(),
        typed_parse_counter: 0,
        buffer_data_slots: HashMap::new(),
        buffer_view_slots: HashMap::new(),
        disable_buffer_fast_path: cross_module.disable_buffer_fast_path,
        min_length_bounds: HashMap::new(),
        bounded_buffer_index_pairs: Vec::new(),
        buffer_hazard_reasons: HashMap::new(),
        native_i32_aliases: HashMap::new(),
        int_range_aliases: HashMap::new(),
        int_range_facts: Vec::new(),
        next_loop_proof_scope_id: 0,
        nonnegative_integer_locals: HashSet::new(),
        native_rep_records: Vec::new(),
        known_noalias_buffer_locals: &hir_facts.known_noalias_buffer_locals,
        buffer_alias_base,
    };

    // Issue #92 follow-up: pre-register `buffer_data_slots` entries for
    // `Buffer`-typed function parameters so that the readInt32BE/etc.
    // intrinsic fast path in `lower_call.rs` fires on
    // `function decode(row: Buffer) { row.readInt32BE(off) }` — the real
    // Postgres-driver hot-path shape, not just the `const buf = Buffer.alloc(N)`
    // micro-benchmark. Skipped when the param is reassigned (has_any_mutation
    // covers LocalSet/Update/ARRAY_MUTATORS — `buf = ...`, `buf.fill(...)` etc.)
    // because a cached data_ptr would go stale, and skipped for boxed params
    // (same reason via cross-closure mutation). Uint8Array-typed params are
    // deliberately excluded: a pre-existing crash surfaces when the same
    // program defines both a Buffer-param and a Uint8Array-param function and
    // then invokes them in sequence (reproducible on main without any of
    // this extension's changes). Tracked separately; Buffer coverage alone
    // hits the Postgres decode path which is the target workload here.
    for p in &f.params {
        let is_buffer_typed = matches!(
            &p.ty,
            perry_types::Type::Named(n) if n == "Buffer"
        );
        if !is_buffer_typed {
            continue;
        }
        if ctx.boxed_vars.contains(&p.id) {
            continue;
        }
        if crate::collectors::has_any_mutation(&f.body, p.id) {
            continue;
        }
        let Some(param_slot) = ctx.locals.get(&p.id).cloned() else {
            continue;
        };
        let blk = ctx.block();
        let arg_val = blk.load(DOUBLE, &param_slot);
        let handle = crate::expr::unbox_to_i64(blk, &arg_val);
        let handle_ptr = blk.inttoptr(I64, &handle);
        let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
        let buf_slot = ctx.func.alloca_entry(PTR);
        ctx.block().store(PTR, &data_ptr, &buf_slot);
        let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
        ctx.buffer_data_slots
            .insert(p.id, (buf_slot.clone(), scope_idx));
        ctx.buffer_view_slots.insert(
            p.id,
            BufferViewSlot {
                data_slot: buf_slot,
                scope_idx: Some(scope_idx),
                elem: BufferElem::U8,
                alias: AliasState::Unknown,
                length_source: Some(LengthSource::Unknown),
            },
        );
    }

    stmt::lower_top_level_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of '{}'", f.name))?;

    // A function that falls off the end without an explicit `return`
    // returns `undefined` in JS — emit the NaN-boxed TAG_UNDEFINED
    // value so the LLVM verifier has a terminator AND user code that
    // does `f() === undefined` / `f() !== undefined` observes the
    // correct value. For async functions, wrap undefined in a
    // resolved promise so callers can await the result.
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
    drop(ctx); // releases &mut LlFunction borrow on llmod
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
