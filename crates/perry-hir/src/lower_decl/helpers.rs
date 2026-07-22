//! Declaration lowering.
//!
//! Contains functions for lowering function declarations, class declarations,
//! enum declarations, interface declarations, type alias declarations,
//! constructors, class methods, getters, setters, and class properties.

use crate::types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower::{throw_reference_error_expr, LoweringContext};
use crate::lower_types::*;
use crate::walker::walk_expr_children_mut;

fn strip_for_await_expr_wrappers(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            ast::Expr::Paren(x) => &x.expr,
            _ => return expr,
        };
    }
}

pub(super) fn is_filehandle_readlines_for_await_target(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> bool {
    matches!(
        infer_type_from_expr(strip_for_await_expr_wrappers(expr), ctx),
        Type::Named(name) if name == FILEHANDLE_READLINES_ITERATOR_TYPE
    )
}

pub(super) fn async_iterator_method_call(iterable: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::IndexGet {
            object: Box::new(iterable),
            index: Box::new(Expr::SymbolFor(Box::new(Expr::String(
                "@@__perry_wk_asyncIterator".to_string(),
            )))),
        }),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    }
}

/// Build `if (param === undefined) { param = default; }` stmts for every
/// param with a default value. Prepended to function/constructor bodies so
/// cross-module callers that pad missing args with `undefined` still observe
/// the intended default. Rest params are skipped (they're handled by the
/// call-site array bundling, not by scalar default substitution).
/// Recognise `WebAssembly.instantiate(...)` call shapes used as a var-decl
/// initializer. Used to populate `ctx.wasm_instance_locals` so the
/// standard `inst.exports.<method>(...)` syntactic match in
/// `lower/expr_call.rs` doesn't fire on unrelated `obj.exports.method()`
/// calls (notably CJS aggregator output). Issue #76.
/// Collect local IDs declared anywhere inside this statement tree (Let
/// statements, for-init Lets, catch-clause variables, etc.) — but do NOT
/// recurse into nested closures, since those introduce their own scope.
///
/// Used by the closure capture detector to filter out ids that are
/// shadowed by inner declarations. See the dayjs-#920 comment in the
/// closure-lowering site for context.
pub fn collect_let_decls_in_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_let_decls_in_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_let_decls_in_stmt(body, out),
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init {
                collect_let_decls_in_stmt(init_stmt, out);
            }
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_let_decls_in_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                if let Some((id, _)) = catch_clause.param {
                    out.insert(id);
                }
                for s in &catch_clause.body {
                    collect_let_decls_in_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_let_decls_in_stmt(s, out);
                }
            }
        }
        // Other forms don't introduce new bindings or only have expression payloads.
        _ => {}
    }
}

pub fn init_is_webassembly_instantiate(expr: &ast::Expr) -> bool {
    let call = match expr {
        ast::Expr::Call(c) => c,
        ast::Expr::Await(a) => return init_is_webassembly_instantiate(&a.arg),
        _ => return false,
    };
    let callee = match &call.callee {
        ast::Callee::Expr(e) => e.as_ref(),
        _ => return false,
    };
    let member = match callee {
        ast::Expr::Member(m) => m,
        _ => return false,
    };
    let obj = match member.obj.as_ref() {
        ast::Expr::Ident(i) => i,
        _ => return false,
    };
    let prop = match &member.prop {
        ast::MemberProp::Ident(i) => i,
        _ => return false,
    };
    obj.sym.as_ref() == "WebAssembly" && prop.sym.as_ref() == "instantiate"
}

pub fn build_default_param_stmts(params: &[Param]) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::new();
    for (idx, param) in params.iter().enumerate() {
        if param.is_rest {
            continue;
        }
        let Some(default_expr) = param.default.as_ref() else {
            continue;
        };
        let tdz_param_ids: std::collections::HashSet<LocalId> = params[idx..]
            .iter()
            .filter(|p| p.arguments_object.is_none())
            .map(|p| p.id)
            .collect();
        let default_is_throw_helper = matches!(
            default_expr,
            Expr::Call { callee, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::ExternFuncRef { name, .. } if name.starts_with("js_throw_")
                )
        );
        let default_is_eval_syntax_error = matches!(
            default_expr,
            Expr::SyntaxErrorNew(msg)
                if matches!(
                    msg.as_ref(),
                    Expr::String(message)
                        if message.starts_with("eval var declaration conflicts with")
                )
        );
        out.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(param.id)),
                right: Box::new(Expr::Undefined),
            },
            then_branch: if default_is_throw_helper || default_is_eval_syntax_error {
                vec![Stmt::Throw(default_expr.clone())]
            } else {
                let mut default_value = default_expr.clone();
                rewrite_default_param_tdz_refs(&mut default_value, &tdz_param_ids);
                vec![Stmt::Expr(Expr::LocalSet(
                    param.id,
                    Box::new(default_value),
                ))]
            },
            else_branch: None,
        });
    }
    out
}

