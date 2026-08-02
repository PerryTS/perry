//! `Buffer` / `Uint8Array` static-method calls.
//!
//! Extracted from `expr_call/native_module.rs` as a mechanical move; the
//! block runs in the same position it occupied inline, and `Ok(Err(args))`
//! means "not one of these statics — keep checking the later receivers".

use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

pub(super) fn try_buffer_uint8array_statics(
    ctx: &LoweringContext,
    member: &ast::MemberExpr,
    obj_name: &str,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
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
            ctx.lookup_native_module(obj_name),
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
                "alloc" => {
                    // #2013: a missing `size` must surface Node's
                    // `ERR_INVALID_ARG_TYPE` (Received undefined), so
                    // default to `undefined` — not `0` — and let the
                    // runtime validator throw.
                    let size = args.first().cloned().unwrap_or(Expr::Undefined);
                    let fill = args.get(1).cloned().map(Box::new);
                    let encoding = args.get(2).cloned().map(Box::new);
                    return Ok(Ok(Expr::BufferAlloc {
                        size: Box::new(size),
                        fill,
                        encoding,
                    }));
                }
                "allocUnsafe" | "allocUnsafeSlow" => {
                    // #2013: missing `size` → Node ERR_INVALID_ARG_TYPE.
                    let size = args.first().cloned().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::BufferAllocUnsafe(Box::new(size))));
                }
                "concat" => {
                    let list = args.first().cloned().unwrap_or(Expr::Array(vec![]));
                    if let Some(total_length) = args.get(1).cloned() {
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

    // Check for Uint8Array static methods
    if obj_name == "Uint8Array" {
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            match method_name {
                "from" => {
                    let data = args.first().cloned().unwrap_or(Expr::Undefined);
                    // #2774: `Uint8Array.from(src, mapFn, thisArg?)` — run
                    // the mapped materialization first (which validates
                    // mapFn + binds thisArg), then truncate to Uint8.
                    if let Some(map_fn) = args.get(1).cloned() {
                        let this_arg = args.get(2).cloned().map(Box::new);
                        return Ok(Ok(Expr::Uint8ArrayFrom(Box::new(Expr::ArrayFromMapped {
                            iterable: Box::new(data),
                            map_fn: Box::new(map_fn),
                            this_arg,
                        }))));
                    }
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

    Ok(Err(args))
}
