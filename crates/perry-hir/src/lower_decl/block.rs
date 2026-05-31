use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::*;

pub fn lower_block_stmt(ctx: &mut LoweringContext, block: &ast::BlockStmt) -> Result<Vec<Stmt>> {
    lower_stmts_using_aware(ctx, &block.stmts)
}

/// Lower a function-body block, with support for ECMAScript function-decl
/// hoisting (issue #569). Pre-defines locals for every non-generator
/// `function name() {...}` at the block's top level so forward-reference
/// callsites resolve at HIR lowering time, then after the body is lowered
/// rearranges the resulting `Vec<Stmt>` so the hoisted FnDecls' `Stmt::Let`
/// entries appear before any other top-level statement (matching JS spec
/// "function declarations are hoisted AND initialized at function entry").
///
/// Sibling/forward captures need their box pre-allocated at the function
/// entry so the hoisted closure's `captures` list can stash a stable box
/// pointer instead of a TAG_UNDEFINED snapshot of the not-yet-run `Stmt::
/// Let`. We compute the set of (a) hoisted FnDecl ids referenced from any
/// closure body in the function, plus (b) function-body lets/consts
/// captured by any hoisted closure, and emit a synthetic `Stmt::Preallocate
/// Boxes(...)` at the very top of the result. Codegen consumes that variant
/// to alloca a slot+box for each id before any user statement runs.
pub fn lower_fn_body_block_stmt(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Vec<Stmt>> {
    use std::collections::HashSet;

    let parent_strict = ctx.strict_mode;
    ctx.strict_mode = parent_strict || crate::lower::block_has_use_strict_directive(&block.stmts);

    // Phase 1: pre-define hoisted FnDecl locals so forward references in
    // any earlier statement resolve via `lookup_local`. Generator and
    // async-generator FnDecls are excluded — those go through the
    // hoist-to-top-level + FuncRef path in `lower_body_stmt` and aren't
    // closure-bound at the source position.
    let mut hoisted_id_set: HashSet<LocalId> = HashSet::new();
    for stmt in &block.stmts {
        if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
            if fn_decl.function.body.is_none() || fn_decl.function.is_generator {
                continue;
            }
            let name = fn_decl.ident.sym.to_string();
            let local_id = if let Some(existing) = ctx.lookup_local(&name) {
                existing
            } else {
                ctx.define_local(name.clone(), Type::Any)
            };
            hoisted_id_set.insert(local_id);
        }
    }

    // Phase 2: lower the body. The inner FnDecl arm in `lower_body_stmt`
    // calls `lookup_local(name)` and reuses our pre-defined id.
    let body = match lower_block_stmt(ctx, block) {
        Ok(body) => body,
        Err(err) => {
            ctx.strict_mode = parent_strict;
            return Err(err);
        }
    };

    if hoisted_id_set.is_empty() {
        ctx.strict_mode = parent_strict;
        return Ok(body);
    }

    // Phase 3: split — pull every top-level `Stmt::Let` whose id is in the
    // hoisted set to the front (preserving relative source order).
    let mut hoisted_lets: Vec<Stmt> = Vec::new();
    let mut other: Vec<Stmt> = Vec::new();
    for s in body {
        let is_hoisted = matches!(
            &s,
            Stmt::Let { id, init: Some(Expr::Closure { .. }), .. }
                if hoisted_id_set.contains(id)
        );
        if is_hoisted {
            hoisted_lets.push(s);
        } else {
            other.push(s);
        }
    }

    // Phase 4: compute the prealloc-box set via shared helper.
    let combined: Vec<Stmt> = hoisted_lets.iter().chain(other.iter()).cloned().collect();
    let prealloc = compute_prealloc_for_hoisted_closures(&combined, &hoisted_id_set);

    // Phase 5: assemble the final body — PreallocateBoxes (if any),
    // then the hoisted FnDecl Lets, then everything else.
    let mut result: Vec<Stmt> = Vec::new();
    if !prealloc.is_empty() {
        result.push(Stmt::PreallocateBoxes(prealloc));
    }
    result.extend(hoisted_lets);
    result.extend(other);
    ctx.strict_mode = parent_strict;
    Ok(result)
}

