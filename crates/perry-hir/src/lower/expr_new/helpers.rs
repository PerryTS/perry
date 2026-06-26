//! Standalone helper functions for `new C(args)` lowering, extracted from
//! `expr_new.rs` so the trunk stays under the file-size budget. Pure code move
//! — no behavior change.

use super::*;

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::Expr;
use crate::lower_decl::lower_class_from_ast;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::expr_new_builtins::{global_member_constructor_name, module_constructor_name};
use super::super::{lower_expr, LoweringContext};

/// Collect the compile-time-constant string fragments of a `+`-concatenation
/// (or template) expression, skipping any dynamic operands. Used to recognize a
/// runtime-constructed `new Function` body by its constant skeleton.
pub(crate) fn collect_const_string_parts(e: &ast::Expr, out: &mut String) {
    match e {
        ast::Expr::Lit(ast::Lit::Str(s)) => out.push_str(s.value.as_str().unwrap_or("")),
        ast::Expr::Bin(b) if b.op == ast::BinaryOp::Add => {
            collect_const_string_parts(&b.left, out);
            collect_const_string_parts(&b.right, out);
        }
        ast::Expr::Paren(p) => collect_const_string_parts(&p.expr, out),
        ast::Expr::Tpl(t) => {
            for q in &t.quasis {
                out.push_str(q.raw.as_str());
            }
        }
        // Dynamic operand (an identifier, call, etc.) — skip it.
        _ => {}
    }
}

/// Recognize depd's `wrapfunction` deprecation-wrapper shape:
/// `new Function("fn","log","deprecate","message","site",
///   '"use strict"\n'+"return function ("+a+") {"+
///   "log.call(deprecate, message, site)\n"+"return fn.apply(this, arguments)\n"+"}")`.
/// The five param-name args are constant string literals; only the body
/// (last arg) is runtime-constructed. The runtime `js_function_ctor_from_strings`
/// re-verifies the full template and returns the wrapped fn, so matching here
/// lets the site proceed to that recognizer instead of being deferred to a
/// throw-on-call value (which `send` invokes eagerly at Next.js startup).
pub(crate) fn is_depd_wrapfunction_shape(args: &[ast::ExprOrSpread]) -> bool {
    if args.len() != 6 {
        return false;
    }
    const PARAM_NAMES: [&str; 5] = ["fn", "log", "deprecate", "message", "site"];
    for (i, name) in PARAM_NAMES.iter().enumerate() {
        if args[i].spread.is_some() {
            return false;
        }
        match crate::eval_classifier::const_string_of(&args[i].expr) {
            Some(s) if s == *name => {}
            _ => return false,
        }
    }
    if args[5].spread.is_some() {
        return false;
    }
    let mut body = String::new();
    collect_const_string_parts(&args[5].expr, &mut body);
    body.contains("return function (")
        && body.contains("log.call(deprecate, message, site)")
        && body.contains("return fn.apply(this, arguments)")
}

