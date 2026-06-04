//! #1679 (Phase 1 of #1677) — const-fold literal `new Function` /
//! `Function(...)` bodies into real native functions, plus the
//! `(0, eval)('this')` indirect-eval `globalThis` idiom.
//!
//! When the Phase 0 classifier ([`crate::eval_classifier`]) would bucket a
//! site as **const-foldable** (every argument is a compile-time-constant
//! string), an ahead-of-time compiler should turn it into a genuine
//! function rather than refuse it — this is true AOT eval. We synthesize
//! the equivalent function-literal source, parse it, and lower it through
//! the normal function-expression path, exactly as if the user had written
//! `function (a, b) { return a + b }`.
//!
//! `new Function` has no access to the enclosing lexical scope (globals
//! only), so the realistic const-foldable body references only its own
//! parameters plus globals and lowers to a capture-free closure. (A body
//! that happens to reference an enclosing local will capture it — a benign
//! deviation from strict `new Function` global-only scope, and the literal
//! function-equivalent lowering #1679 asks for.)
//!
//! Out of scope here (→ Phase 3, #1681): library-generated *non-literal*
//! body strings. Those stay in the classifier's runtime-unknown /
//! known-library buckets.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::error::LowerError;
use crate::eval_classifier::{const_string_of, eval_diag_enabled, EvalSurface};
use crate::ir::Expr;

use super::expr_function::lower_fn_expr;
use super::LoweringContext;

/// Fold a `new Function(...)` / `Function(...)` whose arguments are *all*
/// compile-time-constant strings into a native function (`Expr::Closure`).
///
/// Returns `Ok(None)` when not every argument is a constant string — the
/// caller then falls back to the Phase 0 classifier (which refuses the
/// runtime-unknown bucket and logs the rest). Returns `Err` (span-tagged)
/// when the synthesized body is not valid JavaScript or uses a feature
/// Perry can't compile yet — both are genuine, localized compile errors.
pub(crate) fn try_const_fold_function_construct(
    ctx: &mut LoweringContext,
    args: &[ast::ExprOrSpread],
    surface: EvalSurface,
    span: swc_common::Span,
) -> Result<Option<Expr>> {
    // A spread argument can't be expanded into a static param/body list.
    if args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    // Every argument must be a constant string (params *and* body).
    let mut consts: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        match const_string_of(&a.expr) {
            Some(s) => consts.push(s),
            None => return Ok(None),
        }
    }

    // Node treats the last argument as the body and every earlier argument
    // as a (possibly comma-joined) parameter list: `new Function('a','b',
    // 'return a+b')` ≡ `new Function('a, b', 'return a+b')`. Joining the
    // param args with `,` reproduces either spelling.
    let (body_src, params_src) = match consts.split_last() {
        Some((body, params)) => (body.clone(), params.join(", ")),
        // `new Function()` / `Function()` — empty params, empty body.
        None => (String::new(), String::new()),
    };

    let synth = format!("(function ({params_src}) {{\n{body_src}\n}});\n");
    let module =
        perry_parser::parse_typescript(&synth, "<new Function body>.cjs").map_err(|e| {
            anyhow::Error::new(LowerError::new(
                format!(
                    "`{}` body is not valid JavaScript and cannot be compiled: {} \
                 (#1679)\n  body: {:?}",
                    surface.label(),
                    e,
                    body_src,
                ),
                span,
            ))
        })?;

    let fn_expr = extract_fn_expr(&module).ok_or_else(|| {
        anyhow::Error::new(LowerError::new(
            format!(
                "`{}` body could not be parsed as a function body (#1679)\n  body: {:?}",
                surface.label(),
                body_src,
            ),
            span,
        ))
    })?;

    let outer_strict = ctx.current_strict;
    ctx.current_strict = false;
    let lowered_result = lower_fn_expr(ctx, fn_expr);
    ctx.current_strict = outer_strict;
    let lowered = lowered_result.map_err(|e| {
        // The synthesized body parsed but couldn't lower (an unsupported
        // feature inside the generated function). Surface it as a clear
        // error at the original call site rather than the broken
        // placeholder the pre-#1679 fall-through produced.
        anyhow::Error::new(LowerError::new(
            format!(
                "`{}` body uses a feature Perry can't compile yet: {} (#1679)\n  body: {:?}",
                surface.label(),
                e,
                body_src,
            ),
            span,
        ))
    })?;

    if eval_diag_enabled() {
        eprintln!(
            "[perry-eval-diag] {} -> const-foldable: compiled to native function (#1679)",
            surface.label()
        );
    }
    Ok(Some(lowered))
}

