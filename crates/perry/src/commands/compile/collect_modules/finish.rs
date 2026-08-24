//! Module-collection finish step.
//!
//! Split out of `collect_modules.rs` (2000-line-per-file cap). Pure
//! relocation of `collect_module_finish`.

use super::*;

pub(crate) fn collect_module_finish(
    prepared: PreparedModule,
    ctx: &mut CompilationContext,
    visited: &HashSet<PathBuf>,
    target: Option<&str>,
    skip_transforms: bool,
    progress: &VerboseProgress,
) -> Result<()> {
    let PreparedModule {
        canonical,
        module_name,
        mut hir_module,
    } = prepared;

    // Issue #535 — `perry/ui` `state<T>` desugar pass.
    let is_harmonyos = matches!(target, Some("harmonyos") | Some("harmonyos-simulator"));
    if !is_harmonyos {
        perry_transform::state_desugar::run(&mut hir_module);
    }

    // Run HIR transforms AFTER imports/re-exports have been recursively
    // collected, so `ctx.native_modules` already contains every dependency
    // of this module. The cross-module method-inlining harvester below
    // pulls inlinable methods from those prior modules — without this
    // ordering, a consumer (e.g. `sync-hotpath.test.ts`) would inline
    // BEFORE `world.ts` finished processing, missing every `World.*`
    // candidate and leaving the hot `world.set(...)` call as a runtime
    // dispatch.
    //
    // Pre-existing constraint: `transform_async_to_generator` runs AFTER
    // `inline_functions` (so inlined async bodies are still rewritten)
    // and BEFORE `transform_generators` (which consumes the generator
    // shape it produces). Issue #256.
    if !skip_transforms {
        progress.record(ProgressSnapshot {
            stage: "transform",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        let mut extra_methods: std::collections::HashMap<(String, String), MethodCandidate> =
            std::collections::HashMap::new();
        if std::env::var("PERRY_INLINE_DEBUG").is_ok() {
            eprintln!(
                "[INLINE-DRIVER] processing {}: prior modules={:?}",
                hir_module.name,
                ctx.native_modules
                    .values()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
        let enable_cross_module_inline =
            ctx.native_modules.len() <= MAX_CROSS_MODULE_INLINE_PRIOR_MODULES;
        if std::env::var("PERRY_INLINE_DEBUG").is_ok() && !enable_cross_module_inline {
            eprintln!(
                "[INLINE-DRIVER] skipping cross-module inline harvest for {}: prior_modules={} budget={}",
                hir_module.name,
                ctx.native_modules.len(),
                MAX_CROSS_MODULE_INLINE_PRIOR_MODULES
            );
        }
        if enable_cross_module_inline {
            for prior_module in ctx.native_modules.values() {
                // The strict harvester rejects ExternFuncRef-using methods.
                // The loose variant records each required extern name;
                // `inline_functions` filters by destination imports.
                // First-write-wins on key collision (rare — issue #309 cycle
                // breaker). Strict-harvest entries are functionally equivalent
                // when colliding with the loose variant (same body), so
                // either ordering is correct.
                for (k, v) in gather_cross_module_methods_with_extern_imports(prior_module) {
                    extra_methods.entry(k).or_insert(v);
                }
                for (k, v) in gather_cross_module_methods(prior_module) {
                    extra_methods.entry(k).or_insert(v);
                }
            }
        }
        // Cross-module field-type info: `(class_name, field_name) ->
        // field_class_name`. Lets the inliner's `resolve_receiver_class`
        // walk a chain like `world.commandBuffer.set(...)` — without it,
        // the receiver match bails at the first PropertyGet and the call
        // stays a runtime dispatch. Built from every prior module's
        // class.fields where the type is `Named(...)`.
        let mut extra_class_fields: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        if enable_cross_module_inline {
            for prior_module in ctx.native_modules.values() {
                for class in &prior_module.classes {
                    for f in &class.fields {
                        if let perry_hir::types::Type::Named(field_class) = &f.ty {
                            extra_class_fields
                                .entry((class.name.clone(), f.name.clone()))
                                .or_insert_with(|| field_class.clone());
                        }
                    }
                }
            }
        }
        // Cross-module anon-shape classes. Names are content-addressed
        // (FNV-1a hash of the canonical shape key), so dedup-by-name across
        // modules is correct: any two modules that synthesized a class for
        // the same closed-shape literal end up with byte-identical class
        // definitions under the same name. Required so that when
        // `inline_functions` copies a method body referencing
        // `__AnonShape_<hash>` into this module, codegen can resolve the
        // class definition (otherwise the field list is missing and the
        // literal lowers as a bare object with all properties dropped).
        let mut extra_anon_classes: std::collections::HashMap<String, &perry_hir::Class> =
            std::collections::HashMap::new();
        if enable_cross_module_inline {
            for prior_module in ctx.native_modules.values() {
                for (k, v) in gather_cross_module_anon_classes(prior_module) {
                    extra_anon_classes.entry(k).or_insert(v);
                }
            }
        }
        // Interprocedural deforestation. Runs BEFORE inline_functions
        // so the inliner sees deforested signatures (the rewritten
        // function takes an accumulator param; inlined call sites then
        // already use the new shape). Intra-module only — see
        // `deforest::run` doc-comment for limitations and the manual
        // ABC451D validation.
        progress.record(ProgressSnapshot {
            stage: "transform-deforest",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        perry_transform::deforest::run(&mut hir_module);
        progress.record(ProgressSnapshot {
            stage: "transform-inline-functions",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        inline_functions(
            &mut hir_module,
            &extra_methods,
            &extra_class_fields,
            &extra_anon_classes,
        );
        // Post-inline HIR cleanups, in ONE call because they share their
        // ordering constraint — `perry_transform::post_inline_cleanups`:
        // static-trip-count for-loop unroll, then redundant property-read
        // elimination over diverging guard chains. Both want the INLINED
        // (and unrolled) bodies, and both must run BEFORE the async/generator
        // transforms: those rewrite control flow into state-machine shapes the
        // unroll match no longer recognizes, and box every body local into a
        // shared mutable cell, which would turn a hoisted `const` into one
        // more boxed cell. See crates/perry-transform/src/{unroll,prop_cse}.
        progress.record(ProgressSnapshot {
            stage: "transform-unroll-static-loops",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        perry_transform::post_inline_cleanups(&mut hir_module);
        // Inline `finally` bodies before each abrupt completion
        // (`return` / `break` / `continue` / labeled-break / labeled-
        // continue) reachable inside a `try { ... } finally { Y }`
        // shape. Must run BEFORE `transform_async_to_generator` because
        // the async transform flattens `try`/`finally` into a flat
        // state-machine sequence — an abrupt completion in the body
        // terminates the state, leaving the appended finally as dead
        // code. Issue #536.
        progress.record(ProgressSnapshot {
            stage: "transform-inline-finally",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        inline_finally_into_returns(&mut hir_module);
        progress.record(ProgressSnapshot {
            stage: "transform-async-to-generator",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        transform_async_to_generator(&mut hir_module);
        // #8595: outline an oversized module-entry body into per-chunk
        // functions so no single function carries the whole init (which is
        // pathological for RS4GC relocation fan-out, ISel, and regalloc alike).
        // Automatic only for very large entries; PERRY_OUTLINE_ENTRY=1 forces
        // the transform and =0 disables it. Fail-safe exclusions leave the
        // original body untouched. See perry-codegen `codegen::entry_outline`.
        progress.record(ProgressSnapshot {
            stage: "transform-outline-entry",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        match perry_codegen::codegen::entry_outline::outline_entry_module(&mut hir_module) {
            perry_codegen::codegen::entry_outline::OutlineOutcome::Outlined { chunks } => {
                log::debug!(
                    "perry: outlined entry body of '{}' into {} chunk functions",
                    hir_module.name,
                    chunks
                );
            }
            perry_codegen::codegen::entry_outline::OutlineOutcome::Skipped(reason) => {
                log::debug!(
                    "perry: entry body of '{}' not outlined: {}",
                    hir_module.name,
                    reason
                );
            }
        }
        progress.record(ProgressSnapshot {
            stage: "transform-generators",
            module_path: Some(&canonical),
            module_name: Some(&module_name),
            visited: Some(visited.len()),
            collected: Some(ctx.native_modules.len() + ctx.js_modules.len()),
            ..Default::default()
        });
        transform_generators(&mut hir_module);
    }

    // Set optional-feature gates (regex/temporal/url/crypto/events/etc.) so
    // auto-optimize links only the runtime subsystems this module can reach.
    feature_detect::detect_optional_feature_usage(ctx, &hir_module);

    let collected_after_insert = ctx.native_modules.len() + ctx.js_modules.len() + 1;
    progress.record(ProgressSnapshot {
        stage: "collected",
        module_path: Some(&canonical),
        module_name: Some(&module_name),
        visited: Some(visited.len()),
        collected: Some(collected_after_insert),
        ..Default::default()
    });
    ctx.native_modules.insert(canonical, hir_module);
    Ok(())
}
