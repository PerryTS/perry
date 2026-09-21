//! Public entry points and barrier classification for the `Ptr<Shape>` proof.

use super::*;

pub(crate) use perry_hir::expr_is_shape_barrier;

/// Entry point: collect the shape-proven pointer locals of one lowered region.
/// `not_bigint_locals` feeds the numeric-field proof.
pub(crate) fn collect_shape_proven_ptr_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
    not_bigint_locals: &HashSet<u32>,
    element_facts: &ElementShapeFacts,
) -> HashMap<u32, PtrShapeLocal> {
    collect_shape_proven_ptr_locals_and_element_fields(
        stmts,
        boxed_vars,
        module_globals,
        classes,
        module_dispatch,
        not_bigint_locals,
        element_facts,
        &HashSet::new(),
        &HashMap::new(),
    )
    .0
}

/// Rule 6 (#10769, #10803): the module-wide `Ptr<Shape>` proof for the
/// module-level bindings `collectors/ptr_shape_module_global.rs` admitted.
///
/// The primary region is `hir.init`, which holds the provenance `Stmt::Let`;
/// `regions` is every other region of the module, walked by the same
/// `UseWalk` under the same rules 2-5. Report rows are suppressed: the
/// per-region passes that consume these facts record the selections, and a
/// module-wide pre-pass recording its own would double every row.
pub(crate) fn collect_module_wide_shape_locals<'s>(
    hir: &'s perry_hir::Module,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
    seeds: &HashMap<u32, String>,
    regions: &[&'s [Stmt]],
) -> HashMap<u32, PtrShapeLocal> {
    let _quiet = report::SuppressScope::new();
    collect_shape_proven_ptr_locals_impl(
        &hir.init,
        &HashSet::new(),
        &HashMap::new(),
        classes,
        module_dispatch,
        &HashSet::new(),
        &ElementShapeFacts::default(),
        &HashSet::new(),
        CollectionPurpose::UnguardedRepresentation,
        regions,
        seeds,
    )
    .0
}

/// Collect pointer-local facts plus group-wide numeric layouts of proven
/// element arrays, including arrays read only as `A[i].field`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_shape_proven_ptr_locals_and_element_fields(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
    not_bigint_locals: &HashSet<u32>,
    element_facts: &ElementShapeFacts,
    numeric_param_seeds: &HashSet<u32>,
    module_global_seeds: &HashMap<u32, String>,
) -> (HashMap<u32, PtrShapeLocal>, HashMap<u32, HashSet<String>>) {
    collect_shape_proven_ptr_locals_impl(
        stmts,
        boxed_vars,
        module_globals,
        classes,
        module_dispatch,
        not_bigint_locals,
        element_facts,
        numeric_param_seeds,
        CollectionPurpose::UnguardedRepresentation,
        &[],
        module_global_seeds,
    )
}

/// Containment facts consumable only beside a live argument-shape guard.
///
/// This differs from the representation proof above in exactly one respect:
/// rule 5's module-wide barrier kill is bypassed. Rules 1-4 still bound every
/// path to the object, and the emitted route still revalidates the live class
/// and ShapeId, so the belt-and-braces module kill is redundant here — but the
/// runtime guard is NOT, and the consumer must keep it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_guarded_argument_route_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
    not_bigint_locals: &HashSet<u32>,
    element_facts: &ElementShapeFacts,
    numeric_param_seeds: &HashSet<u32>,
) -> HashMap<u32, PtrShapeLocal> {
    // This second pass is a proof query, not a guard-free representation
    // selection. Suppress report rows for the broader optimization.
    let _quiet = report::SuppressScope::new();
    let (mut facts, _) = collect_shape_proven_ptr_locals_impl(
        stmts,
        boxed_vars,
        module_globals,
        classes,
        module_dispatch,
        not_bigint_locals,
        element_facts,
        numeric_param_seeds,
        CollectionPurpose::GuardedArgumentRoute,
        &[],
        &HashMap::new(),
    );
    // The guarded route consumes class/containment only, never a raw numeric
    // field representation claim.
    for fact in facts.values_mut() {
        fact.numeric_fields.clear();
        fact.report_name = None;
    }
    facts
}
