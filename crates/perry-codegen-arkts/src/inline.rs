// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Phase 2 v6 — discover top-level `let x = state(initial)` declarations
/// and assign each a synthetic id `__state_<N>`. The initial value is
/// stringified for the v3.2 reactive-Text initial state.
/// Build an analysis-only copy of `module.init` with calls to user-defined
/// module-level functions expanded inline. The harvest's collectors
/// (collect_const_bindings, collect_mutations, collect_compile_time_consts)
/// run against this expanded view so widget mutations inside function
/// bodies are seen — but `module.init` itself stays untouched so the
/// downstream LLVM codegen sees the original program semantics.
///
/// Bounds: skip async/generator, ≤16 inlines per harvest call, skip
/// recursive calls. Param substitution lands as synthesized `Stmt::Let`
/// BEFORE the cloned body so collect_const_bindings picks them up the
/// same way as real top-level lets.
pub(crate) fn inlined_analysis_init(module: &Module) -> Vec<Stmt> {
    use perry_hir::analysis::remap_local_ids_in_stmts;
    use perry_hir::types::FuncId;
    use std::collections::HashSet;

    let mut function_map: HashMap<FuncId, perry_hir::ir::Function> = HashMap::new();
    for f in &module.functions {
        if f.is_async || f.is_generator {
            continue;
        }
        function_map.insert(f.id, f.clone());
    }
    if function_map.is_empty() {
        return module.init.clone();
    }

    let mut next_local: u32 = max_local_id_in_module(module).saturating_add(1);
    // Inline budget — bumped from 32 → 256 to handle Mango's full
    // refreshConnectionList (which transitively expands to dozens of
    // makePill / makeLabel / makeMuted / makeCard / makeDangerBtn /
    // makePrimaryBtn calls). Each inline operation is bounded; the
    // overall HIR size is a hard upper bound on how many calls can
    // possibly land. 256 is comfortably above the worst-case Mango
    // shape (verified at v0.5.491: ~25K bytes Index.ets, ~40 inlines).
    let mut budget: usize = 256;
    let mut visited: HashSet<FuncId> = HashSet::new();

    // Phase A: top-level `Stmt::Expr(Call(FuncRef))` inlining (the
    // original v0.5.489 behavior). For each top-level user-function
    // call, splice the body in place of the call statement.
    let mut new_init: Vec<Stmt> = Vec::with_capacity(module.init.len());
    for stmt in &module.init {
        if budget == 0 {
            new_init.push(stmt.clone());
            continue;
        }
        match stmt {
            Stmt::Expr(Expr::Call { callee, args, .. }) => {
                let func_id = match callee.as_ref() {
                    Expr::FuncRef(id) => Some(*id),
                    _ => None,
                };
                let Some(id) = func_id else {
                    new_init.push(stmt.clone());
                    continue;
                };
                if visited.contains(&id) {
                    new_init.push(stmt.clone());
                    continue;
                }
                let Some(func) = function_map.get(&id) else {
                    new_init.push(stmt.clone());
                    continue;
                };
                if func.params.len() != args.len() {
                    new_init.push(stmt.clone());
                    continue;
                }
                visited.insert(id);
                let inlined =
                    inline_one_call(func, args, &mut next_local, &remap_local_ids_in_stmts);
                visited.remove(&id);
                new_init.extend(inlined);
                budget -= 1;
            }
            _ => new_init.push(stmt.clone()),
        }
    }

    // Phase B: expression-level inlining inside the inlined bodies.
    // Walks every Stmt's expressions and substitutes:
    //   - `Expr::Call { callee: FuncRef(id) }` (top-level fn call)
    //   - `Expr::Call { callee: LocalGet(id) }` where bindings[id]
    //     is `Expr::Closure { ... }` (Mango's `function makePill`
    //     nested inside refreshConnectionList lowers to this shape:
    //     `Stmt::Let { id: 297, init: Closure { ... } }`)
    // with the function's return value, hoisting the body's let-and-
    // mutator statements BEFORE the enclosing Stmt.
    // Mango's pattern: `const pillRow = HStack(8, [makePill('A'),
    // makePill('B')])` — each makePill call's body gets hoisted, and
    // the call expression is replaced with `LocalGet(remapped_pill)`.
    // Both calls run before pillRow is constructed, so the array
    // literal's items resolve cleanly.
    let local_bindings_for_inline = collect_const_bindings(&new_init);
    new_init = expr_level_inline_pass(
        new_init,
        &function_map,
        &local_bindings_for_inline,
        &mut next_local,
        &mut budget,
    );

    new_init
}

