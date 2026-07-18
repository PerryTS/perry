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

use super::*;

pub fn lower_block_stmt(ctx: &mut LoweringContext, block: &ast::BlockStmt) -> Result<Vec<Stmt>> {
    rebind_nested_forward_scope_lets(ctx, &block.stmts);
    lower_stmts_using_aware(ctx, &block.stmts)
}

/// Make the forward-captured `let`/`const` bindings that
/// [`pre_register_forward_captured_lets`] pre-registered for THIS nested block
/// scope name-visible for the duration of the block's lowering. Nested-scope
/// pre-registrations allocate a hidden id (no `ctx.locals` entry — see
/// `nested_forward_scope_ids`), so an earlier closure in the block resolves
/// the name only if we push the binding here, at scope entry. The entry is
/// dropped again when the enclosing `pop_block_scope` / `exit_scope` mark
/// unwinds, so the binding is visible exactly within its block — a same-named
/// `let` in a sibling block gets its own id/box, and references after the
/// block resolve to the outer binding (or stay global) as in Node.
///
/// Called from [`lower_block_stmt`] (every `{}`-shaped scope: block, `try` /
/// `catch` / `finally`, block-bodied `if` / loop / labeled bodies) and from
/// the two switch-case lowering arms (`lower/stmt.rs`, `lower_decl/
/// body_stmt.rs`), whose case statement-lists share the switch's block scope
/// without being a `BlockStmt`.
pub(crate) fn rebind_nested_forward_scope_lets(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) {
    if ctx.lexical_forward_decls.is_empty() {
        return;
    }
    for stmt in stmts {
        let ast::Stmt::Decl(ast::Decl::Var(var_decl)) = stmt else {
            continue;
        };
        if !matches!(
            var_decl.kind,
            ast::VarDeclKind::Let | ast::VarDeclKind::Const
        ) {
            continue;
        }
        for decl in &var_decl.decls {
            let mut binding_idents: Vec<(String, u32)> = Vec::new();
            collect_pat_forward_idents(&decl.name, &mut binding_idents);
            for (name, span_lo) in binding_idents {
                if let Some(&id) = ctx.lexical_forward_decls.get(&span_lo) {
                    if ctx.nested_forward_scope_ids.contains(&id) {
                        ctx.locals.push((name, id, Type::Any));
                    }
                }
            }
        }
    }
}

