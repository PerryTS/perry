//! Additional early error rules for eval code (spec: PerformEval).
//!
//! Direct eval applies extra early errors to its parsed ScriptBody based on
//! the lexical context of the eval call site:
//!
//! - `ScriptBody Contains SuperCall` is a SyntaxError unless the eval occurs
//!   directly inside the constructor of a derived class.
//! - `ScriptBody Contains SuperProperty` is a SyntaxError unless the eval
//!   occurs inside a class member (method / constructor / field initializer)
//!   or an object-literal method (anywhere with a [[HomeObject]]).
//! - `AllPrivateIdentifiersValid`: every `#name` referenced by the eval code
//!   must be declared by an enclosing class (of the call site, lexically) or
//!   by a class inside the eval code itself.
//!
//! Indirect eval is global code: any SuperCall / SuperProperty is a
//! SyntaxError, and private names are only valid when declared by a class
//! inside the eval source.
//!
//! The errors are *runtime* SyntaxErrors thrown when the eval call evaluates
//! (the surrounding script must still compile and run up to that point), so
//! the check emits a `js_throw_eval_syntax_error` call expression rather than
//! failing the build. `Contains` semantics: arrow functions are transparent,
//! ordinary function bodies / class member bodies / object-literal method
//! bodies are opaque (their `super` belongs to their own home object).
//! Private-name collection ignores those boundaries — private names are
//! lexically scoped through nested functions.

use std::collections::HashSet;

use perry_types::Type;
use swc_ecma_ast as ast;

use super::LoweringContext;
use crate::ir::Expr;

#[derive(Default)]
struct Scan {
    super_call: bool,
    super_prop: bool,
    /// `arguments` referenced in the eval's own context (arrows transparent,
    /// function bodies opaque — a nested plain function has its own).
    arguments_ref: bool,
    /// `new.target` referenced in the eval's own context (same transparency).
    new_target: bool,
    /// Referenced private names, `#name` form.
    private_refs: Vec<String>,
    /// Private names declared by class bodies inside the eval source.
    private_decls: HashSet<String>,
}

/// The "super is illegal here" throw, exposed for the indirect-eval
/// parse-failure path in `const_fold_fn.rs`.
pub(crate) fn throw_eval_super_unexpected_expr() -> Expr {
    throw_eval_syntax_error_expr("'super' keyword unexpected here")
}

