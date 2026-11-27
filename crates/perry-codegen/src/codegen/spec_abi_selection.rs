//! Representation-selection Phase 2: the specialized-ABI plan of each
//! top-level function, chosen after the typed clone sets so the two stay
//! mutually exclusive.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::typed_clone_sets::record_typed_clone_rejection;
use super::{
    facts_int32_locals, param_guard, signature_entry, spec_abi, spec_entry_name, spec_return_proof,
    split_regions, typed_abi, CrossModuleCtx, SpecDispatch, SpecFnPlan,
};
use crate::module::LlModule;

#[allow(clippy::too_many_arguments)]
pub(super) fn select_spec_abi_entries(
    hir: &HirModule,
    cross_module: &mut CrossModuleCtx<'_>,
    llmod: &mut LlModule,
    typed_clone_rejection_records: &mut Vec<crate::native_value::NativeRepRecord>,
    module_prefix: &str,
    class_table: &HashMap<String, &perry_hir::Class>,
    class_ids: &HashMap<String, u32>,
    func_names: &HashMap<u32, String>,
    func_synthetic_arguments: &std::collections::HashSet<u32>,
    triple: &str,
) {
    let no_int32 = std::collections::BTreeSet::new();
    let proven_int32 = if facts_int32_locals::enabled() {
        &cross_module.wpfacts.int32_inputs
    } else {
        &no_int32
    };
    let spec_facts = crate::collectors::collect_spec_abi_facts_with(
        hir,
        &cross_module.definite_init,
        proven_int32,
        &cross_module.wpfacts.native_member_calls,
    );
    let spec_budget = spec_abi::spec_abi_max();
    let mut spec_emitted = 0usize;
    for f in &hir.functions {
        let reject =
            |reason: typed_abi::TypedCloneRejectionReason,
             records: &mut Vec<crate::native_value::NativeRepRecord>| {
                record_typed_clone_rejection(
                    records,
                    f.name.clone(),
                    "spec_abi_entry_decision",
                    reason,
                    vec![
                        "typed_clone_kind=spec_abi_entry".to_string(),
                        format!("function_id={}", f.id),
                        format!("symbol={}", f.name),
                    ],
                );
            };
        // #7111: a function whose call sites were all inlined away — and
        // then constant-folded by `unroll_static_loops` — has no entry in
        // `spec_facts.call_sites`, which is built by walking `hir.init` and
        // every body for direct `Call` expressions. This used to `continue`
        // BEFORE any rejection was constructed, so `--opt-report` said
        // nothing at all about the function: indistinguishable from "not
        // analysed" and from "analysed and denied". Say "moot" instead.
        // A split region has one call site and one body, already typed
        // by its caller's proofs: no specialized copy.
        if split_regions::is_region_function(f) {
            continue;
        }
        let sites = spec_facts.call_sites.get(&f.id);
        // A guard executed when a generator object is CREATED cannot
        // prove the argument when its body later runs. Likewise, an
        // async body may let external code mutate a reachable argument
        // across `await`. Async functions with no suspension execute
        // their entire body synchronously and are safe to clone.
        if f.is_generator
            || f.was_plain_async
            || (f.is_async && param_guard::body_contains_await(&f.body))
        {
            reject(
                typed_abi::TypedCloneRejectionReason::AsyncOrGenerator,
                typed_clone_rejection_records,
            );
            continue;
        }
        if !f.captures.is_empty() {
            reject(
                typed_abi::TypedCloneRejectionReason::Captures,
                typed_clone_rejection_records,
            );
            continue;
        }
        if f.params.iter().any(|p| p.default.is_some()) {
            reject(
                typed_abi::TypedCloneRejectionReason::ParamDefault,
                typed_clone_rejection_records,
            );
            continue;
        }
        if f.params.iter().any(|p| p.is_rest) {
            reject(
                typed_abi::TypedCloneRejectionReason::RestParam,
                typed_clone_rejection_records,
            );
            continue;
        }
        if f.params.iter().any(|p| p.arguments_object.is_some())
            || func_synthetic_arguments.contains(&f.id)
        {
            reject(
                typed_abi::TypedCloneRejectionReason::ArgumentsObject,
                typed_clone_rejection_records,
            );
            continue;
        }
        if cross_module.funcs_reading_dynamic_this.contains(&f.id) {
            reject(
                typed_abi::TypedCloneRejectionReason::SpecReadsDynamicThis,
                typed_clone_rejection_records,
            );
            continue;
        }
        if cross_module.typed_f64_functions.contains(&f.id)
            || cross_module.typed_i32_functions.contains(&f.id)
            || cross_module.typed_i1_functions.contains(&f.id)
            || cross_module.typed_string_functions.contains(&f.id)
        {
            reject(
                typed_abi::TypedCloneRejectionReason::SpecTypedCloneOverlap,
                typed_clone_rejection_records,
            );
            continue;
        }
        // A specialized entry is a second full copy of the body. For a
        // giant body (a generated straight-line driver, a 400-case
        // switch) that doubles the dominant LLVM cost and its `.text`
        // for entry-bound parameter facts that a body this size
        // amortizes to nothing.
        if spec_abi::spec_body_too_large(&f.body) {
            reject(
                typed_abi::TypedCloneRejectionReason::SpecBodyTooLarge,
                typed_clone_rejection_records,
            );
            continue;
        }
        // Callee-side demotion: params the raw ABI cannot accept keep the
        // boxed protocol (reassigned params would stale the entry-bound
        // proofs; closure-referenced params feed the capture machinery).
        let demoted = crate::collectors::callee_demoted_params(f);
        // (#8094) A descriptor proof describes a heap object and is
        // established once, at entry. Any call in this body can run code
        // that reaches that same object — not only through an argument we
        // hand over, but through any alias the caller arranged before we
        // were entered (a global, a closure, a field of another live
        // object). So a REFERENCE-typed parameter cannot keep its proof
        // across a call. Primitive parameters are immune: a callee has no
        // route to the caller's copy of a number, string or boolean.
        //
        // A write THROUGH the parameter in this body invalidates the same
        // proof without any call being involved (`node.left = x`,
        // `values[i] = v`), so it belongs here too — and ONLY here. It is
        // a claim about the object's CONTENTS; a raw slot (I32 / F64 /
        // TaPtr) makes no such claim, so putting this on `demoted` deletes
        // representation choices that predate this PR. Measured: with it
        // on `demoted`, `fill(values: Float64Array, nodes: number)` loses
        // `fill$spec_ta7x10000_i32` entirely, because writing an element
        // reads as "mutation" of the typed-array parameter.
        //
        // Both halves are the definition of a contents-stable binding
        // (`collectors::contents_stable_bindings`), shared with the
        // let-binding proofs: a parameter is an entry-aliased binding, so
        // it is stable only when no write reaches it (through it, an
        // alias or an escape) and the body runs no unknown code and stores
        // through no foreign root. A primitive keeps only the rebinding
        // half: no callee can reach the caller's copy.
        let guard_blocked: Vec<bool> = f
            .params
            .iter()
            .map(|p| {
                if spec_return_proof::is_reference_like(&cross_module.type_aliases, &p.ty, 0) {
                    !cross_module.contents_stable_bindings.contains(&p.id)
                } else {
                    crate::collectors::has_any_mutation(&f.body, p.id)
                }
            })
            .collect();
        let declaration_guards = param_guard::declaration_guards(
            f.id,
            &module_prefix,
            &f.params,
            &demoted,
            &guard_blocked,
            &cross_module.type_aliases,
            &cross_module.interfaces,
            &class_table,
            &class_ids,
        );
        let plan = match sites
            .and_then(|sites| spec_abi::select_dominant_tuple(sites, f.params.len(), &demoted))
        {
            Some((mut reps, _matching_sites)) => {
                // Raw slots already carry a by-construction proof. Boxed
                // slots may additionally recover an ordinary declared
                // parameter fact, but only through their runtime
                // descriptor. TaPtr remains a construction-only ABI: an
                // unknown/public caller has no equivalent cheap guard for
                // the raw pointer + length contract, so mixed TaPtr plans
                // keep their existing static-only route.
                let mut inferred_guards = vec![None; reps.len()];
                for (index, (rep, param)) in reps.iter_mut().zip(f.params.iter()).enumerate() {
                    if !matches!(rep, crate::collectors::SpecParamRep::NumberArray) {
                        continue;
                    }
                    *rep = crate::collectors::SpecParamRep::Boxed;
                    if crate::collectors::guarded_number_array_param_eligible(&f.body, param.id) {
                        inferred_guards[index] = param_guard::inferred_guard(
                            perry_hir::types::Type::Array(Box::new(perry_hir::types::Type::Number)),
                            param_guard::guard_descriptor_name(&module_prefix, f.id, index),
                            &cross_module.type_aliases,
                            &cross_module.interfaces,
                            &class_table,
                            &class_ids,
                        );
                    }
                }
                let has_ta_ptr = reps
                    .iter()
                    .any(|rep| matches!(rep, crate::collectors::SpecParamRep::TaPtr { .. }));
                let guards: Vec<_> = declaration_guards
                    .into_iter()
                    .zip(inferred_guards)
                    .zip(reps.iter())
                    .map(|((declared, inferred), rep)| {
                        if !matches!(rep, crate::collectors::SpecParamRep::Boxed) {
                            None
                        } else if has_ta_ptr {
                            inferred
                        } else {
                            inferred.or(declared)
                        }
                    })
                    .collect();
                let dispatch = if has_ta_ptr {
                    // Raw typed-array pointers remain direct-call-only.
                    // Any inferred boxed-array descriptor is checked in
                    // the same call-site diamond as an i32 range check.
                    SpecDispatch::Static
                } else if guards.iter().any(Option::is_some) {
                    SpecDispatch::Guarded
                } else {
                    SpecDispatch::Static
                };
                SpecFnPlan {
                    reps,
                    dispatch,
                    guards,
                    signature: None,
                    sig_defaults: Vec::new(),
                    sig_public: Default::default(),
                    record: None,
                }
            }
            None => {
                // Tier B: declaration-derived tuple plus descriptors for
                // boxed ordinary parameters. The raw tuple can be all
                // Boxed here: the clone's win is its post-guard parameter
                // facts rather than a calling-convention change.
                let reps = spec_abi::declaration_tuple(&f.params, &demoted);
                let guards: Vec<_> = declaration_guards
                    .into_iter()
                    .zip(reps.iter())
                    .map(|(guard, rep)| {
                        matches!(rep, crate::collectors::SpecParamRep::Boxed)
                            .then_some(guard)
                            .flatten()
                    })
                    .collect();
                if spec_abi::spec_tuple_is_viable(&reps) || guards.iter().any(Option::is_some) {
                    SpecFnPlan {
                        reps,
                        dispatch: SpecDispatch::Guarded,
                        guards,
                        signature: None,
                        sig_defaults: Vec::new(),
                        sig_public: Default::default(),
                        record: None,
                    }
                } else {
                    reject(
                        if sites.is_none() {
                            typed_abi::TypedCloneRejectionReason::SpecNoCallSites
                        } else {
                            typed_abi::TypedCloneRejectionReason::SpecTupleUnproven
                        },
                        typed_clone_rejection_records,
                    );
                    continue;
                }
            }
        };
        if spec_emitted >= spec_budget {
            reject(
                typed_abi::TypedCloneRejectionReason::SpecBudgetExceeded,
                typed_clone_rejection_records,
            );
            continue;
        }
        spec_emitted += 1;
        cross_module.spec_abi_functions.insert(f.id, plan);
    }
    cross_module.spec_ta_bindings = spec_facts.ta_bindings;
    cross_module.spec_return_proofs = spec_return_proof::collect_proven_returns(
        hir,
        &cross_module.spec_abi_functions,
        &cross_module.type_aliases,
    );
    // P2 (T2/T3): every selector above ran exactly as on
    // main; the typed set is decided once, here, before any body or site
    // is lowered.
    signature_entry::select(
        hir,
        cross_module,
        &func_synthetic_arguments,
        &llmod.target_triple,
    );
    signature_entry::select_methods(hir, cross_module, &class_ids);

    // The descriptor is immutable rodata, not a GC object. Emit in HIR
    // order (never HashMap order) so cache/object bytes stay deterministic.
    for f in &hir.functions {
        let Some(plan) = cross_module.spec_abi_functions.get(&f.id) else {
            continue;
        };
        for guard in plan.guards.iter().flatten() {
            // (#8079) Scalar descriptors are decided inline by a
            // typed-abi leaf guard; no rodata blob is referenced.
            if param_guard::scalar_descriptor_rep(&guard.descriptor).is_some() {
                continue;
            }
            llmod.add_named_string_constant(
                &guard.descriptor_name,
                guard.descriptor.len() + 1,
                &param_guard::descriptor_llvm_literal(&guard.descriptor),
            );
        }
    }

    // #8175: recursion-participating specialized clones take LLVM's
    // `preserve_none` convention. Registered HERE — after the plan is
    // final and before any function body compiles — so every dispatch
    // tier (static, guarded/range-checked, the public trampoline's fast
    // arm, and the clone's own self-recursion) stamps the call-site
    // convention through the one `LlBlock::call` choke point, and the
    // clone's define/declare render it from the same registry. Spec
    // entries are `internal` and direct-call-only by construction
    // (`spec_abi_symbol_reachability`), so the convention cannot escape
    // the module. Gated to recursion because the boundary cost is real:
    // a normal-CC caller saves ~20 CSRs once per entry, which amortizes
    // under a recursive tree and pessimizes a cheap non-recursive callee
    // in a hot loop.
    if spec_abi::spec_preserve_none_enabled()
        && spec_abi::preserve_none_target_ok(&triple)
        && !cross_module.spec_abi_functions.is_empty()
    {
        let recursive = crate::collectors::collect_recursion_participants(hir);
        let mut preserve_none: Vec<String> = hir
            .functions
            .iter()
            .filter(|f| recursive.contains(&f.id))
            .filter_map(|f| {
                let plan = cross_module.spec_abi_functions.get(&f.id)?;
                let public = func_names.get(&f.id)?;
                Some(spec_entry_name(public, plan))
            })
            .collect();
        preserve_none.sort_unstable();
        llmod.set_preserve_none_fns(preserve_none);
    }
}
