//! Compile-time builder WIDTH-hint scan. Split out of `builder_fold.rs`
//! to keep that file under the repo's 2000-line-per-file cap (#10361).
//!
//! #6812 (w16): compile-time builder WIDTH scan.
//!
//! `fold_builder_sequences` in `builder_fold.rs` handles STATIC-key
//! builders. A builder whose keys are computed
//! (`for (let k = 0; k < 24; k++) o["p" + k] = v;`)
//! cannot be folded into a literal, but when the build loop is
//! constant-bounded its FINAL width is still statically known. That width
//! matters beyond the learned resize: the runtime learns a site's width only
//! when the FIRST instance overflows, so that instance stays under-sized
//! forever — and as element 0 of the array built at the site it vetoes the
//! whole-loop clone guard ("first receiver target slot is out of bounds")
//! for every hot loop over that array. A width hint right-sizes instance #1
//! so the site's instances are uniform from the first allocation.
//!
//! The hint is pure allocation capacity: over-counting (e.g. duplicate keys
//! across iterations) wastes slots but can never change semantics, so the
//! VALUE side of the assignments is deliberately unconstrained.

use swc_ecma_ast as ast;

/// Scan for `const o = {};` immediately followed by a constant-bounded
/// build loop writing only to `o`. Returns `span.lo` of each empty object
/// literal → proven final width (writes per iteration × trip count).
pub(crate) fn empty_builder_width_hints(
    module: &ast::Module,
) -> std::collections::HashMap<u32, u32> {
    let mut hints = std::collections::HashMap::new();
    // Pair `const o = {}` (plain or `export const`) with a following build
    // loop; the loop itself is always a plain Stmt.
    for w in module.body.windows(2) {
        let ast::ModuleItem::Stmt(b) = &w[1] else {
            continue;
        };
        if let Some((name, span_lo)) = item_empty_object_decl(&w[0]) {
            note_hint_for_site(&name, span_lo, b, &mut hints);
        }
    }
    for item in &module.body {
        match item {
            ast::ModuleItem::Stmt(s) => hint_walk_stmt(s, &mut hints),
            // Exported declarations are ModuleDecls, not Stmts — and
            // `export function buildX() { const o = {}; ... }` is the most
            // common real-world builder shape.
            ast::ModuleItem::ModuleDecl(md) => match md {
                ast::ModuleDecl::ExportDecl(e) => hint_walk_hint_decl(&e.decl, &mut hints),
                ast::ModuleDecl::ExportDefaultDecl(d) => match &d.decl {
                    ast::DefaultDecl::Fn(f) => {
                        if let Some(body) = &f.function.body {
                            hint_scan_stmts(&body.stmts, &mut hints);
                        }
                    }
                    ast::DefaultDecl::Class(c) => hint_walk_class(&c.class, &mut hints),
                    ast::DefaultDecl::TsInterfaceDecl(_) => {}
                },
                ast::ModuleDecl::ExportDefaultExpr(e) => hint_walk_expr(&e.expr, &mut hints),
                _ => {}
            },
        }
    }
    hints
}

/// `const/let name = {}` from a plain statement or an `export const`.
/// Returns the binding name and the empty literal's span.lo.
fn item_empty_object_decl(item: &ast::ModuleItem) -> Option<(String, u32)> {
    match item {
        ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(v))) => empty_object_decl(v),
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => match &e.decl {
            ast::Decl::Var(v) => empty_object_decl(v),
            _ => None,
        },
        _ => None,
    }
}

fn empty_object_decl(var: &ast::VarDecl) -> Option<(String, u32)> {
    if var.decls.len() != 1 {
        return None;
    }
    let d = &var.decls[0];
    let ast::Pat::Ident(bi) = &d.name else {
        return None;
    };
    let ast::Expr::Object(obj) = d.init.as_deref()? else {
        return None;
    };
    if !obj.props.is_empty() {
        return None;
    }
    Some((bi.id.sym.to_string(), obj.span.lo.0))
}

fn hint_scan_stmts(stmts: &[ast::Stmt], hints: &mut std::collections::HashMap<u32, u32>) {
    for w in stmts.windows(2) {
        note_hint_pair(&w[0], &w[1], hints);
    }
    for s in stmts {
        hint_walk_stmt(s, hints);
    }
}

