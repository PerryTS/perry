//! Function call expression lowering: `ast::Expr::Call`.
//!
//! Tier 2.3 round 4 (v0.5.339) — extracts the 3,986-LOC `Call` arm
//! from `lower_expr`. By far the largest single arm in the entire
//! codebase. This is a giant dispatcher: figure out what's being
//! called (built-in like Math.floor, native module method like
//! `mysql.query()`, user function, closure, etc.) and emit the right
//! HIR variant.
//!
//! Pattern matches the prior expr_*.rs extractions: free
//! `pub(super) fn` entry, recursion through `super::lower_expr`.
//! Module is intentionally one big function; further sub-extraction
//! by call category (Math / JSON / fetch / native / class-static /
//! …) is a follow-up — splitting them all in a single PR would
//! balloon the diff and the borrow-checker dance is non-trivial.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_patterns::{detect_native_instance_expr, pre_scan_fastify_handler_params};
use crate::lower_types::extract_ts_type_with_ctx;

use super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name,
    lower_expr, resolve_typed_parse_ty, try_desugar_reactive_animate,
    try_desugar_reactive_text, LoweringContext,
};

pub(super) fn lower_call(ctx: &mut LoweringContext, call: &ast::CallExpr) -> Result<Expr> {
            // Check if any argument has spread
            let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

            // Pre-scan: if this call is `<fastify app>.get|post|...|addHook(path, handler)`,
            // the handler is an arrow function whose first two params are
            // the FastifyRequest and FastifyReply. Register them as native
            // instances BEFORE lowering the arrow so that `request.header(...)`
            // and `request.headers[...]` inside the handler dispatch through
            // `Expr::NativeMethodCall` instead of generic object access.
            //
            // In v0.4.51 this was (presumably) handled by the old codegen's
            // per-method dispatch table; in v0.5.x the dispatch happens at
            // HIR lower time via `lookup_native_instance(name)`, so we need
            // the annotation here for the lookup to succeed.
            let fastify_handler_names: Option<(String, String)> = pre_scan_fastify_handler_params(ctx, call);
            if let Some((req_name, reply_name)) = &fastify_handler_names {
                ctx.register_native_instance(req_name.clone(), "fastify".to_string(), "Request".to_string());
                if !reply_name.is_empty() {
                    ctx.register_native_instance(reply_name.clone(), "fastify".to_string(), "Reply".to_string());
                }
            }

            // perry/ui reactive Text: `Text(\`...${state.value}...\`)` where at least one
            // interpolation is `<ident>.value` on a State binding. Desugars to
            // `{ __h = Text(concat); stateOnChange(state, v => textSetString(__h, concat)); __h }`
            // so the label updates when state.set(...) fires subscribers. Closes #104.
            if let Some(desugared) = try_desugar_reactive_text(ctx, call)? {
                return Ok(desugared);
            }

            // perry/ui reactive animation: `widget.animateOpacity(<expr reading
            // state.value>, dur)` or `.animatePosition(...)` desugars to an IIFE
            // that runs the initial animation and registers a `stateOnChange`
            // subscriber per referenced State so the animation re-fires when
            // any read state changes. Closes the follow-up to #109.
            if let Some(desugared) = try_desugar_reactive_animate(ctx, call)? {
                return Ok(desugared);
            }

            let mut args = call.args.iter()
                .map(|arg| lower_expr(ctx, &arg.expr))
                .collect::<Result<Vec<_>>>()?;

            // --- Proxy apply / revoke fast path ---
            if !has_spread {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Ident(ident) = callee_expr.as_ref() {
                        let name = ident.sym.to_string();
                        if ctx.proxy_locals.contains(&name) {
                            if let Some(id) = ctx.lookup_local(&name) {
                                return Ok(Expr::ProxyApply {
                                    proxy: Box::new(Expr::LocalGet(id)),
                                    args,
                                });
                            }
                        }
                        if let Some(proxy_name) = ctx.proxy_revoke_locals.get(&name).cloned() {
                            if let Some(id) = ctx.lookup_local(&proxy_name) {
                                return Ok(Expr::ProxyRevoke(Box::new(Expr::LocalGet(id))));
                            }
                        }
                    }
                }
            }

            // --- Object.prototype.toString.call(x) → js_object_to_string(x) ---
            // AST shape is a four-level member expression:
            //   call.call(x)
            //   ^^^^^^^^^^ outer member: (Object.prototype.toString).call
            // The runtime helper consults the class's `Symbol.toStringTag`
            // getter (registered at module init via `__perry_wk_tostringtag_*`)
            // and returns `[object <tag>]` or the default `[object Object]`.
            if !has_spread && args.len() == 1 {
                if let ast::Callee::Expr(callee_expr) = &call.callee {
                    if let ast::Expr::Member(outer) = callee_expr.as_ref() {
                        if let (ast::MemberProp::Ident(outer_prop), ast::Expr::Member(mid)) =
                            (&outer.prop, outer.obj.as_ref())
                        {
                            if outer_prop.sym.as_ref() == "call" {
                                if let (ast::MemberProp::Ident(mid_prop), ast::Expr::Member(inner)) =
                                    (&mid.prop, mid.obj.as_ref())
                                {
                                    if mid_prop.sym.as_ref() == "toString" {
                                        if let (
                                            ast::MemberProp::Ident(inner_prop),
                                            ast::Expr::Ident(inner_obj),
                                        ) = (&inner.prop, inner.obj.as_ref())
                                        {
                                            if inner_obj.sym.as_ref() == "Object"
                                                && inner_prop.sym.as_ref() == "prototype"
                                            {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::ExternFuncRef {
                                                        name: "js_object_to_string".to_string(),
                                                        param_types: Vec::new(),
                                                        return_type: Type::Any,
                                                    }),
                                                    args: vec![arg],
                                                    type_args: Vec::new(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // If spread is present, create CallSpread instead of Call
            let spread_args: Option<Vec<CallArg>> = if has_spread {
                Some(call.args.iter().zip(args.iter())
                    .map(|(ast_arg, lowered)| {
                        if ast_arg.spread.is_some() {
                            CallArg::Spread(lowered.clone())
                        } else {
                            CallArg::Expr(lowered.clone())
                        }
                    })
                    .collect())
            } else {
                None
            };

            match &call.callee {
                ast::Callee::Super(_) => {
                    // super() call in constructor
                    Ok(Expr::SuperCall(args))
                }
                ast::Callee::Expr(expr) => {
                    // Check for super.method() call
                    if let ast::Expr::SuperProp(super_prop) = expr.as_ref() {
                        if let ast::SuperProp::Ident(ident) = &super_prop.prop {
                            return Ok(Expr::SuperMethodCall {
                                method: ident.sym.to_string(),
                                args,
                            });
                        }
                    }

                    // Check for nested process member calls like process.hrtime.bigint()
                    if let ast::Expr::Member(outer_member) = expr.as_ref() {
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

                    // Check for native module method calls (e.g., mysql.createConnection())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();

                            // Check for process module methods
                            if obj_name == "process" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "uptime" => return Ok(Expr::ProcessUptime),
                                        "cwd" => return Ok(Expr::ProcessCwd),
                                        "memoryUsage" => return Ok(Expr::ProcessMemoryUsage),
                                        "nextTick" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ProcessNextTick(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "on" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let event = iter.next().unwrap();
                                                let handler = iter.next().unwrap();
                                                return Ok(Expr::ProcessOn {
                                                    event: Box::new(event),
                                                    handler: Box::new(handler),
                                                });
                                            }
                                        }
                                        "chdir" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ProcessChdir(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "kill" => {
                                            if args.len() >= 1 {
                                                let mut iter = args.into_iter();
                                                let pid = iter.next().unwrap();
                                                let signal = iter.next().map(Box::new);
                                                return Ok(Expr::ProcessKill {
                                                    pid: Box::new(pid),
                                                    signal,
                                                });
                                            }
                                        }
                                        "exit" => {
                                            // process.exit() / process.exit(code) — never
                                            // returns, terminates the process. Until now this
                                            // fell through to generic NativeMethodCall which
                                            // silently no-op'd, so scripts that rely on it to
                                            // end the event loop (e.g. `main().then(() =>
                                            // process.exit(0))` in a net-socket driver) would
                                            // hang with the socket still keeping the loop alive.
                                            let code = if args.len() >= 1 {
                                                Some(Box::new(args.into_iter().next().unwrap()))
                                            } else {
                                                None
                                            };
                                            return Ok(Expr::ProcessExit(code));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os module methods FIRST (before generic NativeMethodCall)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => return Ok(Expr::OsPlatform),
                                        "arch" => return Ok(Expr::OsArch),
                                        "hostname" => return Ok(Expr::OsHostname),
                                        "homedir" => return Ok(Expr::OsHomedir),
                                        "tmpdir" => return Ok(Expr::OsTmpdir),
                                        "totalmem" => return Ok(Expr::OsTotalmem),
                                        "freemem" => return Ok(Expr::OsFreemem),
                                        "uptime" => return Ok(Expr::OsUptime),
                                        "type" => return Ok(Expr::OsType),
                                        "release" => return Ok(Expr::OsRelease),
                                        "cpus" => return Ok(Expr::OsCpus),
                                        "networkInterfaces" => return Ok(Expr::OsNetworkInterfaces),
                                        "userInfo" => return Ok(Expr::OsUserInfo),
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer static methods
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding
                                            });
                                        }
                                        "alloc" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            let fill = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferAlloc {
                                                size: Box::new(size),
                                                fill
                                            });
                                        }
                                        "allocUnsafe" => {
                                            let size = args.get(0).cloned().unwrap_or(Expr::Number(0.0));
                                            return Ok(Expr::BufferAllocUnsafe(Box::new(size)));
                                        }
                                        "concat" => {
                                            let list = args.get(0).cloned().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::BufferConcat(Box::new(list)));
                                        }
                                        "isBuffer" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::BufferIsBuffer(Box::new(obj)));
                                        }
                                        "byteLength" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::String("".to_string()));
                                            return Ok(Expr::BufferByteLength(Box::new(data)));
                                        }
                                        // `Buffer.compare(a, b)` → `a.compare(b)` instance call
                                        // (handled by runtime buffer dispatch).
                                        "compare" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let a = iter.next().unwrap();
                                                let b = iter.next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(a),
                                                        property: "compare".to_string(),
                                                    }),
                                                    args: vec![b],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Uint8Array static methods
                            if obj_name == "Uint8Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::Uint8ArrayFrom(Box::new(data)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Object static methods
                            if obj_name == "Object" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "keys" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectKeys(Box::new(obj)));
                                        }
                                        "values" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectValues(Box::new(obj)));
                                        }
                                        "entries" => {
                                            let obj = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectEntries(Box::new(obj)));
                                        }
                                        // Object.assign(target, src1, src2, ...) - treat as object spread
                                        // Each non-object arg is spread; object literal args are inlined
                                        "assign" => {
                                            let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                                            for arg in &args {
                                                match arg {
                                                    Expr::Object(props) => {
                                                        // Inline object literal props as static key-value pairs
                                                        for (key, val) in props {
                                                            parts.push((Some(key.clone()), val.clone()));
                                                        }
                                                    }
                                                    _ => {
                                                        // Spread non-object expression
                                                        parts.push((None, arg.clone()));
                                                    }
                                                }
                                            }
                                            // If no spreads and only static props, return plain Object
                                            let has_spread = parts.iter().any(|(k, _)| k.is_none());
                                            if !has_spread {
                                                let static_props: Vec<(String, Expr)> = parts.into_iter()
                                                    .filter_map(|(k, v)| k.map(|key| (key, v)))
                                                    .collect();
                                                return Ok(Expr::Object(static_props));
                                            }
                                            return Ok(Expr::ObjectSpread { parts });
                                        }
                                        "fromEntries" => {
                                            let entries = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectFromEntries(Box::new(entries)));
                                        }
                                        "groupBy" => {
                                            // Object.groupBy(items, keyFn) — Node 22+ static method
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let items = iter.next().unwrap();
                                                let key_fn = iter.next().unwrap();
                                                let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                                                return Ok(Expr::ObjectGroupBy {
                                                    items: Box::new(items),
                                                    key_fn: Box::new(key_fn),
                                                });
                                            }
                                        }
                                        "is" => {
                                            let mut iter = args.into_iter();
                                            let a = iter.next().unwrap_or(Expr::Undefined);
                                            let b = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectIs(Box::new(a), Box::new(b)));
                                        }
                                        "hasOwn" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key)));
                                        }
                                        "freeze" => { return Ok(Expr::ObjectFreeze(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "seal" => { return Ok(Expr::ObjectSeal(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "preventExtensions" => { return Ok(Expr::ObjectPreventExtensions(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "create" => { return Ok(Expr::ObjectCreate(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isFrozen" => { return Ok(Expr::ObjectIsFrozen(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isSealed" => { return Ok(Expr::ObjectIsSealed(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "isExtensible" => { return Ok(Expr::ObjectIsExtensible(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "getPrototypeOf" => { return Ok(Expr::ObjectGetPrototypeOf(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined)))); }
                                        "defineProperty" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            let descriptor = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectDefineProperty(Box::new(obj), Box::new(key), Box::new(descriptor)));
                                        }
                                        "getOwnPropertyDescriptor" => {
                                            let mut iter = args.into_iter();
                                            let obj = iter.next().unwrap_or(Expr::Undefined);
                                            let key = iter.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectGetOwnPropertyDescriptor(Box::new(obj), Box::new(key)));
                                        }
                                        "getOwnPropertyNames" => {
                                            return Ok(Expr::ObjectGetOwnPropertyNames(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined))));
                                        }
                                        "getOwnPropertySymbols" => {
                                            return Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(args.into_iter().next().unwrap_or(Expr::Undefined))));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Symbol static methods: Symbol.for / Symbol.keyFor
                            if obj_name == "Symbol" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "for" => {
                                            let key = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::SymbolFor(Box::new(key)));
                                        }
                                        "keyFor" => {
                                            let sym = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::SymbolKeyFor(Box::new(sym)));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            if obj_name == "Reflect" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "get" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectGet { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "set" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            let value = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectSet { target: Box::new(target), key: Box::new(key), value: Box::new(value) });
                                        }
                                        "has" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectHas { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "deleteProperty" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectDelete { target: Box::new(target), key: Box::new(key) });
                                        }
                                        "ownKeys" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectOwnKeys(Box::new(target)));
                                        }
                                        "apply" => {
                                            let mut it = args.into_iter();
                                            let func = it.next().unwrap_or(Expr::Undefined);
                                            let this_arg = it.next().unwrap_or(Expr::Undefined);
                                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::ReflectApply { func: Box::new(func), this_arg: Box::new(this_arg), args: Box::new(args_arr) });
                                        }
                                        "construct" => {
                                            // Special case: `Reflect.construct(ClassName, [args...])`
                                            // where ClassName is a known class — fold to a direct
                                            // `new ClassName(...args)` expression.
                                            if call.args.len() >= 2 {
                                                if let ast::Expr::Ident(cls_ident) = call.args[0].expr.as_ref() {
                                                    let cls_name = cls_ident.sym.to_string();
                                                    if ctx.lookup_class(&cls_name).is_some() {
                                                        if let ast::Expr::Array(arr_lit) = call.args[1].expr.as_ref() {
                                                            let new_args: Vec<Expr> = arr_lit.elems.iter()
                                                                .filter_map(|e| e.as_ref())
                                                                .map(|e| lower_expr(ctx, &e.expr))
                                                                .collect::<Result<Vec<_>>>()?;
                                                            return Ok(Expr::New {
                                                                class_name: cls_name,
                                                                args: new_args,
                                                                type_args: vec![],
                                                            });
                                                        }
                                                    }
                                                }
                                            }
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                                            return Ok(Expr::ReflectConstruct { target: Box::new(target), args: Box::new(args_arr) });
                                        }
                                        "defineProperty" => {
                                            let mut it = args.into_iter();
                                            let target = it.next().unwrap_or(Expr::Undefined);
                                            let key = it.next().unwrap_or(Expr::Undefined);
                                            let descriptor = it.next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectDefineProperty { target: Box::new(target), key: Box::new(key), descriptor: Box::new(descriptor) });
                                        }
                                        "getPrototypeOf" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ReflectGetPrototypeOf(Box::new(target)));
                                        }
                                        "setPrototypeOf" => return Ok(Expr::Bool(true)),
                                        "isExtensible" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectIsExtensible(Box::new(target)));
                                        }
                                        "preventExtensions" => {
                                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ObjectPreventExtensions(Box::new(target)));
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if obj_name == "Proxy" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    if method_ident.sym.as_ref() == "revocable" {
                                        let mut it = args.into_iter();
                                        let target = it.next().unwrap_or(Expr::Undefined);
                                        let handler = it.next().unwrap_or(Expr::Object(vec![]));
                                        return Ok(Expr::ProxyRevocable { target: Box::new(target), handler: Box::new(handler) });
                                    }
                                }
                            }

                            // Check for Array static methods
                            if obj_name == "Array" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "isArray" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            return Ok(Expr::ArrayIsArray(Box::new(value)));
                                        }
                                        "from" => {
                                            let value = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            // `Array.from(iterable, mapFn)` uses a dedicated HIR
                                            // variant so codegen can handle Map/Set/Array sources
                                            // uniformly (materialize + js_array_map).
                                            if let Some(map_fn) = args.get(1).cloned() {
                                                return Ok(Expr::ArrayFromMapped {
                                                    iterable: Box::new(value),
                                                    map_fn: Box::new(map_fn),
                                                });
                                            }
                                            // Check if the source is a generator call — use iterator protocol
                                            let is_gen = is_generator_call_expr(ctx, &value);
                                            if is_gen {
                                                return Ok(Expr::IteratorToArray(Box::new(value)));
                                            }
                                            return Ok(Expr::ArrayFrom(Box::new(value)));
                                        }
                                        "of" => {
                                            // Array.of(1,2,3) is equivalent to [1,2,3]
                                            return Ok(Expr::Array(args));
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net module methods
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let options = args.get(0).cloned().map(Box::new);
                                            let connection_listener = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::NetCreateServer { options, connection_listener });
                                        }
                                        // createConnection/connect fall through to generic NativeMethodCall
                                        // so they dispatch via NATIVE_MODULE_TABLE to the new
                                        // event-driven `js_net_socket_connect` in perry-stdlib (A1/A1.5).
                                        // The dedicated `Expr::NetCreateConnection` variant was never
                                        // lowered by the LLVM backend and remained as vestigial HIR;
                                        // the generic path gives us working codegen for free.
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            if let Some((module_name, _imported_method)) = ctx.lookup_native_module(&obj_name) {
                                // Skip modules handled specifically below (path, fs, child_process, etc.)
                                // `net` used to be in this list back when its method calls
                                // were short-circuited into `Expr::NetCreateConnection` etc.
                                // After A1.5 `net` goes through the generic NativeMethodCall
                                // path so the LLVM backend's NATIVE_MODULE_TABLE dispatches
                                // to `js_net_socket_*` in perry-stdlib.
                                let is_handled_module = module_name == "path" || module_name == "node:path"
                                    || module_name == "fs" || module_name == "node:fs"
                                    || module_name == "child_process" || module_name == "node:child_process"
                                    || module_name == "crypto" || module_name == "node:crypto"
                                    || module_name == "os" || module_name == "node:os";
                                if !is_handled_module {
                                    // This is a call on a native module (e.g., mysql.createConnection)
                                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                        let method_name = method_ident.sym.to_string();
                                        return Ok(Expr::NativeMethodCall {
                                            module: module_name.to_string(),
                                            class_name: None,  // Will be set by js_transform if needed
                                            object: None,  // Static call on module itself
                                            method: method_name,
                                            args,
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // Check for static method calls (e.g., Counter.increment())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            // Treat uppercase imported identifiers as candidate classes —
                            // we don't have cross-module class metadata at HIR-lower
                            // time, so without this `import { MongoClient } from
                            // 'pkg'; MongoClient.connect(...)` falls through to the
                            // dynamic-dispatch path and reads garbage from the static
                            // ClosureHeader.  See compile.rs::imported_classes for the
                            // backing dispatch table that resolves these calls at
                            // codegen time.
                            let is_imported_upper = ctx.lookup_imported_func(&obj_name).is_some()
                                && obj_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false);
                            if ctx.lookup_class(&obj_name).is_some() || is_imported_upper {
                                match &member.prop {
                                    ast::MemberProp::Ident(method_ident) => {
                                        let method_name = method_ident.sym.to_string();
                                        if ctx.has_static_method(&obj_name, &method_name) || is_imported_upper {
                                            return Ok(Expr::StaticMethodCall {
                                                class_name: obj_name,
                                                method_name,
                                                args,
                                            });
                                        }
                                    }
                                    // Private static method: WithPrivateStatic.#helper()
                                    ast::MemberProp::PrivateName(priv_ident) => {
                                        let method_name = format!("#{}", priv_ident.name.to_string());
                                        if ctx.has_static_method(&obj_name, &method_name) {
                                            return Ok(Expr::StaticMethodCall {
                                                class_name: obj_name,
                                                method_name,
                                                args,
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    // Check for native instance method calls (e.g., emitter.on(), ws.send())
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            let obj_name = obj_ident.sym.to_string();
                            // Clone module_name and class_name to avoid borrow issues
                            let native_instance = ctx.lookup_native_instance(&obj_name)
                                .map(|(m, c)| (m.to_string(), c.to_string()));
                            if obj_name == "pool" {
                            }
                            if let Some((module_name, class_name)) = native_instance {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.to_string();
                                    // Get the object expression (the instance variable)
                                    let object_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::NativeMethodCall {
                                        module: module_name,
                                        class_name: Some(class_name),  // Use the registered class name
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }

                        // issue #195: WidgetCtor(...).modifierName(...) is silently dropped.
                        // Reject at compile time so users discover the options-object form.
                        if let ast::Expr::Call(inner_call) = member.obj.as_ref() {
                            if let ast::Callee::Expr(inner_callee) = &inner_call.callee {
                                if let ast::Expr::Ident(widget_ident) = inner_callee.as_ref() {
                                    let widget_name = widget_ident.sym.as_ref();
                                    if matches!(widget_name,
                                        "Text" | "VStack" | "HStack" | "ZStack" |
                                        "Image" | "Spacer" | "Divider" |
                                        "ForEach" | "Label" | "Gauge"
                                    ) {
                                        if matches!(ctx.lookup_native_module(widget_name),
                                            Some(("perry/ui", _))
                                        ) {
                                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                                let modifier_name = method_ident.sym.as_ref();
                                                if is_widget_modifier_name(modifier_name) {
                                                    return Err(anyhow!(
                                                        "modifier '{}' must be passed as an option-object on the widget constructor; use: {}(\"...\", {{ {}: ... }})",
                                                        modifier_name, widget_name, modifier_name
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Check for method calls on new Big/Decimal/BigNumber() expressions
                        // e.g., new Big("100").div(2)
                        if let Some(module_name) = detect_native_instance_expr(&member.obj) {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.to_string();
                                let object_expr = lower_expr(ctx, &member.obj)?;
                                return Ok(Expr::NativeMethodCall {
                                    module: module_name.to_string(),
                                    class_name: None,  // Will be set by js_transform if needed
                                    object: Some(Box::new(object_expr)),
                                    method: method_name,
                                    args,
                                });
                            }
                        }

                        // Check for chained method calls on registered native instances
                        // e.g., r1.times(...).times(...) where r1 is a Big
                        // The inner call might lower to a NativeMethodCall, and we need to chain properly
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.to_string();
                            // Lower the object expression first
                            let object_expr = lower_expr(ctx, &member.obj)?;
                            // Check if it's a NativeMethodCall for a fluent-API native module
                            if let Expr::NativeMethodCall { module, class_name, method: prior_method, .. } = &object_expr {
                                // Methods that return the same type (builder pattern)
                                let is_math_lib = matches!(module.as_str(), "big.js" | "decimal.js" | "bignumber.js");
                                let is_math_method = matches!(method_name.as_str(),
                                    // arithmetic + chainable rounding/formatting
                                    "plus" | "minus" | "times" | "div" | "mod" |
                                    "pow" | "sqrt" | "abs" | "neg" | "round" | "floor" | "ceil" | "toFixed" |
                                    // decimal.js: terminal-shape methods that still need
                                    // NativeMethodCall dispatch (so a.plus(b).eq(c) etc.
                                    // doesn't fall back to the generic Call+PropertyGet path).
                                    "toString" | "toNumber" | "valueOf" |
                                    "eq" | "lt" | "lte" | "gt" | "gte" | "cmp" |
                                    "isZero" | "isPositive" | "isNegative"
                                );
                                // commander Command — every fluent method either
                                // returns the same handle (name/version/description/
                                // option/requiredOption/action) or a sub-Command with
                                // the same module + class (.command(name)). Either way
                                // the next chained call must dispatch through the
                                // commander NativeModSig table, not the generic
                                // dynamic-property fallback. Without this branch
                                // `program.name(...).version(...)` only the first
                                // call landed as a NativeMethodCall and the rest
                                // silently no-op'd at codegen — issue #187.
                                let is_commander = module.as_str() == "commander";
                                let is_commander_method = matches!(method_name.as_str(),
                                    "name" | "version" | "description" |
                                    "option" | "requiredOption" |
                                    "action" | "command" | "parse" | "opts"
                                );
                                if (is_math_lib && is_math_method)
                                    || (is_commander && is_commander_method)
                                {
                                    return Ok(Expr::NativeMethodCall {
                                        module: module.clone(),
                                        class_name: class_name.clone(),
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                                // Database-driver chaining: methods like
                                // `db.prepare(sql).run()` / `db.prepare(sql).get()` /
                                // `db.prepare(sql).all()` where the inner call returns
                                // a *new* native class (Statement) — not the same
                                // handle as the receiver. Look up `(module,
                                // prior_method)` in the chaining table and dispatch
                                // the outer call against the resulting class. Without
                                // this, the outer `.run()`/`.get()`/`.all()` fell
                                // through to the generic js_native_call_method
                                // dispatcher: SQL never executed, returned objects
                                // had no keys_array, `Object.keys(row)` was `[]` and
                                // `row.id` was undefined.
                                let chained_class: Option<&'static str> = match (module.as_str(), prior_method.as_str()) {
                                    ("better-sqlite3", "prepare") => Some("Statement"),
                                    ("mongodb", "db") => Some("Database"),
                                    ("mongodb", "collection") => Some("Collection"),
                                    ("mysql2", "getConnection") | ("mysql2/promise", "getConnection") => Some("PoolConnection"),
                                    ("pg", "connect") => Some("PoolClient"),
                                    ("ioredis", "duplicate") => Some("Redis"),
                                    _ => None,
                                };
                                if let Some(result_class) = chained_class {
                                    return Ok(Expr::NativeMethodCall {
                                        module: module.clone(),
                                        class_name: Some(result_class.to_string()),
                                        object: Some(Box::new(object_expr)),
                                        method: method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    // Check for fs.methodName() calls (including require('fs') aliases)
                    if let ast::Expr::Member(member) = expr.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            // Check if this is 'fs' directly or an alias from require('fs')
                            let obj_name = obj_ident.sym.as_ref();
                            let is_fs_module = obj_name == "fs" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("fs");
                            if is_fs_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "readFileSync" => {
                                            if args.len() >= 2 {
                                                // readFileSync(path, encoding) — returns string
                                                return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() == 1 {
                                                // readFileSync(path) without encoding — returns Buffer (Node parity)
                                                return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "writeFileSync" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path = iter.next().unwrap();
                                                let content = iter.next().unwrap();
                                                return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                            }
                                        }
                                        "appendFileSync" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path = iter.next().unwrap();
                                                let content = iter.next().unwrap();
                                                return Ok(Expr::FsAppendFileSync(Box::new(path), Box::new(content)));
                                            }
                                        }
                                        "existsSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "mkdirSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "unlinkSync" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "readFileBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "rmRecursive" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for path.methodName() calls (including require('path') aliases)
                            let is_path_module = obj_name == "path" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("path");
                            if is_path_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "join" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let mut result = iter.next().unwrap();
                                                for next_arg in iter {
                                                    result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                                }
                                                return Ok(result);
                                            }
                                        }
                                        "dirname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "basename" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let path_arg = iter.next().unwrap();
                                                let ext_arg = iter.next().unwrap();
                                                return Ok(Expr::PathBasenameExt(Box::new(path_arg), Box::new(ext_arg)));
                                            }
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "extname" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "resolve" => {
                                            if args.len() >= 1 {
                                                // path.resolve(a, b, c) => resolve(join(a, b, c))
                                                // For single arg, just resolve directly
                                                let mut iter = args.into_iter();
                                                let first = iter.next().unwrap();
                                                let mut joined = first;
                                                for next_arg in iter {
                                                    joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                                }
                                                return Ok(Expr::PathResolve(Box::new(joined)));
                                            }
                                        }
                                        "isAbsolute" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "relative" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let from = iter.next().unwrap();
                                                let to = iter.next().unwrap();
                                                return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
                                            }
                                        }
                                        "normalize" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathNormalize(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parse" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathParse(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "format" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::PathFormat(Box::new(args.into_iter().next().unwrap())));
                                            }
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
                                                return Ok(Expr::JsonParseWithReviver(Box::new(text), Box::new(reviver)));
                                            } else if args.len() >= 1 {
                                                let text = args.into_iter().next().unwrap();
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
                                                                extract_typed_parse_source_order(
                                                                    ts_type, ctx,
                                                                );
                                                            return Ok(Expr::JsonParseTyped {
                                                                text: Box::new(text),
                                                                ty: resolved,
                                                                ordered_keys,
                                                            });
                                                        }
                                                    }
                                                }
                                                return Ok(Expr::JsonParse(Box::new(text)));
                                            }
                                        }
                                        "stringify" => {
                                            if args.len() >= 2 {
                                                let mut it = args.into_iter();
                                                let value = it.next().unwrap();
                                                let replacer = it.next().unwrap();
                                                let spacer = it.next().unwrap_or(Expr::Null);
                                                return Ok(Expr::JsonStringifyFull(
                                                    Box::new(value),
                                                    Box::new(replacer),
                                                    Box::new(spacer),
                                                ));
                                            } else if args.len() == 1 {
                                                let value = args.into_iter().next().unwrap();
                                                // Route ALL single-arg stringify through JsonStringifyFull
                                                // so the runtime can return TAG_UNDEFINED for undefined input
                                                return Ok(Expr::JsonStringifyFull(
                                                    Box::new(value),
                                                    Box::new(Expr::Null),
                                                    Box::new(Expr::Null),
                                                ));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for performance.now()
                            if obj_ident.sym.as_ref() == "performance" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    if method_ident.sym.as_ref() == "now" {
                                        return Ok(Expr::PerformanceNow);
                                    }
                                }
                            }

                            // Check for Response.json(value) / Response.redirect(url, status?) static factories
                            if obj_ident.sym.as_ref() == "Response" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "json" | "redirect" => {
                                            ctx.uses_fetch = true;
                                            return Ok(Expr::NativeMethodCall {
                                                module: "fetch".to_string(),
                                                class_name: Some("Response".to_string()),
                                                object: None,
                                                method: format!("static_{}", method_name),
                                                args,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            // Check for Math.methodName() calls
                            if obj_ident.sym.as_ref() == "Math" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "floor" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathFloor(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "ceil" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathCeil(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "round" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathRound(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "trunc" => {
                                            // Math.trunc(x) = x >= 0 ? floor(x) : ceil(x)
                                            if args.len() >= 1 {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Conditional {
                                                    condition: Box::new(Expr::Compare {
                                                        op: crate::CompareOp::Ge,
                                                        left: Box::new(arg.clone()),
                                                        right: Box::new(Expr::Number(0.0)),
                                                    }),
                                                    then_expr: Box::new(Expr::MathFloor(Box::new(arg.clone()))),
                                                    else_expr: Box::new(Expr::MathCeil(Box::new(arg))),
                                                });
                                            }
                                        }
                                        "sign" => {
                                            // Math.sign(x) = x > 0 ? 1 : x < 0 ? -1 : 0 (or x for NaN)
                                            if args.len() >= 1 {
                                                let arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Conditional {
                                                    condition: Box::new(Expr::Compare {
                                                        op: crate::CompareOp::Gt,
                                                        left: Box::new(arg.clone()),
                                                        right: Box::new(Expr::Number(0.0)),
                                                    }),
                                                    then_expr: Box::new(Expr::Number(1.0)),
                                                    else_expr: Box::new(Expr::Conditional {
                                                        condition: Box::new(Expr::Compare {
                                                            op: crate::CompareOp::Lt,
                                                            left: Box::new(arg.clone()),
                                                            right: Box::new(Expr::Number(0.0)),
                                                        }),
                                                        then_expr: Box::new(Expr::Number(-1.0)),
                                                        else_expr: Box::new(arg),
                                                    }),
                                                });
                                            }
                                        }
                                        "abs" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAbs(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "sqrt" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathSqrt(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log2" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog2(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "log10" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathLog10(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "pow" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let base = args_iter.next().unwrap();
                                                let exp = args_iter.next().unwrap();
                                                return Ok(Expr::MathPow(Box::new(base), Box::new(exp)));
                                            }
                                        }
                                        "min" => {
                                            if has_spread && args.len() == 1 {
                                                return Ok(Expr::MathMinSpread(Box::new(args.into_iter().next().unwrap())));
                                            }
                                            return Ok(Expr::MathMin(args));
                                        }
                                        "max" => {
                                            if has_spread && args.len() == 1 {
                                                return Ok(Expr::MathMaxSpread(Box::new(args.into_iter().next().unwrap())));
                                            }
                                            return Ok(Expr::MathMax(args));
                                        }
                                        "random" => {
                                            return Ok(Expr::MathRandom);
                                        }
                                        "imul" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let a = args_iter.next().unwrap();
                                                let b = args_iter.next().unwrap();
                                                return Ok(Expr::MathImul(Box::new(a), Box::new(b)));
                                            }
                                        }
                                        "sin" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathSin(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "cos" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathCos(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "tan" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathTan(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "asin" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAsin(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "acos" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAcos(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "atan" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::MathAtan(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "atan2" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let y = args_iter.next().unwrap();
                                                let x = args_iter.next().unwrap();
                                                return Ok(Expr::MathAtan2(Box::new(y), Box::new(x)));
                                            }
                                        }
                                        "cbrt" => { if args.len() >= 1 { return Ok(Expr::MathCbrt(Box::new(args.into_iter().next().unwrap()))); } }
                                        "hypot" => { return Ok(Expr::MathHypot(args)); }
                                        "fround" => { if args.len() >= 1 { return Ok(Expr::MathFround(Box::new(args.into_iter().next().unwrap()))); } }
                                        "clz32" => { if args.len() >= 1 { return Ok(Expr::MathClz32(Box::new(args.into_iter().next().unwrap()))); } }
                                        "expm1" => { if args.len() >= 1 { return Ok(Expr::MathExpm1(Box::new(args.into_iter().next().unwrap()))); } }
                                        "log1p" => { if args.len() >= 1 { return Ok(Expr::MathLog1p(Box::new(args.into_iter().next().unwrap()))); } }
                                        "sinh" => { if args.len() >= 1 { return Ok(Expr::MathSinh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "cosh" => { if args.len() >= 1 { return Ok(Expr::MathCosh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "tanh" => { if args.len() >= 1 { return Ok(Expr::MathTanh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "asinh" => { if args.len() >= 1 { return Ok(Expr::MathAsinh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "acosh" => { if args.len() >= 1 { return Ok(Expr::MathAcosh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "atanh" => { if args.len() >= 1 { return Ok(Expr::MathAtanh(Box::new(args.into_iter().next().unwrap()))); } }
                                        "exp" => { if args.len() >= 1 { return Ok(Expr::MathExp(Box::new(args.into_iter().next().unwrap()))); } }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Number.methodName() static calls
                            if obj_ident.sym.as_ref() == "Number" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "isNaN" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsNaN(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isFinite" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsFinite(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isInteger" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsInteger(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isSafeInteger" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::NumberIsSafeInteger(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parseFloat" => {
                                            // Number.parseFloat is the same as global parseFloat
                                            if args.len() >= 1 {
                                                return Ok(Expr::ParseFloat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "parseInt" => {
                                            // Number.parseInt is the same as global parseInt
                                            let mut iter = args.into_iter();
                                            let string_arg = if let Some(s) = iter.next() { Box::new(s) } else { return Err(anyhow!("Number.parseInt requires at least one argument")); };
                                            let radix_arg = iter.next().map(Box::new);
                                            return Ok(Expr::ParseInt { string: string_arg, radix: radix_arg });
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
                                            if args.len() == 1 {
                                                return Ok(Expr::StringFromCharCode(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() > 1 {
                                                // Multi-arg: concat each char as a separate fromCharCode call
                                                let mut iter = args.into_iter();
                                                let mut acc = Expr::StringFromCharCode(Box::new(iter.next().unwrap()));
                                                for arg in iter {
                                                    acc = Expr::Binary {
                                                        op: crate::ir::BinaryOp::Add,
                                                        left: Box::new(acc),
                                                        right: Box::new(Expr::StringFromCharCode(Box::new(arg))),
                                                    };
                                                }
                                                return Ok(acc);
                                            }
                                        }
                                        "fromCodePoint" => {
                                            if args.len() == 1 {
                                                return Ok(Expr::StringFromCodePoint(Box::new(args.into_iter().next().unwrap())));
                                            } else if args.len() > 1 {
                                                let mut iter = args.into_iter();
                                                let mut acc = Expr::StringFromCodePoint(Box::new(iter.next().unwrap()));
                                                for arg in iter {
                                                    acc = Expr::Binary {
                                                        op: crate::ir::BinaryOp::Add,
                                                        left: Box::new(acc),
                                                        right: Box::new(Expr::StringFromCodePoint(Box::new(arg))),
                                                    };
                                                }
                                                return Ok(acc);
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for crypto.methodName() calls (including require('crypto') aliases)
                            let is_crypto_module = obj_name == "crypto" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("crypto");
                            if is_crypto_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "randomBytes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoRandomBytes(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "randomUUID" => {
                                            return Ok(Expr::CryptoRandomUUID);
                                        }
                                        "sha256" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoSha256(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "md5" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::CryptoMd5(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        // `crypto.getRandomValues(buf)` fills the buffer
                                        // in-place with random bytes and returns it.
                                        // Lower as a synthetic instance method call so
                                        // the runtime buffer dispatcher (added in
                                        // perry-runtime/src/object.rs) handles it via
                                        // `js_buffer_fill_random`.
                                        "getRandomValues" => {
                                            if args.len() >= 1 {
                                                let buf_arg = args.into_iter().next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(buf_arg),
                                                        property: "$$cryptoFillRandom".to_string(),
                                                    }),
                                                    args: vec![],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for os.methodName() calls (including require('os') aliases)
                            let is_os_module = obj_name == "os" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("os");
                            if is_os_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "platform" => {
                                            return Ok(Expr::OsPlatform);
                                        }
                                        "arch" => {
                                            return Ok(Expr::OsArch);
                                        }
                                        "hostname" => {
                                            return Ok(Expr::OsHostname);
                                        }
                                        "homedir" => {
                                            return Ok(Expr::OsHomedir);
                                        }
                                        "tmpdir" => {
                                            return Ok(Expr::OsTmpdir);
                                        }
                                        "totalmem" => {
                                            return Ok(Expr::OsTotalmem);
                                        }
                                        "freemem" => {
                                            return Ok(Expr::OsFreemem);
                                        }
                                        "uptime" => {
                                            return Ok(Expr::OsUptime);
                                        }
                                        "type" => {
                                            return Ok(Expr::OsType);
                                        }
                                        "release" => {
                                            return Ok(Expr::OsRelease);
                                        }
                                        "cpus" => {
                                            return Ok(Expr::OsCpus);
                                        }
                                        "networkInterfaces" => {
                                            return Ok(Expr::OsNetworkInterfaces);
                                        }
                                        "userInfo" => {
                                            return Ok(Expr::OsUserInfo);
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for Buffer.methodName() static calls
                            if obj_name == "Buffer" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "from" => {
                                            let data = args.get(0).cloned().unwrap_or(Expr::Undefined);
                                            let encoding = args.get(1).cloned().map(Box::new);
                                            return Ok(Expr::BufferFrom {
                                                data: Box::new(data),
                                                encoding,
                                            });
                                        }
                                        "alloc" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let size = args_iter.next().unwrap();
                                                let fill = args_iter.next().map(Box::new);
                                                return Ok(Expr::BufferAlloc {
                                                    size: Box::new(size),
                                                    fill,
                                                });
                                            }
                                        }
                                        "allocUnsafe" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferAllocUnsafe(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "concat" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferConcat(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "isBuffer" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferIsBuffer(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        "byteLength" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::BufferByteLength(Box::new(args.into_iter().next().unwrap())));
                                            }
                                        }
                                        // `Buffer.compare(a, b)` returns -1/0/1. The runtime
                                        // dispatch already handles `a.compare(b)` as an
                                        // instance method routing through `js_buffer_compare`.
                                        // Synthesize that form so we don't need a dedicated
                                        // HIR variant or runtime entry point.
                                        "compare" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let a = iter.next().unwrap();
                                                let b = iter.next().unwrap();
                                                return Ok(Expr::Call {
                                                    callee: Box::new(Expr::PropertyGet {
                                                        object: Box::new(a),
                                                        property: "compare".to_string(),
                                                    }),
                                                    args: vec![b],
                                                    type_args: vec![],
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for child_process named imports (execSync, spawnSync, spawn, exec)
                            let is_child_process_module = ctx.lookup_builtin_module_alias(obj_name) == Some("child_process");
                            if is_child_process_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "execSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExecSync {
                                                    command: Box::new(command),
                                                    options,
                                                });
                                            }
                                        }
                                        "spawnSync" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnSync {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "spawn" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let options = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawn {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    options,
                                                });
                                            }
                                        }
                                        "exec" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let options = args_iter.next().map(Box::new);
                                                let callback = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessExec {
                                                    command: Box::new(command),
                                                    options,
                                                    callback,
                                                });
                                            }
                                        }
                                        "spawnBackground" => {
                                            if args.len() >= 3 {
                                                let mut args_iter = args.into_iter();
                                                let command = args_iter.next().unwrap();
                                                let spawn_args = args_iter.next().map(Box::new);
                                                let log_file = args_iter.next().unwrap();
                                                let env_json = args_iter.next().map(Box::new);
                                                return Ok(Expr::ChildProcessSpawnBackground {
                                                    command: Box::new(command),
                                                    args: spawn_args,
                                                    log_file: Box::new(log_file),
                                                    env_json,
                                                });
                                            }
                                        }
                                        "getProcessStatus" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessGetProcessStatus(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        "killProcess" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ChildProcessKillProcess(
                                                    Box::new(args.into_iter().next().unwrap())
                                                ));
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for net.methodName() calls
                            let is_net_module = obj_name == "net" ||
                                ctx.lookup_builtin_module_alias(obj_name) == Some("net");
                            if is_net_module {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    match method_name {
                                        "createServer" => {
                                            let mut args_iter = args.into_iter();
                                            let options = args_iter.next().map(Box::new);
                                            let connection_listener = args_iter.next().map(Box::new);
                                            return Ok(Expr::NetCreateServer {
                                                options,
                                                connection_listener,
                                            });
                                        }
                                        // createConnection/connect: see sibling site above —
                                        // falls through to generic NativeMethodCall so the LLVM
                                        // backend's NATIVE_MODULE_TABLE dispatch can handle it.
                                        _ => {} // Fall through to generic handling
                                    }
                                }
                            }

                            // Check for AbortSignal.timeout(ms) static method call
                            if obj_ident.sym.as_ref() == "AbortSignal" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "timeout" {
                                        return Ok(Expr::StaticMethodCall {
                                            class_name: "AbortSignal".to_string(),
                                            method_name: "timeout".to_string(),
                                            args,
                                        });
                                    }
                                }
                            }

                            // Check for Date.now() / Date.parse() / Date.UTC() static method calls
                            if obj_ident.sym.as_ref() == "Date" {
                                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                    let method_name = method_ident.sym.as_ref();
                                    if method_name == "now" {
                                        return Ok(Expr::DateNow);
                                    }
                                    if method_name == "parse" {
                                        if args.len() >= 1 {
                                            return Ok(Expr::DateParse(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    if method_name == "UTC" {
                                        return Ok(Expr::DateUtc(args));
                                    }
                                }
                            }
                        }

                        // Check for Date instance method calls (date.getTime(), etc.)
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            match method_name {
                                "getTime" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetTime(Box::new(date_expr)));
                                }
                                "toISOString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToISOString(Box::new(date_expr)));
                                }
                                "getFullYear" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetFullYear(Box::new(date_expr)));
                                }
                                "getMonth" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMonth(Box::new(date_expr)));
                                }
                                "getDate" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetDate(Box::new(date_expr)));
                                }
                                "getHours" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetHours(Box::new(date_expr)));
                                }
                                "getMinutes" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMinutes(Box::new(date_expr)));
                                }
                                "getSeconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetSeconds(Box::new(date_expr)));
                                }
                                "getMilliseconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetMilliseconds(Box::new(date_expr)));
                                }
                                // UTC getters
                                "getUTCDay" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcDay(Box::new(date_expr)));
                                }
                                "getUTCFullYear" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcFullYear(Box::new(date_expr)));
                                }
                                "getUTCMonth" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMonth(Box::new(date_expr)));
                                }
                                "getUTCDate" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcDate(Box::new(date_expr)));
                                }
                                "getUTCHours" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcHours(Box::new(date_expr)));
                                }
                                "getUTCMinutes" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMinutes(Box::new(date_expr)));
                                }
                                "getUTCSeconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcSeconds(Box::new(date_expr)));
                                }
                                "getUTCMilliseconds" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetUtcMilliseconds(Box::new(date_expr)));
                                }
                                // Other getters/methods
                                "valueOf" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateValueOf(Box::new(date_expr)));
                                }
                                "toDateString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToDateString(Box::new(date_expr)));
                                }
                                "toTimeString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToTimeString(Box::new(date_expr)));
                                }
                                "toLocaleDateString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleDateString(Box::new(date_expr)));
                                }
                                "toLocaleTimeString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleTimeString(Box::new(date_expr)));
                                }
                                "toLocaleString" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToLocaleString(Box::new(date_expr)));
                                }
                                "getTimezoneOffset" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateGetTimezoneOffset(Box::new(date_expr)));
                                }
                                "toJSON" => {
                                    let date_expr = lower_expr(ctx, &member.obj)?;
                                    return Ok(Expr::DateToJSON(Box::new(date_expr)));
                                }
                                // UTC setters — mutate the local variable in place
                                "setUTCFullYear" | "setUTCMonth" | "setUTCDate" |
                                "setUTCHours" | "setUTCMinutes" | "setUTCSeconds" |
                                "setUTCMilliseconds" => {
                                    if args.len() >= 1 {
                                        let value_expr = args.into_iter().next().unwrap();
                                        let date_expr = lower_expr(ctx, &member.obj)?;
                                        let setter_call = match method_name {
                                            "setUTCFullYear" => Expr::DateSetUtcFullYear { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMonth" => Expr::DateSetUtcMonth { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCDate" => Expr::DateSetUtcDate { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCHours" => Expr::DateSetUtcHours { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMinutes" => Expr::DateSetUtcMinutes { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCSeconds" => Expr::DateSetUtcSeconds { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            "setUTCMilliseconds" => Expr::DateSetUtcMilliseconds { date: Box::new(date_expr.clone()), value: Box::new(value_expr) },
                                            _ => unreachable!(),
                                        };
                                        // If receiver is a local variable, mutate it in place by wrapping
                                        // the setter result in a LocalSet so the new timestamp is stored back.
                                        if let Expr::LocalGet(local_id) = &date_expr {
                                            return Ok(Expr::LocalSet(*local_id, Box::new(setter_call)));
                                        }
                                        return Ok(setter_call);
                                    }
                                }
                                _ => {} // Fall through to other handling
                            }
                        }

                        // Check for WeakRef.deref() / FinalizationRegistry.register() / .unregister()
                        // dispatch BEFORE the generic array method dispatch — these receivers were
                        // tracked in the pre-scan pass.
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if let ast::Expr::Ident(recv_ident) = member.obj.as_ref() {
                                let recv_name = recv_ident.sym.to_string();
                                if ctx.weakref_locals.contains(&recv_name) && method_name == "deref" {
                                    return Ok(Expr::WeakRefDeref(Box::new(Expr::LocalGet(
                                        ctx.lookup_local(&recv_name).unwrap_or(0),
                                    ))));
                                }
                                if ctx.finreg_locals.contains(&recv_name) {
                                    let registry_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    match method_name {
                                        "register" => {
                                            if args.len() >= 2 {
                                                let mut iter = args.into_iter();
                                                let target = iter.next().unwrap();
                                                let held = iter.next().unwrap();
                                                let token = iter.next().map(Box::new);
                                                return Ok(Expr::FinalizationRegistryRegister {
                                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                                    target: Box::new(target),
                                                    held: Box::new(held),
                                                    token,
                                                });
                                            }
                                        }
                                        "unregister" => {
                                            if args.len() >= 1 {
                                                let token = args.into_iter().next().unwrap();
                                                return Ok(Expr::FinalizationRegistryUnregister {
                                                    registry: Box::new(Expr::LocalGet(registry_id)),
                                                    token: Box::new(token),
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                // WeakMap/WeakSet — route to dedicated runtime functions
                                // (NOT the regular Map/Set HIR variants) so reference-equality
                                // works for object keys. Primitive keys/values throw via
                                // js_weak_throw_primitive when the AST shows a bare literal.
                                let make_extern_call = |name: &str, args: Vec<Expr>| -> Expr {
                                    Expr::Call {
                                        callee: Box::new(Expr::ExternFuncRef {
                                            name: name.to_string(),
                                            param_types: Vec::new(),
                                            return_type: Type::Any,
                                        }),
                                        args,
                                        type_args: Vec::new(),
                                    }
                                };
                                let throw_primitive_expr = || -> Expr {
                                    Expr::Call {
                                        callee: Box::new(Expr::ExternFuncRef {
                                            name: "js_weak_throw_primitive".to_string(),
                                            param_types: Vec::new(),
                                            return_type: Type::Any,
                                        }),
                                        args: Vec::new(),
                                        type_args: Vec::new(),
                                    }
                                };
                                if ctx.weakmap_locals.contains(&recv_name) {
                                    let map_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    let recv = Expr::LocalGet(map_id);
                                    match method_name {
                                        "set" if args.len() >= 2 => {
                                            let key_is_primitive_lit = matches!(
                                                call.args.get(0).map(|a| a.expr.as_ref()),
                                                Some(ast::Expr::Lit(_))
                                            );
                                            if key_is_primitive_lit {
                                                return Ok(throw_primitive_expr());
                                            }
                                            let mut iter = args.into_iter();
                                            let key = iter.next().unwrap();
                                            let value = iter.next().unwrap();
                                            return Ok(make_extern_call("js_weakmap_set", vec![recv, key, value]));
                                        }
                                        "get" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_get", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "has" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_has", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "delete" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakmap_delete", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        _ => {}
                                    }
                                }
                                if ctx.weakset_locals.contains(&recv_name) {
                                    let set_id = ctx.lookup_local(&recv_name).unwrap_or(0);
                                    let recv = Expr::LocalGet(set_id);
                                    match method_name {
                                        "add" if args.len() >= 1 => {
                                            let value_is_primitive_lit = matches!(
                                                call.args.get(0).map(|a| a.expr.as_ref()),
                                                Some(ast::Expr::Lit(_))
                                            );
                                            if value_is_primitive_lit {
                                                return Ok(throw_primitive_expr());
                                            }
                                            return Ok(make_extern_call("js_weakset_add", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "has" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakset_has", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        "delete" if args.len() >= 1 => {
                                            return Ok(make_extern_call("js_weakset_delete", vec![recv, args.into_iter().next().unwrap()]));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        // Check for array method calls (arr.push, arr.pop, etc.)
                        // These are called on local variables, not global modules
                        // IMPORTANT: Only apply to actual Array types, not String types
                        if let ast::MemberProp::Ident(method_ident) = &member.prop {
                            let method_name = method_ident.sym.as_ref();
                            if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                let arr_name = arr_ident.sym.to_string();
                                // Check that this is NOT a String type (Array, Set, Map are all OK)
                                // When type is unknown, only enter array block for array-only methods
                                // (push, pop, etc.), NOT for methods shared with strings (indexOf,
                                // includes, split) — those are handled by the general dispatch which
                                // checks is_string at codegen time.
                                let type_info = ctx.lookup_local_type(&arr_name);
                                // `Union<String, Void>` (e.g. `JSON.stringify` return type) is
                                // a possible-string — must NOT be treated as definitely not-a-
                                // string, otherwise `.indexOf`/`.includes` get routed through
                                // ArrayIndexOf/ArrayIncludes and return -1/false on a real
                                // string value.
                                let is_union_with_string = matches!(
                                    type_info,
                                    Some(Type::Union(variants)) if variants.iter().any(|v| matches!(v, Type::String))
                                );
                                let is_known_string = type_info.map(|ty| matches!(ty, Type::String)).unwrap_or(false)
                                    || is_union_with_string;
                                // A user-defined class instance is NOT an array — must skip the array
                                // fast path so user-defined methods like Stack<T>.push() are dispatched
                                // to the class method, not runtime js_array_push. Map/Set/Promise are
                                // handled by explicit checks within the array block below.
                                let builtin_generic_bases = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                                // Imported classes don't show up in `lookup_class`; treat any
                                // uppercase imported identifier as a candidate class so the
                                // array fast-path doesn't swallow `coll.find(filter)` etc.
                                let is_imported_class_name = |n: &str| -> bool {
                                    if let Some(c) = n.chars().next() {
                                        if c.is_uppercase() && ctx.lookup_imported_func(n).is_some() {
                                            return true;
                                        }
                                    }
                                    false
                                };
                                let is_user_class_instance = match type_info {
                                    Some(Type::Named(name)) => {
                                        ctx.lookup_class(name).is_some() || is_imported_class_name(name)
                                    }
                                    Some(Type::Generic { base, .. }) => {
                                        !builtin_generic_bases.contains(&base.as_str())
                                            && (ctx.lookup_class(base).is_some()
                                                || is_imported_class_name(base))
                                    }
                                    _ => false,
                                };
                                // When the receiver type is Any and the method name is one
                                // commonly defined on user classes too (e.g. mongo's
                                // `Collection.find(filter)`), skip the array fast-path so the
                                // dispatch falls through to class-method resolution. Without this
                                // guard, the lowering blindly emits `Expr::ArrayFind` and the
                                // call resolves to `js_array_find` at codegen time, returning 0.
                                let is_class_overlapping_method = matches!(method_name,
                                    "find" | "findIndex" | "findLast" | "findLastIndex"
                                    | "map" | "filter" | "some" | "every"
                                    | "forEach" | "reduce" | "reduceRight"
                                    | "join"
                                );
                                let is_unknown_recv = matches!(type_info, None | Some(Type::Any) | Some(Type::Unknown));
                                let is_known_not_string = type_info
                                    .map(|ty| !matches!(ty, Type::String | Type::Any | Type::Unknown))
                                    .unwrap_or(false)
                                    && !is_union_with_string;
                                // Object type literals (e.g., { push: (v: number) => void; ... })
                                // are NOT arrays — they are plain objects with closure-valued
                                // properties and must NOT enter the array fast path.
                                let is_object_type = matches!(type_info, Some(Type::Object(_)));
                                // `Uint8Array`/`Buffer` instances must NOT enter the generic
                                // array fast path. They have a distinct runtime representation
                                // (raw `BufferHeader`, no f64 elements) and a different method
                                // family (`readUInt8`, `swap16`, byte-level `indexOf` matching
                                // string/buffer needles, etc.). The runtime's
                                // `dispatch_buffer_method` handles all of these via the
                                // universal `js_native_call_method` fallback path.
                                let is_buffer_type = matches!(
                                    type_info,
                                    Some(Type::Named(n))
                                        if n == "Uint8Array" || n == "Buffer" || n == "Uint8ClampedArray"
                                );
                                let is_ambiguous_method = matches!(method_name,
                                    "indexOf" | "includes" | "slice"
                                );
                                let is_not_string = if is_known_string {
                                    false  // definitely a string, skip array block
                                } else if is_user_class_instance {
                                    false  // user class — must dispatch to class method, skip array fast-path
                                } else if is_object_type {
                                    false  // object type literal — dispatch via method call, not array ops
                                } else if is_buffer_type {
                                    false  // Buffer/Uint8Array — runtime dispatch handles byte-level methods
                                } else if is_known_not_string {
                                    true   // definitely not a string, enter array block
                                } else if is_ambiguous_method {
                                    false  // type unknown + ambiguous method, skip array block (fall through to general dispatch)
                                } else if is_unknown_recv && is_class_overlapping_method {
                                    false  // type unknown + method commonly defined on user classes — fall through
                                } else {
                                    true   // type unknown + array-only method (push, pop, etc.), enter array block
                                };
                                // Helper: if the callback arg is a bare Boolean/Number/String identifier,
                                // desugar to a synthetic closure: x => Boolean(x) / Number(x) / String(x).
                                // This is needed because .filter(Boolean) etc. expect a closure pointer at
                                // runtime but built-in constructors aren't first-class closure objects.
                                if is_not_string {
                                if let Some(array_id) = ctx.lookup_local(&arr_name) {
                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // Check if any argument has spread operator —
                                                // when present, route through the spread path.
                                                // Multi-arg push without spread is desugared to a
                                                // Sequence of ArrayPush statements (one per arg);
                                                // JS spec returns the final array length, which is
                                                // exactly what the last ArrayPush returns.
                                                let any_spread = call.args.iter().any(|a| a.spread.is_some());
                                                if any_spread {
                                                    if args.len() == 1 {
                                                        return Ok(Expr::ArrayPushSpread {
                                                            array_id,
                                                            source: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                    // Mixed regular + spread: bail to generic
                                                    // dispatch (no current single-IR-shape).
                                                } else {
                                                    if args.len() == 1 {
                                                        return Ok(Expr::ArrayPush {
                                                            array_id,
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                    let mut stmts: Vec<Expr> = Vec::with_capacity(args.len());
                                                    for a in args.into_iter() {
                                                        stmts.push(Expr::ArrayPush {
                                                            array_id,
                                                            value: Box::new(a),
                                                        });
                                                    }
                                                    return Ok(Expr::Sequence(stmts));
                                                }
                                            }
                                        }
                                        "pop" => {
                                            return Ok(Expr::ArrayPop(array_id));
                                        }
                                        "shift" => {
                                            return Ok(Expr::ArrayShift(array_id));
                                        }
                                        "unshift" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayUnshift {
                                                    array_id,
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            // arr.slice(start, end?) - returns new array
                                            // Only convert to ArraySlice if we KNOW it's an Array type
                                            // (Type::Any could be a string, which has its own .slice() method)
                                            let is_definitely_array = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if is_definitely_array && args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                            // Fall through to normal Call handling for strings or unknown types
                                        }
                                        "splice" => {
                                            // arr.splice(start, deleteCount?, ...items) - returns deleted elements
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArraySplice {
                                                    array_id,
                                                    start: Box::new(start),
                                                    delete_count: delete_count.map(Box::new),
                                                    items,
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if !is_map_or_set && args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "map" | "filter" | "find" | "findIndex"
                                        | "findLast" | "findLastIndex" | "some"
                                        | "every" | "at" => {
                                            // Skip the array-method fast path when the receiver
                                            // is a known class instance (e.g. mongo `Collection.find`).
                                            // Without this guard, `coll.find(filter)` lowers to
                                            // `Expr::ArrayFind` and dispatches to `js_array_find`,
                                            // which silently returns 0 on a class receiver.
                                            let recv_ty = ctx.lookup_local_type(&arr_name);
                                            // TypedArray types are Named but must NOT be treated as
                                            // class instances — they need the array-method fast path
                                            // so `.at()` / `.findLast()` emit the right HIR variants.
                                            let is_typed_array = recv_ty.as_ref().map(|ty| {
                                                matches!(ty, Type::Named(n) if matches!(
                                                    n.as_str(),
                                                    "Int8Array" | "Int16Array" | "Int32Array"
                                                    | "Uint8Array" | "Uint8ClampedArray"
                                                    | "Uint16Array" | "Uint32Array"
                                                    | "Float32Array" | "Float64Array"
                                                    | "BigInt64Array" | "BigUint64Array"
                                                ))
                                            }).unwrap_or(false);
                                            let is_class_instance = !is_typed_array && recv_ty
                                                .as_ref()
                                                .map(|ty| matches!(ty, Type::Named(_) | Type::Generic { .. })
                                                    && !matches!(ty, Type::Array(_)))
                                                .unwrap_or(false);
                                            if !is_class_instance {
                                                if method_name == "at" {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayAt {
                                                            array: Box::new(Expr::LocalGet(array_id)),
                                                            index: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                } else if args.len() >= 1 {
                                                    let cb = args.into_iter().next().unwrap();
                                                    let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                    let array = Box::new(Expr::LocalGet(array_id));
                                                    let callback = Box::new(cb);
                                                    return Ok(match method_name {
                                                        "map" => Expr::ArrayMap { array, callback },
                                                        "filter" => Expr::ArrayFilter { array, callback },
                                                        "find" => Expr::ArrayFind { array, callback },
                                                        "findIndex" => Expr::ArrayFindIndex { array, callback },
                                                        "findLast" => Expr::ArrayFindLast { array, callback },
                                                        "findLastIndex" => Expr::ArrayFindLastIndex { array, callback },
                                                        "some" => Expr::ArraySome { array, callback },
                                                        "every" => Expr::ArrayEvery { array, callback },
                                                        _ => unreachable!(),
                                                    });
                                                }
                                            }
                                        }
                                        "flatMap" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFlatMap {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "join" => {
                                            // arr.join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                separator,
                                            });
                                        }
                                        "flat" => {
                                            // arr.flat() -> flattened array
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                        "reduceRight" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduceRight {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "toReversed" => {
                                            return Ok(Expr::ArrayToReversed {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                            });
                                        }
                                        "toSorted" => {
                                            let comparator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayToSorted {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                comparator,
                                            });
                                        }
                                        "toSpliced" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next().unwrap();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArrayToSpliced {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    start: Box::new(start),
                                                    delete_count: Box::new(delete_count),
                                                    items,
                                                });
                                            }
                                        }
                                        "with" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let index = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::ArrayWith {
                                                    array: Box::new(Expr::LocalGet(array_id)),
                                                    index: Box::new(index),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "copyWithin" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let target = args_iter.next().unwrap();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayCopyWithin {
                                                    array_id,
                                                    target: Box::new(target),
                                                    start: Box::new(start),
                                                    end,
                                                });
                                            }
                                        }
                                        "entries" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayEntries(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        "keys" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayKeys(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        "values" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map {
                                                return Ok(Expr::MapValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set {
                                                return Ok(Expr::SetValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            return Ok(Expr::ArrayValues(Box::new(Expr::LocalGet(array_id))));
                                        }
                                        // Map methods (only apply to actual Map/Set types)
                                        "set" => {
                                            // Check if this is a Map or Set type before treating as Map.set()
                                            let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false);
                                            if is_map_or_set && args.len() >= 2 {
                                                // map.set(key, value) - returns the map for chaining
                                                let mut args_iter = args.into_iter();
                                                let key = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::MapSet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(key),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "get" => {
                                            // Check if this is a Map type before treating as Map.get()
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.len() >= 1 {
                                                // map.get(key) - returns value or undefined
                                                return Ok(Expr::MapGet {
                                                    map: Box::new(Expr::LocalGet(array_id)),
                                                    key: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "has" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetHas {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapHas {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "delete" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if (is_set || is_map) && args.len() >= 1 {
                                                let value = args.into_iter().next().unwrap();
                                                if is_set {
                                                    return Ok(Expr::SetDelete {
                                                        set: Box::new(Expr::LocalGet(array_id)),
                                                        value: Box::new(value),
                                                    });
                                                } else {
                                                    return Ok(Expr::MapDelete {
                                                        map: Box::new(Expr::LocalGet(array_id)),
                                                        key: Box::new(value),
                                                    });
                                                }
                                            }
                                        }
                                        "clear" => {
                                            // Check if this is a Set or Map - only apply to actual Set/Map types
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_set {
                                                return Ok(Expr::SetClear(Box::new(Expr::LocalGet(array_id))));
                                            } else if is_map {
                                                return Ok(Expr::MapClear(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            // Fall through if neither Set nor Map
                                        }
                                        // Map iterator methods: entries(), keys(), values()
                                        "entries" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapEntries(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "keys" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapKeys(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        "values" => {
                                            let is_map = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                                .unwrap_or(false);
                                            if is_map && args.is_empty() {
                                                return Ok(Expr::MapValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.is_empty() {
                                                return Ok(Expr::SetValues(Box::new(Expr::LocalGet(array_id))));
                                            }
                                        }
                                        // Set methods
                                        "add" => {
                                            // Check if this is a Set type before treating as Set.add()
                                            let is_set = ctx.lookup_local_type(&arr_name)
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                                .unwrap_or(false);
                                            if is_set && args.len() >= 1 {
                                                // set.add(value) - returns the set for chaining
                                                let value = args.into_iter().next().unwrap();
                                                return Ok(Expr::SetAdd {
                                                    set_id: array_id,
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        _ => {} // Fall through to generic handling
                                    }

                                    // URLSearchParams methods
                                    let is_url_search_params = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "URLSearchParams"))
                                        .unwrap_or(false);
                                    if is_url_search_params {
                                        match method_name {
                                            "get" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "has" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsHas {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "set" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsSet {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "append" => {
                                                if args.len() >= 2 {
                                                    let mut args_iter = args.into_iter();
                                                    let name_arg = args_iter.next().unwrap();
                                                    let value_arg = args_iter.next().unwrap();
                                                    return Ok(Expr::UrlSearchParamsAppend {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(name_arg),
                                                        value: Box::new(value_arg),
                                                    });
                                                }
                                            }
                                            "delete" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsDelete {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            "toString" => {
                                                return Ok(Expr::UrlSearchParamsToString(
                                                    Box::new(Expr::LocalGet(array_id))
                                                ));
                                            }
                                            "getAll" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::UrlSearchParamsGetAll {
                                                        params: Box::new(Expr::LocalGet(array_id)),
                                                        name: Box::new(args.into_iter().next().unwrap()),
                                                    });
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // TextEncoder methods
                                    let is_text_encoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                        .unwrap_or(false);
                                    if is_text_encoder {
                                        match method_name {
                                            "encode" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::TextEncoderEncode(Box::new(args.into_iter().next().unwrap())));
                                                } else {
                                                    // encode() with no args encodes empty string
                                                    return Ok(Expr::TextEncoderEncode(Box::new(Expr::String(String::new()))));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    // TextDecoder methods
                                    let is_text_decoder = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                        .unwrap_or(false);
                                    if is_text_decoder {
                                        match method_name {
                                            "decode" => {
                                                if args.len() >= 1 {
                                                    return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                                } else {
                                                    // decode() with no args returns empty string
                                                    return Ok(Expr::String(String::new()));
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                }  // close is_array_type check
                            }

                            // Check for array methods on property access (e.g., this.items.push(value))
                            // This handles cases where the array is a property of an object, not a local variable
                            if let ast::Expr::Member(obj_member) = member.obj.as_ref() {
                                if let ast::MemberProp::Ident(obj_prop_ident) = &obj_member.prop {
                                    let _property_name = obj_prop_ident.sym.to_string();
                                    // Lower the object expression (e.g., 'this' or a local variable)
                                    let _object_expr = lower_expr(ctx, &obj_member.obj)?;

                                    match method_name {
                                        "push" => {
                                            if args.len() >= 1 {
                                                // For now, fall through to generic Call handling
                                                // We'll compile this in codegen using inline property access
                                                // property-based push: object.{property}.push()
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on imported variables (e.g., import { CHAIN_NAMES } from './module')
                    // These don't have local IDs but are ExternFuncRef values
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                                    let arr_name = arr_ident.sym.to_string();
                                    // Check if this is an imported variable (not a local)
                                    if ctx.lookup_local(&arr_name).is_none() {
                                        if let Some(orig_name) = ctx.lookup_imported_func(&arr_name) {
                                            // This is an imported variable - create ExternFuncRef for it
                                            let (param_types, return_type) = ctx.lookup_extern_func_types(orig_name)
                                                .map(|(p, r)| (p.clone(), r.clone()))
                                                .unwrap_or_else(|| (Vec::new(), Type::Any));
                                            let extern_ref = Expr::ExternFuncRef {
                                                name: orig_name.to_string(),
                                                param_types,
                                                return_type,
                                            };
                                            match method_name {
                                                "join" => {
                                                    // arr.join(separator?) -> string
                                                    let separator = args.into_iter().next().map(Box::new);
                                                    return Ok(Expr::ArrayJoin {
                                                        array: Box::new(extern_ref),
                                                        separator,
                                                    });
                                                }
                                                "map" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayMap {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "filter" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayFilter {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "forEach" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayForEach {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "find" => {
                                                    if args.len() >= 1 {
                                                        let cb = args.into_iter().next().unwrap();
                                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                        return Ok(Expr::ArrayFind {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(cb),
                                                        });
                                                    }
                                                }
                                                "sort" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArraySort {
                                                            array: Box::new(extern_ref),
                                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "indexOf" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIndexOf {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "includes" => {
                                                    if args.len() >= 1 {
                                                        return Ok(Expr::ArrayIncludes {
                                                            array: Box::new(extern_ref),
                                                            value: Box::new(args.into_iter().next().unwrap()),
                                                        });
                                                    }
                                                }
                                                "slice" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let start = args_iter.next().unwrap();
                                                        let end = args_iter.next();
                                                        return Ok(Expr::ArraySlice {
                                                            array: Box::new(extern_ref),
                                                            start: Box::new(start),
                                                            end: end.map(Box::new),
                                                        });
                                                    }
                                                }
                                                "reduce" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let callback = args_iter.next().unwrap();
                                                        let initial = args_iter.next().map(Box::new);
                                                        return Ok(Expr::ArrayReduce {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(callback),
                                                            initial,
                                                        });
                                                    }
                                                }
                                                "flat" => {
                                                    return Ok(Expr::ArrayFlat {
                                                        array: Box::new(extern_ref),
                                                    });
                                                }
                                                "reduceRight" => {
                                                    if args.len() >= 1 {
                                                        let mut args_iter = args.into_iter();
                                                        let callback = args_iter.next().unwrap();
                                                        let initial = args_iter.next().map(Box::new);
                                                        return Ok(Expr::ArrayReduceRight {
                                                            array: Box::new(extern_ref),
                                                            callback: Box::new(callback),
                                                            initial,
                                                        });
                                                    }
                                                }
                                                "toReversed" => {
                                                    return Ok(Expr::ArrayToReversed {
                                                        array: Box::new(extern_ref),
                                                    });
                                                }
                                                "toSorted" => {
                                                    let comparator = args.into_iter().next().map(Box::new);
                                                    return Ok(Expr::ArrayToSorted {
                                                        array: Box::new(extern_ref),
                                                        comparator,
                                                    });
                                                }
                                                "toSpliced" => {
                                                    if args.len() >= 2 {
                                                        let mut args_iter = args.into_iter();
                                                        let start = args_iter.next().unwrap();
                                                        let delete_count = args_iter.next().unwrap();
                                                        let items: Vec<Expr> = args_iter.collect();
                                                        return Ok(Expr::ArrayToSpliced {
                                                            array: Box::new(extern_ref),
                                                            start: Box::new(start),
                                                            delete_count: Box::new(delete_count),
                                                            items,
                                                        });
                                                    }
                                                }
                                                "with" => {
                                                    if args.len() >= 2 {
                                                        let mut args_iter = args.into_iter();
                                                        let index = args_iter.next().unwrap();
                                                        let value = args_iter.next().unwrap();
                                                        return Ok(Expr::ArrayWith {
                                                            array: Box::new(extern_ref),
                                                            index: Box::new(index),
                                                            value: Box::new(value),
                                                        });
                                                    }
                                                }
                                                "entries" => {
                                                    return Ok(Expr::ArrayEntries(Box::new(extern_ref)));
                                                }
                                                "keys" => {
                                                    return Ok(Expr::ArrayKeys(Box::new(extern_ref)));
                                                }
                                                "values" => {
                                                    return Ok(Expr::ArrayValues(Box::new(extern_ref)));
                                                }
                                                _ => {} // Fall through for other methods
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for array methods on inline array literals (e.g., ['a', 'b'].join('-'))
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                if let ast::Expr::Array(_arr_lit) = member.obj.as_ref() {
                                    // Lower the array literal
                                    let array_expr = lower_expr(ctx, &member.obj)?;
                                    match method_name {
                                        "join" => {
                                            // ['a', 'b'].join(separator?) -> string
                                            let separator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayJoin {
                                                array: Box::new(array_expr),
                                                separator,
                                            });
                                        }
                                        "map" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayMap {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "filter" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFilter {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "forEach" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayForEach {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "find" => {
                                            if args.len() >= 1 {
                                                let cb = args.into_iter().next().unwrap();
                                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                                return Ok(Expr::ArrayFind {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(cb),
                                                });
                                            }
                                        }
                                        "sort" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArraySort {
                                                    array: Box::new(array_expr),
                                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "indexOf" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIndexOf {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "includes" => {
                                            if args.len() >= 1 {
                                                return Ok(Expr::ArrayIncludes {
                                                    array: Box::new(array_expr),
                                                    value: Box::new(args.into_iter().next().unwrap()),
                                                });
                                            }
                                        }
                                        "slice" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let end = args_iter.next();
                                                return Ok(Expr::ArraySlice {
                                                    array: Box::new(array_expr),
                                                    start: Box::new(start),
                                                    end: end.map(Box::new),
                                                });
                                            }
                                        }
                                        "reduce" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduce {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "flat" => {
                                            return Ok(Expr::ArrayFlat {
                                                array: Box::new(array_expr),
                                            });
                                        }
                                        "reduceRight" => {
                                            if args.len() >= 1 {
                                                let mut args_iter = args.into_iter();
                                                let callback = args_iter.next().unwrap();
                                                let initial = args_iter.next().map(Box::new);
                                                return Ok(Expr::ArrayReduceRight {
                                                    array: Box::new(array_expr),
                                                    callback: Box::new(callback),
                                                    initial,
                                                });
                                            }
                                        }
                                        "toReversed" => {
                                            return Ok(Expr::ArrayToReversed {
                                                array: Box::new(array_expr),
                                            });
                                        }
                                        "toSorted" => {
                                            let comparator = args.into_iter().next().map(Box::new);
                                            return Ok(Expr::ArrayToSorted {
                                                array: Box::new(array_expr),
                                                comparator,
                                            });
                                        }
                                        "toSpliced" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let start = args_iter.next().unwrap();
                                                let delete_count = args_iter.next().unwrap();
                                                let items: Vec<Expr> = args_iter.collect();
                                                return Ok(Expr::ArrayToSpliced {
                                                    array: Box::new(array_expr),
                                                    start: Box::new(start),
                                                    delete_count: Box::new(delete_count),
                                                    items,
                                                });
                                            }
                                        }
                                        "with" => {
                                            if args.len() >= 2 {
                                                let mut args_iter = args.into_iter();
                                                let index = args_iter.next().unwrap();
                                                let value = args_iter.next().unwrap();
                                                return Ok(Expr::ArrayWith {
                                                    array: Box::new(array_expr),
                                                    index: Box::new(index),
                                                    value: Box::new(value),
                                                });
                                            }
                                        }
                                        "entries" => {
                                            return Ok(Expr::ArrayEntries(Box::new(array_expr)));
                                        }
                                        "keys" => {
                                            return Ok(Expr::ArrayKeys(Box::new(array_expr)));
                                        }
                                        "values" => {
                                            return Ok(Expr::ArrayValues(Box::new(array_expr)));
                                        }
                                        _ => {} // Fall through for other methods
                                    }
                                }
                            }
                        }
                    }

                    // TextEncoder.encode() / TextDecoder.decode() on inline expressions
                    // e.g., new TextEncoder().encode("hello"), new TextDecoder().decode(buf)
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                // Check if the receiver is new TextEncoder() or new TextDecoder()
                                if let ast::Expr::New(new_expr) = member.obj.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                        let class_name = class_ident.sym.as_ref();
                                        if class_name == "TextEncoder" && method_name == "encode" {
                                            let str_arg = if args.len() >= 1 {
                                                args.into_iter().next().unwrap()
                                            } else {
                                                Expr::String(String::new())
                                            };
                                            return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                                        }
                                        if class_name == "TextDecoder" && method_name == "decode" {
                                            if args.len() >= 1 {
                                                return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                            } else {
                                                return Ok(Expr::String(String::new()));
                                            }
                                        }
                                    }
                                }
                                // Also check for local variable typed as TextEncoder/TextDecoder
                                if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                    let obj_name = obj_ident.sym.to_string();
                                    let is_text_encoder = ctx.lookup_local_type(&obj_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                                        .unwrap_or(false);
                                    if is_text_encoder && method_name == "encode" {
                                        let str_arg = if args.len() >= 1 {
                                            args.into_iter().next().unwrap()
                                        } else {
                                            Expr::String(String::new())
                                        };
                                        return Ok(Expr::TextEncoderEncode(Box::new(str_arg)));
                                    }
                                    let is_text_decoder = ctx.lookup_local_type(&obj_name)
                                        .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                                        .unwrap_or(false);
                                    if is_text_decoder && method_name == "decode" {
                                        if args.len() >= 1 {
                                            return Ok(Expr::TextDecoderDecode(Box::new(args.into_iter().next().unwrap())));
                                        } else {
                                            return Ok(Expr::String(String::new()));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for array-only methods on any expression (e.g., Object.entries(x).reduce(...))
                    // ONLY match methods that are unique to arrays (not shared with strings)
                    // "includes", "indexOf", "slice", "join" also exist on strings, so skip those
                    if let ast::Callee::Expr(expr) = &call.callee {
                        if let ast::Expr::Member(member) = expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let method_name = method_ident.sym.as_ref();
                                // Helper: skip array-method dispatch when the receiver is a
                                // known class instance (e.g. mongo `Collection.find`,
                                // `Stack<T>.map`). Without this guard the lowering blindly
                                // emits `Expr::Array<Method>` and the compiled binary calls
                                // `js_array_<method>` on a class handle.
                                let recv_is_class = match member.obj.as_ref() {
                                    ast::Expr::Ident(ident) => {
                                        let n = ident.sym.to_string();
                                        let ty = ctx.lookup_local_type(&n);
                                        let class_typed = ty
                                            .as_ref()
                                            .map(|t| matches!(t, Type::Named(_) | Type::Generic { .. })
                                                && !matches!(t, Type::Array(_)))
                                            .unwrap_or(false);
                                        let unknown_recv = matches!(ty, None | Some(Type::Any) | Some(Type::Unknown));
                                        let is_overlapping = matches!(method_name,
                                            "find" | "findIndex" | "findLast" | "findLastIndex"
                                            | "map" | "filter" | "some" | "every"
                                            | "forEach" | "reduce" | "reduceRight" | "join"
                                        );
                                        class_typed || (unknown_recv && is_overlapping)
                                    }
                                    ast::Expr::New(_) => true,
                                    _ => false,
                                };
                                match method_name {
                                    "reduce" if args.len() >= 1 && !recv_is_class => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduce {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                    "map" if args.len() >= 1 && !recv_is_class => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayMap {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "filter" if args.len() >= 1 && !recv_is_class => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFilter {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "forEach" if args.len() >= 1 && !recv_is_class => {
                                        // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                                        let is_map_or_set = if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                                .unwrap_or(false)
                                        } else {
                                            false
                                        };
                                        if !is_map_or_set {
                                            let cb = args.into_iter().next().unwrap();
                                            let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            return Ok(Expr::ArrayForEach {
                                                array: Box::new(array_expr),
                                                callback: Box::new(cb),
                                            });
                                        }
                                    }
                                    "find" if args.len() >= 1 && !recv_is_class => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFind {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "findIndex" if args.len() >= 1 && !recv_is_class => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFindIndex {
                                            array: Box::new(array_expr),
                                            callback: Box::new(cb),
                                        });
                                    }
                                    "sort" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArraySort {
                                            array: Box::new(array_expr),
                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                        });
                                    }
                                    // .slice() exists on both Array and String, so we can only safely
                                    // lower to ArraySlice when the receiver is definitely an
                                    // array-producing expression (matches the indexOf/includes pattern
                                    // below). Without this, `arr.sort(cb).slice(0, 5)` falls through to
                                    // generic dynamic dispatch which corrupts the result — the inner
                                    // ArraySort returns a real array pointer but the outer .slice goes
                                    // through `js_native_call_method` which can't unwrap it properly,
                                    // producing an "object" with the right .length but Array.isArray
                                    // returns false and JSON.stringify segfaults.
                                    "slice" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) | Expr::ArraySpread(_) |
                                            Expr::ArrayFrom(_) | Expr::ArrayFromMapped { .. } |
                                            Expr::ArrayFlat { .. } | Expr::StringSplit(_, _) |
                                            Expr::ArrayToReversed { .. } | Expr::ArrayToSorted { .. } |
                                            Expr::ArrayToSpliced { .. } | Expr::ArrayWith { .. } |
                                            Expr::ArrayEntries(_) | Expr::ArrayKeys(_) | Expr::ArrayValues(_) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) | Expr::ObjectEntries(_) |
                                            // `process.argv` is a `string[]`. Without this arm the
                                            // fallthrough picked String.slice semantics — so
                                            // `process.argv.slice(2)` returned a "string" whose
                                            // length was the argv count and whose elements were
                                            // NaN-box bits of string pointers read as doubles
                                            // (closes #41).
                                            Expr::ProcessArgv
                                        ) {
                                            let mut args_iter = args.into_iter();
                                            let start = args_iter.next().unwrap();
                                            let end = args_iter.next();
                                            return Ok(Expr::ArraySlice {
                                                array: Box::new(array_expr),
                                                start: Box::new(start),
                                                end: end.map(Box::new),
                                            });
                                        }
                                        // Fall through to generic Call handling (could be a String.slice).
                                    }
                                    // .join() is exclusively an Array method (strings don't have it),
                                    // so we can always safely lower to ArrayJoin regardless of the
                                    // receiver expression type. Previously this only matched specific
                                    // array-returning expressions, which caused .split().join() chains
                                    // to fall through to generic dispatch and produce wrong results.
                                    "join" if args.len() <= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let separator = if args.is_empty() { None } else { Some(Box::new(args.into_iter().next().unwrap())) };
                                        return Ok(Expr::ArrayJoin {
                                            array: Box::new(array_expr),
                                            separator,
                                        });
                                    }
                                    "indexOf" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        if matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) |
                                            Expr::PropertyGet { .. }
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIndexOf {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "includes" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        // Don't treat error string properties as arrays
                                        let is_error_string_prop = matches!(&array_expr,
                                            Expr::PropertyGet { property, .. }
                                            if matches!(property.as_str(), "stack" | "message" | "name")
                                        );
                                        if !is_error_string_prop && matches!(&array_expr,
                                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                            Expr::ArraySlice { .. } | Expr::Array(_) |
                                            Expr::ArrayFrom(_) | Expr::StringSplit(_, _) |
                                            Expr::ObjectKeys(_) | Expr::ObjectValues(_) |
                                            Expr::PropertyGet { .. }
                                        ) {
                                            let value_expr = args.into_iter().next().unwrap();
                                            return Ok(Expr::ArrayIncludes {
                                                array: Box::new(array_expr),
                                                value: Box::new(value_expr),
                                            });
                                        }
                                    }
                                    "flat" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayFlat {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                    "reduceRight" if args.len() >= 1 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Expr::ArrayReduceRight {
                                            array: Box::new(array_expr),
                                            callback: Box::new(callback),
                                            initial,
                                        });
                                    }
                                    "toReversed" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        return Ok(Expr::ArrayToReversed {
                                            array: Box::new(array_expr),
                                        });
                                    }
                                    "toSorted" => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let comparator = args.into_iter().next().map(Box::new);
                                        return Ok(Expr::ArrayToSorted {
                                            array: Box::new(array_expr),
                                            comparator,
                                        });
                                    }
                                    "toSpliced" if args.len() >= 2 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let start = args_iter.next().unwrap();
                                        let delete_count = args_iter.next().unwrap();
                                        let items: Vec<Expr> = args_iter.collect();
                                        return Ok(Expr::ArrayToSpliced {
                                            array: Box::new(array_expr),
                                            start: Box::new(start),
                                            delete_count: Box::new(delete_count),
                                            items,
                                        });
                                    }
                                    "with" if args.len() >= 2 => {
                                        let array_expr = lower_expr(ctx, &member.obj)?;
                                        let mut args_iter = args.into_iter();
                                        let index = args_iter.next().unwrap();
                                        let value = args_iter.next().unwrap();
                                        return Ok(Expr::ArrayWith {
                                            array: Box::new(array_expr),
                                            index: Box::new(index),
                                            value: Box::new(value),
                                        });
                                    }
                                    "push" if args.len() >= 1 => {
                                        // Generic expr.push(value) or expr.push(...spread)
                                        // GUARD: Skip if the receiver is a user-defined class instance
                                        // (e.g. Stack<T>.push()), or an object type literal (e.g.
                                        // { push: (v) => void, ... }), so its method dispatches correctly.
                                        let is_user_class_receiver = match member.obj.as_ref() {
                                            ast::Expr::Ident(ident) => {
                                                ctx.lookup_local_type(&ident.sym.to_string()).map(|ty| {
                                                    match ty {
                                                        Type::Named(name) => ctx.lookup_class(name).is_some(),
                                                        Type::Generic { base, .. } => {
                                                            let builtin = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                                                            !builtin.contains(&base.as_str()) && ctx.lookup_class(base).is_some()
                                                        }
                                                        Type::Object(_) => true, // object type literal with push property
                                                        _ => false,
                                                    }
                                                }).unwrap_or(false)
                                            }
                                            ast::Expr::New(_) => true, // new ClassName().push()
                                            _ => false,
                                        };
                                        if !is_user_class_receiver {
                                            let array_expr = lower_expr(ctx, &member.obj)?;
                                            if call.args.len() >= 1 && call.args[0].spread.is_some() {
                                                return Ok(Expr::NativeMethodCall {
                                                    module: "array".to_string(),
                                                    method: "push_spread".to_string(),
                                                    class_name: None,
                                                    object: Some(Box::new(array_expr)),
                                                    args: args,
                                                });
                                            } else {
                                                return Ok(Expr::NativeMethodCall {
                                                    module: "array".to_string(),
                                                    method: "push_single".to_string(),
                                                    class_name: None,
                                                    object: Some(Box::new(array_expr)),
                                                    args: args,
                                                });
                                            }
                                        }
                                    }
                                    _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                                }
                            }
                        }
                    }

                    // Check for regex .test() / .exec() method call on any expression
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                let m = method_ident.sym.as_ref();
                                if (m == "test" || m == "exec") && args.len() == 1 {
                                    // Check if the object is a regex literal or a local assigned to a regex
                                    let is_regex_obj = match member.obj.as_ref() {
                                        ast::Expr::Lit(ast::Lit::Regex(_)) => true,
                                        ast::Expr::Ident(ident) => {
                                            ctx.lookup_local_type(&ident.sym.to_string())
                                                .map(|ty| matches!(ty, Type::Any | Type::Unknown) || matches!(ty, Type::Named(n) if n == "RegExp"))
                                                .unwrap_or(true)
                                        }
                                        _ => false,
                                    };
                                    if is_regex_obj {
                                        let regex_expr = lower_expr(ctx, &member.obj)?;
                                        // Only emit RegExp method calls if the object is actually a regex
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            let string_expr = args.into_iter().next().unwrap();
                                            if m == "test" {
                                                return Ok(Expr::RegExpTest {
                                                    regex: Box::new(regex_expr),
                                                    string: Box::new(string_expr),
                                                });
                                            } else {
                                                return Ok(Expr::RegExpExec {
                                                    regex: Box::new(regex_expr),
                                                    string: Box::new(string_expr),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for string .match(regex) method call
                    if let ast::Callee::Expr(callee_expr) = &call.callee {
                        if let ast::Expr::Member(member) = callee_expr.as_ref() {
                            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                                if (method_ident.sym.as_ref() == "match" || method_ident.sym.as_ref() == "matchAll") && args.len() == 1 {
                                    let is_match_all = method_ident.sym.as_ref() == "matchAll";
                                    // Check if the argument is a regex literal or a local holding a regex
                                    let arg_is_regex = match call.args.first().map(|a| a.expr.as_ref()) {
                                        Some(ast::Expr::Lit(ast::Lit::Regex(_))) => true,
                                        Some(ast::Expr::Ident(ident)) => {
                                            match ctx.lookup_local_type(&ident.sym.to_string()) {
                                                // Known regex local
                                                Some(Type::Named(n)) if n == "RegExp" => true,
                                                // Unknown type — assume could be regex
                                                Some(Type::Any) | Some(Type::Unknown) | None => true,
                                                _ => false,
                                            }
                                        }
                                        _ => false,
                                    };
                                    if arg_is_regex {
                                        let string_expr = lower_expr(ctx, &member.obj)?;
                                        let regex_expr = args.remove(0);
                                        if matches!(&regex_expr, Expr::RegExp { .. }) || matches!(&regex_expr, Expr::LocalGet(_)) {
                                            return Ok(if is_match_all {
                                                Expr::StringMatchAll {
                                                    string: Box::new(string_expr),
                                                    regex: Box::new(regex_expr),
                                                }
                                            } else {
                                                Expr::StringMatch {
                                                    string: Box::new(string_expr),
                                                    regex: Box::new(regex_expr),
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check for global built-in function calls (parseInt, parseFloat, Number, String, isNaN, isFinite)
                    if let ast::Expr::Ident(ident) = expr.as_ref() {
                        let func_name = ident.sym.as_ref();
                        match func_name {
                            "parseInt" => {
                                let string_arg = if args.len() >= 1 {
                                    Box::new(args.remove(0))
                                } else {
                                    return Err(anyhow!("parseInt requires at least one argument"));
                                };
                                let radix_arg = if !args.is_empty() {
                                    Some(Box::new(args.remove(0)))
                                } else {
                                    None
                                };
                                return Ok(Expr::ParseInt { string: string_arg, radix: radix_arg });
                            }
                            "parseFloat" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::ParseFloat(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("parseFloat requires one argument"));
                                }
                            }
                            "Number" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::NumberCoerce(Box::new(args.remove(0))));
                                } else {
                                    // Number() with no args returns 0
                                    return Ok(Expr::Number(0.0));
                                }
                            }
                            "BigInt" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::BigIntCoerce(Box::new(args.remove(0))));
                                } else {
                                    // BigInt() with no args returns 0n
                                    return Ok(Expr::BigInt("0".to_string()));
                                }
                            }
                            "String" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StringCoerce(Box::new(args.remove(0))));
                                } else {
                                    // String() with no args returns ""
                                    return Ok(Expr::String(String::new()));
                                }
                            }
                            "Boolean" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::BooleanCoerce(Box::new(args.remove(0))));
                                } else {
                                    // Boolean() with no args returns false
                                    return Ok(Expr::Bool(false));
                                }
                            }
                            "isNaN" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsNaN(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isNaN requires one argument"));
                                }
                            }
                            "isFinite" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::IsFinite(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("isFinite requires one argument"));
                                }
                            }
                            "atob" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::Atob(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("atob requires one argument"));
                                }
                            }
                            "btoa" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::Btoa(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("btoa requires one argument"));
                                }
                            }
                            "encodeURI" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::EncodeURI(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("encodeURI requires one argument"));
                                }
                            }
                            "decodeURI" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::DecodeURI(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("decodeURI requires one argument"));
                                }
                            }
                            "encodeURIComponent" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::EncodeURIComponent(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("encodeURIComponent requires one argument"));
                                }
                            }
                            "decodeURIComponent" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::DecodeURIComponent(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("decodeURIComponent requires one argument"));
                                }
                            }
                            "structuredClone" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StructuredClone(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("structuredClone requires one argument"));
                                }
                            }
                            "queueMicrotask" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::QueueMicrotask(Box::new(args.remove(0))));
                                } else {
                                    return Err(anyhow!("queueMicrotask requires one argument"));
                                }
                            }
                            "Symbol" => {
                                // Symbol() / Symbol(description)
                                if args.is_empty() {
                                    return Ok(Expr::SymbolNew(None));
                                } else {
                                    return Ok(Expr::SymbolNew(Some(Box::new(args.remove(0)))));
                                }
                            }
                            "perryResolveStaticPlugin" => {
                                if args.len() >= 1 {
                                    return Ok(Expr::StaticPluginResolve(Box::new(args.remove(0))));
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
                                    return Ok(Expr::FetchGetWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                    });
                                } else {
                                    return Err(anyhow!("fetchWithAuth requires url and authHeader arguments"));
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
                                    return Ok(Expr::FetchPostWithAuth {
                                        url: Box::new(url),
                                        auth_header: Box::new(auth_header),
                                        body: Box::new(body),
                                    });
                                } else {
                                    return Err(anyhow!("fetchPostWithAuth requires url, authHeader, and body arguments"));
                                }
                            }
                            "fetch" => {
                                // Handle fetch(url) and fetch(url, options)
                                // Extract URL (first argument)
                                let url = if args.len() >= 1 {
                                    args.remove(0)
                                } else {
                                    return Err(anyhow!("fetch requires at least a URL argument"));
                                };

                                // Check if there's an options object (second argument)
                                if args.len() >= 1 {
                                    // Extract options from the object literal
                                    // We need to get the original AST to extract the object properties
                                    if let Some(options_arg) = call.args.get(1) {
                                        if let ast::Expr::Object(obj) = &*options_arg.expr {
                                            // Extract method, body, and headers from options
                                            let mut method = Expr::String("GET".to_string());
                                            let mut body = Expr::Undefined;
                                            let mut headers_obj: Vec<(String, Expr)> = Vec::new();

                                            for prop in &obj.props {
                                                if let ast::PropOrSpread::Prop(prop) = prop {
                                                    match prop.as_ref() {
                                                        ast::Prop::KeyValue(kv) => {
                                                            let key = match &kv.key {
                                                                ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
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
                                                                    // Extract headers object
                                                                    if let ast::Expr::Object(headers_ast) = &*kv.value {
                                                                        for hprop in &headers_ast.props {
                                                                            if let ast::PropOrSpread::Prop(hprop) = hprop {
                                                                                if let ast::Prop::KeyValue(hkv) = hprop.as_ref() {
                                                                                    let hkey = match &hkv.key {
                                                                                        ast::PropName::Ident(ident) => ident.sym.to_string(),
                                                                                        ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                                                                                        _ => continue,
                                                                                    };
                                                                                    let hval = lower_expr(ctx, &hkv.value)?;
                                                                                    headers_obj.push((hkey, hval));
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                _ => {}
                                                            }
                                                        }
                                                        ast::Prop::Shorthand(ident) => {
                                                            // Handle shorthand properties like { body } which means { body: body }
                                                            let key = ident.sym.to_string();
                                                            let value = if let Some(local_id) = ctx.lookup_local(&key) {
                                                                Expr::LocalGet(local_id)
                                                            } else {
                                                                continue;
                                                            };
                                                            match key.as_str() {
                                                                "method" => method = value,
                                                                "body" => body = value,
                                                                _ => {}
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }

                                            // Create a FetchWithOptions expression
                                            ctx.uses_fetch = true;
                                            return Ok(Expr::FetchWithOptions {
                                                url: Box::new(url),
                                                method: Box::new(method),
                                                body: Box::new(body),
                                                headers: headers_obj,
                                            });
                                        }
                                    }
                                }

                                // Simple fetch(url) with no options - use GET
                                ctx.uses_fetch = true;
                                return Ok(Expr::FetchWithOptions {
                                    url: Box::new(url),
                                    method: Box::new(Expr::String("GET".to_string())),
                                    body: Box::new(Expr::Undefined),
                                    headers: Vec::new(),
                                });
                            }
                            _ => {} // Fall through to generic handling
                        }

                        // Check if this is a named import from child_process (e.g., execSync, spawnSync)
                        if let Some((module_name, _method)) = ctx.lookup_native_module(func_name) {
                            if module_name == "child_process" {
                                match func_name {
                                    "execSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExecSync {
                                                command: Box::new(command),
                                                options,
                                            });
                                        }
                                    }
                                    "spawnSync" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnSync {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "spawn" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let options = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawn {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                options,
                                            });
                                        }
                                    }
                                    "exec" => {
                                        if args.len() >= 1 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let options = args_iter.next().map(Box::new);
                                            let callback = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessExec {
                                                command: Box::new(command),
                                                options,
                                                callback,
                                            });
                                        }
                                    }
                                    "spawnBackground" => {
                                        if args.len() >= 3 {
                                            let mut args_iter = args.into_iter();
                                            let command = args_iter.next().unwrap();
                                            let spawn_args = args_iter.next().map(Box::new);
                                            let log_file = args_iter.next().unwrap();
                                            let env_json = args_iter.next().map(Box::new);
                                            return Ok(Expr::ChildProcessSpawnBackground {
                                                command: Box::new(command),
                                                args: spawn_args,
                                                log_file: Box::new(log_file),
                                                env_json,
                                            });
                                        }
                                    }
                                    "getProcessStatus" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessGetProcessStatus(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    "killProcess" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::ChildProcessKillProcess(
                                                Box::new(args.into_iter().next().unwrap())
                                            ));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from path (e.g., join, dirname, basename)
                            if module_name == "path" {
                                match func_name {
                                    "join" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let mut result = iter.next().unwrap();
                                            for next_arg in iter {
                                                result = Expr::PathJoin(Box::new(result), Box::new(next_arg));
                                            }
                                            return Ok(result);
                                        }
                                    }
                                    "dirname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathDirname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "basename" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path_arg = iter.next().unwrap();
                                            let ext_arg = iter.next().unwrap();
                                            return Ok(Expr::PathBasenameExt(Box::new(path_arg), Box::new(ext_arg)));
                                        }
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathBasename(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "extname" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathExtname(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "resolve" => {
                                        if args.len() >= 1 {
                                            let mut iter = args.into_iter();
                                            let first = iter.next().unwrap();
                                            let mut joined = first;
                                            for next_arg in iter {
                                                joined = Expr::PathJoin(Box::new(joined), Box::new(next_arg));
                                            }
                                            return Ok(Expr::PathResolve(Box::new(joined)));
                                        }
                                    }
                                    "isAbsolute" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathIsAbsolute(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "relative" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let from = iter.next().unwrap();
                                            let to = iter.next().unwrap();
                                            return Ok(Expr::PathRelative(Box::new(from), Box::new(to)));
                                        }
                                    }
                                    "normalize" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathNormalize(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "parse" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathParse(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "format" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::PathFormat(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from url (e.g., fileURLToPath)
                            if module_name == "url" {
                                match func_name {
                                    "fileURLToPath" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FileURLToPath(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }

                            // Check if this is a named import from fs (e.g., existsSync, mkdirSync, etc.)
                            if module_name == "fs" {
                                match func_name {
                                    "readFileSync" => {
                                        if args.len() >= 2 {
                                            // readFileSync(path, encoding) — returns string
                                            return Ok(Expr::FsReadFileSync(Box::new(args.into_iter().next().unwrap())));
                                        } else if args.len() == 1 {
                                            // readFileSync(path) without encoding — returns Buffer (Node parity)
                                            return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "writeFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsWriteFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "existsSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsExistsSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "mkdirSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsMkdirSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "unlinkSync" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsUnlinkSync(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "appendFileSync" => {
                                        if args.len() >= 2 {
                                            let mut iter = args.into_iter();
                                            let path = iter.next().unwrap();
                                            let content = iter.next().unwrap();
                                            return Ok(Expr::FsAppendFileSync(Box::new(path), Box::new(content)));
                                        }
                                    }
                                    "readFileBuffer" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsReadFileBinary(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    "rmRecursive" => {
                                        if args.len() >= 1 {
                                            return Ok(Expr::FsRmRecursive(Box::new(args.into_iter().next().unwrap())));
                                        }
                                    }
                                    _ => {} // Fall through
                                }
                            }
                        }

                        // Check if this is a direct call on an aliased named import
                        // e.g., uuid() where import { v4 as uuid } from 'uuid'
                        if let Some((module_name, Some(method_name))) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: method_name.to_string(),
                                args,
                            });
                        }

                        // Check if this is a call on a default import from a native module
                        // e.g., Fastify() where import Fastify from 'fastify'
                        if let Some((module_name, None)) = ctx.lookup_native_module(func_name) {
                            return Ok(Expr::NativeMethodCall {
                                module: module_name.to_string(),
                                class_name: None,
                                object: None,
                                method: "default".to_string(), // Use "default" for default export calls
                                args,
                            });
                        }
                    }

                    let callee_expr = lower_expr(ctx, expr)?;

                    // Fill in default arguments if callee is a known function
                    let mut args = args;
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some((defaults, param_ids)) = ctx.lookup_func_defaults(*func_id) {
                            let defaults = defaults.to_vec();
                            let param_ids = param_ids.to_vec();
                            let num_provided = args.len();
                            // Build substitution map: callee param LocalId -> actual arg expression
                            // For provided args, map to the caller's arg expression
                            // For defaulted args, map to the expanded default (built incrementally)
                            let mut param_map: Vec<(LocalId, Expr)> = Vec::new();
                            for i in 0..param_ids.len().min(num_provided) {
                                param_map.push((param_ids[i], args[i].clone()));
                            }
                            // Fill in missing arguments with their defaults, substituting
                            // any parameter references to use the caller's scope
                            for i in num_provided..defaults.len() {
                                if let Some(default_expr) = &defaults[i] {
                                    let substituted = LoweringContext::substitute_param_refs_in_default(
                                        default_expr, &param_map
                                    );
                                    // Add this expanded default to the map so later defaults
                                    // can reference it (e.g., c = b where b was also defaulted)
                                    if i < param_ids.len() {
                                        param_map.push((param_ids[i], substituted.clone()));
                                    }
                                    args.push(substituted);
                                }
                            }
                        }
                    }

                    // If inside a namespace, convert calls to namespace functions into StaticMethodCall
                    if let Expr::FuncRef(func_id) = &callee_expr {
                        if let Some(ref ns_name) = ctx.current_namespace {
                            if let Some(func_name) = ctx.lookup_func_name(*func_id) {
                                if ctx.has_static_method(ns_name, func_name) {
                                    let method_name = func_name.to_string();
                                    let class_name = ns_name.clone();
                                    return Ok(Expr::StaticMethodCall {
                                        class_name,
                                        method_name,
                                        args,
                                    });
                                }
                            }
                        }
                    }

                    let callee = Box::new(callee_expr);
                    // Extract explicit type arguments if present (e.g., identity<number>(x))
                    let type_args = call.type_args.as_ref()
                        .map(|ta| ta.params.iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect())
                        .unwrap_or_default();

                    // Use CallSpread if any argument has spread
                    if let Some(spread_args) = spread_args {
                        Ok(Expr::CallSpread { callee, args: spread_args, type_args })
                    } else {
                        Ok(Expr::Call { callee, args, type_args })
                    }
                }
                ast::Callee::Import(_) => {
                    // Dynamic import: import('module')
                    // Extract the module path from the first argument if available
                    let module_path = if let Some(first_arg) = args.first() {
                        if let Expr::String(s) = first_arg {
                            s.clone()
                        } else {
                            "<dynamic>".to_string()
                        }
                    } else {
                        "<unknown>".to_string()
                    };
                    eprintln!("Warning: Dynamic import('{}') not fully supported, returning undefined", module_path);
                    // Dynamic imports return a Promise that resolves to the module
                    // For now, return undefined as we'd need full runtime support
                    Ok(Expr::Undefined)
                }
            }
}
