//! GlobalThisExpr..WeakRefNew.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

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
            // arm has to disambiguate. `refine_type_from_init` returns
            // a HirType for a small set of literal / built-in shapes;
            // `Number` / `Integer` route to `js_number_to_locale_string`
            // (formats with thousands separators). Anything else falls
            // through to `js_date_to_locale_string`.
            let v = lower_expr(ctx, d)?;
            let inferred = crate::type_analysis::refine_type_from_init(ctx, d);
            let rt_fn = match inferred {
                Some(perry_types::Type::Number) | Some(perry_types::Type::Int32) => {
                    "js_number_to_locale_string"
                }
                _ => "js_date_to_locale_string",
            };
            let blk = ctx.block();
            let handle = blk.call(I64, rt_fn, &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
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
            let options_i64 = if let Some(opts_expr) = options {
                let opts_box = lower_expr(ctx, opts_expr)?;
                let blk = ctx.block();
                unbox_to_i64(blk, &opts_box)
            } else {
                "0".to_string()
            };
            let listener_i64 = if let Some(cb_expr) = connection_listener {
                let cb_box = lower_expr(ctx, cb_expr)?;
                let blk = ctx.block();
                unbox_to_i64(blk, &cb_box)
            } else {
                "0".to_string()
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
                        "utf8" | "utf-8" | "ascii" | "latin1" | "binary" => 0,
                        "hex" => 1,
                        "base64" | "base64url" => 2,
                        _ => bail!(
                            "perry-codegen: unknown Buffer encoding \"{}\": expected one of utf8, utf-8, hex, base64, base64url, ascii, latin1, binary",
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
            let size_i32 = blk.fptosi(DOUBLE, &size_box, I32);
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

        Expr::BufferAlloc { size, fill } => {
            // Phase H: call js_buffer_alloc(size, fill) which returns
            // a raw *mut BufferHeader i64. NaN-box with POINTER_TAG
            // so downstream BUFFER_REGISTRY checks + `.length` paths
            // can use it. Missing fill defaults to 0.
            let size_box = lower_expr(ctx, size)?;
            let fill_i32 = if let Some(fill_expr) = fill {
                let fill_box = lower_expr(ctx, fill_expr)?;
                ctx.block().fptosi(DOUBLE, &fill_box, I32)
            } else {
                "0".to_string()
            };
            let blk = ctx.block();
            let size_i32 = blk.fptosi(DOUBLE, &size_box, I32);
            let buf_handle = blk.call(
                I64,
                "js_buffer_alloc",
                &[(I32, &size_i32), (I32, &fill_i32)],
            );
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

        // -------- structuredClone(v) — real deep copy --------
        Expr::StructuredClone(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_structured_clone", &[(DOUBLE, &v)]))
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
