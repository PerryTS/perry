//! Nested-namespace member dispatch: `process.hrtime.bigint()`,
//! `crypto.subtle.<method>(...)`, `path.posix/win32.<method>(...)`,
//! and namespace aliases that should lower to a canonical module key.
//!
//! These need a 3-level Member AST shape and are resolved BEFORE the
//! generic `mod.X.Y()` arm so the strict-API gate (#463) doesn't
//! reject sub-namespaces (`subtle`, `posix`, `win32` are not classes).
//!
//! Each helper takes `args` by value and returns
//! `Result<Result<Expr, Vec<Expr>>>` — `Ok(Ok(expr))` if it matched
//! and the caller should return that expression, `Ok(Err(args))` if
//! it didn't match and the caller should keep going. Extracted from
//! `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::LoweringContext;

/// `process.hrtime.bigint()` — nested 3-level member call.
pub(super) fn try_process_hrtime_bigint(
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Expr, Vec<Expr>> {
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(inner_obj) = inner_member.obj.as_ref() {
                if inner_obj.sym.as_ref() == "process" {
                    if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                        if inner_prop.sym.as_ref() == "hrtime" {
                            if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                if method_ident.sym.as_ref() == "bigint" {
                                    return Ok(Expr::ProcessHrtimeBigint);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(args)
}

/// `process.memoryUsage.rss()` — Node's fast-path that returns just the
/// RSS as a number instead of allocating the full `MemoryUsage` object
/// (issue #1395). AST shape mirrors `process.hrtime.bigint()` above.
///
/// Implementation: lower to `(process.memoryUsage()).rss`. Same value
/// Node's fast path returns; we don't have the no-allocation fast path
/// but parity tests only care about the numeric result.
pub(super) fn try_process_memory_usage_rss(
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Expr, Vec<Expr>> {
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(inner_obj) = inner_member.obj.as_ref() {
                if inner_obj.sym.as_ref() == "process" {
                    if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                        if inner_prop.sym.as_ref() == "memoryUsage" {
                            if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                if method_ident.sym.as_ref() == "rss" {
                                    return Ok(Expr::PropertyGet {
                                        object: Box::new(Expr::ProcessMemoryUsage),
                                        property: "rss".to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Err(args)
}

/// Web Crypto API — `crypto.subtle.<method>(args)` (issue #561).
/// AST shape is the same nested-Member pattern as
/// `process.hrtime.bigint()` above. We resolve here BEFORE the
/// generic mod.X.Y() arm so the strict-API gate (#463) doesn't
/// reject `subtle` (which is a sub-namespace, not a class).
pub(super) fn try_web_crypto_subtle(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                let root_name = root_ident.sym.as_ref();
                let is_crypto_root = root_name == "crypto"
                    || ctx.lookup_builtin_module_alias(root_name) == Some("crypto")
                    || ctx
                        .lookup_native_module(root_name)
                        .map(|(m, _)| m == "crypto")
                        .unwrap_or(false);
                if is_crypto_root {
                    if let ast::MemberProp::Ident(inner_prop) = &inner_member.prop {
                        if inner_prop.sym.as_ref() == "subtle" {
                            if let ast::MemberProp::Ident(method_ident) = &outer_member.prop {
                                let method = method_ident.sym.as_ref();
                                match method {
                                    "digest" if args.len() >= 2 => {
                                        let mut iter = args.into_iter();
                                        let algo = iter.next().unwrap();
                                        let data = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoDigest {
                                            algo: Box::new(algo),
                                            data: Box::new(data),
                                        }));
                                    }
                                    "importKey" if args.len() >= 5 => {
                                        let mut iter = args.into_iter();
                                        let format = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let algorithm = iter.next().unwrap();
                                        let extractable = iter.next().unwrap();
                                        let usages = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoImportKey {
                                            format: Box::new(format),
                                            key: Box::new(key),
                                            algorithm: Box::new(algorithm),
                                            extractable: Box::new(extractable),
                                            usages: Box::new(usages),
                                        }));
                                    }
                                    "exportKey" if args.len() >= 2 => {
                                        let mut iter = args.into_iter();
                                        let format = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoExportKey {
                                            format: Box::new(format),
                                            key: Box::new(key),
                                        }));
                                    }
                                    "sign" if args.len() >= 3 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let data = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoSign {
                                            algorithm: Box::new(algorithm),
                                            key: Box::new(key),
                                            data: Box::new(data),
                                        }));
                                    }
                                    "verify" if args.len() >= 4 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let signature = iter.next().unwrap();
                                        let data = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoVerify {
                                            algorithm: Box::new(algorithm),
                                            key: Box::new(key),
                                            signature: Box::new(signature),
                                            data: Box::new(data),
                                        }));
                                    }
                                    "deriveBits" if args.len() >= 3 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let base_key = iter.next().unwrap();
                                        let length = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoDeriveBits {
                                            algorithm: Box::new(algorithm),
                                            base_key: Box::new(base_key),
                                            length: Box::new(length),
                                        }));
                                    }
                                    "deriveKey" if args.len() >= 5 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let base_key = iter.next().unwrap();
                                        let derived_key_algorithm = iter.next().unwrap();
                                        let extractable = iter.next().unwrap();
                                        let usages = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoDeriveKey {
                                            algorithm: Box::new(algorithm),
                                            base_key: Box::new(base_key),
                                            derived_key_algorithm: Box::new(derived_key_algorithm),
                                            extractable: Box::new(extractable),
                                            usages: Box::new(usages),
                                        }));
                                    }
                                    "encrypt" if args.len() >= 3 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let data = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoEncrypt {
                                            algorithm: Box::new(algorithm),
                                            key: Box::new(key),
                                            data: Box::new(data),
                                        }));
                                    }
                                    "decrypt" if args.len() >= 3 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let data = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoDecrypt {
                                            algorithm: Box::new(algorithm),
                                            key: Box::new(key),
                                            data: Box::new(data),
                                        }));
                                    }
                                    "generateKey" if args.len() >= 3 => {
                                        let mut iter = args.into_iter();
                                        let algorithm = iter.next().unwrap();
                                        let extractable = iter.next().unwrap();
                                        let usages = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoGenerateKey {
                                            algorithm: Box::new(algorithm),
                                            extractable: Box::new(extractable),
                                            usages: Box::new(usages),
                                        }));
                                    }
                                    "wrapKey" if args.len() >= 4 => {
                                        let mut iter = args.into_iter();
                                        let format = iter.next().unwrap();
                                        let key = iter.next().unwrap();
                                        let wrapping_key = iter.next().unwrap();
                                        let wrap_algorithm = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoWrapKey {
                                            format: Box::new(format),
                                            key: Box::new(key),
                                            wrapping_key: Box::new(wrapping_key),
                                            wrap_algorithm: Box::new(wrap_algorithm),
                                        }));
                                    }
                                    "unwrapKey" if args.len() >= 7 => {
                                        let mut iter = args.into_iter();
                                        let format = iter.next().unwrap();
                                        let wrapped_key = iter.next().unwrap();
                                        let unwrapping_key = iter.next().unwrap();
                                        let unwrap_algorithm = iter.next().unwrap();
                                        let unwrapped_key_algorithm = iter.next().unwrap();
                                        let extractable = iter.next().unwrap();
                                        let usages = iter.next().unwrap();
                                        return Ok(Ok(Expr::WebCryptoUnwrapKey {
                                            format: Box::new(format),
                                            wrapped_key: Box::new(wrapped_key),
                                            unwrapping_key: Box::new(unwrapping_key),
                                            unwrap_algorithm: Box::new(unwrap_algorithm),
                                            unwrapped_key_algorithm: Box::new(
                                                unwrapped_key_algorithm,
                                            ),
                                            extractable: Box::new(extractable),
                                            usages: Box::new(usages),
                                        }));
                                    }
                                    _ => {
                                        // Unsupported subtle method —
                                        // fail loudly. The supported
                                        // surface is documented in the
                                        // d.ts and at #561; asymmetric
                                        // (RSA-PSS / ECDSA / RSA-OAEP),
                                        // deriveKey are still out of
                                        // scope per the issue.
                                        let allow_unimplemented =
                                            std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                                        if !allow_unimplemented {
                                            let msg = format!(
                                                "`crypto.subtle.{}` is not implemented in Perry — supported subtle methods are digest, importKey, sign, verify, encrypt, decrypt, generateKey, wrapKey, unwrapKey (HMAC + SHA-1/256/384/512; encrypt/decrypt/generateKey/wrapKey currently AES-GCM/AES-KW only). \
                                                 See `perry --print-api-manifest` and #561, or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore.",
                                                method,
                                            );
                                            // #2309: defer under tree-shaking.
                                            if !crate::try_defer_refusal(
                                                msg.clone(),
                                                outer_member.span.lo.0,
                                            ) {
                                                crate::lower_bail!(outer_member.span, "{}", msg);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(Err(args))
}

/// `util.types.<method>(args)` — normalize the namespace-access form to the
/// direct `util/types` module key used by `import { isX } from "util/types"`.
///
/// Keeping this rewrite in HIR means downstream dispatch tables and API docs
/// only need the canonical module name; `util.types` remains a runtime object
/// property, not a second API-manifest module.
pub(super) fn try_util_types_namespace(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let ast::Expr::Member(outer_member) = expr {
        if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
            if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
                let root_name = root_ident.sym.as_ref();
                let is_util_root = root_name == "util"
                    || ctx.lookup_builtin_module_alias(root_name) == Some("util")
                    || ctx
                        .lookup_native_module(root_name)
                        .map(|(m, _)| m == "util")
                        .unwrap_or(false);
                if is_util_root {
                    if let (ast::MemberProp::Ident(namespace), ast::MemberProp::Ident(method)) =
                        (&inner_member.prop, &outer_member.prop)
                    {
                        if namespace.sym.as_ref() == "types" {
                            let method_name = method.sym.as_ref();
                            let allow_unimplemented =
                                std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                            if !allow_unimplemented
                                && perry_api_manifest::module_has_symbol("util/types", method_name)
                                    .is_none()
                            {
                                let msg = format!(
                                    "`util.types.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                     or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463)",
                                    method_name,
                                );
                                // #2309: defer under tree-shaking.
                                if !crate::try_defer_refusal(msg.clone(), outer_member.span.lo.0) {
                                    crate::lower_bail!(outer_member.span, "{}", msg);
                                }
                            }
                            return Ok(Ok(Expr::NativeMethodCall {
                                module: "util/types".to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            }));
                        }
                    }
                }
            }
        }
    }
    Ok(Err(args))
}

