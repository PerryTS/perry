//! Global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite, etc.).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};
use super::os::user_info_expr_for_call;

pub(super) fn try_global_builtins(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    mut args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite)
    if let ast::Expr::Ident(ident) = expr {
        let func_name = ident.sym.as_ref();
        if ctx.lookup_local(func_name).is_some()
            || ctx.lookup_func(func_name).is_some()
            || ctx.lookup_imported_func(func_name).is_some()
            || ctx.lookup_class(func_name).is_some()
        {
            return Ok(Err(args));
        }
        match func_name {
            "parseInt" => {
                let string_arg = if !args.is_empty() {
                    Box::new(args.remove(0))
                } else {
                    Box::new(Expr::Undefined)
                };
                let radix_arg = if !args.is_empty() {
                    Some(Box::new(args.remove(0)))
                } else {
                    None
                };
                return Ok(Ok(Expr::ParseInt {
                    string: string_arg,
                    radix: radix_arg,
                }));
            }
            "parseFloat" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::ParseFloat(Box::new(args.remove(0)))));
                } else {
                    return Ok(Ok(Expr::ParseFloat(Box::new(Expr::Undefined))));
                }
            }
            "Number" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::NumberCoerce(Box::new(args.remove(0)))));
                } else {
                    // Number() with no args returns 0
                    return Ok(Ok(Expr::Number(0.0)));
                }
            }
            "BigInt" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::BigIntCoerce(Box::new(args.remove(0)))));
                } else {
                    // `BigInt()` with no args coerces `undefined`, which Node
                    // rejects with `TypeError: Cannot convert undefined to a
                    // BigInt` (#2754/#2907). Route through the coercion path
                    // so the runtime throws instead of returning 0n.
                    return Ok(Ok(Expr::BigIntCoerce(Box::new(Expr::Undefined))));
                }
            }
            "String" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::StringCoerce(Box::new(args.remove(0)))));
                } else {
                    // String() with no args returns ""
                    return Ok(Ok(Expr::String(String::new())));
                }
            }
            "Boolean" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::BooleanCoerce(Box::new(args.remove(0)))));
                } else {
                    // Boolean() with no args returns false
                    return Ok(Ok(Expr::Bool(false)));
                }
            }
            "Object"
                if ctx.lookup_local("Object").is_none()
                    && ctx.lookup_func("Object").is_none()
                    && ctx.lookup_imported_func("Object").is_none() =>
            {
                // #3149: `Object(x)` called as a plain function (not `new`).
                // ECMAScript §20.1.1.1: `Object()`/`Object(undefined)`/
                // `Object(null)` yield a fresh `{}`; an existing object/array
                // passes through; primitives yield an object placeholder (so
                // `typeof Object(5) === "object"`). Pre-fix the bare call fell
                // through to the generic dispatcher and returned `undefined`
                // (the `new Object(...)` form already worked via
                // `js_new_function_construct`). Route through `ObjectCoerce`,
                // whose runtime (`js_object_coerce`) implements all cases.
                // Shadow-safe: only fires when no local / user fn / imported
                // fn named `Object` is in scope.
                let arg = if args.is_empty() {
                    Expr::Undefined
                } else {
                    args.remove(0)
                };
                return Ok(Ok(Expr::ObjectCoerce(Box::new(arg))));
            }
            "Array"
                if ctx.lookup_local("Array").is_none()
                    && ctx.lookup_func("Array").is_none()
                    && ctx.lookup_imported_func("Array").is_none() =>
            {
                // Issue #904: `Array(n)` (bare call, no `new`) must
                // behave identically to `new Array(n)` per spec
                // (ES2015 §22.1.1.1 / §22.1.1.2). Pre-fix the call
                // fell through to the unknown-ident sentinel path,
                // which lowers to `Call { callee: GlobalGet(0), ... }`
                // and explodes at runtime via `js_closure_callN`'s
                // `throw_not_callable` → `TypeError: value is not a
                // function`. dayjs's `format()` hits this through
                // `padStart`'s `Array(length + 1 - s.length).join(pad)`
                // every time it formats a sub-10 number (e.g. month
                // "07" in "YYYY-MM"). Route through `Expr::New` so the
                // existing Array-constructor codegen in
                // `crates/perry-codegen/src/lower_call/builtin.rs`
                // handles it. Shadow-safe: only fires when no local
                // / user fn / imported fn named `Array` is in scope.
                return Ok(Ok(Expr::New {
                    class_name: "Array".to_string(),
                    args,
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }));
            }
            // A missing argument to these is NOT an error in JS — the parameter is
            // simply `undefined`, and each has well-defined behavior for it
            // (`isNaN() === true`, `isFinite() === false`, `encodeURI() === "undefined"`,
            // …, all verified against Node). Rejecting the call at COMPILE time made
            // perry refuse to compile legal JS. Lower the omitted argument to
            // `Expr::Undefined` and let the existing intrinsic produce Node's answer.
            //
            // NOT extended to `atob`/`btoa`/`structuredClone`: those are WebIDL
            // required-argument throws where `f()` and `f(undefined)` genuinely
            // DIFFER (`atob()` → TypeError but `atob(undefined)` → InvalidCharacterError;
            // `btoa()` → TypeError but `btoa(undefined)` → "dW5kZWZpbmVk"), so they
            // cannot be modelled by padding and need real arity plumbing. See #6366.
            "isNaN" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::IsNaN(Box::new(arg))));
            }
            "isFinite" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::IsFinite(Box::new(arg))));
            }
            "atob" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::Atob(Box::new(args.remove(0)))));
                } else {
                    return Err(anyhow!("atob requires one argument"));
                }
            }
            "btoa" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::Btoa(Box::new(args.remove(0)))));
                } else {
                    return Err(anyhow!("btoa requires one argument"));
                }
            }
            "encodeURI" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::EncodeURI(Box::new(arg))));
            }
            "decodeURI" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::DecodeURI(Box::new(arg))));
            }
            "encodeURIComponent" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::EncodeURIComponent(Box::new(arg))));
            }
            "decodeURIComponent" => {
                let arg = arg_or_undefined(&mut args);
                return Ok(Ok(Expr::DecodeURIComponent(Box::new(arg))));
            }
            "structuredClone" => {
                if !args.is_empty() {
                    let value = args.remove(0);
                    let options = if !args.is_empty() {
                        args.remove(0)
                    } else {
                        Expr::Undefined
                    };
                    return Ok(Ok(Expr::StructuredClone {
                        value: Box::new(value),
                        options: Box::new(options),
                    }));
                } else {
                    return Err(anyhow!("structuredClone requires one argument"));
                }
            }
            "queueMicrotask" => {
                let callback = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                return Ok(Ok(Expr::QueueMicrotask(Box::new(callback))));
            }
            // Internal intrinsic emitted only by the CJS wrapper's `require`
            // fallback (cjs_wrap/wrap.rs): runtime `require(absolutePath.json)`.
            // Reads + JSON.parses the file from disk via the runtime; `.json` is
            // pure data so no eval is involved (Next.js wall 53).
            "__perry_require_json_disk" => {
                let specifier = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "__perry_runtime".to_string(),
                    class_name: None,
                    object: None,
                    method: "requireJsonDisk".to_string(),
                    args: vec![specifier],
                }));
            }
            // require.resolve fallback: node_modules subpath probing for
            // specifiers never statically required (styled-jsx/package.json).
            "__perry_require_resolve_node_modules" => {
                let from = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                let specifier = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "__perry_runtime".to_string(),
                    class_name: None,
                    object: None,
                    method: "requireResolveNodeModules".to_string(),
                    args: vec![from, specifier],
                }));
            }
            // Wall 54: register an AOT-compiled module's exports under its
            // absolute source path (emitted at the tail of each CJS wrapper).
            "__perry_register_path_module" => {
                let path = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                let exports = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "__perry_runtime".to_string(),
                    class_name: None,
                    object: None,
                    method: "registerPathModule".to_string(),
                    args: vec![path, exports],
                }));
            }
            // Wall 54: resolve a runtime `require(absolutePath.js)` to an
            // AOT-compiled module's exports (or `undefined` on miss).
            "__perry_require_path_module" => {
                let path = if !args.is_empty() {
                    args.remove(0)
                } else {
                    Expr::Undefined
                };
                return Ok(Ok(Expr::NativeMethodCall {
                    module: "__perry_runtime".to_string(),
                    class_name: None,
                    object: None,
                    method: "requirePathModule".to_string(),
                    args: vec![path],
                }));
            }
            "Symbol" => {
                // Symbol() / Symbol(description)
                if args.is_empty() {
                    return Ok(Ok(Expr::SymbolNew(None)));
                } else {
                    return Ok(Ok(Expr::SymbolNew(Some(Box::new(args.remove(0))))));
                }
            }
            "perryResolveStaticPlugin" => {
                if !args.is_empty() {
                    return Ok(Ok(Expr::StaticPluginResolve(Box::new(args.remove(0)))));
                } else {
                    return Err(anyhow!("perryResolveStaticPlugin requires one argument"));
                }
            }
            "fetchWithAuth" => {
                // fetchWithAuth(url, authHeader) -> Promise<Response>
                // Calls js_fetch_get_with_auth(url, auth_header)
                if args.len() >= 2 {
                    let url = args.remove(0);
                    let auth_header = args.remove(0);
                    ctx.uses_fetch = true;
                    return Ok(Ok(Expr::FetchGetWithAuth {
                        url: Box::new(url),
                        auth_header: Box::new(auth_header),
                    }));
                } else {
                    return Err(anyhow!(
                        "fetchWithAuth requires url and authHeader arguments"
                    ));
                }
            }
            "fetchPostWithAuth" => {
                // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
                // Calls js_fetch_post_with_auth(url, auth_header, body)
                if args.len() >= 3 {
                    let url = args.remove(0);
                    let auth_header = args.remove(0);
                    let body = args.remove(0);
                    ctx.uses_fetch = true;
                    return Ok(Ok(Expr::FetchPostWithAuth {
                        url: Box::new(url),
                        auth_header: Box::new(auth_header),
                        body: Box::new(body),
                    }));
                } else {
                    return Err(anyhow!(
                        "fetchPostWithAuth requires url, authHeader, and body arguments"
                    ));
                }
            }
            "fetch" => {
                // Handle fetch(url) and fetch(url, options)
                // Extract URL (first argument)
                let url = if !args.is_empty() {
                    args.remove(0)
                } else {
                    return Err(anyhow!("fetch requires at least a URL argument"));
                };

                // Check if there's an options object (second argument)
                if !args.is_empty() {
                    // Extract options from the object literal
                    // We need to get the original AST to extract the object properties
                    if let Some(options_arg) = call.args.get(1) {
                        if let ast::Expr::Object(obj) = &*options_arg.expr {
                            // Extract method, body, and headers from options
                            let mut method = Expr::String("GET".to_string());
                            let mut body = Expr::Undefined;
                            let mut headers_obj: Vec<(String, Expr)> = Vec::new();
                            let mut headers_dynamic: Option<Box<Expr>> = None;
                            let mut signal: Option<Box<Expr>> = None;

                            for prop in &obj.props {
                                if let ast::PropOrSpread::Prop(prop) = prop {
                                    match prop.as_ref() {
                                        ast::Prop::KeyValue(kv) => {
                                            let key = match &kv.key {
                                                ast::PropName::Ident(ident) => {
                                                    ident.sym.to_string()
                                                }
                                                ast::PropName::Str(s) => {
                                                    s.value.as_str().unwrap_or("").to_string()
                                                }
                                                _ => continue,
                                            };
                                            match key.as_str() {
                                                "method" => {
                                                    method = lower_expr(ctx, &kv.value)?;
                                                }
                                                "body" => {
                                                    body = lower_expr(ctx, &kv.value)?;
                                                }
                                                "headers" => {
                                                    // The headers value can be serialized statically
                                                    // only if it is an object *literal* whose props
                                                    // are all plain (non-computed) string/ident keys.
                                                    // Anything else — a variable (`headers: h`), a
                                                    // spread literal (`{...h}`), a call such as
                                                    // `Object.assign({}, h)` / `new Headers(h)` /
                                                    // `JSON.parse(...)`, or a computed/getter prop —
                                                    // must be serialized at runtime, so capture the
                                                    // whole expression in `headers_dynamic`. Without
                                                    // this, dynamically-built header objects silently
                                                    // dropped every header (#4932).
                                                    let static_literal = match &*kv.value {
                                                        ast::Expr::Object(headers_ast) => {
                                                            headers_ast.props.iter().all(|p| {
                                                                match p {
                                                                    ast::PropOrSpread::Prop(prop) => {
                                                                        matches!(
                                                                            prop.as_ref(),
                                                                            ast::Prop::KeyValue(hkv)
                                                                                if matches!(
                                                                                    &hkv.key,
                                                                                    ast::PropName::Ident(_)
                                                                                        | ast::PropName::Str(_)
                                                                                )
                                                                        )
                                                                    }
                                                                    ast::PropOrSpread::Spread(_) => false,
                                                                }
                                                            })
                                                        }
                                                        _ => false,
                                                    };
                                                    if static_literal {
                                                        if let ast::Expr::Object(headers_ast) =
                                                            &*kv.value
                                                        {
                                                            for hprop in &headers_ast.props {
                                                                if let ast::PropOrSpread::Prop(
                                                                    hprop,
                                                                ) = hprop
                                                                {
                                                                    if let ast::Prop::KeyValue(
                                                                        hkv,
                                                                    ) = hprop.as_ref()
                                                                    {
                                                                        let hkey = match &hkv.key {
                                                                            ast::PropName::Ident(
                                                                                ident,
                                                                            ) => ident.sym.to_string(),
                                                                            ast::PropName::Str(s) => s
                                                                                .value
                                                                                .as_str()
                                                                                .unwrap_or("")
                                                                                .to_string(),
                                                                            _ => continue,
                                                                        };
                                                                        let hval = lower_expr(
                                                                            ctx, &hkv.value,
                                                                        )?;
                                                                        headers_obj
                                                                            .push((hkey, hval));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        headers_dynamic = Some(Box::new(
                                                            lower_expr(ctx, &kv.value)?,
                                                        ));
                                                    }
                                                }
                                                "signal" => {
                                                    signal =
                                                        Some(Box::new(lower_expr(ctx, &kv.value)?));
                                                }
                                                _ => {}
                                            }
                                        }
                                        ast::Prop::Shorthand(ident) => {
                                            // Handle shorthand properties like { body } which means { body: body }
                                            let key = ident.sym.to_string();
                                            let value =
                                                if let Some(local_id) = ctx.lookup_local(&key) {
                                                    Expr::LocalGet(local_id)
                                                } else {
                                                    continue;
                                                };
                                            match key.as_str() {
                                                "method" => method = value,
                                                "body" => body = value,
                                                "signal" => signal = Some(Box::new(value)),
                                                _ => {}
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            // Create a FetchWithOptions expression
                            ctx.uses_fetch = true;
                            return Ok(Ok(Expr::FetchWithOptions {
                                url: Box::new(url),
                                method: Box::new(method),
                                body: Box::new(body),
                                headers: headers_obj,
                                headers_dynamic,
                                signal,
                            }));
                        }
                    }
                }

                // Simple fetch(url) with no options - use GET
                ctx.uses_fetch = true;
                return Ok(Ok(Expr::FetchWithOptions {
                    url: Box::new(url),
                    method: Box::new(Expr::String("GET".to_string())),
                    body: Box::new(Expr::Undefined),
                    headers: Vec::new(),
                    headers_dynamic: None,
                    signal: None,
                }));
            }
            _ => {} // Fall through to generic handling
        }

        // Check if this is a named import from child_process (e.g., execSync, spawnSync)
        if let Some((module_name, method)) = ctx.lookup_native_module(func_name) {
            // Resolve aliased named imports of native-module functions to their
            // EXPORTED name (`import { join as p } from "path"` → "join") so the
            // per-module `match func_name` arms below match. Without this, an
            // aliased local (`p`) matched no arm and fell through to a generic
            // path that evaluated to `undefined` — e.g. `p(home, ".x").normalize()`.
            let func_name = method.unwrap_or(func_name);
            if module_name == "child_process" {
                match func_name {
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
                    _ => {} // Fall through
                }
            }

            // Check if this is a named import from path (e.g., join, dirname, basename)
            if module_name == "path" {
                match func_name {
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
                            return Ok(Ok(Expr::PathResolve(Box::new(
                                Expr::String(String::new()),
                            ))));
                        }
                        if !args.is_empty() {
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
                    _ => {} // Fall through
                }
            }

            // Check if this is a named import from url (e.g., fileURLToPath)
            if module_name == "url" {
                match func_name {
                    "fileURLToPath"
                        // Only the 1-arg form takes the dedicated fast path. The
                        // 2-arg form `fileURLToPath(url, { windows })` (#2975)
                        // must fall through to the native dispatch table so the
                        // options object reaches the runtime.
                        if args.len() == 1 => {
                            return Ok(Ok(Expr::FileURLToPath(Box::new(
                                args.into_iter().next().unwrap(),
                            ))));
                        }
                    _ => {} // Fall through
                }
            }

            // Check if this is a named import from fs (e.g., existsSync, mkdirSync, etc.)
            if module_name == "fs" {
                match func_name {
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
                        return Ok(Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content))));
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
                    "appendFileSync" if args.len() == 2 => {
                        let mut iter = args.into_iter();
                        let path = iter.next().unwrap();
                        let content = iter.next().unwrap();
                        return Ok(Ok(Expr::FsAppendFileSync(
                            Box::new(path),
                            Box::new(content),
                        )));
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
                    // Issue #648 fallout: see twin arm above.
                    "rmSync" => {}
                    _ => {} // Fall through
                }
            }

            // Named imports from `node:crypto` (e.g.
            // `import { randomFillSync, randomUUID, randomBytes }
            //  from 'node:crypto'`). Without these arms the bare
            // call `randomFillSync(buf)` falls into the generic
            // `NativeMethodCall` path, which has no `crypto`
            // dispatcher and returns `undefined` — so the buffer
            // never gets filled (jose's signing flow silently
            // produces all-zero IVs / nonces). Route each call
            // straight to the dedicated `Expr::CryptoRandom*`
            // variant that the `crypto.xxx(...)` (object-method)
            // arm above (line ~3060) uses, so both call shapes
            // share one codegen path.
            if module_name == "crypto" {
                if super::crypto::is_passthrough_method(func_name) {
                    if let Some(expr) = super::crypto::lower_crypto_passthrough(
                        func_name,
                        std::mem::take(&mut args),
                    ) {
                        return Ok(Ok(expr));
                    }
                }
                match func_name {
                    // `createSecretKey(key, encoding?)` from a named
                    // import. Without this arm the call lowered to a
                    // generic NativeMethodCall with no dispatcher for
                    // `crypto.createSecretKey`, so the call returned
                    // undefined and `jose.sign(undefined)` threw
                    // "Received undefined". Reuse the same PropertyGet
                    // shape that the `crypto.createSecretKey(...)`
                    // call-site form already exercises (handled in
                    // `expr.rs` near the createHash/createHmac block)
                    // so both shapes share one codegen path.
                    "createSecretKey" if !args.is_empty() => {
                        let mut iter = args.into_iter();
                        let key_arg = iter.next().unwrap();
                        let mut new_args = vec![key_arg];
                        if let Some(enc) = iter.next() {
                            new_args.push(enc);
                        }
                        return Ok(Ok(Expr::Call {
                            callee: Box::new(Expr::PropertyGet {
                                byte_offset: 0,
                                object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                                property: "createSecretKey".to_string(),
                            }),
                            args: new_args,
                            type_args: vec![],
                            byte_offset: 0,
                        }));
                    }
                    // #3927: `generateKeySync(alg, options)` from a named import.
                    // Like createSecretKey above, rewrite to the dotted-form
                    // `crypto.generateKeySync(...)` so the call reaches the
                    // dedicated `js_crypto_generate_key_sync` dispatch in
                    // `expr/calls.rs` (a generic NativeMethodCall has no runtime
                    // dispatcher for it and returns undefined).
                    "generateKeySync" if args.len() >= 2 => {
                        let mut iter = args.into_iter();
                        let alg_arg = iter.next().unwrap();
                        let options_arg = iter.next().unwrap();
                        return Ok(Ok(Expr::Call {
                            callee: Box::new(Expr::PropertyGet {
                                byte_offset: 0,
                                object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
                                property: "generateKeySync".to_string(),
                            }),
                            args: vec![alg_arg, options_arg],
                            type_args: vec![],
                            byte_offset: 0,
                        }));
                    }
                    _ => {} // Fall through
                }
            }
        }

        // Issue #1123 — `import { createServer } from "node:net";
        // createServer(handler)` (named-import form). Pre-fix this
        // fell through to the generic `Expr::NativeMethodCall`
        // arm right below, and the LLVM codegen's
        // `lower_native_method_call` had no `("net", "createServer")`
        // row in `NATIVE_MODULE_TABLE` (unlike the http sibling at
        // `lower_call.rs:10620`), so the call dropped through every
        // path and returned `TAG_UNDEFINED`. Synthesize the same
        // `Expr::NetCreateServer` node the dotted form
        // (`net.createServer(...)`) produces at sites 1899 / 3393,
        // so both forms converge on the new codegen arm in
        // `crates/perry-codegen/src/expr.rs` (the `Expr::FetchPostWithAuth`
        // neighbor, added in the same fix). `createServer` accepts
        // either `(listener)` or `(options, listener)`; mirror the
        // dotted-form positional handling: 1 arg → listener-only,
        // 2+ args → first is options, second is listener.
        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
            if module_name == "net" && method_name == "createServer" {
                let (options, connection_listener) = if args.len() >= 2 {
                    let mut args_iter = args.into_iter();
                    let opts = args_iter.next().map(Box::new);
                    let listener = args_iter.next().map(Box::new);
                    (opts, listener)
                } else {
                    // 0 or 1 args — treat the single arg (if any) as
                    // the connection listener. Matches Node's
                    // `net.createServer(connectionListener)` shorthand.
                    let listener = args.into_iter().next().map(Box::new);
                    (None, listener)
                };
                return Ok(Ok(Expr::NetCreateServer {
                    options,
                    connection_listener,
                }));
            }
        }

        // Check if this is a direct call on an aliased named import
        // e.g., uuid() where import { v4 as uuid } from 'uuid'
        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
            if module_name == "os" || module_name == "node:os" {
                match method_name {
                    "availableParallelism" => return Ok(Ok(Expr::OsAvailableParallelism)),
                    "platform" => return Ok(Ok(Expr::OsPlatform)),
                    "arch" => return Ok(Ok(Expr::OsArch)),
                    "endianness" => return Ok(Ok(Expr::OsEndianness)),
                    "hostname" => return Ok(Ok(Expr::OsHostname)),
                    "homedir" => return Ok(Ok(Expr::OsHomedir)),
                    "tmpdir" => return Ok(Ok(Expr::OsTmpdir)),
                    "loadavg" => return Ok(Ok(Expr::OsLoadavg)),
                    "machine" => return Ok(Ok(Expr::OsMachine)),
                    "totalmem" => return Ok(Ok(Expr::OsTotalmem)),
                    "freemem" => return Ok(Ok(Expr::OsFreemem)),
                    "uptime" => return Ok(Ok(Expr::OsUptime)),
                    "type" => return Ok(Ok(Expr::OsType)),
                    "release" => return Ok(Ok(Expr::OsRelease)),
                    "version" => return Ok(Ok(Expr::OsVersion)),
                    "cpus" => return Ok(Ok(Expr::OsCpus)),
                    "networkInterfaces" => return Ok(Ok(Expr::OsNetworkInterfaces)),
                    "userInfo" => return Ok(Ok(user_info_expr_for_call(call, args))),
                    "getPriority" | "setPriority" => {
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: "os".to_string(),
                            class_name: None,
                            object: None,
                            method: method_name.to_string(),
                            args,
                        }));
                    }
                    _ => {}
                }
            }
            return Ok(Ok(Expr::NativeMethodCall {
                module: module_name.to_string(),
                class_name: None,
                object: None,
                method: method_name.to_string(),
                args,
            }));
        }

        // Check if this is a call on a default import from a native module
        // e.g., Fastify() where import Fastify from 'fastify'
        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
            return Ok(Ok(Expr::NativeMethodCall {
                module: module_name.to_string(),
                class_name: None,
                object: None,
                method: "default".to_string(), // Use "default" for default export calls
                args,
            }));
        }
    }

    // Fall through to the shared call tail. It owns lowering the callee for
    // the generic dispatcher; doing it speculatively here relowers every
    // receiver in a fluent generic chain and turns `a.b().c().d()` into an
    // exponential lowering walk.
    Ok(Err(args))
}

/// An omitted argument to a JS global is `undefined`, not an error — mirrors the
/// `parseInt` / `parseFloat` / `BigInt` / `Object` padding above.
fn arg_or_undefined(args: &mut Vec<Expr>) -> Expr {
    if args.is_empty() {
        Expr::Undefined
    } else {
        args.remove(0)
    }
}
