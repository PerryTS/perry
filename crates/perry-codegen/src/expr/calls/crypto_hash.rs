use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Phase H crypto: collapse `crypto.createHash(alg).update(data).digest(enc)`
/// into a single runtime call (chain-collapse arm). `outer_callee`/`outer_args`
/// are the `Expr::Call { callee, args, .. }` bindings of the outer `.digest(...)`
/// call. See the guard in the trunk dispatcher.
pub(crate) fn arm_crypto_hash_chain(
    ctx: &mut FnCtx<'_>,
    outer_callee: &Expr,
    outer_args: &[Expr],
) -> Result<String> {
    // Walk the chain to extract: alg (from createHash/Hash/createHmac/Hmac args),
    // key (from createHmac's second arg, if present),
    // data (from update args), enc (from digest args).
    let digest_args = outer_args;
    let update_call = if let Expr::PropertyGet { object, .. } = outer_callee {
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
    let want_buffer =
        digest_args.is_empty() || matches!(digest_args.first(), Some(Expr::Undefined));

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
                let result = blk.call(I64, "js_crypto_sha256_bytes", &[(I64, &data_handle)]);
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
                let empty = blk.call(I64, "js_string_from_bytes", &[(I64, "0"), (I32, "0")]);
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
            let hash_options_box_opt = if (create_method == "createHash" || create_method == "Hash")
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

            // #2013/#3146: validate the algorithm (and HMAC key) BEFORE
            // unboxing — a non-string would mask into a bogus pointer
            // and segfault `bytes_from_ptr`. node validates the
            // algorithm first, then the key.
            let is_hmac = create_method == "createHmac" || create_method == "Hmac";
            emit_validate_string_arg(ctx, &alg_box, if is_hmac { "hmac" } else { "algorithm" });
            if is_hmac {
                if let Some(kb) = &key_box_opt {
                    emit_validate_crypto_key_arg(ctx, kb, "key");
                }
            }
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

/// Standalone `crypto.createHash(alg)` / legacy callable `crypto.Hash(alg)`.
pub(crate) fn arm_crypto_create_hash(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let alg_box = lower_expr(ctx, &args[0])?;
    let options_box = if args.len() >= 2 {
        Some(lower_expr(ctx, &args[1])?)
    } else {
        None
    };
    // #2013/#3146: reject a non-string algorithm before unboxing.
    emit_validate_string_arg(ctx, &alg_box, "algorithm");
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