/// Phase B of inlining: walk every Stmt's expressions and substitute
/// `Expr::Call { callee: FuncRef(id) }` with the function's return
/// value, hoisting the function body's statements BEFORE the enclosing
/// Stmt. Each found Call gets its body inlined (with locals remapped
/// to fresh ids) and the call expression is replaced with `LocalGet(
/// remapped_return_id)`.
///
/// The function's return value is detected by walking its body
/// backwards looking for `Stmt::Return(Some(expr))`. If the expr is
/// `Expr::LocalGet(id)`, that local id is the return target (after
/// remapping). If the body has no `Return` or returns a non-LocalGet
/// expression, the call is left as-is — the simple-shape constraint
/// covers Mango's makePill but punts on more complex returning fns.
pub(crate) fn expr_level_inline_pass(
    stmts: Vec<Stmt>,
    function_map: &HashMap<perry_hir::types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts {
        if *budget == 0 {
            out.push(stmt);
            continue;
        }
        let mut hoists: Vec<Stmt> = Vec::new();
        inline_calls_in_stmt(
            &mut stmt,
            function_map,
            bindings,
            next_local,
            budget,
            &mut hoists,
        );
        out.extend(hoists);
        out.push(stmt);
    }
    out
}

pub(crate) fn inline_calls_in_stmt(
    stmt: &mut Stmt,
    function_map: &HashMap<perry_hir::types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
    hoists: &mut Vec<Stmt>,
) {
    match stmt {
        Stmt::Let { init: Some(e), .. } => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists);
        }
        Stmt::Expr(e) => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists)
        }
        Stmt::Return(Some(e)) => {
            inline_calls_in_expr(e, function_map, bindings, next_local, budget, hoists)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            inline_calls_in_expr(
                condition,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            // Recurse into branches: their own hoists land within the
            // branch, not above the if.
            *then_branch = expr_level_inline_pass(
                std::mem::take(then_branch),
                function_map,
                bindings,
                next_local,
                budget,
            );
            if let Some(eb) = else_branch {
                *eb = expr_level_inline_pass(
                    std::mem::take(eb),
                    function_map,
                    bindings,
                    next_local,
                    budget,
                );
            }
        }
        _ => {}
    }
}