fn rewrite_default_param_tdz_refs(
    expr: &mut Expr,
    tdz_param_ids: &std::collections::HashSet<LocalId>,
) {
    match expr {
        Expr::LocalGet(id) if tdz_param_ids.contains(id) => {
            *expr = throw_reference_error_expr("js_throw_reference_error_unresolved_get");
            return;
        }
        Expr::LocalSet(id, value) if tdz_param_ids.contains(id) => {
            rewrite_default_param_tdz_refs(value, tdz_param_ids);
            let value = std::mem::replace(value.as_mut(), Expr::Undefined);
            *expr = Expr::Sequence(vec![
                value,
                throw_reference_error_expr("js_throw_reference_error_unresolved_assignment"),
            ]);
            return;
        }
        Expr::Update { id, .. } | Expr::ArrayPop(id) | Expr::ArrayShift(id)
            if tdz_param_ids.contains(id) =>
        {
            *expr = throw_reference_error_expr("js_throw_reference_error_unresolved_get");
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. }
            if tdz_param_ids.contains(array_id) =>
        {
            *expr = throw_reference_error_expr("js_throw_reference_error_unresolved_get");
            return;
        }
        Expr::SetAdd { set_id, .. } if tdz_param_ids.contains(set_id) => {
            *expr = throw_reference_error_expr("js_throw_reference_error_unresolved_get");
            return;
        }
        Expr::Closure { .. } => return,
        _ => {}
    }

    walk_expr_children_mut(expr, &mut |child| {
        rewrite_default_param_tdz_refs(child, tdz_param_ids)
    });
}

/// Detect the computed key `[Symbol.iterator]` in a class method / object
/// literal. Recognizes the standard `Symbol.iterator` form — doesn't try to
/// evaluate arbitrary expressions, which is enough for `*[Symbol.iterator]()`
/// as emitted by SWC for user code.
pub fn is_symbol_iterator_key(expr: &ast::Expr) -> bool {
    if let ast::Expr::Member(member) = expr {
        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
            (member.obj.as_ref(), &member.prop)
        {
            return obj.sym.as_ref() == "Symbol" && prop.sym.as_ref() == "iterator";
        }
    }
    false
}

/// Detect the computed key `[util.inspect.custom]` / `[inspect.custom]` in a
/// class method / object literal. Mirrors the AST pattern recognised by
/// `lower_member_expression` in `expr_member.rs` so the same `inspect.custom`
/// pattern that becomes `Symbol.for("nodejs.util.inspect.custom")` as a value
/// is detected as a method key too. Refs #1248.
pub fn is_inspect_custom_key(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let member = match expr {
        ast::Expr::Member(m) => m,
        _ => return false,
    };
    let prop_ident = match &member.prop {
        ast::MemberProp::Ident(p) => p,
        _ => return false,
    };
    if prop_ident.sym.as_ref() != "custom" {
        return false;
    }
    // Case A: `inspect.custom` where `inspect` is a named import from
    // `node:util`.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if let Some((module_name, Some(method_name))) =
            ctx.lookup_native_module(obj_ident.sym.as_ref())
        {
            if (module_name == "util" || module_name == "node:util") && method_name == "inspect" {
                return true;
            }
        }
    }
    // Case B: `util.inspect.custom` where `util` is a whole-module alias.
    if let ast::Expr::Member(inner) = member.obj.as_ref() {
        if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(inner_prop)) =
            (inner.obj.as_ref(), &inner.prop)
        {
            let obj_name = obj_ident.sym.to_string();
            let is_util_module =
                obj_name == "util" || ctx.lookup_builtin_module_alias(&obj_name) == Some("util");
            if is_util_module && inner_prop.sym.as_ref() == "inspect" {
                return true;
            }
        }
    }
    false
}