/// Collect identifier names referenced INSIDE any closure (arrow / function
/// expression / nested function declaration / class member) within a statement
/// (`in_cl` = whether we are already inside a closure body).
///
/// Used by the Phase 1.6 forward `let`/`const` pre-registration in
/// `lower_fn_body_block_stmt` to box ONLY bindings a closure can actually
/// capture, rather than every top-level binding in a closure-containing body
/// (the latter regressed Next.js at scale — start-server's `initialize()`
/// exited after "Ready"). Over-collection is harmless (a collected name that
/// isn't a top-level `let`/`const` is simply ignored); under-collection on an
/// exotic AST node degrades to the pre-fix behavior for that one binding.
/// Collect every binding identifier (name + span low offset) introduced by a
/// declarator pattern, recursing through array / object destructuring. The span
/// keys `lexical_forward_decls` so the destructuring binding site reuses the
/// forward-pre-registered (boxed) local. Mirrors the binding sites in
/// `destructuring/pattern_binding.rs` (`Pat::Ident` leaf and the `{ key }`
/// shorthand `ObjectPatProp::Assign`).
/// Pre-register the top-level `let`/`const` bindings of a function body that
/// are FORWARD-captured — referenced by a closure (arrow / function expression
/// / nested function declaration) appearing EARLIER in the body than the
/// binding's declaration. Each such binding is defined as a boxed function-
/// scope local now (so the earlier closure resolves it to the local and
/// captures the live box) and span-keyed in `lexical_forward_decls` so the
/// declaration — including a destructuring leaf — reuses the same id. Returns
/// the pre-registered ids so the caller can prealloc their boxes at entry.
///
/// `body_entry_locals_len` is `ctx.locals.len()` captured before any of this
/// body's own locals were defined — anything at or above it is in THIS scope,
/// so a binding that shadows an outer name still gets a fresh local. Shared by
/// `lower_fn_body_block_stmt` (function declarations + arrows) and
/// `lower_fn_expr` (the cjs `const _cjs = (function(){…})()` wrapper, where the
/// `_export(exports, { X: () => X })` getter forward-captures a later `const {
/// X } = …`).
pub(crate) fn pre_register_forward_captured_lets(
    ctx: &mut LoweringContext,
    block: &ast::BlockStmt,
    body_entry_locals_len: usize,
) -> Vec<LocalId> {
    let mut forward_boxed_ids: Vec<LocalId> = Vec::new();
    // Worklist of block statement-lists, each processed as its OWN
    // forward-capture scope. Nested block scopes (try / catch / finally / `{}`
    // / loop bodies / switch cases — including those behind non-block bodies
    // like `else if` chains, see `push_nested_block_stmt_lists`) are appended
    // so a closure created EARLIER in a nested block that references a
    // `let`/`const` declared LATER in that SAME nested block is pre-registered
    // too. Previously only the function-body TOP level was scanned, so a
    // `try { let cb = () => x; let x = …; cb() }` (esbuild `__esm` streaming
    // closures in the compiled query async-generator) fell through to
    // `js_global_get_or_throw_unresolved` → `ReferenceError: x is not
    // defined`. Forward-captured boxes from any depth still preallocate at
    // function entry (Phase 4/5) and each declaration reuses its id by span
    // (`lexical_forward_decls`).
    //
    // The bool is `is_nested`: only the function-body top level (front entry)
    // defines its pre-registrations as name-visible function-scope locals.
    // Nested scopes allocate a HIDDEN id (`fresh_local`, tracked in
    // `nested_forward_scope_ids`) that `rebind_nested_forward_scope_lets`
    // makes name-visible exactly while its own block is being lowered.
    // Defining nested ids at function level (the initial implementation)
    // leaked the name across the whole body: a same-named forward-captured
    // `let` in a later sibling block was deduped against the earlier block's
    // registration (both closures then shared one box — `1,1` instead of
    // Node's `1,2`), and a reference AFTER the block resolved to the block's
    // box (or its TDZ sentinel) instead of the outer/module binding.
    // Function-WIDE closure-referenced names, for the `var` branch below.
    // `var` bindings hoist to function scope, so a closure ANYWHERE in the
    // body (before or after, same scope or not) that references the name must
    // capture the live box — the per-scope ordered `seen_closure_refs` misses
    // e.g. `let cb = () => n; { var n = 5; } cb()` (closure at top level, var
    // in a nested block: the block's own seen-set never contains `n`).
    let mut fn_wide_closure_refs: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for stmt in &block.stmts {
        cic_stmt(stmt, false, &mut fn_wide_closure_refs);
    }
    let mut fwd_worklist: std::collections::VecDeque<(&[ast::Stmt], bool)> =
        std::collections::VecDeque::new();
    fwd_worklist.push_back((&block.stmts[..], false));
    while let Some((scope_stmts, is_nested)) = fwd_worklist.pop_front() {
        for stmt in scope_stmts {
            push_nested_block_stmt_lists(stmt, &mut fwd_worklist);
        }
        let mut seen_closure_refs: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // Names already pre-registered in THIS scope (a `let`/`const` name can
        // legally repeat only across scopes, but destructuring keeps this a
        // set rather than trusting the parser).
        let mut registered_here: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for stmt in scope_stmts {
            if let ast::Stmt::Decl(ast::Decl::Var(var_decl)) = stmt {
                if matches!(
                    var_decl.kind,
                    ast::VarDeclKind::Let | ast::VarDeclKind::Const
                ) {
                    for decl in &var_decl.decls {
                        let mut binding_idents: Vec<(String, u32)> = Vec::new();
                        collect_pat_forward_idents(&decl.name, &mut binding_idents);
                        for (name, span_lo) in binding_idents {
                            if !seen_closure_refs.contains(&name) || registered_here.contains(&name)
                            {
                                continue;
                            }
                            if is_nested {
                                // Hidden id: name-visibility is scoped to the
                                // block via `rebind_nested_forward_scope_lets`.
                                let id = ctx.fresh_local();
                                ctx.var_hoisted_ids.insert(id);
                                ctx.tdz_forward_ids.insert(id);
                                ctx.nested_forward_scope_ids.insert(id);
                                forward_boxed_ids.push(id);
                                ctx.lexical_forward_decls.insert(span_lo, id);
                                registered_here.insert(name);
                            } else {
                                let already_in_scope = ctx
                                    .locals
                                    .lookup_index_in_scope(&name, body_entry_locals_len)
                                    .is_some();
                                if !already_in_scope {
                                    let id = ctx.define_local(name.clone(), Type::Any);
                                    ctx.var_hoisted_ids.insert(id);
                                    // Lexical let/const forward-decl: mark TDZ-eligible
                                    // so its box is seeded with the TAG_TDZ sentinel and
                                    // a read before the declaration throws.
                                    ctx.tdz_forward_ids.insert(id);
                                    forward_boxed_ids.push(id);
                                    ctx.lexical_forward_decls.insert(span_lo, id);
                                    registered_here.insert(name);
                                }
                            }
                        }
                        // A closure in an EARLIER declarator of THIS same
                        // `let`/`const` can forward-reference a name bound by a LATER
                        // declarator in the SAME declaration:
                        //   `let z = (w) => { … O … Y … A … },
                        //        Y = () => z(false),
                        //        A = () => clearTimeout(O),
                        //        O = setTimeout(z, K);`
                        // (the minified `new Promise` executor shape). Record this
                        // declarator's closure refs NOW so the later declarators are
                        // seen as forward-captured too — `seen_closure_refs` is
                        // otherwise only updated by the trailing `cic_stmt` AFTER the
                        // whole declaration, so intra-declaration forward-refs were
                        // missed: the later names never got pre-registered, so the
                        // ref fell through to `js_global_get_or_throw_unresolved` and
                        // the closure captured a global instead of the local box —
                        // e.g. a `new Promise` `resolve` that never fires, hanging
                        // the awaiting caller. Cross-statement forward-refs were
                        // already handled by the trailing `cic_stmt`.
                        if let Some(init) = &decl.init {
                            cic_expr(init, false, &mut seen_closure_refs);
                        }
                    }
                } else {
                    // `var` bindings are already predefined + boxed by
                    // `predefine_var_bindings_in_function_body`, but their box is
                    // NOT in the prealloc set. A closure created EARLIER in the body
                    // that references a `var` declared LATER (`r.d(t,{x:()=>n.x});
                    // var n=r("…")` — the webpack ESM re-export shape in Next.js'
                    // react-server.node.js) must capture the *live* box, not a
                    // TAG_UNDEFINED snapshot. Add forward-captured `var` ids to the
                    // prealloc set so codegen allocates the box at function entry.
                    // Checked against the function-WIDE closure-ref set: `var`s
                    // hoist, so the capturing closure can live in a DIFFERENT
                    // (e.g. enclosing) scope than the `var` statement itself.
                    for decl in &var_decl.decls {
                        let mut binding_idents: Vec<(String, u32)> = Vec::new();
                        collect_pat_forward_idents(&decl.name, &mut binding_idents);
                        for (name, _span_lo) in binding_idents {
                            if !fn_wide_closure_refs.contains(&name) {
                                continue;
                            }
                            if let Some(id) = ctx.lookup_local(&name) {
                                if !forward_boxed_ids.contains(&id) {
                                    ctx.var_hoisted_ids.insert(id);
                                    forward_boxed_ids.push(id);
                                }
                            }
                        }
                    }
                }
            }
            // Record closures introduced by THIS statement for subsequent decls.
            cic_stmt(stmt, false, &mut seen_closure_refs);
        }
    }
    forward_boxed_ids
}

/// Append the statement lists of the block scopes nested in `stmt` (try /
/// catch / finally / plain `{}` / loop bodies / labeled body / switch cases)
/// to `worklist`, always as nested (`true`) scopes. Non-block single-statement
/// bodies are RECURSED into rather than skipped, so a block behind any chain
/// of wrappers is still found — `else if { … }`, `for (…) if (c) { … }`,
/// `label: while (x) try { … }` and the like. (A lexical declaration cannot
/// itself be a non-block body — `if (c) let x = 1` is a syntax error — so
/// recursing to find blocks is sufficient.) Deeper same-shape nesting is
/// reached transitively as each queued scope is itself scanned. Used by
/// [`pre_register_forward_captured_lets`] so forward-captured `let`/`const`
/// bindings inside nested blocks are pre-registered, not just those at the
/// function-body top level.
fn push_nested_block_stmt_lists<'a>(
    stmt: &'a ast::Stmt,
    worklist: &mut std::collections::VecDeque<(&'a [ast::Stmt], bool)>,
) {
    use ast::Stmt::*;
    match stmt {
        Block(b) => worklist.push_back((&b.stmts[..], true)),
        Try(t) => {
            worklist.push_back((&t.block.stmts[..], true));
            if let Some(h) = &t.handler {
                worklist.push_back((&h.body.stmts[..], true));
            }
            if let Some(f) = &t.finalizer {
                worklist.push_back((&f.stmts[..], true));
            }
        }
        If(i) => {
            push_nested_block_stmt_lists(&i.cons, worklist);
            if let Some(alt) = &i.alt {
                push_nested_block_stmt_lists(alt, worklist);
            }
        }
        For(f) => push_nested_block_stmt_lists(&f.body, worklist),
        ForIn(f) => push_nested_block_stmt_lists(&f.body, worklist),
        ForOf(f) => push_nested_block_stmt_lists(&f.body, worklist),
        While(w) => push_nested_block_stmt_lists(&w.body, worklist),
        DoWhile(w) => push_nested_block_stmt_lists(&w.body, worklist),
        Labeled(l) => push_nested_block_stmt_lists(&l.body, worklist),
        // Sloppy-mode `with (o) { … }` — a block-scoped `let` in the body is
        // nearer than the with-object's properties, so it forward-captures
        // like any other nested block (mirrors the `With` arm in `cic_stmt`).
        With(w) => push_nested_block_stmt_lists(&w.body, worklist),
        Switch(sw) => {
            for c in &sw.cases {
                worklist.push_back((&c.cons[..], true));
            }
        }
        _ => {}
    }
}