/// Scan parsed eval-body statements for an abrupt completion that is illegal at
/// the top level of eval code: an unlabeled `break`/`continue` with no enclosing
/// iteration (or `switch`, for `break`) inside the eval source, or a `return`
/// anywhere outside a contained function. Both are *parse-level* SyntaxErrors in
/// the spec (the eval source is parsed for the goal symbol `Script`), thrown
/// when the eval call evaluates.
///
/// SWC's error-recovery parser accepts `break;`/`continue;`/`return;` at script
/// top level (emitting only a recoverable warning), so they survive into the
/// HIR fold and either crash a downstream closure-lowering pass (`break`/
/// `continue`) or run silently (`return`). Detecting them here lets the eval
/// fold emit the spec-faithful runtime `SyntaxError` instead. Returns the throw
/// expression on the first violation found. (test262 language/eval-code
/// {direct,indirect}/parse-failure-{3,4,5})
pub(crate) fn check_eval_illegal_abrupt(stmts: &[ast::Stmt]) -> Option<Expr> {
    fn walk(stmt: &ast::Stmt, loop_depth: u32, switch_depth: u32) -> Option<&'static str> {
        use ast::Stmt as S;
        match stmt {
            // A `return` is never legal at eval top level (eval code is Script,
            // not a function body). Labeled `break`/`continue` are assumed to
            // target an enclosing labeled statement in the body and are left
            // alone to avoid false positives.
            S::Return(_) => Some("'return' statement is not allowed here"),
            S::Break(b) => {
                if b.label.is_none() && loop_depth == 0 && switch_depth == 0 {
                    Some("Illegal break statement")
                } else {
                    None
                }
            }
            S::Continue(c) => {
                if c.label.is_none() && loop_depth == 0 {
                    Some("Illegal continue statement")
                } else {
                    None
                }
            }
            S::Block(blk) => walk_list(&blk.stmts, loop_depth, switch_depth),
            S::If(i) => walk(&i.cons, loop_depth, switch_depth).or_else(|| {
                i.alt
                    .as_deref()
                    .and_then(|a| walk(a, loop_depth, switch_depth))
            }),
            S::Labeled(l) => walk(&l.body, loop_depth, switch_depth),
            S::With(w) => walk(&w.body, loop_depth, switch_depth),
            S::Try(t) => walk_list(&t.block.stmts, loop_depth, switch_depth)
                .or_else(|| {
                    t.handler
                        .as_ref()
                        .and_then(|h| walk_list(&h.body.stmts, loop_depth, switch_depth))
                })
                .or_else(|| {
                    t.finalizer
                        .as_ref()
                        .and_then(|f| walk_list(&f.stmts, loop_depth, switch_depth))
                }),
            // Iteration bodies admit `break`/`continue`.
            S::While(s) => walk(&s.body, loop_depth + 1, switch_depth),
            S::DoWhile(s) => walk(&s.body, loop_depth + 1, switch_depth),
            S::For(s) => walk(&s.body, loop_depth + 1, switch_depth),
            S::ForIn(s) => walk(&s.body, loop_depth + 1, switch_depth),
            S::ForOf(s) => walk(&s.body, loop_depth + 1, switch_depth),
            // `switch` admits `break` (but not `continue`).
            S::Switch(sw) => sw
                .cases
                .iter()
                .find_map(|case| walk_list(&case.cons, loop_depth, switch_depth + 1)),
            // Function / class bodies own their own abrupt completions — opaque.
            _ => None,
        }
    }
    fn walk_list(stmts: &[ast::Stmt], loop_depth: u32, switch_depth: u32) -> Option<&'static str> {
        stmts.iter().find_map(|s| walk(s, loop_depth, switch_depth))
    }
    walk_list(stmts, 0, 0).map(throw_eval_syntax_error_expr)
}

/// Public wrapper around the eval-`SyntaxError` throw expression, for the
/// general indirect-eval fold's parse-failure / illegal-statement paths.
pub(crate) fn throw_eval_syntax_error_public(msg: &str) -> Expr {
    throw_eval_syntax_error_expr(msg)
}

fn throw_eval_syntax_error_expr(msg: &str) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_eval_syntax_error".to_string(),
            param_types: vec![Type::String],
            return_type: Type::Any,
        }),
        args: vec![Expr::String(msg.to_string())],
        type_args: vec![],
        byte_offset: 0,
    }
}

/// Direct eval: check the parsed body against the call site's capabilities.
/// Returns the throw expression to substitute for the eval call on violation.
pub(crate) fn check_direct_eval_super_private(
    ctx: &LoweringContext,
    stmts: &[ast::Stmt],
) -> Option<Expr> {
    let mut scan = Scan::default();
    for s in stmts {
        scan_stmt(&mut scan, s, true);
    }
    // SuperCall capability: directly inside a derived-class constructor.
    let super_call_ok = ctx.in_constructor_class.is_some() && ctx.current_class_is_derived;
    // SuperProperty capability: inside any class member or object method.
    let super_prop_ok = ctx.current_class.is_some()
        || ctx.in_constructor_class.is_some()
        || !ctx.object_super_home_stack.is_empty();
    if scan.super_call && !super_call_ok {
        return Some(throw_eval_syntax_error_expr(
            "'super' keyword unexpected here",
        ));
    }
    if scan.super_prop && !super_prop_ok {
        return Some(throw_eval_syntax_error_expr(
            "'super' keyword unexpected here",
        ));
    }
    // Class field initializers have no arguments object — `arguments` in a
    // direct eval body there is a SyntaxError at the eval call.
    if scan.arguments_ref && ctx.in_class_field_init {
        return Some(throw_eval_syntax_error_expr(
            "'arguments' is not allowed in class field initializer",
        ));
    }
    // ES2025 §15.2.1.1: `new.target` in a direct eval body is only legal when
    // the eval is contained in function code that is NOT an ArrowFunction.
    // At module/script top-level and inside arrow functions, it is a SyntaxError.
    if scan.new_target && !ctx.in_nonarrow_fn {
        return Some(throw_eval_syntax_error_expr(
            "new.target expression is not allowed here",
        ));
    }
    check_private_refs(&scan, |name| {
        ctx.private_scopes
            .iter()
            .any(|s| s.members.contains_key(name))
    })
}

