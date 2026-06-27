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
use super::global_eval_hoist::apply_global_eval_hoist;
use super::lower_expr::lower_expr;
use super::LoweringContext;

/// Lower an expression that throws a `SyntaxError` when the enclosing call
/// site is evaluated — a throwing IIFE in value position. Used when a folded
/// `new Function(...)` / `Function(...)` body is not syntactically valid JS,
/// matching Node's runtime `SyntaxError` (Test262 catches it via
/// `assert.throws`). The message is generic — Test262 only checks the error's
/// *constructor*, not its text.
fn synth_function_syntax_error(
    ctx: &mut LoweringContext,
    surface: EvalSurface,
    span: swc_common::Span,
) -> Result<Expr> {
    if eval_diag_enabled() {
        eprintln!(
            "[perry-eval-diag] {} -> invalid body: throws SyntaxError at runtime (#1679)",
            surface.label()
        );
    }
    synth_throwing_iife(
        ctx,
        "throw new SyntaxError(\"Function constructor: invalid function body\");",
        span,
    )
}

/// Lower an IIFE-in-value-position that executes `throw_stmt` — used both
/// for the runtime-`SyntaxError` path above and for a constant-`toString`
/// argument that throws (`new Function({toString(){throw 1}})` must throw
/// `1` when the call site is evaluated, before any parsing).
fn synth_throwing_iife(
    ctx: &mut LoweringContext,
    throw_stmt: &str,
    span: swc_common::Span,
) -> Result<Expr> {
    let src = format!("(function () {{ {throw_stmt} }})();\n");
    let module = perry_parser::parse_typescript(&src, "<new Function syntaxerror>.cjs")
        .map_err(|e| anyhow::Error::new(LowerError::new(format!("internal: {e}"), span)))?;
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) =
        module.body.first().ok_or_else(|| {
            anyhow::Error::new(LowerError::new("internal: empty synth".to_string(), span))
        })?
    else {
        return Err(anyhow::Error::new(LowerError::new(
            "internal: synth shape".to_string(),
            span,
        )));
    };
    let outer_strict = ctx.current_strict;
    ctx.current_strict = false;
    let lowered = lower_expr(ctx, &expr_stmt.expr);
    ctx.current_strict = outer_strict;
    lowered
}

/// #5245: synthesize a throw-on-reach value for a recognized-but-unimplemented
/// node/stdlib API reference under the default (defer) mode. The whole gated
/// expression (the member read `process.binding`, or the call callee) is
/// replaced by a throwing IIFE: the descriptive `Error` is thrown the moment
/// control reaches the reference — which a guarding `try/catch` then swallows,
/// exactly as a real package (e.g. `safer-buffer`) expects. `message` is
/// [`crate::eval_classifier::unimplemented_runtime_error_message`].
pub(crate) fn synth_deferred_throw_value(
    ctx: &mut LoweringContext,
    message: &str,
    span: swc_common::Span,
) -> Result<Expr> {
    let lit = json_string_literal(message);
    let throw_stmt = format!("throw new Error({lit});");
    synth_throwing_iife(ctx, &throw_stmt, span)
}

/// #5206: synthesize the throw-on-reach value for a runtime-unknown `eval`
/// / `Function` / `new Function` site under the default (defer) mode. The
/// `message` is the descriptive
/// [`crate::eval_classifier::EvalClassification::deferred_runtime_error_message`].
///
/// - For `eval(...)` the value is a throwing IIFE — the throw happens when
///   the `eval(...)` call evaluates (it is "reached" the moment control
///   reaches the eval).
/// - For `Function(...)` / `new Function(...)` the value is a *function* that
///   throws when called — constructing it is fine; invoking the result is
///   what's "reached". The site replaces the whole `new Function(...)` /
///   `Function(...)` expression, so it must evaluate to that function value.
pub(crate) fn synth_deferred_eval_value(
    ctx: &mut LoweringContext,
    surface: EvalSurface,
    message: &str,
    span: swc_common::Span,
) -> Result<Expr> {
    // JSON-encode the message so embedded quotes / newlines are safe inside
    // the synthesized source string literal.
    let lit = json_string_literal(message);
    let throw_stmt = format!("throw new Error({lit});");
    match surface {
        EvalSurface::Eval => synth_throwing_iife(ctx, &throw_stmt, span),
        EvalSurface::FunctionCall | EvalSurface::NewFunction => {
            // A function value that throws on invocation.
            let src = format!("(function () {{ {throw_stmt} }});\n");
            let module = perry_parser::parse_typescript(&src, "<deferred eval value>.cjs")
                .map_err(|e| anyhow::Error::new(LowerError::new(format!("internal: {e}"), span)))?;
            let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) =
                module.body.into_iter().next().ok_or_else(|| {
                    anyhow::Error::new(LowerError::new("internal: empty synth".to_string(), span))
                })?
            else {
                return Err(anyhow::Error::new(LowerError::new(
                    "internal: synth shape".to_string(),
                    span,
                )));
            };
            let outer_strict = ctx.current_strict;
            ctx.current_strict = false;
            let lowered = lower_expr(ctx, &expr_stmt.expr);
            ctx.current_strict = outer_strict;
            lowered
        }
    }
}

/// Minimal JSON string-literal encoder for embedding a message into
/// synthesized source. Escapes the characters that would break a double-
/// quoted JS string literal.
fn json_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render an `f64` the way JavaScript's `Number::toString` (radix 10) would,
/// for the small primitive subset Test262 hands to `new Function`. Exactness
/// only matters when the number lands in a *parameter* slot (where it is an
/// invalid identifier anyway, so the synth fails to parse and both runtimes
/// reject); in a *body* slot the literal statement is never executed by these
/// tests.
pub(crate) fn js_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        return format!("{}", n as i64);
    }
    format!("{}", n)
}

/// Coerce a `new Function` / `Function` argument that is a compile-time
/// constant *value* to the string Node's `ToString` would produce. Extends
/// [`const_string_of`] (strings / substitution-free templates) with the other
/// primitive literals Test262 passes for params and bodies. Returns `None`
/// for any non-constant or non-primitive argument (objects, identifiers,
/// calls) so those stay on the runtime / refusal path.
fn coerce_arg_to_string(expr: &ast::Expr) -> Option<String> {
    if let Some(s) = const_string_of(expr) {
        return Some(s);
    }
    let mut e = expr;
    while let ast::Expr::Paren(p) = e {
        e = p.expr.as_ref();
    }
    match e {
        ast::Expr::Lit(ast::Lit::Null(_)) => Some("null".to_string()),
        ast::Expr::Lit(ast::Lit::Bool(b)) => {
            Some(if b.value { "true" } else { "false" }.to_string())
        }
        ast::Expr::Lit(ast::Lit::Num(n)) => Some(js_number_to_string(n.value)),
        ast::Expr::Ident(id) if id.sym.as_str() == "undefined" => Some("undefined".to_string()),
        // `void <literal>` always evaluates to `undefined`. Only fold when the
        // operand is itself a primitive literal so no side effect is dropped.
        ast::Expr::Unary(u) if matches!(u.op, ast::UnaryOp::Void) => {
            matches!(u.arg.as_ref(), ast::Expr::Lit(_)).then(|| "undefined".to_string())
        }
        _ => None,
    }
}

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
    try_const_fold_function_construct_kind(
        ctx,
        args,
        surface,
        span,
        super::fn_ctor_env::DynFnCtorKind::Plain,
    )
}

