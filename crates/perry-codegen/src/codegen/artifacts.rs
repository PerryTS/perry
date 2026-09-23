//! Emits `compile_module` artifacts: closures, classes, wrappers, namespace
//! globals, the module entry function, and string-pool initialization.
//! Split from `codegen/mod.rs` to keep the compiler pipeline navigable.

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::types::{LlvmType, DOUBLE, I64, VOID};

use super::artifact_context::{ModuleArtifactsCtx, OptsView};
use super::class_artifacts::{emit_class_artifacts, ClassArtifactsCtx};
use super::closure::{
    compile_closure, compile_typed_f64_closure, compile_typed_i1_closure,
    compile_typed_i32_closure, compile_typed_string_closure,
};
use super::ctor_arity::synthesized_ctor_param_count;
use super::entry::compile_module_entry;
use super::export_value_wrappers::{emit_export_value_wrappers, ExportValueWrapperCtx};
use super::helpers::{
    function_body_returns_generator_object, namespace_live_getter_wrapper_symbol, sanitize,
    unknown_func_wrapper_name,
};
use super::native_namespace_exports::emit_native_namespace_reexport_getters;
use super::spec_function_length;
use super::string_pool::emit_string_pool;

/// Emit the artifact tail: bodies, wrappers, namespace globals, entry
/// function, string pool. Mirrors the in-prelude execution order of
/// the original `compile_module`.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_module_artifacts(c: ModuleArtifactsCtx<'_>) -> Result<()> {
    // Destructure so the verbatim block below reads against the
    // original local names. `llmod` / `strings` are `&mut` bindings
    // (auto-reborrowed on each per-function call site below); the
    // rest are shared borrows.
    let ModuleArtifactsCtx {
        progress,
        llmod,
        target_triple,
        strings,
        hir,
        import_function_prefixes,
        imported_classes,
        constructor_param_counts,
        is_entry_module,
        non_entry_module_prefixes,
        output_type,
        module_prefix,
        class_table,
        class_ids,
        enum_table,
        module_globals,
        module_global_types,
        static_field_globals,
        method_names,
        func_names,
        func_signatures,
        func_synthetic_arguments,
        module_boxed_vars,
        module_local_types,
        module_receiver_types,
        closure_rest_params,
        closure_synthetic_arguments,
        closure_rest_and_arguments,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
        trusted_box_closures,
        versioned_loop_callbacks,
        closures,
        class_keys_init_data,
        class_header_image_inits,
        imported_class_stubs,
        cross_module,
    } = c;

    // Local aliases so the verbatim moved block (which referenced
    // `opts.X`) still reads naturally without an `opts` borrow we
    // can't take after the prelude moved fields into `cross_module`.
    // Each alias is a stable view of the same data the original
    // `&opts.X` would have produced.
    let opts = OptsView {
        import_function_prefixes,
        imported_classes,
        constructor_param_counts,
        is_entry_module,
        non_entry_module_prefixes,
        output_type,
    };

    let module_reassigned_locals = crate::collectors::reassigned_locals_in_module(hir);
    // #9071 follow-up: module-wide GUARD-FREE closure bindings. #7170 R1's
    // `single_binding_closure_locals` is the strong fact: exactly one `Let`,
    // never written at any depth in any body, never rebound by a parameter or
    // catch clause — "a call through this binding names that closure" with
    // `Expr::FuncRef` strength, which is why the function-body pipeline
    // already devirtualizes (and LLVM then folds) these calls. Closure bodies
    // get the same treatment through this map: seeded into the known-func_id
    // path, and the identity guard skipped outright.
    let closure_param_counts: std::collections::HashMap<u32, usize> = closures
        .iter()
        .filter_map(|(func_id, expr)| match expr {
            perry_hir::Expr::Closure { params, .. } => Some((*func_id, params.len())),
            _ => None,
        })
        .collect();
    // `PERRY_CALL_DEVIRT=0`/`off`/`false` empties the map, restoring the
    // entry-resolved indirect path for every binding (A/B bisection).
    let call_devirt_enabled = !matches!(
        std::env::var("PERRY_CALL_DEVIRT").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    );
    let immutable_closure_bindings: std::collections::HashMap<u32, (u32, usize)> =
        crate::collectors::spec_abi_sites::single_binding_closure_locals(hir)
            .into_iter()
            // A closure with a trusted-box clone is better served by the
            // ENTRY-RESOLVED path: `js_closure_resolve_arrow_direct_call`
            // hands back the trusted clone with its entry-cached box-capture
            // pointers, which beats the known arm's public/typed call for
            // capturing bodies (measured: 2.5 vs 5.1 ns). Seeding such an id
            // would also make the resolution emitter skip it, robbing the
            // call of the faster path.
            .filter(|_| call_devirt_enabled)
            .filter(|(_, func_id)| !trusted_box_closures.contains_key(func_id))
            .filter_map(|(id, func_id)| {
                closure_param_counts
                    .get(&func_id)
                    .map(|count| (id, (func_id, *count)))
            })
            .collect();
    progress.checkpoint("reassigned-local analysis");

    let closure_started = Instant::now();
    let closure_progress_step = (closures.len() / 20).max(1);
    for (closure_index, (func_id, closure_expr)) in closures.iter().enumerate() {
        if cross_module.typed_f64_closures.contains(func_id) {
            compile_typed_f64_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-f64 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_i1_closures.contains(func_id) {
            compile_typed_i1_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-i1 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_i32_closures.contains(func_id) {
            compile_typed_i32_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-i32 closure clone func_id={}", func_id))?;
        }
        if cross_module.typed_string_closures.contains(func_id) {
            compile_typed_string_closure(
                llmod,
                *func_id,
                closure_expr,
                module_prefix,
                module_local_types,
            )
            .with_context(|| format!("lowering typed-string closure clone func_id={}", func_id))?;
        }
        compile_closure(
            llmod,
            *func_id,
            closure_expr,
            func_names,
            strings,
            class_table,
            method_names,
            module_globals,
            opts.import_function_prefixes,
            enum_table,
            static_field_globals,
            class_ids,
            func_signatures,
            func_synthetic_arguments,
            module_prefix,
            module_boxed_vars,
            module_receiver_types,
            &module_reassigned_locals,
            &immutable_closure_bindings,
            closure_rest_params,
            cross_module,
            false,
            false,
        )
        .with_context(|| format!("lowering closure func_id={}", func_id))?;
        if trusted_box_closures.contains_key(func_id) {
            compile_closure(
                llmod,
                *func_id,
                closure_expr,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_prefix,
                module_boxed_vars,
                module_receiver_types,
                &module_reassigned_locals,
                &immutable_closure_bindings,
                closure_rest_params,
                cross_module,
                true,
                false,
            )
            .with_context(|| format!("lowering trusted-box closure func_id={}", func_id))?;
        }
        if versioned_loop_callbacks.contains(func_id) {
            compile_closure(
                llmod,
                *func_id,
                closure_expr,
                func_names,
                strings,
                class_table,
                method_names,
                module_globals,
                opts.import_function_prefixes,
                enum_table,
                static_field_globals,
                class_ids,
                func_signatures,
                func_synthetic_arguments,
                module_prefix,
                module_boxed_vars,
                module_receiver_types,
                &module_reassigned_locals,
                &immutable_closure_bindings,
                closure_rest_params,
                cross_module,
                true,
                true,
            )
            .with_context(|| format!("lowering versioned-loop closure func_id={}", func_id))?;
        }
        let done = closure_index + 1;
        if done == closures.len() || done % closure_progress_step == 0 {
            progress.items("closure bodies", done, closures.len(), closure_started);
        }
    }
    progress.checkpoint("closure bodies");

    emit_class_artifacts(ClassArtifactsCtx {
        progress,
        llmod,
        target_triple,
        strings,
        hir,
        module_prefix,
        class_table,
        class_ids,
        enum_table,
        module_globals,
        module_global_types,
        static_field_globals,
        method_names,
        func_names,
        func_signatures,
        func_synthetic_arguments,
        module_boxed_vars,
        closure_rest_params,
        imported_class_stubs,
        opts,
        cross_module,
    })?;

    progress.checkpoint("class methods, constructors, and statics");

    emit_export_value_wrappers(ExportValueWrapperCtx {
        llmod,
        strings,
        hir,
        func_names,
        module_prefix,
    });

    progress.checkpoint("top-level function value wrappers and aliases");

    // Issue #774: emit closure-call wrappers for class instance methods
    // so `Expr::SuperPropertyGet` (value-form `super.<method>`) can
    // materialize them via `js_closure_alloc_singleton(@__perry_wrap_<method>)`.
    // Methods have signature `perry_method_<...>(this_box, args...)`;
    // the closure-call ABI is `(i64 closure, double a0, ...)` and
    // doesn't carry a separate `this`. The receiver therefore comes from
    // IMPLICIT_THIS, set by the dispatcher right before the call:
    //   * A method-style invocation of a stored super-method value
    //     (`this._complete = super._complete; obj._complete()` — rxjs's
    //     `OperatorSubscriber` forwarding `complete`/`error`/`next` to the
    //     base `Subscriber` when no override callback was supplied) sets
    //     IMPLICIT_THIS to the receiver, so the base method runs with the
    //     right `this`. Pre-fix this hardcoded `this=undefined`, so the
    //     forwarded `complete` never reached `this.destination.complete()`
    //     and the pipeline stalled (top-level await never settled, #5138).
    //   * A bare call (`const fn = super.greet; fn(x)`) leaves
    //     IMPLICIT_THIS undefined, matching strict-mode `this`.
    let mut emitted_wrappers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for class in &hir.classes {
        for method in &class.methods {
            let Some(method_fn_name) = method_names
                .get(&(class.name.clone(), method.name.clone()))
                .cloned()
            else {
                continue;
            };
            let wrap_name = format!("__perry_wrap_{}", method_fn_name);
            if !emitted_wrappers.insert(wrap_name.clone()) {
                continue;
            }
            let arity = method.params.len();
            let mut wrap_params: Vec<(LlvmType, String)> = vec![(I64, "%this_closure".to_string())];
            for i in 0..arity {
                wrap_params.push((DOUBLE, format!("%a{}", i)));
            }
            let wf = llmod.define_function(&wrap_name, DOUBLE, wrap_params);
            let _ = wf.create_block("entry");
            let blk = wf.block_mut(0).unwrap();
            // Forward the call-site receiver (IMPLICIT_THIS) as `this`,
            // then the args. See the block comment above (#5138).
            let this_box = blk.call(DOUBLE, "js_implicit_this_get", &[]);
            let mut call_args: Vec<(LlvmType, String)> = Vec::with_capacity(arity + 1);
            call_args.push((DOUBLE, this_box));
            for i in 0..arity {
                call_args.push((DOUBLE, format!("%a{}", i)));
            }
            let call_args_ref: Vec<(LlvmType, &str)> =
                call_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
            let result = blk.call(DOUBLE, &method_fn_name, &call_args_ref);
            blk.ret(DOUBLE, &result);
        }
    }

    // #337: emit an always-defined fallback wrapper for the module-scoped
    // `perry_unknown_func_<module>` sentinel. `expr.rs::Expr::FuncRef` falls
    // through to the matching wrapper when the
    // FuncRef's id isn't in `func_names` (cross-module reference whose
    // Source HIR wasn't lowered into THIS LLVM module — should normally
    // route to ExternFuncRef instead, but some HIR shapes still emit
    // FuncRef with an unresolvable id). Before #337, the wrapper was never
    // defined and clang rejected the unresolved reference during IR
    // validation. This stub returns TAG_UNDEFINED
    // (encoded as `f64::from_bits(0x7FFC_0000_0000_0001)` =
    // 1.7800590868057611e-307 — the canonical undefined sentinel matching
    // `value::TAG_UNDEFINED`); any runtime-side `js_closure_callN`
    // through this wrapper just observes "the callable returned
    // undefined", which is the right fail-closed behavior matching how
    // the `__perry_wrap_extern_*` wrappers handle missing extern
    // imported classes (see the existing comment block below).
    //
    // Emitted unconditionally — link-time DCE strips it when no
    // `Expr::FuncRef(unknown_id)` site exists in this module. The stable
    // module suffix is load-bearing under codegen-unit splitting: splitting
    // promotes this internal definition for cross-unit calls, and a shared
    // final link may contain split objects from many source modules (#8064).
    {
        let wrap_name = unknown_func_wrapper_name(module_prefix);
        let wf = llmod.define_function(
            &wrap_name,
            DOUBLE,
            vec![
                (I64, "%this_closure".to_string()),
                (DOUBLE, "%a0".to_string()),
                (DOUBLE, "%a1".to_string()),
                (DOUBLE, "%a2".to_string()),
                (DOUBLE, "%a3".to_string()),
                (DOUBLE, "%a4".to_string()),
            ],
        );
        // Fix #420 (v0.5.576): internal linkage keeps unsplit modules' copies
        // dead-code-eliminable. Split units promote the definition so calls
        // from sibling units can bind; #8064's module-scoped name keeps those
        // promoted copies distinct at the application link.
        wf.linkage = "internal".to_string();
        let _ = wf.create_block("entry");
        let blk = wf.block_mut(0).unwrap();
        // f64::from_bits(0x7FFC_0000_0000_0001) — TAG_UNDEFINED. Format
        // identical to how other expr.rs lowerings emit it.
        blk.ret(DOUBLE, "0x7FFC000000000001");
    }

    emit_native_namespace_reexport_getters(llmod, hir, module_prefix);

    // Imported function values now materialize the source module's canonical
    // wrapper through `js_closure_alloc_singleton` in ExternFuncRef lowering.
    // The former consumer-local `__perry_wrap_extern_*` globals are no longer
    // referenced; omitting them also avoids creating undefined runtime symbols
    // for imports that survive only as cross-module TypeScript metadata.
    progress.checkpoint("method, fallback, and imported function wrappers");

    // Issue #100: emit the per-module `@__perry_ns_<prefix>` global iff
    // this module is the target of at least one dynamic `import()` site
    // anywhere in the program. Defined here with external linkage so the
    // consumer-side `Expr::DynamicImport` lowering (which may live in a
    // different LLVM module) can `load double, ptr @__perry_ns_<prefix>`.
    // Initialized to NaN-boxed `undefined`; the populator at the end of
    // `__perry_init_<prefix>` overwrites this with the real namespace
    // object built via `js_create_namespace`. Registered as a GC root
    // (same as every other module global) so the underlying ObjectHeader
    // survives sweeps after init returns.
    // Pre-emit each export-key string constant at module level. We add
    // them BEFORE compile_module_entry so the populator can reference
    // them by their stable `.str.N` global name. Stored alongside the
    // entries so the populator emit-site can look up `(global_name,
    // byte_len)` per key without rebuilding any LLVM IR. Vec entries
    // are parallel to `cross_module.namespace_entries`.
    let mut namespace_key_globals: Vec<(String, usize)> = Vec::new();
    // Issue #842: also emit `@__perry_ns_<prefix>` for side-effect-only
    // dynamic-import targets (`namespace_entries` empty but the module
    // is still a target). Without this, the consumer-side extern
    // declaration links to nothing. The empty-entries case still emits
    // the global; the populator below handles `n == 0` by calling
    // `js_create_namespace(0, ...)` which returns an empty object.
    if !cross_module.namespace_entries.is_empty() || cross_module.is_dynamic_import_target {
        let ns_name = format!("__perry_ns_{}", module_prefix);
        // Hex double literal for TAG_UNDEFINED (0x7FFC_0000_0000_0001).
        // #10399: namespace objects are built by module init.
        llmod.add_module_state_global(&ns_name, DOUBLE, "0x7FFC000000000001");
        for (entry_index, entry) in cross_module.namespace_entries.iter().enumerate() {
            let (gname, byte_len) = llmod.add_string_constant(&entry.name);
            namespace_key_globals.push((gname, byte_len));

            let wrapper_name = namespace_live_getter_wrapper_symbol(module_prefix, entry_index);
            let getter_name = match &entry.kind {
                crate::NamespaceEntryKind::LocalVar { global_name } => {
                    let wrapper = llmod.define_function(
                        &wrapper_name,
                        DOUBLE,
                        vec![(I64, "%this_closure".to_string())],
                    );
                    let _ = wrapper.create_block("entry");
                    let blk = wrapper.block_mut(0).unwrap();
                    let value = blk.load(DOUBLE, &format!("@{global_name}"));
                    blk.ret(DOUBLE, &value);
                    continue;
                }
                crate::NamespaceEntryKind::NestedNamespace { source_prefix }
                    if source_prefix == module_prefix =>
                {
                    // #10160: `export * as Self from "./self"` reads this
                    // module's own namespace global at access time; the
                    // populator publishes the entry as a live accessor.
                    let wrapper = llmod.define_function(
                        &wrapper_name,
                        DOUBLE,
                        vec![(I64, "%this_closure".to_string())],
                    );
                    let _ = wrapper.create_block("entry");
                    let blk = wrapper.block_mut(0).unwrap();
                    let value = blk.load(DOUBLE, &format!("@__perry_ns_{module_prefix}"));
                    blk.ret(DOUBLE, &value);
                    continue;
                }
                crate::NamespaceEntryKind::ForeignVar {
                    source_prefix,
                    source_local,
                } => {
                    // Producers emit raw local-name getter aliases. Sanitizing
                    // `$item` here would call an unrelated `_item` function.
                    format!("perry_fn_{}__{}", source_prefix, source_local)
                }
                _ => continue,
            };
            if !llmod.has_function(&getter_name) {
                llmod.declare_function(&getter_name, DOUBLE, &[]);
            }
            let wrapper = llmod.define_function(
                &wrapper_name,
                DOUBLE,
                vec![(I64, "%this_closure".to_string())],
            );
            let _ = wrapper.create_block("entry");
            let blk = wrapper.block_mut(0).unwrap();
            let value = blk.call(DOUBLE, &getter_name, &[]);
            blk.ret(DOUBLE, &value);
        }
    }
    // For each `Expr::DynamicImport` target this module dispatches to,
    // declare the foreign module's `@__perry_ns_<target_prefix>` as an
    // extern global so the dispatch site can load it. Deduped via
    // `BTreeSet` so a multi-path site that resolves to N targets emits
    // N declarations exactly once even when multiple `paths` arrays
    // share entries.
    // #7189: nested namespace members (`export * as ns from "./m.ts"`, read as
    // `B.ns` through a static `import * as B`) load the SAME
    // `@__perry_ns_<prefix>` global a dynamic import would, so they need the
    // same extern declaration. Without it the module references a global it
    // never declared and LLVM refuses to parse the IR — which is a good way for
    // this to fail, since it fails at build time and names the missing global.
    let mut nested_prefixes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for key in &cross_module.namespace_member_nested {
        if let Some(prefix) = cross_module.namespace_member_prefixes.get(key) {
            if prefix != module_prefix.as_str() {
                nested_prefixes.insert(prefix.clone());
            }
        }
    }
    // The other direction: this module's OWN populator has a nested entry for
    // each `export * as ns` it declares, and that entry's value is the source
    // module's namespace global. Before #7189 this could not come up, because a
    // module only got a populator by being a dynamic-import target and those
    // already declare their targets. Now that a namespace re-exporter gets one
    // too, its source needs declaring on the same terms.
    for entry in &cross_module.namespace_entries {
        if let crate::NamespaceEntryKind::NestedNamespace { source_prefix } = &entry.kind {
            if source_prefix != module_prefix.as_str() {
                nested_prefixes.insert(source_prefix.clone());
            }
        }
    }
    if !cross_module.dynamic_import_path_to_prefix.is_empty() || !nested_prefixes.is_empty() {
        let mut foreign_prefixes: std::collections::BTreeSet<String> = nested_prefixes;
        for prefix in cross_module.dynamic_import_path_to_prefix.values() {
            if prefix != module_prefix.as_str() {
                foreign_prefixes.insert(prefix.clone());
            }
        }
        for prefix in foreign_prefixes {
            // #3938: `__native_mod__<name>` / `__node_submod__<key>` sentinel
            // prefixes are resolved at the `Expr::DynamicImport` dispatch site
            // via runtime namespace builders (`js_create_native_module_namespace`
            // / `js_node_submodule_namespace`); they have no compiled-module
            // `@__perry_ns_<prefix>` global or `<prefix>__init` function, so the
            // extern decls below are dead. Worse, for slash-bearing submodule
            // names (`node:path/posix`, `node:util/types`) the `/` is illegal in
            // an LLVM global identifier and broke the whole module. Skip them.
            if prefix.starts_with("__native_mod__") || prefix.starts_with("__node_submod__") {
                continue;
            }
            let ns_name = format!("__perry_ns_{}", prefix);
            llmod.add_external_module_state_global(&ns_name, DOUBLE);
            // Issue #753: declare each dynamic-import target's `__init`
            // so the dispatch site in `Expr::DynamicImport` can call it
            // before loading the namespace. The wrapper-side init is
            // idempotent — calling it for an already-initialized
            // target costs a load + cmp + cond_br. For Deferred
            // targets it's the only thing that triggers their init.
            llmod.declare_function(&format!("{}__init", prefix), VOID, &[]);
        }
    }

    // Emit either `int main()` (entry module) or `void <prefix>__init()`
    // (non-entry module). The entry main calls each non-entry init in
    // order before running its own statements.
    compile_module_entry(
        llmod,
        hir,
        func_names,
        strings,
        class_table,
        method_names,
        module_globals,
        opts.import_function_prefixes,
        enum_table,
        static_field_globals,
        class_ids,
        func_signatures,
        func_synthetic_arguments,
        module_prefix,
        opts.is_entry_module,
        opts.non_entry_module_prefixes,
        module_boxed_vars,
        closure_rest_params,
        cross_module,
        opts.output_type,
        &namespace_key_globals,
    )
    .with_context(|| format!("lowering entry of module '{}'", hir.name))?;
    progress.checkpoint("namespace setup and module entry body");

    // Issue #392: pre-intern every user-class method name into the
    // string pool so `emit_string_pool` (which takes `&strings`) can
    // emit the `js_register_class_method(class_id, name_ptr, ...)`
    // calls in module init without needing mutable access to the
    // pool. Only iterate classes DEFINED in this module — the
    // `perry_method_*` symbols for imported classes live in the
    // defining module's object file. `imported_class_prefix` lists
    // every imported class name; we exclude those.
    for class in &hir.classes {
        let cid = class_ids.get(&class.name).copied().unwrap_or(0);
        if cid == 0 {
            continue;
        }
        for method in &class.methods {
            let _ = strings.intern(&method.name);
        }
        // #1788: intern static-method names too, so the
        // `js_register_class_static_method` registration loop in
        // emit_string_pool finds their bytes_global without re-running
        // through the string pool's mutable interner.
        for sm in &class.static_methods {
            let _ = strings.intern(&sm.name);
        }
        // Refs #486: also intern accessor property names so the cross-module
        // `js_register_class_getter` / setter registration loops in emit_string_pool
        // can find their bytes_global without re-running through the
        // string pool's mutable interner.
        for (prop, _) in &class.getters {
            let _ = strings.intern(prop);
        }
        for (prop, _) in &class.setters {
            let _ = strings.intern(prop);
        }
    }

    // After all user code is lowered, the string pool's contents are final.
    // Emit the bytes globals, handle globals, and the
    // `__perry_init_strings_<prefix>` function that runs once at startup.
    // The function name is scoped by module prefix so multiple modules
    // can each have their own string-pool init without colliding.
    // Issue #653: build wrapper-rest map for top-level user functions.
    // Each `__perry_wrap_<name>` was emitted above; if the underlying
    // function declares a rest param, we tell the runtime to bundle
    // trailing args into an array before invoking the wrapper. Without
    // this, calling a function-as-value via the closure-spread path
    // (`js_closure_call_apply_with_spread`) leaks raw element bits into
    // the rest-param slot.
    let user_fn_wrapper_rest: Vec<(String, usize)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            f.params.iter().position(|p| p.is_rest).and_then(|idx| {
                func_names
                    .get(&f.id)
                    .map(|name| (format!("__perry_wrap_{}", name), idx))
            })
        })
        .collect();

    // Refs #915 (gap 1 from #899): wrappers whose underlying function ends in
    // the HIR-synthesized `arguments` rest param. The wrapper symbol is keyed
    // the same way (`__perry_wrap_<scoped_name>`) but registered with the
    // synthetic-arguments runtime fn so `dispatch_rest_bundled` bundles ALL
    // passed args, matching JS spec semantics for `arguments.length`.
    let user_fn_wrapper_synthetic_arguments: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let last_is_synth_args = f
                .params
                .last()
                .map(|p| p.arguments_object.is_some())
                .unwrap_or(false);
            if last_is_synth_args {
                func_names
                    .get(&f.id)
                    .map(|name| format!("__perry_wrap_{}", name))
            } else {
                None
            }
        })
        .collect();
    let mut user_fn_wrapper_rest_and_arguments: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let last = f.params.last()?;
            let has_user_rest = f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            if last.arguments_object.is_some() && has_user_rest {
                func_names
                    .get(&f.id)
                    .map(|name| format!("__perry_wrap_{}", name))
            } else {
                None
            }
        })
        .collect();
    let mut user_fn_wrapper_rest = user_fn_wrapper_rest;
    let mut user_fn_wrapper_synthetic_arguments = user_fn_wrapper_synthetic_arguments;

    // #5134-followup: a renamed export (`export { local as exported }`, which
    // includes `export default function local`) gets a forwarding value wrapper
    // `__perry_wrap_perry_fn_<src>__<exported>` (emitted above), but the
    // rest/synthetic-`arguments` *metadata* loops below key only on the local
    // function name (`__perry_wrap_..._<local>`). So a consumer that referenced
    // the renamed export as a VALUE and called it via `.apply`/`.call`
    // (`compose`'s `pipe.apply(this, reverse(arguments))` in ramda — `pipe` is
    // `export default function pipe()` with a synthetic `arguments`) reached
    // `js_native_call_value` with an unregistered wrapper func_ptr, so
    // `lookup_closure_rest_full` missed and the args were dispatched positionally
    // instead of bundled — the variadic function saw `arguments.length === 0`.
    // Register the exported-alias wrapper symbol with the same metadata as the
    // local one so the runtime bundles correctly through the rename.
    {
        let func_by_local_name: HashMap<&str, &perry_hir::Function> =
            hir.functions.iter().map(|f| (f.name.as_str(), f)).collect();
        for export in &hir.exports {
            let perry_hir::Export::Named { local, exported } = export else {
                continue;
            };
            if local == exported {
                continue;
            }
            let Some(f) = func_by_local_name.get(local.as_str()) else {
                continue;
            };
            let alias_wrap = format!(
                "__perry_wrap_perry_fn_{}__{}",
                module_prefix,
                sanitize(exported)
            );
            // The registration loop (`string_pool.rs`) iterates
            // `user_fn_wrapper_rest`; the synthetic/rest_and_arguments sets only
            // *refine* which runtime fn each entry uses. So the alias must be
            // added to `user_fn_wrapper_rest` (keyed on the rest param index) in
            // EVERY case, plus the matching refinement set.
            let Some(rest_idx) = f.params.iter().position(|p| p.is_rest) else {
                continue;
            };
            let last_is_synth_args = f
                .params
                .last()
                .map(|p| p.arguments_object.is_some())
                .unwrap_or(false);
            let has_user_rest = f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
            user_fn_wrapper_rest.push((alias_wrap.clone(), rest_idx));
            if last_is_synth_args && has_user_rest {
                user_fn_wrapper_rest_and_arguments.insert(alias_wrap);
            } else if last_is_synth_args {
                user_fn_wrapper_synthetic_arguments.insert(alias_wrap);
            }
        }
    }

    // Wrapper arities — ABI param count per top-level user-function wrapper.
    // Used by dynamic closure dispatch to pad omitted trailing parameters
    // before invoking the wrapper.
    let user_fn_wrapper_arity: Vec<(String, u32)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| (format!("__perry_wrap_{}", name), f.params.len() as u32))
        })
        .collect();

    // Wrapper lengths — ECMAScript-visible `.length`, which stops at the
    // first default parameter and before rest. Refs ramda's `converge` /
    // `juxt` / `useWith` chain that reads `.length` off function values to
    // compute curry arities.
    let user_fn_wrapper_length: Vec<(String, u32)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            func_names.get(&f.id).map(|name| {
                (
                    format!("__perry_wrap_{}", name),
                    spec_function_length(&f.params) as u32,
                )
            })
        })
        .collect();

    let user_fn_wrapper_async: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| f.is_async || f.was_plain_async)
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();

    let mut user_fn_wrapper_generator: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| !f.was_plain_async && function_body_returns_generator_object(&f.body))
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, expr) in closures {
        if let perry_hir::Expr::Closure { body, is_async, .. } = expr {
            if !*is_async && function_body_returns_generator_object(body) {
                user_fn_wrapper_generator
                    .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
            }
        }
    }
    // Strict-mode user functions (file-level `"use strict"` or body
    // directive), for OrdinaryCallBindThis in `call`/`apply`/`bind`: a
    // strict callee must observe the raw primitive `thisArg`, a sloppy one
    // gets it boxed. Same two symbol forms as the generator registries.
    let mut user_fn_wrapper_strict: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| f.is_strict)
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, expr) in closures {
        if let perry_hir::Expr::Closure { is_strict, .. } = expr {
            if *is_strict {
                user_fn_wrapper_strict
                    .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
            }
        }
    }

    // #3664: async-generator wrapper symbols, identified by the func_ids the
    // generator transform recorded (it cleared `is_async` before we get here,
    // so the body shape alone can't tell async generators from sync ones).
    // Named declarations use the `__perry_wrap_<name>` singleton symbol;
    // generator EXPRESSIONS use the inline `perry_closure_<modprefix>__<id>`
    // symbol — the same two symbol forms as `user_fn_wrapper_generator`.
    let mut user_fn_wrapper_async_generator: std::collections::HashSet<String> = hir
        .functions
        .iter()
        .filter(|f| hir.async_generator_funcs.contains(&f.id))
        .filter_map(|f| {
            func_names
                .get(&f.id)
                .map(|name| format!("__perry_wrap_{}", name))
        })
        .collect();
    for (func_id, _expr) in closures {
        if hir.async_generator_funcs.contains(func_id) {
            user_fn_wrapper_async_generator
                .insert(format!("perry_closure_{}__{}", module_prefix, func_id));
        }
    }

    // Display names so `console.log` / `util.inspect` print `[Function:
    // <name>]` instead of `[Function (anonymous)]` (#1202). Two kinds:
    //   (a) Top-level `function name() {}` declarations — keyed against
    //       the singleton-wrapper address (`__perry_wrap_<name>`),
    //       because that's what `js_closure_alloc_singleton` stamps into
    //       ClosureHeader for `FuncRef` references. Skips empty / underscore-
    //       prefixed synthesized names (factories, iife, lambdas).
    //   (b) Arrow functions assigned to a binding (`const fn = () => …`)
    //       — keyed against the inline closure symbol
    //       `perry_closure_<modprefix>__<func_id>` that `js_closure_alloc`
    //       stamps into ClosureHeader for inline closures. We only
    //       harvest top-level `Stmt::Let { init: Closure }` shapes here;
    //       nested closures keep the anonymous label, matching Node
    //       (Node uses `inferred-name` only for direct assignments,
    //       which are exactly these top-level lets).
    // (a) Top-level user functions — keyed against the singleton-wrapper
    // address (`__perry_wrap_<sym>`). #2076: prefer the HIR display-name
    // override (set by lower_fn_expr / lower_method_prop) so synthetic
    // names like `__obj_method_method_42` register as `"method"`.
    let mut user_fn_display_names: Vec<(String, String)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let display = hir.closure_display_names.get(&f.id).cloned().or_else(|| {
                if f.name.is_empty() || f.name.starts_with('_') {
                    None
                } else {
                    Some(f.name.clone())
                }
            })?;
            func_names
                .get(&f.id)
                .map(|sym| (format!("__perry_wrap_{}", sym), display))
        })
        .collect();
    super::artifact_display_names::extend_body_symbol_display_names(
        hir,
        func_names,
        method_names,
        llmod,
        &mut user_fn_display_names,
    );
    // (b) Closures bound to a top-level `let`/`const`. #2076: a named
    // function expression's own name takes precedence over the binding
    // name (`const bar = function namedBar(){}` ⇒ `"namedBar"`).
    let mut named_inline_closure_ids: std::collections::HashSet<perry_hir::types::FuncId> =
        std::collections::HashSet::new();
    for stmt in super::entry_outline::logical_entry_stmts(hir) {
        if let perry_hir::Stmt::Let { name, init, .. } = stmt {
            if name.is_empty() || name.starts_with('_') {
                continue;
            }
            if let Some(perry_hir::Expr::Closure { func_id, .. }) = init {
                let display = hir
                    .closure_display_names
                    .get(func_id)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
                user_fn_display_names.push((sym, display));
                named_inline_closure_ids.insert(*func_id);
            }
        }
    }
    // (c) Inline closures with a HIR display name that weren't picked up
    // by (a) or (b) — e.g. an object-literal shorthand method that
    // captured locals or used `this` and lowered to an inline Closure
    // instead of a FuncRef. Skip ids already covered above and any id
    // that hir.functions already produced a wrapper entry for.
    let registered_fn_ids: std::collections::HashSet<perry_hir::types::FuncId> =
        hir.functions.iter().map(|f| f.id).collect();
    // #3527: only register a display name for a `perry_closure_*` symbol when
    // that closure was actually materialized as an LLVM global (i.e. it is in
    // the `closures` set compiled above via `compile_closure`). `hir.closure_display_names`
    // can hold stale entries for closures that were never emitted — e.g.
    // `module.exports = function named(){}` records a display name for a fid that
    // CJS export lowering replaces with a different, materialized fid. Registering
    // a name for the stale fid emits a `js_register_function_name_static` call referencing
    // an undefined `@perry_closure_*` global, which makes `clang -c` fail with
    // "use of undefined value" (regression class of #318/#343).
    let materialized_closure_ids: std::collections::HashSet<perry_hir::types::FuncId> =
        closures.iter().map(|(id, _)| *id).collect();
    // Sorted, NOT raw `HashMap` iteration (#7622) — the same defect #7038 fixed
    // one loop down for `closure_source_text`, left standing here. Every entry
    // mints a rodata constant through `add_string_constant`, whose `@.str.N`
    // counter numbers in first-use order, and emits one
    // `js_register_function_name_static` call in `__perry_init_strings_*`. Iterating
    // the map directly made both a per-process permutation, so the same source
    // compiled by the same binary produced different `.ll` on every run — which
    // silently invalidates any A/B that compares raw IR (the primary evidence
    // the #7615 rooting slices offer). Emission order is the only thing that
    // changes; sorting by `FuncId` makes it stable without altering what is
    // emitted.
    let mut materialized_closure_display: Vec<(&perry_hir::types::FuncId, &String)> = hir
        .closure_display_names
        .iter()
        .filter(|(func_id, display)| {
            materialized_closure_ids.contains(*func_id)
                && !display.is_empty()
                && !named_inline_closure_ids.contains(*func_id)
                && !registered_fn_ids.contains(*func_id)
        })
        .collect();
    materialized_closure_display.sort_by_key(|(func_id, _)| **func_id);
    for (func_id, display) in materialized_closure_display {
        let sym = format!("perry_closure_{}__{}", module_prefix, func_id);
        user_fn_display_names.push((sym, display.clone()));
    }

    // #4101 + #9468: collecting retained function source text lives in
    // `artifact_source_text::collect_user_fn_source` (split out for the file cap).
    let user_fn_source = super::artifact_source_text::collect_user_fn_source(
        hir,
        &func_names,
        closures,
        &registered_fn_ids,
        &materialized_closure_ids,
        module_prefix,
        llmod,
    );

    // Wall 51: the standalone-ctor arity registered into CLASS_CONSTRUCTORS must
    // match the arity of the ctor function actually emitted above (which, for a
    // no-own-ctor class with heritage, is the synthesized `super(...args)`
    // forwarding ctor whose arity comes from the nearest ancestor ctor — possibly
    // cross-module). Compute that arity here with the same walk so the runtime
    // forwards the right number of construction args to the parent ctor.
    let ctor_arity_overrides: HashMap<String, u32> = class_table
        .iter()
        .filter(|(name, class)| **name == class.name && class.id != 0)
        .map(|(name, class)| {
            let n = synthesized_ctor_param_count(
                class,
                class_table,
                imported_class_stubs,
                opts.imported_classes,
                opts.constructor_param_counts,
            ) as u32;
            (name.clone(), n)
        })
        .collect();

    progress.checkpoint("runtime registration metadata");

    let class_source_elided = super::function_source_header::elide_class_sources(hir);
    let class_source_text = class_source_elided
        .as_ref()
        .unwrap_or(&hir.class_source_text);
    emit_string_pool(
        llmod,
        strings,
        module_prefix,
        output_type,
        class_keys_init_data,
        class_header_image_inits,
        class_ids,
        class_table,
        imported_class_stubs,
        &hir.class_display_names,
        &class_source_text,
        &ctor_arity_overrides,
        closure_rest_params,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
        trusted_box_closures,
        versioned_loop_callbacks,
        &user_fn_wrapper_rest,
        closure_synthetic_arguments,
        &user_fn_wrapper_synthetic_arguments,
        closure_rest_and_arguments,
        &user_fn_wrapper_rest_and_arguments,
        &user_fn_wrapper_arity,
        &user_fn_wrapper_length,
        &user_fn_wrapper_async,
        &user_fn_wrapper_generator,
        &user_fn_wrapper_async_generator,
        &user_fn_wrapper_strict,
        &user_fn_display_names,
        &user_fn_source,
    );
    progress.checkpoint("string pool and registration initializer");

    super::namespace_value_getters::emit(llmod, module_prefix, cross_module);

    Ok(())
}
