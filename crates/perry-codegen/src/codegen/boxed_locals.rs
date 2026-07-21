//! Module-wide boxed-var and local-type collection for `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Both functions walk the entire HIR module — functions,
//! class methods/getters/setters/static-methods/computed-members/ctors, and
//! the module init — and accumulate a single flat set/map keyed by HIR
//! LocalId (which is globally unique within the module).

use std::collections::HashMap;

use perry_hir::Module as HirModule;

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::boxed_vars::{collect_boxed_param_ids, collect_boxed_vars, collect_let_types_in_stmts};

/// Module-level boxed_vars: union of every per-function/method/
/// closure/module-init boxed set. We compute this once because
/// closures emitted in `compile_closure` need to know whether their
/// transitively-captured ids from an enclosing function were boxed
/// at the creation site. Since HIR LocalIds are globally unique
/// across the module, a single union set is enough: each id either
/// lives in a box or it doesn't, irrespective of which function
/// owns it.
pub(crate) fn collect_module_boxed_vars(hir: &HirModule) -> std::collections::HashSet<u32> {
    let mut module_boxed_vars: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for f in &hir.functions {
        module_boxed_vars.extend(collect_boxed_vars(&f.body));
        // #5521: box captured+mutated params (never in the Stmt::Let
        // `declared` set, so missed by `collect_boxed_vars`).
        module_boxed_vars.extend(collect_boxed_param_ids(&f.params, &f.body));
    }
    for c in &hir.classes {
        for m in &c.methods {
            module_boxed_vars.extend(collect_boxed_vars(&m.body));
            module_boxed_vars.extend(collect_boxed_param_ids(&m.params, &m.body));
        }
        for (_, getter_fn) in &c.getters {
            module_boxed_vars.extend(collect_boxed_vars(&getter_fn.body));
            module_boxed_vars.extend(collect_boxed_param_ids(&getter_fn.params, &getter_fn.body));
        }
        for (_, setter_fn) in &c.setters {
            module_boxed_vars.extend(collect_boxed_vars(&setter_fn.body));
            module_boxed_vars.extend(collect_boxed_param_ids(&setter_fn.params, &setter_fn.body));
        }
        for sm in &c.static_methods {
            module_boxed_vars.extend(collect_boxed_vars(&sm.body));
            module_boxed_vars.extend(collect_boxed_param_ids(&sm.params, &sm.body));
        }
        for member in &c.computed_members {
            module_boxed_vars.extend(collect_boxed_vars(&member.function.body));
            module_boxed_vars.extend(collect_boxed_param_ids(
                &member.function.params,
                &member.function.body,
            ));
        }
        if let Some(ctor) = &c.constructor {
            module_boxed_vars.extend(collect_boxed_vars(&ctor.body));
            module_boxed_vars.extend(collect_boxed_param_ids(&ctor.params, &ctor.body));
        }
    }
    module_boxed_vars.extend(collect_boxed_vars(&hir.init));
    module_boxed_vars
}

/// Module-wide LocalId → Type map. Used by closure bodies to
/// learn the types of captured vars from the enclosing scope.
/// HIR LocalIds are globally unique within the module, so a
/// single flat map works.
pub(crate) fn collect_module_local_types(hir: &HirModule) -> HashMap<u32, perry_types::Type> {
    let mut module_local_types: HashMap<u32, perry_types::Type> = HashMap::new();
    collect_let_types_in_stmts(&hir.init, &mut module_local_types);
    for f in &hir.functions {
        for p in &f.params {
            module_local_types.insert(p.id, p.ty.clone());
        }
        collect_let_types_in_stmts(&f.body, &mut module_local_types);
    }
    for c in &hir.classes {
        for m in &c.methods {
            for p in &m.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&m.body, &mut module_local_types);
        }
        for (_, getter_fn) in &c.getters {
            for p in &getter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&getter_fn.body, &mut module_local_types);
        }
        for (_, setter_fn) in &c.setters {
            for p in &setter_fn.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&setter_fn.body, &mut module_local_types);
        }
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&ctor.body, &mut module_local_types);
        }
        for sm in &c.static_methods {
            for p in &sm.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&sm.body, &mut module_local_types);
        }
        for member in &c.computed_members {
            for p in &member.function.params {
                module_local_types.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(&member.function.body, &mut module_local_types);
        }
    }
    module_local_types
}