/// Kind-aware core of the fold: `AsyncFunction(...)` assembles
/// `async function anonymous(...)`, `GeneratorFunction(...)` a
/// `function* anonymous(...)`, etc.
pub(crate) fn try_const_fold_function_construct_kind(
    ctx: &mut LoweringContext,
    args: &[ast::ExprOrSpread],
    surface: EvalSurface,
    span: swc_common::Span,
    kind: super::fn_ctor_env::DynFnCtorKind,
) -> Result<Option<Expr>> {
    // A spread argument can't be expanded into a static param/body list.
    if args.iter().any(|a| a.spread.is_some()) {
        return Ok(None);
    }
    // Every argument must resolve to a compile-time-constant string the way
    // Node's `ToString` would (params *and* body): literals, substitution-free
    // templates, `null`/`undefined`/`void 0`/numbers/booleans, plus —
    // via the pre-scanned `fn_ctor_env` — single-assignment module variables,
    // object literals with a constant `toString`, and `Object(<lit>)`
    // wrappers. ToString runs left-to-right; an argument whose `toString`
    // throws a constant aborts the sequence and the call site lowers to an
    // IIFE that throws that value (`new Function({toString(){throw 1}})`
    // throws `1`). Anything outside the subset bails to the runtime path.
    let mut consts: Vec<String> = Vec::with_capacity(args.len());
    let mut thrown: Option<super::fn_ctor_env::ConstVal> = None;
    ctx.fn_ctor_env.pending_side_effects.clear();
    for a in args {
        match resolve_fn_ctor_arg(ctx, &a.expr) {
            Some(super::fn_ctor_env::ResolvedArg::Str(s)) => consts.push(s),
            Some(super::fn_ctor_env::ResolvedArg::Thrown(v)) => {
                thrown = Some(v);
                break;
            }
            None => return Ok(None),
        }
    }
    if let Some(v) = thrown {
        if eval_diag_enabled() {
            eprintln!(
                "[perry-eval-diag] {} -> constant toString throws at runtime (#1679)",
                surface.label()
            );
        }
        // Replay the toString side effects (`p = 1`) before throwing — the
        // synthesized IIFE lowers in the enclosing scope, so the assignments
        // resolve to the same module-level bindings.
        let effects = ctx.fn_ctor_env.pending_side_effects.join(" ");
        let stmt = format!("{effects} throw {};", v.to_js_literal());
        return synth_throwing_iife(ctx, stmt.trim_start(), span).map(Some);
    }

    // Node treats the last argument as the body and every earlier argument
    // as a (possibly comma-joined) parameter list: `new Function('a','b',
    // 'return a+b')` ≡ `new Function('a, b', 'return a+b')`. Joining the
    // param args with `,` reproduces either spelling.
    let (body_src, params_src) = match consts.split_last() {
        Some((body, params)) => (body.clone(), params.join(",")),
        // `new Function()` / `Function()` — empty params, empty body.
        None => (String::new(), String::new()),
    };

    // CSP capability-probe handling (`PERRY_EVAL_CSP`). A trivial no-op
    // `new Function("")` / `Function("")` is the canonical runtime-codegen
    // feature-test (`try { new Function(""), true } catch { false }`). perry is
    // ahead-of-time compiled and cannot generate code from a runtime string, so
    // under CSP mode this probe must report "unavailable" — throw at
    // *construction* (not when called), exactly as a CSP `unsafe-eval`-blocked
    // environment does — so probing callers (e.g. zod 4's validator JIT) take
    // their non-codegen interpreter fallback. Only the trivial empty-body no-op
    // is refused; real literal bodies (`return 42`, the `return this` globalThis
    // polyfill) still fold, preserving spec behavior by default.
    if body_src.trim().is_empty() && crate::eval_classifier::eval_csp_probe_unavailable() {
        return synth_throwing_iife(
            ctx,
            "throw new TypeError(\"Function: runtime dynamic code generation is \
             unavailable in this ahead-of-time compiled binary\");",
            span,
        )
        .map(Some);
    }

    // Assemble the exact source text the spec's CreateDynamicFunction
    // prescribes: newlines around the body and *before the closing paren*
    // so a `//` comment in the params or body can't swallow a delimiter.
    // This text is also what `fn.toString()` must return, and the function's
    // name is `anonymous`.
    let assembled = format!(
        "{} anonymous({params_src}\n) {{\n{body_src}\n}}",
        kind.prefix()
    );
    let synth = format!("({assembled});\n");
    // A `new Function(...)` / `Function(...)` whose params+body don't form a
    // syntactically valid function is a *runtime* `SyntaxError` in JS — the
    // parse happens inside the constructor call, not at our compile time.
    // Node throws (and Test262 routinely catches it with `assert.throws`), so
    // refusing the program at compile time would diverge. Instead synthesize a
    // throwing IIFE in value position: evaluating the original call site now
    // throws `SyntaxError`, exactly as Node does. (A body that parses but
    // can't be *lowered* — an unsupported Perry feature, below — stays a
    // genuine compile-time gap.)
    let module = match perry_parser::parse_typescript(&synth, "<new Function body>.cjs") {
        Ok(m) => m,
        Err(_) => return synth_function_syntax_error(ctx, surface, span).map(Some),
    };

    let Some(fn_expr) = extract_fn_expr(&module) else {
        return synth_function_syntax_error(ctx, surface, span).map(Some);
    };

    // Early errors SWC's parser doesn't surface: a `"use strict"` directive
    // prologue makes duplicate or `eval`/`arguments` parameter names a
    // SyntaxError, and a private name (`o.#f`) outside any class body is a
    // SyntaxError regardless of mode (AllPrivateIdentifiersValid).
    if fn_ctor_strict_param_early_error(fn_expr) || fn_body_has_stray_private_name(fn_expr) {
        return synth_function_syntax_error(ctx, surface, span).map(Some);
    }

    let outer_strict = ctx.current_strict;
    ctx.current_strict = false;
    let lowered_result = lower_fn_expr(ctx, fn_expr);
    ctx.current_strict = outer_strict;
    let lowered = match lowered_result {
        Ok(l) => l,
        Err(e) => {
            // The synthesized body parsed but couldn't be turned into a
            // callable — either a strict-mode early error SWC accepts but
            // lowering rejects (e.g. `with` under `'use strict'`, a genuine
            // `SyntaxError`) or a valid feature Perry can't compile yet.
            // Either way `new Function`/`Function` validates its body at
            // *construction* time, so the spec-faithful outcome is a runtime
            // throw at this call site, not a compile-time refusal of the
            // whole program (Test262 catches it with `assert.throws`). Throw
            // a `SyntaxError`. (`--eval-diag` records what the body was.)
            if eval_diag_enabled() {
                eprintln!(
                    "[perry-eval-diag] {} -> body could not be lowered ({}); \
                     throwing SyntaxError at runtime (#1679)\n  body: {:?}",
                    surface.label(),
                    e,
                    body_src,
                );
            }
            return synth_function_syntax_error(ctx, surface, span).map(Some);
        }
    };

    // `fn.toString()` must return the spec-assembled source, not a slice of
    // the enclosing module at the synthetic span (which would be garbage).
    if let Expr::Closure { func_id, .. } = &lowered {
        ctx.closure_source_text.insert(*func_id, assembled);
    }

    if eval_diag_enabled() {
        eprintln!(
            "[perry-eval-diag] {} -> const-foldable: compiled to native function (#1679)",
            surface.label()
        );
    }
    Ok(Some(lowered))
}