/// `path.posix.<method>(args)` / `path.win32.<method>(args)` —
/// sub-namespace dispatch (issue #810). Without this arm the
/// generic mod.X.Y() block below skips the call (path.posix /
/// path.win32 are in its sub-namespace exclusion list to keep
/// them off the strict-API gate) and the call falls through
/// to the receiver-less dispatch, returning undefined.
///
/// - `path.posix.X` routes to the existing Expr::PathX variant.
///   The runtime js_path_* functions use POSIX (`/`) semantics,
///   so this is a direct shape rewrite.
/// - `path.win32.join` routes to a dedicated Expr::PathWin32Join
///   so the result uses `\` separators. Other path.win32.<method>
///   calls route to the generic `Expr::PathWin32` carrier, which
///   codegen dispatches to `js_path_win32_*` (issue #1162).
/// True when `root_name` refers to the `node:path` module — directly, via a
/// builtin-module alias, or via a namespace import (`import * as p from
/// "node:path"`).
fn is_path_root(ctx: &LoweringContext, root_name: &str) -> bool {
    root_name == "path"
        || ctx.lookup_builtin_module_alias(root_name) == Some("path")
        || ctx
            .lookup_native_module(root_name)
            .map(|(m, _)| m == "path")
            .unwrap_or(false)
}