/// Detect the computed key `[Symbol.<well-known>]` in a class method (static
/// method, getter, regular method). Returns the short well-known name
/// ("toPrimitive", "hasInstance", "toStringTag", "iterator", "asyncIterator",
/// "dispose", "asyncDispose") if the expression matches `Symbol.X` for a
/// supported well-known.
pub fn symbol_well_known_key(expr: &ast::Expr) -> Option<&'static str> {
    if let ast::Expr::Member(member) = expr {
        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) =
            (member.obj.as_ref(), &member.prop)
        {
            if obj.sym.as_ref() != "Symbol" {
                return None;
            }
            return match prop.sym.as_ref() {
                "toPrimitive" => Some("toPrimitive"),
                "hasInstance" => Some("hasInstance"),
                "toStringTag" => Some("toStringTag"),
                "iterator" => Some("iterator"),
                "asyncIterator" => Some("asyncIterator"),
                "dispose" => Some("dispose"),
                "asyncDispose" => Some("asyncDispose"),
                _ => None,
            };
        }
    }
    None
}

/// Pre-scan a function body to detect references to the `arguments` identifier.
/// Stops descent at nested function declarations and arrow functions, since
/// those have their own `arguments` binding (or, for arrows, inherit the
/// enclosing function's). For our purposes, "uses arguments anywhere in the
/// direct body or nested arrows" is sufficient — nested regular functions
/// shadow with their own arguments object.
pub fn body_uses_arguments(body: &[ast::Stmt]) -> bool {
    for stmt in body {
        if stmt_uses_arguments(stmt) {
            return true;
        }
    }
    false
}

fn stmt_uses_arguments(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::Block(b) => body_uses_arguments(&b.stmts),
        ast::Stmt::Expr(e) => expr_uses_arguments(&e.expr),
        ast::Stmt::Return(r) => r.arg.as_deref().map(expr_uses_arguments).unwrap_or(false),
        ast::Stmt::If(i) => {
            expr_uses_arguments(&i.test)
                || stmt_uses_arguments(&i.cons)
                || i.alt.as_deref().map(stmt_uses_arguments).unwrap_or(false)
        }
        ast::Stmt::While(w) => expr_uses_arguments(&w.test) || stmt_uses_arguments(&w.body),
        ast::Stmt::DoWhile(w) => expr_uses_arguments(&w.test) || stmt_uses_arguments(&w.body),
        ast::Stmt::For(f) => {
            f.test.as_deref().map(expr_uses_arguments).unwrap_or(false)
                || f.update
                    .as_deref()
                    .map(expr_uses_arguments)
                    .unwrap_or(false)
                || stmt_uses_arguments(&f.body)
        }
        ast::Stmt::ForIn(f) => expr_uses_arguments(&f.right) || stmt_uses_arguments(&f.body),
        ast::Stmt::ForOf(f) => expr_uses_arguments(&f.right) || stmt_uses_arguments(&f.body),
        ast::Stmt::Try(t) => {
            body_uses_arguments(&t.block.stmts)
                || t.handler
                    .as_ref()
                    .map(|h| body_uses_arguments(&h.body.stmts))
                    .unwrap_or(false)
                || t.finalizer
                    .as_ref()
                    .map(|f| body_uses_arguments(&f.stmts))
                    .unwrap_or(false)
        }
        ast::Stmt::Switch(s) => {
            expr_uses_arguments(&s.discriminant)
                || s.cases.iter().any(|c| body_uses_arguments(&c.cons))
        }
        ast::Stmt::Decl(ast::Decl::Var(v)) => v
            .decls
            .iter()
            .any(|d| d.init.as_deref().map(expr_uses_arguments).unwrap_or(false)),
        ast::Stmt::Throw(t) => expr_uses_arguments(&t.arg),
        ast::Stmt::Labeled(l) => stmt_uses_arguments(&l.body),
        _ => false,
    }
}

