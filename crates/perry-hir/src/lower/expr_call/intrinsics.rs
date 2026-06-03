//! Compile-time intrinsics + bare-callee CJS/UMD legacy shapes.
//!
//! Handles, in order: `require(literal)` bail, `embedWasm(literal)`,
//! the `(function(){...}).call(this, ...)` IIFE rewrite, the
//! `Function('return this')()` globalThis fold, and the
//! `RegExp(pattern, flags?)` bare-call fold.
//!
//! Each helper returns `Result<Option<Expr>>` — `Some` if it matched
//! and the caller should return that expression; `None` to fall
//! through. Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{is_known_namespace_static_function, lower_expr, LoweringContext};

/// Issue #668: AOT `require(stringLiteral)` from a user TypeScript file
/// currently lowers to `Call { callee: GlobalGet(0), ... }` (the unknown-ident
/// sentinel) and explodes at runtime as `TypeError: value is not a function`.
/// Until we wire up synthetic namespace-imports for `require(literal)`, fail
/// at compile time with a fix-it pointing at `import ...` so the user finds the
/// problem on the first build instead of the first prod request.
pub(super) fn try_require_literal_bail(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<()> {
    if let ast::Callee::Expr(callee_expr) = &call.callee {
        if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
            // Issue #668: only enforce the compile-time error for user-written
            // source files. Many published packages (e.g. `@perryts/redis`)
            // deliberately use `require(literal)` inside a method body to break
            // import cycles; those calls only execute on opt-in code paths and
            // pre-fix simply returned undefined-and-failed-at-call-time. Failing
            // them at compile time would refuse to build any consumer of those
            // packages even if the require'd path is never reached. node_modules
            // sources keep the legacy behavior (silent fall-through to the
            // unknown-callee path) until we wire up real `require(literal)`
            // lowering.
            if !ctx.is_external_module
                && ctx.optional_require_try_depth == 0
                && ident.sym.as_ref() == "require"
                && ctx.lookup_local("require").is_none()
                && ctx.lookup_func("require").is_none()
                && ctx.lookup_imported_func("require").is_none()
                && call.args.len() == 1
                && call.args[0].spread.is_none()
            {
                if let ast::Expr::Lit(ast::Lit::Str(s)) = call.args[0].expr.as_ref() {
                    let spec = s.value.as_str().unwrap_or("");
                    // #925: when we have a module-specific hint (e.g.
                    // distinguishing "this is in stdlib, just swap to
                    // ESM" from "this isn't shimmed at all"), append it.
                    let hint = super::super::unimpl_hints::require_module_hint(spec)
                        .map(|h| format!(" {h}"))
                        .unwrap_or_default();
                    crate::lower_bail!(
                        call.span,
                        "CommonJS `require(\"{}\")` is not supported under `perry compile` \
                         — use a static `import` instead \
                         (e.g. `import * as m from \"{}\"` \
                         or `import {{ x }} from \"{}\"`). Closes #668.{}",
                        spec,
                        spec,
                        spec,
                        hint,
                    );
                }
            }
        }
    }
    Ok(())
}

/// #1678 (Phase 0 of #1677) — classify a bare `Function(...)` /
/// `eval(...)` call. The `Function('return this')()` globalThis fold runs
/// before this (in `lower_call_inner`) and short-circuits, so its inner
/// `Function('return this')` never reaches here.
///
/// Returns `Err` (span-tagged) only for the runtime-unknown bucket —
/// const-foldable (string-literal body) and known-codegen-library sites
/// log under `PERRY_EVAL_DIAG` and fall through to the existing lowering
/// (a bare `Function`/`eval` ident → `GlobalGet(0)` sentinel) unchanged,
/// to be picked up by later phases. `Ok(())` means proceed.
pub(super) fn check_eval_function_call(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<()> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(());
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Ident(ident) = callee else {
        return Ok(());
    };
    let name = ident.sym.as_ref();
    let surface = match name {
        "eval" => crate::eval_classifier::EvalSurface::Eval,
        "Function" => crate::eval_classifier::EvalSurface::FunctionCall,
        _ => return Ok(()),
    };
    // A local/func/imported binding named `eval`/`Function` shadows the
    // builtin — leave those alone.
    if ctx.lookup_local(name).is_some()
        || ctx.lookup_func(name).is_some()
        || ctx.lookup_imported_func(name).is_some()
    {
        return Ok(());
    }
    // Body argument: the only arg for `eval(code)`, the last arg for
    // `Function(p1, p2, body)`. A spread in the body position yields a
    // non-constant inner expr → the classifier buckets it runtime-unknown.
    let body_arg = match surface {
        crate::eval_classifier::EvalSurface::Eval => call.args.first(),
        _ => call.args.last(),
    }
    .map(|a| a.expr.as_ref());
    crate::eval_classifier::check_site(surface, body_arg, &ctx.source_file_path, call.span)
}

pub(super) fn try_strict_eval_arguments_assignment(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
) -> Option<Expr> {
    if !ctx.current_strict_mode() || call.args.len() != 1 || call.args[0].spread.is_some() {
        return None;
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return None;
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Ident(ident) = callee else {
        return None;
    };
    if ident.sym.as_ref() != "eval"
        || ctx.lookup_local("eval").is_some()
        || ctx.lookup_func("eval").is_some()
        || ctx.lookup_imported_func("eval").is_some()
    {
        return None;
    }
    let ast::Expr::Lit(ast::Lit::Str(source)) = call.args[0].expr.as_ref() else {
        return None;
    };
    let source = source.value.as_str().unwrap_or("");
    if !strict_eval_source_assigns_arguments(source) {
        return None;
    }
    Some(Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_throw_strict_eval_arguments_syntax_error".to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
    })
}

fn strict_eval_source_assigns_arguments(source: &str) -> bool {
    let bytes = source.as_bytes();
    let needle = b"arguments";
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        let before_ok = i == 0 || !is_ident_continue(bytes[i - 1]);
        let after = i + needle.len();
        let after_ok = after == bytes.len() || !is_ident_continue(bytes[after]);
        if before_ok && after_ok {
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len()
                && bytes[j] == b'='
                && bytes.get(j + 1).copied() != Some(b'=')
                && bytes.get(j + 1).copied() != Some(b'>')
            {
                return true;
            }
        }
        i = after;
    }
    false
}