/// Lower `new TextDecoder(label?, { fatal?, ignoreBOM? })` into
/// `Expr::TextDecoderNew { label, fatal, ignore_bom }`. Shared by
/// `expr_new.rs` (bound to a local) and `textencoder.rs` (inline
/// `new TextDecoder(...).decode(...)`).
pub(crate) fn lower_text_decoder_new(
    ctx: &mut LoweringContext,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Expr> {
    let label = match args.and_then(|a| a.first()) {
        Some(arg) => lower_expr(ctx, &arg.expr)?,
        None => Expr::Undefined,
    };
    let mut fatal = Expr::Bool(false);
    let mut ignore_bom = Expr::Bool(false);
    if let Some(opts) = args.and_then(|a| a.get(1)) {
        if let ast::Expr::Object(obj) = opts.expr.as_ref() {
            for prop in &obj.props {
                if let ast::PropOrSpread::Prop(p) = prop {
                    if let ast::Prop::KeyValue(kv) = p.as_ref() {
                        let key = match &kv.key {
                            ast::PropName::Ident(i) => i.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            _ => continue,
                        };
                        match key.as_str() {
                            "fatal" => fatal = lower_expr(ctx, &kv.value)?,
                            "ignoreBOM" => ignore_bom = lower_expr(ctx, &kv.value)?,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(Expr::TextDecoderNew {
        label: Box::new(label),
        fatal: Box::new(fatal),
        ignore_bom: Box::new(ignore_bom),
    })
}

pub(crate) fn peel_new_callee(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        match expr {
            ast::Expr::Paren(paren) => expr = paren.expr.as_ref(),
            ast::Expr::TsAs(ts_as) => expr = ts_as.expr.as_ref(),
            ast::Expr::TsTypeAssertion(ts_ta) => expr = ts_ta.expr.as_ref(),
            ast::Expr::TsNonNull(ts_non_null) => expr = ts_non_null.expr.as_ref(),
            ast::Expr::TsConstAssertion(ts_const) => expr = ts_const.expr.as_ref(),
            _ => return expr,
        }
    }
}

pub(crate) fn nonconstructable_builtin_throw_expr(name: &str, mut args: Vec<Expr>) -> Expr {
    let helper = match name {
        "Symbol" => "js_throw_symbol_constructor_type_error",
        "BigInt" => "js_throw_bigint_constructor_type_error",
        "Math" => "js_throw_math_constructor_type_error",
        _ => unreachable!(),
    };
    let throw_expr = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: helper.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    };

    if args.is_empty() {
        throw_expr
    } else {
        args.push(throw_expr);
        Expr::Sequence(args)
    }
}

pub(crate) fn lower_optional_args(
    ctx: &mut LoweringContext,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Vec<Expr>> {
    args.map(|args| {
        args.iter()
            .map(|a| lower_expr(ctx, &a.expr))
            .collect::<Result<Vec<_>>>()
    })
    .transpose()
    .map(|args| args.unwrap_or_default())
}

/// Lower a `new` argument list preserving spread positions as
/// `CallArg::Spread`, for the `NewDynamicSpread` path.
pub(crate) fn lower_new_spread_args(
    ctx: &mut LoweringContext,
    args: &[ast::ExprOrSpread],
) -> Result<Vec<crate::ir::CallArg>> {
    use crate::ir::CallArg;
    args.iter()
        .map(|a| {
            let e = lower_expr(ctx, &a.expr)?;
            Ok(if a.spread.is_some() {
                CallArg::Spread(e)
            } else {
                CallArg::Expr(e)
            })
        })
        .collect()
}

/// Whether a `new` callee is a generic constructable shape that the
/// `NewDynamicSpread` path can handle: a function/class expression, an IIFE
/// (`new (function(){…})()`), or an arrow (constructing one is a `TypeError` —
/// the runtime reports it). Bare-identifier callees (user classes, native
/// module constructors, built-ins) are intentionally excluded — they keep their
/// dedicated per-constructor lowering, whose argument marshalling (rest
/// parameters, default values, …) the generic construct helper does not
/// replicate. `callee` must already be peeled (see `peel_new_callee`).
pub(crate) fn callee_is_generic_construct_shape(ctx: &LoweringContext, callee: &ast::Expr) -> bool {
    // A bare-identifier callee that resolves to a *local* binding (a parameter
    // or `let`/`const` holding a runtime constructor value, e.g. test262's
    // `checkSubclassingIgnored`'s `new construct(...constructArgs)`) has no
    // dedicated per-constructor lowering — it falls through to the generic
    // construct path, which otherwise collapses a spread into one array arg.
    // Route it through `NewDynamicSpread`. Top-level class/function names keep
    // their dedicated lowering (they aren't local bindings).
    if let ast::Expr::Ident(ident) = callee {
        if ctx.lookup_local(ident.sym.as_ref()).is_some() {
            return true;
        }
        // A bare-identifier callee naming a known USER CLASS (`class S {…}`;
        // `new S(...args)`). The dedicated static-class `new` lowering maps over
        // `a.expr` and DROPS the `a.spread` marker, collapsing `new S(...arr)`
        // into a single array argument (`this.field = arr` instead of `arr[0]`),
        // which SIGBUSes on the first method call (NestJS DI:
        // `new Provider(...resolvedDeps)`). Route it through `NewDynamicSpread`
        // so the spread positions survive; the generic apply path allocates the
        // instance with the class's registered inline-keys shape and replays the
        // constructor, so the result is identical to the fixed-arity `new S(arg)`.
        if ctx.lookup_class(ident.sym.as_ref()).is_some() {
            return true;
        }
    }
    matches!(
        callee,
        ast::Expr::Fn(_)
            | ast::Expr::Class(_)
            | ast::Expr::Arrow(_)
            | ast::Expr::Call(_)
            // Member-expression callees (`new Temporal.Duration(...args)`,
            // `new ns.Ctor(...args)`) also route through the generic
            // construct path, whose argument lowering otherwise collapses a
            // spread into a single array argument. The handful of specially
            // lowered member constructors (URL, TextEncoder, …) are never
            // invoked with a spread in practice.
            | ast::Expr::Member(_)
    )
}

pub(crate) fn lower_url_encoding_constructor(
    ctx: &mut LoweringContext,
    class_name: &str,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Option<Expr>> {
    match class_name {
        "URL" => {
            let args = lower_optional_args(ctx, args)?;
            let mut args_iter = args.into_iter();
            let url_arg = args_iter
                .next()
                .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
            let base_arg = args_iter.next();
            Ok(Some(Expr::UrlNew {
                url: Box::new(url_arg),
                base: base_arg.map(Box::new),
            }))
        }
        "URLSearchParams" => {
            let args = lower_optional_args(ctx, args)?;
            let init_arg = args.into_iter().next();
            Ok(Some(Expr::UrlSearchParamsNew(init_arg.map(Box::new))))
        }
        "URLPattern" => {
            let args = lower_optional_args(ctx, args)?;
            let mut args_iter = args.into_iter();
            let input = args_iter.next().unwrap_or(Expr::Undefined);
            let base = args_iter.next();
            Ok(Some(Expr::UrlPatternNew {
                input: Box::new(input),
                base: base.map(Box::new),
            }))
        }
        "TextEncoder" => Ok(Some(Expr::TextEncoderNew)),
        "TextDecoder" => Ok(Some(lower_text_decoder_new(ctx, args)?)),
        _ => Ok(None),
    }
}

pub(crate) fn is_url_encoding_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "URL" | "URLSearchParams" | "URLPattern" | "TextEncoder" | "TextDecoder"
    )
}

pub(crate) fn is_worker_messaging_constructor_name(name: &str) -> bool {
    matches!(name, "MessageChannel" | "BroadcastChannel")
}

pub(crate) fn lower_worker_messaging_new(
    ctx: &mut LoweringContext,
    class_name: &str,
    args: Option<&[ast::ExprOrSpread]>,
) -> Result<Expr> {
    Ok(Expr::NativeMethodCall {
        module: "worker_threads".to_string(),
        class_name: None,
        object: None,
        method: class_name.to_string(),
        args: lower_optional_args(ctx, args)?,
    })
}

pub(crate) fn lower_worker_new(ctx: &mut LoweringContext, new_expr: &ast::NewExpr) -> Result<Expr> {
    let args = new_expr
        .args
        .as_ref()
        .map(|args| {
            args.iter()
                .map(|a| lower_expr(ctx, &a.expr))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let mut args = args.into_iter();
    let filename = args.next().unwrap_or(Expr::Undefined);
    let options = args.next().map(Box::new);
    Ok(Expr::WorkerNew {
        paths: Vec::new(),
        filename: Box::new(filename),
        options,
    })
}

pub(crate) fn is_worker_threads_module_name(module_name: &str) -> bool {
    module_name == "worker_threads" || module_name == "node:worker_threads"
}

pub(crate) fn is_fetch_constructor_name(name: &str) -> bool {
    matches!(
        name,
        "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

pub(crate) fn is_global_object_expr(ctx: &LoweringContext, expr: &Expr) -> bool {
    match expr {
        Expr::GlobalGet(_) => true,
        Expr::LocalGet(id) => ctx.global_this_aliases.contains(id),
        Expr::PropertyGet { object, property } => {
            property == "globalThis" && matches!(object.as_ref(), Expr::GlobalGet(_))
        }
        _ => false,
    }
}
