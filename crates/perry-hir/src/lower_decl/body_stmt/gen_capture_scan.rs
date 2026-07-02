//! Conservative free-variable scan for nested generator declarations.
//!
//! A nested `function*` declaration that references any enclosing-scope local
//! must be lowered as a generator `Expr::Closure` (so the closure-aware
//! generator transform threads its captures into the synthesized step
//! closures) rather than hoisted to a capture-less top-level `Function`.
//! Otherwise the free variables forward into the step closures as nullish
//! values (`Cannot convert undefined or null to object` when the body indexes
//! them) — the path-to-regexp `lexer`/`SIMPLE_TOKENS` failure.
//!
//! This walker collects every identifier reference in the function body
//! (over-approximating: collecting an extra name only routes through the
//! closure path, which is always correct), then asks `ctx.lookup_local` whether
//! any of them resolves to a live enclosing local. Identifiers bound *inside*
//! the body (params, inner declarations) are not in the enclosing scope, so
//! `lookup_local` only returns `Some` for genuine outer captures.

use super::*;

/// Scan an enclosing function/IIFE body's statements and return the names of
/// nested `function*` declarations that are referenced by an EARLIER sibling
/// statement (a forward reference). Such generators must be lowered via the
/// closure path (see `lower_body_stmt`'s FnDecl arm), because the top-level
/// hoist path registers the `FuncRef` name binding too late for the earlier
/// reference. Statements are scanned in source order: a generator name is
/// "forward referenced" if it appears in any identifier position before its own
/// declaration statement.
pub(crate) fn forward_referenced_nested_generators(stmts: &[ast::Stmt]) -> Vec<String> {
    // Collect declaration order of nested generator fn-decls.
    let mut gen_decl_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (i, stmt) in stmts.iter().enumerate() {
        if let ast::Stmt::Decl(ast::Decl::Fn(fd)) = stmt {
            if fd.function.is_generator && fd.function.body.is_some() {
                gen_decl_index.entry(fd.ident.sym.to_string()).or_insert(i);
            }
        }
    }
    if gen_decl_index.is_empty() {
        return Vec::new();
    }
    let mut forward: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, stmt) in stmts.iter().enumerate() {
        // The generator's own declaration statement is not a forward reference
        // to itself (self-recursion is handled inside its body either way).
        let mut names: Vec<String> = Vec::new();
        collect_idents_stmt(stmt, &mut names);
        for n in names {
            if let Some(&decl_i) = gen_decl_index.get(&n) {
                if i < decl_i {
                    forward.insert(n);
                }
            }
        }
    }
    forward.into_iter().collect()
}

/// Does this nested generator body reference any identifier bound in an
/// enclosing (outer) scope?
pub(super) fn nested_generator_references_outer_locals(
    ctx: &LoweringContext,
    func: &ast::Function,
    own_name: &str,
) -> bool {
    let Some(body) = func.body.as_ref() else {
        return false;
    };
    // The generator's own name (for self-recursion) and its parameters bind
    // inside the function, not the enclosing scope — exclude them so a purely
    // self-recursive generator (e.g. path-to-regexp `flatten`) is NOT treated
    // as capturing. (Function declarations are hoisted, so a pre-scan may have
    // already registered the own name as a local; without this exclusion the
    // self-reference would force the closure path and lose hoisting.)
    let mut bound: std::collections::HashSet<String> = std::collections::HashSet::new();
    bound.insert(own_name.to_string());
    for p in &func.params {
        collect_pat_bound_names(&p.pat, &mut bound);
    }

    let mut names: Vec<String> = Vec::new();
    for stmt in &body.stmts {
        collect_idents_stmt(stmt, &mut names);
    }
    names
        .iter()
        .any(|n| !bound.contains(n) && ctx.lookup_local(n).is_some())
}

fn collect_pat_bound_names(pat: &ast::Pat, out: &mut std::collections::HashSet<String>) {
    match pat {
        ast::Pat::Ident(i) => {
            out.insert(i.id.sym.to_string());
        }
        ast::Pat::Array(a) => {
            for e in a.elems.iter().flatten() {
                collect_pat_bound_names(e, out);
            }
        }
        ast::Pat::Rest(r) => collect_pat_bound_names(&r.arg, out),
        ast::Pat::Object(o) => {
            for p in &o.props {
                match p {
                    ast::ObjectPatProp::KeyValue(kv) => collect_pat_bound_names(&kv.value, out),
                    ast::ObjectPatProp::Assign(a) => {
                        out.insert(a.key.sym.to_string());
                    }
                    ast::ObjectPatProp::Rest(r) => collect_pat_bound_names(&r.arg, out),
                }
            }
        }
        ast::Pat::Assign(a) => collect_pat_bound_names(&a.left, out),
        _ => {}
    }
}

fn push_ident(name: &str, out: &mut Vec<String>) {
    out.push(name.to_string());
}