/// Compute the prealloc-box set for a function/arrow/fn-expr body that
/// performs ECMAScript function-decl hoisting. `body` is the already-
/// hoisted body (with FnDecl `Stmt::Let`s ahead of other top-level
/// stmts); `hoisted_id_set` is the set of LocalIds those FnDecls were
/// hoisted under. Returns the sorted list of LocalIds that need a
/// pre-allocated box at function entry — covers both (a) hoisted FnDecl
/// ids referenced from any closure body in this function (sibling
/// recursion), and (b) function-body let/const ids captured by any
/// hoisted closure (the closure literal is built before the let's source
/// position, so the let's box must already exist).
///
/// Issue #633 followup: previously only `lower_fn_body_block_stmt`
/// (function-decl bodies) emitted the prealloc; arrow-fn and fn-expr
/// bodies did their own hoisting inline and skipped this analysis,
/// leading to capture-of-uninitialized-slot for hoisted async fn decls
/// that captured outer `let`s (the dispatch chain pattern in hono
/// `compose()`).
pub fn compute_prealloc_for_hoisted_closures(
    body: &[Stmt],
    hoisted_id_set: &std::collections::HashSet<LocalId>,
) -> Vec<LocalId> {
    use std::collections::HashSet;

    let mut closure_body_refs: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_refs_in_closure_bodies_stmt(s, &mut closure_body_refs);
    }

    let mut body_let_ids: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_top_level_let_ids_stmt(s, &mut body_let_ids);
    }

    let mut prealloc_set: HashSet<LocalId> = HashSet::new();
    for &id in hoisted_id_set {
        if closure_body_refs.contains(&id) {
            prealloc_set.insert(id);
        }
    }
    for s in body {
        if let Stmt::Let {
            id,
            init: Some(Expr::Closure { captures, .. }),
            ..
        } = s
        {
            if hoisted_id_set.contains(id) {
                for &cap in captures {
                    if body_let_ids.contains(&cap) && !hoisted_id_set.contains(&cap) {
                        prealloc_set.insert(cap);
                    }
                }
            }
        }
    }

    let mut prealloc: Vec<LocalId> = prealloc_set.into_iter().collect();
    prealloc.sort();
    prealloc
}

/// Collect every `LocalId` referenced (LocalGet / LocalSet / Update / etc.)
/// from inside any `Expr::Closure` body found within `stmt`. Used by
/// `lower_fn_body_block_stmt` to decide which hoisted FnDecl ids need a
/// pre-allocated box.
pub fn collect_refs_in_closure_bodies_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => collect_refs_in_closure_bodies_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_refs_in_closure_bodies_expr(e, out);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_refs_in_closure_bodies_expr(e, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_refs_in_closure_bodies_expr(condition, out);
            for s in then_branch {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_refs_in_closure_bodies_expr(condition, out);
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_refs_in_closure_bodies_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_refs_in_closure_bodies_expr(c, out);
            }
            if let Some(u) = update {
                collect_refs_in_closure_bodies_expr(u, out);
            }
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_refs_in_closure_bodies_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_refs_in_closure_bodies_expr(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_refs_in_closure_bodies_expr(t, out);
                }
                for s in &case.body {
                    collect_refs_in_closure_bodies_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_refs_in_closure_bodies_stmt(body, out),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn collect_refs_in_closure_bodies_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    if let Expr::Closure { body, .. } = expr {
        // Inside a closure body — collect every reference (including refs
        // from any further-nested closures, since those run when the outer
        // closure runs, after the function body has set up bindings).
        let mut tmp_refs: Vec<LocalId> = Vec::new();
        let mut visited = std::collections::HashSet::new();
        for s in body {
            collect_local_refs_stmt(s, &mut tmp_refs, &mut visited);
        }
        for id in tmp_refs {
            out.insert(id);
        }
        return;
    }
    crate::walker::walk_expr_children(expr, &mut |child| {
        collect_refs_in_closure_bodies_expr(child, out)
    });
}

/// Collect `LocalId`s declared by a top-level `Stmt::Let` in `stmt`. Does
/// NOT recurse into nested blocks (those are block-scoped — their lets
/// aren't hoisted to function-entry).
pub fn collect_top_level_let_ids_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    if let Stmt::Let { id, .. } = stmt {
        out.insert(*id);
    }
}