fn hint_walk_stmt(s: &ast::Stmt, hints: &mut std::collections::HashMap<u32, u32>) {
    match s {
        ast::Stmt::Block(b) => hint_scan_stmts(&b.stmts, hints),
        ast::Stmt::If(i) => {
            hint_walk_stmt(&i.cons, hints);
            if let Some(alt) = &i.alt {
                hint_walk_stmt(alt, hints);
            }
        }
        ast::Stmt::While(w) => hint_walk_stmt(&w.body, hints),
        ast::Stmt::DoWhile(w) => hint_walk_stmt(&w.body, hints),
        ast::Stmt::For(f) => hint_walk_stmt(&f.body, hints),
        ast::Stmt::ForIn(f) => hint_walk_stmt(&f.body, hints),
        ast::Stmt::ForOf(f) => hint_walk_stmt(&f.body, hints),
        ast::Stmt::Labeled(l) => hint_walk_stmt(&l.body, hints),
        ast::Stmt::Try(t) => {
            hint_scan_stmts(&t.block.stmts, hints);
            if let Some(h) = &t.handler {
                hint_scan_stmts(&h.body.stmts, hints);
            }
            if let Some(f) = &t.finalizer {
                hint_scan_stmts(&f.stmts, hints);
            }
        }
        ast::Stmt::Switch(sw) => {
            for case in &sw.cases {
                hint_scan_stmts(&case.cons, hints);
            }
        }
        ast::Stmt::Decl(d) => hint_walk_hint_decl(d, hints),
        ast::Stmt::Expr(e) => hint_walk_expr(&e.expr, hints),
        ast::Stmt::Return(r) => {
            if let Some(arg) = &r.arg {
                hint_walk_expr(arg, hints);
            }
        }
        _ => {}
    }
}

fn hint_walk_hint_decl(d: &ast::Decl, hints: &mut std::collections::HashMap<u32, u32>) {
    match d {
        ast::Decl::Fn(f) => {
            if let Some(body) = &f.function.body {
                hint_scan_stmts(&body.stmts, hints);
            }
        }
        ast::Decl::Var(v) => {
            for decl in &v.decls {
                if let Some(init) = &decl.init {
                    hint_walk_expr(init, hints);
                }
            }
        }
        ast::Decl::Class(c) => hint_walk_class(&c.class, hints),
        _ => {}
    }
}

fn hint_walk_class(class: &ast::Class, hints: &mut std::collections::HashMap<u32, u32>) {
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) => {
                if let Some(body) = &m.function.body {
                    hint_scan_stmts(&body.stmts, hints);
                }
            }
            ast::ClassMember::PrivateMethod(m) => {
                if let Some(body) = &m.function.body {
                    hint_scan_stmts(&body.stmts, hints);
                }
            }
            ast::ClassMember::Constructor(c) => {
                if let Some(body) = &c.body {
                    hint_scan_stmts(&body.stmts, hints);
                }
            }
            ast::ClassMember::ClassProp(p) => {
                if let Some(v) = &p.value {
                    hint_walk_expr(v, hints);
                }
            }
            ast::ClassMember::PrivateProp(p) => {
                if let Some(v) = &p.value {
                    hint_walk_expr(v, hints);
                }
            }
            _ => {}
        }
    }
}

fn hint_walk_expr(e: &ast::Expr, hints: &mut std::collections::HashMap<u32, u32>) {
    use ast::Expr as E;
    match e {
        E::Fn(f) => {
            if let Some(body) = &f.function.body {
                hint_scan_stmts(&body.stmts, hints);
            }
        }
        E::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => hint_scan_stmts(&b.stmts, hints),
            ast::BlockStmtOrExpr::Expr(x) => hint_walk_expr(x, hints),
        },
        E::Class(c) => hint_walk_class(&c.class, hints),
        E::Paren(p) => hint_walk_expr(&p.expr, hints),
        E::Seq(s) => {
            for x in &s.exprs {
                hint_walk_expr(x, hints);
            }
        }
        E::Cond(c) => {
            hint_walk_expr(&c.test, hints);
            hint_walk_expr(&c.cons, hints);
            hint_walk_expr(&c.alt, hints);
        }
        E::Bin(b) => {
            hint_walk_expr(&b.left, hints);
            hint_walk_expr(&b.right, hints);
        }
        E::Unary(u) => hint_walk_expr(&u.arg, hints),
        E::Assign(a) => hint_walk_expr(&a.right, hints),
        E::Await(a) => hint_walk_expr(&a.arg, hints),
        E::Call(c) => {
            for arg in &c.args {
                hint_walk_expr(&arg.expr, hints);
            }
        }
        E::New(n) => {
            if let Some(args) = &n.args {
                for arg in args {
                    hint_walk_expr(&arg.expr, hints);
                }
            }
        }
        E::Array(arr) => {
            for el in arr.elems.iter().flatten() {
                hint_walk_expr(&el.expr, hints);
            }
        }
        E::Object(o) => {
            for prop in &o.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        hint_walk_expr(&kv.value, hints);
                    }
                }
            }
        }
        E::Tpl(t) => {
            for x in &t.exprs {
                hint_walk_expr(x, hints);
            }
        }
        E::Member(m) => hint_walk_expr(&m.obj, hints),
        _ => {}
    }
}

