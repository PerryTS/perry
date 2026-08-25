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
    pub closures: Vec<(perry_hir::types::FuncId, perry_hir::Expr)>,
    pub direct_call_closures: std::collections::HashSet<u32>,
    pub closure_rest_params: HashMap<u32, usize>,
    pub closure_synthetic_arguments: std::collections::HashSet<u32>,
    pub closure_rest_and_arguments: std::collections::HashSet<u32>,
    pub closure_arities: HashMap<u32, u32>,
    pub closure_lengths: HashMap<u32, u32>,
    pub closure_arrow_functions: std::collections::HashSet<u32>,
}

const MAX_TRUSTED_BOX_CLONES_PER_MODULE: usize = 16;
const MAX_TRUSTED_BOX_CLONE_NODES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrustedBoxClosure {
    pub capture_count: u32,
    pub boxed_capture_mask: u64,
}

fn versioned_loop_value_expr(
    expr: &perry_hir::Expr,
    param_ids: &std::collections::HashSet<u32>,
    capture_ids: &std::collections::HashSet<u32>,
) -> bool {
    match expr {
        perry_hir::Expr::Integer(_) | perry_hir::Expr::Number(_) => true,
        perry_hir::Expr::LocalGet(id) => param_ids.contains(id) || capture_ids.contains(id),
        perry_hir::Expr::PropertyGet { object, .. } => {
            matches!(object.as_ref(), perry_hir::Expr::LocalGet(id) if param_ids.contains(id))
        }
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => {
            versioned_loop_value_expr(left, param_ids, capture_ids)
                && versioned_loop_value_expr(right, param_ids, capture_ids)
        }
        _ => false,
    }
}

fn versioned_loop_expr_uses_local(expr: &perry_hir::Expr, local_id: u32) -> bool {
    match expr {
        perry_hir::Expr::LocalGet(id) => *id == local_id,
        perry_hir::Expr::PropertyGet { object, .. } => {
            matches!(object.as_ref(), perry_hir::Expr::LocalGet(id) if *id == local_id)
        }
        perry_hir::Expr::Binary { left, right, .. } => {
            versioned_loop_expr_uses_local(left, local_id)
                || versioned_loop_expr_uses_local(right, local_id)
        }
        _ => false,
    }
}

/// Select exact arrow callbacks whose only source-level effect is replacing a
/// captured box with an additive expression over callback parameters, or an
/// unused-result `++`/`--` on that box. The private clone forces parameter
/// property reads through the descriptor-aware generic PIC and poisons its
/// caller's fast loop before any PIC, dynamic-+, or ToNumeric fallback can run
/// user code. Everything outside this deliberately small grammar keeps the
/// ordinary guarded loop.
pub(crate) fn select_versioned_loop_callbacks(
    closures: &[(perry_hir::types::FuncId, perry_hir::Expr)],
    trusted_box_closures: &std::collections::HashMap<u32, TrustedBoxClosure>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
) -> std::collections::HashSet<u32> {
    closures
        .iter()
        .filter_map(|(func_id, expr)| {
            if !trusted_box_closures.contains_key(func_id) {
                return None;
            }
            let perry_hir::Expr::Closure {
                params,
                body,
                captures,
                captures_this: false,
                captures_new_target: false,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                ..
            } = expr
            else {
                return None;
            };
            let (target, value) = match body.as_slice() {
                [perry_hir::Stmt::Expr(perry_hir::Expr::LocalSet(target, value))] => {
                    (*target, Some(value.as_ref()))
                }
                // The result of a prefix/postfix update is unobservable when
                // the update is the callback's complete expression statement.
                // A private clone can therefore guard the captured value as a
                // Number, perform the step inline, and deopt before ToNumeric
                // for strings, objects, BigInts, TDZ, or any other cold case.
                [perry_hir::Stmt::Expr(perry_hir::Expr::Update { id, .. })] => (*id, None),
                _ => return None,
            };
            if !module_boxed_vars.contains(&target) {
                return None;
            }
            // The private body carries the caller's stack context in the first
            // otherwise-unused callback argument. Reusing the public ABI keeps
            // the context out of an extra live argument on every iteration.
            let scratch_param = params.first()?;
            if let Some(value) = value {
                if versioned_loop_expr_uses_local(value, scratch_param.id) {
                    return None;
                }
            }
            let param_ids: std::collections::HashSet<u32> =
                params.iter().map(|param| param.id).collect();
            let capture_ids: std::collections::HashSet<u32> =
                crate::type_analysis::compute_auto_captures_with_globals(
                    params,
                    body,
                    captures,
                    module_globals,
                )
                .into_iter()
                .collect();
            (capture_ids.contains(&target)
                && value
                    .is_none_or(|value| versioned_loop_value_expr(value, &param_ids, &capture_ids)))
            .then_some(*func_id)
        })
        .collect()
}

