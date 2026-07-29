// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Phase 2 v3.6 — pre-walk that returns one `ViewBuilder` per function
/// matching the view-builder pattern: at least one
/// `widgetAddChild(LocalGet(target), X)` call where `target` is a
/// MODULE-LEVEL `let X = widget` declaration AND the function is invoked
/// from at least one `Expr::Closure` body anywhere in the module.
///
/// Functions called only from `module.init` (e.g. Mango's
/// `refreshConnectionList()` at the top-level) are EXCLUDED — they're
/// already inlined by `inlined_analysis_init`'s Phase A and the result
/// becomes module-init mutations directly. Lifting them as conditional
/// branches would emit duplicate content.
///
/// Functions called from BOTH module-init AND closures are also excluded
/// from this pass for now (the duplicate-emit hazard would require a
/// more involved merge); they fall back to v0.5.489 inlining (renders
/// the initial state but doesn't update on tap). Tracked as a follow-up.
pub(crate) fn collect_view_builders(module: &Module, next_group_id: &mut u32) -> Vec<ViewBuilder> {
    use std::collections::HashSet;

    // Pass 1 — collect every module-level `let X = ...` LocalId.
    let mut module_level_locals: HashSet<LocalId> = HashSet::new();
    for stmt in &module.init {
        if let Stmt::Let { id, .. } = stmt {
            module_level_locals.insert(*id);
        }
    }

    // Pass 2 — collect every function's `widgetAddChild(LocalGet(id), _)`
    // targets that are module-level. Pick the function's "primary target"
    // as the first matching one (Mango's pattern is one terminal target
    // per view-builder; multi-target view-builders aren't supported yet).
    let mut primary_target: HashMap<perry_hir::types::FuncId, LocalId> = HashMap::new();
    for f in &module.functions {
        if f.is_async || f.is_generator {
            continue;
        }
        let mut found: Option<LocalId> = None;
        scan_module_level_addchild(&f.body, &module_level_locals, &mut found);
        if let Some(target) = found {
            primary_target.insert(f.id, target);
        }
    }
    if primary_target.is_empty() {
        return Vec::new();
    }

    // Pass 3 — find functions called from any `Expr::Closure` body
    // anywhere in the module (closures in module.init AND inside other
    // function bodies). Calls inside top-level Stmts of module.init or
    // function bodies that are NOT inside a closure don't count.
    let mut called_from_closure: HashSet<perry_hir::types::FuncId> = HashSet::new();
    walk_for_funcref_calls_in_closures_in_stmts(&module.init, &mut called_from_closure);
    for f in &module.functions {
        walk_for_funcref_calls_in_closures_in_stmts(&f.body, &mut called_from_closure);
    }

    // Pass 4 — find functions called from module.init OR from a function
    // that's itself called from module.init. Used to EXCLUDE module-init
    // call paths from view-builder treatment (avoids duplicate emit).
    let mut called_from_module_init: HashSet<perry_hir::types::FuncId> = HashSet::new();
    walk_for_funcref_calls_top_level_in_stmts(&module.init, &mut called_from_module_init);

    // Pass 5 — assemble ViewBuilders. Stable target_synth assignment by
    // sorted target LocalId so re-runs produce the same output.
    let mut target_synth_for: HashMap<LocalId, String> = HashMap::new();
    let mut next_target_synth: usize = 0;

    let mut builders: Vec<ViewBuilder> = Vec::new();
    let function_lookup: HashMap<perry_hir::types::FuncId, &perry_hir::ir::Function> =
        module.functions.iter().map(|f| (f.id, f)).collect();
    let mut sorted_func_ids: Vec<perry_hir::types::FuncId> =
        primary_target.keys().copied().collect();
    sorted_func_ids.sort();
    for func_id in sorted_func_ids {
        if !called_from_closure.contains(&func_id) {
            continue;
        }
        if called_from_module_init.contains(&func_id) {
            // Mixed call sites — defer.
            continue;
        }
        let target_id = primary_target[&func_id];
        let target_synth = target_synth_for
            .entry(target_id)
            .or_insert_with(|| {
                let synth = format!("cv_{}", next_target_synth);
                next_target_synth += 1;
                synth
            })
            .clone();
        let func_name = function_lookup
            .get(&func_id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| format!("fn_{}", func_id));
        let view_id = sanitize_view_id(&func_name);
        let group_id = *next_group_id;
        *next_group_id += 1;
        builders.push(ViewBuilder {
            func_id,
            func_name: func_name.clone(),
            target_id,
            target_synth,
            view_id,
            group_id,
        });
    }
    builders
}

pub(crate) fn sanitize_view_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("view");
    }
    out
}

pub(crate) fn scan_module_level_addchild(
    stmts: &[Stmt],
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    for stmt in stmts {
        scan_module_level_addchild_in_stmt(stmt, module_locals, found);
    }
}