/// Lower a block statement that introduces its own lexical scope for
/// `let`/`const`. Inner bindings shadow outer ones and are removed on exit.
/// `var` declarations remain visible (function-scoped).
pub fn lower_block_stmt_scoped(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Result<Vec<Stmt>> {
    let mark = ctx.push_block_scope();
    let stmts = lower_stmts_using_aware(ctx, &block.stmts)?;
    ctx.pop_block_scope(mark);
    Ok(stmts)
}

/// Lower a sequence of body statements, desugaring `using` / `await using`
/// declarations into nested try/finally blocks that invoke the bound value's
/// `[Symbol.dispose]()` (sync `using`) or `await [Symbol.asyncDispose]()`
/// (`await using`) on block exit, in reverse declaration order. Issue #154.
///
/// Class methods written as `[Symbol.dispose]()` / `[Symbol.asyncDispose]()`
/// are renamed at lowering time (`lower_class_method`) to the stable string
/// names `__perry_dispose__` / `__perry_async_dispose__` so this desugarer
/// can dispatch via plain `obj.__perry_dispose__()` method calls.
///
/// Bindings whose initializer evaluates to `null` or `undefined` are skipped
/// per spec (no dispose call, no error). Multi-binding using declarations
/// (`using a = e1, b = e2`) are unrolled left-to-right with each binding
/// getting its own try/finally so the rightmost disposes first. SuppressedError
/// chaining when a body throw is followed by a dispose throw is not yet
/// implemented — the dispose throw shadows the original.
pub fn lower_stmts_using_aware(
    ctx: &mut LoweringContext,
    stmts: &[ast::Stmt],
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if let ast::Stmt::Decl(ast::Decl::Using(using_decl)) = stmt {
            let is_async = using_decl.is_await;
            let mut binding_ids: Vec<LocalId> = Vec::new();
            for decl in &using_decl.decls {
                if !matches!(&decl.name, ast::Pat::Ident(_)) {
                    bail!("`using` / `await using` requires an identifier binding");
                }
                // Reuse lower_var_decl_with_destructuring so the binding's type
                // is inferred from `new ClassName(...)` initializers — that
                // makes `obj.__perry_dispose__()` route through static class-
                // method dispatch (`receiver_class_name` returns the class name
                // for `Type::Named` locals; without inference it stays `Any`
                // and the call goes nowhere on missing-method).
                let stmts = lower_var_decl_with_destructuring(ctx, decl, false)?;
                for s in &stmts {
                    if let Stmt::Let { id, .. } = s {
                        binding_ids.push(*id);
                    }
                }
                result.extend(stmts);
            }
            // Recursively lower remaining stmts as the try body.
            let body_stmts = lower_stmts_using_aware(ctx, &stmts[i + 1..])?;
            // Wrap each binding in its own try/finally — innermost (rightmost
            // binding) finally runs first, giving reverse-declaration disposal.
            let mut wrapped = body_stmts;
            for &id in binding_ids.iter().rev() {
                let method_name = if is_async {
                    "__perry_async_dispose__"
                } else {
                    "__perry_dispose__"
                };
                // if (id !== null && id !== undefined) [await] id.<method>()
                let null_check = Expr::Logical {
                    op: LogicalOp::And,
                    left: Box::new(Expr::Compare {
                        op: CompareOp::Ne,
                        left: Box::new(Expr::LocalGet(id)),
                        right: Box::new(Expr::Null),
                    }),
                    right: Box::new(Expr::Compare {
                        op: CompareOp::Ne,
                        left: Box::new(Expr::LocalGet(id)),
                        right: Box::new(Expr::Undefined),
                    }),
                };
                let mut call_expr = Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        object: Box::new(Expr::LocalGet(id)),
                        property: method_name.to_string(),
                    }),
                    args: Vec::new(),
                    type_args: Vec::new(),
                };
                if is_async {
                    call_expr = Expr::Await(Box::new(call_expr));
                }
                let finally_stmts = vec![Stmt::If {
                    condition: null_check,
                    then_branch: vec![Stmt::Expr(call_expr)],
                    else_branch: None,
                }];
                wrapped = vec![Stmt::Try {
                    body: wrapped,
                    catch: None,
                    finally: Some(finally_stmts),
                }];
            }
            result.extend(wrapped);
            return Ok(result);
        }
        result.extend(lower_body_stmt(ctx, stmt)?);
    }
    Ok(result)
}