pub(crate) fn inline_calls_in_expr(
    expr: &mut Expr,
    function_map: &HashMap<perry_hir::types::FuncId, perry_hir::ir::Function>,
    bindings: &HashMap<LocalId, Expr>,
    next_local: &mut u32,
    budget: &mut usize,
    hoists: &mut Vec<Stmt>,
) {
    use perry_hir::analysis::remap_local_ids_in_stmts;
    // First descend into sub-expressions (post-order: children inlined
    // first so a call's args might themselves be inlined calls).
    match expr {
        Expr::Call { callee, args, .. } => {
            for a in args.iter_mut() {
                inline_calls_in_expr(a, function_map, bindings, next_local, budget, hoists);
            }
            inline_calls_in_expr(callee, function_map, bindings, next_local, budget, hoists);
        }
        Expr::NativeMethodCall { args, object, .. } => {
            for a in args.iter_mut() {
                inline_calls_in_expr(a, function_map, bindings, next_local, budget, hoists);
            }
            if let Some(obj) = object {
                inline_calls_in_expr(obj, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Array(items) => {
            for item in items.iter_mut() {
                inline_calls_in_expr(item, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props.iter_mut() {
                inline_calls_in_expr(v, function_map, bindings, next_local, budget, hoists);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            inline_calls_in_expr(
                condition,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            inline_calls_in_expr(
                then_expr,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
            inline_calls_in_expr(
                else_expr,
                function_map,
                bindings,
                next_local,
                budget,
                hoists,
            );
        }
        _ => {}
    }
    // Now check if THIS expression is a Call we can inline. Resolve
    // the callee through:
    //   - Expr::FuncRef(id): module-level user function in function_map
    //   - Expr::LocalGet(id) → bindings[id] = Expr::Closure: nested
    //     function declared via `function name() { ... }` inside
    //     another function (Mango's `makePill` shape — lowered to
    //     `Stmt::Let { id, init: Closure { params, body, ... } }`).
    if *budget == 0 {
        return;
    }
    let (params, body, args, is_strict) = match expr {
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::FuncRef(id) => match function_map.get(id) {
                Some(func) if func.params.len() == args.len() => (
                    func.params.clone(),
                    func.body.clone(),
                    args.clone(),
                    func.is_strict,
                ),
                _ => return,
            },
            Expr::LocalGet(local_id) => match bindings.get(local_id) {
                Some(Expr::Closure {
                    params,
                    body,
                    is_strict,
                    is_async: false,
                    ..
                }) if params.len() == args.len() => {
                    (params.clone(), body.clone(), args.clone(), *is_strict)
                }
                _ => return,
            },
            _ => return,
        },
        _ => return,
    };
    // Detect simple-return shape: last `Stmt::Return(Some(LocalGet(id)))`.
    // Anything else (no return, return non-local, multiple returns) is
    // out of scope — leaves the call as-is.
    if !matches!(body.last(), Some(Stmt::Return(Some(Expr::LocalGet(_))))) {
        return;
    }
    // Build a synthetic Function so `inline_one_call` can do its
    // standard remapping work. Re-uses the existing helper rather
    // than duplicating the local-id offset / param-substitution logic.
    let synth_func = perry_hir::ir::Function {
        id: 0,
        name: String::new(),
        type_params: Vec::new(),
        params,
        return_type: perry_hir::types::Type::Any,
        body,
        is_strict,
        is_async: false,
        is_generator: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    };
    let mut inlined = inline_one_call(&synth_func, &args, next_local, &remap_local_ids_in_stmts);
    let return_remapped = match inlined.last() {
        Some(Stmt::Return(Some(Expr::LocalGet(id)))) => *id,
        _ => return,
    };
    inlined.pop(); // drop the trailing Return
    hoists.extend(inlined);
    *expr = Expr::LocalGet(return_remapped);
    *budget -= 1;
}

pub(crate) fn inline_one_call(
    func: &perry_hir::ir::Function,
    call_args: &[Expr],
    next_local: &mut u32,
    remap_fn: &dyn Fn(&mut Vec<Stmt>, &HashMap<u32, u32>),
) -> Vec<Stmt> {
    let mut local_ids: Vec<u32> = Vec::new();
    for param in &func.params {
        local_ids.push(param.id);
    }
    collect_local_ids_in_stmts(&func.body, &mut local_ids);
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    local_ids.retain(|id| seen.insert(*id));

    let mut remap: HashMap<u32, u32> = HashMap::new();
    for &id in &local_ids {
        remap.insert(id, *next_local);
        *next_local += 1;
    }

    // Rewrite early-return patterns to if/else before remapping. Mango's
    // `refreshConnectionList`:
    //
    //     if (connectionNames.length === 0) {
    //         /* welcome card */
    //         widgetAddChild(connListContainer, welcomeCard);
    //         return;
    //     }
    //     const sectionTitle = ...;
    //     widgetAddChild(connListContainer, sectionTitle);
    //     /* connection-list build */
    //     widgetAddChild(connListContainer, addMoreBtn);
    //
    // After rewrite the rest of the body becomes the else branch:
    //
    //     if (connectionNames.length === 0) {
    //         /* welcome card */
    //         widgetAddChild(connListContainer, welcomeCard);
    //     } else {
    //         const sectionTitle = ...;
    //         /* ... */
    //         widgetAddChild(connListContainer, addMoreBtn);
    //     }
    //
    // collect_mutations's dead-branch elim then picks ONE branch (the
    // then-branch heuristic when the condition is unfoldable / unclean
    // serializable). Without this rewrite both the welcomeCard's CTA
    // button AND the addMoreBtn would render unconditionally because
    // the addMoreBtn lives at the function-body sibling level rather
    // than inside an else block.
    let mut body = rewrite_early_returns(func.body.clone());
    remap_fn(&mut body, &remap);
    // perry-hir's `remap_local_ids_in_stmts` only walks Stmt::Let's init,
    // not the Let's `id` field itself (its #212 design constraint:
    // outer-scope captured ids shouldn't get rewritten when remapping
    // inner-scope refs). We need both — the Let creates a new binding
    // and the LocalGets that reference it must agree. Walk the inlined
    // body once more and rewrite any Stmt::Let / catch-param / for-init
    // ids that match the remap.
    remap_let_ids_in_stmts(&mut body, &remap);

    let mut out: Vec<Stmt> = Vec::with_capacity(func.params.len() + body.len());
    for (i, param) in func.params.iter().enumerate() {
        let new_id = remap[&param.id];
        out.push(Stmt::Let {
            id: new_id,
            name: param.name.clone(),
            ty: param.ty.clone(),
            mutable: false,
            init: Some(call_args[i].clone()),
        });
    }
    out.extend(body);
    out
}

pub(crate) fn max_local_id_in_module(module: &Module) -> u32 {
    let mut buf: Vec<u32> = Vec::new();
    collect_local_ids_in_stmts(&module.init, &mut buf);
    for f in &module.functions {
        for p in &f.params {
            buf.push(p.id);
        }
        collect_local_ids_in_stmts(&f.body, &mut buf);
    }
    for c in &module.classes {
        if let Some(ctor) = &c.constructor {
            for p in &ctor.params {
                buf.push(p.id);
            }
            collect_local_ids_in_stmts(&ctor.body, &mut buf);
        }
        for m in &c.methods {
            for p in &m.params {
                buf.push(p.id);
            }
            collect_local_ids_in_stmts(&m.body, &mut buf);
        }
    }
    buf.into_iter().max().unwrap_or(0)
}

/// Rewrite `if (cond) { ...; return; } <rest>` → `if (cond) { ... }
/// else { <rest> }` so dead-branch elim can correctly drop one or the
/// other. Stops walking after the first such pattern (the rest moved
/// into the else). Recurses into nested if/else / for / while bodies.
pub(crate) fn rewrite_early_returns(stmts: Vec<Stmt>) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    let mut iter = stmts.into_iter();
    while let Some(stmt) = iter.next() {
        match stmt {
            Stmt::If {
                condition,
                then_branch,
                else_branch: None,
            } if matches!(then_branch.last(), Some(Stmt::Return(_))) => {
                // Found the early-return pattern. Pull the trailing
                // return out of the then-branch (it's redundant once
                // the rest is in the else), and gather all remaining
                // siblings into a new else-branch. Recurse into both
                // branches so nested patterns are handled too.
                let mut new_then = rewrite_early_returns(then_branch);
                new_then.pop(); // drop the trailing Return
                let rest: Vec<Stmt> = iter.collect();
                let new_else = rewrite_early_returns(rest);
                out.push(Stmt::If {
                    condition,
                    then_branch: new_then,
                    else_branch: Some(new_else),
                });
                return out;
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let new_then = rewrite_early_returns(then_branch);
                let new_else = else_branch.map(rewrite_early_returns);
                out.push(Stmt::If {
                    condition,
                    then_branch: new_then,
                    else_branch: new_else,
                });
            }
            Stmt::While { condition, body } => {
                out.push(Stmt::While {
                    condition,
                    body: rewrite_early_returns(body),
                });
            }
            Stmt::DoWhile { body, condition } => {
                out.push(Stmt::DoWhile {
                    body: rewrite_early_returns(body),
                    condition,
                });
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                out.push(Stmt::For {
                    init,
                    condition,
                    update,
                    body: rewrite_early_returns(body),
                });
            }
            other => out.push(other),
        }
    }
    out
}

/// Walk Stmt::Let / catch-param / Stmt::For-init looking for declared
/// local ids that match the remap; rewrite them in place. Sibling to
/// `perry_hir::analysis::remap_local_ids_in_stmts` which only remaps
/// LocalGet / LocalSet / Update references — not the declarations.
/// The inliner needs both: the cloned body's let creates a new binding
/// and the references to it must agree.
pub(crate) fn remap_let_ids_in_stmts(stmts: &mut Vec<Stmt>, remap: &HashMap<u32, u32>) {
    for s in stmts.iter_mut() {
        remap_let_ids_in_stmt(s, remap);
    }
}

pub(crate) fn remap_let_ids_in_stmt(stmt: &mut Stmt, remap: &HashMap<u32, u32>) {
    match stmt {
        Stmt::Let { id, .. } => {
            if let Some(&new_id) = remap.get(id) {
                *id = new_id;
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            remap_let_ids_in_stmts(then_branch, remap);
            if let Some(eb) = else_branch {
                remap_let_ids_in_stmts(eb, remap);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            remap_let_ids_in_stmts(body, remap);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                remap_let_ids_in_stmt(init_stmt.as_mut(), remap);
            }
            remap_let_ids_in_stmts(body, remap);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            remap_let_ids_in_stmts(body, remap);
            if let Some(c) = catch {
                if let Some((id, _)) = &mut c.param {
                    if let Some(&new_id) = remap.get(id) {
                        *id = new_id;
                    }
                }
                remap_let_ids_in_stmts(&mut c.body, remap);
            }
            if let Some(f) = finally {
                remap_let_ids_in_stmts(f, remap);
            }
        }
        _ => {}
    }
}

pub(crate) fn collect_local_ids_in_stmts(stmts: &[Stmt], out: &mut Vec<u32>) {
    for s in stmts {
        match s {
            Stmt::Let { id, .. } => out.push(*id),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_local_ids_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_local_ids_in_stmts(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_local_ids_in_stmts(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                        out.push(*id);
                    }
                }
                collect_local_ids_in_stmts(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_local_ids_in_stmts(body, out);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        out.push(*id);
                    }
                    collect_local_ids_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_local_ids_in_stmts(f, out);
                }
            }
            _ => {}
        }
    }
}