fn collect_direct_call_closures_in_stmts(
    stmts: &[perry_hir::Stmt],
    out: &mut std::collections::HashSet<u32>,
) {
    for stmt in stmts {
        match stmt {
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    collect_direct_call_closures_in_expr(init, out);
                }
            }
            perry_hir::Stmt::Expr(expr) | perry_hir::Stmt::Throw(expr) => {
                collect_direct_call_closures_in_expr(expr, out)
            }
            perry_hir::Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    collect_direct_call_closures_in_expr(expr, out);
                }
            }
            perry_hir::Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_direct_call_closures_in_expr(condition, out);
                collect_direct_call_closures_in_stmts(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_direct_call_closures_in_stmts(else_branch, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_direct_call_closures_in_expr(condition, out);
                collect_direct_call_closures_in_stmts(body, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_direct_call_closures_in_stmts(body, out);
                collect_direct_call_closures_in_expr(condition, out);
            }
            perry_hir::Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    collect_direct_call_closures_in_stmts(std::slice::from_ref(init.as_ref()), out);
                }
                if let Some(condition) = condition {
                    collect_direct_call_closures_in_expr(condition, out);
                }
                if let Some(update) = update {
                    collect_direct_call_closures_in_expr(update, out);
                }
                collect_direct_call_closures_in_stmts(body, out);
            }
            perry_hir::Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_direct_call_closures_in_stmts(body, out);
                if let Some(catch) = catch {
                    collect_direct_call_closures_in_stmts(&catch.body, out);
                }
                if let Some(finally) = finally {
                    collect_direct_call_closures_in_stmts(finally, out);
                }
            }
            perry_hir::Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_direct_call_closures_in_expr(discriminant, out);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_direct_call_closures_in_expr(test, out);
                    }
                    collect_direct_call_closures_in_stmts(&case.body, out);
                }
            }
            perry_hir::Stmt::Labeled { body, .. } => {
                collect_direct_call_closures_in_stmts(std::slice::from_ref(body.as_ref()), out)
            }
            perry_hir::Stmt::Break
            | perry_hir::Stmt::Continue
            | perry_hir::Stmt::LabeledBreak(_)
            | perry_hir::Stmt::LabeledContinue(_)
            | perry_hir::Stmt::PreallocateBoxes(_)
            | perry_hir::Stmt::PreallocateTdzBoxes(_)
            | perry_hir::Stmt::ReleaseBoxes(_) => {}
        }
    }
}

fn collect_direct_call_closures_in_expr(
    expr: &perry_hir::Expr,
    out: &mut std::collections::HashSet<u32>,
) {
    if let perry_hir::Expr::Call { args, .. } = expr {
        for arg in args {
            if let perry_hir::Expr::Closure { func_id, .. } = arg {
                out.insert(*func_id);
            }
        }
    }
    if let perry_hir::Expr::Closure { body, .. } = expr {
        collect_direct_call_closures_in_stmts(body, out);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        collect_direct_call_closures_in_expr(child, out)
    });
}

fn count_expr_nodes(expr: &perry_hir::Expr) -> usize {
    let mut count = 1;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        count += count_expr_nodes(child);
    });
    count
}

fn count_stmt_nodes(stmt: &perry_hir::Stmt) -> usize {
    let mut count = 1;
    match stmt {
        perry_hir::Stmt::Let { init, .. } => {
            count += init.as_ref().map(count_expr_nodes).unwrap_or(0)
        }
        perry_hir::Stmt::Expr(expr) | perry_hir::Stmt::Throw(expr) => {
            count += count_expr_nodes(expr)
        }
        perry_hir::Stmt::Return(expr) => count += expr.as_ref().map(count_expr_nodes).unwrap_or(0),
        perry_hir::Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count += count_expr_nodes(condition);
            count += then_branch.iter().map(count_stmt_nodes).sum::<usize>();
            count += else_branch
                .as_ref()
                .map(|body| body.iter().map(count_stmt_nodes).sum())
                .unwrap_or(0);
        }
        perry_hir::Stmt::While { condition, body } => {
            count += count_expr_nodes(condition);
            count += body.iter().map(count_stmt_nodes).sum::<usize>();
        }
        perry_hir::Stmt::DoWhile { body, condition } => {
            count += body.iter().map(count_stmt_nodes).sum::<usize>();
            count += count_expr_nodes(condition);
        }
        perry_hir::Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            count += init
                .as_ref()
                .map(|stmt| count_stmt_nodes(stmt))
                .unwrap_or(0);
            count += condition.as_ref().map(count_expr_nodes).unwrap_or(0);
            count += update.as_ref().map(count_expr_nodes).unwrap_or(0);
            count += body.iter().map(count_stmt_nodes).sum::<usize>();
        }
        perry_hir::Stmt::Try {
            body,
            catch,
            finally,
        } => {
            count += body.iter().map(count_stmt_nodes).sum::<usize>();
            count += catch
                .as_ref()
                .map(|catch| catch.body.iter().map(count_stmt_nodes).sum())
                .unwrap_or(0);
            count += finally
                .as_ref()
                .map(|body| body.iter().map(count_stmt_nodes).sum())
                .unwrap_or(0);
        }
        perry_hir::Stmt::Switch {
            discriminant,
            cases,
        } => {
            count += count_expr_nodes(discriminant);
            for case in cases {
                count += case.test.as_ref().map(count_expr_nodes).unwrap_or(0);
                count += case.body.iter().map(count_stmt_nodes).sum::<usize>();
            }
        }
        perry_hir::Stmt::Labeled { body, .. } => count += count_stmt_nodes(body),
        perry_hir::Stmt::Break
        | perry_hir::Stmt::Continue
        | perry_hir::Stmt::LabeledBreak(_)
        | perry_hir::Stmt::LabeledContinue(_)
        | perry_hir::Stmt::PreallocateBoxes(_)
        | perry_hir::Stmt::PreallocateTdzBoxes(_)
        | perry_hir::Stmt::ReleaseBoxes(_) => {}
    }
    count
}

