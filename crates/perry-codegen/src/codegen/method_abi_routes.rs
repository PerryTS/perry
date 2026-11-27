//! Per-method ABI routes: the typed method clones, the proven-`this` and
//! proven-argument-shape clones, and the guarded-parameter method entries.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

use super::typed_clone_sets::record_typed_clone_rejection;
use super::{param_guard, typed_abi, CompileOptions};
use crate::collectors::ModuleDispatchFacts;

/// The method ABI routes `lower_module` hands to `CrossModuleCtx`, each field
/// named for the local it binds.
pub(super) struct MethodAbiRoutes {
    pub(super) typed_f64_methods: std::collections::HashSet<(String, String)>,
    pub(super) typed_i32_methods: std::collections::HashSet<(String, String)>,
    pub(super) typed_i1_methods: std::collections::HashSet<(String, String)>,
    pub(super) typed_string_methods: std::collections::HashSet<(String, String)>,
    pub(super) typed_i1_method_param_reps:
        std::collections::HashMap<(String, String), Vec<typed_abi::TypedParamRep>>,
    pub(super) typed_f64_receiver_methods:
        std::collections::HashMap<(String, String), typed_abi::TypedReceiverMethodInfo>,
    pub(super) guarded_undefined_method_params: std::collections::HashMap<(String, String), usize>,
    pub(super) guarded_falsy_field_default_methods: std::collections::HashMap<
        (String, String),
        param_guard::GuardedFalsyFieldDefaultMethodCandidate,
    >,
    pub(super) pshape_methods:
        std::collections::HashMap<(String, String), crate::collectors::PtrShapeLocal>,
    pub(super) pshape_arg_methods:
        std::collections::HashMap<(String, String), crate::collectors::ProvenShapeArgPlan>,
    pub(super) pshape_tower_routable: std::collections::HashSet<(String, String)>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn classify_method_abis(
    hir: &HirModule,
    opts: &CompileOptions,
    class_table: &HashMap<String, &perry_hir::Class>,
    module_dispatch_facts: &mut ModuleDispatchFacts,
    typed_clone_rejection_records: &mut Vec<crate::native_value::NativeRepRecord>,
    nonnegative_index_methods: &std::collections::HashMap<(String, String), Vec<u32>>,
    mut guarded_undefined_method_candidates: Vec<(usize, (String, String), usize)>,
    mut guarded_falsy_field_default_candidates: Vec<(
        usize,
        (String, String),
        param_guard::GuardedFalsyFieldDefaultMethodCandidate,
    )>,
) -> MethodAbiRoutes {
    let mut typed_f64_methods = std::collections::HashSet::new();
    let mut typed_i32_methods = std::collections::HashSet::new();
    let mut typed_i1_methods = std::collections::HashSet::new();
    let mut typed_string_methods = std::collections::HashSet::new();
    let mut typed_i1_method_param_reps = std::collections::HashMap::new();
    let mut typed_f64_receiver_methods = std::collections::HashMap::new();
    // Representation-selection Phase 5a: proven-`this` method clones.
    let mut pshape_methods: std::collections::HashMap<
        (String, String),
        crate::collectors::PtrShapeLocal,
    > = std::collections::HashMap::new();
    // #7142: the profitability subset the class-id dispatch tower may route to.
    let mut pshape_tower_routable: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    // Phase 3b typed-receiver widening: chain-global field indexes need the
    // full class table — and it must be the SAME table dynamic dispatch's
    // call-site gating consults (`class_table`, incl. class-expression
    // aliases), or a chain resolvable only through an alias would gate a
    // clone call the emission loop never produced (undefined symbol at
    // link).
    let receiver_class_table = &class_table;
    for class in &hir.classes {
        for (prefix, accessors) in [("__get_", &class.getters), ("__set_", &class.setters)] {
            for (property, method) in accessors {
                if class.static_accessor_fn_ids.contains(&method.id) {
                    continue;
                }
                let mut method = method.clone();
                method.name = format!("{prefix}{property}");
                if let Some(fact) = crate::collectors::accessor_proven_this(
                    class,
                    &method,
                    receiver_class_table,
                    &module_dispatch_facts,
                ) {
                    pshape_methods.insert((class.name.clone(), method.name), fact);
                }
            }
        }
        for method in &class.methods {
            let source_function = format!("{}::{}", class.name, method.name);
            // Representation-selection Phase 5a: does this method admit a
            // proven-`this` clone? Uses the SAME class table as the
            // typed-receiver decision below, for the same reason — the two
            // routing sites gate on this map, and a chain resolvable only
            // through an alias would gate a call to a symbol the emission
            // loop never produced.
            if let Some(fact) = crate::collectors::method_proven_this(
                class,
                method,
                receiver_class_table,
                &module_dispatch_facts,
            ) {
                let key = (class.name.clone(), method.name.clone());
                // Indexed methods compose this receiver proof with their
                // exact nonnegative-i32 argument proof. Their published
                // The published proven-receiver entry guards the live
                // argument once, then selects its combined integer clone or
                // its receiver-safe generic body.
                pshape_methods.insert(key, fact);
            }
            match typed_abi::typed_f64_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_f64_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_f64_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_f64_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_f64_receiver_method_info(class, method, receiver_class_table) {
                Some(info) => {
                    typed_f64_receiver_methods
                        .insert((class.name.clone(), method.name.clone()), info);
                }
                None => {
                    if let Some(reason) = typed_abi::typed_f64_receiver_method_rejection_reason(
                        class,
                        method,
                        &receiver_class_table,
                    ) {
                        record_typed_clone_rejection(
                            typed_clone_rejection_records,
                            source_function.clone(),
                            "typed_f64_receiver_method_clone_decision",
                            reason,
                            vec![
                                "typed_clone_kind=typed_f64_receiver_method".to_string(),
                                format!("class={}", class.name),
                                format!("method={}", method.name),
                                format!("function_id={}", method.id),
                            ],
                        );
                    }
                }
            }
            match typed_abi::typed_i1_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_i1_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_i1_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_i1_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_i32_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_i32_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_i32_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_i32_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
            match typed_abi::typed_string_method_rejection_reason(method) {
                None => {
                    let key = (class.name.clone(), method.name.clone());
                    typed_string_methods.insert(key.clone());
                    if let Some(reps) = typed_abi::typed_param_reps_for_params(&method.params) {
                        typed_i1_method_param_reps.insert(key, reps);
                    }
                }
                Some(reason) => record_typed_clone_rejection(
                    typed_clone_rejection_records,
                    source_function.clone(),
                    "typed_string_method_clone_decision",
                    reason,
                    vec![
                        "typed_clone_kind=typed_string_method".to_string(),
                        format!("class={}", class.name),
                        format!("method={}", method.name),
                        format!("function_id={}", method.id),
                    ],
                ),
            }
        }
    }
    // #7142: the tower routing site emits its own inline shape re-check, so it
    // only takes a clone where that clone deletes strictly more guarded work
    // than the check costs. Price these routes after all local clone facts are
    // known so nested `this.other()` calls can count an inherited clone too.
    let local_pshape_methods: std::collections::HashSet<(String, String)> =
        pshape_methods.keys().cloned().collect();
    for class in &hir.classes {
        for method in &class.methods {
            let key = (class.name.clone(), method.name.clone());
            if local_pshape_methods.contains(&key)
                && crate::collectors::pshape_tower_route_profitable(
                    class,
                    method,
                    receiver_class_table,
                    &local_pshape_methods,
                )
            {
                pshape_tower_routable.insert(key);
            }
        }
    }
    // Imported classes publish only clone names the defining module proved and
    // emitted. Installing those capabilities in the same registries lets both
    // the ordinary exact-class/shape guarded arm and profitable adapter-field
    // dispatch towers retain the receiver proof across ESM and npm boundaries.
    // The tower subset is producer-authored because only the defining module
    // can see enough of the body to price its additional keys-token check.
    for imported in &opts.imported_classes {
        let effective_name = imported.effective_name();
        if hir.classes.iter().any(|class| class.name == effective_name) {
            continue;
        }
        for method in &imported.proven_this_method_names {
            if !imported.method_names.contains(method) {
                continue;
            }
            pshape_methods.insert(
                (effective_name.clone(), method.clone()),
                crate::collectors::PtrShapeLocal {
                    family: None,
                    class_name: effective_name.clone(),
                    numeric_fields: std::collections::HashSet::new(),
                    report_name: crate::opt_report::enabled()
                        .then(|| format!("imported:{}", imported.source_prefix)),
                },
            );
            if imported.proven_this_tower_method_names.contains(method) {
                pshape_tower_routable.insert((effective_name.clone(), method.clone()));
            }
        }
    }
    // Keep this first implementation non-combinatorial. Existing typed/raw
    // method clone families have their own public trampolines and calling
    // conventions; composing those proofs is separate work. The optional
    // undefined version retains the ordinary boxed ABI throughout.
    guarded_undefined_method_candidates.retain(|(_, key, _)| {
        !typed_f64_methods.contains(key)
            && !typed_i32_methods.contains(key)
            && !typed_i1_methods.contains(key)
            && !typed_string_methods.contains(key)
            && !typed_f64_receiver_methods.contains_key(key)
            && !nonnegative_index_methods.contains_key(key)
    });
    guarded_undefined_method_candidates.sort_unstable_by(|left, right| left.cmp(right));
    let guarded_undefined_method_params: std::collections::HashMap<(String, String), usize> =
        guarded_undefined_method_candidates
            .into_iter()
            .take(16)
            .map(|(_, key, param_index)| (key, param_index))
            .collect();
    guarded_falsy_field_default_candidates
        .sort_unstable_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let guarded_falsy_field_default_methods: std::collections::HashMap<_, _> =
        guarded_falsy_field_default_candidates
            .into_iter()
            .take(16)
            .map(|(_, key, candidate)| (key, candidate))
            .collect();
    // #8774: one non-combinatorial tagged-ABI clone per local method. Source
    // annotations or a unique unannotated field signature only nominate a
    // class; every routed call emits an exact runtime class+shape guard. Keep
    // this disjoint from typed/index/undefined clone families, whose
    // trampolines have separate routing conventions.
    // Argument layouts may come from an imported class stub. The clone itself
    // is still emitted only for a method body owned by this module; imported
    // field metadata, class id and class-keys/ShapeId globals provide the same
    // exact runtime guard and offsets as local metadata. This is required by
    // perform-ecs: `ECS.addComponentsToEntity` is local to ECS.ts while its
    // `Entity` parameter is declared in Entity.ts.
    let visible_class_names: std::collections::HashSet<&str> =
        receiver_class_table.keys().map(String::as_str).collect();
    let mut pshape_arg_methods = std::collections::HashMap::new();
    for class in &hir.classes {
        for method in &class.methods {
            let key = (class.name.clone(), method.name.clone());
            if typed_f64_methods.contains(&key)
                || typed_i32_methods.contains(&key)
                || typed_i1_methods.contains(&key)
                || typed_string_methods.contains(&key)
                || typed_f64_receiver_methods.contains_key(&key)
                || nonnegative_index_methods.contains_key(&key)
                || guarded_undefined_method_params.contains_key(&key)
                || guarded_falsy_field_default_methods.contains_key(&key)
            {
                continue;
            }
            let Some(plan) = crate::collectors::method_proven_shape_args(
                method,
                receiver_class_table,
                &visible_class_names,
            ) else {
                continue;
            };
            if !plan.args.is_empty() {
                pshape_arg_methods.insert(key, plan);
            }
        }
    }
    module_dispatch_facts.install_argument_shape_routes(pshape_arg_methods.iter().map(
        |(key, plan)| {
            (
                key.clone(),
                plan.args
                    .iter()
                    .map(|arg| {
                        (
                            arg.param_index,
                            arg.fact.class_name.clone(),
                            arg.preserves_containment,
                        )
                    })
                    .collect(),
            )
        },
    ));
    MethodAbiRoutes {
        typed_f64_methods,
        typed_i32_methods,
        typed_i1_methods,
        typed_string_methods,
        typed_i1_method_param_reps,
        typed_f64_receiver_methods,
        guarded_undefined_method_params,
        guarded_falsy_field_default_methods,
        pshape_methods,
        pshape_arg_methods,
        pshape_tower_routable,
    }
}
