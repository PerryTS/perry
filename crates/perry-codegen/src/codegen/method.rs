//! Class-method and static-method compilation. Split out of
//! `codegen.rs` (now `codegen/mod.rs`).

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context, Result};
use perry_hir::Function;

use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::stmt;
use crate::strings::StringPool;
use crate::types::{LlvmType, DOUBLE, I64};

use super::helpers::sanitize;
use super::opts::CrossModuleCtx;

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
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;

    // Build the param list: (this, arg0, arg1, ...). All are doubles.
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((DOUBLE, "%this_arg".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    // Allocate slots for `this` and each parameter; pre-populate with
    // the incoming values.
    let (this_slot, locals): (String, HashMap<u32, String>) = {
        let blk = lf.block_mut(0).unwrap();
        let this_slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "%this_arg", &this_slot);
        let mut map = HashMap::new();
        for p in &method.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
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

    let method_boxed_vars = module_boxed_vars.clone();

    let clamp_fn_ids: std::collections::HashSet<u32> = cross_module
        .clamp3_functions
        .union(&cross_module.clamp_u8_functions)
        .chain(cross_module.returns_int_functions.iter())
        .copied()
        .collect();
    let flat_const_ids: std::collections::HashSet<u32> =
        cross_module.flat_const_arrays.keys().copied().collect();
    let hir_facts =
        crate::collectors::collect_hir_facts(&method.body, &flat_const_ids, &clamp_fn_ids);

    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &method.body,
        &method_boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_new_used_fields =
        crate::collectors::collect_non_escaping_new_used_fields(&method.body, &non_escaping_news);
    let non_escaping_arrays = crate::collectors::collect_non_escaping_arrays(
        &method.body,
        &method_boxed_vars,
        module_globals,
    );
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &method.body,
        &method_boxed_vars,
        module_globals,
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
        this_stack: vec![this_slot],
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
        static_field_globals,
        class_ids,
        class_keys_globals: &cross_module.class_keys_globals,
        imported_class_ctors: &cross_module.imported_class_ctors,
        func_signatures,
        func_synthetic_arguments,
        func_returns_class: &cross_module.func_returns_class,
        boxed_vars: method_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        local_closure_param_counts: HashMap::new(),
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
        shadow_slot_map: std::collections::HashMap::new(),
        shadow_slot_clears_after_stmt: std::collections::HashMap::new(),
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
        was_unrolled: method.was_unrolled,
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
            let mut effective_parent: Option<&str> = class.extends_name.as_deref();
            while let Some(pname) = effective_parent {
                let Some(pc) = ctx.classes.get(pname).copied() else {
                    break;
                };
                let has_local_body = pc.constructor.is_some();
                let has_imported_ctor = ctx.imported_class_ctors.contains_key(pname);
                if has_local_body || has_imported_ctor {
                    break;
                }
                effective_parent = pc.extends_name.as_deref();
            }
            if let Some(pname) = effective_parent {
                let pname_owned = pname.to_string();
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
                    } else if let Some((sym, n)) =
                        ctx.imported_class_ctors.get(&pname_owned).cloned()
                    {
                        (sym, n)
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
                } else if let Some((sym, n)) = ctx.imported_class_ctors.get(&pname_owned).cloned() {
                    (sym, n)
                } else {
                    ("".to_string(), 0)
                };
                if !ctor_sym.is_empty() {
                    let undef_lit =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
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
                    ctx.pending_declares.push((
                        ctor_sym.clone(),
                        crate::types::VOID,
                        ctor_param_types,
                    ));
                    ctx.block().call_void(&ctor_sym, &ctor_args);
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

    stmt::lower_stmts(&mut ctx, &method.body)
        .with_context(|| format!("lowering body of method '{}::{}'", class.name, method.name))?;

    if !ctx.block().is_terminated() {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.block().ret(DOUBLE, &undef);
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

/// Compile a static class method as a top-level LLVM function with
/// no `this` parameter. Mostly identical to `compile_function` but
/// the LLVM symbol name is `perry_static_<modprefix>__<class>__<method>`
/// instead of `perry_fn_<modprefix>__<name>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_static_method(
    llmod: &mut LlModule,
    class_name: &str,
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
    func_signatures: &HashMap<u32, (usize, bool, bool)>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    module_prefix: &str,
    module_boxed_vars: &std::collections::HashSet<u32>,
    closure_rest_params: &HashMap<u32, usize>,
    cross_module: &CrossModuleCtx,
) -> Result<()> {
    let llvm_name = format!(
        "perry_static_{}__{}__{}",
        module_prefix,
        sanitize(class_name),
        sanitize(&f.name),
    );

    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();

    let ic_base = llmod.ic_counter;
    let buffer_alias_base = llmod.buffer_alias_counter;
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    let _ = lf.create_block("entry");

    let locals: HashMap<u32, String> = {
        let blk = lf.block_mut(0).unwrap();
        let mut map = HashMap::new();
        for p in &f.params {
            let slot = blk.alloca(DOUBLE);
            blk.store(DOUBLE, &format!("%arg{}", p.id), &slot);
            map.insert(p.id, slot);
        }
        map
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
    let hir_facts = crate::collectors::collect_hir_facts(&f.body, &flat_const_ids, &clamp_fn_ids);

    let static_boxed_vars = module_boxed_vars.clone();
    let non_escaping_news = crate::collectors::collect_non_escaping_news(
        &f.body,
        &static_boxed_vars,
        module_globals,
        classes,
    );
    let non_escaping_new_used_fields =
        crate::collectors::collect_non_escaping_new_used_fields(&f.body, &non_escaping_news);
    let non_escaping_arrays =
        crate::collectors::collect_non_escaping_arrays(&f.body, &static_boxed_vars, module_globals);
    let non_escaping_object_literals = crate::collectors::collect_non_escaping_object_literals(
        &f.body,
        &static_boxed_vars,
        module_globals,
    );

    let mut ctx = FnCtx {
        func: lf,
        module_slug: crate::expr::native_region_slug(strings.module_prefix()),
        source_function: format!("{}.{}", class_name, f.name),
        source_function_slug: crate::expr::native_region_slug(&format!(
            "{}.{}",
            class_name, f.name
        )),
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
        // Static methods have no `this` but they CAN reference
        // sibling static methods/fields via the class name (which
        // they handle via StaticFieldGet/StaticMethodCall, not via
        // `this`). The class_stack is empty here.
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
        boxed_vars: static_boxed_vars,
        prealloc_boxes: std::collections::HashSet::new(),
        closure_rest_params,
        local_closure_func_ids: HashMap::new(),
        local_closure_param_counts: HashMap::new(),
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
        shadow_slot_map: std::collections::HashMap::new(),
        shadow_slot_clears_after_stmt: std::collections::HashMap::new(),
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
    stmt::lower_stmts(&mut ctx, &f.body)
        .with_context(|| format!("lowering body of static '{}::{}'", class_name, f.name))?;

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