/// A `"use strict"` directive prologue in a dynamic function's body makes
/// duplicate parameter names and parameters named `eval` / `arguments`
/// SyntaxErrors — early errors SWC's parser accepts in sloppy mode.
fn fn_ctor_strict_param_early_error(fn_expr: &ast::FnExpr) -> bool {
    let Some(body) = &fn_expr.function.body else {
        return false;
    };
    let mut strict = false;
    for stmt in &body.stmts {
        let Some(directive) = super::string_directive_stmt_lit(stmt) else {
            break;
        };
        if super::is_raw_use_strict_directive(directive) {
            strict = true;
            break;
        }
    }
    if !strict {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for p in &fn_expr.function.params {
        if let ast::Pat::Ident(b) = &p.pat {
            let name = b.id.sym.to_string();
            if name == "eval" || name == "arguments" || !seen.insert(name) {
                return true;
            }
        }
    }
    false
}

/// AllPrivateIdentifiersValid: a private name (`o.#f`, `#f in o`) outside
/// any class body is a SyntaxError. SWC parses it without complaint, so walk
/// the synthesized body — skipping class expressions/declarations, where
/// private names are legal — and report any stray use.
fn fn_body_has_stray_private_name(fn_expr: &ast::FnExpr) -> bool {
    fn expr_has(e: &ast::Expr) -> bool {
        match e {
            ast::Expr::Member(m) => {
                matches!(m.prop, ast::MemberProp::PrivateName(_)) || expr_has(&m.obj)
            }
            ast::Expr::Bin(b) => {
                matches!(b.left.as_ref(), ast::Expr::PrivateName(_))
                    || expr_has(&b.left)
                    || expr_has(&b.right)
            }
            ast::Expr::PrivateName(_) => true,
            ast::Expr::Paren(p) => expr_has(&p.expr),
            ast::Expr::Unary(u) => expr_has(&u.arg),
            ast::Expr::Update(u) => expr_has(&u.arg),
            ast::Expr::Assign(a) => expr_has(&a.right),
            ast::Expr::Cond(c) => expr_has(&c.test) || expr_has(&c.cons) || expr_has(&c.alt),
            ast::Expr::Seq(s) => s.exprs.iter().any(|e| expr_has(e)),
            ast::Expr::Call(c) => {
                let callee = match &c.callee {
                    ast::Callee::Expr(e) => expr_has(e),
                    _ => false,
                };
                callee || c.args.iter().any(|a| expr_has(&a.expr))
            }
            ast::Expr::New(n) => {
                expr_has(&n.callee)
                    || n.args
                        .as_ref()
                        .map(|args| args.iter().any(|a| expr_has(&a.expr)))
                        .unwrap_or(false)
            }
            ast::Expr::Array(arr) => arr.elems.iter().flatten().any(|elem| expr_has(&elem.expr)),
            ast::Expr::Object(obj) => obj.props.iter().any(|p| match p {
                ast::PropOrSpread::Spread(s) => expr_has(&s.expr),
                ast::PropOrSpread::Prop(p) => match p.as_ref() {
                    ast::Prop::KeyValue(kv) => expr_has(&kv.value),
                    _ => false,
                },
            }),
            ast::Expr::Fn(f) => f
                .function
                .body
                .as_ref()
                .map(|b| b.stmts.iter().any(stmt_has))
                .unwrap_or(false),
            ast::Expr::Arrow(a) => match a.body.as_ref() {
                ast::BlockStmtOrExpr::BlockStmt(b) => b.stmts.iter().any(stmt_has),
                ast::BlockStmtOrExpr::Expr(e) => expr_has(e),
            },
            // Private names are legal inside class bodies.
            ast::Expr::Class(_) => false,
            _ => false,
        }
    }
    fn stmt_has(s: &ast::Stmt) -> bool {
        match s {
            ast::Stmt::Expr(e) => expr_has(&e.expr),
            ast::Stmt::Return(r) => r.arg.as_deref().map(expr_has).unwrap_or(false),
            ast::Stmt::Throw(t) => expr_has(&t.arg),
            ast::Stmt::If(i) => {
                expr_has(&i.test)
                    || stmt_has(&i.cons)
                    || i.alt.as_deref().map(stmt_has).unwrap_or(false)
            }
            ast::Stmt::Block(b) => b.stmts.iter().any(stmt_has),
            ast::Stmt::Decl(ast::Decl::Var(v)) => v
                .decls
                .iter()
                .any(|d| d.init.as_deref().map(expr_has).unwrap_or(false)),
            ast::Stmt::Decl(ast::Decl::Fn(f)) => f
                .function
                .body
                .as_ref()
                .map(|b| b.stmts.iter().any(stmt_has))
                .unwrap_or(false),
            ast::Stmt::While(w) => expr_has(&w.test) || stmt_has(&w.body),
            ast::Stmt::Try(t) => {
                t.block.stmts.iter().any(stmt_has)
                    || t.handler
                        .as_ref()
                        .map(|h| h.body.stmts.iter().any(stmt_has))
                        .unwrap_or(false)
                    || t.finalizer
                        .as_ref()
                        .map(|f| f.stmts.iter().any(stmt_has))
                        .unwrap_or(false)
            }
            _ => false,
        }
    }
    fn_expr
        .function
        .body
        .as_ref()
        .map(|b| b.stmts.iter().any(stmt_has))
        .unwrap_or(false)
}

/// Resolve one `Function(...)` argument to the string `ToString` would
/// produce (or the constant it would throw). Extends [`coerce_arg_to_string`]
/// with inline object literals and — at module top level, where shadowing
/// can't bite — identifiers resolved through the pre-scanned
/// [`super::fn_ctor_env::FnCtorEnv`].
fn resolve_fn_ctor_arg(
    ctx: &mut LoweringContext,
    expr: &ast::Expr,
) -> Option<super::fn_ctor_env::ResolvedArg> {
    use super::fn_ctor_env::{eval_tostring, object_tostring_body, FnCtorShape, ResolvedArg};
    if let Some(s) = coerce_arg_to_string(expr) {
        return Some(ResolvedArg::Str(s));
    }
    let mut e = expr;
    loop {
        match e {
            ast::Expr::Paren(p) => e = p.expr.as_ref(),
            ast::Expr::TsAs(t) => e = t.expr.as_ref(),
            ast::Expr::TsTypeAssertion(t) => e = t.expr.as_ref(),
            _ => break,
        }
    }
    if let ast::Expr::Object(obj) = e {
        if obj.props.is_empty() {
            return Some(ResolvedArg::Str("[object Object]".to_string()));
        }
        if let Some(body) = object_tostring_body(e) {
            return eval_tostring(&mut ctx.fn_ctor_env, &body);
        }
        return None;
    }
    if let Some(s) = super::fn_ctor_env::wrapper_const_string(e) {
        return Some(ResolvedArg::Str(s));
    }
    if ctx.scope_depth == 0 {
        if let ast::Expr::Ident(id) = e {
            let shape = ctx.fn_ctor_env.entries.get(id.sym.as_str()).cloned()?;
            return match shape {
                FnCtorShape::Str(s) => Some(ResolvedArg::Str(s)),
                FnCtorShape::UndefinedVar => Some(ResolvedArg::Str("undefined".to_string())),
                FnCtorShape::ObjToString(body) => eval_tostring(&mut ctx.fn_ctor_env, &body),
                // A dynamic-function ctor VALUE used as a ToString-able arg
                // isn't a constant string.
                FnCtorShape::DynCtor(_) | FnCtorShape::FnLiteral(_) => None,
            };
        }
        // A constant EXPRESSION over env entries — `Function(p + "," + p,
        // …)` where `p` is a recorded toString object. The partial
        // evaluator runs the toStrings (counters, side effects) in order.
        if let Some(v) = super::fn_ctor_env::eval_arg_expr(&mut ctx.fn_ctor_env, e) {
            return Some(ResolvedArg::Str(v));
        }
    }
    None
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
/// Re-parse a direct-eval body that failed the plain script parse inside an
/// object-method wrapper so SuperProperty forms parse, and return the method
/// body statements. Only used when the eval call site provides a super
/// capability (class member / object method); returns None otherwise so the
/// caller keeps the parse-failure SyntaxError.
fn reparse_eval_body_with_super(ctx: &LoweringContext, body_src: &str) -> Option<Vec<ast::Stmt>> {
    let super_capable = ctx.current_class.is_some()
        || ctx.in_constructor_class.is_some()
        || !ctx.object_super_home_stack.is_empty();
    if !super_capable || !body_src.contains("super") {
        return None;
    }
    let wrapped = format!("({{ __perry_eval_m() {{\n{body_src}\n}} }});");
    let module = perry_parser::parse_typescript(&wrapped, "<eval body>.cjs").ok()?;
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) = module.body.into_iter().next()? else {
        return None;
    };
    let mut e = *expr_stmt.expr;
    loop {
        match e {
            ast::Expr::Paren(p) => e = *p.expr,
            ast::Expr::Object(obj) => {
                let prop = obj.props.into_iter().next()?;
                let ast::PropOrSpread::Prop(prop) = prop else {
                    return None;
                };
                let ast::Prop::Method(method) = *prop else {
                    return None;
                };
                return Some(method.function.body?.stmts);
            }
            _ => return None,
        }
    }
}

/// Match the `(0, eval)('<const>')` indirect-eval shape and return the
/// constant body source.
fn indirect_eval_const_body(ctx: &LoweringContext, call: &ast::CallExpr) -> Option<String> {
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
    const_string_of(&call.args[0].expr)
}

/// Indirect eval evaluates as *global* code: any `super()` / `super.x` in the
/// body is a SyntaxError at the eval call, and private names are only valid
/// when a class inside the eval source declares them.
fn try_indirect_eval_super_private(ctx: &LoweringContext, call: &ast::CallExpr) -> Option<Expr> {
    let body = indirect_eval_const_body(ctx, call)?;
    let module = match perry_parser::parse_typescript(&body, "<eval body>.cjs") {
        Ok(m) => m,
        // SWC rejects some super / new.target forms at parse time (`super.x`
        // at script top level). Global code can never contain either, so a
        // body that mentions one and fails to parse is the same SyntaxError;
        // other parse failures stay on the existing fallthrough path.
        Err(_) => {
            if body.contains("super") || body.contains("new.target") {
                return Some(super::eval_super_scan::throw_eval_super_unexpected_expr());
            }
            return None;
        }
    };
    let mut stmts: Vec<ast::Stmt> = Vec::with_capacity(module.body.len());
    for item in module.body {
        match item {
            ast::ModuleItem::Stmt(s) => stmts.push(s),
            _ => return None,
        }
    }
    super::eval_super_scan::check_indirect_eval_super_private(&stmts)
}

/// General indirect-eval fold for a `(0, eval)('<const>')` site whose body is
/// not the `this`/`globalThis` idiom. Indirect eval runs as sloppy *global*
/// code: a parse failure (or a top-level `return`/`break`/`continue`, or
/// `import`/`export`) is a runtime `SyntaxError`; an otherwise-valid body
/// evaluated at module top level (`scope_depth == 0`, where the only enclosing
/// bindings are the globals indirect eval may legitimately see) runs through the
/// shared completion-tracking IIFE so the call yields the body's completion
/// value. A non-constant or non-string argument returns `None` so the call
/// reaches the runtime global-`eval` thunk (which returns a non-string argument
/// unchanged). (test262 language/eval-code/indirect/parse-failure-*,
/// cptn-nrml-expr-prim)
pub(crate) fn try_indirect_eval_general(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    span: swc_common::Span,
) -> Result<Option<Expr>> {
    let Some(body_src) = indirect_eval_const_body(ctx, call) else {
        return Ok(None);
    };
    let body_module = match perry_parser::parse_typescript(&body_src, "<eval body>.cjs") {
        Ok(m) => m,
        Err(_) => {
            return Ok(Some(
                super::eval_super_scan::throw_eval_syntax_error_public("Unexpected end of input"),
            ))
        }
    };
    let mut body_stmts: Vec<ast::Stmt> = Vec::with_capacity(body_module.body.len());
    for item in body_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => body_stmts.push(s),
            // `import` / `export` is illegal in eval (global) code.
            _ => {
                return Ok(Some(
                    super::eval_super_scan::throw_eval_syntax_error_public(
                        "Cannot use import/export statements in eval code",
                    ),
                ))
            }
        }
    }
    if let Some(throw) = super::eval_super_scan::check_eval_illegal_abrupt(&body_stmts) {
        return Ok(Some(throw));
    }
    // Indirect eval runs as *global* code: its body sees only the global
    // environment, never any enclosing module/function bindings. A completion-
    // tracking IIFE captures the *enclosing* scope, so it only matches global
    // eval semantics where the enclosing scope already IS the global scope:
    // module top level (`scope_depth == 0`, no enclosing class / `with` / ESM)
    // under global-script mode, where module-top `var`s/functions and `this`
    // are the global bindings (#5608/#5609). There, fold the body to the shared
    // completion-value IIFE so `(0,eval)('x = 1')` mutates the global `x` and
    // yields its completion value (test262 language/eval-code/indirect/
    // cptn-nrml-expr-*, always-non-strict). The wrapper runs sloppy unless the
    // body opens with its own Use Strict Directive — indirect eval never
    // inherits the caller's strictness.
    //
    // Anywhere else (nested scope, or default CJS mode), capturing the
    // enclosing scope would wrongly resolve module/function-locals that real
    // global eval cannot see, so defer those to the runtime global-`eval`
    // thunk. (Only the parse-/early-error SyntaxError cases above are modeled.)
    let module_top_global = eval_is_module_top_global(ctx);
    // A scope-capturing IIFE places any `var`/`function`/`class`/`let`/`const`
    // the body declares inside the wrapper, but real global eval routes them to
    // the global var environment (`var`/`function`) or the eval's own fresh
    // lexical environment (`let`/`const`/`class`). A declaration-free body has no
    // such binding to misplace; it only reads/assigns the globals it names,
    // which the IIFE resolves correctly.
    if module_top_global && !eval_body_declares_bindings(&body_stmts) {
        let eval_strict = crate::lower_decl::body_has_use_strict(&body_stmts);
        return build_eval_completion_iife(ctx, body_stmts, eval_strict, span);
    }
    if module_top_global {
        let eval_strict = crate::lower_decl::body_has_use_strict(&body_stmts);
        // Annex B.3.3.3: a *sloppy* global (indirect) eval whose body declares
        // `var`/`function` bindings hoists them into the global variable
        // environment. Rewrite those to global assignments and fold; the rewrite
        // bails (→ falls through below) on a `class` declaration — which Perry
        // would otherwise register at module scope, leaking it past the eval
        // (test262 language/eval-code/indirect/lex-env-distinct-cls expects it to
        // stay invisible). Strict eval keeps its own variable environment, which
        // the completion IIFE already models, so it skips the hoist.
        if !eval_strict {
            if let Some(hoisted) = apply_global_eval_hoist(&body_stmts) {
                return build_eval_completion_iife(ctx, hoisted, false, span);
            }
        }
        // No nested function to hoist (top-level declarations only, or a strict
        // body). Still fold the body to the completion IIFE so it runs for its
        // side effects and yields its ECMAScript completion value — the runtime
        // eval thunk otherwise returns `undefined` *without executing the body*,
        // dropping both. Guarded by `eval_body_iife_foldable`, which keeps a
        // class-declaring body on the runtime thunk (the class would leak to
        // module scope when lowered in the IIFE). (test262 language/eval-code/
        // indirect/cptn-nrml-* with declarations, var-env-var-* completion.)
        if eval_body_iife_foldable(&body_stmts) {
            return build_eval_completion_iife(ctx, body_stmts, eval_strict, span);
        }
    }
    let _ = span;
    Ok(None)
}

