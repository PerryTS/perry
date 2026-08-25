//! Public entry points and barrier classification for the `Ptr<Shape>` proof.

use super::*;

/// Whether an expression node is a §5.2 shape barrier for the module-wide
/// first-increment kill rule. Targets are NOT inspected — any occurrence
/// disables all `Ptr<Shape>` promotion in the module.
pub(crate) fn expr_is_shape_barrier(expr: &Expr) -> bool {
    match expr {
        Expr::ObjectDefineProperty(..)
        | Expr::ObjectDefineProperties(..)
        | Expr::ReflectDefineProperty { .. }
        | Expr::ObjectSetPrototypeOf(..)
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ReflectSet { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectPreventExtensions(..)
        | Expr::Delete(..)
        | Expr::ProxyNew { .. } => true,
        // `__proto__` writes mutate the prototype chain of an arbitrary
        // object. Reads and class-prototype naming are handled by the
        // dispatch-stability facts; only writes are shape barriers.
        Expr::PropertySet { property, .. } | Expr::PropertyUpdate { property, .. } => {
            property == "__proto__"
        }
        Expr::PutValueSet { key, .. } => {
            matches!(key.as_ref(), Expr::String(k) if k == "__proto__")
        }
        Expr::IndexSet { index, .. } => {
            matches!(index.as_ref(), Expr::String(k) if k == "__proto__")
        }
        _ => false,
    }
}

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
    )
}

/// Containment facts consumable only beside a `$pshape_args` live guard.
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
    );
    // The guarded route consumes class/containment only, never a raw numeric
    // field representation claim.
    for fact in facts.values_mut() {
        fact.numeric_fields.clear();
        fact.report_name = None;
    }
    facts
}
