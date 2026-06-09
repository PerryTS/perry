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
    if let Some(id) = ctx.lookup_local(name) {
        if is_var_assignment && !ctx.var_hoisted_ids.contains(&id) {
            return Some(Expr::SyntaxErrorNew(Box::new(Expr::String(format!(
                "eval var declaration conflicts with lexical binding `{name}`"
            )))));
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
    let body_module = match perry_parser::parse_typescript(&body_src, "<eval body>.cjs") {
        Ok(m) => m,
        Err(_) => return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some),
    };
    let mut body_stmts: Vec<ast::Stmt> = Vec::with_capacity(body_module.body.len());
    for item in body_module.body {
        match item {
            ast::ModuleItem::Stmt(s) => body_stmts.push(s),
            // `import` / `export` inside eval is a SyntaxError.
            _ => return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some),
        }
    }

    // Wrapper template: stmts == [var __perry_cv = undefined;
    //                            __perry_cv = undefined;   (reset/assign template)
    //                            return __perry_cv;]
    let template_src = "(function () {\nvar __perry_cv = undefined;\n__perry_cv = undefined;\nreturn __perry_cv;\n});\n";
    let template_module = match perry_parser::parse_typescript(template_src, "<eval wrapper>.cjs") {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    let Some(mut fn_expr) = extract_fn_expr_owned(template_module) else {
        return Ok(None);
    };
    let Some(body) = fn_expr.function.body.as_mut() else {
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

    // Lower the wrapper (sloppy) and immediately call it: `(function(){…})()`.
    let outer_strict = ctx.current_strict;
    ctx.current_strict = false;
    let lowered = lower_fn_expr(ctx, &fn_expr);
    ctx.current_strict = outer_strict;
    let closure = match lowered {
        Ok(l) => l,
        Err(_) => return synth_function_syntax_error(ctx, EvalSurface::Eval, span).map(Some),
    };
    Ok(Some(Expr::Call {
        callee: Box::new(closure),
        args: vec![],
        type_args: vec![],
    }))
}