/// Does an (indirect) eval body declare any binding — `var` / `function` /
/// `class` / `let` / `const` / `using` — anywhere within it? Scans recursively
/// through every statement form that can nest a declaration (a `var`/`function`
/// hoists out of blocks, loops, `try`, `switch`, `with`, labeled and `if`
/// statements), so the answer is conservative: a `true` defers the fold.
fn eval_body_declares_bindings(stmts: &[ast::Stmt]) -> bool {
    stmts.iter().any(stmt_declares_binding)
}

fn stmt_declares_binding(stmt: &ast::Stmt) -> bool {
    use ast::Stmt;
    match stmt {
        Stmt::Decl(_) => true,
        Stmt::Block(b) => eval_body_declares_bindings(&b.stmts),
        Stmt::Labeled(l) => stmt_declares_binding(&l.body),
        Stmt::If(i) => {
            stmt_declares_binding(&i.cons) || i.alt.as_deref().is_some_and(stmt_declares_binding)
        }
        Stmt::For(f) => {
            matches!(&f.init, Some(ast::VarDeclOrExpr::VarDecl(_)))
                || stmt_declares_binding(&f.body)
        }
        Stmt::ForIn(f) => {
            matches!(
                &f.left,
                ast::ForHead::VarDecl(_) | ast::ForHead::UsingDecl(_)
            ) || stmt_declares_binding(&f.body)
        }
        Stmt::ForOf(f) => {
            matches!(
                &f.left,
                ast::ForHead::VarDecl(_) | ast::ForHead::UsingDecl(_)
            ) || stmt_declares_binding(&f.body)
        }
        Stmt::While(w) => stmt_declares_binding(&w.body),
        Stmt::DoWhile(d) => stmt_declares_binding(&d.body),
        Stmt::With(w) => stmt_declares_binding(&w.body),
        Stmt::Try(t) => {
            eval_body_declares_bindings(&t.block.stmts)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| eval_body_declares_bindings(&h.body.stmts))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| eval_body_declares_bindings(&f.stmts))
        }
        Stmt::Switch(s) => s.cases.iter().any(|c| eval_body_declares_bindings(&c.cons)),
        // Statements that cannot introduce a binding. Listed explicitly (no
        // `_` catch-all) so a future `ast::Stmt` variant that *can* nest a
        // declaration is a compile error here rather than a silent miss.
        Stmt::Expr(_)
        | Stmt::Empty(_)
        | Stmt::Debugger(_)
        | Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_) => false,
    }
}

