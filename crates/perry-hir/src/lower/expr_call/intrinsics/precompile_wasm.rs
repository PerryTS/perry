use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

use super::super::super::{lower_expr, LoweringContext};

/// #1681 (Phase 3 of #1677) — `precompile(EXPR)` build-time intrinsic.
///
/// `precompile` marks a build-time-evaluable codegen expression: `EXPR` is
/// run **at build time** (by Perry compiling and running its own output —
/// no node, no embedded engine) and must produce a *function-source
/// string*; that source is then compiled natively and substituted for the
/// call. This is the self-hosted "evaporate dynamism at build time" path:
/// the generated function ships native, with no `new Function`/engine in
/// the binary.
///
/// Two lowering modes (set by the driver via `set_precompile_capture` /
/// `set_precompile_results`):
///   - **Capture stage** (the Stage-1 subprocess): lower to
///     `console.log("<marker>…" + JSON.stringify(EXPR))` so running the
///     produced binary emits `EXPR`'s build-time value, keyed by this call
///     site's `(source_file, span.lo)`.
///   - **Main compile**: look up the captured source for this `(file, lo)`,
///     parse it as a function expression, and lower it in place. A missing
///     result (the capture run never reached this site) is a hard error —
///     no silent fallback (acceptance criterion of #1681).
pub(crate) fn try_precompile(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
) -> Result<Option<Expr>> {
    // Bare unshadowed `precompile(<one non-spread arg>)`.
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    if ident.sym.as_ref() != "precompile"
        || ctx.lookup_local("precompile").is_some()
        || ctx.lookup_func("precompile").is_some()
        || ctx.lookup_imported_func("precompile").is_some()
        || call.args.len() != 1
        || call.args[0].spread.is_some()
    {
        return Ok(None);
    }
    let span = call.span;
    let site_lo = span.lo.0;
    let file = ctx.source_file_path.clone();

    if crate::ir::precompile_capture_enabled() {
        // Stage 1: emit `console.log("<marker>" + JSON.stringify(EXPR))`.
        // Synthesize the AST and re-dispatch through `lower_call` so the
        // normal console.log / JSON.stringify / string-concat lowerings do
        // the work. The marker carries the site key so the driver can route
        // the captured source back without depending on lowering order.
        let marker = format!("\u{1}PERRY_PRECOMPILE\u{1}{file}\u{1}{site_lo}\u{1}");
        let sctx = swc_common::SyntaxContext::empty();
        let member = |obj: &str, prop: &str| {
            ast::Expr::Member(ast::MemberExpr {
                span,
                obj: Box::new(ast::Expr::Ident(ast::Ident::new(obj.into(), span, sctx))),
                prop: ast::MemberProp::Ident(ast::IdentName {
                    span,
                    sym: prop.into(),
                }),
            })
        };
        // JSON.stringify(EXPR)
        let mut json_call = call.clone();
        json_call.callee = ast::Callee::Expr(Box::new(member("JSON", "stringify")));
        json_call.args = vec![call.args[0].clone()];
        // "<marker>" + JSON.stringify(EXPR)
        let concat = ast::Expr::Bin(ast::BinExpr {
            span,
            op: ast::BinaryOp::Add,
            left: Box::new(ast::Expr::Lit(ast::Lit::Str(ast::Str {
                span,
                value: marker.into(),
                raw: None,
            }))),
            right: Box::new(ast::Expr::Call(json_call)),
        });
        // console.log(<concat>)
        let mut log_call = call.clone();
        log_call.callee = ast::Callee::Expr(Box::new(member("console", "log")));
        log_call.args = vec![ast::ExprOrSpread {
            spread: None,
            expr: Box::new(concat),
        }];
        return Ok(Some(super::super::lower_call(ctx, &log_call)?));
    }

    // Main compile: substitute the captured generated function.
    match crate::ir::precompile_result_at(&file, site_lo) {
        Some(src) => Ok(Some(lower_precompiled_source(ctx, &src, span)?)),
        None => {
            crate::lower_bail!(
                span,
                "`precompile(...)` produced no build-time result for this call site \
                 ({}:{}). The build-time capture run did not reach it — its argument \
                 must be evaluable at build time and produce a function-source string. \
                 (#1681)",
                file,
                site_lo,
            );
        }
    }
}

/// Parse a build-time-captured function-source string (e.g. `"(a) => a + 3"`
/// or `"function (a) { return a }"`) and lower it as an ordinary function
/// expression — the same path the Phase 1 const-fold uses.
fn lower_precompiled_source(
    ctx: &mut LoweringContext,
    src: &str,
    span: swc_common::Span,
) -> Result<Expr> {
    let wrapped = format!("({src});\n");
    let module = perry_parser::parse_typescript(&wrapped, "<precompiled>").map_err(|e| {
        anyhow::Error::new(crate::error::LowerError::new(
            format!(
                "build-time `precompile` result is not a valid function expression: {e} \
                 (#1681)\n  source: {src:?}"
            ),
            span,
        ))
    })?;
    let fn_expr = module
        .body
        .first()
        .and_then(|item| match item {
            ast::ModuleItem::Stmt(ast::Stmt::Expr(es)) => Some(es.expr.as_ref()),
            _ => None,
        })
        .map(|mut e| {
            while let ast::Expr::Paren(p) = e {
                e = p.expr.as_ref();
            }
            e
        });
    match fn_expr {
        Some(e @ (ast::Expr::Fn(_) | ast::Expr::Arrow(_))) => lower_expr(ctx, e),
        _ => crate::lower_bail!(
            span,
            "build-time `precompile` result must be a function expression (#1681)\n  source: {src:?}"
        ),
    }
}

/// Issue #76 — `embedWasm("./file.wasm")` from `perry/build` is a
/// compile-time intrinsic that bakes the file's bytes directly into the
/// produced binary. Resolves the path relative to the current source
/// file (matches the maintainer's preferred MVP shape vs. the in-flight
/// import-attributes proposal). The argument MUST be a string literal —
/// dynamic paths defeat the whole purpose. Unknown failure (file not
/// found, etc.) bails the compile with a clear error.
pub(crate) fn try_embed_wasm(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<Option<Expr>> {
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
            if ident.sym.as_ref() == "embedWasm"
                && ctx.lookup_local("embedWasm").is_none()
                && ctx.lookup_func("embedWasm").is_none()
                && call.args.len() == 1
                && call.args[0].spread.is_none()
            {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() {
                    let rel: String = s.value.as_str().unwrap_or("").to_string();
                    let base_dir = std::path::Path::new(&ctx.source_file_path)
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."));
                    let resolved = base_dir.join(&rel);
                    let bytes = std::fs::read(&resolved).map_err(|e| {
                        anyhow::anyhow!(
                            "embedWasm(\"{}\") failed to read {}: {}",
                            rel,
                            resolved.display(),
                            e
                        )
                    })?;
                    let elems: Vec<Expr> = bytes.iter().map(|b| Expr::Number(*b as f64)).collect();
                    return Ok(Some(Expr::Uint8ArrayNew(Some(Box::new(Expr::Array(
                        elems,
                    ))))));
                }
                crate::lower_bail!(
                    call.span,
                    "embedWasm(...) requires a string-literal path argument so the bytes can be embedded at compile time"
                );
            }
        }
    }
    Ok(None)
}
