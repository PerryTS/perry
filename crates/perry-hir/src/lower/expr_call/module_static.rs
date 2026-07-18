//! fs/path/JSON/Math/Number/String/crypto/os/Buffer/cp/net/AbortSignal/Date/URL static method dispatch.
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use super::static_receiver::static_receiver_class;
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{extract_typed_parse_source_order, resolve_typed_parse_ty, LoweringContext};
use super::os::user_info_expr_for_call;

fn peel_webassembly_receiver_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::TsAs(x) => x.expr.as_ref(),
            ast::Expr::TsNonNull(x) => x.expr.as_ref(),
            ast::Expr::TsSatisfies(x) => x.expr.as_ref(),
            ast::Expr::TsTypeAssertion(x) => x.expr.as_ref(),
            ast::Expr::TsConstAssertion(x) => x.expr.as_ref(),
            ast::Expr::Paren(x) => x.expr.as_ref(),
            _ => return expr,
        };
    }
}

fn is_webassembly_namespace_receiver(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match peel_webassembly_receiver_expr(expr) {
        ast::Expr::Ident(id) => {
            id.sym.as_ref() == "WebAssembly" && ctx.lookup_local("WebAssembly").is_none()
        }
        ast::Expr::Member(member) => {
            let ast::MemberProp::Ident(prop) = &member.prop else {
                return false;
            };
            if prop.sym.as_ref() != "WebAssembly" {
                return false;
            }
            matches!(
                peel_webassembly_receiver_expr(member.obj.as_ref()),
                ast::Expr::Ident(root)
                    if root.sym.as_ref() == "globalThis"
                        && ctx.lookup_local("globalThis").is_none()
            )
        }
        _ => false,
    }
}