/// Indirect eval: global code — no super of either kind, and private names
/// must be declared inside the eval source itself.
pub(crate) fn check_indirect_eval_super_private(stmts: &[ast::Stmt]) -> Option<Expr> {
    let mut scan = Scan::default();
    for s in stmts {
        scan_stmt(&mut scan, s, true);
    }
    if scan.super_call || scan.super_prop {
        return Some(throw_eval_syntax_error_expr(
            "'super' keyword unexpected here",
        ));
    }
    // Indirect eval is global code — it has no new.target binding.
    if scan.new_target {
        return Some(throw_eval_syntax_error_expr(
            "new.target expression is not allowed here",
        ));
    }
    check_private_refs(&scan, |_| false)
}

fn check_private_refs(scan: &Scan, outer_declares: impl Fn(&str) -> bool) -> Option<Expr> {
    for name in &scan.private_refs {
        if !scan.private_decls.contains(name) && !outer_declares(name) {
            return Some(throw_eval_syntax_error_expr(&format!(
                "Private field '{name}' must be declared in an enclosing class"
            )));
        }
    }
    None
}

/// `transparent` — whether SuperCall/SuperProperty found here belongs to the
/// eval's own context (arrows keep it; function/method bodies clear it).
/// Private-name refs are collected regardless.
fn scan_stmt(scan: &mut Scan, stmt: &ast::Stmt, transparent: bool) {
    use ast::Stmt as S;
    match stmt {
        S::Block(b) => {
            for s in &b.stmts {
                scan_stmt(scan, s, transparent);
            }
        }
        S::With(w) => {
            scan_expr(scan, &w.obj, transparent);
            scan_stmt(scan, &w.body, transparent);
        }
        S::Return(r) => {
            if let Some(arg) = &r.arg {
                scan_expr(scan, arg, transparent);
            }
        }
        S::Labeled(l) => scan_stmt(scan, &l.body, transparent),
        S::If(i) => {
            scan_expr(scan, &i.test, transparent);
            scan_stmt(scan, &i.cons, transparent);
            if let Some(alt) = &i.alt {
                scan_stmt(scan, alt, transparent);
            }
        }
        S::Switch(sw) => {
            scan_expr(scan, &sw.discriminant, transparent);
            for case in &sw.cases {
                if let Some(test) = &case.test {
                    scan_expr(scan, test, transparent);
                }
                for s in &case.cons {
                    scan_stmt(scan, s, transparent);
                }
            }
        }
        S::Throw(t) => scan_expr(scan, &t.arg, transparent),
        S::Try(t) => {
            for s in &t.block.stmts {
                scan_stmt(scan, s, transparent);
            }
            if let Some(handler) = &t.handler {
                for s in &handler.body.stmts {
                    scan_stmt(scan, s, transparent);
                }
            }
            if let Some(fin) = &t.finalizer {
                for s in &fin.stmts {
                    scan_stmt(scan, s, transparent);
                }
            }
        }
        S::While(w) => {
            scan_expr(scan, &w.test, transparent);
            scan_stmt(scan, &w.body, transparent);
        }
        S::DoWhile(w) => {
            scan_expr(scan, &w.test, transparent);
            scan_stmt(scan, &w.body, transparent);
        }
        S::For(f) => {
            match &f.init {
                Some(ast::VarDeclOrExpr::VarDecl(v)) => scan_var_decl(scan, v, transparent),
                Some(ast::VarDeclOrExpr::Expr(e)) => scan_expr(scan, e, transparent),
                None => {}
            }
            if let Some(test) = &f.test {
                scan_expr(scan, test, transparent);
            }
            if let Some(update) = &f.update {
                scan_expr(scan, update, transparent);
            }
            scan_stmt(scan, &f.body, transparent);
        }
        S::ForIn(f) => {
            if let ast::ForHead::VarDecl(v) = &f.left {
                scan_var_decl(scan, v, transparent);
            }
            scan_expr(scan, &f.right, transparent);
            scan_stmt(scan, &f.body, transparent);
        }
        S::ForOf(f) => {
            if let ast::ForHead::VarDecl(v) = &f.left {
                scan_var_decl(scan, v, transparent);
            }
            scan_expr(scan, &f.right, transparent);
            scan_stmt(scan, &f.body, transparent);
        }
        S::Decl(decl) => scan_decl(scan, decl, transparent),
        S::Expr(e) => scan_expr(scan, &e.expr, transparent),
        S::Empty(_) | S::Debugger(_) | S::Break(_) | S::Continue(_) => {}
    }
}