fn is_ident_continue(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}

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
pub(super) fn try_precompile(
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
        return Ok(Some(super::lower_call(ctx, &log_call)?));
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
pub(super) fn try_embed_wasm(ctx: &LoweringContext, call: &ast::CallExpr) -> Result<Option<Expr>> {
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

fn pod_layout_intrinsic_is_shadowed(ctx: &LoweringContext, name: &str) -> bool {
    ctx.lookup_local(name).is_some()
        || ctx.lookup_func(name).is_some()
        || ctx.lookup_imported_func(name).is_some()
}

fn explicit_single_type_arg(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    name: &str,
) -> Result<Type> {
    let Some(type_args) = call.type_args.as_ref() else {
        crate::lower_bail!(
            call.span,
            "{}<T>() requires exactly one explicit PerryPod type argument",
            name
        );
    };
    if type_args.params.len() != 1 {
        crate::lower_bail!(
            call.span,
            "{}<T>() requires exactly one explicit PerryPod type argument",
            name
        );
    }
    let type_arg = &type_args.params[0];
    if let Some(ty) = bare_type_param_type_arg(ctx, type_arg) {
        return Ok(ty);
    }
    Ok(extract_ts_type_with_ctx(type_arg, Some(ctx)))
}

fn bare_type_param_type_arg(ctx: &LoweringContext, type_arg: &ast::TsType) -> Option<Type> {
    let ast::TsType::TsTypeRef(type_ref) = type_arg else {
        return None;
    };
    if type_ref.type_params.is_some() {
        return None;
    }
    let ast::TsEntityName::Ident(ident) = &type_ref.type_name else {
        return None;
    };
    let name = ident.sym.to_string();
    ctx.is_type_param(&name).then_some(Type::TypeVar(name))
}

fn literal_offset_path(arg: &ast::Expr) -> Option<Vec<String>> {
    let ast::Expr::Lit(ast::Lit::Str(s)) = arg else {
        return None;
    };
    let raw = s.value.as_str().unwrap_or("");
    let path: Vec<String> = raw.split('.').map(str::to_string).collect();
    (!path.is_empty() && path.iter().all(|segment| !segment.is_empty())).then_some(path)
}

/// Public compile-time POD layout constants.
pub(super) fn try_pod_layout_constants(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let name = ident.sym.as_ref();
    if !matches!(name, "sizeof" | "alignof" | "offsetof") {
        return Ok(None);
    }
    if pod_layout_intrinsic_is_shadowed(ctx, name) {
        return Ok(None);
    }
    if has_spread {
        crate::lower_bail!(call.span, "{}(...) does not accept spread arguments", name);
    }

    let ty = explicit_single_type_arg(ctx, call, name)?;
    match name {
        "sizeof" => {
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "sizeof<T>() expects no arguments");
            }
            Ok(Some(Expr::PodLayoutSizeOf { ty }))
        }
        "alignof" => {
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "alignof<T>() expects no arguments");
            }
            Ok(Some(Expr::PodLayoutAlignOf { ty }))
        }
        "offsetof" => {
            if call.args.len() != 1 {
                crate::lower_bail!(
                    call.span,
                    "offsetof<T>(field) expects exactly one string-literal field path"
                );
            }
            let Some(field_path) = literal_offset_path(call.args[0].expr.as_ref()) else {
                crate::lower_bail!(
                    call.span,
                    "offsetof<T>(field) requires a compile-time string-literal field path"
                );
            };
            Ok(Some(Expr::PodLayoutOffsetOf { ty, field_path }))
        }
        _ => Ok(None),
    }
}

fn native_arena_hidden_kind_from_expr(expr: &ast::Expr) -> Option<u8> {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            crate::ir::typed_array_kind_for_name(s.value.as_str().unwrap_or(""))
        }
        ast::Expr::Lit(ast::Lit::Num(n)) if n.value.fract() == 0.0 => {
            let raw = n.value as i64;
            (0..=crate::ir::TYPED_ARRAY_KIND_BIGUINT64 as i64)
                .contains(&raw)
                .then_some(raw as u8)
        }
        _ => None,
    }
}

fn native_arena_public_kind_from_expr(ctx: &LoweringContext, expr: &ast::Expr) -> Option<u8> {
    match expr {
        ast::Expr::Lit(ast::Lit::Str(s)) => {
            crate::ir::typed_array_kind_for_name(s.value.as_str().unwrap_or(""))
        }
        ast::Expr::Ident(ident)
            if ctx.lookup_local(ident.sym.as_ref()).is_none()
                && ctx.lookup_func(ident.sym.as_ref()).is_none()
                && ctx.lookup_imported_func(ident.sym.as_ref()).is_none()
                && ctx.lookup_class(ident.sym.as_ref()).is_none() =>
        {
            crate::ir::typed_array_kind_for_name(ident.sym.as_ref())
        }
        ast::Expr::Paren(paren) => native_arena_public_kind_from_expr(ctx, &paren.expr),
        ast::Expr::TsAs(ts_as) => native_arena_public_kind_from_expr(ctx, &ts_as.expr),
        ast::Expr::TsTypeAssertion(ts_assert) => {
            native_arena_public_kind_from_expr(ctx, &ts_assert.expr)
        }
        ast::Expr::TsNonNull(non_null) => native_arena_public_kind_from_expr(ctx, &non_null.expr),
        ast::Expr::TsConstAssertion(const_assert) => {
            native_arena_public_kind_from_expr(ctx, &const_assert.expr)
        }
        _ => None,
    }
}

fn native_arena_global_is_shadowed(ctx: &LoweringContext) -> bool {
    ctx.lookup_local("NativeArena").is_some()
        || ctx.lookup_func("NativeArena").is_some()
        || ctx.lookup_imported_func("NativeArena").is_some()
        || ctx.lookup_class("NativeArena").is_some()
}

fn native_memory_global_is_shadowed(ctx: &LoweringContext) -> bool {
    ctx.lookup_local("NativeMemory").is_some()
        || ctx.lookup_func("NativeMemory").is_some()
        || ctx.lookup_imported_func("NativeMemory").is_some()
        || ctx.lookup_class("NativeMemory").is_some()
}

pub(super) fn try_native_memory_public_api(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    if !matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeMemory")
        || native_memory_global_is_shadowed(ctx)
    {
        return Ok(None);
    }

    match prop.sym.as_ref() {
        "fillU32" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.fillU32(view, value) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.fillU32(view, value) expects exactly two arguments"
                );
            }
            Ok(Some(Expr::NativeMemoryFillU32 {
                view: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                value: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            }))
        }
        "copy" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.copy(dst, src) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeMemory.copy(dst, src) expects exactly two arguments"
                );
            }
            Ok(Some(Expr::NativeMemoryCopy {
                dst: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                src: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            }))
        }
        _ => Ok(None),
    }
}

