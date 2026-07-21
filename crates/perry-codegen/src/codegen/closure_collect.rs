//! Closure collection + derived per-closure dispatch maps for
//! `compile_module`.
//!
//! Extracted verbatim from the `compile_module` body (pure code move, no
//! behavior change). Walks every container the compile loop also compiles —
//! functions, methods, ctors, getters, setters, static_methods,
//! computed-members, and (instance + static) field initializers — collecting
//! every `Expr::Closure` so the closure creation site can take its address,
//! then derives the rest/arity/arguments/arrow maps from the collected set.

use std::collections::HashMap;

use perry_hir::Module as HirModule;

// Collector and boxing-analysis walkers live in dedicated modules.
use crate::collectors::collect_closures_in_stmts;

// `spec_function_length` is a trunk free fn (also reachable via `super::*`).
use super::spec_function_length;

/// Result bundle of the module-wide closure collection pass.
pub(crate) struct ModuleClosures {
    pub closures: Vec<(perry_types::FuncId, perry_hir::Expr)>,
    pub closure_rest_params: HashMap<u32, usize>,
    pub closure_synthetic_arguments: std::collections::HashSet<u32>,
    pub closure_rest_and_arguments: std::collections::HashSet<u32>,
    pub closure_arities: HashMap<u32, u32>,
    pub closure_lengths: HashMap<u32, u32>,
    pub closure_arrow_functions: std::collections::HashSet<u32>,
}

/// Collect every `Expr::Closure` in the program and build the derived
/// per-closure dispatch maps. See the inline comments (preserved from the
/// original `compile_module` body) for the per-map rationale.
pub(crate) fn collect_module_closures(hir: &HirModule) -> ModuleClosures {
    // Pre-walk for closures: every `Expr::Closure` in the program needs
    // its body emitted as a top-level LLVM function so the closure
    // creation site can take its address. Collect them all first, then
    // emit each via `compile_closure` (Phase D.1).
    //
    // We must walk every container that the compile loop below also
    // compiles — methods, ctors, getters, setters, static_methods —
    // otherwise a closure body in (say) a `get size() { return arr.filter(...).length }`
    // ends up referenced by `js_closure_alloc(@perry_closure_*)` but
    // never defined, and clang errors with "use of undefined value".
    let mut closures: Vec<(perry_types::FuncId, perry_hir::Expr)> = Vec::new();
    {
        let mut seen: std::collections::HashSet<perry_types::FuncId> =
            std::collections::HashSet::new();
        for f in &hir.functions {
            collect_closures_in_stmts(&f.body, &mut seen, &mut closures);
        }
        for c in &hir.classes {
            for m in &c.methods {
                collect_closures_in_stmts(&m.body, &mut seen, &mut closures);
            }
            for (_, getter_fn) in &c.getters {
                collect_closures_in_stmts(&getter_fn.body, &mut seen, &mut closures);
            }
            for (_, setter_fn) in &c.setters {
                collect_closures_in_stmts(&setter_fn.body, &mut seen, &mut closures);
            }
            for sm in &c.static_methods {
                collect_closures_in_stmts(&sm.body, &mut seen, &mut closures);
            }
            for member in &c.computed_members {
                collect_closures_in_stmts(&member.function.body, &mut seen, &mut closures);
            }
            if let Some(ctor) = &c.constructor {
                collect_closures_in_stmts(&ctor.body, &mut seen, &mut closures);
            }
            // Class field initializers (`private foo = (x) => this.bar(x)`) are
            // hoisted into the constructor at codegen time via
            // `apply_field_initializers_recursive`, so any closure literal inside
            // an `init` expression gets a `js_closure_alloc(@perry_closure_*)`
            // emission. We must walk the inits too, otherwise the body never
            // gets compiled and clang errors with "use of undefined value" (#261).
            for field in &c.fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
            // #338: static fields with closure inits (`static make = (x) =>
            // ...`) emit `js_closure_alloc(@perry_closure_*)` at module-init
            // time too — the codegen path that initialises
            // `@perry_static_<class>__<field>` globals. Pre-fix this loop
            // walked instance fields (`c.fields`) only, so closures inside
            // `c.static_fields[i].init` were never collected and clang
            // errored on the undefined `@perry_closure_*` reference.
            // Surfaced on Effect's `SchemaAST.ts` (Union.make / Union.unify)
            // and any class shipping arrow-style static helpers.
            for field in &c.static_fields {
                if let Some(init) = &field.init {
                    collect_closures_in_stmts(
                        &[perry_hir::Stmt::Expr(init.clone())],
                        &mut seen,
                        &mut closures,
                    );
                }
            }
        }
        collect_closures_in_stmts(&hir.init, &mut seen, &mut closures);
    }

    // Build closure rest param index: for each closure that has a rest
    // parameter, record its func_id → rest param position. Used by
    // the closure call site in `lower_call` to bundle trailing args.
    let closure_rest_params: HashMap<u32, usize> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                params.iter().position(|p| p.is_rest).map(|idx| (*fid, idx))
            } else {
                None
            }
        })
        .collect();

    // Refs #915 (gap 1 from #899): closures whose rest param is the
    // HIR-synthesized `arguments` need to bundle ALL passed args into
    // the rest slot at dispatch time — JS spec semantics for
    // `arguments.length` count every passed arg, not just the trailing
    // tail after the fixed params. The runtime side reads this through
    // `js_register_closure_synthetic_arguments` (vs the regular
    // `js_register_closure_rest`).
    let closure_synthetic_arguments: std::collections::HashSet<u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                let last_is_synth_args = params
                    .last()
                    .map(|p| p.arguments_object.is_some())
                    .unwrap_or(false);
                let has_user_rest = params
                    .iter()
                    .any(|p| p.is_rest && p.arguments_object.is_none());
                if last_is_synth_args && !has_user_rest {
                    Some(*fid)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let closure_rest_and_arguments: std::collections::HashSet<u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                let last_is_synth_args = params
                    .last()
                    .map(|p| p.arguments_object.is_some())
                    .unwrap_or(false);
                let has_user_rest = params
                    .iter()
                    .any(|p| p.is_rest && p.arguments_object.is_none());
                if last_is_synth_args && has_user_rest {
                    Some(*fid)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Refs #421: declared param count for every non-rest closure. Used by
    // `emit_string_pool` to register each closure's ABI arity so the runtime
    // can pad missing args with TAG_UNDEFINED in the dynamic-dispatch path.
    let closure_arities: HashMap<u32, u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                if params.iter().any(|p| p.is_rest) {
                    return None;
                }
                Some((*fid, params.len() as u32))
            } else {
                None
            }
        })
        .collect();
    let closure_lengths: HashMap<u32, u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { params, .. } = expr {
                Some((*fid, spec_function_length(params) as u32))
            } else {
                None
            }
        })
        .collect();
    let closure_arrow_functions: std::collections::HashSet<u32> = closures
        .iter()
        .filter_map(|(fid, expr)| {
            if let perry_hir::Expr::Closure { is_arrow, .. } = expr {
                is_arrow.then_some(*fid)
            } else {
                None
            }
        })
        .collect();

    ModuleClosures {
        closures,
        closure_rest_params,
        closure_synthetic_arguments,
        closure_rest_and_arguments,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
    }
}