fn scan_decl(scan: &mut Scan, decl: &ast::Decl, transparent: bool) {
    match decl {
        ast::Decl::Class(class_decl) => scan_class(scan, &class_decl.class, transparent),
        ast::Decl::Fn(fn_decl) => scan_function(scan, &fn_decl.function),
        ast::Decl::Var(v) => scan_var_decl(scan, v, transparent),
        ast::Decl::Using(u) => {
            for d in &u.decls {
                if let Some(init) = &d.init {
                    scan_expr(scan, init, transparent);
                }
            }
        }
        _ => {}
    }
}

fn scan_var_decl(scan: &mut Scan, var: &ast::VarDecl, transparent: bool) {
    for d in &var.decls {
        if let Some(init) = &d.init {
            scan_expr(scan, init, transparent);
        }
    }
}

/// Function bodies are opaque to `Contains` for super; private names still
/// collected.
fn scan_function(scan: &mut Scan, func: &ast::Function) {
    for p in &func.params {
        scan_pat(scan, &p.pat, false);
    }
    if let Some(body) = &func.body {
        for s in &body.stmts {
            scan_stmt(scan, s, false);
        }
    }
}

fn scan_class(scan: &mut Scan, class: &ast::Class, transparent: bool) {
    // Heritage and computed keys evaluate in the outer context.
    if let Some(sc) = &class.super_class {
        scan_expr(scan, sc, transparent);
    }
    for member in &class.body {
        match member {
            ast::ClassMember::Constructor(ctor) => {
                if let Some(body) = &ctor.body {
                    for s in &body.stmts {
                        scan_stmt(scan, s, false);
                    }
                }
            }
            ast::ClassMember::Method(m) => {
                if let ast::PropName::Computed(c) = &m.key {
                    scan_expr(scan, &c.expr, transparent);
                }
                scan_function(scan, &m.function);
            }
            ast::ClassMember::PrivateMethod(m) => {
                scan.private_decls.insert(format!("#{}", m.key.name));
                scan_function(scan, &m.function);
            }
            ast::ClassMember::ClassProp(p) => {
                if let ast::PropName::Computed(c) = &p.key {
                    scan_expr(scan, &c.expr, transparent);
                }
                if let Some(value) = &p.value {
                    scan_expr(scan, value, false);
                }
            }
            ast::ClassMember::PrivateProp(p) => {
                scan.private_decls.insert(format!("#{}", p.key.name));
                if let Some(value) = &p.value {
                    scan_expr(scan, value, false);
                }
            }
            ast::ClassMember::StaticBlock(b) => {
                for s in &b.body.stmts {
                    scan_stmt(scan, s, false);
                }
            }
            _ => {}
        }
    }
}

fn scan_pat(scan: &mut Scan, pat: &ast::Pat, transparent: bool) {
    match pat {
        ast::Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                scan_pat(scan, elem, transparent);
            }
        }
        ast::Pat::Object(obj) => {
            for p in &obj.props {
                match p {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        if let ast::PropName::Computed(c) = &kv.key {
                            scan_expr(scan, &c.expr, transparent);
                        }
                        scan_pat(scan, &kv.value, transparent);
                    }
                    ast::ObjectPatProp::Assign(a) => {
                        if let Some(value) = &a.value {
                            scan_expr(scan, value, transparent);
                        }
                    }
                    ast::ObjectPatProp::Rest(r) => scan_pat(scan, &r.arg, transparent),
                }
            }
        }
        ast::Pat::Assign(a) => {
            scan_pat(scan, &a.left, transparent);
            scan_expr(scan, &a.right, transparent);
        }
        ast::Pat::Rest(r) => scan_pat(scan, &r.arg, transparent),
        ast::Pat::Expr(e) => scan_expr(scan, e, transparent),
        ast::Pat::Ident(_) | ast::Pat::Invalid(_) => {}
    }
}