/// Core dispatch for `path.<sub>.<method>(...)` where `sub` is `"win32"` or
/// `"posix"`. Shared by the direct member form and the aliased-local form
/// (`const w = path.win32; w.<method>(...)`, #1750). Returns `Err(args)` when
/// no method matches so the caller can fall through to generic lowering.
fn dispatch_path_subnamespace(sub: &str, method: &str, args: Vec<Expr>) -> Result<Expr, Vec<Expr>> {
    // path.<sub>.join(...)
    if method == "join" {
        if args.is_empty() {
            return Ok(Expr::String(".".to_string()));
        }
        if sub == "win32" {
            let mut iter = args.into_iter();
            let mut result = iter.next().unwrap();
            for next_arg in iter {
                result = Expr::PathWin32Join(Box::new(result), Box::new(next_arg));
            }
            return Ok(result);
        } else {
            // posix.join → existing PathJoin
            if args.len() == 1 {
                return Ok(Expr::PathNormalize(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            let mut iter = args.into_iter();
            let mut result = iter.next().unwrap();
            for next_arg in iter {
                result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
            }
            return Ok(result);
        }
    }

    // path.win32.<method> — issue #1162. Route every supported method to the
    // generic `Expr::PathWin32` carrier; codegen dispatches to the matching
    // `js_path_win32_*` runtime function.
    if sub == "win32" {
        use crate::ir::PathWin32Method as M;
        let m = match method {
            "dirname" if !args.is_empty() => Some(M::Dirname),
            "basename" if args.len() >= 2 => Some(M::BasenameExt),
            "basename" if !args.is_empty() => Some(M::Basename),
            "extname" if !args.is_empty() => Some(M::Extname),
            "isAbsolute" if !args.is_empty() => Some(M::IsAbsolute),
            "normalize" if !args.is_empty() => Some(M::Normalize),
            "parse" if !args.is_empty() => Some(M::Parse),
            "format" if !args.is_empty() => Some(M::Format),
            "relative" if args.len() >= 2 => Some(M::Relative),
            "toNamespacedPath" if !args.is_empty() => Some(M::ToNamespacedPath),
            "matchesGlob" if args.len() >= 2 => Some(M::MatchesGlob),
            _ => None,
        };
        if let Some(m) = m {
            return Ok(Expr::PathWin32 { method: m, args });
        }
        // resolve has multi-arg chaining like POSIX — first arg seeds,
        // remaining fold via ResolveJoin, final via Resolve.
        if method == "resolve" {
            if args.is_empty() {
                return Ok(Expr::PathWin32 {
                    method: M::Resolve,
                    args: vec![Expr::String(String::new())],
                });
            }
            let mut it = args.into_iter();
            let first = it.next().unwrap();
            let mut joined = first;
            for next_arg in it {
                joined = Expr::PathWin32 {
                    method: M::ResolveJoin,
                    args: vec![joined, next_arg],
                };
            }
            return Ok(Expr::PathWin32 {
                method: M::Resolve,
                args: vec![joined],
            });
        }
    }
    // The remaining methods route to the existing POSIX Expr::Path* variants
    // only for the `posix` sub-namespace.
    if sub == "posix" {
        match method {
            "dirname" if !args.is_empty() => {
                return Ok(Expr::PathDirname(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "basename" if args.len() >= 2 => {
                let mut it = args.into_iter();
                let p = it.next().unwrap();
                let e = it.next().unwrap();
                return Ok(Expr::PathBasenameExt(Box::new(p), Box::new(e)));
            }
            "basename" if !args.is_empty() => {
                return Ok(Expr::PathBasename(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "extname" if !args.is_empty() => {
                return Ok(Expr::PathExtname(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "isAbsolute" if !args.is_empty() => {
                return Ok(Expr::PathIsAbsolute(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "normalize" if !args.is_empty() => {
                return Ok(Expr::PathNormalize(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "parse" if !args.is_empty() => {
                return Ok(Expr::PathParse(Box::new(args.into_iter().next().unwrap())));
            }
            "format" if !args.is_empty() => {
                return Ok(Expr::PathFormat(Box::new(args.into_iter().next().unwrap())));
            }
            "toNamespacedPath" if !args.is_empty() => {
                return Ok(Expr::PathToNamespacedPath(Box::new(
                    args.into_iter().next().unwrap(),
                )));
            }
            "relative" if args.len() >= 2 => {
                let mut it = args.into_iter();
                let from = it.next().unwrap();
                let to = it.next().unwrap();
                return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
            }
            "resolve" if args.is_empty() => {
                return Ok(Expr::PathResolve(Box::new(Expr::String(String::new()))));
            }
            "resolve" if !args.is_empty() => {
                let mut it = args.into_iter();
                let first = it.next().unwrap();
                let mut joined = first;
                for next_arg in it {
                    joined = Expr::PathResolveJoin(Box::new(joined), Box::new(next_arg));
                }
                return Ok(Expr::PathResolve(Box::new(joined)));
            }
            "matchesGlob" if args.len() >= 2 => {
                let mut it = args.into_iter();
                let p = it.next().unwrap();
                let pat = it.next().unwrap();
                return Ok(Expr::PathMatchesGlob(Box::new(p), Box::new(pat)));
            }
            _ => {}
        }
    }
    Err(args)
}

pub(super) fn try_path_subnamespace(
    ctx: &LoweringContext,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Expr, Vec<Expr>> {
    let ast::Expr::Member(outer_member) = expr else {
        return Err(args);
    };
    let ast::MemberProp::Ident(method_prop) = &outer_member.prop else {
        return Err(args);
    };
    let method = method_prop.sym.as_ref();

    // Direct form: `path.<sub>.<method>(...)` — a 3-level member where the
    // root identifier resolves to the path module.
    if let ast::Expr::Member(inner_member) = outer_member.obj.as_ref() {
        if let ast::Expr::Ident(root_ident) = inner_member.obj.as_ref() {
            if is_path_root(ctx, root_ident.sym.as_ref()) {
                if let ast::MemberProp::Ident(sub_prop) = &inner_member.prop {
                    let sub = sub_prop.sym.as_ref();
                    if sub == "posix" || sub == "win32" {
                        return dispatch_path_subnamespace(sub, method, args);
                    }
                }
            }
        }
    }

    // Aliased form (#1750): `const w = path.<sub>; w.<method>(...)` — a 2-level
    // member whose receiver is a local recorded as a path sub-namespace alias.
    if let ast::Expr::Ident(recv) = outer_member.obj.as_ref() {
        if let Some((root, sub)) = ctx.lookup_subns_path_alias(recv.sym.as_ref()) {
            let (root, sub) = (root.to_string(), sub.to_string());
            if is_path_root(ctx, &root) {
                return dispatch_path_subnamespace(&sub, method, args);
            }
        }
    }

    Err(args)
}