/// Can a declaration-bearing (indirect) eval body be folded to the completion
/// IIFE — executing it for its side effects and completion value — without an
/// observably wrong result? The one disqualifier is a *class declaration*: Perry
/// registers class names at module scope when lowering them inside the IIFE,
/// which would leak the class past the eval (real global eval discards the
/// eval's own lexical environment, so the class is invisible afterward — test262
/// `language/eval-code/indirect/lex-env-distinct-cls`). `var`/`function` only
/// fail to *publish* to the global var environment (trapped as arrow-locals),
/// which is no worse than the runtime thunk not executing the body at all; and
/// `let`/`const` correctly stay arrow-local (matching the eval's fresh, discarded
/// lexical environment). Scans recursively, mirroring [`stmt_declares_binding`].
fn eval_body_iife_foldable(stmts: &[ast::Stmt]) -> bool {
    !stmts.iter().any(stmt_has_class_decl)
}

fn stmt_has_class_decl(stmt: &ast::Stmt) -> bool {
    use ast::Stmt;
    match stmt {
        Stmt::Decl(ast::Decl::Class(_)) => true,
        Stmt::Decl(_) => false,
        Stmt::Block(b) => b.stmts.iter().any(stmt_has_class_decl),
        Stmt::Labeled(l) => stmt_has_class_decl(&l.body),
        Stmt::If(i) => {
            stmt_has_class_decl(&i.cons) || i.alt.as_deref().is_some_and(stmt_has_class_decl)
        }
        Stmt::For(f) => stmt_has_class_decl(&f.body),
        Stmt::ForIn(f) => stmt_has_class_decl(&f.body),
        Stmt::ForOf(f) => stmt_has_class_decl(&f.body),
        Stmt::While(w) => stmt_has_class_decl(&w.body),
        Stmt::DoWhile(d) => stmt_has_class_decl(&d.body),
        Stmt::With(w) => stmt_has_class_decl(&w.body),
        Stmt::Try(t) => {
            t.block.stmts.iter().any(stmt_has_class_decl)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.stmts.iter().any(stmt_has_class_decl))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.stmts.iter().any(stmt_has_class_decl))
        }
        Stmt::Switch(s) => s
            .cases
            .iter()
            .any(|c| c.cons.iter().any(stmt_has_class_decl)),
        Stmt::Expr(_)
        | Stmt::Empty(_)
        | Stmt::Debugger(_)
        | Stmt::Return(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Throw(_) => false,
    }
}