fn expr_uses_arguments(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(i) => i.sym.as_ref() == "arguments",
        ast::Expr::Call(c) => {
            let callee_uses = match &c.callee {
                ast::Callee::Expr(e) => expr_uses_arguments(e),
                _ => false,
            };
            callee_uses || c.args.iter().any(|a| expr_uses_arguments(&a.expr))
        }
        ast::Expr::Member(m) => {
            expr_uses_arguments(&m.obj)
                || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_uses_arguments(&c.expr))
        }
        ast::Expr::Bin(b) => expr_uses_arguments(&b.left) || expr_uses_arguments(&b.right),
        ast::Expr::Unary(u) => expr_uses_arguments(&u.arg),
        ast::Expr::Update(u) => expr_uses_arguments(&u.arg),
        ast::Expr::Cond(c) => {
            expr_uses_arguments(&c.test)
                || expr_uses_arguments(&c.cons)
                || expr_uses_arguments(&c.alt)
        }
        ast::Expr::Assign(a) => {
            assign_target_uses_arguments(&a.left) || expr_uses_arguments(&a.right)
        }
        ast::Expr::Paren(p) => expr_uses_arguments(&p.expr),
        ast::Expr::TsAs(t) => expr_uses_arguments(&t.expr),
        ast::Expr::TsNonNull(t) => expr_uses_arguments(&t.expr),
        ast::Expr::TsTypeAssertion(t) => expr_uses_arguments(&t.expr),
        ast::Expr::Tpl(t) => t.exprs.iter().any(|e| expr_uses_arguments(e)),
        ast::Expr::Array(a) => a.elems.iter().any(|el| {
            el.as_ref()
                .map(|e| expr_uses_arguments(&e.expr))
                .unwrap_or(false)
        }),
        ast::Expr::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(s) => expr_uses_arguments(&s.expr),
            ast::PropOrSpread::Prop(p) => {
                if let ast::Prop::KeyValue(kv) = p.as_ref() {
                    expr_uses_arguments(&kv.value)
                } else {
                    false
                }
            }
        }),
        ast::Expr::New(n) => {
            expr_uses_arguments(&n.callee)
                || n.args
                    .as_ref()
                    .map(|args| args.iter().any(|a| expr_uses_arguments(&a.expr)))
                    .unwrap_or(false)
        }
        // Arrows inherit `arguments` from the enclosing function (per spec).
        // If an inner arrow references `arguments`, the enclosing non-arrow
        // function must synthesize the binding so the arrow's closure
        // capture mechanism can see it via the scope chain.
        ast::Expr::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => body_uses_arguments(&b.stmts),
            ast::BlockStmtOrExpr::Expr(e) => expr_uses_arguments(e),
        },
        // Sequence (comma) expressions: `return n.date = t, n.args = arguments, x`.
        // Minified bundlers (e.g. dayjs) hide `arguments` inside these, so the
        // synthetic-arguments pre-scan must descend through every operand.
        ast::Expr::Seq(s) => s.exprs.iter().any(|e| expr_uses_arguments(e)),
        ast::Expr::Await(a) => expr_uses_arguments(&a.arg),
        ast::Expr::Yield(y) => y.arg.as_deref().map(expr_uses_arguments).unwrap_or(false),
        ast::Expr::OptChain(o) => match &*o.base {
            ast::OptChainBase::Member(m) => {
                expr_uses_arguments(&m.obj)
                    || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_uses_arguments(&c.expr))
            }
            ast::OptChainBase::Call(c) => {
                expr_uses_arguments(&c.callee)
                    || c.args.iter().any(|a| expr_uses_arguments(&a.expr))
            }
        },
        ast::Expr::SuperProp(sp) => {
            matches!(&sp.prop, ast::SuperProp::Computed(c) if expr_uses_arguments(&c.expr))
        }
        ast::Expr::TaggedTpl(t) => {
            expr_uses_arguments(&t.tag) || t.tpl.exprs.iter().any(|e| expr_uses_arguments(e))
        }
        // Don't descend into nested function declarations or function
        // expressions — those have their own `arguments` binding that
        // shadows the enclosing scope.
        _ => false,
    }
}

fn assign_target_uses_arguments(target: &ast::AssignTarget) -> bool {
    match target {
        ast::AssignTarget::Simple(simple) => simple_assign_target_uses_arguments(simple),
        ast::AssignTarget::Pat(pat) => pat_uses_arguments(pat),
    }
}

