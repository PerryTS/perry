//! Declaration lowering.
//!
//! Contains functions for lowering function declarations, class declarations,
//! enum declarations, interface declarations, type alias declarations,
//! constructors, class methods, getters, setters, and class properties.

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
    for param in params {
        if param.is_rest {
            continue;
        }
        let Some(default_expr) = param.default.as_ref() else {
            continue;
        };
        out.push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(param.id)),
                right: Box::new(Expr::Undefined),
            },
            then_branch: vec![Stmt::Expr(Expr::LocalSet(
                param.id,
                Box::new(default_expr.clone()),
            ))],
            else_branch: None,
        });
    }
    out
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
        ast::Expr::Assign(a) => expr_uses_arguments(&a.right),
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
        ast::Expr::New(n) => n
            .args
            .as_ref()
            .map(|args| args.iter().any(|a| expr_uses_arguments(&a.expr)))
            .unwrap_or(false),
        // Arrows inherit `arguments` from the enclosing function (per spec).
        // If an inner arrow references `arguments`, the enclosing non-arrow
        // function must synthesize the binding so the arrow's closure
        // capture mechanism can see it via the scope chain.
        ast::Expr::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => body_uses_arguments(&b.stmts),
            ast::BlockStmtOrExpr::Expr(e) => expr_uses_arguments(e),
        },
        // Don't descend into nested function declarations or function
        // expressions — those have their own `arguments` binding that
        // shadows the enclosing scope.
        _ => false,
    }
}

/// Synthesize a trailing `...arguments` rest parameter. Call after lowering
/// the user's parameters, before lowering the body, when the body references
/// `arguments` and the user hasn't already bound it (either explicitly or via
/// their own rest param).
pub fn append_synthetic_arguments_param(ctx: &mut LoweringContext, params: &mut Vec<Param>) {
    let arguments_id = ctx.define_local("arguments".to_string(), Type::Any);
    params.push(Param {
        id: arguments_id,
        name: "arguments".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: true,
    });
}