fn is_native_arena_alloc_call(ctx: &LoweringContext, call: &ast::CallExpr) -> bool {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return false;
    };
    matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeArena")
        && matches!(&member.prop, ast::MemberProp::Ident(prop) if prop.sym.as_ref() == "alloc")
        && !native_arena_global_is_shadowed(ctx)
}

fn native_arena_owner_type(ty: &perry_types::Type) -> bool {
    matches!(ty, perry_types::Type::Named(name) if name == "NativeArena" || name == "NativeArenaOwner")
}

fn is_native_arena_owner_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ctx
            .lookup_local_type(ident.sym.as_ref())
            .is_some_and(native_arena_owner_type),
        ast::Expr::Call(call) => is_native_arena_alloc_call(ctx, call),
        ast::Expr::Paren(paren) => is_native_arena_owner_expr(ctx, &paren.expr),
        ast::Expr::TsAs(ts_as) => is_native_arena_owner_expr(ctx, &ts_as.expr),
        ast::Expr::TsTypeAssertion(ts_assert) => is_native_arena_owner_expr(ctx, &ts_assert.expr),
        ast::Expr::TsNonNull(non_null) => is_native_arena_owner_expr(ctx, &non_null.expr),
        ast::Expr::TsConstAssertion(const_assert) => {
            is_native_arena_owner_expr(ctx, &const_assert.expr)
        }
        _ => false,
    }
}

/// Public compile-time NativeArena API. The runtime still exposes only the
/// internal helpers; these direct dot-call shapes lower to the same HIR nodes.
pub(super) fn try_native_arena_public_api(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Member(member) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    let method = prop.sym.as_ref();

    if matches!(member.obj.as_ref(), ast::Expr::Ident(obj) if obj.sym.as_ref() == "NativeArena") {
        if method != "alloc" || native_arena_global_is_shadowed(ctx) {
            return Ok(None);
        }
        if has_spread {
            crate::lower_bail!(
                call.span,
                "NativeArena.alloc(byteLength) does not accept spread arguments"
            );
        }
        if call.args.len() != 1 {
            crate::lower_bail!(
                call.span,
                "NativeArena.alloc(byteLength) expects exactly one argument"
            );
        }
        return Ok(Some(Expr::NativeArenaAlloc(Box::new(lower_expr(
            ctx,
            &call.args[0].expr,
        )?))));
    }

    if !is_native_arena_owner_expr(ctx, member.obj.as_ref()) {
        return Ok(None);
    }

    match method {
        "view" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view(kind, byteOffset, length) does not accept spread arguments"
                );
            }
            if call.args.len() != 3 {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view(kind, byteOffset, length) expects exactly three arguments"
                );
            }
            let Some(kind) = native_arena_public_kind_from_expr(ctx, call.args[0].expr.as_ref())
            else {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.view kind must be a typed-array constructor or string literal"
                );
            };
            Ok(Some(Expr::NativeArenaView {
                owner: Box::new(lower_expr(ctx, member.obj.as_ref())?),
                kind,
                byte_offset: Box::new(lower_expr(ctx, &call.args[1].expr)?),
                length: Box::new(lower_expr(ctx, &call.args[2].expr)?),
            }))
        }
        "podView" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.podView(byteOffset, count) does not accept spread arguments"
                );
            }
            if call.args.len() != 2 {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.podView(byteOffset, count) expects exactly two arguments"
                );
            }
            let view_type = match call.type_args.as_ref() {
                Some(type_args) if type_args.params.len() == 1 => {
                    let type_arg = &type_args.params[0];
                    let pod_ty = bare_type_param_type_arg(ctx, type_arg)
                        .unwrap_or_else(|| extract_ts_type_with_ctx(type_arg, Some(ctx)));
                    Some(Type::Generic {
                        base: "PerryPodView".to_string(),
                        type_args: vec![pod_ty],
                    })
                }
                Some(_) => {
                    crate::lower_bail!(
                        call.span,
                        "NativeArena.podView<T>(byteOffset, count) expects exactly one explicit type argument"
                    );
                }
                None => None,
            };
            Ok(Some(Expr::NativePodView {
                owner: Box::new(lower_expr(ctx, member.obj.as_ref())?),
                byte_offset: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                count: Box::new(lower_expr(ctx, &call.args[1].expr)?),
                view_type,
            }))
        }
        "dispose" => {
            if has_spread {
                crate::lower_bail!(
                    call.span,
                    "NativeArena.dispose() does not accept spread arguments"
                );
            }
            if !call.args.is_empty() {
                crate::lower_bail!(call.span, "NativeArena.dispose() expects no arguments");
            }
            Ok(Some(Expr::NativeArenaDispose(Box::new(lower_expr(
                ctx,
                member.obj.as_ref(),
            )?))))
        }
        _ => Ok(None),
    }
}

/// Hidden internal native-arena intrinsics. They intentionally require the
/// view kind to be a literal so native lowering can carry width facts.
pub(super) fn try_native_arena_intrinsics(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let ast::Expr::Ident(ident) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let name = ident.sym.as_ref();
    if name == "__perry_native_pod_view" {
        if call.args.len() != 3 || call.args.iter().any(|arg| arg.spread.is_some()) {
            crate::lower_bail!(
                call.span,
                "__perry_native_pod_view(owner, byteOffset, count) expects exactly three arguments"
            );
        }
        return Ok(Some(Expr::NativePodView {
            owner: Box::new(lower_expr(ctx, &call.args[0].expr)?),
            byte_offset: Box::new(lower_expr(ctx, &call.args[1].expr)?),
            count: Box::new(lower_expr(ctx, &call.args[2].expr)?),
            view_type: None,
        }));
    }
    if !name.starts_with("__perry_native_arena_") {
        return Ok(None);
    }
    if ctx.lookup_local(name).is_some() || ctx.lookup_func(name).is_some() {
        return Ok(None);
    }
    match name {
        "__perry_native_arena_alloc" => {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_alloc(byteLength) expects exactly one argument"
                );
            }
            Ok(Some(Expr::NativeArenaAlloc(Box::new(lower_expr(
                ctx,
                &call.args[0].expr,
            )?))))
        }
        "__perry_native_arena_view" => {
            if call.args.len() != 4 || call.args.iter().any(|arg| arg.spread.is_some()) {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_view(owner, kind, byteOffset, length) expects exactly four arguments"
                );
            }
            let Some(kind) = native_arena_hidden_kind_from_expr(call.args[1].expr.as_ref()) else {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_view kind must be a typed-array name or kind literal"
                );
            };
            Ok(Some(Expr::NativeArenaView {
                owner: Box::new(lower_expr(ctx, &call.args[0].expr)?),
                kind,
                byte_offset: Box::new(lower_expr(ctx, &call.args[2].expr)?),
                length: Box::new(lower_expr(ctx, &call.args[3].expr)?),
            }))
        }
        "__perry_native_arena_dispose" => {
            if call.args.len() != 1 || call.args[0].spread.is_some() {
                crate::lower_bail!(
                    call.span,
                    "__perry_native_arena_dispose(owner) expects exactly one argument"
                );
            }
            Ok(Some(Expr::NativeArenaDispose(Box::new(lower_expr(
                ctx,
                &call.args[0].expr,
            )?))))
        }
        _ => Ok(None),
    }
}