fn collect_pat_forward_idents(pat: &ast::Pat, out: &mut Vec<(String, u32)>) {
    match pat {
        ast::Pat::Ident(i) => out.push((i.id.sym.to_string(), i.id.span.lo.0)),
        ast::Pat::Array(arr) => arr
            .elems
            .iter()
            .flatten()
            .for_each(|el| collect_pat_forward_idents(el, out)),
        ast::Pat::Object(o) => {
            for p in &o.props {
                match p {
                    ast::ObjectPatProp::KeyValue(kv) => collect_pat_forward_idents(&kv.value, out),
                    ast::ObjectPatProp::Assign(a) => {
                        out.push((a.key.sym.to_string(), a.key.span.lo.0))
                    }
                    ast::ObjectPatProp::Rest(r) => collect_pat_forward_idents(&r.arg, out),
                }
            }
        }
        ast::Pat::Assign(a) => collect_pat_forward_idents(&a.left, out),
        ast::Pat::Rest(r) => collect_pat_forward_idents(&r.arg, out),
        _ => {}
    }
}

fn cic_stmt(s: &ast::Stmt, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    use ast::Stmt::*;
    match s {
        Block(b) => b.stmts.iter().for_each(|st| cic_stmt(st, in_cl, out)),
        Return(r) => {
            if let Some(a) = &r.arg {
                cic_expr(a, in_cl, out);
            }
        }
        Expr(e) => cic_expr(&e.expr, in_cl, out),
        If(i) => {
            cic_expr(&i.test, in_cl, out);
            cic_stmt(&i.cons, in_cl, out);
            if let Some(a) = &i.alt {
                cic_stmt(a, in_cl, out);
            }
        }
        Throw(t) => cic_expr(&t.arg, in_cl, out),
        While(w) => {
            cic_expr(&w.test, in_cl, out);
            cic_stmt(&w.body, in_cl, out);
        }
        DoWhile(w) => {
            cic_expr(&w.test, in_cl, out);
            cic_stmt(&w.body, in_cl, out);
        }
        For(f) => {
            if let Some(init) = &f.init {
                match init {
                    ast::VarDeclOrExpr::Expr(e) => cic_expr(e, in_cl, out),
                    ast::VarDeclOrExpr::VarDecl(vd) => vd.decls.iter().for_each(|d| {
                        if let Some(i) = &d.init {
                            cic_expr(i, in_cl, out);
                        }
                    }),
                }
            }
            if let Some(t) = &f.test {
                cic_expr(t, in_cl, out);
            }
            if let Some(u) = &f.update {
                cic_expr(u, in_cl, out);
            }
            cic_stmt(&f.body, in_cl, out);
        }
        ForIn(f) => {
            cic_expr(&f.right, in_cl, out);
            cic_stmt(&f.body, in_cl, out);
        }
        ForOf(f) => {
            cic_expr(&f.right, in_cl, out);
            cic_stmt(&f.body, in_cl, out);
        }
        Try(t) => {
            t.block.stmts.iter().for_each(|st| cic_stmt(st, in_cl, out));
            if let Some(h) = &t.handler {
                h.body.stmts.iter().for_each(|st| cic_stmt(st, in_cl, out));
            }
            if let Some(f) = &t.finalizer {
                f.stmts.iter().for_each(|st| cic_stmt(st, in_cl, out));
            }
        }
        Switch(sw) => {
            cic_expr(&sw.discriminant, in_cl, out);
            for c in &sw.cases {
                if let Some(t) = &c.test {
                    cic_expr(t, in_cl, out);
                }
                c.cons.iter().for_each(|st| cic_stmt(st, in_cl, out));
            }
        }
        Labeled(l) => cic_stmt(&l.body, in_cl, out),
        With(w) => {
            cic_expr(&w.obj, in_cl, out);
            cic_stmt(&w.body, in_cl, out);
        }
        Decl(d) => cic_decl(d, in_cl, out),
        _ => {}
    }
}

fn cic_decl(d: &ast::Decl, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    match d {
        ast::Decl::Var(vd) => vd.decls.iter().for_each(|de| {
            if let Some(i) = &de.init {
                cic_expr(i, in_cl, out);
            }
        }),
        // A nested function declaration's body is a closure scope.
        ast::Decl::Fn(f) => cic_function(&f.function, out),
        ast::Decl::Class(c) => cic_class(&c.class, in_cl, out),
        _ => {}
    }
}

/// Param patterns (defaults evaluate at CALL time) + body of a closure-scoped
/// `ast::Function` — nested fn declarations/expressions and class methods all
/// share this traversal.
fn cic_function(f: &ast::Function, out: &mut std::collections::HashSet<String>) {
    for p in &f.params {
        cic_pat(&p.pat, true, out);
    }
    if let Some(b) = &f.body {
        b.stmts.iter().for_each(|st| cic_stmt(st, true, out));
    }
}

fn cic_class(c: &ast::Class, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    if let Some(sc) = &c.super_class {
        cic_expr(sc, in_cl, out);
    }
    for m in &c.body {
        match m {
            ast::ClassMember::Method(mm) => cic_function(&mm.function, out),
            ast::ClassMember::PrivateMethod(mm) => cic_function(&mm.function, out),
            // #6523: the CONSTRUCTOR body runs at `new` time, not at class
            // definition — a binding it references that is declared AFTER the
            // class (`class C { constructor(){ a() } } const a = ...`) must be
            // pre-registered as a forward-captured lexical exactly like a
            // method-body reference. This arm was missing, so such refs never
            // got a box: `collect_method_captures` dropped them (not in
            // `ctx.locals` at the class decl) and the ref fell through to the
            // global lookup — "a is not defined" at construction (bundled
            // semver's Comparator debug/constant pattern).
            ast::ClassMember::Constructor(ctor) => {
                for p in &ctor.params {
                    match p {
                        ast::ParamOrTsParamProp::Param(p) => cic_pat(&p.pat, true, out),
                        ast::ParamOrTsParamProp::TsParamProp(tp) => {
                            if let ast::TsParamPropParam::Assign(a) = &tp.param {
                                cic_expr(&a.right, true, out);
                            }
                        }
                    }
                }
                if let Some(b) = &ctor.body {
                    b.stmts.iter().for_each(|st| cic_stmt(st, true, out));
                }
            }
            ast::ClassMember::ClassProp(p) => {
                if let Some(v) = &p.value {
                    cic_expr(v, true, out);
                }
            }
            ast::ClassMember::PrivateProp(p) => {
                if let Some(v) = &p.value {
                    cic_expr(v, true, out);
                }
            }
            ast::ClassMember::StaticBlock(sb) => {
                sb.body.stmts.iter().for_each(|st| cic_stmt(st, true, out));
            }
            _ => {}
        }
    }
}

