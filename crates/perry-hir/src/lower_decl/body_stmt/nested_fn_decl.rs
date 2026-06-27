use super::*;

fn function_has_use_strict(func: &ast::Function) -> bool {
    let Some(block) = func.body.as_ref() else {
        return false;
    };
    for stmt in &block.stmts {
        let Some(directive) = crate::lower::string_directive_stmt_lit(stmt) else {
            break;
        };
        if crate::lower::is_raw_use_strict_directive(directive) {
            return true;
        }
    }
    false
}

pub(super) fn lower_nested_fn_decl(
    ctx: &mut LoweringContext,
    fn_decl: &ast::FnDecl,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    let func_name = fn_decl.ident.sym.to_string();
    let func_id = ctx.fresh_func();

    // Register the function name temporarily so self-recursive calls
    // inside the body resolve to FuncRef(func_id).
    ctx.register_func(func_name.clone(), func_id);

    // Annex B B.3.3 (#5297): a *block-nested* `function f(){}` in sloppy mode is
    // block-scoped (its own binding) and ALSO writes the enclosing-scope `var f`
    // at its declaration point — while the block keeps its independent binding
    // (`f = 123` inside the body must not change the outer `var f`, per
    // `block-decl-*-block-scoping`). `annexb_block_fn_names_all` holds every
    // block-nested function name of the enclosing sloppy body; membership means
    // "give this a fresh block-local rather than reuse an enclosing same-named
    // binding" (so a `function f(){}` block-shadowing a parameter `f` doesn't
    // clobber it). The `annexb_block_fn_var_ids` subset additionally gets the
    // enclosing `var` write; names suppressed by a parameter/lexical conflict
    // are in `_all` but not the map (fresh local, no outer write).
    let is_block_nested = ctx.inside_block_scope > 0
        && !ctx.current_strict
        && ctx.annexb_block_fn_names_all.contains(&func_name);
    let annexb_outer_var = if is_block_nested {
        ctx.annexb_block_fn_var_ids.get(&func_name).copied()
    } else {
        None
    };

    // Define the local for the function name BEFORE lowering the body,
    // so self-recursive references inside the body resolve to
    // LocalGet(local_id) rather than FuncRef(func_id). This ensures
    // the LLVM backend's boxed-var analysis sees the same LocalId at
    // both the declaration and self-reference sites.
    let local_id = if is_block_nested {
        // Fresh block-local binding, independent of any enclosing same-named
        // parameter / `var` (the latter is written separately below).
        ctx.define_local(func_name.clone(), Type::Any)
    } else {
        ctx.lookup_local(&func_name)
            .unwrap_or_else(|| ctx.define_local(func_name.clone(), Type::Any))
    };

    let scope_mark = ctx.enter_scope();

    // Lower parameters. Skip the TypeScript `this:` annotation —
    // it has no runtime existence (see the sibling site above for
    // the full rationale).
    let mut params = Vec::new();
    let mut default_param_pats: Vec<ast::Pat> = Vec::new();
    let mut destructuring_params: Vec<(LocalId, ast::Pat)> = Vec::new();
    for param in &fn_decl.function.params {
        let param_name = get_pat_name(&param.pat)?;
        if param_name == "this" {
            continue;
        }
        let is_rest = is_rest_param(&param.pat);
        let param_id = ctx.define_local(param_name.clone(), Type::Any);
        ctx.shadow_native_instance_if_present(&param_name);
        ctx.shadow_native_module_if_present(&param_name);
        params.push(Param {
            id: param_id,
            name: param_name,
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest,
            arguments_object: None,
        });
        default_param_pats.push(param.pat.clone());
        // Unwrap a `Pat::Assign` (destructured param with a default, e.g.
        // `function f([x, y] = [1, 2]) {}`) so the destructuring binding is
        // still emitted; the default is applied via `get_param_default`.
        // Mirrors `lower_fn_decl`.
        let inner_pat = if let ast::Pat::Assign(assign) = &param.pat {
            assign.left.as_ref()
        } else {
            &param.pat
        };
        if is_destructuring_pattern(inner_pat) {
            destructuring_params.push((param_id, inner_pat.clone()));
        }
    }
    for (param, pat) in params.iter_mut().zip(default_param_pats.iter()) {
        param.default = get_param_default(ctx, pat)?;
    }

    // #677: synthesize `arguments` for nested function decls.
    let user_has_arguments_param = fn_decl
        .function
        .params
        .iter()
        .any(|p| get_pat_name(&p.pat).ok().as_deref() == Some("arguments"));
    let outer_strict = ctx.current_strict;
    let is_strict = outer_strict || function_has_use_strict(&fn_decl.function);
    let simple_parameters = params_are_simple_arguments_list(&fn_decl.function.params);
    let needs_arguments_synth = !user_has_arguments_param
        && fn_decl
            .function
            .body
            .as_ref()
            .map(|b| body_uses_arguments(&b.stmts))
            .unwrap_or(false);
    if needs_arguments_synth {
        let mapped = !is_strict && simple_parameters;
        let mapped_parameter_ids = if mapped {
            mapped_argument_parameter_ids(&params)
        } else {
            Vec::new()
        };
        append_synthetic_arguments_param(
            ctx,
            &mut params,
            is_strict,
            simple_parameters,
            !mapped,
            mapped_parameter_ids,
        );
    }

    // Generate destructuring stmts
    let mut destructuring_stmts = Vec::new();
    for (param_id, pat) in &destructuring_params {
        let stmts = generate_param_destructuring_stmts(ctx, pat, *param_id)?;
        destructuring_stmts.extend(stmts);
    }
    let destructuring_prologue_len = destructuring_stmts.len();

    ctx.current_strict = is_strict;

    // Lower body — see issue #569; hoist nested function-decl
    // statements within this inner fn body to the top so
    // forward refs and sibling captures work end-to-end.
    let mut body = if let Some(ref block) = fn_decl.function.body {
        lower_fn_body_block_stmt(ctx, block)?
    } else {
        Vec::new()
    };
    ctx.current_strict = outer_strict;

    if !destructuring_stmts.is_empty() {
        let mut new_body = destructuring_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    // Prepend defaulted-parameter application (`if (p === undefined) p =
    // <default>`). Mirrors `lower_fn_decl` (fn_decl.rs) — without it, a
    // `function f(a, opts = {})` nested in a block (which is what EVERY
    // top-level function becomes once cjs_wrap wraps the module body in an
    // IIFE) records `param.default` but never materializes the guard, so a
    // caller that omits the arg (or pads it with TAG_UNDEFINED) reads the
    // param as `undefined` instead of its default. This broke Next.js
    // `recursiveReadDir(dir)` → `setupFsCheck` → the whole server boot
    // (`Cannot convert undefined or null to object` destructuring the
    // dropped `options = {}`). Defaults run before any destructuring, so
    // prepend after the destructuring block (ending up first in the body).
    let default_stmts = build_default_param_stmts(&params);
    // Record the combined param-prologue length for generator function decls
    // (`function* g([x] = d) {}`) so the generator transform runs param binding
    // synchronously at call time (spec FunctionDeclarationInstantiation order).
    // Mirrors `lower_fn_expr_anon`; without it generator bodies skip their
    // default/destructuring param setup. See `Module.gen_param_prologue_len`.
    if fn_decl.function.is_generator {
        let prologue_len = default_stmts.len() + destructuring_prologue_len;
        if prologue_len > 0 {
            ctx.gen_param_prologue_len.insert(func_id, prologue_len);
        }
    }
    if !default_stmts.is_empty() {
        let mut new_body = default_stmts;
        new_body.append(&mut body);
        body = new_body;
    }

    ctx.exit_scope(scope_mark);

    // Detect captured variables
    let mut all_refs = Vec::new();
    let mut visited_closures = std::collections::HashSet::new();
    for stmt in &body {
        collect_local_refs_stmt(stmt, &mut all_refs, &mut visited_closures);
    }

    // The function's own scope has been popped (`exit_scope` above), so the
    // live `ctx.locals.id_set()` is exactly the enclosing scope's locals — the
    // membership view capture detection needs. Previously this was rebuilt into
    // a fresh `HashSet` from a per-closure cloned snapshot of `ctx.locals`,
    // which made capture analysis O(scope) per nested function = O(n²) over a
    // scope of n sibling functions (the perf bug behind this change).
    let outer_local_ids = ctx.locals.id_set();
    let param_ids: std::collections::HashSet<LocalId> = params.iter().map(|p| p.id).collect();

    // dayjs (issue: format() returned `292278994-08`): local
    // IDs are scope-local — see expr_function.rs
    // compute_closure_captures for the long explanation.
    // Strip locally-declared ids from the capture set so an
    // inner `var i = ...` doesn't collide with a same-id
    // outer constant.
    let inner_decls: std::collections::HashSet<LocalId> = {
        let mut s = std::collections::HashSet::new();
        for stmt in &body {
            collect_let_decls_in_stmt(stmt, &mut s);
        }
        s
    };

    let mut captures: Vec<LocalId> = all_refs
        .into_iter()
        .filter(|id| {
            outer_local_ids.contains(id) && !param_ids.contains(id) && !inner_decls.contains(id)
        })
        .collect();
    captures.sort();
    captures.dedup();
    captures = ctx.filter_module_level_captures(captures);

    // Detect mutable captures
    let mut all_assigned = Vec::new();
    for stmt in &body {
        collect_assigned_locals_stmt(stmt, &mut all_assigned);
    }
    let assigned_set: std::collections::HashSet<LocalId> = all_assigned.into_iter().collect();
    let mutable_captures: Vec<LocalId> = captures
        .iter()
        .filter(|id| assigned_set.contains(id) || ctx.var_hoisted_ids.contains(id))
        .copied()
        .collect();

    // Issue #838 followup (b): tag the function-decl's
    // local id as function-valued so the assignment
    // recogniser routes `M.prototype.x = fn` (and the
    // `var m = M.prototype` aliased form) through the
    // function-classic prototype-method path. Babel's
    // class-from-function emit pattern and dayjs's
    // minified bundle both lower `function M(){}` inside
    // an IIFE to exactly this `Stmt::Let { init:
    // Some(Closure{…}) }` shape — the destructuring.rs
    // path only fires for `var/let/const` lets, so the
    // tag has to be applied here too.
    ctx.function_valued_locals.insert(local_id);

    let closure = Expr::Closure {
        func_id,
        params,
        return_type: Type::Any,
        body,
        captures,
        mutable_captures,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: fn_decl.function.is_async,
        is_generator: fn_decl.function.is_generator,
        is_strict,
    };
    result.push(Stmt::Let {
        id: local_id,
        name: func_name,
        ty: Type::Any,
        init: Some(closure),
        mutable: false,
    });

    // Annex B B.3.3 (#5297): copy the just-bound block-local function into the
    // enclosing-scope `var` so the name is visible (as the function) outside the
    // block, while later mutation of the block-local stays local.
    if let Some(outer_id) = annexb_outer_var {
        result.push(Stmt::Expr(Expr::LocalSet(
            outer_id,
            Box::new(Expr::LocalGet(local_id)),
        )));
    }

    Ok(())
}
