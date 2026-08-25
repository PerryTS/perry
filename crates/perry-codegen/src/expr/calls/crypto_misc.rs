use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

/// Standalone `crypto.createHmac(alg, key)` / legacy `crypto.Hmac(alg, key)`.
pub(crate) fn arm_crypto_create_hmac(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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
    // #2013/#3146: validate algorithm (then key) before unboxing.
    emit_validate_string_arg(ctx, &alg_box, "hmac");
    emit_validate_crypto_key_arg(ctx, &key_box, "key");
    let blk = ctx.block();
    let alg_handle = unbox_to_i64(blk, &alg_box);
    let key_handle = unbox_to_i64(blk, &key_box);
    Ok(blk.call(
        DOUBLE,
        "js_crypto_create_hmac",
        &[(I64, &alg_handle), (I64, &key_handle)],
    ))
}

/// `crypto.createCipheriv(alg, key, iv)` / `crypto.createDecipheriv(...)`.
pub(crate) fn arm_crypto_create_cipheriv(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
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

/// `crypto.randomBytes(size, callback)` — callback form.
pub(crate) fn arm_crypto_random_bytes_async(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let size_box = lower_expr(ctx, &args[0])?;
    let cb_box = lower_expr(ctx, &args[1])?;
    let blk = ctx.block();
    Ok(blk.call(
        DOUBLE,
        "js_crypto_random_bytes_async",
        &[(DOUBLE, &size_box), (DOUBLE, &cb_box)],
    ))
}

/// `crypto.randomFill(buffer[, offset][, size], callback)`.
pub(crate) fn arm_crypto_random_fill(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.createSign(alg)` / `crypto.createVerify(alg)` (#1364) handle.
pub(crate) fn arm_crypto_create_sign_verify(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
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

/// Phase H crypto: `crypto.randomBytes(n)` as a Buffer.
pub(crate) fn arm_crypto_random_bytes(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(0.0));
    }
    let size_box = lower_expr(ctx, &args[0])?;
    let blk = ctx.block();
    let buf_handle = blk.call(I64, "js_crypto_random_bytes_buffer", &[(DOUBLE, &size_box)]);
    Ok(nanbox_pointer_inline(blk, &buf_handle))
}

/// Phase H crypto: `crypto.randomUUID()`.
pub(crate) fn arm_crypto_random_uuid(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let options_box = if let Some(options) = args.first() {
        lower_expr(ctx, options)?
    } else {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    };
    let blk = ctx.block();
    let handle = blk.call(I64, "js_crypto_random_uuid", &[(DOUBLE, &options_box)]);
    Ok(nanbox_string_inline(blk, &handle))
}

/// `crypto.randomUUIDv7([options])` — RFC 9562 v7 (#2550).
pub(crate) fn arm_crypto_random_uuidv7(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    _args: &[Expr],
) -> Result<String> {
    let blk = ctx.block();
    let handle = blk.call(I64, "js_crypto_random_uuidv7", &[]);
    Ok(nanbox_string_inline(blk, &handle))
}

/// Phase H crypto: `crypto.randomInt([min,] max[, callback])`.
pub(crate) fn arm_crypto_random_int(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// Phase H crypto: `crypto.timingSafeEqual(a, b)`.
pub(crate) fn arm_crypto_timing_safe_equal(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.len() < 2 {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let a_box = lower_expr(ctx, &args[0])?;
    let b_box = lower_expr(ctx, &args[1])?;
    let blk = ctx.block();
    Ok(blk.call(
        DOUBLE,
        "js_crypto_timing_safe_equal",
        &[(DOUBLE, &a_box), (DOUBLE, &b_box)],
    ))
}

/// Prime generation/checking APIs (`generatePrime*` / `checkPrime*`).
pub(crate) fn arm_crypto_prime(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let property = if let Expr::PropertyGet { property, .. } = callee {
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
    let callback_box = if matches!(property, "generatePrime" | "checkPrime") && args.len() >= 3 {
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
        Ok(blk.call(
            DOUBLE,
            "js_crypto_generate_prime_sync",
            &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
        ))
    } else {
        Ok(blk.call(
            DOUBLE,
            "js_crypto_check_prime_sync",
            &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
        ))
    }
}

/// `crypto.getHashes()` / `getCiphers()` / `getCurves()` inventories.
pub(crate) fn arm_crypto_get_inventory(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    _args: &[Expr],
) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
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

/// `crypto.getCipherInfo(algorithm, options?)`.
pub(crate) fn arm_crypto_get_cipher_info(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.getFips()` — Perry does not expose OpenSSL FIPS mode.
pub(crate) fn arm_crypto_get_fips(
    _ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    _args: &[Expr],
) -> Result<String> {
    Ok(double_literal(0.0))
}

/// `crypto.setFips(false|0)` — disabling no-op.
pub(crate) fn arm_crypto_set_fips(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    for a in args {
        let _ = lower_expr(ctx, a)?;
    }
    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
}

/// `crypto.secureHeapUsed()` — default Node shape when secure heap off.
pub(crate) fn arm_crypto_secure_heap_used(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    _args: &[Expr],
) -> Result<String> {
    let blk = ctx.block();
    let obj = blk.call(I64, "js_crypto_secure_heap_used", &[]);
    Ok(nanbox_pointer_inline(blk, &obj))
}

/// One-shot asymmetric `crypto.sign(alg, data, key[, callback])`.
pub(crate) fn arm_crypto_sign(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// One-shot asymmetric `crypto.verify(alg, data, key, sig[, callback])`.
pub(crate) fn arm_crypto_verify(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// RSA `publicEncrypt`/`privateDecrypt`/`privateEncrypt`/`publicDecrypt`.
pub(crate) fn arm_crypto_public_private_crypt(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.len() < 2 {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let property = if let Expr::PropertyGet { property, .. } = callee {
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

/// `crypto.createSecretKey(key, encoding?)`.
pub(crate) fn arm_crypto_create_secret_key(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.generateKeySync("aes"|"hmac", { length })`.
pub(crate) fn arm_crypto_generate_key_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.generateKey("aes"|"hmac", { length }, cb)`.
pub(crate) fn arm_crypto_generate_key_async(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.generateKeyPairSync(type, options)` → { publicKey, privateKey }.
pub(crate) fn arm_crypto_generate_key_pair_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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