fn cic_expr(e: &ast::Expr, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    use ast::Expr::*;
    match e {
        Ident(i) => {
            if in_cl {
                out.insert(i.sym.to_string());
            }
        }
        Arrow(a) => {
            for p in &a.params {
                cic_pat(p, true, out);
            }
            match &*a.body {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    b.stmts.iter().for_each(|st| cic_stmt(st, true, out))
                }
                ast::BlockStmtOrExpr::Expr(ex) => cic_expr(ex, true, out),
            }
        }
        Fn(f) => cic_function(&f.function, out),
        Class(c) => cic_class(&c.class, in_cl, out),
        Array(a) => a
            .elems
            .iter()
            .flatten()
            .for_each(|el| cic_expr(&el.expr, in_cl, out)),
        Object(o) => {
            for p in &o.props {
                match p {
                    ast::PropOrSpread::Spread(s) => cic_expr(&s.expr, in_cl, out),
                    ast::PropOrSpread::Prop(pr) => cic_prop(pr, in_cl, out),
                }
            }
        }
        Unary(u) => cic_expr(&u.arg, in_cl, out),
        Update(u) => cic_expr(&u.arg, in_cl, out),
        Bin(b) => {
            cic_expr(&b.left, in_cl, out);
            cic_expr(&b.right, in_cl, out);
        }
        Assign(a) => {
            cic_assign_target(&a.left, in_cl, out);
            cic_expr(&a.right, in_cl, out);
        }
        Member(m) => {
            cic_expr(&m.obj, in_cl, out);
            if let ast::MemberProp::Computed(c) = &m.prop {
                cic_expr(&c.expr, in_cl, out);
            }
        }
        Cond(c) => {
            cic_expr(&c.test, in_cl, out);
            cic_expr(&c.cons, in_cl, out);
            cic_expr(&c.alt, in_cl, out);
        }
        Call(c) => {
            if let ast::Callee::Expr(e) = &c.callee {
                cic_expr(e, in_cl, out);
            }
            c.args.iter().for_each(|a| cic_expr(&a.expr, in_cl, out));
        }
        New(n) => {
            cic_expr(&n.callee, in_cl, out);
            if let Some(args) = &n.args {
                args.iter().for_each(|a| cic_expr(&a.expr, in_cl, out));
            }
        }
        Seq(s) => s.exprs.iter().for_each(|e| cic_expr(e, in_cl, out)),
        Tpl(t) => t.exprs.iter().for_each(|e| cic_expr(e, in_cl, out)),
        TaggedTpl(t) => {
            cic_expr(&t.tag, in_cl, out);
            t.tpl.exprs.iter().for_each(|e| cic_expr(e, in_cl, out));
        }
        Paren(p) => cic_expr(&p.expr, in_cl, out),
        Await(a) => cic_expr(&a.arg, in_cl, out),
        Yield(y) => {
            if let Some(a) = &y.arg {
                cic_expr(a, in_cl, out);
            }
        }
        OptChain(o) => match &*o.base {
            ast::OptChainBase::Member(m) => {
                cic_expr(&m.obj, in_cl, out);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    cic_expr(&c.expr, in_cl, out);
                }
            }
            ast::OptChainBase::Call(c) => {
                cic_expr(&c.callee, in_cl, out);
                c.args.iter().for_each(|a| cic_expr(&a.expr, in_cl, out));
            }
        },
        _ => {}
    }
}

fn cic_pat(p: &ast::Pat, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    match p {
        ast::Pat::Assign(a) => {
            cic_pat(&a.left, in_cl, out);
            cic_expr(&a.right, in_cl, out);
        }
        ast::Pat::Array(arr) => arr
            .elems
            .iter()
            .flatten()
            .for_each(|el| cic_pat(el, in_cl, out)),
        ast::Pat::Object(o) => {
            for pp in &o.props {
                match pp {
                    ast::ObjectPatProp::KeyValue(kv) => cic_pat(&kv.value, in_cl, out),
                    ast::ObjectPatProp::Assign(a) => {
                        if let Some(v) = &a.value {
                            cic_expr(v, in_cl, out);
                        }
                    }
                    ast::ObjectPatProp::Rest(r) => cic_pat(&r.arg, in_cl, out),
                }
            }
        }
        ast::Pat::Rest(r) => cic_pat(&r.arg, in_cl, out),
        _ => {}
    }
}

fn cic_prop(p: &ast::Prop, in_cl: bool, out: &mut std::collections::HashSet<String>) {
    match p {
        ast::Prop::Shorthand(i) => {
            if in_cl {
                out.insert(i.sym.to_string());
            }
        }
        ast::Prop::KeyValue(kv) => {
            if let ast::PropName::Computed(c) = &kv.key {
                cic_expr(&c.expr, in_cl, out);
            }
            cic_expr(&kv.value, in_cl, out);
        }
        ast::Prop::Getter(g) => {
            if let Some(b) = &g.body {
                b.stmts.iter().for_each(|st| cic_stmt(st, true, out));
            }
        }
        ast::Prop::Setter(s) => {
            if let Some(b) = &s.body {
                b.stmts.iter().for_each(|st| cic_stmt(st, true, out));
            }
        }
        ast::Prop::Method(m) => {
            if let Some(b) = &m.function.body {
                b.stmts.iter().for_each(|st| cic_stmt(st, true, out));
            }
        }
        ast::Prop::Assign(a) => cic_expr(&a.value, in_cl, out),
    }
}

fn cic_assign_target(
    t: &ast::AssignTarget,
    in_cl: bool,
    out: &mut std::collections::HashSet<String>,
) {
    if let ast::AssignTarget::Simple(s) = t {
        match s {
            ast::SimpleAssignTarget::Ident(i) => {
                if in_cl {
                    out.insert(i.id.sym.to_string());
                }
            }
            ast::SimpleAssignTarget::Member(m) => {
                cic_expr(&m.obj, in_cl, out);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    cic_expr(&c.expr, in_cl, out);
                }
            }
            ast::SimpleAssignTarget::Paren(p) => cic_expr(&p.expr, in_cl, out),
            _ => {}
        }
    }
}