pub(crate) fn scan_module_level_addchild_in_stmt(
    stmt: &Stmt,
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    if found.is_some() {
        return;
    }
    match stmt {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } | Stmt::Return(Some(e)) => {
            scan_module_level_addchild_in_expr(e, module_locals, found);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            scan_module_level_addchild(then_branch, module_locals, found);
            if let Some(eb) = else_branch {
                scan_module_level_addchild(eb, module_locals, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            scan_module_level_addchild(body, module_locals, found);
        }
        Stmt::For { body, .. } => {
            scan_module_level_addchild(body, module_locals, found);
        }
        _ => {}
    }
}

pub(crate) fn scan_module_level_addchild_in_expr(
    e: &Expr,
    module_locals: &std::collections::HashSet<LocalId>,
    found: &mut Option<LocalId>,
) {
    if found.is_some() {
        return;
    }
    if let Expr::NativeMethodCall {
        module,
        method,
        args,
        object: None,
        ..
    } = e
    {
        if module == "perry/ui" && method == "widgetAddChild" && args.len() == 2 {
            if let Expr::LocalGet(target_id) = &args[0] {
                if module_locals.contains(target_id) {
                    *found = Some(*target_id);
                    return;
                }
            }
        }
    }
    // Don't recurse into closures — only the outer function body counts
    // for primary-target detection. (We will still handle the closure
    // body separately if it contains a view-builder pattern.)
    match e {
        Expr::Call { callee, args, .. } => {
            scan_module_level_addchild_in_expr(callee, module_locals, found);
            for a in args {
                scan_module_level_addchild_in_expr(a, module_locals, found);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                scan_module_level_addchild_in_expr(o, module_locals, found);
            }
            for a in args {
                scan_module_level_addchild_in_expr(a, module_locals, found);
            }
        }
        _ => {}
    }
}

/// Phase 2 v3.6 — rewrite every closure body's call to a view-builder
/// function: prepend a `setContentView(target_synth, view_id)` call
/// before the existing function call. The function call itself is left
/// untouched so non-UI side effects (state assignments to module locals,
/// etc.) continue to fire. Widget construction inside the function is
/// no-op stubs on harmonyos so doesn't need stripping.
pub(crate) fn rewrite_view_builder_calls_in_stmts(stmts: &mut Vec<Stmt>, builders: &[ViewBuilder]) {
    if builders.is_empty() {
        return;
    }
    let lookup: HashMap<perry_hir::types::FuncId, &ViewBuilder> =
        builders.iter().map(|b| (b.func_id, b)).collect();
    rewrite_view_builder_calls_in_stmts_with_lookup(stmts, &lookup);
}

pub(crate) fn rewrite_view_builder_calls_in_stmts_with_lookup(
    stmts: &mut Vec<Stmt>,
    lookup: &HashMap<perry_hir::types::FuncId, &ViewBuilder>,
) {
    let mut i = 0;
    while i < stmts.len() {
        rewrite_view_builder_calls_in_stmt(&mut stmts[i], lookup);
        // After rewrite, the stmt's expr may have been wrapped — but we
        // don't insert siblings here; the prepend happens INSIDE closures,
        // not at the call's enclosing stmt level. (Top-level closure-call
        // shape is `Stmt::Expr(Closure { body: vec![Stmt::Expr(Call(...))] })`,
        // so the prepend lands inside the closure body.)
        i += 1;
    }
}

pub(crate) fn rewrite_view_builder_calls_in_stmt(
    stmt: &mut Stmt,
    lookup: &HashMap<perry_hir::types::FuncId, &ViewBuilder>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            rewrite_view_builder_calls_in_expr(e, lookup);
        }
        Stmt::Let { init: Some(e), .. } => {
            rewrite_view_builder_calls_in_expr(e, lookup);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_stmts_with_lookup(then_branch, lookup);
            if let Some(eb) = else_branch {
                rewrite_view_builder_calls_in_stmts_with_lookup(eb, lookup);
            }
        }
        Stmt::While {
            condition, body, ..
        }
        | Stmt::DoWhile {
            body, condition, ..
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(i) = init {
                rewrite_view_builder_calls_in_stmt(i.as_mut(), lookup);
            }
            if let Some(c) = condition {
                rewrite_view_builder_calls_in_expr(c, lookup);
            }
            if let Some(u) = update {
                rewrite_view_builder_calls_in_expr(u, lookup);
            }
            rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        }
        _ => {}
    }
}