/// Cap mirroring the runtime's `LEARNED_INLINE_MAX_FIELDS`: hints past this
/// stop paying for themselves and a pathological constant loop must not
/// inflate every instance.
const WIDTH_HINT_MAX: u32 = 64;

fn note_hint_pair(a: &ast::Stmt, b: &ast::Stmt, hints: &mut std::collections::HashMap<u32, u32>) {
    let ast::Stmt::Decl(ast::Decl::Var(var)) = a else {
        return;
    };
    let Some((name, span_lo)) = empty_object_decl(var) else {
        return;
    };
    note_hint_for_site(&name, span_lo, b, hints);
}

fn note_hint_for_site(
    name: &str,
    literal_span_lo: u32,
    build_loop: &ast::Stmt,
    hints: &mut std::collections::HashMap<u32, u32>,
) {
    let Some(width) = const_build_loop_width(build_loop, name) else {
        return;
    };
    if width == 0 || width > WIDTH_HINT_MAX {
        return;
    }
    hints.insert(literal_span_lo, width);
}

/// `for (let k = C0; k < C1; k++) body` where every body statement is a
/// plain `name.x = value` / `name[expr] = value` assignment. Returns writes
/// per iteration × trip count. Values and key expressions are arbitrary —
/// the width is capacity only.
fn const_build_loop_width(s: &ast::Stmt, name: &str) -> Option<u32> {
    let ast::Stmt::For(f) = s else {
        return None;
    };
    let Some(ast::VarDeclOrExpr::VarDecl(vd)) = &f.init else {
        return None;
    };
    if vd.decls.len() != 1 {
        return None;
    }
    let d0 = &vd.decls[0];
    let ast::Pat::Ident(kb) = &d0.name else {
        return None;
    };
    let counter = kb.id.sym.as_ref();
    let c0 = width_int_lit(d0.init.as_deref()?)?;
    let ast::Expr::Bin(cmp) = f.test.as_deref()? else {
        return None;
    };
    if cmp.op != ast::BinaryOp::Lt {
        return None;
    }
    let ast::Expr::Ident(ci) = &*cmp.left else {
        return None;
    };
    if ci.sym.as_ref() != counter {
        return None;
    }
    let c1 = width_int_lit(&cmp.right)?;
    match f.update.as_deref()? {
        ast::Expr::Update(u) if u.op == ast::UpdateOp::PlusPlus => {
            let ast::Expr::Ident(ui) = &*u.arg else {
                return None;
            };
            if ui.sym.as_ref() != counter {
                return None;
            }
        }
        _ => return None,
    }
    if c1 <= c0 {
        return None;
    }
    let trips = u32::try_from(c1 - c0).ok()?;
    let body: &[ast::Stmt] = match &*f.body {
        ast::Stmt::Block(bs) => &bs.stmts,
        other => std::slice::from_ref(other),
    };
    if body.is_empty() || body.len() > 4 {
        return None;
    }
    let mut writes = 0u32;
    for stmt in body {
        let ast::Stmt::Expr(es) = stmt else {
            return None;
        };
        let ast::Expr::Assign(assign) = &*es.expr else {
            return None;
        };
        if assign.op != ast::AssignOp::Assign {
            return None;
        }
        let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) = &assign.left else {
            return None;
        };
        let ast::Expr::Ident(oi) = &*m.obj else {
            return None;
        };
        if oi.sym.as_ref() != name {
            return None;
        }
        if matches!(&m.prop, ast::MemberProp::PrivateName(_)) {
            return None;
        }
        writes += 1;
    }
    trips.checked_mul(writes)
}

fn width_int_lit(e: &ast::Expr) -> Option<i64> {
    let ast::Expr::Lit(ast::Lit::Num(n)) = e else {
        return None;
    };
    if n.value.fract() != 0.0 || !(0.0..=1_000_000_000.0).contains(&n.value) {
        return None;
    }
    Some(n.value as i64)
}
