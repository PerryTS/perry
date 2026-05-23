//! Native module method calls (process/tty/os/Buffer/Uint8Array/Object/Symbol/Array/net).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use super::super::unimpl_hints;
use super::object_static::build_object_static_method_call;
use super::reflect_args::{take_reflect_ktp_args, take_reflect_kvtp_args, take_reflect_tp_args};
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

pub(super) fn try_native_module_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for native module method calls (e.g., mysql.createConnection())
    if let ast::Expr::Member(member) = expr {
        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
            let obj_name = obj_ident.sym.to_string();

            // Check for process module methods
            if obj_name == "process" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "uptime" => return Ok(Ok(Expr::ProcessUptime)),
                        "cwd" => return Ok(Ok(Expr::ProcessCwd)),
                        "memoryUsage" => return Ok(Ok(Expr::ProcessMemoryUsage)),
                        "nextTick" => {
                            if !args.is_empty() {
                                let mut iter = args.into_iter();
                                let callback = iter.next().unwrap();
                                let trailing: Vec<Expr> = iter.collect();
                                return Ok(Ok(Expr::ProcessNextTick {
                                    callback: Box::new(callback),
                                    args: trailing,
                                }));
                            }
                        }
                        "on" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let event = iter.next().unwrap();
                                let handler = iter.next().unwrap();
                                return Ok(Ok(Expr::ProcessOn {
                                    event: Box::new(event),
                                    handler: Box::new(handler),
                                }));
                            }
                        }
                        "once" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let event = iter.next().unwrap();
                                let handler = iter.next().unwrap();
                                return Ok(Ok(Expr::ProcessOnce {
                                    event: Box::new(event),
                                    handler: Box::new(handler),
                                }));
                            }
                        }
                        "chdir" => {
                            if !args.is_empty() {
                                return Ok(Ok(Expr::ProcessChdir(Box::new(
                                    args.into_iter().next().unwrap(),
                                ))));
                            }
                        }
                        "kill" => {
                            if !args.is_empty() {
                                let mut iter = args.into_iter();
                                let pid = iter.next().unwrap();
                                let signal = iter.next().map(Box::new);
                                return Ok(Ok(Expr::ProcessKill {
                                    pid: Box::new(pid),
                                    signal,
                                }));
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
                            let code = if !args.is_empty() {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            } else {
                                None
                            };
                            return Ok(Ok(Expr::ProcessExit(code)));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for tty module methods (#347 Phase 3)
            let is_tty_module =
                obj_name == "tty" || ctx.lookup_builtin_module_alias(&obj_name) == Some("tty");
            if is_tty_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    if method_ident.sym.as_ref() == "isatty" && !args.is_empty() {
                        let arg = args.into_iter().next().unwrap();
                        return Ok(Ok(Expr::TtyIsAtty(Box::new(arg))));
                    }
                }
            }

            // Check for os module methods FIRST (before generic NativeMethodCall)
            let is_os_module =
                obj_name == "os" || ctx.lookup_builtin_module_alias(&obj_name) == Some("os");
            if is_os_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
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
                        "userInfo" => return Ok(Ok(Expr::OsUserInfo)),
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for Buffer static methods. Issue #831: aliased
            // imports of Buffer (`import { Buffer as RuntimeBuffer } from
            // "node:buffer"`) must route through the same dedicated
            // BufferFrom/BufferAlloc/etc HIR variants as the global
            // `Buffer`; otherwise the lowering falls through to a
            // receiver-less `NativeMethodCall { module: "buffer", method:
            // "from", object: None }` for which the codegen has no
            // dispatch table entry — it would silently return
            // TAG_UNDEFINED, and any caller that subsequently treats the
            // result as a Buffer (e.g. `b[0]` → Uint8ArrayGet) segfaults
            // on the undefined value.
            let is_buffer_ref = obj_name == "Buffer"
                || matches!(
                    ctx.lookup_native_module(&obj_name),
                    Some(("buffer", Some("Buffer")))
                );
            if is_buffer_ref {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "from" => {
                            let data = args.first().cloned().unwrap_or(Expr::Undefined);
                            // Disambiguate `Buffer.from(data, encoding?)` vs
                            // `Buffer.from(arrayBuffer, byteOffset?, length?)`.
                            // Encoding args are strings, byteOffset/length are
                            // numbers. Issue #1273: previously any non-string
                            // literal second arg routed to BufferFromArrayBuffer,
                            // so `Buffer.from(str, encVar)` produced an empty
                            // buffer. Now: 3+ args, or a Number-literal second
                            // arg, or a string-literal first arg with a Number
                            // second arg → ArrayBuffer form. Otherwise default
                            // to BufferFrom (the runtime helper dispatches on
                            // the actual type of `data`, and routes through
                            // `js_encoding_tag_from_value` for runtime-string
                            // encodings).
                            let is_arraybuffer_form =
                                args.len() >= 3 || matches!(args.get(1), Some(Expr::Number(_)));
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
                        "alloc" => {
                            let size = args.first().cloned().unwrap_or(Expr::Number(0.0));
                            let fill = args.get(1).cloned().map(Box::new);
                            let encoding = args.get(2).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferAlloc {
                                size: Box::new(size),
                                fill,
                                encoding,
                            }));
                        }
                        "allocUnsafe" | "allocUnsafeSlow" => {
                            let size = args.first().cloned().unwrap_or(Expr::Number(0.0));
                            return Ok(Ok(Expr::BufferAllocUnsafe(Box::new(size))));
                        }
                        "concat" => {
                            let list = args.first().cloned().unwrap_or(Expr::Array(vec![]));
                            return Ok(Ok(Expr::BufferConcat(Box::new(list))));
                        }
                        "of" => {
                            return Ok(Ok(Expr::BufferFrom {
                                data: Box::new(Expr::Array(args)),
                                encoding: None,
                            }));
                        }
                        "isBuffer" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::BufferIsBuffer(Box::new(obj))));
                        }
                        "isEncoding" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::BufferIsEncoding(Box::new(obj))));
                        }
                        "byteLength" => {
                            let data = args
                                .first()
                                .cloned()
                                .unwrap_or(Expr::String("".to_string()));
                            let encoding = args.get(1).cloned().map(Box::new);
                            return Ok(Ok(Expr::BufferByteLength {
                                data: Box::new(data),
                                encoding,
                            }));
                        }
                        // `Buffer.compare(a, b)` → `a.compare(b)` instance call
                        // (handled by runtime buffer dispatch).
                        "compare" => {
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let a = iter.next().unwrap();
                                let b = iter.next().unwrap();
                                return Ok(Ok(Expr::Call {
                                    callee: Box::new(Expr::PropertyGet {
                                        object: Box::new(a),
                                        property: "compare".to_string(),
                                    }),
                                    args: vec![b],
                                    type_args: vec![],
                                }));
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
                            let data = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(data))));
                        }
                        // Issue #871 (part 2): `Uint8Array.of(a, b, c, ...)` —
                        // uuid's `sha1.js` calls this with 20 args (the SHA-1
                        // hash output, byte by byte), which hit the
                        // `Call callee shape not supported (PropertyGet) with N args`
                        // bail in `crates/perry-codegen/src/lower_call.rs::~3226`
                        // because the receiver `Uint8Array` lowers to `GlobalGet(0)`
                        // (which `lower_expr` evaluates to `0.0`) so the closure-call
                        // fallback at `~3167` skipped past it, and there's no
                        // `js_closure_call17..20` to dispatch ≥17-arg calls anyway.
                        //
                        // Per ECMAScript: `Uint8Array.of(...items)` is `Uint8Array.from([...items])`
                        // — same shape as the existing `from` arm above, just wrap the
                        // varargs in an array literal first. Routes through `Expr::Array`
                        // → `Expr::Uint8ArrayFrom` which already lowers correctly for any
                        // arity (it's just `js_uint8array_from_array`-or-equivalent on the
                        // packed array). Mirrors the `Array.of` arm at :1618.
                        "of" => {
                            return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(Expr::Array(args)))));
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
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectKeys(Box::new(obj))));
                        }
                        "values" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectValues(Box::new(obj))));
                        }
                        "entries" => {
                            let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectEntries(Box::new(obj))));
                        }
                        // Object.assign(target, ...sources) — per ECMAScript spec, this
                        // MUTATES target with each source's own enumerable string-keyed
                        // and Symbol-keyed properties, and RETURNS target (preserving
                        // identity, class_id, and the SYMBOL_PROPERTIES side-table).
                        // Refs #590: the previous lowering folded the call into
                        // ObjectSpread which allocates a fresh object — that breaks
                        // `result === target` and orphans target's symbol-keyed
                        // properties since the side table is keyed by raw pointer.
                        //
                        // Special case: no target at all (`Object.assign()`) is
                        // a TypeError per spec; we coerce to an empty object literal.
                        // Special case: `Object.assign({}, ...)` with a fresh empty
                        // object-literal target — the user is explicitly asking for
                        // a fresh object, so we keep the old ObjectSpread path
                        // (matches `{...src1, ...src2}` semantics, no observable
                        // difference and avoids regressing the no-spread fold below).
                        "assign" => {
                            if args.is_empty() {
                                return Ok(Ok(Expr::Object(Vec::new())));
                            }
                            let mut iter = args.into_iter();
                            let target = iter.next().unwrap();
                            let sources: Vec<Expr> = iter.collect();
                            // `Object.assign({}, ...sources)` — fresh empty object as
                            // target. Preserve the literal-friendly fast paths
                            // (no_spread fold to plain Object, otherwise ObjectSpread)
                            // since there's no pre-existing identity / class_id to
                            // preserve and downstream codegen has more aggressive
                            // inlining for these shapes.
                            if matches!(&target, Expr::Object(props) if props.is_empty()) {
                                let mut parts: Vec<(Option<String>, Expr)> = Vec::new();
                                for arg in &sources {
                                    match arg {
                                        Expr::Object(props) => {
                                            for (key, val) in props {
                                                parts.push((Some(key.clone()), val.clone()));
                                            }
                                        }
                                        _ => {
                                            parts.push((None, arg.clone()));
                                        }
                                    }
                                }
                                let has_spread = parts.iter().any(|(k, _)| k.is_none());
                                if !has_spread {
                                    let static_props: Vec<(String, Expr)> = parts
                                        .into_iter()
                                        .filter_map(|(k, v)| k.map(|key| (key, v)))
                                        .collect();
                                    return Ok(Ok(Expr::Object(static_props)));
                                }
                                return Ok(Ok(Expr::ObjectSpread { parts }));
                            }
                            // Real `Object.assign(target, ...sources)` — mutate target.
                            return Ok(Ok(Expr::ObjectAssign {
                                target: Box::new(target),
                                sources,
                            }));
                        }
                        "fromEntries" => {
                            let entries = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectFromEntries(Box::new(entries))));
                        }
                        "groupBy" => {
                            // Object.groupBy(items, keyFn) — Node 22+ static method
                            if args.len() >= 2 {
                                let mut iter = args.into_iter();
                                let items = iter.next().unwrap();
                                let key_fn = iter.next().unwrap();
                                let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                                return Ok(Ok(Expr::ObjectGroupBy {
                                    items: Box::new(items),
                                    key_fn: Box::new(key_fn),
                                }));
                            }
                        }
                        "is" => {
                            let mut iter = args.into_iter();
                            let a = iter.next().unwrap_or(Expr::Undefined);
                            let b = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectIs(Box::new(a), Box::new(b))));
                        }
                        "hasOwn" => {
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key))));
                        }
                        "freeze" => {
                            return Ok(Ok(Expr::ObjectFreeze(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "seal" => {
                            return Ok(Ok(Expr::ObjectSeal(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "preventExtensions" => {
                            return Ok(Ok(Expr::ObjectPreventExtensions(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "create" => {
                            return Ok(Ok(Expr::ObjectCreate(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "isFrozen" => {
                            return Ok(Ok(Expr::ObjectIsFrozen(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "isSealed" => {
                            return Ok(Ok(Expr::ObjectIsSealed(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "isExtensible" => {
                            return Ok(Ok(Expr::ObjectIsExtensible(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "getPrototypeOf" => {
                            return Ok(Ok(Expr::ObjectGetPrototypeOf(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "setPrototypeOf" => {
                            // `Object.setPrototypeOf(obj, proto)` is the foundation
                            // of chalk's "callable + getter-bag" shape (a closure has
                            // its `[[Prototype]]` reset to a Function-derived
                            // accessor-bag). Pre-fix this fell through to a generic
                            // `Object.setPrototypeOf` PropertyGet → Call where
                            // `Object.setPrototypeOf` resolves to undefined and the
                            // call throws `TypeError: value is not a function` —
                            // chalk's `import chalk from "chalk"` died at module init.
                            //
                            // Perry's runtime doesn't track mutable per-instance
                            // prototype chains (class IDs are baked at allocation),
                            // so we model setPrototypeOf as a no-op that still
                            // returns the target — matching the spec's "return obj"
                            // contract. The runtime helper registers (obj, proto)
                            // in a side-table that `Object.getPrototypeOf(obj)` is
                            // free to consult later if a downstream pattern needs it.
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let proto = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectSetPrototypeOf(
                                Box::new(obj),
                                Box::new(proto),
                            )));
                        }
                        "defineProperty" => {
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            let descriptor = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectDefineProperty(
                                Box::new(obj),
                                Box::new(key),
                                Box::new(descriptor),
                            )));
                        }
                        "defineProperties" => {
                            // `Object.defineProperties(target, descriptors)` — bulk
                            // form of `defineProperty`. Used by chalk's index.js to
                            // attach the `styles` getter-bag onto
                            // `createChalk.prototype`. Pre-fix this fell through to a
                            // generic `(Object).defineProperties(...)` call which
                            // throws `TypeError: value is not a function` at module
                            // init because `Object` isn't a runtime object with
                            // method dispatch.
                            //
                            // Desugar to a sequence of `ObjectDefineProperty`
                            // applications by reading `descriptors`'s own keys at
                            // compile time when it's an object literal, otherwise
                            // route through a runtime helper that iterates the
                            // descriptor object's keys.
                            let mut iter = args.into_iter();
                            let target = iter.next().unwrap_or(Expr::Undefined);
                            let descs = iter.next().unwrap_or(Expr::Undefined);
                            if let Expr::Object(props) = &descs {
                                // Static descriptor literal — desugar to a Sequence
                                // of `defineProperty(target, key, desc)` calls and
                                // yield `target` as the result value.
                                let target = target;
                                let mut exprs: Vec<Expr> = Vec::with_capacity(props.len() + 1);
                                for (key_name, desc_expr) in props {
                                    exprs.push(Expr::ObjectDefineProperty(
                                        Box::new(target.clone()),
                                        Box::new(Expr::String(key_name.clone())),
                                        Box::new(desc_expr.clone()),
                                    ));
                                }
                                exprs.push(target);
                                if exprs.len() == 1 {
                                    return Ok(Ok(exprs.into_iter().next().unwrap()));
                                }
                                return Ok(Ok(Expr::Sequence(exprs)));
                            }
                            return Ok(Ok(Expr::ObjectDefineProperties(
                                Box::new(target),
                                Box::new(descs),
                            )));
                        }
                        "getOwnPropertyDescriptor" => {
                            let mut iter = args.into_iter();
                            let obj = iter.next().unwrap_or(Expr::Undefined);
                            let key = iter.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectGetOwnPropertyDescriptor(
                                Box::new(obj),
                                Box::new(key),
                            )));
                        }
                        "getOwnPropertyNames" => {
                            return Ok(Ok(Expr::ObjectGetOwnPropertyNames(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
                        }
                        "getOwnPropertySymbols" => {
                            return Ok(Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(
                                args.into_iter().next().unwrap_or(Expr::Undefined),
                            ))));
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
                            return Ok(Ok(Expr::SymbolFor(Box::new(key))));
                        }
                        "keyFor" => {
                            let sym = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::SymbolKeyFor(Box::new(sym))));
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
                            return Ok(Ok(Expr::ReflectGet {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "set" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            let value = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectSet {
                                target: Box::new(target),
                                key: Box::new(key),
                                value: Box::new(value),
                            }));
                        }
                        "has" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectHas {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "deleteProperty" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectDelete {
                                target: Box::new(target),
                                key: Box::new(key),
                            }));
                        }
                        "ownKeys" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectOwnKeys(Box::new(target))));
                        }
                        "apply" => {
                            let mut it = args.into_iter();
                            let func = it.next().unwrap_or(Expr::Undefined);
                            let this_arg = it.next().unwrap_or(Expr::Undefined);
                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                            return Ok(Ok(Expr::ReflectApply {
                                func: Box::new(func),
                                this_arg: Box::new(this_arg),
                                args: Box::new(args_arr),
                            }));
                        }
                        "construct" => {
                            // Special case: `Reflect.construct(ClassName, [args...])`
                            // where ClassName is a known class — fold to a direct
                            // `new ClassName(...args)` expression.
                            if call.args.len() >= 2 {
                                if let ast::Expr::Ident(cls_ident) = call.args[0].expr.as_ref() {
                                    let cls_name = cls_ident.sym.to_string();
                                    if ctx.lookup_class(&cls_name).is_some() {
                                        if let ast::Expr::Array(arr_lit) =
                                            call.args[1].expr.as_ref()
                                        {
                                            let new_args: Vec<Expr> = arr_lit
                                                .elems
                                                .iter()
                                                .filter_map(|e| e.as_ref())
                                                .map(|e| lower_expr(ctx, &e.expr))
                                                .collect::<Result<Vec<_>>>()?;
                                            return Ok(Ok(Expr::New {
                                                class_name: cls_name,
                                                args: new_args,
                                                type_args: vec![],
                                            }));
                                        }
                                    }
                                }
                            }
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                            return Ok(Ok(Expr::ReflectConstruct {
                                target: Box::new(target),
                                args: Box::new(args_arr),
                            }));
                        }
                        "defineProperty" => {
                            let mut it = args.into_iter();
                            let target = it.next().unwrap_or(Expr::Undefined);
                            let key = it.next().unwrap_or(Expr::Undefined);
                            let descriptor = it.next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectDefineProperty {
                                target: Box::new(target),
                                key: Box::new(key),
                                descriptor: Box::new(descriptor),
                            }));
                        }
                        "getPrototypeOf" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ReflectGetPrototypeOf(Box::new(target))));
                        }
                        "defineMetadata" => {
                            let (key, value, target, property_key) = take_reflect_kvtp_args(args);
                            return Ok(Ok(Expr::ReflectDefineMetadata {
                                key: Box::new(key),
                                value: Box::new(value),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectGetMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getOwnMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectGetOwnMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "hasMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectHasMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "hasOwnMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectHasOwnMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getMetadataKeys" => {
                            let (target, property_key) = take_reflect_tp_args(args);
                            return Ok(Ok(Expr::ReflectGetMetadataKeys {
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "getOwnMetadataKeys" => {
                            let (target, property_key) = take_reflect_tp_args(args);
                            return Ok(Ok(Expr::ReflectGetOwnMetadataKeys {
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "deleteMetadata" => {
                            let (key, target, property_key) = take_reflect_ktp_args(args);
                            return Ok(Ok(Expr::ReflectDeleteMetadata {
                                key: Box::new(key),
                                target: Box::new(target),
                                property_key,
                            }));
                        }
                        "setPrototypeOf" => return Ok(Ok(Expr::Bool(true))),
                        "isExtensible" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectIsExtensible(Box::new(target))));
                        }
                        "preventExtensions" => {
                            let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ObjectPreventExtensions(Box::new(target))));
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
                        return Ok(Ok(Expr::ProxyRevocable {
                            target: Box::new(target),
                            handler: Box::new(handler),
                        }));
                    }
                }
            }

            // Check for Array static methods
            if obj_name == "Array" {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "isArray" => {
                            let value = args.first().cloned().unwrap_or(Expr::Undefined);
                            return Ok(Ok(Expr::ArrayIsArray(Box::new(value))));
                        }
                        "from" => {
                            let value = args.first().cloned().unwrap_or(Expr::Undefined);
                            // `Array.from(iterable, mapFn)` uses a dedicated HIR
                            // variant so codegen can handle Map/Set/Array sources
                            // uniformly (materialize + js_array_map).
                            if let Some(map_fn) = args.get(1).cloned() {
                                return Ok(Ok(Expr::ArrayFromMapped {
                                    iterable: Box::new(value),
                                    map_fn: Box::new(map_fn),
                                }));
                            }
                            // Check if the source is a generator call — use iterator protocol
                            let is_gen = is_generator_call_expr(ctx, &value);
                            if is_gen {
                                return Ok(Ok(Expr::IteratorToArray(Box::new(value))));
                            }
                            return Ok(Ok(Expr::ArrayFrom(Box::new(value))));
                        }
                        "of" => {
                            // Array.of(1,2,3) is equivalent to [1,2,3]
                            return Ok(Ok(Expr::Array(args)));
                        }
                        _ => {} // Fall through to generic handling
                    }
                }
            }

            // Check for net module methods
            let is_net_module =
                obj_name == "net" || ctx.lookup_builtin_module_alias(&obj_name) == Some("net");
            if is_net_module {
                if let ast::MemberProp::Ident(method_ident) = &member.prop {
                    let method_name = method_ident.sym.as_ref();
                    match method_name {
                        "createServer" => {
                            let options = args.first().cloned().map(Box::new);
                            let connection_listener = args.get(1).cloned().map(Box::new);
                            return Ok(Ok(Expr::NetCreateServer {
                                options,
                                connection_listener,
                            }));
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
                let is_handled_module = module_name == "path"
                    || module_name == "node:path"
                    || module_name == "fs"
                    || module_name == "node:fs"
                    || module_name == "child_process"
                    || module_name == "node:child_process"
                    || module_name == "crypto"
                    || module_name == "node:crypto"
                    || module_name == "os"
                    || module_name == "node:os";
                if !is_handled_module {
                    // This is a call on a native module (e.g., mysql.createConnection)
                    if let ast::MemberProp::Ident(method_ident) = &member.prop {
                        let method_name = method_ident.sym.to_string();
                        // Unimplemented-API gate (#463 / #525) for the 2-deep
                        // `mod.method()` call form. Without this, perry/* and
                        // other native-module call sites short-circuited past
                        // the `lower_member` gate that fires for the property-
                        // read form, then bailed in codegen with a per-module
                        // message (`'X' is not a known function`) — different
                        // wording, different escape hatch, harder for users to
                        // recognize as the same class of mistake. Mirrors the
                        // 3-deep gate above for `mod.X.Y()`.
                        let allow_unimplemented =
                            std::env::var_os("PERRY_ALLOW_UNIMPLEMENTED").is_some();
                        if !allow_unimplemented
                            && perry_api_manifest::module_has_any_entries(module_name)
                            && perry_api_manifest::module_has_symbol(module_name, &method_name)
                                .is_none()
                        {
                            // #925: this is the gate that fires
                            // for `crypto.hmacSha256(data, key)`.
                            let hint = super::super::unimpl_hints::module_member_hint(
                                module_name,
                                &method_name,
                            )
                            .map(|h| format!(" {h}"))
                            .unwrap_or_default();
                            crate::lower_bail!(
                                member.span,
                                "`{}.{}` is not implemented in Perry — see `perry --print-api-manifest` for the supported surface, \
                                 or set `PERRY_ALLOW_UNIMPLEMENTED=1` to ignore. (#463){}",
                                module_name,
                                method_name,
                                hint,
                            );
                        }
                        return Ok(Ok(Expr::NativeMethodCall {
                            module: module_name.to_string(),
                            class_name: None, // Will be set by js_transform if needed
                            object: None,     // Static call on module itself
                            method: method_name,
                            args,
                        }));
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