/// Issue #957 — `(function(...) { ... }.call(<thisArg>, ...args))` IIFE
/// pattern used at the top of older CJS packages (lodash, underscore, and
/// every package that copies their UMD prelude). Pre-fix the inner
/// function expression lowers to a Closure, then `.call(thisArg, ...args)`
/// falls through to `js_native_call_method` on the closure handle which
/// doesn't recognize Function.prototype.call — the body never runs and
/// mutations to outer captures (e.g. `module.exports = _` inside the
/// wrap) are silently dropped, so `import _ from "lodash"` resolves to
/// `undefined` and `_.add` throws. Rewrite the AST shape directly to a
/// plain Call on the inner function expression, dropping the thisArg.
///
/// Conservative scope: only fires when the callee's receiver is a
/// FunctionExpression or ArrowExpression literal AND the inner function
/// does NOT reference `this` (`captures_this == false` after lowering).
/// Method dispatch like `obj.fn.call(otherObj, args)` keeps its existing
/// semantics — those go through the generic property-call path. We can
/// safely drop the thisArg because `captures_this == false` means the
/// body has no `this` references that depend on the bound value.
pub(super) fn try_iife_call_rewrite(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if !has_spread {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            if let ast::Expr::Member(member) = callee_expr.as_ref() {
                if let ast::MemberProp::Ident(prop) = &member.prop {
                    if prop.sym.as_ref() == "call" && !call.args.is_empty() {
                        // Unwrap `(`...`)` parens so `((a,b) => a+b).call(...)`
                        // matches the same shape as `(function(){...}).call(...)`.
                        let mut inner = member.obj.as_ref();
                        while let ast::Expr::Paren(p) = inner {
                            inner = p.expr.as_ref();
                        }
                        let is_fn_lit = matches!(inner, ast::Expr::Fn(_) | ast::Expr::Arrow(_));
                        if is_fn_lit {
                            let lowered_callee = lower_expr(ctx, inner)?;
                            if let Expr::Closure {
                                captures_this: false,
                                ..
                            } = &lowered_callee
                            {
                                let rest_args = call
                                    .args
                                    .iter()
                                    .skip(1)
                                    .map(|arg| lower_expr(ctx, &arg.expr))
                                    .collect::<Result<Vec<_>>>()?;
                                return Ok(Some(Expr::Call {
                                    callee: Box::new(lowered_callee),
                                    args: rest_args,
                                    type_args: Vec::new(),
                                }));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Issue #1722 — `<stdlibNamespace>.<method>.apply(thisArg, args)` /
/// `<stdlibNamespace>.<method>.call(thisArg, ...args)`.
///
/// Stdlib namespace methods (`path.join`, `fs.existsSync`, `os.platform`,
/// …) are dispatched by dedicated HIR lowerings keyed on the
/// `<namespace>.<method>(...)` *direct-call* shape — `path.join(a, b)`
/// folds to `Expr::PathJoin`, etc. The bare value `path.join` lowers to a
/// runtime namespace-property read that returns `undefined` for methods
/// not on the callable-export whitelist, so invoking it *indirectly* via
/// `Function.prototype.apply` / `.call` never reaches the native impl and
/// silently evaluates to `undefined` (Node returns the real result).
/// Surfaced by the #800 node-core radar (`test-path-join.js` uses
/// `path.join.apply(...)`).
///
/// Fix: when the callee is exactly `<ns>.<method>.{apply,call}` and `<ns>`
/// is a known native-module namespace binding (so `this` is irrelevant —
/// these are plain free functions), rewrite the AST to the equivalent
/// direct call and re-dispatch through `lower_call`, reusing every
/// existing per-method lowering. `thisArg` is dropped (correct for
/// namespace functions, which ignore `this`).
///
/// Conservative scope:
///   - `.call(thisArg, a, b, …)`         → `ns.method(a, b, …)`
///   - `.apply(thisArg)` / `.apply()`    → `ns.method()`
///   - `.apply(thisArg, [a, b, …])`      → `ns.method(a, b, …)` — only for
///     a clean array *literal* (no holes, no element spreads).
/// A non-literal apply-args array (a variable / call result) can't be
/// statically expanded into positional args, so it falls through
/// unchanged (the runtime spread path `ns.method(...arr)` is a separate
/// gap). The namespace-binding guard keeps this away from `obj.fn.call(…)`
/// method dispatch, function-literal IIFEs (`try_iife_call_rewrite`), and
/// `Object.prototype.<m>.call(…)` (`try_object_prototype_call`).
pub(super) fn try_native_module_method_apply_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    // Outer member: `<inner>.apply` / `<inner>.call`.
    let ast::Expr::Member(outer) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(outer_prop) = &outer.prop else {
        return Ok(None);
    };
    let is_apply = match outer_prop.sym.as_ref() {
        "apply" => true,
        "call" => false,
        _ => return Ok(None),
    };
    // Inner member: `<ns>.<method>` where `<ns>` is a native-module
    // namespace ident and `<method>` is a plain (non-computed) name.
    let ast::Expr::Member(inner) = outer.obj.as_ref() else {
        return Ok(None);
    };
    if !matches!(&inner.prop, ast::MemberProp::Ident(_)) {
        return Ok(None);
    }
    let ast::Expr::Ident(ns_id) = inner.obj.as_ref() else {
        return Ok(None);
    };
    let ns_name = ns_id.sym.as_ref();
    // Namespace bindings register both an alias (require / `import * as`)
    // and a `(module, None)` native-module entry; named imports register
    // `(module, Some(symbol))` and must NOT match here.
    let is_module_ns = ctx.lookup_builtin_module_alias(ns_name).is_some()
        || matches!(ctx.lookup_native_module(ns_name), Some((_, None)));
    if !is_module_ns {
        return Ok(None);
    }

    // Build the synthesized direct-call argument list at the AST level.
    let synth_args: Vec<ast::ExprOrSpread> = if is_apply {
        match call.args.get(1) {
            // `.apply(thisArg)` / `.apply()` → no positional args.
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    // Only a clean literal (no holes, no element spreads)
                    // can be expanded into positional args statically.
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return Ok(None);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                // Non-literal args array — can't statically expand.
                _ => return Ok(None),
            },
        }
    } else {
        // `.call(thisArg, a, b, …)` → drop thisArg, keep the rest.
        call.args.iter().skip(1).cloned().collect()
    };

    // Synthesize `<ns>.<method>(synth_args)` and re-dispatch. The new
    // callee carries no `.apply`/`.call`, so this hook can't re-match it.
    let mut synth_call = call.clone();
    synth_call.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    synth_call.args = synth_args;
    Ok(Some(super::lower_call(ctx, &synth_call)?))
}

/// Issue #1777 — `<builtinProto>.<method>.{call,apply}(thisArg, …)` where the
/// receiver is a **builtin prototype** (`Array.prototype`, `String.prototype`,
/// …) or an array/string literal (`[].slice.call(…)`, `"".charAt.call(…)`).
///
/// This is the general case of #1722. A builtin prototype method read as a
/// *value* — `Array.prototype.slice`, `[].slice` — lowers to `undefined`, so
/// `Array.prototype.slice.call(arguments, 1)` / `[].slice.call(arguments)`
/// throws `TypeError: Cannot read properties of undefined (reading 'call')`.
/// The arguments-to-array idiom (`[].slice.call(arguments)`) and prototype
/// borrowing (`Array.prototype.map.call(arrayLike, fn)`) are pervasive in
/// real-world JS and in the node-core test harness (`mustCall`/`mustSucceed`),
/// the single largest runtime-fail cluster in the #800 radar.
///
/// Unlike the namespace case (#1722, where `this` is irrelevant), here the
/// first argument **is** the receiver: `Proto.method.call(thisArg, ...rest)`
/// is semantically `thisArg.method(...rest)`. We rewrite to that direct
/// member call and re-dispatch through `lower_call`, so the normal
/// type-directed method dispatch picks the right native impl based on
/// `thisArg`'s runtime value (perry materializes `arguments` as a real
/// array, so `arguments.slice(1)` dispatches to Array.prototype.slice — the
/// exact behavior the idiom wants).
///
/// Conservative scope:
///   - `.call(thisArg, a, b, …)`        → `thisArg.method(a, b, …)`
///   - `.apply(thisArg)` / `.apply()`   → `thisArg.method()`
///   - `.apply(thisArg, [a, b, …])`     → `thisArg.method(a, b, …)` — only a
///     clean array *literal* (no holes/spreads); a non-literal apply-args
///     array can't be statically expanded, so it falls through unchanged.
///
/// `Object.prototype.{toString,hasOwnProperty}.call(…)` is intentionally NOT
/// matched here — the post-args hooks `try_object_prototype_call` /
/// `try_object_has_own_call` rewrite those to dedicated runtime helpers
/// (`js_object_to_string` / `js_object_has_own`), so `Object.prototype` is
/// excluded from the receiver guard below to preserve that path. This hook
/// only ever fires on a shape that currently *throws* (the method value reads
/// `undefined`), so it cannot regress working code.
/// #4101: is `expr` the member expression `Function.prototype`? Used to keep
/// `Function.prototype.toString.call(x)` from folding into `x.toString()` so
/// the runtime brand check (throw on non-function `this`) still fires.
fn is_function_prototype_member(expr: &ast::Expr) -> bool {
    let ast::Expr::Member(member) = expr else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    if prop.sym.as_ref() != "prototype" {
        return false;
    }
    matches!(member.obj.as_ref(), ast::Expr::Ident(id) if id.sym.as_ref() == "Function")
}

/// #4100: true when `recv.<method>` is a primitive-wrapper prototype method that
/// performs a spec `this` brand check at runtime (throws `TypeError` on an
/// incompatible receiver). Folding `<recv>.<method>.call(x)` into `x.<method>()`
/// would route through the lenient codegen fast-path / `Object.prototype`
/// fallback (returns `"[object Object]"`, no throw). Keeping it reflective lets
/// the installed brand-check thunk run. `Number.prototype.toFixed`/
/// `toExponential`/`toPrecision` are deliberately excluded — the fold is the
/// *correct* path for those (their reflective dispatch over-throws on a valid
/// receiver), and only the brand-checked `valueOf`/`toString`/`toLocaleString`
/// methods are affected. Symbol/BigInt have no codegen fold path, so they need
/// no guard here.
fn is_primitive_wrapper_brand_method(recv: &ast::Expr, method: &str) -> bool {
    let ast::Expr::Member(member) = recv else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return false;
    };
    if prop.sym.as_ref() != "prototype" {
        return false;
    }
    let ast::Expr::Ident(base) = member.obj.as_ref() else {
        return false;
    };
    match base.sym.as_ref() {
        "Number" => matches!(method, "valueOf" | "toString" | "toLocaleString"),
        "Boolean" => matches!(method, "valueOf" | "toString"),
        _ => false,
    }
}

pub(super) fn try_builtin_prototype_method_apply_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    // Outer member: `<inner>.apply` / `<inner>.call`.
    let ast::Expr::Member(outer) = callee_expr.as_ref() else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(outer_prop) = &outer.prop else {
        return Ok(None);
    };
    let is_apply = match outer_prop.sym.as_ref() {
        "apply" => true,
        "call" => false,
        _ => return Ok(None),
    };
    // Resolve the builtin prototype method name from the thing we're calling
    // `.call`/`.apply` ON. Two shapes are supported:
    //   * `<recv>.<method>.call(...)` — a member whose object is a builtin
    //     prototype receiver (array/string literal or `<Ctor>.prototype`).
    //   * `local.call(...)` — an identifier previously bound to such a method
    //     ref, e.g. `const m = [].map` (#3144).
    // `method_prop` is the `IdentName` for the resolved method; we reuse it as
    // the synthesized member's `.prop`.
    let method_prop: ast::IdentName = match outer.obj.as_ref() {
        ast::Expr::Member(inner) => {
            let ast::MemberProp::Ident(method_ident) = &inner.prop else {
                return Ok(None);
            };
            if !is_builtin_prototype_receiver(ctx, inner.obj.as_ref()) {
                return Ok(None);
            }
            // #4101: keep `Function.prototype.toString.call(x)` reflective so
            // the runtime thunk runs its brand check (throw a TypeError on a
            // non-function `this`) and reconstructs source. Folding it to
            // `x.toString()` would erase the Function brand and route through
            // the lenient universal `toString` (returns "[object Object]", no
            // throw). `Object.prototype.toString.call(x)` is unaffected — it
            // keeps folding (ramda relies on it).
            if method_ident.sym.as_ref() == "toString"
                && is_function_prototype_member(inner.obj.as_ref())
            {
                return Ok(None);
            }
            // #4100: keep `Number.prototype.valueOf.call(x)` /
            // `Boolean.prototype.toString.call(x)` reflective so the installed
            // brand-check thunk runs (throws a `TypeError` on an incompatible
            // `this`). Folding to `x.<method>()` routes through the lenient
            // `Object.prototype` fallback (`"[object Object]"`, no throw).
            if is_primitive_wrapper_brand_method(inner.obj.as_ref(), method_ident.sym.as_ref()) {
                return Ok(None);
            }
            method_ident.clone()
        }
        ast::Expr::Ident(id) => match ctx.builtin_proto_method_locals.get(id.sym.as_ref()) {
            Some(name) => {
                // Build the method `.prop` IdentName by cloning the outer
                // `.call`/`.apply` IdentName and overwriting its `sym`
                // (avoids needing a synthetic span).
                let mut prop = outer_prop.clone();
                prop.sym = name.as_str().into();
                prop
            }
            // Not a tracked builtin-method local: leave unrelated
            // `someFn.call(...)` untouched.
            None => return Ok(None),
        },
        _ => return Ok(None),
    };

    // `.call`/`.apply` need at least the `thisArg` (the new receiver). A
    // spread in the `thisArg` slot can't be statically resolved to a receiver.
    let Some(this_arg) = call.args.first() else {
        return Ok(None);
    };
    if this_arg.spread.is_some() {
        return Ok(None);
    }
    let this_arg = this_arg.clone();

    // Build the synthesized positional argument list (everything after thisArg).
    let rest_args: Vec<ast::ExprOrSpread> = if is_apply {
        match call.args.get(1) {
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return Ok(None);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                // Non-literal apply-args array — can't statically expand.
                _ => return Ok(None),
            },
        }
    } else {
        call.args.iter().skip(1).cloned().collect()
    };

    // Synthesize `(thisArg).<method>(rest_args)`: use the resolved method
    // name, make the receiver the real `thisArg`, drop the `.apply`/`.call`
    // wrapper, and re-dispatch.
    let synth_member = ast::MemberExpr {
        span: outer.span,
        obj: this_arg.expr.clone(),
        prop: ast::MemberProp::Ident(method_prop),
    };
    let mut synth_call = call.clone();
    synth_call.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(synth_member)));
    synth_call.args = rest_args;
    Ok(Some(super::lower_call(ctx, &synth_call)?))
}