pub(crate) fn collect_var_binding_names_from_pat(pat: &ast::Pat, out: &mut Vec<String>) {
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

/// Collect the lexically-declared names (`let` / `const` / `class`) at the top
/// level of a statement list. A `var` or a `function` declaration is NOT
/// lexical and does not belong here. Used to build the Annex B "forbidden" set:
/// a block-level function declaration whose name collides with a lexical
/// binding in an enclosing scope would make the equivalent `var` an early
/// error, so B.3.3 skips creating the enclosing-scope `var`.
pub(crate) fn collect_lexical_decl_names(
    stmts: &[ast::Stmt],
    out: &mut std::collections::HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) if var_decl.kind != ast::VarDeclKind::Var => {
                for decl in &var_decl.decls {
                    let mut names = Vec::new();
                    collect_var_binding_names_from_pat(&decl.name, &mut names);
                    out.extend(names);
                }
            }
            ast::Stmt::Decl(ast::Decl::Class(class_decl)) => {
                out.insert(class_decl.ident.sym.to_string());
            }
            _ => {}
        }
    }
}

/// Annex B B.3.3 (#5297): collect the names of function declarations that
/// appear *inside a nested block* of a function/program body. In sloppy mode
/// such a legacy block-level function declaration ALSO creates a `var`-style
/// binding in the enclosing function/global scope (`f` is visible — as a `var`
/// initialised to `undefined` until the declaration runs — outside the block).
///
/// `body_stmts` are the body's own top-level statements: a `function f(){}`
/// directly among them is an ordinary FunctionDeclaration (already function-
/// scoped) and is NOT collected; every function declaration reached by
/// descending through a block / `if` branch / loop body / `switch` case /
/// `try` part / labeled / `with` body IS. `forbidden` seeds the names for which
/// the legacy `var` must be skipped — the spec gates B.3.3 on "replacing the
/// FunctionDeclaration with a `var` produces no early error and the name is not
/// a parameter": callers pass the parameter names, the body's own top-level
/// lexical names, and `"arguments"`. As we descend, each block contributes its
/// own `let`/`const`/`class` names to the forbidden set for everything nested
/// within it (so `{ let f; { function f(){} } }` is correctly skipped). Nested
/// function and class bodies own their own var environment and are not entered.
/// One traversal yields two results:
/// - `all_out`: EVERY block-nested function declaration name. Every block-level
///   function declaration is block-scoped (gets its own binding), so
///   `lower_nested_fn_decl` gives these a fresh local rather than clobbering an
///   enclosing same-named parameter/binding.
/// - `var_out`: the subset that ALSO gets the legacy enclosing-scope `var` —
///   names not in `forbidden` and not shadowed by an enclosing block's
///   `let`/`const`/`class` (which would make `var f` an early error).
pub(crate) fn collect_annexb_block_fn_decl_names(
    body_stmts: &[ast::Stmt],
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    for stmt in body_stmts {
        // A direct top-level function declaration is already function-scoped.
        if matches!(stmt, ast::Stmt::Decl(ast::Decl::Fn(_))) {
            continue;
        }
        annexb_nested_stmt(stmt, forbidden, all_out, var_out);
    }
}

