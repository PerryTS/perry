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

fn collect_var_binding_names_from_pat(pat: &ast::Pat, out: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(ident) => out.push(ident.id.sym.to_string()),
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_var_binding_names_from_pat(elem, out);
            }
        }
        ast::Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::ObjectPatProp::Assign(assign) => out.push(assign.key.sym.to_string()),
                    ast::ObjectPatProp::KeyValue(kv) => {
                        collect_var_binding_names_from_pat(&kv.value, out)
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        collect_var_binding_names_from_pat(&rest.arg, out)
                    }
                }
            }
        }
        ast::Pat::Assign(assign) => collect_var_binding_names_from_pat(&assign.left, out),
        ast::Pat::Rest(rest) => collect_var_binding_names_from_pat(&rest.arg, out),
        _ => {}
    }
}

fn collect_var_binding_names_from_var_decl(var_decl: &ast::VarDecl, out: &mut Vec<String>) {
    if var_decl.kind != ast::VarDeclKind::Var {
        return;
    }
    for decl in &var_decl.decls {
        collect_var_binding_names_from_pat(&decl.name, out);
    }
}

pub(crate) fn collect_var_binding_names_from_stmt(stmt: &ast::Stmt, out: &mut Vec<String>) {
    match stmt {
        ast::Stmt::Block(block) => {
            for stmt in &block.stmts {
                collect_var_binding_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
            collect_var_binding_names_from_var_decl(var_decl, out);
        }
        // Nested function/class bodies have their own var environments.
        ast::Stmt::Decl(ast::Decl::Fn(_)) | ast::Stmt::Decl(ast::Decl::Class(_)) => {}
        ast::Stmt::If(if_stmt) => {
            collect_var_binding_names_from_stmt(&if_stmt.cons, out);
            if let Some(alt) = &if_stmt.alt {
                collect_var_binding_names_from_stmt(alt, out);
            }
        }
        ast::Stmt::While(while_stmt) => collect_var_binding_names_from_stmt(&while_stmt.body, out),
        ast::Stmt::DoWhile(do_while) => collect_var_binding_names_from_stmt(&do_while.body, out),
        ast::Stmt::For(for_stmt) => {
            if let Some(ast::VarDeclOrExpr::VarDecl(var_decl)) = &for_stmt.init {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_stmt.body, out);
        }
        ast::Stmt::ForIn(for_in) => {
            if let ast::ForHead::VarDecl(var_decl) = &for_in.left {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_in.body, out);
        }
        ast::Stmt::ForOf(for_of) => {
            if let ast::ForHead::VarDecl(var_decl) = &for_of.left {
                collect_var_binding_names_from_var_decl(var_decl, out);
            }
            collect_var_binding_names_from_stmt(&for_of.body, out);
        }
        ast::Stmt::Labeled(labeled) => collect_var_binding_names_from_stmt(&labeled.body, out),
        ast::Stmt::Switch(switch_stmt) => {
            for case in &switch_stmt.cases {
                for stmt in &case.cons {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
        }
        ast::Stmt::Try(try_stmt) => {
            for stmt in &try_stmt.block.stmts {
                collect_var_binding_names_from_stmt(stmt, out);
            }
            if let Some(handler) = &try_stmt.handler {
                for stmt in &handler.body.stmts {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                for stmt in &finalizer.stmts {
                    collect_var_binding_names_from_stmt(stmt, out);
                }
            }
        }
        ast::Stmt::With(with_stmt) => collect_var_binding_names_from_stmt(&with_stmt.body, out),
        _ => {}
    }
}

/// Returns the (name, id) pairs newly created here (i.e. names that did not
/// already have a binding in the current scope, like a same-named param).
/// The caller emits an undefined-initialised `Stmt::Let` for each at body
/// entry: codegen creates local storage at the first `Stmt::Let` for an id,
/// so a read compiled before the nested decl (`if (c) break;` ahead of
/// `var c = ...` in the same loop body) would otherwise bake in an
/// `undefined` constant and never observe the later write.
fn predefine_var_bindings_in_function_body(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
) -> Vec<(String, LocalId)> {
    let mut names = Vec::new();
    for stmt in &block.stmts {
        collect_var_binding_names_from_stmt(stmt, &mut names);
    }
    names.sort();
    names.dedup();

    let mut created = Vec::new();
    let scope_start = ctx.scope_local_marks.last().copied().unwrap_or(0);
    for name in names {
        let existing_current_scope = ctx.locals[scope_start..]
            .iter()
            .rev()
            .find(|(n, _, _)| n == &name)
            .map(|(_, id, _)| *id);
        let local_id = existing_current_scope.unwrap_or_else(|| {
            let id = ctx.define_local(name.clone(), Type::Any);
            created.push((name, id));
            id
        });
        ctx.var_hoisted_ids.insert(local_id);
    }
    created
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

    let parent_strict = ctx.current_strict;
    ctx.current_strict =
        parent_strict || crate::lower::stmt_list_starts_with_use_strict_directive(&block.stmts);
    let hoisted_var_slots = predefine_var_bindings_in_function_body(ctx, block);

    // Phase 1: pre-define hoisted FnDecl locals so forward references in
    // any earlier statement resolve via `lookup_local`. Generator and
    // async-generator FnDecls ARE included: `lower_body_stmt` lowers them to
    // a top-level function plus a source-position `Stmt::Let { init: FuncRef }`
    // binding the name. Spec function-declaration hoisting still applies to
    // generators, so a forward reference (`A.gen = gen` ABOVE the
    // `function* gen(){}` in a webpack/ncc inner module — next/dist/compiled/
    // edge-runtime's `consumeUint8ArrayReadableStream`) must resolve. We
    // pre-define the local here (so `lookup_local` succeeds at the forward
    // reference) and Phase 3 moves the FuncRef `Let` to the front (so it is
    // initialized before that reference runs). The FuncRef value is pure, so
    // reordering it ahead of other statements is safe.
    let mut hoisted_id_set: HashSet<LocalId> = HashSet::new();
    for stmt in &block.stmts {
        if let ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) = stmt {
            if fn_decl.function.body.is_none() {
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

    // Phase 1.5: pre-register sibling class DECLARATION names so forward
    // references inside earlier statements/method bodies resolve to
    // `ClassRef` instead of the unknown-global sentinel. JS resolves
    // these at call time (vendored zod: `ZodType.optional()` calls
    // `ZodOptional.create(...)` declared far below in the same webpack
    // module function). Scoped: the previous set is restored on exit so
    // names don't leak across function bodies.
    let saved_forward_class_names = ctx.forward_class_names.clone();
    for stmt in &block.stmts {
        if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
            ctx.forward_class_names
                .insert(class_decl.ident.sym.to_string());
        }
    }

    // Phase 2: lower the body. The inner FnDecl arm in `lower_body_stmt`
    // calls `lookup_local(name)` and reuses our pre-defined id.
    let mut body = match lower_block_stmt(ctx, block) {
        Ok(body) => body,
        Err(err) => {
            ctx.current_strict = parent_strict;
            ctx.forward_class_names = saved_forward_class_names;
            return Err(err);
        }
    };
    ctx.forward_class_names = saved_forward_class_names;

    // Re-register capture snapshots for classes declared in this body at
    // its END. The decl-site `RegisterClassCaptures` runs before later
    // statements assign captured vars (tsc emits TS-enum namespaces AFTER
    // the classes that reference them — vendored zod's
    // ZodFirstPartyTypeKind), so static-method snapshot reads and post-
    // return dynamic constructions need the FINAL values. Inserted before
    // a trailing `return` when present; bodies with early returns keep the
    // decl-site snapshot for those paths.
    {
        let mut re_regs: Vec<Stmt> = Vec::new();
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
                let cname = class_decl.ident.sym.to_string();
                if let Some(captured) = ctx.lookup_class_captures(&cname) {
                    if !captured.is_empty() {
                        let captures: Vec<Expr> =
                            captured.iter().map(|id| Expr::LocalGet(*id)).collect();
                        // Sibling code lowered BEFORE this class registered
                        // its captures (forward refs — zod's
                        // `function createZodEnum(...) { return new
                        // ZodEnum({...}) }` declared above the class) has
                        // `new <class>(…)` sites with NO cap args appended;
                        // the inline binder then misfills the ctor params.
                        // Append the raw outer ids now; sites lowered after
                        // registration already end with exactly these ids
                        // and are skipped (tail-match guard). Class members
                        // were handled by `append_self_sites` with remapped
                        // ids — their tails don't match the raw ids, but
                        // they ALREADY carry appends; restrict this pass to
                        // non-member code by walking the lowered body only
                        // (member bodies live in pending_classes, not here).
                        let cap_args: Vec<(perry_types::LocalId, perry_types::LocalId)> =
                            captured.iter().map(|id| (*id, *id)).collect();
                        for s in body.iter_mut() {
                            super::class_captures::append_new_args_stmt(s, &cname, &cap_args, true);
                        }
                        re_regs.push(Stmt::Expr(Expr::RegisterClassCaptures {
                            class_name: cname,
                            captures,
                        }));
                    }
                }
            }
        }
        if !re_regs.is_empty() {
            let insert_at = if matches!(body.last(), Some(Stmt::Return(_))) {
                body.len() - 1
            } else {
                body.len()
            };
            for (i, s) in re_regs.into_iter().enumerate() {
                body.insert(insert_at + i, s);
            }
        }
    }

    // Undefined-initialised entry slots for hoisted `var`s declared in
    // nested blocks (see predefine_var_bindings_in_function_body docs).
    let var_slot_lets: Vec<Stmt> = hoisted_var_slots
        .into_iter()
        .map(|(name, id)| Stmt::Let {
            id,
            name,
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        })
        .collect();

    if hoisted_id_set.is_empty() {
        ctx.current_strict = parent_strict;
        let mut result = var_slot_lets;
        result.extend(body);
        return Ok(result);
    }

    // Phase 3: split — pull every top-level `Stmt::Let` whose id is in the
    // hoisted set to the front (preserving relative source order).
    let mut hoisted_lets: Vec<Stmt> = Vec::new();
    let mut other: Vec<Stmt> = Vec::new();
    for s in body {
        // A regular/async FnDecl lowers to a `Let { init: Closure }`; a
        // generator/async-generator FnDecl lowers to a `Let { init: FuncRef }`
        // (the body lives in a hoisted top-level function). Both forms are
        // hoisted to the front per spec function-declaration semantics.
        let is_hoisted = matches!(
            &s,
            Stmt::Let { id, init: Some(Expr::Closure { .. }), .. }
                if hoisted_id_set.contains(id)
        ) || matches!(
            &s,
            Stmt::Let { id, init: Some(Expr::FuncRef(_)), .. }
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
    result.extend(var_slot_lets);
    result.extend(hoisted_lets);
    result.extend(other);
    ctx.current_strict = parent_strict;
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
/// getting its own try/catch/finally so the rightmost disposes first. When a
/// body throw (or an earlier dispose throw) is followed by another dispose
/// throw, the later error is wrapped in a `SuppressedError` whose `.suppressed`
/// is the accumulated completion (spec `DisposeResources`).
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
                let stmts = lower_var_decl_with_destructuring(ctx, decl, false, false)?;
                let mut decl_ids: Vec<LocalId> = Vec::new();
                for s in &stmts {
                    if let Stmt::Let { id, .. } = s {
                        binding_ids.push(*id);
                        decl_ids.push(*id);
                    }
                }
                result.extend(stmts);
                // Validate disposability at the declaration point (spec
                // `CreateDisposableResource`): a non-nullish initializer with no
                // callable `[Symbol.dispose]` / `[Symbol.asyncDispose]` throws a
                // `TypeError` here, before the block body runs. `null` /
                // `undefined` are accepted. The runtime `__perry_using_check__`
                // dispatch validates and returns; primitives throw via the
                // ordinary "not a function" method-call path.
                for &id in &decl_ids {
                    let check_call = Expr::Call {
                        callee: Box::new(Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(id)),
                            property: "__perry_using_check__".to_string(),
                        }),
                        args: vec![Expr::Bool(is_async)],
                        type_args: Vec::new(),
                    };
                    result.push(Stmt::If {
                        condition: Expr::Logical {
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
                        },
                        then_branch: vec![Stmt::Expr(check_call)],
                        else_branch: None,
                    });
                }
            }
            // Recursively lower remaining stmts as the try body.
            let body_stmts = lower_stmts_using_aware(ctx, &stmts[i + 1..])?;
            // Wrap each binding in its own try/catch/finally — innermost
            // (rightmost binding) disposes first, giving reverse-declaration
            // order. Each level captures a thrown body completion into a pair
            // of locals (`__pending` / `__has`) so the finally can aggregate a
            // dispose-throw into a `SuppressedError` (spec `DisposeResources`):
            //
            //   let __pending; let __has = false;
            //   try { <inner> }
            //   catch (__c) { __pending = __c; __has = true; }
            //   finally {
            //     try { if (x != null) [await] x.<dispose>(); }
            //     catch (__d) {
            //        if (__has) __pending = new SuppressedError(__d, __pending);
            //        else { __pending = __d; __has = true; }
            //     }
            //     if (__has) throw __pending;
            //   }
            //
            // `try`/`finally` (not bare `catch`) is required so the disposal
            // runs on every abrupt completion of `<inner>` — `return` /
            // `break` / `continue` as well as `throw`. Nesting composes the
            // chaining: a body error becomes the innermost `suppressed`, and
            // each outer dispose-throw wraps the accumulated value, so the
            // last (outermost, first-declared) dispose throw is `.error`.
            let mut wrapped = body_stmts;
            for (level, &id) in binding_ids.iter().rev().enumerate() {
                let method_name = if is_async {
                    "__perry_async_dispose__"
                } else {
                    "__perry_dispose__"
                };
                let pending = ctx.fresh_local();
                let has = ctx.fresh_local();
                let body_err = ctx.fresh_local();
                let dispose_err = ctx.fresh_local();

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

                // catch (__d) { if (__has) __pending = new SuppressedError(__d,
                // __pending); else { __pending = __d; __has = true; } }
                let dispose_catch = CatchClause {
                    param: Some((dispose_err, format!("__perry_dispose_err_{level}"))),
                    body: vec![Stmt::If {
                        condition: Expr::LocalGet(has),
                        then_branch: vec![Stmt::Expr(Expr::LocalSet(
                            pending,
                            Box::new(Expr::New {
                                class_name: "SuppressedError".to_string(),
                                args: vec![
                                    Expr::LocalGet(dispose_err),
                                    Expr::LocalGet(pending),
                                    Expr::String(
                                        "An error was suppressed during disposal".to_string(),
                                    ),
                                ],
                                type_args: Vec::new(),
                            }),
                        ))],
                        else_branch: Some(vec![
                            Stmt::Expr(Expr::LocalSet(
                                pending,
                                Box::new(Expr::LocalGet(dispose_err)),
                            )),
                            Stmt::Expr(Expr::LocalSet(has, Box::new(Expr::Bool(true)))),
                        ]),
                    }],
                };

                let finally_stmts = vec![
                    Stmt::Try {
                        body: vec![Stmt::If {
                            condition: null_check,
                            then_branch: vec![Stmt::Expr(call_expr)],
                            else_branch: None,
                        }],
                        catch: Some(dispose_catch),
                        finally: None,
                    },
                    Stmt::If {
                        condition: Expr::LocalGet(has),
                        then_branch: vec![Stmt::Throw(Expr::LocalGet(pending))],
                        else_branch: None,
                    },
                ];

                let body_catch = CatchClause {
                    param: Some((body_err, format!("__perry_body_err_{level}"))),
                    body: vec![
                        Stmt::Expr(Expr::LocalSet(pending, Box::new(Expr::LocalGet(body_err)))),
                        Stmt::Expr(Expr::LocalSet(has, Box::new(Expr::Bool(true)))),
                    ],
                };

                wrapped = vec![
                    Stmt::Let {
                        id: pending,
                        name: format!("__perry_pending_{level}"),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(Expr::Undefined),
                    },
                    Stmt::Let {
                        id: has,
                        name: format!("__perry_has_err_{level}"),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(Expr::Bool(false)),
                    },
                    Stmt::Try {
                        body: wrapped,
                        catch: Some(body_catch),
                        finally: Some(finally_stmts),
                    },
                ];
            }
            result.extend(wrapped);
            return Ok(result);
        }
        result.extend(lower_body_stmt(ctx, stmt)?);
    }
    Ok(result)
}