/// #3144: if `init` is a value-read of a builtin prototype method whose
/// receiver passes [`is_builtin_prototype_receiver`] (e.g. `[].map`,
/// `"".slice`, `Array.prototype.filter`), return the method name. Used to
/// track locals like `const m = [].map` so a later `m.call(arr, ...)` /
/// `m.apply(arr, [...])` can be rewritten to a direct call.
pub(crate) fn as_builtin_proto_method_ref(
    ctx: &LoweringContext,
    init: &ast::Expr,
) -> Option<String> {
    let ast::Expr::Member(member) = init else {
        return None;
    };
    let ast::MemberProp::Ident(method) = &member.prop else {
        return None;
    };
    if !is_builtin_prototype_receiver(ctx, &member.obj) {
        return None;
    }
    // #4100: don't track `const v = Number.prototype.valueOf` for the fold —
    // a later `v.call(x)` must stay reflective so the brand-check thunk runs
    // (see `is_primitive_wrapper_brand_method`). Untracked, the value read goes
    // through the reflective dispatch, which throws correctly.
    if is_primitive_wrapper_brand_method(&member.obj, method.sym.as_ref()) {
        return None;
    }
    // For a `<Ctor>.prototype` receiver, any method ident is accepted (mirrors
    // the existing `.call`/`.apply` rewrite, which doesn't gate on the method
    // name). For an array/string literal receiver, gate on the known
    // array/string prototype-method predicates so we don't track unrelated
    // member reads.
    let is_proto_base = matches!(&*member.obj, ast::Expr::Member(_));
    let known = crate::lower::array_fold::is_known_array_prototype_method(method.sym.as_ref())
        || crate::lower::array_fold::is_known_string_prototype_method(method.sym.as_ref());
    if is_proto_base || known {
        Some(method.sym.to_string())
    } else {
        None
    }
}