/// Fold the indirect-eval `globalThis` idiom — `(0, eval)('this')` /
/// `(0, eval)('globalThis')` (and parenthesized variants) — to
/// [`Expr::GlobalThisExpr`], the same singleton `Function('return this')()`
/// folds to (#957/#959). Indirect `eval` runs in global scope, so
/// `eval('this')` yields the global object.
///
/// Conservative: requires the comma-sequence callee whose last element is
/// the *unshadowed* `eval` builtin, a single non-spread argument, and a
/// constant body that trims to exactly `this` / `globalThis`. Anything
/// else returns `None`.
pub(crate) fn try_indirect_eval_globalthis(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Option<Expr> {
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let mut c = callee.as_ref();
    while let ast::Expr::Paren(p) = c {
        c = p.expr.as_ref();
    }
    // Indirect eval is spelled as a comma sequence: `(0, eval)`.
    let ast::Expr::Seq(seq) = c else {
        return None;
    };
    let mut last = seq.exprs.last()?.as_ref();
    while let ast::Expr::Paren(p) = last {
        last = p.expr.as_ref();
    }
    let ast::Expr::Ident(id) = last else {
        return None;
    };
    if id.sym.as_ref() != "eval"
        || ctx.lookup_local("eval").is_some()
        || ctx.lookup_func("eval").is_some()
    {
        return None;
    }
    let body = const_string_of(&call.args[0].expr)?;
    let trimmed = body.trim().trim_end_matches(';').trim();
    if matches!(trimmed, "this" | "globalThis") {
        if eval_diag_enabled() {
            eprintln!("[perry-eval-diag] (0, eval)({trimmed:?}) -> globalThis (#1679)");
        }
        Some(Expr::GlobalThisExpr)
    } else {
        None
    }
}

/// Fold the tiny direct-eval surface Perry can model without a runtime JS
/// evaluator. Direct eval observes the caller's current `this` binding; in a
/// strict function that binding can be `undefined`, while global direct eval
/// still sees global `this`. The indirect/global case is handled separately by
/// [`try_indirect_eval_globalthis`] and the callable global eval thunk.
fn try_direct_eval_this_fold(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Option<Expr> {
    if call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let mut c = callee.as_ref();
    while let ast::Expr::Paren(p) = c {
        c = p.expr.as_ref();
    }
    let ast::Expr::Ident(id) = c else {
        return None;
    };
    if id.sym.as_ref() != "eval"
        || ctx.lookup_local("eval").is_some()
        || ctx.lookup_func("eval").is_some()
        || ctx.lookup_imported_func("eval").is_some()
    {
        return None;
    }
    let body = const_string_of(&call.args[0].expr)?;
    if let Some(normalized) = normalize_eval_this_body(&body) {
        return match normalized.as_str() {
            "globalThis" => Some(Expr::GlobalThisExpr),
            "this" if ctx.current_strict && ctx.scope_depth > 0 => Some(Expr::Undefined),
            "this" => Some(Expr::This),
            "typeof this" if ctx.current_strict && ctx.scope_depth > 0 => {
                Some(Expr::String("undefined".to_string()))
            }
            "typeof this" => Some(Expr::TypeOf(Box::new(Expr::This))),
            _ => None,
        };
    }
    if let Some(expr) = try_direct_eval_simple_assignment_fold(ctx, &body) {
        return Some(expr);
    }
    try_direct_eval_constant_add_fold(&body)
}

fn parse_eval_ident_name(src: &str) -> Option<&str> {
    let mut chars = src.chars();
    let first = chars.next()?;
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return None;
    }
    if chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric()) {
        Some(src)
    } else {
        None
    }
}

pub(crate) fn direct_eval_var_decl_name(body: &str) -> Option<String> {
    let src = body.trim().trim_end_matches(';').trim();
    let decl = src.strip_prefix("var ")?;
    let name_raw = decl
        .split_once('=')
        .map(|(name, _)| name)
        .unwrap_or(decl)
        .trim_end_matches(';');
    parse_eval_ident_name(trim_js_eval_ws(name_raw)).map(str::to_string)
}

fn parse_eval_literal(src: &str) -> Option<Expr> {
    let s = trim_js_eval_ws(src);
    if let Some(inner) = s.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Some(Expr::String(inner.to_string()));
    }
    if let Some(inner) = s
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return Some(Expr::String(inner.to_string()));
    }
    if s == "undefined" {
        return Some(Expr::Undefined);
    }
    parse_eval_number_literal(s).map(Expr::Number)
}