pub(crate) fn try_indirect_eval_globalthis(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Option<Expr> {
    let body = indirect_eval_const_body(ctx, call)?;
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
    // Direct eval observes the caller's `this`. At module top level that is the
    // CJS `module.exports` stand-in (`Expr::ModuleTopThis`), the SAME cached
    // object a bare `this` lowers to there (see `lower_expr` `ast::Expr::This`)
    // — NOT `globalThis`, so `eval('this') === this`. (test262
    // language/eval-code/direct/this-value-global)
    let module_top_this = ctx.scope_depth == 0
        && ctx.current_class.is_none()
        && ctx.with_env_stack.is_empty()
        && !ctx.is_external_module;
    // In global-script mode the module top-level `this` IS `globalThis`, so a
    // direct `eval('this')` must fold to the same — keeping `eval('this') ===
    // this` true under a conforming Test262 host (#5579). See `lower_expr`'s
    // `ast::Expr::This` arm for the matching default-vs-script split.
    let module_top_this_global = module_top_this && super::lower_expr::global_script_this_enabled();
    if let Some(normalized) = normalize_eval_this_body(&body) {
        return match normalized.as_str() {
            "globalThis" => Some(Expr::GlobalThisExpr),
            "this" if module_top_this_global => Some(Expr::GlobalThisExpr),
            "this" if module_top_this => Some(Expr::ModuleTopThis),
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

/// Words that are reserved only in strict-mode code. Using one as a binding
/// name (`var public`) inside strict eval code is a SyntaxError. (`eval` /
/// `arguments` bindings are a separate early error handled in
/// `intrinsics::try_strict_eval_arguments_assignment`.)
fn is_strict_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
    )
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
            // Strict direct eval instantiates `var` bindings in the eval's OWN
            // variable environment (EvalDeclarationInstantiation runs with a
            // fresh varEnv when `strictEval` is true), discarded when the eval
            // returns. It must neither alias the caller's binding nor introduce
            // a visible one, so a later reference to the same name still throws
            // ReferenceError. (test262 language/eval-code/direct/
            // var-env-var-strict-caller{,-2,-3}, var-env-lower-lex-strict-caller)
            if ctx.current_strict {
                if is_strict_reserved_word(&name) {
                    return Some(super::eval_super_scan::throw_eval_syntax_error_public(
                        &format!("Unexpected strict mode reserved word `{name}`"),
                    ));
                }
                return Some(Expr::Undefined);
            }
            if let Some(id) = ctx.lookup_local(&name) {
                if !ctx.var_hoisted_ids.contains(&id) {
                    return Some(super::eval_super_scan::throw_eval_syntax_error_public(
                        &format!("Identifier '{name}' has already been declared"),
                    ));
                }
                return Some(Expr::Undefined);
            }
            let id = ctx.define_local(name, perry_types::Type::Any);
            ctx.var_hoisted_ids.insert(id);
            return Some(Expr::Undefined);
        }
        None => return None,
    };
    let name = parse_eval_ident_name(trim_js_eval_ws(name_raw))?;
    let value = parse_eval_literal(value_raw)?;
    // A `var x = <v>` DECLARATION runs the binding initialization as a side
    // effect, but its completion value is empty (→ `undefined`) per spec
    // (VariableStatement yields an empty completion). A bare `x = <v>`
    // AssignmentExpression has completion value `<v>`. Wrap the var-declaration
    // form so it still stores but yields `undefined`. Refs test262
    // language/eval-code cptn-nrml-empty-var (`eval("var x = 1") === undefined`).
    let finish = |assign: Expr| -> Expr {
        if is_var_assignment {
            Expr::Sequence(vec![assign, Expr::Undefined])
        } else {
            assign
        }
    };
    // Strict direct eval `var x = v` binds in the eval's own variable
    // environment, discarded on return — it must not touch the caller's `x` nor
    // introduce a visible binding. The folded initializer here is always a
    // primitive literal (no side effects to preserve), so the whole declaration
    // collapses to its empty completion value. (var-env-var-strict-caller{,-2,-3})
    if is_var_assignment && ctx.current_strict {
        if is_strict_reserved_word(name) {
            return Some(super::eval_super_scan::throw_eval_syntax_error_public(
                &format!("Unexpected strict mode reserved word `{name}`"),
            ));
        }
        return Some(Expr::Undefined);
    }
    if let Some(id) = ctx.lookup_local(name) {
        if is_var_assignment && !ctx.var_hoisted_ids.contains(&id) {
            return Some(super::eval_super_scan::throw_eval_syntax_error_public(
                &format!("Identifier '{name}' has already been declared"),
            ));
        }
        Some(finish(Expr::LocalSet(id, Box::new(value))))
    } else if ctx.current_strict {
        Some(super::throw_reference_error_expr(&format!(
            "eval assignment to undeclared identifier `{name}`"
        )))
    } else {
        let id = ctx.define_local(name.to_string(), perry_types::Type::Any);
        ctx.var_hoisted_ids.insert(id);
        Some(finish(Expr::LocalSet(id, Box::new(value))))
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
    if let Some(expr) = try_indirect_eval_super_private(ctx, call) {
        return Ok(Some(expr));
    }
    if let Some(expr) = try_direct_eval_this_fold(ctx, call) {
        return Ok(Some(expr));
    }
    // General indirect eval `(0, eval)('<const>')`: parse-failure / illegal
    // top-level abrupt completion → runtime SyntaxError; valid body at module
    // scope → completion-value IIFE.
    if let Some(expr) = try_indirect_eval_general(ctx, call, call.span)? {
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
    if id.sym.as_ref() == "eval"
        && ctx.lookup_local("eval").is_none()
        && ctx.lookup_func("eval").is_none()
        && ctx.lookup_imported_func("eval").is_none()
    {
        return try_const_fold_eval(ctx, &call.args, call.span);
    }
    // `var AsyncFunction = (async function(){}).constructor; AsyncFunction(...)`
    // — a single-assignment module var recorded as a dynamic-function ctor.
    if ctx.scope_depth == 0 {
        if let Some(super::fn_ctor_env::FnCtorShape::DynCtor(kind)) =
            ctx.fn_ctor_env.entries.get(id.sym.as_str()).cloned()
        {
            return try_const_fold_function_construct_kind(
                ctx,
                &call.args,
                EvalSurface::FunctionCall,
                call.span,
                kind,
            );
        }
    }
    Ok(None)
}

/// Fold `Function.call(thisArg, ...ctorArgs)` / `Function.apply(thisArg,
/// [ctorArgs])` — CreateDynamicFunction ignores its `this`, so these are the
/// plain constructor call with the leading argument dropped (Test262
/// S15.3_A2_T*: `Function.call(this, "var x / = 1;")` must throw a
/// SyntaxError at runtime).
pub(crate) fn try_eval_function_member_call_fold(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee) = &call.callee else {
        return Ok(None);
    };
    let mut c = callee.as_ref();
    while let ast::Expr::Paren(p) = c {
        c = p.expr.as_ref();
    }
    let ast::Expr::Member(m) = c else {
        return Ok(None);
    };
    let ast::Expr::Ident(obj) = m.obj.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return Ok(None);
    };
    if obj.sym.as_ref() != "Function"
        || ctx.lookup_local("Function").is_some()
        || ctx.lookup_func("Function").is_some()
        || ctx.lookup_imported_func("Function").is_some()
    {
        return Ok(None);
    }
    match prop.sym.as_ref() {
        "call" if !call.args.is_empty() && call.args.iter().all(|a| a.spread.is_none()) => {
            try_const_fold_function_construct(
                ctx,
                &call.args[1..],
                EvalSurface::FunctionCall,
                call.span,
            )
        }
        "apply" if call.args.len() == 2 && call.args.iter().all(|a| a.spread.is_none()) => {
            let mut arg1 = call.args[1].expr.as_ref();
            while let ast::Expr::Paren(p) = arg1 {
                arg1 = p.expr.as_ref();
            }
            let ast::Expr::Array(arr) = arg1 else {
                return Ok(None);
            };
            if arr.elems.iter().any(|e| e.is_none())
                || arr.elems.iter().flatten().any(|e| e.spread.is_some())
            {
                return Ok(None);
            }
            let synth_args: Vec<ast::ExprOrSpread> = arr.elems.iter().flatten().cloned().collect();
            try_const_fold_function_construct(
                ctx,
                &synth_args,
                EvalSurface::FunctionCall,
                call.span,
            )
        }
        _ => Ok(None),
    }
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

/// Owning arrow analog of [`extract_fn_expr_owned`] — pull the `ArrowExpr`
/// out of a synthesized `(() => { ... });` module.
fn extract_arrow_expr_owned(module: ast::Module) -> Option<ast::ArrowExpr> {
    let item = module.body.into_iter().next()?;
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) = item else {
        return None;
    };
    let mut e = *expr_stmt.expr;
    loop {
        match e {
            ast::Expr::Paren(p) => e = *p.expr,
            ast::Expr::Arrow(arrow) => return Some(arrow),
            _ => return None,
        }
    }
}