/// True when `recv` is a builtin constructor's `.prototype` (and that
/// constructor name is not shadowed by a local/function binding) or an
/// array/string literal — the receiver shapes whose prototype-method *values*
/// currently lower to `undefined`. `Object` is deliberately excluded; see
/// `try_builtin_prototype_method_apply_call`.
fn is_builtin_prototype_receiver(ctx: &LoweringContext, recv: &ast::Expr) -> bool {
    match recv {
        // `Array.prototype` / `String.prototype` / … (not `Object`).
        ast::Expr::Member(m) => {
            let ast::MemberProp::Ident(p) = &m.prop else {
                return false;
            };
            if p.sym.as_ref() != "prototype" {
                return false;
            }
            let ast::Expr::Ident(base) = m.obj.as_ref() else {
                return false;
            };
            let name = base.sym.as_ref();
            // Number/Boolean primitive methods need to stay reflective so
            // their prototype thunks brand-check `this` (#4100).
            matches!(name, "Array" | "String" | "Function")
                && ctx.lookup_local(name).is_none()
                && ctx.lookup_func(name).is_none()
        }
        // `[].slice.call(…)` / `[1,2,3].map.call(…)`.
        ast::Expr::Array(_) => true,
        // `"".charAt.call(…)`.
        ast::Expr::Lit(ast::Lit::Str(_)) => true,
        _ => false,
    }
}

