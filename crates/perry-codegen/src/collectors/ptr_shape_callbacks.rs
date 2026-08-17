//! Module pre-pass for `Ptr<Shape>` facts that cross an inline array-callback
//! boundary.
//!
//! Array HOF callbacks are emitted as separate LLVM functions before their
//! enclosing HIR regions are compiled. The enclosing region is the only place
//! that can prove the source array dense and monomorphic; this pass performs
//! that proof early and routes only the fully-vetted element-parameter facts
//! to the corresponding closure body.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Module, Param, Stmt};

use super::ptr_shape::PtrShapeLocal;

#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_array_callback_shapes(
    hir: &Module,
    closures: &[(u32, Expr)],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    binding_types: &HashMap<u32, perry_hir::types::Type>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &super::ModuleDispatchFacts,
) -> HashMap<u32, HashMap<u32, PtrShapeLocal>> {
    let _suppress_report = super::ptr_shape_report::SuppressScope::new();
    let mut out = HashMap::new();
    let mut conflicted = HashSet::new();

    collect_region(
        &hir.init,
        &[],
        boxed_vars,
        module_globals,
        binding_types,
        classes,
        module_dispatch,
        &mut out,
        &mut conflicted,
    );
    for function in &hir.functions {
        collect_function(
            function,
            boxed_vars,
            module_globals,
            binding_types,
            classes,
            module_dispatch,
            &mut out,
            &mut conflicted,
        );
    }
    for class in &hir.classes {
        if let Some(constructor) = &class.constructor {
            collect_function(
                constructor,
                boxed_vars,
                module_globals,
                binding_types,
                classes,
                module_dispatch,
                &mut out,
                &mut conflicted,
            );
        }
        for function in class
            .methods
            .iter()
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.static_methods.iter())
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            collect_function(
                function,
                boxed_vars,
                module_globals,
                binding_types,
                classes,
                module_dispatch,
                &mut out,
                &mut conflicted,
            );
        }
    }

    // Each closure is its own executable region. This second layer discovers
    // array callbacks nested inside callbacks or ordinary closure bodies.
    for (_, closure) in closures {
        if let Expr::Closure { params, body, .. } = closure {
            collect_region(
                body,
                params,
                boxed_vars,
                module_globals,
                binding_types,
                classes,
                module_dispatch,
                &mut out,
                &mut conflicted,
            );
        }
    }

    for func_id in conflicted {
        out.remove(&func_id);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn collect_function(
    function: &perry_hir::Function,
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    binding_types: &HashMap<u32, perry_hir::types::Type>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &super::ModuleDispatchFacts,
    out: &mut HashMap<u32, HashMap<u32, PtrShapeLocal>>,
    conflicted: &mut HashSet<u32>,
) {
    collect_region(
        &function.body,
        &function.params,
        boxed_vars,
        module_globals,
        binding_types,
        classes,
        module_dispatch,
        out,
        conflicted,
    );
}

#[allow(clippy::too_many_arguments)]
fn collect_region(
    stmts: &[Stmt],
    params: &[Param],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    binding_types: &HashMap<u32, perry_hir::types::Type>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &super::ModuleDispatchFacts,
    out: &mut HashMap<u32, HashMap<u32, PtrShapeLocal>>,
    conflicted: &mut HashSet<u32>,
) {
    let element_facts = super::ptr_shape_elements::collect_element_shape_facts(
        stmts,
        boxed_vars,
        module_globals,
        classes,
        module_dispatch,
    );
    // Most regions contain no array HOF at all. Avoid duplicating the more
    // expensive complete shape/numeric proof for those regions.
    if element_facts.callback_param_sites().next().is_none() {
        return;
    }
    let not_bigint =
        super::not_bigint_locals::collect_not_bigint_locals(stmts, params, binding_types);
    let shape_facts = super::ptr_shape::collect_shape_proven_ptr_locals(
        stmts,
        boxed_vars,
        module_globals,
        classes,
        module_dispatch,
        &not_bigint,
        &element_facts,
    );

    for (func_id, param_id) in element_facts.callback_param_sites() {
        let Some(fact) = shape_facts.get(&param_id) else {
            continue;
        };
        if conflicted.contains(&func_id) {
            continue;
        }
        let params = out.entry(func_id).or_default();
        if let Some(existing) = params.get(&param_id) {
            if existing.class_name != fact.class_name
                || existing.numeric_fields != fact.numeric_fields
            {
                conflicted.insert(func_id);
            }
        } else {
            params.insert(param_id, fact.clone());
        }
    }
}
