//! MathFround..SetClear.
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
use crate::native_value::{ExpectedNativeRep, LoweredValue, MaterializationReason, NativeRep};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, F32, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32, lower_expr_native,
    lower_expr_value, lower_index_set_fast, lower_js_args_array, lower_math_operand,
    lower_object_literal, lower_stream_super_init, lower_url_string_getter, materialize_js_value,
    nanbox_bigint_inline, nanbox_pointer_inline, nanbox_pointer_inline_pub, nanbox_string_inline,
    proxy_build_args_array, try_flat_const_2d_int, try_lower_flat_const_index_get,
    try_match_channel_reduction, try_static_class_name, unbox_str_handle, unbox_to_i64,
    variant_name, ChannelReduction, FlatConstInfo, FnCtx, I18nLowerCtx,
};

fn lowered_value_to_iter_result_f64(
    ctx: &mut FnCtx<'_>,
    lowered: LoweredValue,
) -> (LoweredValue, &'static str) {
    match lowered.rep {
        NativeRep::F64 => (lowered, "slot_kind=raw_f64_proven"),
        NativeRep::F32 => {
            let value = ctx.block().fpext(F32, &lowered.value, DOUBLE);
            (LoweredValue::f64(value), "slot_kind=raw_f64_proven")
        }
        NativeRep::I32 => {
            let value = ctx.block().sitofp(I32, &lowered.value, DOUBLE);
            (LoweredValue::f64(value), "slot_kind=raw_f64_proven")
        }
        NativeRep::U8 => {
            let widened = ctx.block().zext(I8, &lowered.value, I32);
            let value = ctx.block().uitofp(I32, &widened, DOUBLE);
            (LoweredValue::f64(value), "slot_kind=raw_f64_proven")
        }
        NativeRep::U32 | NativeRep::BufferLen => {
            let value = ctx.block().uitofp(I32, &lowered.value, DOUBLE);
            (LoweredValue::f64(value), "slot_kind=raw_f64_proven")
        }
        _ => {
            let boxed = materialize_js_value(ctx, lowered, MaterializationReason::RuntimeApi);
            let value = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)]);
            (LoweredValue::f64(value), "slot_kind=raw_f64_coerced")
        }
    }
}

fn lower_iter_result_f64_payload(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
) -> Result<(LoweredValue, &'static str)> {
    match value {
        Expr::Integer(_) | Expr::Number(_) | Expr::IterResultGetValue => {
            let Some(lowered) = lower_expr_value(ctx, value)? else {
                let boxed = lower_expr(ctx, value)?;
                let value = ctx
                    .block()
                    .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)]);
                return Ok((LoweredValue::f64(value), "slot_kind=raw_f64_coerced"));
            };
            Ok(lowered_value_to_iter_result_f64(ctx, lowered))
        }
        _ => {
            let boxed = lower_expr(ctx, value)?;
            let value = ctx
                .block()
                .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &boxed)]);
            Ok((LoweredValue::f64(value), "slot_kind=raw_f64_coerced"))
        }
    }
}

fn is_definite_bool_iter_result_payload(value: &Expr) -> bool {
    matches!(
        value,
        Expr::Bool(_)
            | Expr::Compare { .. }
            | Expr::Unary {
                op: UnaryOp::Not,
                ..
            }
            | Expr::BooleanCoerce(_)
            | Expr::IsFinite(_)
            | Expr::IsNaN(_)
            | Expr::NumberIsNaN(_)
            | Expr::NumberIsFinite(_)
            | Expr::NumberIsInteger(_)
            | Expr::IsUndefinedOrBareNan(_)
            | Expr::SetHas { .. }
            | Expr::SetDelete { .. }
            | Expr::MapHas { .. }
            | Expr::MapDelete { .. }
            | Expr::ArrayIncludes { .. }
    )
}