pub(super) fn try_module_static_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    mut args: Vec<Expr>,
    has_spread: bool,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let ast::Expr::Member(member) = expr {
        // #6560: `Bun.<method>(...)` — the Bun global shim pack, member
        // position only (mirrors the `WebAssembly` receiver pattern below:
        // an unshadowed bare `Bun` ident). Bare `Bun` / `typeof Bun` stay
        // untouched so node-targeting bundles that feature-detect Bun via
        // `typeof Bun !== "undefined"` keep taking their node paths. Only
        // the implemented surface lowers natively; unknown members fall
        // through to the ordinary (undefined-global) lowering.
        if !has_spread {
            if let (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident)) =
                (member.obj.as_ref(), &member.prop)
            {
                if obj_ident.sym.as_ref() == "Bun"
                    && ctx.lookup_local("Bun").is_none()
                    && ctx.lookup_native_module("Bun").is_none()
                    && matches!(
                        method_ident.sym.as_ref(),
                        "stringWidth"
                            | "hash"
                            | "file"
                            | "write"
                            | "pathToFileURL"
                            | "fileURLToPath"
                    )
                {
                    return Ok(Ok(Expr::NativeMethodCall {
                        module: "bun".to_string(),
                        class_name: None,
                        object: None,
                        method: method_ident.sym.as_ref().to_string(),
                        args,
                    }));
                }
            }
        }

        // WebAssembly.Module.* metadata statics. This is intentionally a
        // direct-call lowering so it does not duplicate the runtime namespace
        // descriptor work handled separately in #3635.
        if let (ast::Expr::Member(class_member), ast::MemberProp::Ident(method_ident)) =
            (member.obj.as_ref(), &member.prop)
        {
            if let ast::MemberProp::Ident(class_ident) = &class_member.prop {
                if class_ident.sym.as_ref() == "Module"
                    && is_webassembly_namespace_receiver(ctx, class_member.obj.as_ref())
                {
                    match method_ident.sym.as_ref() {
                        "exports" if !args.is_empty() => {
                            ctx.uses_webassembly = true;
                            return Ok(Ok(Expr::WebAssemblyModuleExports(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "imports" if !args.is_empty() => {
                            ctx.uses_webassembly = true;
                            return Ok(Ok(Expr::WebAssemblyModuleImports(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "customSections" if args.len() >= 2 => {
                            ctx.uses_webassembly = true;
                            let mut it = args.into_iter();
                            let module = it.next().unwrap();
                            let name = it.next().unwrap();
                            return Ok(Ok(Expr::WebAssemblyModuleCustomSections {
                                module: Box::new(module),
                                name: Box::new(name),
                            }));
                        }
                        _ => {}
                    }
                }
            }
        }

        if is_webassembly_namespace_receiver(ctx, member.obj.as_ref()) {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                match method_name {
                    "validate" if !args.is_empty() => {
                        ctx.uses_webassembly = true;
                        return Ok(Ok(Expr::WebAssemblyValidate(Box::new(
                            args.into_iter().next().unwrap(),
                        ))));
                    }
                    "instantiate" if !args.is_empty() => {
                        ctx.uses_webassembly = true;
                        return Ok(Ok(Expr::WebAssemblyInstantiate(Box::new(
                            args.into_iter().next().unwrap(),
                        ))));
                    }
                    "callExport" if args.len() >= 2 => {
                        ctx.uses_webassembly = true;
                        let mut it = args.into_iter();
                        let instance = it.next().unwrap();
                        let name = it.next().unwrap();
                        let rest: Vec<Expr> = it.collect();
                        return Ok(Ok(Expr::WebAssemblyCallExport {
                            instance: Box::new(instance),
                            name: Box::new(name),
                            args: rest,
                        }));
                    }
                    _ => {}
                }
            }
        }

        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
            let obj_name = obj_ident.sym.as_ref();
            let is_fs_module =
                obj_name == "fs" || ctx.lookup_builtin_module_alias(obj_name) == Some("fs");
            if is_fs_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "readFileSync" if args.len() == 1 => {
                            // readFileSync(path) without encoding — returns Buffer (Node parity)
                            return Ok(Ok(Expr::FsReadFileBinary(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "writeFileSync" if args.len() == 2 => {
                            let mut iter = args.into_iter();
                            let path = iter.next().unwrap();
                            let content = iter.next().unwrap();
                            return Ok(Ok(Expr::FsWriteFileSync(
                                Box::new(path),
                                Box::new(content),
                            )));
                        }
                        "appendFileSync" if args.len() == 2 => {
                            let mut iter = args.into_iter();
                            let path = iter.next().unwrap();
                            let content = iter.next().unwrap();
                            return Ok(Ok(Expr::FsAppendFileSync(
                                Box::new(path),
                                Box::new(content),
                            )));
                        }
                        "existsSync" if !args.is_empty() => {
                            return Ok(Ok(Expr::FsExistsSync(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "mkdirSync" if args.len() == 1 => {
                            return Ok(Ok(Expr::FsMkdirSync(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "unlinkSync" if !args.is_empty() => {
                            return Ok(Ok(Expr::FsUnlinkSync(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "readFileBuffer" if !args.is_empty() => {
                            return Ok(Ok(Expr::FsReadFileBinary(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "rmRecursive" if !args.is_empty() => {
                            return Ok(Ok(Expr::FsRmRecursive(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        // Issue #648 fallout: `fs.rmSync(path, opts?)` was
                        // historically silently no-op'd by `js_native_call_method`'s
                        // catch-all when the method wasn't statically wired. Now
                        // that catch-all throws, we need an explicit lowering.
                        // Routes to FsRmRecursive (recursive removal); the
                        // `{recursive,force,maxRetries,retryDelay}` opts arg is
                        // ignored — `js_fs_rm_recursive` already does recursive
                        // removal unconditionally.
                        "rmSync" => {}
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for path.methodName() calls (including require('path') aliases)
            let is_path_module =
                obj_name == "path" || ctx.lookup_builtin_module_alias(obj_name) == Some("path");
            if is_path_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "join" => {
                            if args.is_empty() {
                                return Ok(Ok(Expr::String(".".to_string())));
                            }
                            if args.len() == 1 {
                                return Ok(Ok(Expr::PathNormalize(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                            let mut iter = args.into_iter();
                            let mut result = iter.next().unwrap();
                            for next_arg in iter {
                                result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                            }
                            return Ok(Ok(result));
                        }
                        "dirname" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathDirname(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "basename" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let path_arg = iter.next().unwrap();
                                let ext_arg = iter.next().unwrap();
                                return Ok(Ok(Expr::PathBasenameExt(
                                    Box::new(path_arg),
                                    Box::new(ext_arg),
                                )));
                            }
                            if !args.is_empty() {
                                return Ok(Ok(Expr::PathBasename(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                        }
                        "extname" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathExtname(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "resolve" => {
                            if args.is_empty() {
                                return Ok(Ok(Expr::PathResolve(Box::new(Expr::String(
                                    String::new(),
                                )))));
                            }
                            if !args.is_empty() {
                                // path.resolve(a, b, c): per Node, a later
                                // absolute segment resets the accumulation —
                                // distinct from path.join. Use PathResolveJoin
                                // (reset-on-absolute) for the chain.
                                let mut iter = args.into_iter();
                                let first = iter.next().unwrap();
                                let mut joined = first;
                                for next_arg in iter {
                                    joined =
                                        Expr::PathResolveJoin(Box::new(joined), Box::new(next_arg));
                                }
                                return Ok(Ok(Expr::PathResolve(Box::new(joined))));
                            }
                        }
                        "isAbsolute" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathIsAbsolute(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "relative" if args.len() >= 2 => {
                            let mut iter = args.into_iter();
                            let from = iter.next().unwrap();
                            let to = iter.next().unwrap();
                            return Ok(Ok(Expr::PathRelative(Box::new(from), Box::new(to))));
                        }
                        "normalize" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathNormalize(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "parse" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathParse(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "format" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathFormat(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "toNamespacedPath" | "_makeLong" if !args.is_empty() => {
                            return Ok(Ok(Expr::PathToNamespacedPath(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "matchesGlob" if args.len() >= 2 => {
                            let mut iter = args.into_iter();
                            let path_arg = iter.next().unwrap();
                            let pattern = iter.next().unwrap();
                            return Ok(Ok(Expr::PathMatchesGlob(
                                Box::new(path_arg),
                                Box::new(pattern),
                            )));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for JSON.methodName() calls
            if obj_ident.sym.as_ref() == "JSON" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "parse" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let text = iter.next().unwrap();
                                let reviver = iter.next().unwrap();
                                return Ok(Ok(Expr::JsonParseWithReviver(
                                    Box::new(text),
                                    Box::new(reviver),
                                )));
                            } else {
                                // 0 or 1 args. `JSON.parse()` with no argument is
                                // `JSON.parse(undefined)` → ToString(undefined) =
                                // "undefined" → SyntaxError (NOT a TypeError from
                                // the generic fall-through dispatch).
                                let text = args.into_iter().next().unwrap_or(Expr::Undefined);
                                // Issue #179 typed-parse plan: if the call site
                                // provides a TypeScript type argument (e.g.
                                // `JSON.parse<Item[]>(blob)`), carry it into HIR
                                // so codegen can emit a specialized parse path.
                                // Semantically identical to JsonParse at runtime
                                // (the `<T>` erases — Node-compatible).
                                if let Some(type_args) = call.type_args.as_ref() {
                                    if let Some(ts_type) = type_args.params.first() {
                                        let ty = extract_ts_type_with_ctx(ts_type, Some(ctx));
                                        // Resolve Named → structural (interface)
                                        // aliases so codegen sees the full
                                        // ObjectType without re-walking the alias
                                        // table. Array<Named> inner element
                                        // also gets resolved.
                                        let resolved = resolve_typed_parse_ty(ctx, ty);
                                        if !matches!(resolved, Type::Any | Type::Unknown) {
                                            // Source-order field list for the
                                            // inner Object type, if we can
                                            // extract it from the AST. Codegen
                                            // uses this for the fast-path
                                            // per-field comparison.
                                            let ordered_keys =
                                                extract_typed_parse_source_order(ts_type, ctx);
                                            return Ok(Ok(Expr::JsonParseTyped {
                                                text: Box::new(text),
                                                ty: resolved,
                                                ordered_keys,
                                            }));
                                        }
                                    }
                                }
                                return Ok(Ok(Expr::JsonParse(Box::new(text))));
                            }
                        }
                        "stringify" => {
                            if !args.is_empty() {
                                // `JSON.stringify(url)` should invoke `url.toJSON()`
                                // (which returns href) and stringify the resulting
                                // string. Perry's runtime JSON stringifier doesn't
                                // honor `toJSON` on opaque runtime objects, so
                                // intercept the URL case at HIR time. Recognize URL
                                // via the original AST (typed local / direct
                                // `new URL`) and via `Expr::UrlNew` when the arg
                                // was already lowered. `UrlInstanceToJSON` /
                                // `UrlInstanceToString` must NOT match here: those
                                // are the href STRING produced by `u.toJSON()` /
                                // `u.toString()`, and wrapping them in another
                                // `UrlInstanceToJSON` reads a `StringHeader` as a
                                // URL `ObjectHeader` → undefined (#6488). The
                                // AST-side check can't misfire on those either:
                                // `static_receiver_class` of a call expression is
                                // `None`, not the callee receiver's class.
                                //
                                // The wrap applies to the replacer/spacer forms
                                // too: per SerializeJSONProperty, `toJSON` runs
                                // BEFORE the replacer, so the replacer observing
                                // the href string matches Node. Without it,
                                // `JSON.stringify(u, null, 2)` walked the opaque
                                // URL object and threw a circular-structure
                                // TypeError on its searchParams back-reference.
                                let original_arg = call.args.first().map(|a| a.expr.as_ref());
                                let arg_is_url = original_arg
                                    .map(|e| static_receiver_class(ctx, e) == Some("URL"))
                                    .unwrap_or(false)
                                    || matches!(args.first(), Some(Expr::UrlNew { .. }));
                                let mut it = args.into_iter();
                                let mut value = it.next().unwrap();
                                let replacer = it.next().unwrap_or(Expr::Null);
                                let spacer = it.next().unwrap_or(Expr::Null);
                                if arg_is_url {
                                    value = Expr::UrlInstanceToJSON(Box::new(value));
                                }
                                // Route ALL stringify calls through JsonStringifyFull
                                // so the runtime can return TAG_UNDEFINED for undefined input
                                return Ok(Ok(Expr::JsonStringifyFull(
                                    Box::new(value),
                                    Box::new(replacer),
                                    Box::new(spacer),
                                )));
                            }
                        }
                        "rawJSON"
                            // #2900: `JSON.rawJSON(text)` -> raw-JSON wrapper.
                            if !args.is_empty() => {
                                let text = args.into_iter().next().unwrap();
                                return Ok(Ok(Expr::JsonRawJson(Box::new(text))));
                            }
                        "isRawJSON"
                            // #2900: `JSON.isRawJSON(value)` -> boolean.
                            if !args.is_empty() => {
                                let value = args.into_iter().next().unwrap();
                                return Ok(Ok(Expr::JsonIsRawJson(Box::new(value))));
                            }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for performance.now() and the W3C User Timing methods.
            // `now()` keeps its dedicated Expr; the rest lower to a
            // receiver-less NativeMethodCall on the perf_hooks module, which
            // the codegen dispatches to the `js_perf_*` runtime helpers. This
            // is purely syntactic (matches the identifier `performance`), so
            // it fires whether `performance` is the global or the named
            // import from node:perf_hooks.
            if obj_ident.sym.as_ref() == "performance" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let m = method_ident.sym.as_ref();
                    if m == "now" {
                        return Ok(Ok(Expr::PerformanceNow));
                    }
                    if matches!(
                        m,
                        "mark"
                            | "measure"
                            | "getEntries"
                            | "getEntriesByName"
                            | "getEntriesByType"
                            | "clearMarks"
                            | "clearMeasures"
                            | "eventLoopUtilization"
                            | "toJSON"
                            | "clearResourceTimings"
                            | "setResourceTimingBufferSize"
                            | "markResourceTiming"
                            | "timerify"
                    ) {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "perf_hooks".to_string(),
                            class_name: None,
                            object: None,
                            method: m.to_string(),
                            args,
                        }));
                    }
                }
            }

            // node:events module-level static helpers via `import * as events
            // from "node:events"; events.once(em, name)` (and `on`,
            // `listenerCount`, `getMaxListeners`, `setMaxListeners`,
            // `getEventListeners`, `addAbortListener`). These take the emitter
            // as a positional arg (`has_receiver: false` in the codegen native
            // table), so they lower to a receiver-less NativeMethodCall on the
            // `events` module. Without this, `events.<helper>(...)` fell through
            // to a generic property-call on the resolved `NativeModuleRef` and
            // returned `undefined` (issue #850: `events.once(...)` never built a
            // Promise). Gated on the receiver identifier actually resolving to
            // the node:events namespace import (not a shadowing local).
            if ctx.lookup_local(obj_name).is_none()
                && (ctx.lookup_builtin_module_alias(obj_name) == Some("events")
                    || matches!(ctx.lookup_native_module(obj_name), Some(("events", _))))
            {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let m = method_ident.sym.as_ref();
                    if matches!(
                        m,
                        "once"
                            | "on"
                            | "listenerCount"
                            | "getMaxListeners"
                            | "setMaxListeners"
                            | "getEventListeners"
                            | "addAbortListener"
                    ) {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "events".to_string(),
                            class_name: None,
                            object: None,
                            method: m.to_string(),
                            args,
                        }));
                    }
                }
            }

            // Check for Response.json(value) / Response.redirect(url, status?) /
            // Response.error() static factories.
            if obj_ident.sym.as_ref() == "Response" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "json" | "redirect" | "error" => {
                            ctx.uses_fetch = true;
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "fetch".to_string(),
                                class_name: Some("Response".to_string()),
                                object: None,
                                method: format!("static_{}", method_name),
                                args,
                            }));
                        }
                        _ => {}
                    }
                }
            }

            // WebAssembly.* host runtime (issue #76). MVP surface:
            // - `WebAssembly.validate(bytes)`
            // - `WebAssembly.instantiate(bytes)` (sync, Perry-shape)
            // - `WebAssembly.callExport(inst, name, ...args)` (legacy
            //   helper kept for compatibility with the first PoC pass)
            // The standard `inst.exports.<method>(...)` shape is
            // recognised separately below as a syntactic pattern.
            if obj_ident.sym.as_ref() == "WebAssembly" && ctx.lookup_local("WebAssembly").is_none()
            {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "validate" if !args.is_empty() => {
                            ctx.uses_webassembly = true;
                            return Ok(Ok(Expr::WebAssemblyValidate(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "compile" if !args.is_empty() => {
                            ctx.uses_webassembly = true;
                            return Ok(Ok(Expr::WebAssemblyCompile(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "instantiate" if !args.is_empty() => {
                            ctx.uses_webassembly = true;
                            return Ok(Ok(Expr::WebAssemblyInstantiate(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "callExport" if args.len() >= 2 => {
                            ctx.uses_webassembly = true;
                            let mut it = args.into_iter();
                            let instance = it.next().unwrap();
                            let name = it.next().unwrap();
                            let rest: Vec<Expr> = it.collect();
                            return Ok(Ok(Expr::WebAssemblyCallExport {
                                instance: Box::new(instance),
                                name: Box::new(name),
                                args: rest,
                            }));
                        }
                        _ => {}
                    }
                }
            }

            // (Standard `<inst>.exports.<method>` shape moved to a
            // dedicated block above — see `// Issue #76` near the
            // fs-method dispatch — so it can match Member receivers
            // without being gated on the Ident-receiver wrapper.)

            // Check for Math.methodName() calls
            if obj_ident.sym.as_ref() == "Math" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "floor" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathFloor(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "ceil" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathCeil(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "round" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathRound(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "trunc" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathTrunc(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "sign" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathSign(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "abs" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAbs(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "sqrt" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathSqrt(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "log" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathLog(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "log2" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathLog2(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "log10" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathLog10(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "pow" if args.len() >= 2 => {
                            let mut args_iter = args.into_iter();
                            let base = args_iter.next().unwrap();
                            let exp = args_iter.next().unwrap();
                            return Ok(Ok(Expr::MathPow(Box::new(base), Box::new(exp))));
                        }
                        "min" => {
                            if has_spread && args.len() == 1 {
                                return Ok(Ok(Expr::MathMinSpread(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                            return Ok(Ok(Expr::MathMin(args)));
                        }
                        "max" => {
                            if has_spread && args.len() == 1 {
                                return Ok(Ok(Expr::MathMaxSpread(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                            return Ok(Ok(Expr::MathMax(args)));
                        }
                        "random" => {
                            return Ok(Ok(Expr::MathRandom));
                        }
                        "imul" if args.len() >= 2 => {
                            let mut args_iter = args.into_iter();
                            let a = args_iter.next().unwrap();
                            let b = args_iter.next().unwrap();
                            return Ok(Ok(Expr::MathImul(Box::new(a), Box::new(b))));
                        }
                        "sin" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathSin(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "cos" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathCos(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "tan" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathTan(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "asin" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAsin(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "acos" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAcos(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "atan" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAtan(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "atan2" if args.len() >= 2 => {
                            let mut args_iter = args.into_iter();
                            let y = args_iter.next().unwrap();
                            let x = args_iter.next().unwrap();
                            return Ok(Ok(Expr::MathAtan2(Box::new(y), Box::new(x))));
                        }
                        "cbrt" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathCbrt(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "hypot" => {
                            return Ok(Ok(Expr::MathHypot(args)));
                        }
                        "fround" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathFround(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "f16round" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathF16round(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "clz32" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathClz32(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "expm1" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathExpm1(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "log1p" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathLog1p(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "sinh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathSinh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "cosh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathCosh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "tanh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathTanh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "asinh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAsinh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "acosh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAcosh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "atanh" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathAtanh(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "exp" if !args.is_empty() => {
                            return Ok(Ok(Expr::MathExp(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // #2877: `ArrayBuffer.isView(x)` — true for TypedArray / DataView
            // values, false for ArrayBuffer / anything else. Route through the
            // existing `util.types.isArrayBufferView` runtime predicate (it
            // already recognizes typed arrays, Uint8Array-from-ctor and
            // DataView-marked buffers) by re-emitting a `util/types`
            // NativeMethodCall — no new HIR variant or runtime helper needed.
            if obj_ident.sym.as_ref() == "ArrayBuffer" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "isView" {
                        let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "util/types".to_string(),
                            class_name: None,
                            object: None,
                            method: "isArrayBufferView".to_string(),
                            args: vec![arg],
                        }));
                    }
                }
            }

            // `BigInt.asIntN(bits, x)` / `BigInt.asUintN(bits, x)`. Bare `BigInt`
            // in member position lowers to `globalThis`, so a direct
            // `BigInt.asIntN(...)` call would read `globalThis.asIntN`
            // (undefined → "value is not a function"). Route to the runtime via
            // a receiver-less NativeMethodCall. (The statics are also installed
            // on the BigInt ctor closure for the `const B = BigInt; B.asIntN`
            // value path.)
            if obj_ident.sym.as_ref() == "BigInt" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let m = method_ident.sym.as_ref();
                    if m == "asIntN" || m == "asUintN" {
                        let mut it = args.into_iter();
                        let bits = it.next().unwrap_or(Expr::Undefined);
                        let value = it.next().unwrap_or(Expr::Undefined);
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "bigint".to_string(),
                            class_name: None,
                            object: None,
                            method: m.to_string(),
                            args: vec![bits, value],
                        }));
                    }
                }
            }

            // Check for Number.methodName() static calls
            if obj_ident.sym.as_ref() == "Number" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        // A missing argument is `undefined` (Type ≠ Number), so
                        // each predicate is `false`. Fold the no-arg form to the
                        // same HIR node with an `Undefined` operand — otherwise a
                        // zero-arg call fell through to the generic dispatch where
                        // the missing `f64` parameter defaulted to a real NaN,
                        // making `Number.isNaN()` wrongly return `true`
                        // (built-ins/Number/isNaN/arg-is-not-number.js "no arg").
                        "isNaN" => {
                            let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::NumberIsNaN(Box::new(arg))));
                        }
                        "isFinite" => {
                            let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::NumberIsFinite(Box::new(arg))));
                        }
                        "isInteger" => {
                            let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::NumberIsInteger(Box::new(arg))));
                        }
                        "isSafeInteger" => {
                            let arg = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::NumberIsSafeInteger(Box::new(arg))));
                        }
                        "parseFloat" => {
                            // Number.parseFloat is the same as global parseFloat
                            if !args.is_empty() {
                                return Ok(Ok(Expr::ParseFloat(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            } else {
                                return Ok(Ok(Expr::ParseFloat(Box::new(Expr::Undefined))));
                            }
                        }
                        "parseInt" => {
                            // Number.parseInt is the same as global parseInt
                            let mut iter = args.into_iter();
                            let string_arg = if let Some(s) = iter.next() {
                                Box::new(s)
                            } else {
                                Box::new(Expr::Undefined)
                            };
                            let radix_arg = iter.next().map(Box::new);
                            return Ok(Ok(Expr::ParseInt {
                                string: string_arg,
                                radix: radix_arg,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for String.methodName() static calls
            if obj_ident.sym.as_ref() == "String" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "fromCharCode" => {
                            if args.is_empty() {
                                // #2788: String.fromCharCode() -> "".
                                return Ok(Ok(Expr::String(String::new())));
                            }
                            if has_spread && args.len() == 1 {
                                return Ok(Ok(Expr::StringFromCharCodeSpread(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                            if args.len() == 1 {
                                return Ok(Ok(Expr::StringFromCharCode(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            } else if args.len() > 1 {
                                // Multi-arg: concat each char as a separate fromCharCode call
                                let mut iter = args.into_iter();
                                let mut acc =
                                    Expr::StringFromCharCode(Box::new(iter.next().unwrap()));
                                for arg in iter {
                                    acc = Expr::Binary {
                                        op: crate::ir::BinaryOp::Add,
                                        left: Box::new(acc),
                                        right: Box::new(Expr::StringFromCharCode(Box::new(arg))),
                                    };
                                }
                                return Ok(Ok(acc));
                            }
                        }
                        "fromCodePoint" => {
                            if args.is_empty() {
                                // #2788: String.fromCodePoint() -> "".
                                return Ok(Ok(Expr::String(String::new())));
                            }
                            if args.len() == 1 {
                                return Ok(Ok(Expr::StringFromCodePoint(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            } else if args.len() > 1 {
                                let mut iter = args.into_iter();
                                let mut acc =
                                    Expr::StringFromCodePoint(Box::new(iter.next().unwrap()));
                                for arg in iter {
                                    acc = Expr::Binary {
                                        op: crate::ir::BinaryOp::Add,
                                        left: Box::new(acc),
                                        right: Box::new(Expr::StringFromCodePoint(Box::new(arg))),
                                    };
                                }
                                return Ok(Ok(acc));
                            }
                        }
                        // Callable String.raw(callSite, ...subs) — the
                        // non-tagged form. The tagged ``String.raw`...` ``
                        // form is handled at the TaggedTpl lowering site.
                        // (#2789)
                        "raw" => {
                            let mut iter = args.into_iter();
                            let call_site = iter.next().unwrap_or(Expr::Undefined);
                            let substitutions: Vec<Expr> = iter.collect();
                            return Ok(Ok(Expr::StringRaw {
                                call_site: Box::new(call_site),
                                substitutions,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for crypto.methodName() calls (including require('crypto') aliases)
            let is_crypto_module =
                obj_name == "crypto" || ctx.lookup_builtin_module_alias(obj_name) == Some("crypto");
            if is_crypto_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    // #1434: keep the named-import + dotted form on one
                    // shared lowering path for the methods whose shape
                    // matches between sites.
                    if super::crypto::is_passthrough_method(method_name) {
                        if let Some(expr) = super::crypto::lower_crypto_passthrough(
                            method_name,
                            std::mem::take(&mut args),
                        ) {
                            return Ok(Ok(expr));
                        }
                    }
                    match method_name {
                        "sha256" if !args.is_empty() => {
                            return Ok(Ok(Expr::CryptoSha256(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "md5" if !args.is_empty() => {
                            return Ok(Ok(Expr::CryptoMd5(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        // `crypto.getRandomValues(buf)` fills the buffer
                        // in-place with random bytes and returns it.
                        // Lower as a synthetic instance method call so
                        // the runtime buffer dispatcher (added in
                        // perry-runtime/src/object.rs) handles it via
                        // `js_buffer_fill_random`.
                        "getRandomValues" if !args.is_empty() => {
                            let buf_arg = args.into_iter().next().unwrap();
                            return Ok(Ok(Expr::Call {
                                callee: Box::new(Expr::PropertyGet {
                                    byte_offset: 0,
                                    object: Box::new(buf_arg),
                                    property: "$$cryptoFillRandom".to_string(),
                                }),
                                args: vec![],
                                type_args: vec![],
                                byte_offset: 0,
                            }));
                        }
                        // `crypto.randomBytes` / `randomUUID` /
                        // `randomFillSync` are handled by the shared
                        // `crypto::lower_crypto_passthrough` above —
                        // see #1434.
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for os.methodName() calls (including require('os') aliases)
            let is_os_module =
                obj_name == "os" || ctx.lookup_builtin_module_alias(obj_name) == Some("os");
            if is_os_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "availableParallelism" => {
                            return Ok(Ok(Expr::OsAvailableParallelism));
                        }
                        "platform" => {
                            return Ok(Ok(Expr::OsPlatform));
                        }
                        "arch" => {
                            return Ok(Ok(Expr::OsArch));
                        }
                        "endianness" => {
                            return Ok(Ok(Expr::OsEndianness));
                        }
                        "hostname" => {
                            return Ok(Ok(Expr::OsHostname));
                        }
                        "homedir" => {
                            return Ok(Ok(Expr::OsHomedir));
                        }
                        "tmpdir" => {
                            return Ok(Ok(Expr::OsTmpdir));
                        }
                        "loadavg" => {
                            return Ok(Ok(Expr::OsLoadavg));
                        }
                        "machine" => {
                            return Ok(Ok(Expr::OsMachine));
                        }
                        "totalmem" => {
                            return Ok(Ok(Expr::OsTotalmem));
                        }
                        "freemem" => {
                            return Ok(Ok(Expr::OsFreemem));
                        }
                        "uptime" => {
                            return Ok(Ok(Expr::OsUptime));
                        }
                        "type" => {
                            return Ok(Ok(Expr::OsType));
                        }
                        "release" => {
                            return Ok(Ok(Expr::OsRelease));
                        }
                        "version" => {
                            return Ok(Ok(Expr::OsVersion));
                        }
                        "cpus" => {
                            return Ok(Ok(Expr::OsCpus));
                        }
                        "networkInterfaces" => {
                            return Ok(Ok(Expr::OsNetworkInterfaces));
                        }
                        "userInfo" => {
                            return Ok(Ok(user_info_expr_for_call(call, args)));
                        }
                        "getPriority" | "setPriority" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "os".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for Buffer.methodName() static calls — also accept
            // aliased imports per #831 (see the longer note at the
            // first Buffer site above).
            let is_buffer_ref = obj_name == "Buffer"
                || matches!(
                    ctx.lookup_native_module(obj_name),
                    Some(("buffer", Some("Buffer")))
                );
            if is_buffer_ref {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "from" => {
                            let data = args.first().cloned().unwrap_or(Expr::Undefined);
                            // See native_module.rs for the disambiguation rule
                            // — issue #1273.
                            let is_arraybuffer_form = args.len() >= 3
                                || matches!(args.get(1), Some(Expr::Number(_) | Expr::Integer(_)));
                            if args.len() >= 2 && is_arraybuffer_form {
                                let byte_offset = args.get(1).cloned().unwrap_or(Expr::Number(0.0));
                                let length = args.get(2).cloned().map(Box::new);
                                return Ok(Ok(Expr::BufferFromArrayBuffer {
                                    data: Box::new(data),
                                    byte_offset: Box::new(byte_offset),
                                    length,
                                }));
                            }
                            let encoding = args.get(1).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferFrom {
                                data: Box::new(data),
                                encoding,
                            }));
                        }
                        "alloc" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let size = args_iter.next().unwrap();
                            let fill = args_iter.next().map(Box::new);
                            let encoding = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::BufferAlloc {
                                size: Box::new(size),
                                fill,
                                encoding,
                            }));
                        }
                        "allocUnsafe" | "allocUnsafeSlow" if !args.is_empty() => {
                            return Ok(Ok(Expr::BufferAllocUnsafe(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "concat" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let list = args_iter.next().unwrap();
                            if let Some(total_length) = args_iter.next() {
                                return Ok(Ok(Expr::BufferConcatWithLength {
                                    list: Box::new(list),
                                    total_length: Box::new(total_length),
                                }));
                            }
                            return Ok(Ok(Expr::BufferConcat(Box::new(list))));
                        }
                        "copyBytesFrom" => {
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "buffer".to_string(),
                                class_name: None,
                                object: None,
                                method: "copyBytesFrom".to_string(),
                                args,
                            }));
                        }
                        "of" => {
                            return Ok(Ok(Expr::BufferFrom {
                                data: Box::new(Expr::Array(args)),
                                encoding: None,
                            }));
                        }
                        "isBuffer" if !args.is_empty() => {
                            return Ok(Ok(Expr::BufferIsBuffer(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "isEncoding" if !args.is_empty() => {
                            return Ok(Ok(Expr::BufferIsEncoding(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "byteLength" if !args.is_empty() => {
                            let mut it = args.into_iter();
                            let data = it.next().unwrap();
                            let encoding = it.next().map(Box::new);
                            return Ok(Ok(Expr::BufferByteLength {
                                data: Box::new(data),
                                encoding,
                            }));
                        }
                        // `Buffer.compare(a, b)` returns -1/0/1. The runtime
                        // dispatch already handles `a.compare(b)` as an
                        // instance method routing through `js_buffer_compare`.
                        // Synthesize that form so we don't need a dedicated
                        // HIR variant or runtime entry point.
                        "compare" if args.len() >= 2 => {
                            let mut iter = args.into_iter();
                            let a = iter.next().unwrap();
                            let b = iter.next().unwrap();
                            return Ok(Ok(Expr::Call {
                                callee: Box::new(Expr::PropertyGet {
                                    byte_offset: 0,
                                    object: Box::new(a),
                                    property: "compare".to_string(),
                                }),
                                args: vec![b],
                                type_args: vec![],
                                byte_offset: 0,
                            }));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // #2901: TC39 `Uint8Array.fromBase64(str, opts)` / `fromHex(str)`
            // static factories. Perry aliases Uint8Array → Buffer; route these
            // through the buffer native-module dispatch table (no new HIR
            // variant) so the runtime decodes into a fresh BufferHeader.
            if obj_name == "Uint8Array" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if matches!(method_name, "fromBase64" | "fromHex") {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "buffer".to_string(),
                            class_name: None,
                            object: None,
                            method: method_name.to_string(),
                            args,
                        }));
                    }
                }
            }

            // #2902: generic `<TypedArray>.from(src)` / `<TypedArray>.of(...items)`
            // for the multi-byte numeric kinds (Int16/Uint16/Int32/Uint32/
            // Float16/Float32/Float64). Uint8Array is handled by the Buffer
            // path above; with a mapFn we fall through to dynamic dispatch.
            // `from(src)` and `of(...items)` both reduce to copying a JS array
            // into a fresh typed array, which `TypedArrayNew` already does
            // (no mapFn). This intentionally only fires when the receiver name
            // is a real global typed-array constructor (not shadowed by a
            // local/import/class binding).
            if let Some(kind) = crate::ir::typed_array_kind_for_name(obj_name) {
                let shadowed = ctx.lookup_local(obj_name).is_some()
                    || ctx.lookup_func(obj_name).is_some()
                    || ctx.lookup_imported_func(obj_name).is_some()
                    || ctx.lookup_class(obj_name).is_some();
                if !shadowed && obj_name != "Uint8Array" {
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.as_ref();
                        match method_name {
                            // `from(arrayLike)` with no map function: copy.
                            "from" if args.len() == 1 => {
                                return Ok(Ok(Expr::TypedArrayNew {
                                    kind,
                                    arg: Some(Box::new(args.into_iter().next().unwrap())),
                                }));
                            }
                            // `of(...items)` is `from([...items])`.
                            "of" => {
                                return Ok(Ok(Expr::TypedArrayNew {
                                    kind,
                                    arg: Some(Box::new(Expr::Array(args))),
                                }));
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Check for child_process named imports (execSync, spawnSync, spawn, exec)
            let is_child_process_module =
                ctx.lookup_builtin_module_alias(obj_name) == Some("child_process");
            if is_child_process_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "execSync" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let command = args_iter.next().unwrap();
                            let options = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessExecSync {
                                command: Box::new(command),
                                options,
                            }));
                        }
                        "spawnSync" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let command = args_iter.next().unwrap();
                            let spawn_args = args_iter.next().map(Box::new);
                            let options = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessSpawnSync {
                                command: Box::new(command),
                                args: spawn_args,
                                options,
                            }));
                        }
                        "spawn" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let command = args_iter.next().unwrap();
                            let spawn_args = args_iter.next().map(Box::new);
                            let options = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessSpawn {
                                command: Box::new(command),
                                args: spawn_args,
                                options,
                            }));
                        }
                        "fork" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let module = args_iter.next().unwrap();
                            let fork_args = args_iter.next().map(Box::new);
                            let options = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessFork {
                                module: Box::new(module),
                                args: fork_args,
                                options,
                            }));
                        }
                        "exec" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let command = args_iter.next().unwrap();
                            let options = args_iter.next().map(Box::new);
                            let callback = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessExec {
                                command: Box::new(command),
                                options,
                                callback,
                            }));
                        }
                        "execFile" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let file = args_iter.next().unwrap();
                            let file_args = args_iter.next().map(Box::new);
                            let options = args_iter.next().map(Box::new);
                            let callback = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessExecFile {
                                file: Box::new(file),
                                args: file_args,
                                options,
                                callback,
                            }));
                        }
                        "execFileSync" if !args.is_empty() => {
                            let mut args_iter = args.into_iter();
                            let file = args_iter.next().unwrap();
                            let file_args = args_iter.next().map(Box::new);
                            let options = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessExecFileSync {
                                file: Box::new(file),
                                args: file_args,
                                options,
                            }));
                        }
                        "spawnBackground" if args.len() >= 3 => {
                            let mut args_iter = args.into_iter();
                            let command = args_iter.next().unwrap();
                            let spawn_args = args_iter.next().map(Box::new);
                            let log_file = args_iter.next().unwrap();
                            let env_json = args_iter.next().map(Box::new);
                            return Ok(Ok(Expr::ChildProcessSpawnBackground {
                                command: Box::new(command),
                                args: spawn_args,
                                log_file: Box::new(log_file),
                                env_json,
                            }));
                        }
                        "getProcessStatus" if !args.is_empty() => {
                            return Ok(Ok(Expr::ChildProcessGetProcessStatus(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        "killProcess" if !args.is_empty() => {
                            return Ok(Ok(Expr::ChildProcessKillProcess(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for net.methodName() calls
            let is_net_module =
                obj_name == "net" || ctx.lookup_builtin_module_alias(obj_name) == Some("net");
            if is_net_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "createServer" => {
                            let (options, connection_listener) = match args.as_slice() {
                                [Expr::Closure { .. }] => {
                                    (None, args.into_iter().next().map(Box::new))
                                }
                                _ => {
                                    let mut args_iter = args.into_iter();
                                    let options = args_iter.next().map(Box::new);
                                    let connection_listener = args_iter.next().map(Box::new);
                                    (options, connection_listener)
                                }
                            };
                            return Ok(Ok(Expr::NetCreateServer {
                                options,
                                connection_listener,
                            }));
                        }
                        // createConnection/connect: see sibling site above —
                        // falls through to generic NativeMethodCall so the LLVM
                        // backend's NATIVE_MODULE_TABLE dispatch can handle it.
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for AbortSignal static helpers: timeout / abort / any (#2582).
            if obj_ident.sym.as_ref() == "AbortSignal" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if matches!(method_name, "timeout" | "abort" | "any") {
                        return Ok(Ok(Expr::StaticMethodCall {
                            class_name: "AbortSignal".to_string(),
                            method_name: method_name.to_string(),
                            args,
                        }));
                    }
                }
            }

            // Check for Date.now() / Date.parse() / Date.UTC() static method calls
            if obj_ident.sym.as_ref() == "Date" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if method_name == "now" {
                        return Ok(Ok(Expr::DateNow));
                    }
                    if method_name == "parse" && !args.is_empty() {
                        return Ok(Ok(Expr::DateParse(Box::new(
                            args.into_iter().next().unwrap(),
                        ))));
                    }
                    if method_name == "UTC" {
                        return Ok(Ok(Expr::DateUtc(args)));
                    }
                }
            }

            // Issue #650: URL.canParse(s) / URL.parse(s) static
            // methods. URL is special-cased at HIR (the `URL`
            // identifier resolves to a synthetic `0` value rather
            // than a real ClassRef), so the static-method dispatch
            // never reached the canonical class-method path. This
            // arm intercepts both spellings and routes to dedicated
            // HIR variants.
            if obj_ident.sym.as_ref() == "URL" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    if method_name == "canParse" && !args.is_empty() {
                        let mut iter = args.into_iter();
                        let input = iter.next().unwrap();
                        if let Some(base) = iter.next() {
                            return Ok(Ok(Expr::UrlCanParseWithBase {
                                input: Box::new(input),
                                base: Box::new(base),
                            }));
                        }
                        return Ok(Ok(Expr::UrlCanParse(Box::new(input))));
                    }
                    if method_name == "parse" && !args.is_empty() {
                        let mut iter = args.into_iter();
                        let input = iter.next().unwrap();
                        if let Some(base) = iter.next() {
                            return Ok(Ok(Expr::UrlParseWithBase {
                                input: Box::new(input),
                                base: Box::new(base),
                            }));
                        }
                        return Ok(Ok(Expr::UrlParse(Box::new(input))));
                    }
                    // Issue #1211: `URL.createObjectURL(blob)` /
                    // `URL.revokeObjectURL(url)` route to the
                    // Blob-registry helpers. Modelled as receiver-less
                    // `NativeMethodCall { module: "url" }` so the
                    // native dispatch table picks them up.
                    if method_name == "createObjectURL" {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "url".to_string(),
                            class_name: None,
                            object: None,
                            method: "createObjectURL".to_string(),
                            args,
                        }));
                    }
                    if method_name == "revokeObjectURL" {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "url".to_string(),
                            class_name: None,
                            object: None,
                            method: "revokeObjectURL".to_string(),
                            args,
                        }));
                    }
                }
            }
        }
    }
    Ok(Err(args))
}