fn annexb_nested_stmt(
    stmt: &ast::Stmt,
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    match stmt {
        ast::Stmt::Decl(ast::Decl::Fn(fn_decl)) => {
            let name = fn_decl.ident.sym.to_string();
            all_out.push(name.clone());
            if !forbidden.contains(&name) {
                var_out.push(name);
            }
        }
        // Nested function/class bodies have their own var environment.
        ast::Stmt::Decl(ast::Decl::Class(_)) => {}
        ast::Stmt::Block(block) => annexb_nested_block(&block.stmts, forbidden, all_out, var_out),
        ast::Stmt::If(if_stmt) => {
            annexb_nested_stmt(&if_stmt.cons, forbidden, all_out, var_out);
            if let Some(alt) = &if_stmt.alt {
                annexb_nested_stmt(alt, forbidden, all_out, var_out);
            }
        }
        ast::Stmt::While(while_stmt) => {
            annexb_nested_stmt(&while_stmt.body, forbidden, all_out, var_out)
        }
        ast::Stmt::DoWhile(do_while) => {
            annexb_nested_stmt(&do_while.body, forbidden, all_out, var_out)
        }
        // A `for`/`for-in`/`for-of` lexical head (`for (let f; ...)`,
        // `for (let f in/of ...)`) introduces a binding whose scope encloses
        // the loop body; an equivalent `var f` in the body is an early error
        // (14.7.4.1 / 14.7.5.1), so the AnnexB legacy `var` for a same-named
        // block function in the body must be skipped.
        ast::Stmt::For(for_stmt) => {
            let names = match &for_stmt.init {
                Some(ast::VarDeclOrExpr::VarDecl(vd)) => var_decl_lexical_names(vd),
                _ => Vec::new(),
            };
            annexb_nested_loop_body(&for_stmt.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::ForIn(for_in) => {
            let names = for_head_lexical_names(&for_in.left);
            annexb_nested_loop_body(&for_in.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::ForOf(for_of) => {
            let names = for_head_lexical_names(&for_of.left);
            annexb_nested_loop_body(&for_of.body, names, forbidden, all_out, var_out);
        }
        ast::Stmt::Labeled(labeled) => {
            annexb_nested_stmt(&labeled.body, forbidden, all_out, var_out)
        }
        ast::Stmt::Switch(switch_stmt) => {
            // All cases of a switch share one block scope, so their lexical
            // names contribute together to the forbidden set.
            let mut inner = forbidden.clone();
            for case in &switch_stmt.cases {
                collect_lexical_decl_names(&case.cons, &mut inner);
            }
            for case in &switch_stmt.cases {
                for stmt in &case.cons {
                    annexb_nested_stmt(stmt, &inner, all_out, var_out);
                }
            }
        }
        ast::Stmt::Try(try_stmt) => {
            annexb_nested_block(&try_stmt.block.stmts, forbidden, all_out, var_out);
            if let Some(handler) = &try_stmt.handler {
                // B.3.5: a `var` whose name is also a bound name of a
                // *destructuring* CatchParameter is an early error, so the
                // equivalent AnnexB legacy `var` for a same-named block
                // function in the handler body must be skipped. The B.3.5
                // exception only exempts a simple `catch (e)` BindingIdentifier
                // (where the var IS allowed), so only pattern catch params
                // (`catch ({ f })` / `catch ([f])`) contribute to `forbidden`.
                let mut handler_forbidden;
                let inner = match &handler.param {
                    Some(param) if !matches!(param, ast::Pat::Ident(_)) => {
                        handler_forbidden = forbidden.clone();
                        let mut names = Vec::new();
                        collect_var_binding_names_from_pat(param, &mut names);
                        handler_forbidden.extend(names);
                        &handler_forbidden
                    }
                    _ => forbidden,
                };
                annexb_nested_block(&handler.body.stmts, inner, all_out, var_out);
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                annexb_nested_block(&finalizer.stmts, forbidden, all_out, var_out);
            }
        }
        ast::Stmt::With(with_stmt) => {
            annexb_nested_stmt(&with_stmt.body, forbidden, all_out, var_out)
        }
        _ => {}
    }
}

/// Lexical (`let`/`const`) binding names introduced by a `VarDecl`. A `var`
/// declaration introduces no lexical names and yields an empty list.
fn var_decl_lexical_names(vd: &ast::VarDecl) -> Vec<String> {
    if vd.kind == ast::VarDeclKind::Var {
        return Vec::new();
    }
    let mut names = Vec::new();
    for decl in &vd.decls {
        collect_var_binding_names_from_pat(&decl.name, &mut names);
    }
    names
}

/// Lexical binding names of a `for-in` / `for-of` head (`for (let f in …)`).
/// A `var` head or a bare assignment-target pattern introduces no lexical
/// binding here and yields an empty list.
fn for_head_lexical_names(head: &ast::ForHead) -> Vec<String> {
    match head {
        ast::ForHead::VarDecl(vd) => var_decl_lexical_names(vd),
        _ => Vec::new(),
    }
}

/// Descend into a loop body, adding the loop head's lexical binding names to
/// the forbidden set so a same-named block function in the body skips its
/// AnnexB legacy `var` (the equivalent `var` would be an early error).
fn annexb_nested_loop_body(
    body: &ast::Stmt,
    lexical_names: Vec<String>,
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    if lexical_names.is_empty() {
        annexb_nested_stmt(body, forbidden, all_out, var_out);
    } else {
        let mut inner = forbidden.clone();
        inner.extend(lexical_names);
        annexb_nested_stmt(body, &inner, all_out, var_out);
    }
}

fn annexb_nested_block(
    stmts: &[ast::Stmt],
    forbidden: &std::collections::HashSet<String>,
    all_out: &mut Vec<String>,
    var_out: &mut Vec<String>,
) {
    let mut inner = forbidden.clone();
    collect_lexical_decl_names(stmts, &mut inner);
    for stmt in stmts {
        annexb_nested_stmt(stmt, &inner, all_out, var_out);
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
        // O(1) innermost-in-scope lookup instead of an O(n) reverse scan of
        // `locals[scope_start..]` per var name — the per-binding scan made a
        // function body with N `var`s lower in O(n²) (#5267).
        let existing_current_scope = ctx
            .locals
            .lookup_index_in_scope(&name, scope_start)
            .map(|pos| ctx.locals[pos].1);
        let local_id = existing_current_scope.unwrap_or_else(|| {
            let id = ctx.define_local(name.clone(), Type::Any);
            created.push((name, id));
            id
        });
        ctx.var_hoisted_ids.insert(local_id);
    }

    // Annex B B.3.3 (#5297): in sloppy mode a block-nested `function f(){}`
    // also gets an enclosing-scope `var f` (undefined until the declaration
    // runs). Register one hoisted slot per such name and record name -> slot in
    // `annexb_block_fn_var_ids` so `lower_nested_fn_decl` can write the closure
    // into it at the declaration point while keeping the block-local binding
    // independent. Strict bodies get pure block scoping (no outer var).
    if !ctx.current_strict {
        // Forbidden: names for which the legacy `var` must be skipped. The
        // body's own top-level `let`/`const`/`class` make `var f` an early
        // error; `arguments` is excluded by spec. Parameters are handled below
        // (at this point only params and the `var`s collected just above are in
        // this scope — a `var`-hoisted binding is reusable, a non-hoisted one is
        // a parameter and yields to it).
        let mut forbidden = std::collections::HashSet::new();
        collect_lexical_decl_names(&block.stmts, &mut forbidden);
        forbidden.insert("arguments".to_string());

        let mut all_names = Vec::new();
        let mut annexb_names = Vec::new();
        collect_annexb_block_fn_decl_names(
            &block.stmts,
            &forbidden,
            &mut all_names,
            &mut annexb_names,
        );
        ctx.annexb_block_fn_names_all.extend(all_names);
        annexb_names.sort();
        annexb_names.dedup();
        for name in annexb_names {
            let existing = ctx
                .locals
                .lookup_index_in_scope(&name, scope_start)
                .map(|pos| ctx.locals[pos].1);
            match existing {
                Some(id) if ctx.var_hoisted_ids.contains(&id) => {
                    // Shares the existing `var` binding (entry slot already
                    // emitted via `created` by the var pre-pass above).
                    ctx.annexb_block_fn_var_ids.insert(name, id);
                }
                Some(_) => {
                    // A parameter of the same name — B.3.3 yields to it.
                }
                None => {
                    let id = ctx.define_local(name.clone(), Type::Any);
                    created.push((name.clone(), id));
                    ctx.var_hoisted_ids.insert(id);
                    ctx.annexb_block_fn_var_ids.insert(name, id);
                }
            }
        }
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
    // Annex B B.3.3 (#5297): this body's block-nested function declarations get
    // their own enclosing-scope `var` map; nested function bodies lowered while
    // we are inside this one save/restore their own, so take ours aside now and
    // restore it on every exit. `predefine_var_bindings_in_function_body`
    // repopulates it for this body below.
    let saved_annexb_block_fn_var_ids = std::mem::take(&mut ctx.annexb_block_fn_var_ids);
    let saved_annexb_block_fn_names_all = std::mem::take(&mut ctx.annexb_block_fn_names_all);
    // Nested `function*` declarations forward-referenced by an earlier sibling
    // in THIS body must use the closure-lowering path (see `lower_body_stmt`'s
    // FnDecl arm). Scope the set to this body and restore on every exit.
    let saved_nested_gen_fwd = std::mem::take(&mut ctx.nested_generator_forward_referenced);
    ctx.nested_generator_forward_referenced =
        crate::lower_decl::forward_referenced_nested_generators(&block.stmts)
            .into_iter()
            .collect();
    // Boundary between outer-scope locals (+ this function's params, defined by
    // the caller before entry) and locals defined while lowering THIS body.
    // Used by the Phase 1.6 forward `let`/`const` pre-registration so a const
    // that shadows an outer binding still gets a fresh this-body local.
    let body_entry_locals_len = ctx.locals.len();
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
            // Reuse only a CURRENT-scope binding (a sibling `var`/`function`
            // hoisted into this same body). A same-named local from an OUTER
            // scope must be shadowed with a fresh local, else this declaration
            // would write into the outer binding's box at runtime.
            let local_id = if let Some(existing) = ctx.lookup_local_in_current_scope(&name) {
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
    let saved_forward_class_decl_depth = ctx.forward_class_decl_depth.clone();
    let saved_class_renames = ctx.class_renames.clone();
    let cur_scope_depth = ctx.scope_depth;
    for stmt in &block.stmts {
        if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
            // Disambiguate a distinct same-named class declared in this body so
            // its references don't bind to a colliding `class X` elsewhere in
            // the bundled module (see `class_renames`).
            ctx.maybe_rename_colliding_class(class_decl.ident.sym.as_str());
            let cname = class_decl.ident.sym.to_string();
            // Record the (shallowest) scope depth this class is declared at so a
            // later bare-ident reference can compare it against a same-named
            // local by JS nearest-binding rules.
            ctx.forward_class_decl_depth
                .entry(cname.clone())
                .and_modify(|d| *d = (*d).min(cur_scope_depth))
                .or_insert(cur_scope_depth);
            ctx.forward_class_names.insert(cname);
        }
    }

    // Phase 1.6: pre-register top-level `let`/`const` Ident bindings of this
    // function body so a closure created EARLIER in the body — a hoisted
    // FnDecl, or an arrow / function expression assigned to a `const`/`let`
    // (`const handler = async (req, res) => { … later … }`) — that references
    // a binding declared LATER (`const later = …`) resolves it to the (boxed)
    // function-scope local instead of falling through to a `globalThis` read.
    // Next.js `router-server.js` `initialize()` does exactly this: the request
    // handler closure reads `relativeProjectDir`, a `const` declared ~400
    // lines later in the same function — without this it lowered to a global
    // read and threw `ReferenceError: relativeProjectDir is not defined` at
    // request time. Each pre-registered binding is boxed (`var_hoisted_ids`),
    // its declaration reuses the same id via `lexical_forward_decls`, and its
    // box is preallocated at function entry (`forward_boxed_ids`, merged into
    // the Phase-5 prealloc set below) so the earlier closure literal captures
    // the live box. Scoped to bindings a closure ACTUALLY references
    // (`collect_closure_referenced_idents`) — boxing every top-level binding
    // regressed Next.js at scale.
    //
    // CRUCIAL: these ids are NOT added to `hoisted_id_set`. Phase 3 hoists
    // every `Let { init: Closure }` in that set to the function top, which is
    // correct ONLY for `function` declarations (they hoist per spec). A
    // `const handler = async () => {}` does NOT hoist — reordering it ahead of
    // the bindings it depends on corrupted `initialize()` (the server exited
    // after "Ready"). We therefore prealloc the captured boxes directly
    // instead of routing through the FnDecl-hoisting machinery. Shared with the
    // function-expression body path (`lower_fn_expr`) via
    // `pre_register_forward_captured_lets`; also handles destructuring leaves
    // (`const { SpanKind } = api`).
    let forward_boxed_ids = pre_register_forward_captured_lets(ctx, block, body_entry_locals_len);

    // Phase 2: lower the body. The inner FnDecl arm in `lower_body_stmt`
    // calls `lookup_local(name)` and reuses our pre-defined id.
    let mut body = match lower_block_stmt(ctx, block) {
        Ok(body) => body,
        Err(err) => {
            ctx.current_strict = parent_strict;
            ctx.forward_class_names = saved_forward_class_names;
            ctx.forward_class_decl_depth = saved_forward_class_decl_depth;
            ctx.class_renames = saved_class_renames;
            ctx.annexb_block_fn_var_ids = saved_annexb_block_fn_var_ids;
            ctx.nested_generator_forward_referenced = saved_nested_gen_fwd;
            ctx.annexb_block_fn_names_all = saved_annexb_block_fn_names_all;
            return Err(err);
        }
    };
    // Re-register capture snapshots for classes declared in this body at
    // its END. The decl-site `RegisterClassCaptures` runs before later
    // statements assign captured vars (tsc emits TS-enum namespaces AFTER
    // the classes that reference them — vendored zod's
    // ZodFirstPartyTypeKind), so static-method snapshot reads and post-
    // return dynamic constructions need the FINAL values.
    //
    // MUST run while this body's `class_renames` are still ACTIVE, using the
    // RESOLVED class name (2026-07-02 audit P0): with the restore-first +
    // raw-ident order, factory B's renamed `class e` (→ `e$0`) re-registered
    // the OUTER factory's `e` with B's out-of-scope ids — codegen's LocalGet
    // soft-fallback then clobbered A's snapshot to all-undefined the moment
    // B ran, resurrecting the W6 undefined-captures class for every dynamic
    // construct of A's class; meanwhile `e$0` itself never got refreshed and
    // its forward-ref `new` sites never got their cap args appended.
    // Mirrors the function-expression twin (`lower_fn_expr`), including the
    // refresh-before-EVERY-return placement.
    {
        let mut re_regs: Vec<Stmt> = Vec::new();
        let mut re_reg_capsets: Vec<(Stmt, std::collections::HashSet<perry_types::LocalId>)> =
            Vec::new();
        for stmt in &block.stmts {
            if let ast::Stmt::Decl(ast::Decl::Class(class_decl)) = stmt {
                let cname = ctx.resolve_class_name(class_decl.ident.sym.as_str());
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
                        let re_reg = Stmt::Expr(Expr::RegisterClassCaptures {
                            class_name: cname,
                            captures,
                        });
                        re_reg_capsets.push((re_reg.clone(), captured.iter().copied().collect()));
                        re_regs.push(re_reg);
                    }
                }
            }
        }
        if !re_regs.is_empty() {
            // Audit P0-B: the decl-site snapshot is authoritative at
            // construct time, so keep it TRACKING same-body assignments —
            // refresh after every statement that assigns a captured local
            // (else `let x=1; class C{..x..}; x=2; new C()` reads 1 and the
            // capture write-back resets x to 1).
            crate::lower::expr_function::insert_class_capture_refresh_after_assignments(
                &mut body,
                &re_reg_capsets,
            );
            crate::lower::expr_function::insert_class_capture_refresh_before_returns(
                &mut body, &re_regs,
            );
        }
    }
    ctx.forward_class_names = saved_forward_class_names;
    ctx.forward_class_decl_depth = saved_forward_class_decl_depth;
    ctx.class_renames = saved_class_renames;

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

    if hoisted_id_set.is_empty() && forward_boxed_ids.is_empty() {
        ctx.current_strict = parent_strict;
        ctx.annexb_block_fn_var_ids = saved_annexb_block_fn_var_ids;
        ctx.annexb_block_fn_names_all = saved_annexb_block_fn_names_all;
        ctx.nested_generator_forward_referenced = saved_nested_gen_fwd;
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

    // Phase 4: compute the prealloc-box set via shared helper, then add the
    // forward-captured `let`/`const` boxes pre-registered in Phase 1.6. Those
    // are deliberately kept out of `hoisted_id_set` (so Phase 3 doesn't hoist
    // their non-hoistable `const = closure` declarations), so their boxes must
    // be preallocated here explicitly — the earlier closure literal captures
    // the box before the declaration assigns through it.
    let combined: Vec<Stmt> = hoisted_lets.iter().chain(other.iter()).cloned().collect();
    let mut prealloc = compute_prealloc_for_hoisted_closures(&combined, &hoisted_id_set);
    for id in forward_boxed_ids {
        if !prealloc.contains(&id) {
            prealloc.push(id);
        }
    }
    prealloc.sort();

    // Split the prealloc set into TDZ-seeded lexical `let`/`const` boxes and
    // ordinary (`var` / hoisted-closure / FnDecl-capture) boxes. Only genuine
    // lexical bindings recorded in `tdz_forward_ids` get the TAG_TDZ sentinel;
    // everything else keeps the historical `undefined`-seeded behavior so a
    // forward-captured `var` or hoisted function still reads `undefined`, never
    // throws. The dead zone ends when the binding's own `Stmt::Let` runs.
    let mut tdz_prealloc: Vec<LocalId> = Vec::new();
    let mut plain_prealloc: Vec<LocalId> = Vec::new();
    for id in prealloc {
        if ctx.tdz_forward_ids.contains(&id) {
            tdz_prealloc.push(id);
        } else {
            plain_prealloc.push(id);
        }
    }

    // Phase 5: assemble the final body — PreallocateBoxes /
    // PreallocateTdzBoxes (if any), then the hoisted FnDecl Lets, then
    // everything else.
    let mut result: Vec<Stmt> = Vec::new();
    if !plain_prealloc.is_empty() {
        result.push(Stmt::PreallocateBoxes(plain_prealloc));
    }
    if !tdz_prealloc.is_empty() {
        result.push(Stmt::PreallocateTdzBoxes(tdz_prealloc));
    }
    result.extend(var_slot_lets);
    result.extend(hoisted_lets);
    result.extend(other);
    ctx.current_strict = parent_strict;
    ctx.annexb_block_fn_var_ids = saved_annexb_block_fn_var_ids;
    ctx.annexb_block_fn_names_all = saved_annexb_block_fn_names_all;
    ctx.nested_generator_forward_referenced = saved_nested_gen_fwd;
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
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
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
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        // Array-destructuring bindings (`let [a, b] = it`) lower to the iterator
        // protocol wrapped in a `Try` (see
        // `destructuring::pattern_binding::lower_array_pattern_binding`), so the
        // leaf `let`s live INSIDE the try body / its `if`s, not at the top level.
        // Recurse into those so a hoisted FnDecl that captures a destructured var
        // still gets its box preallocated at function entry. Without this, the
        // hoisted closure captures the not-yet-assigned, unboxed slot and reads
        // `undefined` — e.g. `const [K,_] = useState(0)` captured by a hoisted
        // handler. Loops are intentionally NOT recursed: a per-iteration `let`
        // needs a fresh box, not a single entry-prealloc.
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_top_level_let_ids_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_top_level_let_ids_stmt(s, out);
                }
            }
            if let Some(f) = finally {
                for s in f {
                    collect_top_level_let_ids_stmt(s, out);
                }
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_top_level_let_ids_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_top_level_let_ids_stmt(s, out);
                }
            }
        }
        _ => {}
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
    // Via `lower_block_stmt` so this scope's pre-registered forward-captured
    // lets are re-bound at entry (`rebind_nested_forward_scope_lets`).
    let stmts = lower_block_stmt(ctx, block)?;
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
/// #6062: record a block's DIRECT `let`/`const`/`class` declaration names (not
/// nested blocks — those own their scope) in `ctx.forward_lexical_names`, so a
/// `typeof <name>` lowered before the declarator throws a TDZ `ReferenceError`
/// instead of yielding `"undefined"`. Returns the names this call newly
/// inserted, for removal on block exit (a name already present belongs to an
/// enclosing scope and must survive this block).
fn register_block_forward_lexicals(ctx: &mut LoweringContext, stmts: &[ast::Stmt]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for stmt in stmts {
        match stmt {
            ast::Stmt::Decl(ast::Decl::Var(var_decl))
                if matches!(
                    var_decl.kind,
                    ast::VarDeclKind::Let | ast::VarDeclKind::Const
                ) =>
            {
                for decl in &var_decl.decls {
                    let mut idents: Vec<(String, u32)> = Vec::new();
                    collect_pat_forward_idents(&decl.name, &mut idents);
                    names.extend(idents.into_iter().map(|(n, _)| n));
                }
            }
            ast::Stmt::Decl(ast::Decl::Class(class_decl)) => {
                names.push(class_decl.ident.sym.to_string());
            }
            // `using` / `await using` are block-scoped bindings with the same
            // TDZ semantics as `let`/`const` (a read before the declarator
            // throws), so `typeof <forward using>` must throw too.
            ast::Stmt::Decl(ast::Decl::Using(using_decl)) => {
                for decl in &using_decl.decls {
                    let mut idents: Vec<(String, u32)> = Vec::new();
                    collect_pat_forward_idents(&decl.name, &mut idents);
                    names.extend(idents.into_iter().map(|(n, _)| n));
                }
            }
            _ => {}
        }
    }
    let mut newly = Vec::new();
    for name in names {
        if ctx.forward_lexical_names.insert(name.clone()) {
            newly.push(name);
        }
    }
    newly
}

pub fn lower_stmts_using_aware(
    ctx: &mut LoweringContext,
    stmts: &[ast::Stmt],
) -> Result<Vec<Stmt>> {
    // #6062: register this block's forward lexicals before lowering any
    // statement (so a `typeof z` preceding `const z` sees `z`), then remove
    // exactly what we added once the block is lowered — via a wrapper so the
    // cleanup runs on every early return inside the inner lowering.
    let newly = register_block_forward_lexicals(ctx, stmts);
    let r = lower_stmts_using_aware_inner(ctx, stmts);
    for name in &newly {
        ctx.forward_lexical_names.remove(name);
    }
    r
}

fn lower_stmts_using_aware_inner(
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
                            byte_offset: 0,
                            object: Box::new(Expr::LocalGet(id)),
                            property: "__perry_using_check__".to_string(),
                        }),
                        args: vec![Expr::Bool(is_async)],
                        type_args: Vec::new(),
                        byte_offset: 0,
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
                        byte_offset: 0,
                        object: Box::new(Expr::LocalGet(id)),
                        property: method_name.to_string(),
                    }),
                    args: Vec::new(),
                    type_args: Vec::new(),
                    byte_offset: 0,
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
                                byte_offset: 0,
                                cap_args_appended: 0,
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