/// Owning variant of [`extract_fn_expr`] — consumes the module so the caller
/// can mutate the function body before lowering.
fn extract_fn_expr_owned(module: ast::Module) -> Option<ast::FnExpr> {
    let item = module.body.into_iter().next()?;
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) = item else {
        return None;
    };
    let mut e = *expr_stmt.expr;
    loop {
        match e {
            ast::Expr::Paren(p) => e = *p.expr,
            ast::Expr::Fn(fn_expr) => return Some(fn_expr),
            _ => return None,
        }
    }
}

/// Build `__perry_cv = <value>` by cloning a parsed `__perry_cv = undefined`
/// assignment template and swapping its right-hand side. Avoids hand-building
/// version-sensitive SWC `AssignExpr` nodes.
fn cv_assign_from_template(reset_template: &ast::Stmt, value: Box<ast::Expr>) -> Box<ast::Expr> {
    let ast::Stmt::Expr(es) = reset_template else {
        // Caller guarantees the template is an expression statement.
        return value;
    };
    let mut assign = es.expr.clone();
    if let ast::Expr::Assign(a) = assign.as_mut() {
        a.right = value;
    }
    assign
}

/// Statements whose ECMAScript completion value is `UpdateEmpty(..., undefined)`
/// or accumulates from `undefined` — i.e. evaluating them resets the running
/// statement-list completion value to `undefined` before their inner
/// (value-producing) statements may overwrite it. See §13/§14 of the spec
/// (IfStatement / IterationStatement / SwitchStatement / TryStatement all wrap
/// their result in `UpdateEmpty(_, undefined)`).
fn stmt_resets_completion(stmt: &ast::Stmt) -> bool {
    matches!(
        stmt,
        ast::Stmt::If(_)
            | ast::Stmt::While(_)
            | ast::Stmt::DoWhile(_)
            | ast::Stmt::For(_)
            | ast::Stmt::ForIn(_)
            | ast::Stmt::ForOf(_)
            | ast::Stmt::Switch(_)
            | ast::Stmt::Try(_)
            | ast::Stmt::With(_)
    )
}

/// Recurse into a single nested statement, treating it as a one-element
/// statement list. If completion tracking inserts statements (e.g. a reset),
/// the result is re-wrapped in a block so it remains a single `Stmt`.
fn track_completion_single(stmt: &mut ast::Stmt, reset_template: &ast::Stmt) {
    let placeholder = ast::Stmt::Empty(ast::EmptyStmt {
        span: swc_common::DUMMY_SP,
    });
    let mut list = vec![std::mem::replace(stmt, placeholder)];
    track_completion(&mut list, reset_template);
    if list.len() == 1 {
        *stmt = list.pop().unwrap();
    } else {
        *stmt = ast::Stmt::Block(ast::BlockStmt {
            span: swc_common::DUMMY_SP,
            ctxt: Default::default(),
            stmts: list,
        });
    }
}

