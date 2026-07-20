//! StringFromCodePoint..OsMachine (string/regex/process arms).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
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
    lower_index_set_fast, lower_js_args_array, lower_math_operand, lower_object_literal,
    lower_stream_super_init, lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::SetClear(s) => {
            let s_box = lower_expr(ctx, s)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            blk.call_void("js_set_clear", &[(I64, &s_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- String.fromCodePoint(cp) — returns single-char string --------
        Expr::StringFromCodePoint(o) => {
            // #2788: pass the raw NaN-boxed value; the runtime validates the
            // code point and throws RangeError for negative/non-integer/>0x10FFFF.
            // A prior fptosi truncated `3.14` to `3` and hid the error.
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_from_code_point", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        // -------- Callable String.raw(callSite, ...substitutions) (#2789) --------
        Expr::StringRaw {
            call_site,
            substitutions,
        } => {
            // callSite as a NaN-boxed value; substitutions collected into a
            // NaN-boxed array. The runtime reads `callSite.raw` (array-like),
            // interleaves the substitutions, and throws TypeError on nullish
            // callSite / raw.
            let cs = lower_expr(ctx, call_site)?;
            let subs_arr = lower_array_literal(ctx, substitutions)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_raw", &[(DOUBLE, &cs), (DOUBLE, &subs_arr)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        // -------- str.at(i) — returns single-char string or undefined --------
        Expr::StringAt { string, index } => {
            let s_box = lower_expr(ctx, string)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            // Runtime returns NaN-boxed f64 directly (string or undefined).
            Ok(blk.call(DOUBLE, "js_string_at", &[(I64, &s_handle), (I32, &idx_i32)]))
        }
        Expr::StringCodePointAt { string, index } => {
            let s_box = lower_expr(ctx, string)?;
            let idx_d = lower_expr(ctx, index)?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            let idx_i32 = blk.fptosi(DOUBLE, &idx_d, I32);
            Ok(blk.call(
                DOUBLE,
                "js_string_code_point_at",
                &[(I64, &s_handle), (I32, &idx_i32)],
            ))
        }
        Expr::RegExpSource(o) => {
            let r_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            let s_handle = blk.call(I64, "js_regexp_get_source", &[(I64, &r_handle)]);
            Ok(nanbox_string_inline(blk, &s_handle))
        }
        Expr::RegExpFlags(o) => {
            let r_box = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            let s_handle = blk.call(I64, "js_regexp_get_flags", &[(I64, &r_handle)]);
            Ok(nanbox_string_inline(blk, &s_handle))
        }
        Expr::ProcessChdir(p) => {
            // #2013 — route through the f64-taking entry so a
            // non-string argument throws TypeError ERR_INVALID_ARG_TYPE
            // instead of dereferencing whatever NaN-boxed bits we'd
            // have shoved into a `*const StringHeader`. The runtime
            // entry validates and re-dispatches to the legacy
            // string-only path.
            let p_box = lower_expr(ctx, p)?;
            ctx.block()
                .call_void("js_process_chdir_jsv", &[(DOUBLE, &p_box)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ProcessExit(code) => {
            // `process.exit(code?)` terminates immediately. Before the
            // explicit lowering it fell through to generic NativeMethodCall
            // which silently no-op'd — scripts whose tail was
            // `main().then(() => process.exit(0))` would see the callback
            // fire, fail to exit, and hang in the event loop with any
            // live net.Socket keeping `js_stdlib_has_active_handles`
            // non-zero. The runtime fn calls `_exit(code as i32)`.
            let code_val = if let Some(e) = code {
                lower_expr(ctx, e)?
            } else {
                // #6666: bare `process.exit()` passes `undefined` (not `0`) so
                // the runtime falls back to the stored `process.exitCode` —
                // matching Node, where `process.exit()` honours a previously
                // set `process.exitCode`. An explicit `process.exit(0)` still
                // forces 0.
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            ctx.block()
                .call_void("js_process_exit", &[(DOUBLE, &code_val)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ProcessAbort => {
            // process.abort() — raises SIGABRT immediately. The runtime fn
            // calls libc::abort(); we still return undefined to satisfy the
            // expression type even though control never reaches the caller.
            ctx.block().call_void("js_process_abort", &[]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ProcessUmask(mask) => {
            // process.umask(mask?) — returns the current file-mode creation
            // mask as a number. The arg form sets the mask first and returns
            // the previous value. The no-arg form reads-and-restores so the
            // mask isn't disturbed.
            if let Some(e) = mask {
                let v = lower_expr(ctx, e)?;
                Ok(ctx
                    .block()
                    .call(DOUBLE, "js_process_umask_set", &[(DOUBLE, &v)]))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_process_umask", &[]))
            }
        }
        Expr::ObjectGetPrototypeOf(o) => {
            // v0.5.751: route through the runtime helper which walks
            // the class registry's parent_class_id chain for INT32-tagged
            // class refs. Pre-fix this returned the operand unchanged,
            // causing infinite loops in `cur = Object.getPrototypeOf(cur);
            // while (cur) {...}` walks (drizzle's is() chain). Refs
            // #420 / #618 followup.
            let v = lower_expr(ctx, o)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_object_get_prototype_of", &[(DOUBLE, &v)]))
        }
        Expr::ObjectDefineProperties(target, descs) => {
            // chalk's `Object.defineProperties(createChalk.prototype, styles)`
            // — `styles` is constructed from `Object.create(null)` + dynamic
            // assignment, so the static desugaring in expr_call.rs's
            // `defineProperties` arm doesn't fire and we fall here. Route
            // to a runtime helper that iterates the descriptor object's
            // own keys and reuses `js_object_define_property` per key.
            let t = lower_expr(ctx, target)?;
            let d = lower_expr(ctx, descs)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_define_properties",
                &[(DOUBLE, &t), (DOUBLE, &d)],
            ))
        }
        Expr::ObjectSetPrototypeOf(obj, proto) => {
            // chalk's foundation idiom (`Object.setPrototypeOf(closure,
            // ClassProto)`): perry doesn't track per-instance prototype
            // chains (class IDs are baked at allocation, the runtime walks
            // `parent_class_id` for INT32-tagged class refs and stops
            // there). The runtime helper registers the (obj, proto) pair
            // in a side-table so a later `Object.getPrototypeOf(obj)` can
            // observe the user's intent — even if Perry's downstream
            // dispatch ignores it. Returning the target matches the spec.
            //
            // Pre-fix this expression fell through to a generic
            // `(Object).setPrototypeOf(...)` PropertyGet → Call which
            // throws `TypeError: value is not a function` because
            // `Object` isn't a runtime object with method dispatch.
            // chalk's `import chalk from "chalk"` died at module init.
            let obj_v = lower_expr(ctx, obj)?;
            let proto_v = lower_expr(ctx, proto)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_set_prototype_of",
                &[(DOUBLE, &obj_v), (DOUBLE, &proto_v)],
            ))
        }
        Expr::MathExpm1(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_expm1", &[(DOUBLE, &v)]))
        }
        Expr::MathExp(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "llvm.exp.f64", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcFullYear { date, args } => super::os_uri_dates::lower_date_setter(
            ctx,
            date,
            args,
            true,
            super::os_uri_dates::DATE_FIELD_FULL_YEAR,
        ),
        Expr::DateGetDate(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_date", &[(DOUBLE, &v)]))
        }
        Expr::DateGetDay(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx.block().call(DOUBLE, "js_date_get_day", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcDate(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_date", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcFullYear(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_full_year", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMonth(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_month", &[(DOUBLE, &v)]))
        }
        Expr::DateGetHours(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_hours", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMinutes(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_minutes", &[(DOUBLE, &v)]))
        }
        Expr::DateGetSeconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_seconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMilliseconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_milliseconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcHours(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_hours", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMinutes(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_minutes", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcSeconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_seconds", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcMilliseconds(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_milliseconds", &[(DOUBLE, &v)]))
        }
        Expr::Atob(inner) => {
            // atob(base64) — decode to a binary string. Runtime takes a
            // NaN-boxed string (f64) and returns a raw *const StringHeader
            // (i64), which we re-NaN-box with STRING_TAG.
            let v = lower_expr(ctx, inner)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_atob", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::Btoa(inner) => {
            // btoa(string) — base64-encode a binary string. Same ABI as atob.
            let v = lower_expr(ctx, inner)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_btoa", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::ArrayFlat { array } => {
            let arr_box = lower_expr(ctx, array)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(I64, "js_array_flat", &[(I64, &arr_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        Expr::ArrayFlatMap { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let result = blk.call(
                I64,
                "js_array_flatMap",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- Math.sin/cos via LLVM intrinsics --------
        Expr::MathSin(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "llvm.sin.f64", &[(DOUBLE, &v)]))
        }
        Expr::MathCos(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "llvm.cos.f64", &[(DOUBLE, &v)]))
        }
        // Hyperbolic + extra trig via runtime (uses Rust's f64 methods).
        Expr::MathSinh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_sinh", &[(DOUBLE, &v)]))
        }
        Expr::MathCosh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_cosh", &[(DOUBLE, &v)]))
        }
        Expr::MathTanh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_tanh", &[(DOUBLE, &v)]))
        }
        Expr::MathTan(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_tan", &[(DOUBLE, &v)]))
        }
        Expr::MathAsin(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_asin", &[(DOUBLE, &v)]))
        }
        Expr::MathAcos(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_acos", &[(DOUBLE, &v)]))
        }
        Expr::MathAtan(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_atan", &[(DOUBLE, &v)]))
        }
        Expr::MathAtan2(y, x) => {
            let y_v = lower_math_operand(ctx, y)?;
            let x_v = lower_math_operand(ctx, x)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_math_atan2", &[(DOUBLE, &y_v), (DOUBLE, &x_v)]))
        }

        // -------- String.fromCharCode(code) --------
        Expr::StringFromCharCode(o) => {
            // #2788: pass the raw NaN-boxed value; the runtime applies ToUint16
            // so negative/out-of-range codes wrap modulo 65536. A prior fptosi
            // is UB on a NaN and dropped the wrap semantics.
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_from_char_code", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::StringFromCharCodeSpread(o) => {
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_from_char_code_array", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::RegExpSetLastIndex { regex, value } => {
            let r_box = lower_expr(ctx, regex)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            let r_handle = unbox_to_i64(blk, &r_box);
            blk.call_void(
                "js_regexp_set_last_index",
                &[(I64, &r_handle), (DOUBLE, &v)],
            );
            Ok(v)
        }
        Expr::ProcessStdin => Ok(ctx.block().call(DOUBLE, "js_process_stdin", &[])),
        Expr::ProcessStdout => Ok(ctx.block().call(DOUBLE, "js_process_stdout", &[])),
        Expr::ProcessStderr => Ok(ctx.block().call(DOUBLE, "js_process_stderr", &[])),
        Expr::MathAsinh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_asinh", &[(DOUBLE, &v)]))
        }
        Expr::MathAcosh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_acosh", &[(DOUBLE, &v)]))
        }
        Expr::MathAtanh(o) => {
            let v = lower_math_operand(ctx, o)?;
            Ok(ctx.block().call(DOUBLE, "js_math_atanh", &[(DOUBLE, &v)]))
        }
        Expr::DateSetUtcDate { date, args } => super::os_uri_dates::lower_date_setter(
            ctx,
            date,
            args,
            true,
            super::os_uri_dates::DATE_FIELD_DATE,
        ),
        Expr::DateSetUtcHours { date, args } => super::os_uri_dates::lower_date_setter(
            ctx,
            date,
            args,
            true,
            super::os_uri_dates::DATE_FIELD_HOURS,
        ),
        Expr::ProcessKill { pid, signal } => {
            let pid_d = lower_expr(ctx, pid)?;
            let sig_d = match signal {
                Some(s) => lower_expr(ctx, s)?,
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_process_kill",
                &[(DOUBLE, &pid_d), (DOUBLE, &sig_d)],
            ))
        }
        // -------- Symbol() / Symbol.for / ObjectGetOwnPropertySymbols --------
        // Runtime functions in perry-runtime/src/symbol.rs take and return
        // NaN-boxed f64 values directly, so no unbox/box dance needed.
        Expr::SymbolNew(desc) => match desc {
            Some(d) => {
                let d_box = lower_expr(ctx, d)?;
                let blk = ctx.block();
                Ok(blk.call(DOUBLE, "js_symbol_new", &[(DOUBLE, &d_box)]))
            }
            None => {
                let blk = ctx.block();
                Ok(blk.call(DOUBLE, "js_symbol_new_empty", &[]))
            }
        },
        Expr::SymbolFor(key) => {
            let k_box = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_symbol_for", &[(DOUBLE, &k_box)]))
        }
        Expr::SymbolKeyFor(sym) => {
            let s_box = lower_expr(ctx, sym)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_symbol_key_for", &[(DOUBLE, &s_box)]))
        }
        // RegExp.escape(str) — runtime returns a NaN-boxed string f64.
        Expr::RegExpEscape(arg) => {
            let a_box = lower_expr(ctx, arg)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_regexp_escape", &[(DOUBLE, &a_box)]))
        }
        Expr::SymbolDescription(sym) => {
            let s_box = lower_expr(ctx, sym)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_symbol_description", &[(DOUBLE, &s_box)]))
        }
        Expr::SymbolToString(sym) => {
            // Returns i64 string pointer (not NaN-boxed).
            let s_box = lower_expr(ctx, sym)?;
            let blk = ctx.block();
            let h = blk.call(I64, "js_symbol_to_string", &[(DOUBLE, &s_box)]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::ObjectGetOwnPropertySymbols(obj) => {
            // Runtime takes a NaN-boxed f64 (the runtime decl is `[DOUBLE]`),
            // returns a raw `*mut ArrayHeader` as i64. Pass the boxed value
            // directly — do NOT unbox to i64, that would put the raw pointer
            // in an integer register while the runtime expects it in a float
            // register.
            let o_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr = blk.call(
                I64,
                "js_object_get_own_property_symbols",
                &[(DOUBLE, &o_box)],
            );
            Ok(nanbox_pointer_inline(blk, &arr))
        }
        Expr::TextEncoderNew => {
            // Stateless UTF-8 encoder — return a non-null sentinel pointer.
            // NaN-box with POINTER_TAG so `typeof encoder === "object"` holds.
            let blk = ctx.block();
            let h = blk.call(I64, "js_text_encoder_new", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::TextDecoderNew {
            label,
            fatal,
            ignore_bom,
        } => {
            // new TextDecoder(label?, { fatal?, ignoreBOM? }) — the runtime
            // validates the label, stores per-instance state, and returns a
            // small-int handle. NaN-box with POINTER_TAG so the handle reads
            // back through `decoder_handle_id` for decode/property access.
            let label = lower_expr(ctx, label)?;
            let fatal = lower_expr(ctx, fatal)?;
            let ignore_bom = lower_expr(ctx, ignore_bom)?;
            let blk = ctx.block();
            let h = blk.call(
                I64,
                "js_text_decoder_new",
                &[(DOUBLE, &label), (DOUBLE, &fatal), (DOUBLE, &ignore_bom)],
            );
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::TextEncoderEncode(o) => {
            // encoder.encode(str) — runtime returns an i64 pointer to an
            // ArrayHeader whose f64 elements hold the UTF-8 byte values
            // (see crates/perry-runtime/src/text.rs). NaN-box with
            // POINTER_TAG so `.length` / `[i]` inline paths can unbox it
            // as an array handle. The runtime also registers the result
            // pointer in BUFFER_REGISTRY so `instanceof Uint8Array` holds.
            let v = lower_expr(ctx, o)?;
            let blk = ctx.block();
            let arr_ptr = blk.call(I64, "js_text_encoder_encode_llvm", &[(DOUBLE, &v)]);
            Ok(nanbox_pointer_inline(blk, &arr_ptr))
        }
        Expr::TextEncoderEncodeInto { source, dest } => {
            let source = lower_expr(ctx, source)?;
            let dest = lower_expr(ctx, dest)?;
            let blk = ctx.block();
            let obj_ptr = blk.call(
                I64,
                "js_text_encoder_encode_into_llvm",
                &[(DOUBLE, &source), (DOUBLE, &dest)],
            );
            Ok(nanbox_pointer_inline(blk, &obj_ptr))
        }
        Expr::TextDecoderDecode { decoder, input } => {
            // decoder.decode(bufOrArr) — runtime reads the decoder handle's
            // encoding/fatal state and decodes `input` (BufferHeader-backed
            // value from `encoder.encode(...)` or `new Uint8Array([...])`).
            // NaN-box the result with STRING_TAG.
            let dec = lower_expr(ctx, decoder)?;
            let v = lower_expr(ctx, input)?;
            let blk = ctx.block();
            let str_ptr = blk.call(
                I64,
                "js_text_decoder_decode_llvm",
                &[(DOUBLE, &dec), (DOUBLE, &v)],
            );
            Ok(nanbox_string_inline(blk, &str_ptr))
        }
        Expr::TextDecoderEncoding(d) => {
            let dec = lower_expr(ctx, d)?;
            let blk = ctx.block();
            let str_ptr = blk.call(I64, "js_text_decoder_encoding", &[(DOUBLE, &dec)]);
            Ok(nanbox_string_inline(blk, &str_ptr))
        }
        Expr::TextDecoderFatal(d) => {
            let dec = lower_expr(ctx, d)?;
            let blk = ctx.block();
            // Runtime returns a NaN-boxed boolean (DOUBLE) directly.
            Ok(blk.call(DOUBLE, "js_text_decoder_fatal", &[(DOUBLE, &dec)]))
        }
        Expr::TextDecoderIgnoreBom(d) => {
            let dec = lower_expr(ctx, d)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_text_decoder_ignore_bom", &[(DOUBLE, &dec)]))
        }
        Expr::OsArch => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_arch", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsType => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_type", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsPlatform => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_platform", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsRelease => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_release", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsHostname => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_hostname", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsHomedir => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_homedir", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsTmpdir => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_tmpdir", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsTotalmem => Ok(ctx.block().call(DOUBLE, "js_os_totalmem", &[])),
        Expr::OsFreemem => Ok(ctx.block().call(DOUBLE, "js_os_freemem", &[])),
        Expr::OsUptime => Ok(ctx.block().call(DOUBLE, "js_os_uptime", &[])),
        Expr::OsCpus => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_cpus", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::OsNetworkInterfaces => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_network_interfaces", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::OsUserInfo => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_user_info", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::OsUserInfoBuffer => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_user_info_buffer", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::OsDevNull => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_dev_null", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsAvailableParallelism => {
            Ok(ctx.block().call(DOUBLE, "js_os_available_parallelism", &[]))
        }
        Expr::OsEndianness => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_endianness", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        Expr::OsLoadavg => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_loadavg", &[]);
            Ok(nanbox_pointer_inline(blk, &h))
        }
        Expr::OsMachine => {
            let blk = ctx.block();
            let h = blk.call(I64, "js_os_machine", &[]);
            Ok(nanbox_string_inline(blk, &h))
        }
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