pub(super) fn count_body_nodes(body: &[perry_hir::Stmt]) -> usize {
    body.iter().map(count_stmt_nodes).sum()
}

pub(crate) fn select_trusted_box_closures(
    closures: &[(perry_hir::types::FuncId, perry_hir::Expr)],
    direct_call_closures: &std::collections::HashSet<u32>,
    module_boxed_vars: &std::collections::HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    excluded_func_ids: &std::collections::HashSet<u32>,
) -> std::collections::HashMap<u32, TrustedBoxClosure> {
    let mut candidates: Vec<(usize, u32, TrustedBoxClosure)> = closures
        .iter()
        .filter_map(|(func_id, expr)| {
            let perry_hir::Expr::Closure {
                params,
                body,
                captures,
                captures_this,
                captures_new_target,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                ..
            } = expr
            else {
                return None;
            };
            if !direct_call_closures.contains(func_id)
                || excluded_func_ids.contains(func_id)
                || params.len() > 16
                || params.iter().any(|param| {
                    param.default.is_some() || param.is_rest || param.arguments_object.is_some()
                })
            {
                return None;
            }

            // Use the creation site's ordering implementation so module init
            // registers the exact closure layout the runtime validates before
            // selecting the private body.
            let auto_captures = crate::type_analysis::compute_auto_captures_with_globals(
                params,
                body,
                captures,
                module_globals,
            );
            let capture_count = auto_captures.len()
                + usize::from(*captures_new_target)
                + usize::from(*captures_this);
            if capture_count > u64::BITS as usize {
                return None;
            }
            let boxed_capture_mask = auto_captures
                .iter()
                .enumerate()
                .filter(|(_, id)| module_boxed_vars.contains(id))
                .fold(0u64, |mask, (index, _)| mask | (1u64 << index));
            if boxed_capture_mask == 0 {
                return None;
            }
            let cost = count_body_nodes(body);
            (cost <= MAX_TRUSTED_BOX_CLONE_NODES).then_some((
                cost,
                *func_id,
                TrustedBoxClosure {
                    capture_count: capture_count as u32,
                    boxed_capture_mask,
                },
            ))
        })
        .collect();
    candidates.sort_unstable_by_key(|(cost, func_id, _)| (*cost, *func_id));
    candidates
        .into_iter()
        .take(MAX_TRUSTED_BOX_CLONES_PER_MODULE)
        .map(|(_, func_id, plan)| (func_id, plan))
        .collect()
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
    let mut closures: Vec<(perry_hir::types::FuncId, perry_hir::Expr)> = Vec::new();
    {
        let mut seen: std::collections::HashSet<perry_hir::types::FuncId> =
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

    let mut direct_call_closures = std::collections::HashSet::new();
    for f in &hir.functions {
        collect_direct_call_closures_in_stmts(&f.body, &mut direct_call_closures);
    }
    for class in &hir.classes {
        for method in &class.methods {
            collect_direct_call_closures_in_stmts(&method.body, &mut direct_call_closures);
        }
        for (_, getter) in &class.getters {
            collect_direct_call_closures_in_stmts(&getter.body, &mut direct_call_closures);
        }
        for (_, setter) in &class.setters {
            collect_direct_call_closures_in_stmts(&setter.body, &mut direct_call_closures);
        }
        for method in &class.static_methods {
            collect_direct_call_closures_in_stmts(&method.body, &mut direct_call_closures);
        }
        for member in &class.computed_members {
            collect_direct_call_closures_in_stmts(&member.function.body, &mut direct_call_closures);
        }
        if let Some(constructor) = &class.constructor {
            collect_direct_call_closures_in_stmts(&constructor.body, &mut direct_call_closures);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(init) = &field.init {
                collect_direct_call_closures_in_expr(init, &mut direct_call_closures);
            }
        }
    }
    collect_direct_call_closures_in_stmts(&hir.init, &mut direct_call_closures);

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
        direct_call_closures,
        closure_rest_params,
        closure_synthetic_arguments,
        closure_rest_and_arguments,
        closure_arities,
        closure_lengths,
        closure_arrow_functions,
    }
}
