//! NativeMethodCall + Call (all dispatch arms).
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
    static_type_of,
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

/// Whether a `createHash(...).update(e)` / `createHmac(alg, e)` argument is a
/// Buffer / Uint8Array — either a direct buffer-producing expression or a
/// local/field whose static type is `Buffer` / `Uint8Array`. Such inputs must
/// not take the inline `*StringHeader` hash fast path, whose UTF-8 string
/// unboxing reads the wrong bytes for a Buffer (#1354).
fn hash_input_is_buffer(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if matches!(
        e,
        Expr::BufferFrom { .. }
            | Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(_)
            | Expr::BufferConcat(_)
            | Expr::CryptoRandomBytes(_)
    ) {
        return true;
    }
    // `crypto.createSecretKey(...)` / `crypto.generateKeySync(...)` /
    // `crypto.pbkdf2Sync(...)` / `crypto.scryptSync(...)` / `crypto.hkdfSync(...)`
    // all return a BufferHeader (Uint8Array-marked) — the HIR cannot infer
    // that statically without this hint, so without it `createHmac(secretKey, ...)`
    // would route to the string fast-path that misreads buffer bytes as UTF-8.
    if let Expr::Call { callee, .. } = e {
        if let Expr::PropertyGet { object, property } = callee.as_ref() {
            if matches!(object.as_ref(), Expr::NativeModuleRef(n) if n == "crypto")
                && matches!(
                    property.as_str(),
                    "createSecretKey"
                        | "generateKeySync"
                        | "pbkdf2Sync"
                        | "scryptSync"
                        | "hkdfSync"
                        | "randomBytes"
                        | "randomFillSync"
                )
            {
                return true;
            }
        }
    }
    matches!(
        static_type_of(ctx, e),
        Some(HirType::Named(ref n)) if n == "Buffer" || n == "Uint8Array"
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::NativeMethodCall {
            module,
            class_name,
            method,
            object,
            args,
            ..
        } => lower_native_method_call(
            ctx,
            module,
            class_name.as_deref(),
            method,
            object.as_deref(),
            args,
        ),

        // Phase H crypto: collapse `crypto.createHash(alg).update(data).digest(enc)`
        // into a single runtime call. The HIR shape is a triple-nested
        // Call whose innermost callee is `NativeModuleRef("crypto")`.
        // Only "sha256" and "md5" algorithms have direct runtime
        // helpers (`js_crypto_sha256` / `js_crypto_md5`); other
        // algorithms fall through to the generic dispatch path.
        Expr::Call {
            callee: outer_callee,
            args: outer_args,
            ..
        } if matches!(
            outer_callee.as_ref(),
            Expr::PropertyGet { property: p, object } if p == "digest" && matches!(
                object.as_ref(),
                Expr::Call { callee: c2, .. } if matches!(
                    c2.as_ref(),
                    Expr::PropertyGet { property: p2, object: obj2 } if p2 == "update" && matches!(
                        obj2.as_ref(),
                        Expr::Call { callee: c3, .. } if matches!(
                            c3.as_ref(),
                            Expr::PropertyGet { property: p3, object: obj3 } if (p3 == "createHash" || p3 == "Hash" || p3 == "createHmac" || p3 == "Hmac") && matches!(
                                obj3.as_ref(),
                                Expr::NativeModuleRef(n) if n == "crypto"
                            )
                        )
                    )
                )
            )
        ) =>
        {
            // Walk the chain to extract: alg (from createHash/Hash/createHmac/Hmac args),
            // key (from createHmac's second arg, if present),
            // data (from update args), enc (from digest args).
            let digest_args = outer_args;
            let update_call = if let Expr::PropertyGet { object, .. } = outer_callee.as_ref() {
                object.as_ref()
            } else {
                unreachable!()
            };
            let (update_args, create_call) = if let Expr::Call {
                callee: uc,
                args: ua,
                ..
            } = update_call
            {
                let inner = if let Expr::PropertyGet { object, .. } = uc.as_ref() {
                    object.as_ref()
                } else {
                    unreachable!()
                };
                (ua.as_slice(), inner)
            } else {
                unreachable!()
            };
            let (create_method, create_args) = if let Expr::Call {
                callee: cc,
                args: ca,
                ..
            } = create_call
            {
                let m = if let Expr::PropertyGet { property, .. } = cc.as_ref() {
                    property.as_str()
                } else {
                    unreachable!()
                };
                (m, ca.as_slice())
            } else {
                unreachable!()
            };

            // Determine algorithm from the first arg of createHash/createHmac.
            let alg = if let Some(Expr::String(s)) = create_args.first() {
                s.as_str()
            } else {
                ""
            };

            // `.digest()` (no arg) returns a Buffer of the raw digest bytes;
            // `.digest('hex')` returns a hex string. SCRAM (and any binary
            // crypto workload) needs the Buffer path — it XORs, hashes, and
            // base64-encodes raw bytes. Route to _bytes FFI variants when no
            // encoding was specified.
            let want_buffer = digest_args.first().is_none()
                || matches!(digest_args.first(), Some(Expr::Undefined));

            // The inline `js_crypto_sha256` / `js_crypto_md5` fast path only
            // produces a hex string (or, for the no-arg form, a raw-byte
            // Buffer). Any other digest encoding (`'base64'`, `'base64url'`,
            // …) must fall through to the runtime handle dispatch, whose
            // `dispatch_hash` honors the encoding (#1352). A non-literal
            // encoding arg also can't be folded inline.
            let enc_fast_ok = match digest_args.first() {
                None | Some(Expr::Undefined) => true,
                Some(Expr::String(s)) => s.eq_ignore_ascii_case("hex"),
                _ => false,
            };
            // The inline path unboxes the data/key as a `*StringHeader` and
            // hashes the UTF-8 string bytes. A Buffer / Uint8Array input has a
            // different header layout, so hashing it through the string path
            // reads the wrong bytes (#1354). Route Buffer-typed inputs to the
            // handle dispatch, whose `bytes_from_ptr` reads either layout.
            // Detect both inline buffer-producing expressions (`Buffer.from(…)`,
            // `crypto.randomBytes(…)`, …) and locals/fields whose static type
            // is Buffer / Uint8Array (see `hash_input_is_buffer`). Each borrow
            // of `ctx` is scoped to the `is_some_and` call so it does not
            // collide with the `&mut ctx` borrows in the arm bodies.
            let data_is_buffer = update_args
                .first()
                .is_some_and(|e| hash_input_is_buffer(ctx, e));
            let key_is_buffer = create_args
                .get(1)
                .is_some_and(|e| hash_input_is_buffer(ctx, e));
            // The fast paths below unbox the data/key via `unbox_str_handle`
            // and hash the raw `StringHeader` bytes. A literal string is
            // statically known to be a `StringHeader`; any non-literal
            // (Call, Identifier, PropertyGet, ...) may resolve to a Buffer
            // or KeyObject at runtime (e.g. `crypto.createSecretKey(...)`),
            // which `hash_input_is_buffer` cannot detect from the HIR alone.
            // Tightening to literal-string keys/data closes that gap (this
            // restores PR #1419's original gating). Non-literal cases drop
            // through to the handle-dispatch fallback that calls
            // `bytes_from_ptr` and reads either layout correctly.
            let data_is_literal_string = matches!(update_args.first(), Some(Expr::String(_)));
            let key_is_literal_string = matches!(create_args.get(1), Some(Expr::String(_)));
            let fast_ok = enc_fast_ok && !data_is_buffer && data_is_literal_string;
            let hmac_fast_ok = fast_ok && !key_is_buffer && key_is_literal_string;

            match (create_method, alg) {
                ("createHash", "sha256") if fast_ok && update_args.len() == 1 => {
                    let data_box = lower_expr(ctx, &update_args[0])?;
                    let blk = ctx.block();
                    // SSO-safe data unbox — both `js_crypto_sha256` and the
                    // `_bytes` variant deref as `*StringHeader`. #214 class.
                    let data_handle = unbox_str_handle(blk, &data_box);
                    if want_buffer {
                        let result =
                            blk.call(I64, "js_crypto_sha256_bytes", &[(I64, &data_handle)]);
                        Ok(nanbox_pointer_inline(blk, &result))
                    } else {
                        let result = blk.call(I64, "js_crypto_sha256", &[(I64, &data_handle)]);
                        Ok(nanbox_string_inline(blk, &result))
                    }
                }
                ("createHash", "md5") if fast_ok && update_args.len() == 1 => {
                    let data_box = lower_expr(ctx, &update_args[0])?;
                    let blk = ctx.block();
                    // SSO-safe — see sha256 arm above.
                    let data_handle = unbox_str_handle(blk, &data_box);
                    let result = blk.call(I64, "js_crypto_md5", &[(I64, &data_handle)]);
                    Ok(nanbox_string_inline(blk, &result))
                }
                ("createHmac", "sha256")
                    if hmac_fast_ok && create_args.len() >= 2 && update_args.len() == 1 =>
                {
                    let key_box = lower_expr(ctx, &create_args[1])?;
                    let data_box = lower_expr(ctx, &update_args[0])?;
                    let blk = ctx.block();
                    // SSO-safe — both runtime fns deref as `*StringHeader`.
                    let key_handle = unbox_str_handle(blk, &key_box);
                    let data_handle = unbox_str_handle(blk, &data_box);
                    if want_buffer {
                        let result = blk.call(
                            I64,
                            "js_crypto_hmac_sha256_bytes",
                            &[(I64, &key_handle), (I64, &data_handle)],
                        );
                        Ok(nanbox_pointer_inline(blk, &result))
                    } else {
                        let result = blk.call(
                            I64,
                            "js_crypto_hmac_sha256",
                            &[(I64, &key_handle), (I64, &data_handle)],
                        );
                        Ok(nanbox_string_inline(blk, &result))
                    }
                }
                _ => {
                    // Fallback for non-literal alg (#1076) and for algorithms
                    // we don't have a direct FFI helper for (sha1, sha512,
                    // md5 for HMAC; sha1, sha512 for hash). Route through
                    // the same handle protocol the standalone `createHash`
                    // / `createHmac` arms use: allocate a Hash/Hmac handle,
                    // chain `.update(data).digest(enc)` via runtime method
                    // dispatch. Previously this arm returned `""` silently —
                    // see #1076 (HMAC signature verification always failing
                    // when `alg` was a `const`-bound or for-of-bound name).
                    if create_args.is_empty() || update_args.is_empty() {
                        // Mirror the legacy empty-string return for malformed
                        // input so downstream chains keep their shape.
                        let blk = ctx.block();
                        let empty =
                            blk.call(I64, "js_string_from_bytes", &[(I64, "0"), (I32, "0")]);
                        return Ok(nanbox_string_inline(blk, &empty));
                    }
                    // Lower all the sub-expressions before any FFI call so
                    // their side-effects run in the source order Node sees.
                    let alg_box = lower_expr(ctx, &create_args[0])?;
                    let key_box_opt = if (create_method == "createHmac" || create_method == "Hmac")
                        && create_args.len() >= 2
                    {
                        Some(lower_expr(ctx, &create_args[1])?)
                    } else {
                        None
                    };
                    let hash_options_box_opt = if (create_method == "createHash"
                        || create_method == "Hash")
                        && create_args.len() >= 2
                    {
                        Some(lower_expr(ctx, &create_args[1])?)
                    } else {
                        None
                    };
                    let data_box = lower_expr(ctx, &update_args[0])?;
                    let update_encoding_box_opt = if update_args.len() >= 2 {
                        Some(lower_expr(ctx, &update_args[1])?)
                    } else {
                        None
                    };
                    let enc_box_opt = if digest_args.is_empty() {
                        None
                    } else {
                        Some(lower_expr(ctx, &digest_args[0])?)
                    };

                    let blk = ctx.block();
                    let alg_handle = unbox_to_i64(blk, &alg_box);
                    // Allocate the handle. Both helpers return f64 already
                    // NaN-boxed with POINTER_TAG, suitable as the receiver
                    // for `js_native_call_method`.
                    let recv = if create_method == "createHmac" || create_method == "Hmac" {
                        let key_box = key_box_opt.expect("createHmac needs a key arg");
                        let key_handle = unbox_to_i64(blk, &key_box);
                        blk.call(
                            DOUBLE,
                            "js_crypto_create_hmac",
                            &[(I64, &alg_handle), (I64, &key_handle)],
                        )
                    } else {
                        if let Some(options_box) = hash_options_box_opt {
                            blk.call(
                                DOUBLE,
                                "js_crypto_create_hash_options",
                                &[(I64, &alg_handle), (DOUBLE, &options_box)],
                            )
                        } else {
                            blk.call(DOUBLE, "js_crypto_create_hash", &[(I64, &alg_handle)])
                        }
                    };

                    // Invoke `.update(data[, inputEncoding])` via the runtime's generic
                    // handle-method dispatcher.
                    let update_name = emit_string_literal_global(ctx, "update");
                    let update_argc_usize = if update_encoding_box_opt.is_some() {
                        2
                    } else {
                        1
                    };
                    let update_argc = update_argc_usize.to_string();
                    let blk = ctx.block();
                    let update_args_buf = ctx.func.alloca_entry_array(DOUBLE, update_argc_usize);
                    {
                        let blk = ctx.block();
                        let slot = blk.gep(DOUBLE, &update_args_buf, &[(I64, "0")]);
                        blk.store(DOUBLE, &data_box, &slot);
                        if let Some(update_encoding_box) = update_encoding_box_opt.as_ref() {
                            let slot = blk.gep(DOUBLE, &update_args_buf, &[(I64, "1")]);
                            blk.store(DOUBLE, update_encoding_box, &slot);
                        }
                    }
                    let update_args_ptr = {
                        let blk = ctx.block();
                        let reg = blk.next_reg();
                        blk.emit_raw(format!(
                            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                            reg, update_argc_usize, update_args_buf
                        ));
                        reg
                    };
                    let blk = ctx.block();
                    let updated = blk.call(
                        DOUBLE,
                        "js_native_call_method",
                        &[
                            (DOUBLE, &recv),
                            (PTR, &update_name),
                            (I64, &format!("{}", "update".len())),
                            (PTR, &update_args_ptr),
                            (I64, &update_argc),
                        ],
                    );

                    // Invoke `.digest(enc?)` — 0 or 1 args.
                    let digest_name = emit_string_literal_global(ctx, "digest");
                    let (digest_args_ptr, digest_argc) = if let Some(enc_box) = enc_box_opt {
                        let buf = ctx.func.alloca_entry_array(DOUBLE, 1);
                        {
                            let blk = ctx.block();
                            let slot = blk.gep(DOUBLE, &buf, &[(I64, "0")]);
                            blk.store(DOUBLE, &enc_box, &slot);
                        }
                        let blk = ctx.block();
                        let reg = blk.next_reg();
                        blk.emit_raw(format!(
                            "{} = getelementptr [1 x double], ptr {}, i64 0, i64 0",
                            reg, buf
                        ));
                        (reg, "1".to_string())
                    } else {
                        ("null".to_string(), "0".to_string())
                    };
                    let blk = ctx.block();
                    let result = blk.call(
                        DOUBLE,
                        "js_native_call_method",
                        &[
                            (DOUBLE, &updated),
                            (PTR, &digest_name),
                            (I64, &format!("{}", "digest".len())),
                            (PTR, &digest_args_ptr),
                            (I64, &digest_argc),
                        ],
                    );
                    Ok(result)
                }
            }
        }

        // Standalone `crypto.createHash(alg)` / legacy callable
        // `crypto.Hash(alg)` — when the user binds the
        // result to a local before calling `.update(...)` / `.digest()`,
        // the three-level chain-collapse above no longer matches and this
        // arm runs instead. It registers a HashHandle in perry-stdlib and
        // returns a small-integer handle NaN-boxed as POINTER_TAG.
        // `js_native_call_method` routes subsequent method calls on that
        // handle through `HANDLE_METHOD_DISPATCH` → `dispatch_hash`. See
        // `perry-stdlib/src/crypto.rs::js_crypto_create_hash`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "createHash" || property == "Hash") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let options_box = if args.len() >= 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            // Returns an already-NaN-boxed f64 (POINTER_TAG + handle id).
            if let Some(options_box) = options_box {
                Ok(blk.call(
                    DOUBLE,
                    "js_crypto_create_hash_options",
                    &[(I64, &alg_handle), (DOUBLE, &options_box)],
                ))
            } else {
                Ok(blk.call(DOUBLE, "js_crypto_create_hash", &[(I64, &alg_handle)]))
            }
        }

        // `crypto.createSign(alg)` / legacy `crypto.Sign(alg)` and
        // `crypto.createVerify(alg)` / legacy `crypto.Verify(alg)` streaming
        // RSA signature handles.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "createSign" || property == "Sign" || property == "createVerify" || property == "Verify") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            let alg_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let fname = if property == "createSign" || property == "Sign" {
                "js_crypto_create_sign"
            } else {
                "js_crypto_create_verify"
            };
            Ok(blk.call(DOUBLE, fname, &[(I64, &alg_handle)]))
        }

        // `crypto.createECDH(curve)` — Node-compatible ECDH handle. The
        // runtime currently covers the high-value P-256 aliases used by
        // Node/Bun/Deno parity tests: prime256v1, secp256r1, P-256.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "createECDH" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let curve_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let curve_handle = unbox_to_i64(blk, &curve_box);
            Ok(blk.call(DOUBLE, "js_crypto_create_ecdh", &[(I64, &curve_handle)]))
        }

        // `crypto.createDiffieHellman(...)` / `crypto.getDiffieHellman(name)`
        // / `crypto.createDiffieHellmanGroup(name)` classic DH handles.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "createDiffieHellman" || property == "getDiffieHellman" || property == "createDiffieHellmanGroup") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            if property == "getDiffieHellman" || property == "createDiffieHellmanGroup" {
                let group = if let Some(arg) = args.first() {
                    lower_expr(ctx, arg)?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                return Ok(blk.call(DOUBLE, "js_crypto_get_diffie_hellman", &[(DOUBLE, &group)]));
            }
            let first = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let second = if let Some(arg) = args.get(1) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let third = if let Some(arg) = args.get(2) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_crypto_create_diffie_hellman",
                &[(DOUBLE, &first), (DOUBLE, &second), (DOUBLE, &third)],
            ))
        }

        // Minimal KeyObject-compatible input path:
        // `createPrivateKey(pem)` returns the PEM surrogate directly, while
        // `createPublicKey(privateOrPublicPem)` derives a public PEM string.
        // The asymmetric native helpers accept these PEM surrogates as keys.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "createPrivateKey" || property == "createPublicKey") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            let key_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let fname = if property == "createPrivateKey" {
                "js_crypto_create_private_key_value"
            } else {
                "js_crypto_create_public_key_value"
            };
            let pem = blk.call(I64, fname, &[(DOUBLE, &key_box)]);
            Ok(nanbox_string_inline(blk, &pem))
        }

        // `crypto.generateKeyPair("rsa"|"ec"|"ed25519"|"x25519", options,
        // callback)` — callback form. Native shim invokes `(err, publicKey,
        // privateKey)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "generateKeyPair" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 3 =>
        {
            let alg_box = lower_expr(ctx, &args[0])?;
            let options = lower_expr(ctx, &args[1])?;
            let callback = lower_expr(ctx, &args[2])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_generate_key_pair_async",
                &[(I64, &alg_handle), (DOUBLE, &options), (DOUBLE, &callback)],
            ))
        }

        // `crypto.generateKeyPairSync("rsa", { ...pem encodings... })` —
        // returns a plain object with `publicKey`/`privateKey` PEM strings.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "generateKeyPairSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            let options = if let Some(arg) = args.get(1) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let fname = match args.first() {
                Some(Expr::String(alg)) if alg == "ec" => {
                    "js_crypto_generate_key_pair_sync_ec_p256"
                }
                Some(Expr::String(alg)) if alg == "ed25519" => {
                    "js_crypto_generate_key_pair_sync_ed25519"
                }
                Some(Expr::String(alg)) if alg == "x25519" => {
                    "js_crypto_generate_key_pair_sync_x25519"
                }
                _ => "js_crypto_generate_key_pair_sync_rsa",
            };
            let pair = blk.call(I64, fname, &[(DOUBLE, &options)]);
            Ok(nanbox_pointer_inline(blk, &pair))
        }

        // `crypto.diffieHellman({ privateKey, publicKey })` — currently
        // covers the high-value X25519 stateless DH path from Node/Bun.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "diffieHellman" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let options = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let secret = blk.call(I64, "js_crypto_diffie_hellman", &[(DOUBLE, &options)]);
            Ok(nanbox_pointer_inline(blk, &secret))
        }

        // Standalone `crypto.createHmac(alg, key)` / legacy
        // callable `crypto.Hmac(alg, key)` — same shape as
        // `createHash` above. Closes #1076 for the `const h = createHmac(...)`
        // / for-of patterns where the chain-collapse can't match because
        // `.update()` / `.digest()` happen on subsequent statements (or
        // because the alg isn't a literal the fast path recognizes).
        // `js_crypto_create_hmac` returns a NaN-boxed handle; dispatch_hmac
        // (registered in `perry-stdlib/src/common/dispatch.rs`) handles the
        // method routing.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "createHmac" || property == "Hmac") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 2 {
                // Lower whatever's there to honor side effects, then
                // return undefined — Node throws here, but our other
                // crypto arms degrade gracefully rather than panic.
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let key_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let key_handle = unbox_to_i64(blk, &key_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_create_hmac",
                &[(I64, &alg_handle), (I64, &key_handle)],
            ))
        }

        // `crypto.createCipheriv(alg, key, iv)` / `crypto.createDecipheriv(...)`
        // (issue #1075) — registers a CipherHandle in perry-stdlib and
        // returns a small-integer handle NaN-boxed as POINTER_TAG. The
        // runtime's HANDLE_METHOD_DISPATCH then routes subsequent
        // `.update(buf)` / `.final()` / `.getAuthTag()` / `.setAuthTag(tag)`
        // through `dispatch_cipher`. Supports aes-128-cbc, aes-256-cbc,
        // aes-128-gcm, aes-256-gcm. See
        // `perry-stdlib/src/crypto.rs::js_crypto_create_cipheriv`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property }
                    if (property == "createCipheriv" || property == "createDecipheriv")
                        && matches!(
                            object.as_ref(),
                            Expr::NativeModuleRef(n) if n == "crypto"
                        )
            ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            if args.len() < 3 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let key_box = lower_expr(ctx, &args[1])?;
            let iv_box = lower_expr(ctx, &args[2])?;
            let options_box = if let Some(options) = args.get(3) {
                lower_expr(ctx, options)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let key_handle = unbox_to_i64(blk, &key_box);
            let iv_handle = unbox_to_i64(blk, &iv_box);
            let fname = if property == "createCipheriv" {
                "js_crypto_create_cipheriv"
            } else {
                "js_crypto_create_decipheriv"
            };
            // Returns an already-NaN-boxed f64 (POINTER_TAG + handle id).
            Ok(blk.call(
                DOUBLE,
                fname,
                &[
                    (I64, &alg_handle),
                    (I64, &key_handle),
                    (I64, &iv_handle),
                    (DOUBLE, &options_box),
                ],
            ))
        }

        // `crypto.randomBytes(size, callback)` — callback form. Perry
        // invokes the callback synchronously in the native shim, but keeps
        // Node's `(err, buffer)` shape.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "randomBytes" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 2 =>
        {
            let size_box = lower_expr(ctx, &args[0])?;
            let cb_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_crypto_random_bytes_async",
                &[(DOUBLE, &size_box), (DOUBLE, &cb_box)],
            ))
        }

        // `crypto.randomFill(buffer[, offset][, size], callback)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "randomFill" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) && args.len() >= 2 =>
        {
            let last = args.len() - 1;
            let buf_box = lower_expr(ctx, &args[0])?;
            let off_box = if last >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let sz_box = if last >= 3 {
                lower_expr(ctx, &args[2])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let cb_box = lower_expr(ctx, &args[last])?;
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_crypto_random_fill_async",
                &[
                    (DOUBLE, &buf_box),
                    (DOUBLE, &off_box),
                    (DOUBLE, &sz_box),
                    (DOUBLE, &cb_box),
                ],
            ))
        }

        // `crypto.createSign(alg)` / `crypto.createVerify(alg)` (#1364) —
        // registers a SignHandle and returns a small-integer handle NaN-boxed
        // as POINTER_TAG. HANDLE_METHOD_DISPATCH then routes `.update(d)` /
        // `.sign(key, enc?)` / `.verify(key, sig, enc?)` through
        // `dispatch_sign`. RSA PKCS#1 v1.5 over sha1/224/256/384/512.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property }
                    if (property == "createSign" || property == "createVerify")
                        && matches!(
                            object.as_ref(),
                            Expr::NativeModuleRef(n) if n == "crypto"
                        )
            ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let fname = if property == "createSign" {
                "js_crypto_create_sign"
            } else {
                "js_crypto_create_verify"
            };
            // Returns an already-NaN-boxed f64 (POINTER_TAG + handle id).
            Ok(blk.call(DOUBLE, fname, &[(I64, &alg_handle)]))
        }

        // Phase H crypto: `crypto.randomBytes(n)` as a Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "randomBytes" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(0.0));
            }
            let size_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let buf_handle = blk.call(I64, "js_crypto_random_bytes_buffer", &[(DOUBLE, &size_box)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // Phase H crypto: `crypto.randomUUID()`.
        Expr::Call {
            callee, args: _, ..
        } if matches!(
            callee.as_ref(),
            Expr::PropertyGet { object, property } if property == "randomUUID" && matches!(
                object.as_ref(),
                Expr::NativeModuleRef(n) if n == "crypto"
            )
        ) =>
        {
            let blk = ctx.block();
            let handle = blk.call(I64, "js_crypto_random_uuid", &[]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // Phase H crypto: `crypto.randomInt([min,] max[, callback])` —
        // uniform integer in `[min, max)`. The single-arg form defaults
        // `min` to 0. The runtime returns the value as a plain double (a
        // JS number), so no NaN-box is needed at the call site. The
        // 3-arg callback form preserves Node's `(err, n)` shape and
        // returns `undefined`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "randomInt" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(0.0));
            }
            let zero = Expr::Integer(0);
            let (min_expr, max_expr, callback_expr) = match args.len() {
                1 => (&zero, &args[0], None),
                2 => (&args[0], &args[1], None),
                _ => (&args[0], &args[1], Some(&args[2])),
            };
            let min_box = lower_expr(ctx, min_expr)?;
            let max_box = lower_expr(ctx, max_expr)?;
            let callback_box = if let Some(callback_expr) = callback_expr {
                Some(lower_expr(ctx, callback_expr)?)
            } else {
                None
            };
            let blk = ctx.block();
            if let Some(callback_box) = callback_box {
                return Ok(blk.call(
                    DOUBLE,
                    "js_crypto_random_int_async",
                    &[
                        (DOUBLE, &min_box),
                        (DOUBLE, &max_box),
                        (DOUBLE, &callback_box),
                    ],
                ));
            }
            Ok(blk.call(
                DOUBLE,
                "js_crypto_random_int",
                &[(DOUBLE, &min_box), (DOUBLE, &max_box)],
            ))
        }

        // Phase H crypto: `crypto.timingSafeEqual(a, b)` — constant-time
        // compare of two byte sequences. Returns a NaN-boxed boolean.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "timingSafeEqual" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 2 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let a_box = lower_expr(ctx, &args[0])?;
            let b_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let a_handle = unbox_to_i64(blk, &a_box);
            let b_handle = unbox_to_i64(blk, &b_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_timing_safe_equal",
                &[(I64, &a_handle), (I64, &b_handle)],
            ))
        }

        // Prime generation/checking APIs used by Node's crypto prime suite.
        // Perry covers practical Buffer-returning shapes plus callback forms:
        //   generatePrimeSync(size, options?)
        //   generatePrime(size, options, callback)
        //   checkPrimeSync(candidate, options?)
        //   checkPrime(candidate, options, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if matches!(property.as_str(), "generatePrimeSync" | "generatePrime" | "checkPrimeSync" | "checkPrime") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            let first_box = lower_expr(ctx, &args[0])?;
            let options_box = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let callback_box =
                if matches!(property, "generatePrime" | "checkPrime") && args.len() >= 3 {
                    Some(lower_expr(ctx, &args[2])?)
                } else {
                    None
                };
            let blk = ctx.block();
            let is_generate = property == "generatePrime" || property == "generatePrimeSync";
            if let Some(callback_box) = callback_box {
                let fname = if is_generate {
                    "js_crypto_generate_prime_async"
                } else {
                    "js_crypto_check_prime_async"
                };
                return Ok(blk.call(
                    DOUBLE,
                    fname,
                    &[
                        (DOUBLE, &first_box),
                        (DOUBLE, &options_box),
                        (DOUBLE, &callback_box),
                    ],
                ));
            }
            if is_generate {
                let buf = blk.call(
                    I64,
                    "js_crypto_generate_prime_sync",
                    &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
                );
                Ok(nanbox_pointer_inline(blk, &buf))
            } else {
                Ok(blk.call(
                    DOUBLE,
                    "js_crypto_check_prime_sync",
                    &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
                ))
            }
        }

        // `crypto.getHashes()` / `getCiphers()` / `getCurves()` — stable
        // deterministic inventories used for feature detection. The runtime
        // helper returns an ArrayHeader pointer.
        Expr::Call {
            callee, args: _, ..
        } if matches!(
            callee.as_ref(),
            Expr::PropertyGet { object, property } if matches!(property.as_str(), "getHashes" | "getCiphers" | "getCurves") && matches!(
                object.as_ref(),
                Expr::NativeModuleRef(n) if n == "crypto"
            )
        ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            let fname = match property {
                "getHashes" => "js_crypto_get_hashes",
                "getCiphers" => "js_crypto_get_ciphers",
                _ => "js_crypto_get_curves",
            };
            let blk = ctx.block();
            let arr = blk.call(I64, fname, &[]);
            Ok(nanbox_pointer_inline(blk, &arr))
        }

        // `crypto.getCipherInfo(algorithm, options?)` — feature detection
        // for supported symmetric ciphers.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "getCipherInfo" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let options_box = if let Some(arg) = args.get(1) {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            Ok(blk.call(
                DOUBLE,
                "js_crypto_get_cipher_info",
                &[(DOUBLE, &alg_box), (DOUBLE, &options_box)],
            ))
        }

        // `crypto.getFips()` — Perry does not expose OpenSSL FIPS mode.
        Expr::Call {
            callee, args: _, ..
        } if matches!(
            callee.as_ref(),
            Expr::PropertyGet { object, property } if property == "getFips" && matches!(
                object.as_ref(),
                Expr::NativeModuleRef(n) if n == "crypto"
            )
        ) =>
        {
            Ok(double_literal(0.0))
        }

        // `crypto.setFips(false|0)` — Perry has no OpenSSL FIPS mode, so
        // accepting the disabling no-op matches Node's default environment.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "setFips" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // `crypto.secureHeapUsed()` — default Node shape when secure heap
        // is not enabled: { total: 0, used: 0, utilization: 0, min: 0 }.
        Expr::Call {
            callee, args: _, ..
        } if matches!(
            callee.as_ref(),
            Expr::PropertyGet { object, property } if property == "secureHeapUsed" && matches!(
                object.as_ref(),
                Expr::NativeModuleRef(n) if n == "crypto"
            )
        ) =>
        {
            let blk = ctx.block();
            let obj = blk.call(I64, "js_crypto_secure_heap_used", &[]);
            Ok(nanbox_pointer_inline(blk, &obj))
        }

        // One-shot asymmetric signing/verification. Initial native parity
        // coverage supports Node's common RSA-SHA256/RSASSA-PKCS1-v1_5 PEM
        // path and returns a Buffer / boolean respectively.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "sign" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 3 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let data_box = lower_expr(ctx, &args[1])?;
            let key_box = lower_expr(ctx, &args[2])?;
            let callback_box = if args.len() >= 4 {
                Some(lower_expr(ctx, &args[3])?)
            } else {
                None
            };
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let data_handle = unbox_to_i64(blk, &data_box);
            if let Some(callback_box) = callback_box {
                return Ok(blk.call(
                    DOUBLE,
                    "js_crypto_sign_async",
                    &[
                        (I64, &alg_handle),
                        (I64, &data_handle),
                        (DOUBLE, &key_box),
                        (DOUBLE, &callback_box),
                    ],
                ));
            }
            let buf_handle = blk.call(
                I64,
                "js_crypto_sign_rsa_sha256",
                &[(I64, &alg_handle), (I64, &data_handle), (DOUBLE, &key_box)],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "verify" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 4 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_FALSE)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let data_box = lower_expr(ctx, &args[1])?;
            let key_box = lower_expr(ctx, &args[2])?;
            let sig_box = lower_expr(ctx, &args[3])?;
            let callback_box = if args.len() >= 5 {
                Some(lower_expr(ctx, &args[4])?)
            } else {
                None
            };
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let data_handle = unbox_to_i64(blk, &data_box);
            let sig_handle = unbox_to_i64(blk, &sig_box);
            if let Some(callback_box) = callback_box {
                return Ok(blk.call(
                    DOUBLE,
                    "js_crypto_verify_async",
                    &[
                        (I64, &alg_handle),
                        (I64, &data_handle),
                        (DOUBLE, &key_box),
                        (I64, &sig_handle),
                        (DOUBLE, &callback_box),
                    ],
                ));
            }
            Ok(blk.call(
                DOUBLE,
                "js_crypto_verify_rsa_sha256",
                &[
                    (I64, &alg_handle),
                    (I64, &data_handle),
                    (DOUBLE, &key_box),
                    (I64, &sig_handle),
                ],
            ))
        }

        // RSA encryption/decryption one-shot APIs. Covers the common
        // Node/Bun `publicEncrypt(key, data)` → `privateDecrypt(key, data)`
        // default OAEP roundtrip and `privateEncrypt` → `publicDecrypt`
        // PKCS#1 v1.5 transform for PEM keys.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if (property == "publicEncrypt" || property == "privateDecrypt" || property == "privateEncrypt" || property == "publicDecrypt") && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 2 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            let key_box = lower_expr(ctx, &args[0])?;
            let data_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let key_converter = match property {
                "publicEncrypt" | "publicDecrypt" => "js_crypto_create_public_key_value",
                "privateDecrypt" | "privateEncrypt" => "js_crypto_create_private_key_value",
                _ => unreachable!(),
            };
            let key_handle = blk.call(I64, key_converter, &[(DOUBLE, &key_box)]);
            let data_handle = unbox_to_i64(blk, &data_box);
            let fname = match property {
                "publicEncrypt" => "js_crypto_public_encrypt",
                "privateDecrypt" => "js_crypto_private_decrypt",
                "privateEncrypt" => "js_crypto_private_encrypt",
                "publicDecrypt" => "js_crypto_public_decrypt",
                _ => unreachable!(),
            };
            let buf_handle = blk.call(I64, fname, &[(I64, &key_handle), (I64, &data_handle)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // `crypto.createSecretKey(key, encoding?)` — JWT signing key for
        // HS* algorithms. Native-side this returns a Uint8Array-marked
        // BufferHeader; the bridge then materializes a real v8::Uint8Array
        // when the value crosses into a V8-fallback module (jose). See
        // `js_crypto_create_secret_key` for the encoding handling.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "createSecretKey" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let key_box = lower_expr(ctx, &args[0])?;
            let enc_box = if args.len() >= 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let key_handle = unbox_to_i64(blk, &key_box);
            let enc_handle = if let Some(enc) = enc_box {
                unbox_to_i64(blk, &enc)
            } else {
                "0".to_string()
            };
            let buf_handle = blk.call(
                I64,
                "js_crypto_create_secret_key",
                &[(I64, &key_handle), (I64, &enc_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // `crypto.generateKeySync("aes"|"hmac", { length })` — returns a
        // secret KeyObject-shaped BufferHeader, matching createSecretKey's
        // property/export/equality surface.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "generateKeySync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 2 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let options_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let buf_handle = blk.call(
                I64,
                "js_crypto_generate_key_sync",
                &[(I64, &alg_handle), (DOUBLE, &options_box)],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // `crypto.generateKey("aes"|"hmac", { length }, cb)` — async Node
        // shape. Perry computes synchronously and invokes the callback with
        // `(null, key)`, matching the observable parity tests.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "generateKey" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 3 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let options_box = lower_expr(ctx, &args[1])?;
            let cb_box = lower_expr(ctx, &args[2])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_generate_key_async",
                &[
                    (I64, &alg_handle),
                    (DOUBLE, &options_box),
                    (DOUBLE, &cb_box),
                ],
            ))
        }

        // crypto.hkdfSync(algorithm, ikm, salt, info, keylen) -> Buffer.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "hkdfSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 5 {
                return Ok(double_literal(0.0));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let ikm_box = lower_expr(ctx, &args[1])?;
            let salt_box = lower_expr(ctx, &args[2])?;
            let info_box = lower_expr(ctx, &args[3])?;
            let len_box = lower_expr(ctx, &args[4])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let ikm_handle = unbox_to_i64(blk, &ikm_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let info_handle = unbox_to_i64(blk, &info_box);
            let buf_handle = blk.call(
                I64,
                "js_crypto_hkdf_bytes_alg",
                &[
                    (I64, &alg_handle),
                    (I64, &ikm_handle),
                    (I64, &salt_handle),
                    (I64, &info_handle),
                    (DOUBLE, &len_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // crypto.hkdf(algorithm, ikm, salt, info, keylen, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "hkdf" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 6 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let alg_box = lower_expr(ctx, &args[0])?;
            let ikm_box = lower_expr(ctx, &args[1])?;
            let salt_box = lower_expr(ctx, &args[2])?;
            let info_box = lower_expr(ctx, &args[3])?;
            let len_box = lower_expr(ctx, &args[4])?;
            let cb_box = lower_expr(ctx, &args[5])?;
            let blk = ctx.block();
            let alg_handle = unbox_to_i64(blk, &alg_box);
            let ikm_handle = unbox_to_i64(blk, &ikm_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let info_handle = unbox_to_i64(blk, &info_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_hkdf_async_alg",
                &[
                    (I64, &alg_handle),
                    (I64, &ikm_handle),
                    (I64, &salt_handle),
                    (I64, &info_handle),
                    (DOUBLE, &len_box),
                    (DOUBLE, &cb_box),
                ],
            ))
        }

        // crypto.scrypt(password, salt, keylen[, options], callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "scrypt" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 4 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let pwd_box = lower_expr(ctx, &args[0])?;
            let salt_box = lower_expr(ctx, &args[1])?;
            let len_box = lower_expr(ctx, &args[2])?;
            let cb_expr = if args.len() >= 5 {
                let _ = lower_expr(ctx, &args[3])?;
                &args[4]
            } else {
                &args[3]
            };
            let cb_box = lower_expr(ctx, cb_expr)?;
            let blk = ctx.block();
            let pwd_handle = unbox_to_i64(blk, &pwd_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_scrypt_async",
                &[
                    (I64, &pwd_handle),
                    (I64, &salt_handle),
                    (DOUBLE, &len_box),
                    (DOUBLE, &cb_box),
                ],
            ))
        }

        // crypto.pbkdf2Sync(password, salt, iterations, keylen, digest) -> Buffer.
        // The digest algorithm (sha256/sha512/sha224/sha384/sha1) is passed
        // through to the runtime so non-SHA256 keys derive correctly (#1355).
        // An absent digest arg passes a null pointer; the runtime defaults to
        // SHA-256 (what SCRAM relies on).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "pbkdf2Sync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 4 {
                return Ok(double_literal(0.0));
            }
            let pwd_box = lower_expr(ctx, &args[0])?;
            let salt_box = lower_expr(ctx, &args[1])?;
            let iter_box = lower_expr(ctx, &args[2])?;
            let keylen_box = lower_expr(ctx, &args[3])?;
            let digest_box = if args.len() >= 5 {
                Some(lower_expr(ctx, &args[4])?)
            } else {
                None
            };
            let blk = ctx.block();
            let pwd_handle = unbox_to_i64(blk, &pwd_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let digest_handle = match &digest_box {
                Some(b) => unbox_to_i64(blk, b),
                None => "0".to_string(),
            };
            let buf_handle = blk.call(
                I64,
                "js_crypto_pbkdf2_bytes",
                &[
                    (I64, &pwd_handle),
                    (I64, &salt_handle),
                    (DOUBLE, &iter_box),
                    (DOUBLE, &keylen_box),
                    (I64, &digest_handle),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // crypto.pbkdf2(password, salt, iterations, keylen, algorithm, callback)
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "pbkdf2" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 6 {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let pwd_box = lower_expr(ctx, &args[0])?;
            let salt_box = lower_expr(ctx, &args[1])?;
            let iter_box = lower_expr(ctx, &args[2])?;
            let keylen_box = lower_expr(ctx, &args[3])?;
            let alg_box = lower_expr(ctx, &args[4])?;
            let cb_box = lower_expr(ctx, &args[5])?;
            let blk = ctx.block();
            let pwd_handle = unbox_to_i64(blk, &pwd_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let alg_handle = unbox_to_i64(blk, &alg_box);
            Ok(blk.call(
                DOUBLE,
                "js_crypto_pbkdf2_async_alg",
                &[
                    (I64, &pwd_handle),
                    (I64, &salt_handle),
                    (DOUBLE, &iter_box),
                    (DOUBLE, &keylen_box),
                    (I64, &alg_handle),
                    (DOUBLE, &cb_box),
                ],
            ))
        }

        // crypto.scryptSync(password, salt, keylen, options?) -> Buffer.
        // The runtime returns a Buffer (HIR types scryptSync as Uint8Array)
        // and reads optional `{ N, r, p }` cost params from the options
        // object pointer; an absent options arg passes a null pointer and the
        // runtime uses Node's defaults (N=16384, r=8, p=1).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "scryptSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 3 {
                return Ok(double_literal(0.0));
            }
            let pwd_box = lower_expr(ctx, &args[0])?;
            let salt_box = lower_expr(ctx, &args[1])?;
            let keylen_box = lower_expr(ctx, &args[2])?;
            let opts_box = if args.len() >= 4 {
                Some(lower_expr(ctx, &args[3])?)
            } else {
                None
            };
            let blk = ctx.block();
            let pwd_handle = unbox_to_i64(blk, &pwd_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let opts_handle = match &opts_box {
                Some(b) => unbox_to_i64(blk, b),
                None => "0".to_string(),
            };
            let buf_handle = blk.call(
                I64,
                "js_crypto_scrypt_bytes",
                &[
                    (I64, &pwd_handle),
                    (I64, &salt_handle),
                    (DOUBLE, &keylen_box),
                    (I64, &opts_handle),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
        // The runtime returns an array-buffer-marked Buffer; callers wrap it
        // with `Buffer.from(...)` / `new Uint8Array(...)`.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "hkdfSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.len() < 5 {
                return Ok(double_literal(0.0));
            }
            let digest_box = lower_expr(ctx, &args[0])?;
            let ikm_box = lower_expr(ctx, &args[1])?;
            let salt_box = lower_expr(ctx, &args[2])?;
            let info_box = lower_expr(ctx, &args[3])?;
            let keylen_box = lower_expr(ctx, &args[4])?;
            let blk = ctx.block();
            let digest_handle = unbox_to_i64(blk, &digest_box);
            let ikm_handle = unbox_to_i64(blk, &ikm_box);
            let salt_handle = unbox_to_i64(blk, &salt_box);
            let info_handle = unbox_to_i64(blk, &info_box);
            let buf_handle = blk.call(
                I64,
                "js_crypto_hkdf_sync",
                &[
                    (I64, &digest_handle),
                    (I64, &ikm_handle),
                    (I64, &salt_handle),
                    (I64, &info_handle),
                    (DOUBLE, &keylen_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }.
        // The runtime builds the object (PEM strings) and returns it already
        // NaN-boxed; `.publicKey` / `.privateKey` reads go through the generic
        // object property dispatch (the object carries a keys array).
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property } if property == "generateKeyPairSync" && matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(n) if n == "crypto"
                )
            ) =>
        {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let type_box = lower_expr(ctx, &args[0])?;
            let opts_box = if args.len() >= 2 {
                Some(lower_expr(ctx, &args[1])?)
            } else {
                None
            };
            let blk = ctx.block();
            let type_handle = unbox_to_i64(blk, &type_box);
            let opts_handle = match &opts_box {
                Some(b) => unbox_to_i64(blk, b),
                None => "0".to_string(),
            };
            // Returns an already-NaN-boxed object (POINTER_TAG).
            Ok(blk.call(
                DOUBLE,
                "js_crypto_generate_key_pair_sync",
                &[(I64, &type_handle), (I64, &opts_handle)],
            ))
        }

        // Phase H fs: `fs.promises.METHOD(args...)` — HIR shape is a
        // nested PropertyGet { PropertyGet { NativeModuleRef("fs"),
        // "promises" }, method }. We route these to their sync
        // counterparts and wrap the result in an already-resolved
        // Promise via `js_promise_resolved`. This is enough for the
        // test's `await fs.promises.readFile(...)` pattern.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, .. } if matches!(
                    object.as_ref(),
                    Expr::PropertyGet { object: inner, property: p }
                        if p == "promises" && matches!(
                            inner.as_ref(),
                            Expr::NativeModuleRef(name) if name == "fs"
                        )
                )
            ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            match property {
                "readFile" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let blk = ctx.block();
                    let value = blk.call(
                        DOUBLE,
                        "js_fs_read_file_dispatch",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    );
                    let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &value)]);
                    Ok(nanbox_pointer_inline(blk, &promise_handle))
                }
                "writeFile" if args.len() >= 2 => {
                    let path = lower_expr(ctx, &args[0])?;
                    let content = lower_expr(ctx, &args[1])?;
                    let options = if args.len() >= 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_write_file_sync_options",
                        &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                    );
                    let blk = ctx.block();
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
                    Ok(nanbox_pointer_inline(blk, &promise_handle))
                }
                "appendFile" if args.len() >= 2 => {
                    let path = lower_expr(ctx, &args[0])?;
                    let content = lower_expr(ctx, &args[1])?;
                    let options = if args.len() >= 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_append_file_sync_options",
                        &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                    );
                    let blk = ctx.block();
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
                    Ok(nanbox_pointer_inline(blk, &promise_handle))
                }
                "mkdir" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let _ = ctx.block().call(I32, "js_fs_mkdir_sync", &[(DOUBLE, &p)]);
                    let blk = ctx.block();
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
                    Ok(nanbox_pointer_inline(blk, &promise_handle))
                }
                _ => {
                    // Unsupported — return a resolved promise holding
                    // undefined so `await` sees a real pending→settled
                    // transition instead of a null pointer.
                    for a in args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    let blk = ctx.block();
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
                    Ok(nanbox_pointer_inline(blk, &promise_handle))
                }
            }
        }

        // Phase H fs: `fs.METHOD(args...)` — catch all Call expressions
        // where the callee is a PropertyGet on a `NativeModuleRef("fs")`
        // and dispatch to the matching runtime function. HIR already
        // routes the common cases (`readFileSync`, `writeFileSync`,
        // etc.) into dedicated `Expr::Fs*` variants, but several sync
        // APIs (`statSync`, `readdirSync`, `renameSync`, ...) fall
        // through to this generic shape. Handling them here avoids
        // touching HIR or the lower_call dispatch tower.
        Expr::Call { callee, args, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, .. } if matches!(
                    object.as_ref(),
                    Expr::NativeModuleRef(name) if name == "fs"
                )
            ) =>
        {
            let property = if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                property.as_str()
            } else {
                unreachable!()
            };
            match property {
                "readFileSync" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_read_file_dispatch",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "statSync" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_stat_sync_options",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "readdirSync" if !args.is_empty() => {
                    // Runtime returns a raw ArrayHeader pointer
                    // transmuted to f64 (no NaN-box tag). Unbox as i64
                    // and re-NaN-box with POINTER_TAG so downstream
                    // length/index paths see a proper array handle.
                    // Issue #631: forward optional `options` arg to
                    // pick up `withFileTypes:true`.
                    let p = lower_expr(ctx, &args[0])?;
                    let opts = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let blk = ctx.block();
                    let raw = blk.call(
                        DOUBLE,
                        "js_fs_readdir_sync",
                        &[(DOUBLE, &p), (DOUBLE, &opts)],
                    );
                    let raw_bits = blk.bitcast_double_to_i64(&raw);
                    Ok(nanbox_pointer_inline(blk, &raw_bits))
                }
                "renameSync" if args.len() >= 2 => {
                    let from = lower_expr(ctx, &args[0])?;
                    let to = lower_expr(ctx, &args[1])?;
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_rename_sync",
                        &[(DOUBLE, &from), (DOUBLE, &to)],
                    );
                    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
                }
                "copyFileSync" if args.len() >= 2 => {
                    let from = lower_expr(ctx, &args[0])?;
                    let to = lower_expr(ctx, &args[1])?;
                    let flags = if args.len() >= 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_copy_file_sync_flags",
                        &[(DOUBLE, &from), (DOUBLE, &to), (DOUBLE, &flags)],
                    );
                    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
                }
                "writeFileSync" if args.len() >= 2 => {
                    let path = lower_expr(ctx, &args[0])?;
                    let content = lower_expr(ctx, &args[1])?;
                    let options = if args.len() >= 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_write_file_sync_options",
                        &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                    );
                    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
                }
                "appendFileSync" if args.len() >= 2 => {
                    let path = lower_expr(ctx, &args[0])?;
                    let content = lower_expr(ctx, &args[1])?;
                    let options = if args.len() >= 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_append_file_sync_options",
                        &[(DOUBLE, &path), (DOUBLE, &content), (DOUBLE, &options)],
                    );
                    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
                }
                "accessSync" if !args.is_empty() => {
                    // Node throws on inaccessible paths. We dispatch
                    // through `js_fs_access_sync_throw` which calls
                    // `js_throw` on failure, longjmping into the
                    // nearest enclosing try/catch. Returns NaN-boxed
                    // undefined on success.
                    let p = lower_expr(ctx, &args[0])?;
                    let mode = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_access_sync_throw_mode",
                        &[(DOUBLE, &p), (DOUBLE, &mode)],
                    ))
                }
                "realpathSync" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_realpath_dispatch",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "mkdtempSync" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_mkdtemp_dispatch",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "rmdirSync" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    let _ = ctx.block().call(
                        I32,
                        "js_fs_rmdir_sync_options",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    );
                    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
                }
                "createWriteStream" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_create_write_stream",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "createReadStream" if !args.is_empty() => {
                    let p = lower_expr(ctx, &args[0])?;
                    let options = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    };
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_create_read_stream",
                        &[(DOUBLE, &p), (DOUBLE, &options)],
                    ))
                }
                "readFile" if args.len() >= 3 => {
                    // Node `fs.readFile(path, encoding, callback)` —
                    // sync read + immediate callback invocation.
                    let p = lower_expr(ctx, &args[0])?;
                    let enc = lower_expr(ctx, &args[1])?;
                    let cb = lower_expr(ctx, &args[2])?;
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_read_file_callback",
                        &[(DOUBLE, &p), (DOUBLE, &enc), (DOUBLE, &cb)],
                    ))
                }
                "readFile" if args.len() >= 2 => {
                    // Node `fs.readFile(path, callback)` (no encoding).
                    let p = lower_expr(ctx, &args[0])?;
                    let cb = lower_expr(ctx, &args[1])?;
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_fs_read_file_callback",
                        &[(DOUBLE, &p), (DOUBLE, &undef), (DOUBLE, &cb)],
                    ))
                }
                _ => lower_call(ctx, callee, args),
            }
        }

        // -------- Calls --------
        Expr::Call { callee, args, .. } => lower_call(ctx, callee, args),

        // -------- Proxy / Reflect (metaprogramming) --------
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