pub(crate) fn rewrite_view_builder_calls_in_expr(
    e: &mut Expr,
    lookup: &HashMap<perry_hir::types::FuncId, &ViewBuilder>,
) {
    // When we hit a closure: prepend a setContentView call for every
    // view-builder funcref called inside the closure's body, then recurse
    // into the body for any nested closures.
    if let Expr::Closure { body, .. } = e {
        // Collect all view-builder funcrefs called inside this closure.
        let mut called_builders: Vec<&ViewBuilder> = Vec::new();
        let mut seen: std::collections::HashSet<perry_hir::types::FuncId> =
            std::collections::HashSet::new();
        scan_closure_body_for_view_builder_calls(body, lookup, &mut called_builders, &mut seen);
        if !called_builders.is_empty() {
            // Prepend one setContentView call per unique view-builder
            // (deduped by func_id). Order: stable by sorted target_synth
            // so re-runs produce the same .ets bytes.
            let mut sorted = called_builders.clone();
            sorted.sort_by_key(|b| b.func_id);
            let prepends: Vec<Stmt> = sorted
                .iter()
                .map(|b| {
                    Stmt::Expr(Expr::NativeMethodCall {
                        module: "perry/arkts".to_string(),
                        class_name: None,
                        object: None,
                        method: "setContentView".to_string(),
                        args: vec![
                            Expr::String(b.target_synth.clone()),
                            Expr::String(b.view_id.clone()),
                        ],
                    })
                })
                .collect();
            let mut new_body = prepends;
            new_body.extend(std::mem::take(body));
            *body = new_body;
        }
        // Recurse into nested closures regardless.
        rewrite_view_builder_calls_in_stmts_with_lookup(body, lookup);
        return;
    }
    match e {
        Expr::Call { callee, args, .. } => {
            rewrite_view_builder_calls_in_expr(callee, lookup);
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                rewrite_view_builder_calls_in_expr(o, lookup);
            }
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        Expr::PropertyGet { object, .. } => {
            rewrite_view_builder_calls_in_expr(object, lookup);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            rewrite_view_builder_calls_in_expr(condition, lookup);
            rewrite_view_builder_calls_in_expr(then_expr, lookup);
            rewrite_view_builder_calls_in_expr(else_expr, lookup);
        }
        Expr::Binary { left, right, .. } | Expr::Logical { left, right, .. } => {
            rewrite_view_builder_calls_in_expr(left, lookup);
            rewrite_view_builder_calls_in_expr(right, lookup);
        }
        Expr::Unary { operand, .. } => {
            rewrite_view_builder_calls_in_expr(operand, lookup);
        }
        Expr::Array(items) => {
            for i in items.iter_mut() {
                rewrite_view_builder_calls_in_expr(i, lookup);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                rewrite_view_builder_calls_in_expr(v, lookup);
            }
        }
        Expr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_view_builder_calls_in_expr(a, lookup);
            }
        }
        _ => {}
    }
}

pub(crate) fn scan_closure_body_for_view_builder_calls<'a>(
    stmts: &[Stmt],
    lookup: &HashMap<perry_hir::types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    for stmt in stmts {
        scan_closure_body_for_view_builder_calls_in_stmt(stmt, lookup, out, seen);
    }
}

pub(crate) fn scan_closure_body_for_view_builder_calls_in_stmt<'a>(
    stmt: &Stmt,
    lookup: &HashMap<perry_hir::types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e)) => {
            scan_closure_body_for_view_builder_calls_in_expr(e, lookup, out, seen);
        }
        Stmt::Let { init: Some(e), .. } => {
            scan_closure_body_for_view_builder_calls_in_expr(e, lookup, out, seen);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_closure_body_for_view_builder_calls_in_expr(condition, lookup, out, seen);
            scan_closure_body_for_view_builder_calls(then_branch, lookup, out, seen);
            if let Some(eb) = else_branch {
                scan_closure_body_for_view_builder_calls(eb, lookup, out, seen);
            }
        }
        _ => {}
    }
}

pub(crate) fn scan_closure_body_for_view_builder_calls_in_expr<'a>(
    e: &Expr,
    lookup: &HashMap<perry_hir::types::FuncId, &'a ViewBuilder>,
    out: &mut Vec<&'a ViewBuilder>,
    seen: &mut std::collections::HashSet<perry_hir::types::FuncId>,
) {
    // Don't recurse into nested closures — their setContentView prepend
    // happens at their own level via `rewrite_view_builder_calls_in_expr`.
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    if let Expr::Call { callee, .. } = e {
        if let Expr::FuncRef(id) = callee.as_ref() {
            if let Some(b) = lookup.get(id) {
                if seen.insert(*id) {
                    out.push(*b);
                }
            }
        }
    }
    match e {
        Expr::Call { callee, args, .. } => {
            scan_closure_body_for_view_builder_calls_in_expr(callee, lookup, out, seen);
            for a in args {
                scan_closure_body_for_view_builder_calls_in_expr(a, lookup, out, seen);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                scan_closure_body_for_view_builder_calls_in_expr(o, lookup, out, seen);
            }
            for a in args {
                scan_closure_body_for_view_builder_calls_in_expr(a, lookup, out, seen);
            }
        }
        _ => {}
    }
}