fn scan_expr(scan: &mut Scan, expr: &ast::Expr, transparent: bool) {
    use ast::Expr as E;
    match expr {
        E::SuperProp(sp) => {
            if transparent {
                scan.super_prop = true;
            }
            if let ast::SuperProp::Computed(c) = &sp.prop {
                scan_expr(scan, &c.expr, transparent);
            }
        }
        E::Call(call) => {
            if matches!(call.callee, ast::Callee::Super(_)) && transparent {
                scan.super_call = true;
            }
            if let ast::Callee::Expr(callee) = &call.callee {
                scan_expr(scan, callee, transparent);
            }
            for arg in &call.args {
                scan_expr(scan, &arg.expr, transparent);
            }
        }
        E::New(new) => {
            scan_expr(scan, &new.callee, transparent);
            if let Some(args) = &new.args {
                for arg in args {
                    scan_expr(scan, &arg.expr, transparent);
                }
            }
        }
        E::Member(m) => {
            scan_expr(scan, &m.obj, transparent);
            match &m.prop {
                ast::MemberProp::PrivateName(p) => {
                    scan.private_refs.push(format!("#{}", p.name));
                }
                ast::MemberProp::Computed(c) => scan_expr(scan, &c.expr, transparent),
                ast::MemberProp::Ident(_) => {}
            }
        }
        E::OptChain(oc) => match oc.base.as_ref() {
            ast::OptChainBase::Member(m) => {
                scan_expr(scan, &m.obj, transparent);
                match &m.prop {
                    ast::MemberProp::PrivateName(p) => {
                        scan.private_refs.push(format!("#{}", p.name));
                    }
                    ast::MemberProp::Computed(c) => scan_expr(scan, &c.expr, transparent),
                    ast::MemberProp::Ident(_) => {}
                }
            }
            ast::OptChainBase::Call(c) => {
                scan_expr(scan, &c.callee, transparent);
                for arg in &c.args {
                    scan_expr(scan, &arg.expr, transparent);
                }
            }
        },
        E::Bin(bin) => {
            // `#x in obj` — the left operand of `in` may be a private name.
            if let E::PrivateName(p) = bin.left.as_ref() {
                scan.private_refs.push(format!("#{}", p.name));
            } else {
                scan_expr(scan, &bin.left, transparent);
            }
            scan_expr(scan, &bin.right, transparent);
        }
        E::PrivateName(p) => {
            scan.private_refs.push(format!("#{}", p.name));
        }
        E::Ident(id) if transparent && id.sym.as_ref() == "arguments" => {
            scan.arguments_ref = true;
        }
        E::MetaProp(mp) if transparent && mp.kind == ast::MetaPropKind::NewTarget => {
            scan.new_target = true;
        }
        E::Unary(u) => scan_expr(scan, &u.arg, transparent),
        E::Update(u) => scan_expr(scan, &u.arg, transparent),
        E::Assign(a) => {
            match &a.left {
                ast::AssignTarget::Simple(simple) => match simple {
                    ast::SimpleAssignTarget::Member(m) => {
                        scan_expr(scan, &m.obj, transparent);
                        match &m.prop {
                            ast::MemberProp::PrivateName(p) => {
                                scan.private_refs.push(format!("#{}", p.name));
                            }
                            ast::MemberProp::Computed(c) => scan_expr(scan, &c.expr, transparent),
                            ast::MemberProp::Ident(_) => {}
                        }
                    }
                    ast::SimpleAssignTarget::SuperProp(sp) => {
                        if transparent {
                            scan.super_prop = true;
                        }
                        if let ast::SuperProp::Computed(c) = &sp.prop {
                            scan_expr(scan, &c.expr, transparent);
                        }
                    }
                    _ => {}
                },
                ast::AssignTarget::Pat(pat) => match pat {
                    ast::AssignTargetPat::Array(arr) => {
                        scan_pat(scan, &ast::Pat::Array(arr.clone()), transparent)
                    }
                    ast::AssignTargetPat::Object(obj) => {
                        scan_pat(scan, &ast::Pat::Object(obj.clone()), transparent)
                    }
                    ast::AssignTargetPat::Invalid(_) => {}
                },
            }
            scan_expr(scan, &a.right, transparent);
        }
        E::Cond(c) => {
            scan_expr(scan, &c.test, transparent);
            scan_expr(scan, &c.cons, transparent);
            scan_expr(scan, &c.alt, transparent);
        }
        E::Seq(s) => {
            for e in &s.exprs {
                scan_expr(scan, e, transparent);
            }
        }
        E::Paren(p) => scan_expr(scan, &p.expr, transparent),
        E::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                scan_expr(scan, &elem.expr, transparent);
            }
        }
        E::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ast::PropOrSpread::Spread(s) => scan_expr(scan, &s.expr, transparent),
                    ast::PropOrSpread::Prop(p) => match p.as_ref() {
                        ast::Prop::KeyValue(kv) => {
                            if let ast::PropName::Computed(c) = &kv.key {
                                scan_expr(scan, &c.expr, transparent);
                            }
                            scan_expr(scan, &kv.value, transparent);
                        }
                        ast::Prop::Assign(a) => scan_expr(scan, &a.value, transparent),
                        // Object methods/accessors have their own home
                        // object — opaque for super, still scanned for
                        // private names.
                        ast::Prop::Getter(g) => {
                            if let ast::PropName::Computed(c) = &g.key {
                                scan_expr(scan, &c.expr, transparent);
                            }
                            if let Some(body) = &g.body {
                                for s in &body.stmts {
                                    scan_stmt(scan, s, false);
                                }
                            }
                        }
                        ast::Prop::Setter(s) => {
                            if let ast::PropName::Computed(c) = &s.key {
                                scan_expr(scan, &c.expr, transparent);
                            }
                            if let Some(body) = &s.body {
                                for st in &body.stmts {
                                    scan_stmt(scan, st, false);
                                }
                            }
                        }
                        ast::Prop::Method(m) => {
                            if let ast::PropName::Computed(c) = &m.key {
                                scan_expr(scan, &c.expr, transparent);
                            }
                            scan_function(scan, &m.function);
                        }
                        ast::Prop::Shorthand(_) => {}
                    },
                }
            }
        }
        E::Fn(fn_expr) => scan_function(scan, &fn_expr.function),
        E::Arrow(arrow) => {
            for p in &arrow.params {
                scan_pat(scan, p, transparent);
            }
            match arrow.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    for s in &b.stmts {
                        scan_stmt(scan, s, transparent);
                    }
                }
                ast::BlockStmtOrExpr::Expr(e) => scan_expr(scan, e, transparent),
            }
        }
        E::Class(class_expr) => scan_class(scan, &class_expr.class, transparent),
        E::Tpl(tpl) => {
            for e in &tpl.exprs {
                scan_expr(scan, e, transparent);
            }
        }
        E::TaggedTpl(tt) => {
            scan_expr(scan, &tt.tag, transparent);
            for e in &tt.tpl.exprs {
                scan_expr(scan, e, transparent);
            }
        }
        E::Yield(y) => {
            if let Some(arg) = &y.arg {
                scan_expr(scan, arg, transparent);
            }
        }
        E::Await(a) => scan_expr(scan, &a.arg, transparent),
        E::TsAs(t) => scan_expr(scan, &t.expr, transparent),
        E::TsNonNull(t) => scan_expr(scan, &t.expr, transparent),
        E::TsTypeAssertion(t) => scan_expr(scan, &t.expr, transparent),
        E::TsConstAssertion(t) => scan_expr(scan, &t.expr, transparent),
        E::TsSatisfies(t) => scan_expr(scan, &t.expr, transparent),
        _ => {}
    }
}