fn simple_assign_target_uses_arguments(target: &ast::SimpleAssignTarget) -> bool {
    match target {
        ast::SimpleAssignTarget::Ident(i) => i.id.sym.as_ref() == "arguments",
        ast::SimpleAssignTarget::Member(m) => {
            expr_uses_arguments(&m.obj)
                || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_uses_arguments(&c.expr))
        }
        ast::SimpleAssignTarget::Paren(p) => expr_uses_arguments(&p.expr),
        ast::SimpleAssignTarget::TsAs(t) => expr_uses_arguments(&t.expr),
        ast::SimpleAssignTarget::TsNonNull(t) => expr_uses_arguments(&t.expr),
        ast::SimpleAssignTarget::TsTypeAssertion(t) => expr_uses_arguments(&t.expr),
        ast::SimpleAssignTarget::TsSatisfies(t) => expr_uses_arguments(&t.expr),
        _ => false,
    }
}

fn pat_uses_arguments(pat: &ast::AssignTargetPat) -> bool {
    match pat {
        ast::AssignTargetPat::Array(a) => a.elems.iter().flatten().any(binding_pat_uses_arguments),
        ast::AssignTargetPat::Object(o) => o.props.iter().any(|prop| match prop {
            ast::ObjectPatProp::KeyValue(kv) => binding_pat_uses_arguments(&kv.value),
            ast::ObjectPatProp::Assign(a) => {
                a.key.sym.as_ref() == "arguments"
                    || a.value.as_deref().map(expr_uses_arguments).unwrap_or(false)
            }
            ast::ObjectPatProp::Rest(r) => binding_pat_uses_arguments(&r.arg),
        }),
        ast::AssignTargetPat::Invalid(_) => false,
    }
}

fn binding_pat_uses_arguments(pat: &ast::Pat) -> bool {
    match pat {
        ast::Pat::Ident(i) => i.id.sym.as_ref() == "arguments",
        ast::Pat::Array(a) => a.elems.iter().flatten().any(binding_pat_uses_arguments),
        ast::Pat::Rest(r) => binding_pat_uses_arguments(&r.arg),
        ast::Pat::Object(o) => o.props.iter().any(|prop| match prop {
            ast::ObjectPatProp::KeyValue(kv) => binding_pat_uses_arguments(&kv.value),
            ast::ObjectPatProp::Assign(a) => {
                a.key.sym.as_ref() == "arguments"
                    || a.value.as_deref().map(expr_uses_arguments).unwrap_or(false)
            }
            ast::ObjectPatProp::Rest(r) => binding_pat_uses_arguments(&r.arg),
        }),
        ast::Pat::Assign(a) => binding_pat_uses_arguments(&a.left) || expr_uses_arguments(&a.right),
        ast::Pat::Expr(e) => expr_uses_arguments(e),
        ast::Pat::Invalid(_) => false,
    }
}

pub fn body_has_use_strict(body: &[ast::Stmt]) -> bool {
    for stmt in body {
        let Some(directive) = crate::lower::string_directive_stmt_lit(stmt) else {
            return false;
        };
        if crate::lower::is_raw_use_strict_directive(directive) {
            return true;
        }
    }
    false
}

pub fn params_are_simple_arguments_list(params: &[ast::Param]) -> bool {
    params.iter().all(|param| match &param.pat {
        ast::Pat::Ident(ident) => ident.id.sym.as_ref() == "this" || !ident.id.sym.is_empty(),
        _ => false,
    })
}

pub fn mapped_argument_parameter_ids(params: &[Param]) -> Vec<(u32, LocalId)> {
    let mut seen = std::collections::HashSet::new();
    let mut mapped = Vec::new();
    for (idx, param) in params.iter().enumerate().rev() {
        if param.is_rest || param.arguments_object.is_some() {
            continue;
        }
        if seen.insert(param.name.clone()) {
            mapped.push((idx as u32, param.id));
        }
    }
    mapped
}

/// True when any parameter's DEFAULT expression references `arguments`
/// (`method(x = arguments[2], y) {}`). Parameter defaults evaluate in the
/// function's own scope, so they see the same arguments object as the body —
/// the synthetic-arguments check must include them (test262
/// class/params-dflt-meth-ref-arguments).
pub fn params_use_arguments(params: &[ast::Param]) -> bool {
    params.iter().any(|p| param_pat_uses_arguments(&p.pat))
}