fn collect_idents_expr(expr: &ast::Expr, out: &mut Vec<String>) {
    use ast::Expr;
    match expr {
        Expr::Ident(i) => push_ident(i.sym.as_ref(), out),
        Expr::This(_) | Expr::Lit(_) | Expr::Invalid(_) => {}
        Expr::Array(a) => {
            for e in a.elems.iter().flatten() {
                collect_idents_expr(&e.expr, out);
            }
        }
        Expr::Object(o) => {
            for p in &o.props {
                match p {
                    ast::PropOrSpread::Spread(s) => collect_idents_expr(&s.expr, out),
                    ast::PropOrSpread::Prop(prop) => collect_idents_prop(prop, out),
                }
            }
        }
        Expr::Fn(_) | Expr::Arrow(_) => {
            // Nested functions/arrows form their own scopes. Their free
            // references could still be outer captures of THIS generator, so
            // recurse to over-approximate.
            collect_idents_nested_callable(expr, out);
        }
        Expr::Unary(u) => collect_idents_expr(&u.arg, out),
        Expr::Update(u) => collect_idents_expr(&u.arg, out),
        Expr::Bin(b) => {
            collect_idents_expr(&b.left, out);
            collect_idents_expr(&b.right, out);
        }
        Expr::Assign(a) => {
            collect_idents_assign_target(&a.left, out);
            collect_idents_expr(&a.right, out);
        }
        Expr::Member(m) => {
            collect_idents_expr(&m.obj, out);
            if let ast::MemberProp::Computed(c) = &m.prop {
                collect_idents_expr(&c.expr, out);
            }
        }
        Expr::SuperProp(s) => {
            if let ast::SuperProp::Computed(c) = &s.prop {
                collect_idents_expr(&c.expr, out);
            }
        }
        Expr::Cond(c) => {
            collect_idents_expr(&c.test, out);
            collect_idents_expr(&c.cons, out);
            collect_idents_expr(&c.alt, out);
        }
        Expr::Call(c) => {
            if let ast::Callee::Expr(e) = &c.callee {
                collect_idents_expr(e, out);
            }
            for a in &c.args {
                collect_idents_expr(&a.expr, out);
            }
        }
        Expr::New(n) => {
            collect_idents_expr(&n.callee, out);
            if let Some(args) = &n.args {
                for a in args {
                    collect_idents_expr(&a.expr, out);
                }
            }
        }
        Expr::Seq(s) => {
            for e in &s.exprs {
                collect_idents_expr(e, out);
            }
        }
        Expr::Tpl(t) => {
            for e in &t.exprs {
                collect_idents_expr(e, out);
            }
        }
        Expr::TaggedTpl(t) => {
            collect_idents_expr(&t.tag, out);
            for e in &t.tpl.exprs {
                collect_idents_expr(e, out);
            }
        }
        Expr::Paren(p) => collect_idents_expr(&p.expr, out),
        Expr::Yield(y) => {
            if let Some(a) = &y.arg {
                collect_idents_expr(a, out);
            }
        }
        Expr::Await(a) => collect_idents_expr(&a.arg, out),
        Expr::OptChain(o) => collect_idents_opt_chain(&o.base, out),
        Expr::TsAs(t) => collect_idents_expr(&t.expr, out),
        Expr::TsConstAssertion(t) => collect_idents_expr(&t.expr, out),
        Expr::TsNonNull(t) => collect_idents_expr(&t.expr, out),
        Expr::TsTypeAssertion(t) => collect_idents_expr(&t.expr, out),
        Expr::TsSatisfies(t) => collect_idents_expr(&t.expr, out),
        Expr::TsInstantiation(t) => collect_idents_expr(&t.expr, out),
        // Class expressions / JSX / others: conservatively skip. A class
        // expression's body could capture, but these are rare in generator
        // bodies; if missed, the worst case is the old (top-level Function)
        // path, which is exactly the prior behavior.
        _ => {}
    }
}

fn collect_idents_opt_chain(base: &ast::OptChainBase, out: &mut Vec<String>) {
    match base {
        ast::OptChainBase::Member(m) => {
            collect_idents_expr(&m.obj, out);
            if let ast::MemberProp::Computed(c) = &m.prop {
                collect_idents_expr(&c.expr, out);
            }
        }
        ast::OptChainBase::Call(c) => {
            collect_idents_expr(&c.callee, out);
            for a in &c.args {
                collect_idents_expr(&a.expr, out);
            }
        }
    }
}

fn collect_idents_prop(prop: &ast::Prop, out: &mut Vec<String>) {
    match prop {
        ast::Prop::Shorthand(i) => push_ident(i.sym.as_ref(), out),
        ast::Prop::KeyValue(kv) => {
            if let ast::PropName::Computed(c) = &kv.key {
                collect_idents_expr(&c.expr, out);
            }
            collect_idents_expr(&kv.value, out);
        }
        ast::Prop::Assign(a) => collect_idents_expr(&a.value, out),
        ast::Prop::Getter(_) | ast::Prop::Setter(_) | ast::Prop::Method(_) => {
            // Method bodies are their own scopes; over-approximating recursion
            // is unnecessary for the common cases — skip.
        }
    }
}