fn try_direct_eval_simple_assignment_fold(ctx: &mut LoweringContext, body: &str) -> Option<Expr> {
    let src = body.trim().trim_end_matches(';').trim();
    let is_var_assignment = src.starts_with("var ");
    let assignment = src.strip_prefix("var ").unwrap_or(src);
    let (name_raw, value_raw) = match assignment.split_once('=') {
        Some(parts) => parts,
        None if is_var_assignment => {
            let name = direct_eval_var_decl_name(body)?;
            if let Some(id) = ctx.lookup_local(&name) {
                if !ctx.var_hoisted_ids.contains(&id) {
                    return Some(Expr::SyntaxErrorNew(Box::new(Expr::String(format!(
                        "eval var declaration conflicts with lexical binding `{name}`"
                    )))));
                }
                return Some(Expr::Undefined);
            }
            if !ctx.current_strict {
                let id = ctx.define_local(name, perry_types::Type::Any);
                ctx.var_hoisted_ids.insert(id);
            }
            return Some(Expr::Undefined);
        }
        None => return None,
    };
    let name = parse_eval_ident_name(trim_js_eval_ws(name_raw))?;
    let value = parse_eval_literal(value_raw)?;
    if let Some(id) = ctx.lookup_local(name) {
        if is_var_assignment && !ctx.var_hoisted_ids.contains(&id) {
            return Some(Expr::SyntaxErrorNew(Box::new(Expr::String(format!(
                "eval var declaration conflicts with lexical binding `{name}`"
            )))));
        }
        Some(Expr::LocalSet(id, Box::new(value)))
    } else if ctx.current_strict {
        Some(super::throw_reference_error_expr(&format!(
            "eval assignment to undeclared identifier `{name}`"
        )))
    } else {
        let id = ctx.define_local(name.to_string(), perry_types::Type::Any);
        ctx.var_hoisted_ids.insert(id);
        Some(Expr::LocalSet(id, Box::new(value)))
    }
}

fn trim_js_eval_ws(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '\u{0009}'
                    | '\u{000B}'
                    | '\u{000C}'
                    | '\u{0020}'
                    | '\u{00A0}'
                    | '\u{FEFF}'
                    | '\u{2028}'
                    | '\u{2029}'
            )
    })
}

fn parse_eval_number_literal(s: &str) -> Option<f64> {
    let trimmed = trim_js_eval_ws(s);
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn try_direct_eval_constant_add_fold(body: &str) -> Option<Expr> {
    let src = body.trim().trim_end_matches(';').trim();
    let mut parts = src.split('+');
    let left = parse_eval_number_literal(parts.next()?)?;
    let right = parse_eval_number_literal(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(Expr::Number(left + right))
}

fn normalize_eval_this_body(body: &str) -> Option<String> {
    let mut src = body.trim().trim_end_matches(';').trim();
    for directive in ["\"use strict\"", "'use strict'"] {
        if let Some(rest) = src.strip_prefix(directive) {
            let rest = rest.trim_start();
            if let Some(after_semicolon) = rest.strip_prefix(';') {
                src = after_semicolon.trim().trim_end_matches(';').trim();
            }
        }
    }
    if matches!(src, "this" | "globalThis" | "typeof this") {
        Some(src.to_string())
    } else {
        None
    }
}

/// Combined fold entry for the call form, run from `lower_call_inner`
/// before the Phase 0 refusal: the `(0, eval)('this')` idiom first, then a
/// bare-ident `Function(...)` const-fold. `Ok(None)` → fall through to the
/// classifier.
pub(crate) fn try_eval_function_call_fold(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    if let Some(expr) = try_indirect_eval_globalthis(ctx, call) {
        return Ok(Some(expr));
    }
    if let Some(expr) = try_direct_eval_this_fold(ctx, call) {
        return Ok(Some(expr));
    }
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let mut c = callee.as_ref();
    while let ast::Expr::Paren(p) = c {
        c = p.expr.as_ref();
    }
    let ast::Expr::Ident(id) = c else {
        return Ok(None);
    };
    if id.sym.as_ref() == "Function"
        && ctx.lookup_local("Function").is_none()
        && ctx.lookup_func("Function").is_none()
        && ctx.lookup_imported_func("Function").is_none()
    {
        return try_const_fold_function_construct(
            ctx,
            &call.args,
            EvalSurface::FunctionCall,
            call.span,
        );
    }
    Ok(None)
}

/// Pull the `FnExpr` out of a synthesized `(function (...) { ... });`
/// module (a single expression statement wrapping a parenthesized
/// function expression).
fn extract_fn_expr(module: &ast::Module) -> Option<&ast::FnExpr> {
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) = module.body.first()? else {
        return None;
    };
    let mut e = expr_stmt.expr.as_ref();
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Fn(fn_expr) => Some(fn_expr),
        _ => None,
    }
}