fn param_pat_uses_arguments(pat: &ast::Pat) -> bool {
    match pat {
        ast::Pat::Assign(a) => expr_uses_arguments(&a.right) || param_pat_uses_arguments(&a.left),
        ast::Pat::Array(arr) => arr.elems.iter().flatten().any(param_pat_uses_arguments),
        ast::Pat::Object(obj) => obj.props.iter().any(|p| match p {
            ast::ObjectPatProp::Assign(a) => {
                a.value.as_deref().map(expr_uses_arguments).unwrap_or(false)
            }
            ast::ObjectPatProp::KeyValue(kv) => param_pat_uses_arguments(&kv.value),
            ast::ObjectPatProp::Rest(r) => param_pat_uses_arguments(&r.arg),
        }),
        ast::Pat::Rest(r) => param_pat_uses_arguments(&r.arg),
        _ => false,
    }
}

/// Synthesize a hidden raw-arguments parameter. Call after lowering the user's
/// parameters, before lowering the body, when the body references `arguments`
/// and the user hasn't already bound it explicitly.
pub fn append_synthetic_arguments_param(
    ctx: &mut LoweringContext,
    params: &mut Vec<Param>,
    strict: bool,
    simple_parameters: bool,
    restricted_callee: bool,
    mapped_parameter_ids: Vec<(u32, LocalId)>,
) {
    let arguments_id = ctx.define_local("arguments".to_string(), Type::Any);
    let has_user_rest = params.iter().any(|p| p.is_rest);
    params.push(Param {
        id: arguments_id,
        name: "arguments".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: !has_user_rest,
        arguments_object: Some(ArgumentsObjectMeta {
            strict,
            simple_parameters,
            mapped_parameter_ids,
            restricted_callee,
        }),
    });
}

/// Scope a member-lowering closure with `ctx.current_class_member_is_static`
/// set for the member's staticness (restored afterwards).
pub(crate) fn with_static_member_context<T>(
    ctx: &mut LoweringContext,
    is_static: bool,
    f: impl FnOnce(&mut LoweringContext) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let old = ctx.current_class_member_is_static;
    ctx.current_class_member_is_static = is_static;
    let result = f(ctx);
    ctx.current_class_member_is_static = old;
    result
}

/// Outcome of `lower_well_known_computed_method` for a class method whose
/// computed key is a well-known symbol (`[Symbol.asyncIterator]() {}`,
/// `static [Symbol.hasInstance]() {}`, `get [Symbol.toStringTag]() {}`, …).
pub(crate) enum WellKnownComputedMethod {
    /// Register the method under this synthetic registration name
    /// (`@@asyncIterator`, `@@toPrimitive`, `__perry_dispose__`, …) via the
    /// caller's regular method-pushing path. Never source-order registered.
    Rename(String),
    /// Fully handled here — the method body was lifted to a pending
    /// top-level function (`__perry_wk_hasinstance_<class>` /
    /// `__perry_wk_tostringtag_<class>`); the caller skips the member.
    Lifted,
    /// A recognized well-known symbol in a form perry doesn't implement
    /// yet (e.g. a static `[Symbol.toPrimitive]`) — the caller drops the
    /// member, preserving the historical behavior.
    Unsupported,
}