fn collect_idents_nested_callable(expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Fn(f) => {
            if let Some(b) = &f.function.body {
                for s in &b.stmts {
                    collect_idents_stmt(s, out);
                }
            }
        }
        ast::Expr::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => {
                for s in &b.stmts {
                    collect_idents_stmt(s, out);
                }
            }
            ast::BlockStmtOrExpr::Expr(e) => collect_idents_expr(e, out),
        },
        _ => {}
    }
}

fn collect_idents_assign_target(t: &ast::AssignTarget, out: &mut Vec<String>) {
    match t {
        ast::AssignTarget::Simple(s) => match s {
            ast::SimpleAssignTarget::Ident(i) => push_ident(i.id.sym.as_ref(), out),
            ast::SimpleAssignTarget::Member(m) => {
                collect_idents_expr(&m.obj, out);
                if let ast::MemberProp::Computed(c) = &m.prop {
                    collect_idents_expr(&c.expr, out);
                }
            }
            _ => {}
        },
        ast::AssignTarget::Pat(_) => {}
    }
}

fn collect_idents_var_decl(decl: &ast::VarDecl, out: &mut Vec<String>) {
    for d in &decl.decls {
        if let Some(init) = &d.init {
            collect_idents_expr(init, out);
        }
    }
}

fn collect_idents_stmt(stmt: &ast::Stmt, out: &mut Vec<String>) {
    use ast::Stmt;
    match stmt {
        Stmt::Expr(e) => collect_idents_expr(&e.expr, out),
        Stmt::Decl(ast::Decl::Var(v)) => collect_idents_var_decl(v, out),
        Stmt::Decl(ast::Decl::Fn(f)) => {
            if let Some(b) = &f.function.body {
                for s in &b.stmts {
                    collect_idents_stmt(s, out);
                }
            }
        }
        Stmt::Decl(_) => {}
        Stmt::Block(b) => {
            for s in &b.stmts {
                collect_idents_stmt(s, out);
            }
        }
        Stmt::Return(r) => {
            if let Some(a) = &r.arg {
                collect_idents_expr(a, out);
            }
        }
        Stmt::If(i) => {
            collect_idents_expr(&i.test, out);
            collect_idents_stmt(&i.cons, out);
            if let Some(alt) = &i.alt {
                collect_idents_stmt(alt, out);
            }
        }
        Stmt::While(w) => {
            collect_idents_expr(&w.test, out);
            collect_idents_stmt(&w.body, out);
        }
        Stmt::DoWhile(w) => {
            collect_idents_stmt(&w.body, out);
            collect_idents_expr(&w.test, out);
        }
        Stmt::For(f) => {
            match &f.init {
                Some(ast::VarDeclOrExpr::Expr(e)) => collect_idents_expr(e, out),
                Some(ast::VarDeclOrExpr::VarDecl(v)) => collect_idents_var_decl(v, out),
                None => {}
            }
            if let Some(t) = &f.test {
                collect_idents_expr(t, out);
            }
            if let Some(u) = &f.update {
                collect_idents_expr(u, out);
            }
            collect_idents_stmt(&f.body, out);
        }
        Stmt::ForIn(f) => {
            collect_idents_expr(&f.right, out);
            if let ast::ForHead::VarDecl(v) = &f.left {
                collect_idents_var_decl(v, out);
            }
            collect_idents_stmt(&f.body, out);
        }
        Stmt::ForOf(f) => {
            collect_idents_expr(&f.right, out);
            if let ast::ForHead::VarDecl(v) = &f.left {
                collect_idents_var_decl(v, out);
            }
            collect_idents_stmt(&f.body, out);
        }
        Stmt::Switch(s) => {
            collect_idents_expr(&s.discriminant, out);
            for case in &s.cases {
                if let Some(t) = &case.test {
                    collect_idents_expr(t, out);
                }
                for st in &case.cons {
                    collect_idents_stmt(st, out);
                }
            }
        }
        Stmt::Throw(t) => collect_idents_expr(&t.arg, out),
        Stmt::Try(t) => {
            for s in &t.block.stmts {
                collect_idents_stmt(s, out);
            }
            if let Some(h) = &t.handler {
                for s in &h.body.stmts {
                    collect_idents_stmt(s, out);
                }
            }
            if let Some(f) = &t.finalizer {
                for s in &f.stmts {
                    collect_idents_stmt(s, out);
                }
            }
        }
        Stmt::Labeled(l) => collect_idents_stmt(&l.body, out),
        Stmt::With(w) => {
            collect_idents_expr(&w.obj, out);
            collect_idents_stmt(&w.body, out);
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) | Stmt::Debugger(_) => {}
    }
}