/// Recurse into the nested statement lists / bodies of a compound statement so
/// their expression statements are tracked too. The `finally` block is
/// intentionally skipped: a normally-completing `finally` does not contribute
/// to the `try` statement's completion value.
fn track_completion_inner(stmt: &mut ast::Stmt, reset_template: &ast::Stmt) {
    match stmt {
        ast::Stmt::Block(b) => track_completion(&mut b.stmts, reset_template),
        ast::Stmt::If(s) => {
            track_completion_single(&mut s.cons, reset_template);
            if let Some(alt) = s.alt.as_mut() {
                track_completion_single(alt, reset_template);
            }
        }
        ast::Stmt::While(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::DoWhile(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::For(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::ForIn(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::ForOf(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::Labeled(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::With(s) => track_completion_single(&mut s.body, reset_template),
        ast::Stmt::Switch(s) => {
            for case in s.cases.iter_mut() {
                track_completion(&mut case.cons, reset_template);
            }
        }
        ast::Stmt::Try(s) => {
            track_completion(&mut s.block.stmts, reset_template);
            if let Some(handler) = s.handler.as_mut() {
                track_completion(&mut handler.body.stmts, reset_template);
            }
        }
        _ => {}
    }
}

/// Rewrite a statement list so that, after evaluation, `__perry_cv` holds the
/// list's ECMAScript completion value. Each `ExpressionStatement e` becomes
/// `__perry_cv = e`; each statement whose completion value is
/// `UpdateEmpty(_, undefined)` is preceded by a `__perry_cv = undefined` reset;
/// declarations / empty statements leave `__perry_cv` unchanged. Recurses into
/// nested statement lists so the rule holds at every depth.
fn track_completion(stmts: &mut Vec<ast::Stmt>, reset_template: &ast::Stmt) {
    let mut out: Vec<ast::Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts.drain(..) {
        let resets = stmt_resets_completion(&stmt);
        track_completion_inner(&mut stmt, reset_template);
        if let ast::Stmt::Expr(es) = &mut stmt {
            let inner = std::mem::replace(
                &mut es.expr,
                Box::new(ast::Expr::Invalid(ast::Invalid {
                    span: swc_common::DUMMY_SP,
                })),
            );
            es.expr = cv_assign_from_template(reset_template, inner);
        }
        if resets {
            out.push(reset_template.clone());
        }
        out.push(stmt);
    }
    *stmts = out;
}

/// #1679: fold a direct `eval("<constant string>")` into a scope-capturing
/// IIFE — `(function () { var __perry_cv; <tracked body>; return __perry_cv })()`
/// — so the program runs the code string AOT and yields its ECMAScript
/// completion value. The wrapper is lowered in sloppy mode (the eval body may
/// use `with`, undeclared assignments, etc.) and captures the enclosing scope,
/// so `eval("with(o){p=1}")` mutates the surrounding `o`. Only single
/// string-constant arguments fold; everything else bails to the runtime path.
fn try_const_fold_eval(
    ctx: &mut LoweringContext,
    args: &[ast::ExprOrSpread],
    span: swc_common::Span,
) -> Result<Option<Expr>> {
    if args.len() != 1 || args[0].spread.is_some() {
        return Ok(None);
    }
    let Some(body_src) = const_string_of(&args[0].expr) else {
        return Ok(None);
    };

    // Parse the eval body as sloppy-mode statements (`.cjs` → script, not
    // module, so `with` is allowed). A parse failure is a runtime SyntaxError.
    let mut body_stmts: Vec<ast::Stmt>;
    match perry_parser::parse_typescript(&body_src, "<eval body>.cjs") {
        Ok(body_module) => {
            body_stmts = Vec::with_capacity(body_module.body.len());
            for item in body_module.body {
                match item {
                    ast::ModuleItem::Stmt(s) => body_stmts.push(s),
                    // `import` / `export` inside eval is a SyntaxError.
                    _ => {
                        return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some)
                    }
                }
            }
        }
        Err(_) => {
            // SWC rejects SuperProperty at script top level, but direct eval
            // inside a class member may legally contain `super.x` (PerformEval
            // runs the eval code with the member's home object). Re-parse the
            // body inside an object-method wrapper — which admits super — and
            // splice the method body out; the super-scan below still rejects
            // SuperCall. Contexts with no super capability keep the plain
            // parse-failure SyntaxError.
            match reparse_eval_body_with_super(ctx, &body_src) {
                Some(stmts) => body_stmts = stmts,
                None => return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some),
            }
        }
    }

    // PerformEval early errors: `super()` / `super.x` outside a context that
    // provides them, and private names with no declaring class in scope, are
    // SyntaxErrors thrown when the eval call evaluates.
    if let Some(throw) = super::eval_super_scan::check_direct_eval_super_private(ctx, &body_stmts) {
        return Ok(Some(throw));
    }

    // PerformEval parse-level early errors: a top-level `return`, or an
    // unlabeled `break`/`continue` with no enclosing loop/switch in the eval
    // source, is a SyntaxError thrown when the eval call evaluates.
    if let Some(throw) = super::eval_super_scan::check_eval_illegal_abrupt(&body_stmts) {
        return Ok(Some(throw));
    }

    // Eval code is strict when the calling context is strict (direct eval) or
    // the body opens with a Use Strict Directive — detected on the *original*
    // statements, before completion-tracking rewrites the directive into a
    // plain assignment. (test262 language/eval-code/direct/strictness-override)
    let eval_strict = ctx.current_strict || crate::lower_decl::body_has_use_strict(&body_stmts);

    // Annex B.3.3.3: a *sloppy global* direct eval routes the `var`/`function`
    // declarations of its body into the global variable environment, so they
    // survive after the eval returns. Rewrite them to global assignments before
    // folding (otherwise the completion IIFE traps them as arrow-locals). Strict
    // eval keeps its own variable environment (the IIFE already models that).
    if !eval_strict && eval_is_module_top_global(ctx) {
        if let Some(hoisted) = apply_global_eval_hoist(&body_stmts) {
            return build_eval_completion_iife(ctx, hoisted, eval_strict, span);
        }
    }

    build_eval_completion_iife(ctx, body_stmts, eval_strict, span)
}

/// Is the current eval call site at module top level in global-script mode,
/// where the enclosing variable environment *is* the global object — the only
/// place the Annex B.3.3.3 global var-scoped hoisting ([`apply_global_eval_hoist`])
/// applies? (Mirrors the `module_top_this`/`module_top_global` guards.)
fn eval_is_module_top_global(ctx: &LoweringContext) -> bool {
    super::lower_expr::global_script_this_enabled()
        && ctx.scope_depth == 0
        && ctx.current_class.is_none()
        && ctx.with_env_stack.is_empty()
        && !ctx.is_external_module
}

/// Build the completion-tracking IIFE that runs an eval body AOT and yields its
/// ECMAScript completion value: `(() => { var __perry_cv; <tracked body>; return
/// __perry_cv })()`. Shared by direct eval and global (indirect) eval. `strict`
/// selects the lowering mode for the wrapper body — direct eval inherits the
/// caller's strictness (or a Use Strict Directive in the body); indirect eval is
/// sloppy global code unless its own directive opts in.
fn build_eval_completion_iife(
    ctx: &mut LoweringContext,
    mut body_stmts: Vec<ast::Stmt>,
    strict: bool,
    span: swc_common::Span,
) -> Result<Option<Expr>> {
    // Wrapper template: stmts == [var __perry_cv = undefined;
    //                            __perry_cv = undefined;   (reset/assign template)
    //                            return __perry_cv;]
    // An ARROW wrapper, not a plain function: direct eval code runs with the
    // caller's `this` / `arguments` / `super` / `new.target` bindings, and the
    // arrow gets all of those lexically. A plain-function wrapper rebound
    // `this` to undefined, so `eval("this.#x")` inside a method brand-checked
    // against the wrong receiver and `eval("this.prop")` read undefined.
    let template_src =
        "(() => {\nvar __perry_cv = undefined;\n__perry_cv = undefined;\nreturn __perry_cv;\n});\n";
    let template_module = match perry_parser::parse_typescript(template_src, "<eval wrapper>.cjs") {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let Some(mut arrow) = extract_arrow_expr_owned(template_module) else {
        return Ok(None);
    };
    let ast::BlockStmtOrExpr::BlockStmt(body) = arrow.body.as_mut() else {
        return Ok(None);
    };
    if body.stmts.len() != 3 {
        return Ok(None);
    }
    let reset_template = body.stmts.remove(1);

    // Track completion values, then splice the tracked body before `return`.
    track_completion(&mut body_stmts, &reset_template);
    let mut insert_at = 1; // after the `var __perry_cv` decl
    for s in body_stmts {
        body.stmts.insert(insert_at, s);
        insert_at += 1;
    }

    // Lower the wrapper and immediately call it: `(() => {…})()`. The wrapper
    // body may use `with` / undeclared sloppy assignments only when `strict` is
    // false; a strict eval honors strict early errors (assignment to an
    // unresolvable reference throws ReferenceError, etc.).
    let outer_strict = ctx.current_strict;
    ctx.current_strict = strict;
    let lowered = lower_expr(ctx, &ast::Expr::Arrow(arrow));
    ctx.current_strict = outer_strict;
    let closure = match lowered {
        Ok(l) => l,
        Err(_) => return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some),
    };
    Ok(Some(Expr::Call {
        callee: Box::new(closure),
        args: vec![],
        type_args: vec![],
        byte_offset: 0,
    }))
}

#[cfg(test)]
mod foldable_tests {
    use super::eval_body_iife_foldable;
    use swc_ecma_ast as ast;

    fn parse(src: &str) -> Vec<ast::Stmt> {
        perry_parser::parse_typescript(src, "<eval body>.cjs")
            .expect("parses")
            .body
            .into_iter()
            .filter_map(|item| match item {
                ast::ModuleItem::Stmt(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn declaration_bearing_non_class_bodies_are_foldable() {
        // The bodies that regressed to `undefined` because they declare a
        // binding (so the declaration-free fast path skipped them) yet have no
        // nested function to hoist — now fold to the completion IIFE.
        for src in [
            "var a = 1; 42",
            "initial = x; var x = 9;",
            "let y = 2; y + 1",
            "const z = 3; ({})",
            "{ var nested; } 7",
            "for (var i = 0; i < 1; i++) {} 5",
            "'use strict'; var s = 1; s",
        ] {
            assert!(
                eval_body_iife_foldable(&parse(src)),
                "expected foldable: {src:?}"
            );
        }
    }

    #[test]
    fn class_declaring_bodies_are_not_foldable() {
        // A class declaration (top-level or nested) would leak to module scope
        // when lowered in the IIFE, so it stays on the runtime thunk.
        for src in [
            "class C {}",
            "{ class C {} }",
            "if (true) { class C {} }",
            "switch (1) { case 1: class C {} }",
            "try { class C {} } catch (e) {}",
        ] {
            assert!(
                !eval_body_iife_foldable(&parse(src)),
                "expected NOT foldable: {src:?}"
            );
        }
        // A class *expression* binds through `var`/`let` (no module-scope
        // registration) and stays foldable.
        assert!(eval_body_iife_foldable(&parse("var x = class {}; x")));
    }
}