/// Shared lowering for class methods keyed by a well-known symbol. Both the
/// class-declaration path (`lower_class_decl`) and the class-expression path
/// (`lower_class_from_ast`) call this from their computed-key match arm, so
/// `const C = class { [Symbol.asyncIterator]() {…} }` registers the same
/// vtable entries as the equivalent class declaration. Pre-fix the
/// expression path fell through `_ => continue` and silently dropped every
/// well-known-symbol method except `@@iterator`, so `for await (… of
/// new C())` threw `TypeError: value is not iterable` (the SDK-style
/// `oV = class { [Symbol.asyncIterator]() { return this.iterator() } }`
/// stream wrapper in a large esbuild-bundled CLI app is the canonical case).
///
/// Returns `Ok(None)` when the method's key is not a well-known-symbol
/// computed key at all.
pub(crate) fn lower_well_known_computed_method(
    ctx: &mut LoweringContext,
    method: &ast::ClassMethod,
    class_name: &str,
) -> anyhow::Result<Option<WellKnownComputedMethod>> {
    let ast::PropName::Computed(computed) = &method.key else {
        return Ok(None);
    };
    let Some(wk) = symbol_well_known_key(&computed.expr) else {
        return Ok(None);
    };
    // hasInstance (static method): lift the method body to a top-level
    // function named `__perry_wk_hasinstance_<class>`. Signature:
    // `(value: f64) -> f64` — no `this`.
    if wk == "hasInstance" && method.is_static && matches!(method.kind, ast::MethodKind::Method) {
        let mut func = with_static_member_context(ctx, method.is_static, |ctx| {
            super::lower_class_method(ctx, method)
        })?;
        func.name = format!("__perry_wk_hasinstance_{}", class_name);
        ctx.pending_functions.push(func);
        return Ok(Some(WellKnownComputedMethod::Lifted));
    }
    // toStringTag (instance getter): lift the getter body to a top-level
    // function named `__perry_wk_tostringtag_<class>`. Signature:
    // `(this: f64) -> f64` — getter takes `this` as an explicit first
    // parameter and returns a string.
    if wk == "toStringTag" && !method.is_static && matches!(method.kind, ast::MethodKind::Getter) {
        let getter = with_static_member_context(ctx, method.is_static, |ctx| {
            super::lower_getter_method(ctx, method)
        })?;
        // Inject a `this` parameter at position 0 and rewrite any
        // `Expr::This` in the body to `LocalGet(this_id)`.
        let this_id = ctx.fresh_local();
        let mut new_params = Vec::with_capacity(getter.params.len() + 1);
        new_params.push(Param {
            id: this_id,
            name: "this".to_string(),
            ty: Type::Named(class_name.to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        });
        new_params.extend(getter.params);
        let mut body = getter.body;
        crate::analysis::replace_this_in_stmts(&mut body, this_id);
        let top_fn = Function {
            id: ctx.fresh_func(),
            name: format!("__perry_wk_tostringtag_{}", class_name),
            type_params: Vec::new(),
            params: new_params,
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            was_plain_async: false,
            was_unrolled: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
        };
        ctx.pending_functions.push(top_fn);
        return Ok(Some(WellKnownComputedMethod::Lifted));
    }
    // `[Symbol.dispose]()` / `[Symbol.asyncDispose]()`: ES2024
    // explicit-resource-management dispose hooks. Rename the method to a
    // stable string-keyed name so the using-block desugarer can call it via
    // plain method dispatch (`obj.__perry_dispose__()` /
    // `obj.__perry_async_dispose__()`). Flows through the caller's regular
    // method-pushing path with the renamed key.
    if (wk == "dispose" || wk == "asyncDispose")
        && !method.is_static
        && matches!(method.kind, ast::MethodKind::Method)
    {
        let renamed = if wk == "asyncDispose" {
            "__perry_async_dispose__".to_string()
        } else {
            "__perry_dispose__".to_string()
        };
        return Ok(Some(WellKnownComputedMethod::Rename(renamed)));
    }
    // #1838 follow-up: `[Symbol.asyncIterator]() {}` on a class — register
    // under `@@asyncIterator` so the symbol resolver in `runtime/src/symbol.rs`
    // (`well_known_symbol_method_key`) binds it as
    // `instance[Symbol.asyncIterator]`. Mirrors the `@@iterator` path;
    // for-await over a class instance picks the same vtable entry.
    if wk == "asyncIterator" && !method.is_static && matches!(method.kind, ast::MethodKind::Method)
    {
        return Ok(Some(WellKnownComputedMethod::Rename(
            "@@asyncIterator".to_string(),
        )));
    }
    // #2374: `[Symbol.toPrimitive](hint) {}` on a class — register under
    // `@@toPrimitive` so the symbol resolver in `runtime/src/symbol.rs`
    // (`well_known_symbol_method_key`) binds it as
    // `instance[Symbol.toPrimitive]`. The runtime's ToPrimitive
    // (`js_to_primitive`, consulted by unary `+` numeric coercion and
    // template/`String()` string coercion) then invokes it with the
    // appropriate hint before falling back to `valueOf`/`toString`.
    if wk == "toPrimitive" && !method.is_static && matches!(method.kind, ast::MethodKind::Method) {
        return Ok(Some(WellKnownComputedMethod::Rename(
            "@@toPrimitive".to_string(),
        )));
    }
    // Other well-known on a class: not yet implemented — drop.
    Ok(Some(WellKnownComputedMethod::Unsupported))
}
