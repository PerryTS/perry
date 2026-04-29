//! Member expression lowering: `ast::Expr::Member`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 405-LOC `Member` arm
//! from `lower_expr`. Member expressions cover `obj.prop`,
//! `obj["key"]`, `obj[i]`, the namespace-form `Math.PI`, enum member
//! access (`Color.Red`), private field reads (`#field`), and a fast
//! path for `Symbol.iterator` / `Symbol.asyncIterator` / friends.
//! The arm is mostly a long match cascade: identify the receiver kind
//! (regular object vs class static vs enum vs builtin namespace) then
//! emit the right HIR variant.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::Expr;

use super::{lower_expr, LoweringContext};

pub(super) fn lower_member(ctx: &mut LoweringContext, member: &ast::MemberExpr) -> Result<Expr> {
    // Check if this is process.* property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "process" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                match prop_ident.sym.as_ref() {
                    "argv" => return Ok(Expr::ProcessArgv),
                    "platform" => return Ok(Expr::OsPlatform),
                    "arch" => return Ok(Expr::OsArch),
                    "pid" => return Ok(Expr::ProcessPid),
                    "ppid" => return Ok(Expr::ProcessPpid),
                    "version" => return Ok(Expr::ProcessVersion),
                    "versions" => return Ok(Expr::ProcessVersions),
                    "stdin" => return Ok(Expr::ProcessStdin),
                    "stdout" => return Ok(Expr::ProcessStdout),
                    "stderr" => return Ok(Expr::ProcessStderr),
                    "env" => return Ok(Expr::ProcessEnv),
                    _ => {}
                }
            }
        }
        // `globalThis.process` returns an object whose `.env`/`.argv`/
        // etc. should resolve just like bare `process.*`. Without this
        // shim, `globalThis.process.env` walks through generic
        // PropertyGet dispatch and hits a 0.0 sentinel. Matches the
        // static `process.env` fast path above.
        if obj_ident.sym.as_ref() == "globalThis" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                if prop_ident.sym.as_ref() == "process" {
                    // `globalThis.process` on its own — fall through
                    // to generic handling below (returns 0.0 sentinel,
                    // which is fine as the outer chain handles env/etc.).
                }
            }
        }
    }
    // Handle `globalThis.process.X` (and any PropertyGet whose object
    // resolves to `globalThis.process`): treat the outer `.X` as if
    // it were a bare `process.X` access. Unwraps transparent TS
    // wrappers (TsAs, TsNonNull, TsSatisfies, TsTypeAssertion, Paren)
    // so that `(globalThis as any).process.env` works too.
    fn unwrap_transparent(e: &ast::Expr) -> &ast::Expr {
        let mut cur = e;
        loop {
            match cur {
                ast::Expr::TsAs(x) => cur = &x.expr,
                ast::Expr::TsNonNull(x) => cur = &x.expr,
                ast::Expr::TsSatisfies(x) => cur = &x.expr,
                ast::Expr::TsTypeAssertion(x) => cur = &x.expr,
                ast::Expr::TsConstAssertion(x) => cur = &x.expr,
                ast::Expr::Paren(x) => cur = &x.expr,
                _ => return cur,
            }
        }
    }
    let member_obj_unwrapped = unwrap_transparent(member.obj.as_ref());
    if let ast::Expr::Member(inner) = member_obj_unwrapped {
        let inner_obj_unwrapped = unwrap_transparent(inner.obj.as_ref());
        let inner_is_global_process = matches!(
            inner_obj_unwrapped,
            ast::Expr::Ident(i) if i.sym.as_ref() == "globalThis"
        ) && matches!(
            &inner.prop,
            ast::MemberProp::Ident(p) if p.sym.as_ref() == "process"
        );
        if inner_is_global_process {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                match prop_ident.sym.as_ref() {
                    "argv" => return Ok(Expr::ProcessArgv),
                    "platform" => return Ok(Expr::OsPlatform),
                    "arch" => return Ok(Expr::OsArch),
                    "pid" => return Ok(Expr::ProcessPid),
                    "ppid" => return Ok(Expr::ProcessPpid),
                    "version" => return Ok(Expr::ProcessVersion),
                    "versions" => return Ok(Expr::ProcessVersions),
                    "env" => return Ok(Expr::ProcessEnv),
                    _ => {}
                }
            }
        }
    }

    // Check if this is Symbol.<well-known> — Symbol.toPrimitive,
    // Symbol.hasInstance, Symbol.toStringTag, Symbol.iterator,
    // Symbol.asyncIterator, Symbol.dispose, Symbol.asyncDispose.
    // Lowered to `SymbolFor(String("@@__perry_wk_<name>"))` which the
    // runtime's `js_symbol_for` sniffs via prefix and resolves from
    // the well-known cache (not the registry). Gives each well-known
    // symbol a stable pointer without needing a new HIR variant.
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Symbol" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let prop_name = prop_ident.sym.as_ref();
                if matches!(
                    prop_name,
                    "toPrimitive"
                        | "hasInstance"
                        | "toStringTag"
                        | "iterator"
                        | "asyncIterator"
                        | "dispose"
                        | "asyncDispose"
                ) {
                    return Ok(Expr::SymbolFor(Box::new(Expr::String(format!(
                        "@@__perry_wk_{}",
                        prop_name
                    )))));
                }
            }
        }
    }

    // Check if this is path.sep / path.delimiter constant access
    // (where `path` is an imported alias of the node:path module).
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        let is_path_module =
            obj_name == "path" || ctx.lookup_builtin_module_alias(&obj_name) == Some("path");
        if is_path_module {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                match prop_ident.sym.as_ref() {
                    "sep" => return Ok(Expr::PathSep),
                    "delimiter" => return Ok(Expr::PathDelimiter),
                    _ => {}
                }
            }
        }
    }

    // Check if this is a process.env.VARNAME or process.env[expr] access
    if let ast::Expr::Member(inner_member) = member.obj.as_ref() {
        if let ast::Expr::Ident(obj_ident) = inner_member.obj.as_ref() {
            if obj_ident.sym.as_ref() == "process" {
                if let ast::MemberProp::Ident(prop_ident) = &inner_member.prop {
                    if prop_ident.sym.as_ref() == "env" {
                        // This is process.env access
                        match &member.prop {
                            ast::MemberProp::Ident(var_ident) => {
                                // process.env.VARNAME (static key)
                                let var_name = var_ident.sym.to_string();
                                return Ok(Expr::EnvGet(var_name));
                            }
                            ast::MemberProp::Computed(computed) => {
                                // process.env[expr] (dynamic key)
                                let key_expr = Box::new(lower_expr(ctx, &computed.expr)?);
                                return Ok(Expr::EnvGetDynamic(key_expr));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Check for Math constants (e.g., Math.PI, Math.E)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Math" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let val = match prop_ident.sym.as_ref() {
                    "PI" => Some(std::f64::consts::PI),
                    "E" => Some(std::f64::consts::E),
                    "LN2" => Some(std::f64::consts::LN_2),
                    "LN10" => Some(std::f64::consts::LN_10),
                    "LOG2E" => Some(std::f64::consts::LOG2_E),
                    "LOG10E" => Some(std::f64::consts::LOG10_E),
                    "SQRT2" => Some(std::f64::consts::SQRT_2),
                    "SQRT1_2" => Some(std::f64::consts::FRAC_1_SQRT_2),
                    _ => None,
                };
                if let Some(v) = val {
                    return Ok(Expr::Number(v));
                }
            }
        }
    }

    // Check for Number constants (e.g., Number.MAX_SAFE_INTEGER)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        if obj_ident.sym.as_ref() == "Number" {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let val = match prop_ident.sym.as_ref() {
                    "MAX_SAFE_INTEGER" => Some(9007199254740991.0),
                    "MIN_SAFE_INTEGER" => Some(-9007199254740991.0),
                    "MAX_VALUE" => Some(f64::MAX),
                    "MIN_VALUE" => Some(f64::MIN_POSITIVE),
                    "EPSILON" => Some(f64::EPSILON),
                    "POSITIVE_INFINITY" => Some(f64::INFINITY),
                    "NEGATIVE_INFINITY" => Some(f64::NEG_INFINITY),
                    "NaN" => Some(f64::NAN),
                    _ => None,
                };
                if let Some(v) = val {
                    return Ok(Expr::Number(v));
                }
            }
        }
    }

    // Check if this is an enum member access (e.g., Color.Red)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if ctx.lookup_enum(&obj_name).is_some() {
            // This is an enum access
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let member_name = prop_ident.sym.to_string();
                return Ok(Expr::EnumMember {
                    enum_name: obj_name,
                    member_name,
                });
            }
        }
    }

    // Check if this is a static field access (e.g., Counter.count)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if ctx.lookup_class(&obj_name).is_some() {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let field_name = prop_ident.sym.to_string();
                if ctx.has_static_field(&obj_name, &field_name) {
                    return Ok(Expr::StaticFieldGet {
                        class_name: obj_name,
                        field_name,
                    });
                }
            }
        }
    }

    // Check if this is a namespace variable access (e.g., Flag.OPENCODE_AUTO_SHARE)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            let member_name = prop_ident.sym.to_string();
            if let Some(local_id) = ctx.lookup_namespace_var(&obj_name, &member_name) {
                return Ok(Expr::LocalGet(local_id));
            }
        }
    }

    // Check if this is os.EOL property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.as_ref();
        let is_os_module =
            obj_name == "os" || ctx.lookup_builtin_module_alias(obj_name) == Some("os");
        if is_os_module {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                if prop_ident.sym.as_ref() == "EOL" {
                    return Ok(Expr::OsEOL);
                }
            }
        }
    }

    // --- Proxy property get: `p.foo` / `p[k]` for known proxy locals ---
    {
        fn unwrap_member_obj<'a>(mut e: &'a ast::Expr) -> &'a ast::Expr {
            loop {
                match e {
                    ast::Expr::TsAs(ts_as) => e = &ts_as.expr,
                    ast::Expr::TsNonNull(nn) => e = &nn.expr,
                    ast::Expr::TsConstAssertion(ca) => e = &ca.expr,
                    ast::Expr::TsTypeAssertion(ta) => e = &ta.expr,
                    ast::Expr::Paren(p) => e = &p.expr,
                    _ => break,
                }
            }
            e
        }
        let inner = unwrap_member_obj(member.obj.as_ref());
        if let ast::Expr::Ident(obj_ident) = inner {
            let obj_name = obj_ident.sym.to_string();
            if ctx.proxy_locals.contains(&obj_name) {
                let proxy_expr = if let Some(id) = ctx.lookup_local(&obj_name) {
                    Expr::LocalGet(id)
                } else {
                    lower_expr(ctx, &member.obj)?
                };
                let key_expr = match &member.prop {
                    ast::MemberProp::Ident(i) => Expr::String(i.sym.to_string()),
                    ast::MemberProp::Computed(c) => lower_expr(ctx, &c.expr)?,
                    ast::MemberProp::PrivateName(pn) => {
                        Expr::String(format!("#{}", pn.name.as_str()))
                    }
                };
                return Ok(Expr::ProxyGet {
                    proxy: Box::new(proxy_expr),
                    key: Box::new(key_expr),
                });
            }
        }
    }

    // Check for native instance property access (e.g., response.status, response.ok)
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        // Clone module_name early to avoid borrow issues
        let native_instance = ctx
            .lookup_native_instance(&obj_name)
            .map(|(m, _c)| m.to_string());
        if let Some(module_name) = native_instance {
            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                let property_name = prop_ident.sym.to_string();
                // For properties that map to FFI functions, generate a NativeMethodCall
                // with no args (property getter)
                let object_expr = lower_expr(ctx, &member.obj)?;
                return Ok(Expr::NativeMethodCall {
                    module: module_name,
                    class_name: None,
                    object: Some(Box::new(object_expr)),
                    method: property_name,
                    args: Vec::new(),
                });
            }
        }
    }

    // TextEncoder / TextDecoder property access
    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
        let obj_name = obj_ident.sym.to_string();
        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
            let prop_name = prop_ident.sym.as_ref();
            let is_text_encoder = ctx
                .lookup_local_type(&obj_name)
                .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                .unwrap_or(false);
            let is_text_decoder = ctx
                .lookup_local_type(&obj_name)
                .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                .unwrap_or(false);
            if (is_text_encoder || is_text_decoder) && prop_name == "encoding" {
                return Ok(Expr::String("utf-8".to_string()));
            }
        }
    }

    // RegExp property access: regex.source / .flags / .lastIndex
    // Detect when receiver is a regex literal or local typed as RegExp.
    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
        let prop_name = prop_ident.sym.as_ref();
        if prop_name == "source" || prop_name == "flags" || prop_name == "lastIndex" {
            let is_regex_obj = match member.obj.as_ref() {
                ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                ast::Expr::Ident(ident) => ctx
                    .lookup_local_type(&ident.sym.to_string())
                    .map(|ty| matches!(ty, Type::Named(n) if n == "RegExp"))
                    .unwrap_or(false),
                _ => false,
            };
            if is_regex_obj {
                let regex_expr = lower_expr(ctx, &member.obj)?;
                if matches!(&regex_expr, Expr::RegExp { .. })
                    || matches!(&regex_expr, Expr::LocalGet(_))
                {
                    return Ok(match prop_name {
                        "source" => Expr::RegExpSource(Box::new(regex_expr)),
                        "flags" => Expr::RegExpFlags(Box::new(regex_expr)),
                        "lastIndex" => Expr::RegExpLastIndex(Box::new(regex_expr)),
                        _ => unreachable!(),
                    });
                }
            }
        }
        // RegExpExecArray.index / .groups — receiver is a local that holds the result
        // of regex.exec(...). The runtime stores the most recent exec metadata in
        // thread-locals which RegExpExecIndex/Groups read.
        if prop_name == "index" || prop_name == "groups" {
            // Strip non-null assertion (m1! → m1)
            let inner = match member.obj.as_ref() {
                ast::Expr::TsNonNull(nn) => nn.expr.as_ref(),
                other => other,
            };
            if let ast::Expr::Ident(ident) = inner {
                if ctx.regex_exec_locals.contains(&ident.sym.to_string()) {
                    return Ok(if prop_name == "index" {
                        Expr::RegExpExecIndex
                    } else {
                        Expr::RegExpExecGroups
                    });
                }
            }
        }
    }

    let object = Box::new(lower_expr(ctx, &member.obj)?);
    match &member.prop {
        ast::MemberProp::Ident(ident) => {
            let property = ident.sym.to_string();
            Ok(Expr::PropertyGet { object, property })
        }
        ast::MemberProp::Computed(computed) => {
            let index = Box::new(lower_expr(ctx, &computed.expr)?);
            // Specialize for Uint8Array/Buffer variables → byte-level access.
            // Params declared `Buffer` (e.g. `function f(src: Buffer)`)
            // reach here with `Type::Named("Buffer")` — treat it as a
            // synonym for Uint8Array so `src[i]` uses the byte-read
            // path instead of the generic f64-element IndexGet, which
            // would return NaN-boxed pointer bits as a denormal f64.
            if let Expr::LocalGet(id) = &*object {
                if let Some((_, _, ty)) = ctx.locals.iter().find(|(_, lid, _)| lid == id) {
                    if matches!(ty, Type::Named(n) if n == "Uint8Array" || n == "Buffer") {
                        return Ok(Expr::Uint8ArrayGet {
                            array: object,
                            index,
                        });
                    }
                }
            }
            Ok(Expr::IndexGet { object, index })
        }
        ast::MemberProp::PrivateName(private) => {
            // Private field access: this.#field -> PropertyGet with "#field"
            let property = format!("#{}", private.name.to_string());
            Ok(Expr::PropertyGet { object, property })
        }
    }
}
