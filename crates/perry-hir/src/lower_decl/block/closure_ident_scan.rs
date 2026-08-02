//! `cic_*` — the "collect identifiers referenced from inside a closure"
//! traversal used by `pre_register_forward_captured_lets`.
//!
//! Moved verbatim out of `lower_decl/block.rs` (file-size gate).

use swc_ecma_ast as ast;

pub(super) fn cic_stmt(s: &ast::Stmt, in_cl: bool, out: &mut std::collections::HashSet<String>) {
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

pub(super) fn cic_expr(e: &ast::Expr, in_cl: bool, out: &mut std::collections::HashSet<String>) {
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
