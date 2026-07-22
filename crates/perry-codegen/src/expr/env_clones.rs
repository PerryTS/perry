//! GlobalThisExpr..WeakRefNew.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::GlobalThisExpr => {
            // `Function('return this')()` (and any other AST shape we
            // recognise as "get the global this") materialises here as
            // the runtime's lazily-allocated `globalThis` singleton —
            // same object that `globalThis[...]= v` writes target via
            // the IndexSet arm above. Returns an already-NaN-boxed
            // f64 POINTER_TAG; the property-get arms route through
            // this same singleton for `globalThis.process.env` etc.
            Ok(ctx.block().call(DOUBLE, "js_get_global_this", &[]))
        }
        Expr::ModuleTopThis => {
            // CJS-style module top-level `this`: a lazily-allocated plain
            // object (the module's `exports` stand-in), NOT `globalThis`.
            Ok(ctx.block().call(DOUBLE, "js_module_top_this", &[]))
        }
        Expr::DateToISOString(d) => {
            let v = lower_expr(ctx, d)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_date_to_iso_string_or_throw", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        // #600: `(12345).toLocaleString()` and `date.toLocaleString()`
        // both lower to the misnamed `Expr::DateToLocaleString`. Route
        // statically-Number receivers to `js_number_to_locale_string`
        // (formats with thousands separators); everything else to
        // `js_date_to_locale_string`. The LLVM backend had no arm at
        // all pre-fix — Phase 2 raised
        // "expression DateToLocaleString not yet supported". The
        // fall-through path matches what the JS / WASM backends emit.
        Expr::DateToLocaleString(d) => {
            // Pick the runtime helper based on the receiver's static
            // type. The HIR variant is misnamed — it covers BOTH
            // `(12345).toLocaleString()` (number) and
            // `new Date(...).toLocaleString()` (date) — so the LLVM
            // arm has to disambiguate.
            //
            // #3917: a `LocalGet` whose declared type is `number` was
            // falling through to `js_date_to_locale_string` and printing
            // a 1970-epoch date string. `refine_type_from_init` only
            // inspects AST shapes (literals, arithmetic, `new T(...)`)
            // and has no `LocalGet` arm, so it returned `None` for
            // `const num: number = 20; num.toLocaleString(...)`. Fall
            // back to `static_type_of`, which DOES read `local_types`,
            // when the structural refinement comes up empty.
            let v = lower_expr(ctx, d)?;
            let inferred = crate::type_analysis::refine_type_from_init(ctx, d)
                .or_else(|| crate::type_analysis::static_type_of(ctx, d));
            match inferred {
                Some(perry_hir::types::Type::Number) | Some(perry_hir::types::Type::Int32) => {
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_number_to_locale_string", &[(DOUBLE, &v)]);
                    Ok(nanbox_string_inline(blk, &handle))
                }
                // #4546: a plain object / string / boolean receiver was
                // mis-routed to `js_date_to_locale_string`, printing a
                // 1970-epoch "Invalid Date" instead of `[object Object]`
                // (or a custom `toLocaleString`). Dispatch on the value's
                // runtime tag instead; the helper returns an already
                // NaN-boxed value, so do NOT re-box it.
                _ => {
                    let blk = ctx.block();
                    Ok(blk.call(DOUBLE, "js_value_to_locale_string", &[(DOUBLE, &v)]))
                }
            }
        }
        // #600: `fetchWithAuth(url, "Bearer ...")` — perry's recognized
        // built-in for authenticated GET. Dispatches to perry-stdlib's
        // `js_fetch_get_with_auth(url_ptr, auth_header_ptr) -> *mut Promise`
        // and NaN-boxes the returned promise pointer with POINTER_TAG.
        // Both args are unboxed to raw `*const StringHeader`.
        Expr::FetchGetWithAuth { url, auth_header } => {
            let url_box = lower_expr(ctx, url)?;
            let auth_box = lower_expr(ctx, auth_header)?;
            let blk = ctx.block();
            let url_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &url_box)]);
            let auth_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &auth_box)]);
            let promise = blk.call(
                I64,
                "js_fetch_get_with_auth",
                &[(I64, &url_ptr), (I64, &auth_ptr)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        // #600: `fetchPostWithAuth(url, "Bearer ...", body)` — three-arg
        // POST form. Body is pre-stringified by the caller (matches the
        // existing perry-stdlib impl's signature).
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            let url_box = lower_expr(ctx, url)?;
            let auth_box = lower_expr(ctx, auth_header)?;
            let body_box = lower_expr(ctx, body)?;
            let blk = ctx.block();
            let url_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &url_box)]);
            let auth_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &auth_box)]);
            let body_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &body_box)]);
            let promise = blk.call(
                I64,
                "js_fetch_post_with_auth",
                &[(I64, &url_ptr), (I64, &auth_ptr), (I64, &body_ptr)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }

        // Issue #1123 — `net.createServer(handler?)` / `net.createServer(opts,
        // handler?)`. HIR lowering at `crates/perry-hir/src/lower/expr_call.rs`
        // produces `Expr::NetCreateServer { options, connection_listener }`
        // for the dotted (`import * as net from "node:net"`) form; the
        // LLVM backend previously had no arm here and dropped through to
        // the `"expression NetCreateServer not yet supported"` Phase-2
        // catch-all. The runtime symbol lives at
        // `crates/perry-ext-net/src/lib.rs::js_net_create_server`
        // (perry-runtime/src/net.rs is gated off at lib.rs:79 since
        // A1/A1.5; net.Socket moved to perry-stdlib/perry-ext-net's
        // event-driven model, and this fix adds the missing createServer
        // entry on the same side). It's declared at `runtime_decls.rs:2690`
        // as `(I64, I64) -> DOUBLE`. The first slot is the options object
        // pointer (or `0` for omitted — the runtime tolerates a null
        // options ptr); the second slot is the connection-listener
        // closure pointer (or `0` for omitted, same tolerance). Closures
        // arrive NaN-boxed with POINTER_TAG; strip the tag with
        // `unbox_to_i64` before handing to the FFI signature. Options
        // here are an optional plain object — pass the value through
        // after stripping the NaN-box tag so the runtime sees the raw
        // `*ObjectHeader`. The returned `f64` is a raw handle (positive
        // small integer) that subsequent server-side ops would consume;
        // no extra NaN-boxing required at this layer (callers store the
        // value through the JSValue F64 slot, matching the historic
        // contract carried over from the deprecated runtime entry).
        Expr::NetCreateServer {
            options,
            connection_listener,
        } => {
            // Lower each arg at most once (avoid double-evaluating a side-
            // effecting argument expression), then derive the i64 forms.
            let options_box = match options {
                Some(opts_expr) => Some(lower_expr(ctx, opts_expr)?),
                None => None,
            };
            let listener_box = match connection_listener {
                Some(cb_expr) => Some(lower_expr(ctx, cb_expr)?),
                None => None,
            };
            // #2013: validate the first positional argument — `options` when
            // present (the 2-arg form), otherwise the single arg (which the
            // HIR routed to `connection_listener`). It must be a function or
            // object; a number/boolean/string throws ERR_INVALID_ARG_TYPE.
            if let Some(first) = options_box.as_ref().or(listener_box.as_ref()) {
                let blk = ctx.block();
                blk.call_void("js_net_validate_create_server_options", &[(DOUBLE, first)]);
            }
            let options_i64 = match &options_box {
                Some(opts_box) => {
                    let blk = ctx.block();
                    unbox_to_i64(blk, opts_box)
                }
                None => "0".to_string(),
            };
            let listener_i64 = match &listener_box {
                Some(cb_box) => {
                    let blk = ctx.block();
                    unbox_to_i64(blk, cb_box)
                }
                None => "0".to_string(),
            };
            // Issue #1123 followup — call returns the raw handle as `i64`
            // (runtime_decls.rs declares `(I64, I64) -> I64`); NaN-box
            // with POINTER_TAG so `unbox_to_i64` on the receiver in
            // `server.listen(...)` round-trips correctly. This matches
            // the `js_node_http_create_server` → `nanbox_pointer_inline`
            // pattern in lower_native_module_dispatch's NR_PTR arm; we
            // can't go through that arm because the dotted/named-import
            // forms both lower to `Expr::NetCreateServer` (not a
            // NativeMethodCall against the table).
            let blk = ctx.block();
            let raw = blk.call(
                I64,
                "js_net_create_server",
                &[(I64, &options_i64), (I64, &listener_i64)],
            );
            Ok(nanbox_pointer_inline(blk, &raw))
        }
        Expr::DateParse(s) => {
            let s_box = lower_expr(ctx, s)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            Ok(blk.call(DOUBLE, "js_date_parse", &[(I64, &s_handle)]))
        }
        Expr::ProcessVersions => {
            // Runtime returns already NaN-boxed pointer.
            Ok(ctx.block().call(DOUBLE, "js_process_versions", &[]))
        }
        Expr::ProcessUptime => Ok(ctx.block().call(DOUBLE, "js_process_uptime", &[])),
        Expr::ProcessCwd => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_process_cwd", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsEOL => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_eol", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::BufferFrom { data, encoding } => {
            // `Buffer.from(value, encoding?)` accepts strings, arrays of
            // numbers, or other buffers. Route through `js_buffer_from_value`
            // which dispatches on the input type at runtime — strings via
            // `js_buffer_from_string`, arrays via `js_buffer_from_array`,
            // existing buffers via copy. The result is a raw `*mut
            // BufferHeader` registered in BUFFER_REGISTRY; NaN-box with
            // POINTER_TAG so chained `.toString(enc)` / `.length` /
            // method dispatch see the same registered pointer.
            //
            // The encoding argument is a JS string ('utf8'/'hex'/'base64').
            // Compile-time fold string literals; for non-literal encoding
            // values call the runtime helper `js_encoding_tag_from_value`.
            let data_box = lower_expr(ctx, data)?;
            let enc_tag_i32 = if let Some(enc_expr) = encoding {
                if let Expr::String(s) = enc_expr.as_ref() {
                    let lower = s.to_ascii_lowercase();
                    let tag: i32 = match lower.as_str() {
                        "utf8" | "utf-8" => 0,
                        "hex" => 1,
                        "base64" => 2,
                        "base64url" => 3,
                        "latin1" | "binary" => 4,
                        "ascii" => 5,
                        "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => 6,
                        _ => bail!(
                            "perry-codegen: unknown Buffer encoding \"{}\": expected one of utf8, utf-8, hex, base64, base64url, ascii, latin1, binary, utf16le, utf-16le, ucs2, ucs-2",
                            s
                        ),
                    };
                    tag.to_string()
                } else {
                    let enc_box = lower_expr(ctx, enc_expr)?;
                    let blk = ctx.block();
                    blk.call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &enc_box)])
                }
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            // Pass the NaN-boxed value as i64 — `js_buffer_from_value`
            // sniffs string vs array vs buffer at runtime by inspecting tags.
            let value_i64 = blk.bitcast_double_to_i64(&data_box);
            let buf_handle = blk.call(
                I64,
                "js_buffer_from_value",
                &[(I64, &value_i64), (I32, &enc_tag_i32)],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => {
            let data_box = lower_expr(ctx, data)?;
            let offset_box = lower_expr(ctx, byte_offset)?;
            let len_i32 = if let Some(len_expr) = length {
                let len_box = lower_expr(ctx, len_expr)?;
                ctx.block().fptosi(DOUBLE, &len_box, I32)
            } else {
                "-1".to_string()
            };
            let blk = ctx.block();
            let data_i64 = blk.bitcast_double_to_i64(&data_box);
            let offset_i32 = blk.fptosi(DOUBLE, &offset_box, I32);
            let buf_handle = blk.call(
                I64,
                "js_buffer_from_arraybuffer_slice",
                &[(I64, &data_i64), (I32, &offset_i32), (I32, &len_i32)],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }
        // Issue #630: `Buffer.allocUnsafe(size)` — fast-path allocator
        // (no zero-fill). The runtime helper returns a raw `*mut
        // BufferHeader`; NaN-box with POINTER_TAG so downstream
        // BUFFER_REGISTRY checks + `.length` paths recognize it as a
        // buffer.
        Expr::BufferAllocUnsafe(size) => {
            let size_box = lower_expr(ctx, size)?;
            let blk = ctx.block();
            // #2013: validate `size` (number, in [0, kMaxLength]) and recover
            // the truncated i32 in one runtime call instead of a bare fptosi.
            let size_i32 = blk.call(I32, "js_buffer_validate_size", &[(DOUBLE, &size_box)]);
            let buf_handle = blk.call(I64, "js_buffer_alloc_unsafe", &[(I32, &size_i32)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }
        // Issue #630: `Buffer.byteLength(s, encoding?)` — UTF-8 byte
        // count of `s`. The runtime helper currently honors UTF-8 only
        // (matches the existing `BufferHeader.byte_len` field semantics);
        // a `encoding === "hex"` / `"base64"` arg would need a separate
        // helper but those aren't in the issue's repro. Returns i32;
        // sitofp to f64 for the Number return.
        Expr::BufferByteLength { data, encoding } => {
            let data_box = lower_expr(ctx, data)?;
            let enc_box = if let Some(enc) = encoding {
                lower_expr(ctx, enc)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let len_i32 = blk.call(
                I32,
                "js_buffer_byte_length_value",
                &[(DOUBLE, &data_box), (DOUBLE, &enc_box)],
            );
            Ok(blk.sitofp(I32, &len_i32, DOUBLE))
        }

        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => {
            // Phase H: call js_buffer_alloc(size, fill) which returns
            // a raw *mut BufferHeader i64. NaN-box with POINTER_TAG
            // so downstream BUFFER_REGISTRY checks + `.length` paths
            // can use it. Missing fill defaults to 0.
            let size_box = lower_expr(ctx, size)?;
            // #2013: validate `size` (number, in [0, kMaxLength]) and recover
            // the truncated i32 in one runtime call instead of a bare fptosi.
            let size_i32 = ctx
                .block()
                .call(I32, "js_buffer_validate_size", &[(DOUBLE, &size_box)]);
            let buf_handle = if let Some(fill_expr) = fill {
                let fill_box = lower_expr(ctx, fill_expr)?;
                let enc_tag_i32 = if let Some(enc_expr) = encoding {
                    if let Expr::String(s) = enc_expr.as_ref() {
                        let lower = s.to_ascii_lowercase();
                        let tag: i32 = match lower.as_str() {
                            "utf8" | "utf-8" => 0,
                            "hex" => 1,
                            "base64" => 2,
                            "base64url" => 3,
                            "latin1" | "binary" => 4,
                            "ascii" => 5,
                            "utf16le" | "utf-16le" | "ucs2" | "ucs-2" => 6,
                            _ => bail!(
                                "perry-codegen: unknown Buffer encoding \"{}\": expected one of utf8, utf-8, hex, base64, base64url, ascii, latin1, binary, utf16le, utf-16le, ucs2, ucs-2",
                                s
                            ),
                        };
                        tag.to_string()
                    } else {
                        let enc_box = lower_expr(ctx, enc_expr)?;
                        ctx.block()
                            .call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &enc_box)])
                    }
                } else {
                    "0".to_string()
                };
                ctx.block().call(
                    I64,
                    "js_buffer_alloc_fill_value",
                    &[(I32, &size_i32), (DOUBLE, &fill_box), (I32, &enc_tag_i32)],
                )
            } else {
                ctx.block()
                    .call(I64, "js_buffer_alloc", &[(I32, &size_i32), (I32, "0")])
            };
            let blk = ctx.block();
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // -------- process.pid / process.ppid — raw f64 number --------
        Expr::ProcessPid => Ok(ctx.block().call(DOUBLE, "js_process_pid", &[])),
        Expr::ProcessPpid => Ok(ctx.block().call(DOUBLE, "js_process_ppid", &[])),
        Expr::ProcessArgv => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_process_argv", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }

        // -------- structuredClone(v[, options]) — real deep copy --------
        Expr::StructuredClone { value, options } => {
            let v = lower_expr(ctx, value)?;
            let opts = lower_expr(ctx, options)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_structured_clone_with_options",
                &[(DOUBLE, &v), (DOUBLE, &opts)],
            ))
        }

        // -------- `new WeakRef(target)` — allocate a wrapper object --------
        Expr::WeakRefNew(operand) => {
            // Runtime strongly holds the target in a `target` field, so
            // `deref()` always returns it. Pass the NaN-boxed target through;
            // the runtime reads the bits directly. Result is a raw
            // *mut ObjectHeader (i64) — re-NaN-box with POINTER_TAG.
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let obj = blk.call(I64, "js_weakref_new", &[(DOUBLE, &v)]);
            Ok(nanbox_pointer_inline(blk, &obj))
        }

        // -------- fs.unlinkSync(path) --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