fn lower_iter_result_i1_payload(ctx: &mut FnCtx<'_>, value: &Expr) -> Result<Option<LoweredValue>> {
    if matches!(value, Expr::LocalGet(_)) {
        let Some(lowered) = lower_expr_value(ctx, value)? else {
            return Ok(None);
        };
        return Ok(matches!(lowered.rep, NativeRep::I1).then_some(lowered));
    }
    if !is_definite_bool_iter_result_payload(value) {
        return Ok(None);
    }
    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::I1)?;
    Ok(matches!(lowered.rep, NativeRep::I1).then_some(lowered))
}

fn lower_iter_result_i32_payload(
    ctx: &mut FnCtx<'_>,
    value: &Expr,
) -> Result<Option<LoweredValue>> {
    if !super::can_lower_expr_as_i32_in_current_region(ctx, value) {
        return Ok(None);
    }
    let lowered = lower_expr_native(ctx, value, ExpectedNativeRep::I32)?;
    Ok(matches!(lowered.rep, NativeRep::I32).then_some(lowered))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::MathFround(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_fround", &[(DOUBLE, &v)]))
        }
        Expr::MathF16round(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_math_f16round", &[(DOUBLE, &v)]))
        }

        // -------- new Map(init) — consume any iterable + validate (#2770) --------
        // Pass the NaN-boxed init value so the runtime can classify it by tag
        // (number/symbol/object/iterable) and throw Node's exact TypeErrors for
        // non-iterables / malformed entries instead of mis-reading an
        // ArrayHeader.
        Expr::MapNewFromArray(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_map_from_iterable", &[(DOUBLE, &arr_box)]);
            Ok(nanbox_pointer_inline(blk, &handle))
        }

        // -------- DateGetTime / DateGetTimezoneOffset --------
        Expr::DateGetTime(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_time", &[(DOUBLE, &v)]))
        }
        Expr::DateGetTimezoneOffset(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_timezone_offset", &[(DOUBLE, &v)]))
        }
        // -------- Date.UTC(year, month?, day?, hour?, minute?, second?, ms?) --------
        // #2826: the runtime needs the actual argument count to apply
        // Node-correct defaults (omitted month→0, day→1; argc==0→NaN; year
        // 0..99→1900+year), so we pass a NaN-boxed args buffer + count rather
        // than padding missing slots with 0.
        Expr::DateUtc(args) => {
            let mut vals: Vec<String> = Vec::with_capacity(args.len());
            for a in args.iter() {
                vals.push(lower_expr(ctx, a)?);
            }
            let blk = ctx.block();
            let (args_ptr, argc) = if vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = vals.len();
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, format!("{}", n))
            };
            Ok(blk.call(DOUBLE, "js_date_utc", &[(PTR, &args_ptr), (I32, &argc)]))
        }

        // -------- Object.defineProperty --------
        Expr::ObjectDefineProperty(obj, key, value) => {
            let o = lower_expr(ctx, obj)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let blk = ctx.block();
            blk.call(
                DOUBLE,
                "js_object_define_property",
                &[(DOUBLE, &o), (DOUBLE, &k), (DOUBLE, &v)],
            );
            Ok(o)
        }

        // -------- path.isAbsolute(p) -> boolean --------
        Expr::PathIsAbsolute(p) => {
            let p_box = lower_expr(ctx, p)?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            let i32_res = blk.call(I32, "js_path_is_absolute", &[(I64, &p_handle)]);
            Ok(i32_bool_to_nanbox(blk, &i32_res))
        }

        // -------- process.hrtime.bigint() — returns already NaN-boxed BigInt --------
        Expr::ProcessHrtimeBigint => Ok(ctx.block().call(DOUBLE, "js_process_hrtime_bigint", &[])),

        // -------- process.hrtime(prior?) — [secs, nanos] tuple (#1345) --------
        Expr::ProcessHrtime(prior) => {
            let prior_val = if let Some(e) = prior {
                lower_expr(ctx, e)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            Ok(ctx
                .block()
                .call(DOUBLE, "js_process_hrtime", &[(DOUBLE, &prior_val)]))
        }

        // -------- process.title getter/setter (#1401) --------
        Expr::ProcessTitle => Ok(ctx.block().call(DOUBLE, "js_process_title", &[])),
        Expr::ProcessSetTitle(value) => {
            let v = lower_expr(ctx, value)?;
            ctx.block()
                .call_void("js_process_set_title", &[(DOUBLE, &v)]);
            Ok(v)
        }

        // -------- RegExpExecIndex — reads thread-local from the last exec() call --------
        Expr::RegExpExecIndex => Ok(ctx.block().call(DOUBLE, "js_regexp_exec_get_index", &[])),

        // -------- Crypto.* wired to real runtime helpers --------
        Expr::CryptoRandomUUID => {
            let blk = ctx.block();
            let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let handle = blk.call(I64, "js_crypto_random_uuid", &[(DOUBLE, &undefined)]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::CryptoRandomUUIDv7 => {
            let blk = ctx.block();
            let handle = blk.call(I64, "js_crypto_random_uuidv7", &[]);
            Ok(nanbox_string_inline(blk, &handle))
        }
        Expr::CryptoRandomBytes(operand) => {
            // Returns a raw *mut BufferHeader i64. NaN-box with
            // POINTER_TAG so downstream BUFFER_REGISTRY checks
            // (format_jsvalue, .length, etc.) see a real buffer.
            let size_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let buf_handle = blk.call(I64, "js_crypto_random_bytes_buffer", &[(DOUBLE, &size_box)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }
        Expr::CryptoSha256(operand) => {
            let data_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let data_handle = unbox_to_i64(blk, &data_box);
            let result = blk.call(I64, "js_crypto_sha256", &[(I64, &data_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }
        Expr::CryptoMd5(operand) => {
            let data_box = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let data_handle = unbox_to_i64(blk, &data_box);
            let result = blk.call(I64, "js_crypto_md5", &[(I64, &data_handle)]);
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- Web Crypto API (issue #561) --------
        // Each helper takes the JS values as f64 (NaN-boxed) and returns
        // a *mut Promise that codegen NaN-boxes with POINTER_TAG. The
        // runtime resolves synchronously inside the Promise body since
        // SHA / HMAC are CPU-bound — the await is decorative.
        Expr::WebCryptoDigest { algo, data } => {
            let algo_box = lower_expr(ctx, algo)?;
            let data_box = lower_expr(ctx, data)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_digest",
                &[(DOUBLE, &algo_box), (DOUBLE, &data_box)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoImportKey {
            format,
            key,
            algorithm,
            extractable,
            usages,
        } => {
            let format_box = lower_expr(ctx, format)?;
            let key_box = lower_expr(ctx, key)?;
            let algo_box = lower_expr(ctx, algorithm)?;
            let extractable_box = lower_expr(ctx, extractable)?;
            let usages_box = lower_expr(ctx, usages)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_import_key",
                &[
                    (DOUBLE, &format_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &algo_box),
                    (DOUBLE, &extractable_box),
                    (DOUBLE, &usages_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoExportKey { format, key } => {
            let format_box = lower_expr(ctx, format)?;
            let key_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_export_key",
                &[(DOUBLE, &format_box), (DOUBLE, &key_box)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoSign {
            algorithm,
            key,
            data,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, key)?;
            let data_box = lower_expr(ctx, data)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_sign",
                &[(DOUBLE, &algo_box), (DOUBLE, &key_box), (DOUBLE, &data_box)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoVerify {
            algorithm,
            key,
            signature,
            data,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, key)?;
            let sig_box = lower_expr(ctx, signature)?;
            let data_box = lower_expr(ctx, data)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_verify",
                &[
                    (DOUBLE, &algo_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &sig_box),
                    (DOUBLE, &data_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoDeriveBits {
            algorithm,
            base_key,
            length,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, base_key)?;
            let length_box = lower_expr(ctx, length)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_derive_bits",
                &[
                    (DOUBLE, &algo_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &length_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoDeriveKey {
            algorithm,
            base_key,
            derived_key_algorithm,
            extractable,
            usages,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, base_key)?;
            let derived_algo_box = lower_expr(ctx, derived_key_algorithm)?;
            let extractable_box = lower_expr(ctx, extractable)?;
            let usages_box = lower_expr(ctx, usages)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_derive_key",
                &[
                    (DOUBLE, &algo_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &derived_algo_box),
                    (DOUBLE, &extractable_box),
                    (DOUBLE, &usages_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoEncrypt {
            algorithm,
            key,
            data,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, key)?;
            let data_box = lower_expr(ctx, data)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_encrypt",
                &[(DOUBLE, &algo_box), (DOUBLE, &key_box), (DOUBLE, &data_box)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoDecrypt {
            algorithm,
            key,
            data,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let key_box = lower_expr(ctx, key)?;
            let data_box = lower_expr(ctx, data)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_decrypt",
                &[(DOUBLE, &algo_box), (DOUBLE, &key_box), (DOUBLE, &data_box)],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoGenerateKey {
            algorithm,
            extractable,
            usages,
        } => {
            let algo_box = lower_expr(ctx, algorithm)?;
            let extractable_box = lower_expr(ctx, extractable)?;
            let usages_box = lower_expr(ctx, usages)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_generate_key",
                &[
                    (DOUBLE, &algo_box),
                    (DOUBLE, &extractable_box),
                    (DOUBLE, &usages_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoWrapKey {
            format,
            key,
            wrapping_key,
            wrap_algorithm,
        } => {
            let format_box = lower_expr(ctx, format)?;
            let key_box = lower_expr(ctx, key)?;
            let wrapping_key_box = lower_expr(ctx, wrapping_key)?;
            let wrap_algo_box = lower_expr(ctx, wrap_algorithm)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_wrap_key",
                &[
                    (DOUBLE, &format_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &wrapping_key_box),
                    (DOUBLE, &wrap_algo_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::WebCryptoUnwrapKey {
            format,
            wrapped_key,
            unwrapping_key,
            unwrap_algorithm,
            unwrapped_key_algorithm,
            extractable,
            usages,
        } => {
            let format_box = lower_expr(ctx, format)?;
            let wrapped_key_box = lower_expr(ctx, wrapped_key)?;
            let unwrapping_key_box = lower_expr(ctx, unwrapping_key)?;
            let unwrap_algo_box = lower_expr(ctx, unwrap_algorithm)?;
            let unwrapped_algo_box = lower_expr(ctx, unwrapped_key_algorithm)?;
            let extractable_box = lower_expr(ctx, extractable)?;
            let usages_box = lower_expr(ctx, usages)?;
            let blk = ctx.block();
            let promise = blk.call(
                I64,
                "js_webcrypto_unwrap_key",
                &[
                    (DOUBLE, &format_box),
                    (DOUBLE, &wrapped_key_box),
                    (DOUBLE, &unwrapping_key_box),
                    (DOUBLE, &unwrap_algo_box),
                    (DOUBLE, &unwrapped_algo_box),
                    (DOUBLE, &extractable_box),
                    (DOUBLE, &usages_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }
        Expr::CryptoRandomFillSync {
            buffer,
            offset,
            size,
        } => {
            // Fill `buffer` (Buffer or TypedArray) with random bytes
            // in-place; return the same NaN-boxed buffer value. `offset`
            // and `size` are NaN-boxed JS values (Undefined → use
            // defaults). The runtime accepts both layouts.
            let buf_box = lower_expr(ctx, buffer)?;
            let off_box = lower_expr(ctx, offset)?;
            let sz_box = lower_expr(ctx, size)?;
            let blk = ctx.block();
            let result = blk.call(
                DOUBLE,
                "js_crypto_random_fill_sync",
                &[(DOUBLE, &buf_box), (DOUBLE, &off_box), (DOUBLE, &sz_box)],
            );
            Ok(result)
        }

        // -------- arr.indexOf(value) -> number --------
        // Issue #214: route through `_jsvalue` so string elements
        // match by content (handles SSO + heap-string mixed arrays).
        // Mirrors the `includes` arm + the `lower_array_method::indexOf`
        // arm.
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let v = lower_expr(ctx, value)?;
            // #2804: optional fromIndex. has_from=1 + lowered index when
            // present; otherwise has_from=0 with a placeholder DOUBLE (`v`).
            let (from_box, has_from) = match from_index {
                Some(fi) => (lower_expr(ctx, fi)?, "1"),
                None => (v.clone(), "0"),
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let i64_v = blk.call(
                I64,
                "js_array_indexOf_jsvalue",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &v),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            Ok(blk.sitofp(I64, &i64_v, DOUBLE))
        }

        // arr.lastIndexOf(value, fromIndex?) — mirrors ArrayIndexOf + the
        // `lower_array_method::lastIndexOf` arm. Routed here (instead of the
        // string `lastIndexOf`) for known-not-string / typed-array locals.
        Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => {
            let arr_box = lower_expr(ctx, array)?;
            let v = lower_expr(ctx, value)?;
            // With a fromIndex, pass has_from=1 + the lowered index; without,
            // pass has_from=0 and reuse `v` as an ignored placeholder DOUBLE
            // operand (runtime defaults to length-1).
            let (from_box, has_from) = match from_index {
                Some(fi) => (lower_expr(ctx, fi)?, "1"),
                None => (v.clone(), "0"),
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let i64_v = blk.call(
                I64,
                "js_array_last_index_of_jsvalue",
                &[
                    (I64, &arr_handle),
                    (DOUBLE, &v),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            Ok(blk.sitofp(I64, &i64_v, DOUBLE))
        }

        // -------- arr.forEach(callback) — invoke callback for side effects --------
        Expr::ArrayForEach { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating
            // (validated up front so even an empty array throws, per spec).
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            blk.call_void("js_array_forEach", &[(I64, &arr_handle), (I64, &cb_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- Object.getOwnPropertyDescriptor(obj, key) --------
        Expr::ObjectGetOwnPropertyDescriptor(obj, key) => {
            let o = lower_expr(ctx, obj)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_get_own_property_descriptor",
                &[(DOUBLE, &o), (DOUBLE, &k)],
            ))
        }

        // -------- Object.getOwnPropertyDescriptors(obj) --------
        Expr::ObjectGetOwnPropertyDescriptors(obj) => {
            let o = lower_expr(ctx, obj)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_get_own_property_descriptors",
                &[(DOUBLE, &o)],
            ))
        }

        // -------- Math.cbrt --------
        Expr::MathCbrt(operand) => {
            let v = lower_math_operand(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_math_cbrt", &[(DOUBLE, &v)]))
        }

        // -------- Date.* getters: real runtime calls --------
        Expr::DateGetFullYear(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_full_year", &[(DOUBLE, &v)]))
        }
        Expr::DateGetMonth(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_month", &[(DOUBLE, &v)]))
        }
        Expr::DateGetUtcDay(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_get_utc_day", &[(DOUBLE, &v)]))
        }
        Expr::DateValueOf(d) => {
            let v = lower_expr(ctx, d)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_date_value_of", &[(DOUBLE, &v)]))
        }

        // -------- process.on(event, handler) — EventEmitter listener
        // registration on the process singleton.
        Expr::ProcessOn { event, handler } => {
            let event_box = lower_expr(ctx, event)?;
            let handler_box = lower_expr(ctx, handler)?;
            let blk = ctx.block();
            let event_handle = unbox_str_handle(blk, &event_box);
            let handler_handle = unbox_to_i64(blk, &handler_box);
            Ok(blk.call(
                DOUBLE,
                "js_process_on",
                &[(I64, &event_handle), (I64, &handler_handle)],
            ))
        }

        // -------- process.once(event, handler) — one-shot listener;
        // the handler is removed after its first invocation (Node parity).
        Expr::ProcessOnce { event, handler } => {
            let event_box = lower_expr(ctx, event)?;
            let handler_box = lower_expr(ctx, handler)?;
            let blk = ctx.block();
            let event_handle = unbox_str_handle(blk, &event_box);
            let handler_handle = unbox_to_i64(blk, &handler_box);
            Ok(blk.call(
                DOUBLE,
                "js_process_once",
                &[(I64, &event_handle), (I64, &handler_handle)],
            ))
        }

        // -------- process.stdin.setRawMode(enabled) — toggle raw-mode
        // termios on stdin and flip the readline reader's mode flag
        // (#347 Phase 2).
        Expr::ProcessStdinSetRawMode(arg) => {
            let arg_box = lower_expr(ctx, arg)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_readline_set_raw_mode", &[(DOUBLE, &arg_box)]))
        }

        // -------- process.stdin.on(event, handler) — register a callback
        // for raw-mode 'data' / 'keypress' events or 'end'/'close' EOF
        // (#347 Phase 2). Event string goes through unbox_str_handle so
        // SSO operands resolve to a real heap StringHeader; handler is
        // unboxed to its closure pointer via the standard mask.
        Expr::ProcessStdinOn { event, handler } => {
            let event_box = lower_expr(ctx, event)?;
            let handler_box = lower_expr(ctx, handler)?;
            let blk = ctx.block();
            let event_handle = unbox_str_handle(blk, &event_box);
            let handler_handle = unbox_to_i64(blk, &handler_box);
            blk.call_void(
                "js_readline_stdin_on",
                &[(I64, &event_handle), (I64, &handler_handle)],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        Expr::ProcessStdinRemoveListener { event, handler } => {
            let event_box = lower_expr(ctx, event)?;
            let handler_box = lower_expr(ctx, handler)?;
            let blk = ctx.block();
            let event_handle = unbox_str_handle(blk, &event_box);
            let handler_handle = unbox_to_i64(blk, &handler_box);
            Ok(blk.call(
                DOUBLE,
                "js_readline_stdin_remove_listener",
                &[(I64, &event_handle), (I64, &handler_handle)],
            ))
        }

        Expr::ProcessStdinLifecycle(method) => {
            let symbol = match method {
                perry_hir::ProcessStdinLifecycleMethod::Pause => "js_readline_stdin_pause",
                perry_hir::ProcessStdinLifecycleMethod::Resume => "js_readline_stdin_resume",
                perry_hir::ProcessStdinLifecycleMethod::Unref => "js_readline_stdin_unref",
                perry_hir::ProcessStdinLifecycleMethod::Ref => "js_readline_stdin_ref",
                perry_hir::ProcessStdinLifecycleMethod::Destroy => "js_readline_stdin_destroy",
            };
            Ok(ctx.block().call(DOUBLE, symbol, &[]))
        }

        // -------- process.stdout.on(event, handler) — register a callback
        // for the 'resize' event (#347 Phase 3). Other events fall
        // through to the runtime's no-op (silently ignored).
        Expr::ProcessStdoutOn { event, handler } => {
            let event_box = lower_expr(ctx, event)?;
            let handler_box = lower_expr(ctx, handler)?;
            let blk = ctx.block();
            let event_handle = unbox_str_handle(blk, &event_box);
            let handler_handle = unbox_to_i64(blk, &handler_box);
            Ok(blk.call(
                DOUBLE,
                "js_process_stdout_on",
                &[(I64, &event_handle), (I64, &handler_handle)],
            ))
        }

        // -------- tty.isatty(fd) (#347 Phase 3) --------
        Expr::TtyIsAtty(fd) => {
            let fd_box = lower_expr(ctx, fd)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_tty_isatty", &[(DOUBLE, &fd_box)]))
        }

        // -------- process.std{in,out,err}.isTTY (#347 Phase 3) --------
        Expr::ProcessStdinIsTTY => Ok(ctx.block().call(DOUBLE, "js_process_stdin_isatty", &[])),
        Expr::ProcessStdoutIsTTY => Ok(ctx.block().call(DOUBLE, "js_process_stdout_isatty", &[])),
        Expr::ProcessStderrIsTTY => Ok(ctx.block().call(DOUBLE, "js_process_stderr_isatty", &[])),

        // -------- process.stdout.columns / .rows (#347 Phase 3) --------
        Expr::ProcessStdoutColumns => {
            Ok(ctx.block().call(DOUBLE, "js_process_stdout_columns", &[]))
        }
        Expr::ProcessStdoutRows => Ok(ctx.block().call(DOUBLE, "js_process_stdout_rows", &[])),

        // -------- performance.now() — sub-millisecond resolution --------
        Expr::PerformanceNow => Ok(ctx.block().call(DOUBLE, "js_performance_now", &[])),

        // -------- async-step iter-result scratch helpers --------
        // Emitted only by the generator transform for `was_plain_async`
        // functions. The state machine writes (value, done) via
        // `IterResultSet`; the async-step driver reads them back via
        // `IterResultGetValue` / `IterResultGetDone`. Eliminates the
        // per-await `{value, done}` heap alloc on the hot path.
        Expr::IterResultSet(value, done) => {
            let done_str = if *done { "1" } else { "0" };
            if let Some(raw) = lower_iter_result_i1_payload(ctx, value)? {
                let value_i32 = ctx.block().zext(I1, &raw.value, I32);
                let result = ctx.block().call(
                    DOUBLE,
                    "js_iter_result_set_i1",
                    &[(I32, &value_i32), (I32, done_str)],
                );
                ctx.record_lowered_value(
                    "IterResultSet",
                    None,
                    "compiler_private_async_iter_result_set_i1",
                    &raw,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec!["slot_kind=raw_i1_proven".to_string()],
                );
                Ok(result)
            } else if let Some(raw) = lower_iter_result_i32_payload(ctx, value)? {
                let result = ctx.block().call(
                    DOUBLE,
                    "js_iter_result_set_i32",
                    &[(I32, raw.value.as_str()), (I32, done_str)],
                );
                ctx.record_lowered_value(
                    "IterResultSet",
                    None,
                    "compiler_private_async_iter_result_set_i32",
                    &raw,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec!["slot_kind=raw_i32_proven".to_string()],
                );
                Ok(result)
            } else if is_numeric_expr(ctx, value) {
                let (raw, slot_note) = lower_iter_result_f64_payload(ctx, value)?;
                let result = ctx.block().call(
                    DOUBLE,
                    "js_iter_result_set_f64",
                    &[(DOUBLE, raw.value.as_str()), (I32, done_str)],
                );
                ctx.record_lowered_value(
                    "IterResultSet",
                    None,
                    "compiler_private_async_iter_result_set_f64",
                    &raw,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![slot_note.to_string()],
                );
                Ok(result)
            } else {
                let v_box = lower_expr(ctx, value)?;
                let blk = ctx.block();
                Ok(blk.call(
                    DOUBLE,
                    "js_iter_result_set",
                    &[(DOUBLE, &v_box), (I32, done_str)],
                ))
            }
        }
        Expr::IterResultGetValue => Ok(ctx.block().call(DOUBLE, "js_iter_result_get_value", &[])),
        Expr::IterResultGetDone => {
            // Returns NaN-boxed bool (TAG_TRUE / TAG_FALSE) directly,
            // so it can be used in any conditional / property context
            // without a separate bool-to-JSValue conversion.
            Ok(ctx.block().call(DOUBLE, "js_iter_result_get_done", &[]))
        }

        // -------- Optimized async-step chain (perf hot path) --------
        // Equivalent to `Promise.resolve(value).then(v => step(v, false), e => step(e, true))`
        // but skips the wrapper-arrow allocations + dispatches.
        Expr::AsyncStepChain {
            value,
            step_closure,
        } => {
            let value_box = lower_expr(ctx, value)?;
            let step_box = lower_expr(ctx, step_closure)?;
            let blk = ctx.block();
            let step_handle = unbox_to_i64(blk, &step_box);
            let promise_handle = blk.call(
                I64,
                "js_async_step_chain",
                &[(DOUBLE, &value_box), (I64, &step_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &promise_handle))
        }

        // -------- Optimized async-step done (perf hot path) --------
        // Equivalent to `Promise.resolve(value)` at the state-machine
        // terminal position, but reuses the in-flight `next` Promise
        // (stashed in INLINE_TRAP_NEXT by the microtask runner) when
        // step is being dispatched. Saves one fresh Promise alloc per
        // async function call. Gated by step_closure matching
        // CURRENT_STEP_CLOSURE so nested async-fn calls can't accidentally
        // resolve the outer activation's `next`.
        Expr::AsyncStepDone {
            value,
            step_closure,
        } => {
            let value_box = lower_expr(ctx, value)?;
            let step_box = lower_expr(ctx, step_closure)?;
            let blk = ctx.block();
            let step_handle = unbox_to_i64(blk, &step_box);
            let promise_handle = blk.call(
                I64,
                "js_async_step_done",
                &[(DOUBLE, &value_box), (I64, &step_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &promise_handle))
        }

        // -------- #691 Phase 2: current step closure (self-ref) ----
        // Reads the live step closure pointer from INLINE_TRAP.current_step
        // TLS and NaN-boxes it. Only safe inside a step body or any
        // code wrapped by js_async_first_call.
        Expr::CurrentStepClosure => {
            let blk = ctx.block();
            let step_handle = blk.call(I64, "js_get_current_step_closure", &[]);
            Ok(nanbox_pointer_inline(blk, &step_handle))
        }

        // -------- #691 Phase 2: first invocation with TLS setup -----
        // Runtime helper takes the NaN-boxed closure pointer, saves
        // the previous INLINE_TRAP, sets current_step, calls
        // js_closure_call2(closure, undefined, false), then restores.
        Expr::AsyncFirstCall { step_closure } => {
            let step_box = lower_expr(ctx, step_closure)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_async_first_call", &[(DOUBLE, &step_box)]))
        }

        // -------- Object.getOwnPropertyNames(obj) --------
        // Returns ALL own keys (including non-enumerable ones from
        // defineProperty), unlike Object.keys which skips them.
        Expr::ObjectGetOwnPropertyNames(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr_box = blk.call(
                DOUBLE,
                "js_object_get_own_property_names",
                &[(DOUBLE, &obj_box)],
            );
            Ok(arr_box)
        }

        // -------- Math.hypot(...values) --------
        // Routes through `js_math_hypot(a, b)` which uses Rust's
        // `f64::hypot` (numerically stable for very large / very small
        // operands vs. the naive sqrt(a² + b²)). For 3+ args we chain:
        // hypot(a, b, c) ≡ hypot(hypot(a, b), c).
        Expr::MathHypot(values) => {
            if values.is_empty() {
                return Ok(double_literal(0.0));
            }
            if values.len() == 1 {
                let v = lower_math_operand(ctx, &values[0])?;
                // Math.hypot(x) = |x|
                return Ok(ctx.block().call(DOUBLE, "llvm.fabs.f64", &[(DOUBLE, &v)]));
            }
            let mut acc = lower_math_operand(ctx, &values[0])?;
            for v in &values[1..] {
                let rhs = lower_math_operand(ctx, v)?;
                let blk = ctx.block();
                acc = blk.call(DOUBLE, "js_math_hypot", &[(DOUBLE, &acc), (DOUBLE, &rhs)]);
            }
            Ok(acc)
        }

        // -------- RegExpExecGroups — reads thread-local from the last exec() call --------
        // Returns an ObjectHeader* (as raw i64); NaN-box with POINTER_TAG so
        // `lastExecResult.groups.year` reaches the generic object field path.
        // When no named groups were matched the runtime returns 0, which we
        // surface as TAG_UNDEFINED so `groups?.year` and `groups === undefined`
        // probes behave correctly.
        Expr::RegExpExecGroups => {
            let blk = ctx.block();
            let handle = blk.call(I64, "js_regexp_exec_get_groups", &[]);
            let is_zero = blk.icmp_eq(I64, &handle, "0");
            let ptr_boxed = nanbox_pointer_inline(ctx.block(), &handle);
            let ptr_bits = ctx.block().bitcast_double_to_i64(&ptr_boxed);
            let selected = ctx.block().select(
                I1,
                &is_zero,
                I64,
                crate::nanbox::TAG_UNDEFINED_I64,
                &ptr_bits,
            );
            Ok(ctx.block().bitcast_i64_to_double(&selected))
        }

        // -------- set.clear() --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