/// #2143 — namespace-static `.bind`/`.call`/`.apply` immediate-call rewrites.
///
/// Built-in function values like `Promise.resolve`, `Math.min`, `JSON.parse`
/// do not inherit `Function.prototype` in Perry's representation (each direct
/// call site is special-cased in codegen — there's no reified function value
/// to hang `.call`/`.apply`/`.bind` off). The bare value-read lowers to a
/// numeric fallback, so `Promise.resolve.bind(Promise)(x)` throws
/// "value is not a function" at the outer call.
///
/// Rewrite at the AST level for the shapes whose intent is unambiguous and
/// where the `thisArg` is irrelevant (namespace statics don't read `this`):
///
///   `<NS>.<static>.call(thisArg, a, b, …)`     → `<NS>.<static>(a, b, …)`
///   `<NS>.<static>.apply(thisArg)`             → `<NS>.<static>()`
///   `<NS>.<static>.apply(thisArg, [a, b, …])`  → `<NS>.<static>(a, b, …)`
///   `<NS>.<static>.bind(thisArg, …pre)(…rest)` → `<NS>.<static>(…pre, …rest)`
///
/// The deferred-bind shape (`const f = Promise.resolve.bind(Promise);
/// f(x);`) cannot be rewritten purely at the AST level — that needs a
/// real reified function value and is tracked as follow-up.
pub(super) fn try_namespace_static_method_apply_call_bind(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };

    // Form A/B: `<NS>.<static>.call(…)` or `.apply(…)`.
    if let ast::Expr::Member(outer) = callee_expr.as_ref() {
        if let ast::MemberProp::Ident(outer_prop) = &outer.prop {
            let mode = match outer_prop.sym.as_ref() {
                "call" => Some(false),
                "apply" => Some(true),
                _ => None,
            };
            if let Some(is_apply) = mode {
                if let Some(inner) = match_namespace_static_member(ctx, outer.obj.as_ref()) {
                    return rewrite_dropping_this(ctx, call, &inner, is_apply);
                }
            }
        }
    }

    // Form C: `(<NS>.<static>.bind(thisArg, …pre))(…rest)` — the outer call's
    // callee is itself a CallExpr to `.bind`.
    if let ast::Expr::Call(bind_call) = callee_expr.as_ref() {
        if let ast::Callee::Expr(bind_callee) = &bind_call.callee {
            if let ast::Expr::Member(bind_member) = bind_callee.as_ref() {
                if let ast::MemberProp::Ident(bind_prop) = &bind_member.prop {
                    if bind_prop.sym.as_ref() == "bind" {
                        // The bind call itself can't have spreads we don't
                        // understand; require at least `thisArg`.
                        let bind_spread = bind_call.args.iter().any(|a| a.spread.is_some());
                        if !bind_spread && !bind_call.args.is_empty() {
                            if let Some(inner_member) =
                                match_namespace_static_member(ctx, bind_member.obj.as_ref())
                            {
                                // Build: <inner_member>(…preBound, …rest)
                                let pre_bound: Vec<ast::ExprOrSpread> =
                                    bind_call.args.iter().skip(1).cloned().collect();
                                let mut synth = call.clone();
                                synth.callee =
                                    ast::Callee::Expr(Box::new(ast::Expr::Member(inner_member)));
                                let mut combined = pre_bound;
                                combined.extend(call.args.iter().cloned());
                                synth.args = combined;
                                return Ok(Some(super::lower_call(ctx, &synth)?));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

/// If `expr` is `<NS>.<static>` where `<NS>` is a known namespace-static
/// holder (Promise/Math/JSON/Number/String/Object/Array) not shadowed by a
/// local, and `<static>` is a known method on it, return a clone of that
/// MemberExpr so it can be reused as the rewritten callee.
fn match_namespace_static_member(
    ctx: &LoweringContext,
    expr: &ast::Expr,
) -> Option<ast::MemberExpr> {
    let ast::Expr::Member(m) = expr else {
        return None;
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return None;
    };
    let ast::Expr::Ident(base) = m.obj.as_ref() else {
        return None;
    };
    let ns = base.sym.as_ref();
    let name = prop.sym.as_ref();
    if ctx.lookup_local(ns).is_some() || ctx.lookup_func(ns).is_some() {
        return None;
    }
    if !is_known_namespace_static_function(ns, name) {
        return None;
    }
    Some(m.clone())
}

/// Rewrite `<NS>.<static>.{call,apply}(thisArg, …)` to a direct call,
/// dropping the `thisArg` (namespace statics don't use it). For `.apply`,
/// the args array must be a clean literal.
fn rewrite_dropping_this(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    inner: &ast::MemberExpr,
    is_apply: bool,
) -> Result<Option<Expr>> {
    let mut synth = call.clone();
    synth.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    if is_apply {
        // `.apply(thisArg)` / `.apply(thisArg, [a, b, …])`.
        synth.args = match call.args.get(1) {
            None => Vec::new(),
            Some(arr_arg) => match arr_arg.expr.as_ref() {
                ast::Expr::Array(arr) => {
                    let clean = arr
                        .elems
                        .iter()
                        .all(|e| matches!(e, Some(eos) if eos.spread.is_none()));
                    if !clean {
                        return rewrite_dynamic_apply_spread(ctx, call, inner);
                    }
                    arr.elems.iter().filter_map(|e| e.clone()).collect()
                }
                _ => return rewrite_dynamic_apply_spread(ctx, call, inner),
            },
        };
    } else {
        // `.call(thisArg, …args)` — drop thisArg, keep the rest.
        synth.args = call.args.iter().skip(1).cloned().collect();
    }
    Ok(Some(super::lower_call(ctx, &synth)?))
}

fn rewrite_dynamic_apply_spread(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    inner: &ast::MemberExpr,
) -> Result<Option<Expr>> {
    if !namespace_static_supports_dynamic_apply_spread(inner) {
        return Ok(None);
    }
    let Some(arg_array) = call.args.get(1) else {
        return Ok(Some(super::lower_call(
            ctx,
            &ast::CallExpr {
                callee: ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone()))),
                args: Vec::new(),
                ..call.clone()
            },
        )?));
    };
    let mut synth = call.clone();
    synth.callee = ast::Callee::Expr(Box::new(ast::Expr::Member(inner.clone())));
    synth.args = vec![ast::ExprOrSpread {
        spread: Some(call.span),
        expr: arg_array.expr.clone(),
    }];
    Ok(Some(super::lower_call(ctx, &synth)?))
}

fn namespace_static_supports_dynamic_apply_spread(inner: &ast::MemberExpr) -> bool {
    let ast::Expr::Ident(base) = inner.obj.as_ref() else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &inner.prop else {
        return false;
    };
    matches!(
        (base.sym.as_ref(), prop.sym.as_ref()),
        ("Math", "min" | "max") | ("String", "fromCharCode")
    )
}

/// Followup to #957 / PR #959 — `Function('return this')()`.
///
/// Every CJS/UMD-shaped library (lodash, underscore, Effect, …)
/// computes its "give me whatever the host calls `globalThis` here"
/// root with the double-call idiom:
///   var root = freeGlobal || freeSelf || Function('return this')();
/// Pre-fix the bare `Function` ident lowers to `Expr::GlobalGet(0)`
/// (the no-resolution sentinel), then the inner `Function('return this')`
/// lowers to `Call { callee: GlobalGet(0), args: [String("return this")] }`
/// which codegen treats as "call a non-callable" — the outer `()` then
/// tries to call the returned value and the closure validator throws
/// `TypeError: value is not a function` at module init, leaving the
/// import resolved to undefined.
///
/// PR #959 closed the sibling `.call(this)` IIFE bug and called this
/// one out in its commit message ("the next runtime gap"); fix here.
/// Match the full two-call shape at the AST level (the inner `Function`
/// ident still carries its name, so we can verify it really is the
/// builtin) and fold to `Expr::GlobalThisExpr`, which lowers to the
/// runtime's `js_get_global_this()` singleton — the same object
/// `globalThis[X] = V` already writes to (see #611).
///
/// Conservative: requires the LITERAL "return this" (with optional
/// semicolon / whitespace) AND the outer Call must have no args. Any
/// other `Function(...)` shape (e.g. dynamic body, real `new Function`)
/// falls through to the existing GlobalGet(0) path; arbitrary
/// `new Function(body)` is still not supported (an architectural
/// change — issue #960 / future work).
pub(super) fn try_function_return_this(
    ctx: &LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Option<Expr> {
    if !has_spread && call.args.is_empty() {
        if let ast::Callee::Expr(outer_callee) = &call.callee {
            let mut inner = outer_callee.as_ref();
            while let ast::Expr::Paren(p) = inner {
                inner = p.expr.as_ref();
            }
            if let ast::Expr::Call(inner_call) = inner {
                let inner_args_ok =
                    inner_call.args.len() == 1 && inner_call.args[0].spread.is_none();
                if inner_args_ok {
                    if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                        let mut inner_target = inner_callee.as_ref();
                        while let ast::Expr::Paren(p) = inner_target {
                            inner_target = p.expr.as_ref();
                        }
                        if let ast::Expr::Ident(ident) = inner_target {
                            if ident.sym.as_ref() == "Function"
                                && ctx.lookup_local("Function").is_none()
                                && ctx.lookup_func("Function").is_none()
                            {
                                if let ast::Expr::Lit(ast::Lit::Str(s)) =
                                    inner_call.args[0].expr.as_ref()
                                {
                                    let body = s.value.as_str().unwrap_or("").trim();
                                    let body = body.trim_end_matches(';').trim();
                                    if body == "return this" {
                                        return Some(Expr::GlobalThisExpr);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Followup to #957 / PR #959 — `RegExp(<args>)` as a bare function call.
///
/// lodash 4 builds half a dozen of these at module init:
///   var reEscapedHtml = /&(?:amp|lt|gt|quot|#39);/g,
///       reHasEscapedHtml = RegExp(reEscapedHtml.source);
/// The bare `RegExp` ident lowers to `Expr::GlobalGet(0)` (no resolved
/// value), so the function-call form dispatches through
/// `js_closure_call1` with a null closure handle and throws
/// `TypeError: value is not a function`. Fold here to
/// `Expr::RegExpDynamic` which lowers to the same `js_regexp_new`
/// runtime entrypoint the static `/foo/g` arm uses.
///
/// Conservative: only `RegExp(pattern)` and `RegExp(pattern, flags)`
/// with no spread. Any local/import named `RegExp` shadows the
/// builtin and falls through to its normal dispatch.
pub(super) fn try_bare_regexp_call(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if !has_spread && !call.args.is_empty() && call.args.len() <= 2 {
        if let ast::Callee::Expr(callee_expr) = &call.callee {
            let mut callee_inner = callee_expr.as_ref();
            while let ast::Expr::Paren(p) = callee_inner {
                callee_inner = p.expr.as_ref();
            }
            if let ast::Expr::Ident(ident) = callee_inner {
                if ident.sym.as_ref() == "RegExp"
                    && ctx.lookup_local("RegExp").is_none()
                    && ctx.lookup_func("RegExp").is_none()
                {
                    let pattern = lower_expr(ctx, &call.args[0].expr)?;
                    let flags = if call.args.len() == 2 {
                        Some(Box::new(lower_expr(ctx, &call.args[1].expr)?))
                    } else {
                        None
                    };
                    return Ok(Some(Expr::RegExpDynamic {
                        pattern: Box::new(pattern),
                        flags,
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// #2874: `Iterator.from(x)` — wrap an iterable in a lazy iterator-helper
/// object. Only fires when `Iterator` is the global (not a local/func/import).
/// The produced helper's `.map`/`.filter`/`.take`/etc. dispatch at runtime via
/// `js_native_call_method`, so no further HIR variants are needed.
pub(super) fn try_iterator_from(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    has_spread: bool,
) -> Result<Option<Expr>> {
    if has_spread {
        return Ok(None);
    }
    let ast::Callee::Expr(callee_expr) = &call.callee else {
        return Ok(None);
    };
    let mut callee = callee_expr.as_ref();
    while let ast::Expr::Paren(p) = callee {
        callee = p.expr.as_ref();
    }
    let ast::Expr::Member(member) = callee else {
        return Ok(None);
    };
    let ast::MemberProp::Ident(prop) = &member.prop else {
        return Ok(None);
    };
    if prop.sym.as_ref() != "from" {
        return Ok(None);
    }
    let mut obj = member.obj.as_ref();
    while let ast::Expr::Paren(p) = obj {
        obj = p.expr.as_ref();
    }
    let ast::Expr::Ident(obj_ident) = obj else {
        return Ok(None);
    };
    if obj_ident.sym.as_ref() != "Iterator"
        || ctx.lookup_local("Iterator").is_some()
        || ctx.lookup_func("Iterator").is_some()
    {
        return Ok(None);
    }
    let arg = if call.args.is_empty() {
        Expr::Undefined
    } else {
        lower_expr(ctx, &call.args[0].expr)?
    };
    Ok(Some(Expr::IteratorFrom(Box::new(arg))))
}
