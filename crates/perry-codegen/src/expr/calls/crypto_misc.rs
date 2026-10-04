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
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let (arg_values, arg_group) = crate::lower_call::lower_call_args_rooted(ctx, args)?;
    let alg_box = arg_values[0].clone();
    let key_box = arg_values[1].clone();
    // #2013/#3146: validate algorithm (then key) before unboxing.
    emit_validate_string_arg(ctx, &alg_box, "hmac");
    emit_validate_crypto_key_arg(ctx, &key_box, "key");
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let key_handle = unbox_ffi_str_arg(blk, &key_box);
    let result = blk.call(
        DOUBLE,
        "js_crypto_create_hmac",
        &[(I64, &alg_handle), (I64, &key_handle)],
    );
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(4)])?;
    let alg_box = arg_values[0].clone();
    let key_box = arg_values[1].clone();
    let iv_box = arg_values[2].clone();
    let options_box = arg_values
        .get(3)
        .cloned()
        .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let key_handle = unbox_ffi_str_arg(blk, &key_box);
    let iv_handle = unbox_ffi_str_arg(blk, &iv_box);
    let fname = if property == "createCipheriv" {
        "js_crypto_create_cipheriv"
    } else {
        "js_crypto_create_decipheriv"
    };
    // Returns an already-NaN-boxed f64 (POINTER_TAG + handle id).
    let result = blk.call(
        DOUBLE,
        fname,
        &[
            (I64, &alg_handle),
            (I64, &key_handle),
            (I64, &iv_handle),
            (DOUBLE, &options_box),
        ],
    );
    arg_group.release(ctx);
    Ok(result)
}

/// `crypto.randomBytes(size, callback)` — callback form.
pub(crate) fn arm_crypto_random_bytes_async(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let size_box = arg_values[0].clone();
    let cb_box = arg_values[1].clone();
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_crypto_random_bytes_async",
        &[(DOUBLE, &size_box), (DOUBLE, &cb_box)],
    );
    arg_group.release(ctx);
    Ok(result)
}

/// `crypto.randomFill(buffer[, offset][, size], callback)`.
pub(crate) fn arm_crypto_random_fill(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let last = args.len() - 1;
    // #11789 sweep: the buffer, offset and size are each held across the
    // ones after them and across the callback's evaluation.
    let mut operands: Vec<&Expr> = vec![&args[0]];
    if last >= 2 {
        operands.push(&args[1]);
    }
    if last >= 3 {
        operands.push(&args[2]);
    }
    operands.push(&args[last]);
    let (operand_values, operand_group) =
        crate::lower_call::lower_operand_list_rooted(ctx, &operands)?;
    let mut operand_values = operand_values.into_iter();
    let undef = || double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let buf_box = operand_values.next().expect("buffer operand");
    let off_box = if last >= 2 {
        operand_values.next().expect("offset operand")
    } else {
        undef()
    };
    let sz_box = if last >= 3 {
        operand_values.next().expect("size operand")
    } else {
        undef()
    };
    let cb_box = operand_values.next().expect("callback operand");
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_crypto_random_fill_async",
        &[
            (DOUBLE, &buf_box),
            (DOUBLE, &off_box),
            (DOUBLE, &sz_box),
            (DOUBLE, &cb_box),
        ],
    );
    operand_group.release(ctx);
    Ok(result)
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
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
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
    // #11789 sweep: min and max are each held across the ones after them.
    let mut operands: Vec<&Expr> = vec![min_expr, max_expr];
    operands.extend(callback_expr);
    let (operand_values, operand_group) =
        crate::lower_call::lower_operand_list_rooted(ctx, &operands)?;
    let min_box = operand_values[0].clone();
    let max_box = operand_values[1].clone();
    let callback_box = operand_values.get(2).cloned();
    let blk = ctx.block();
    let result = if let Some(callback_box) = callback_box {
        blk.call(
            DOUBLE,
            "js_crypto_random_int_async",
            &[
                (DOUBLE, &min_box),
                (DOUBLE, &max_box),
                (DOUBLE, &callback_box),
            ],
        )
    } else {
        blk.call(
            DOUBLE,
            "js_crypto_random_int",
            &[(DOUBLE, &min_box), (DOUBLE, &max_box)],
        )
    };
    operand_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let a_box = arg_values[0].clone();
    let b_box = arg_values[1].clone();
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_crypto_timing_safe_equal",
        &[(DOUBLE, &a_box), (DOUBLE, &b_box)],
    );
    arg_group.release(ctx);
    Ok(result)
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
    let is_async = matches!(property, "generatePrime" | "checkPrime");
    // The callback forms are `(value, callback)` or
    // `(value, options, callback)`.  Treating the second argument as options
    // unconditionally accidentally routed the common two-argument form to
    // the synchronous implementation and returned the generated value.
    //
    // #11789 sweep: the value, the options and the callback are each held
    // across the ones after them.
    let has_options = args.len() >= 2 && (!is_async || args.len() >= 3);
    let has_callback = is_async && args.len() >= 2;
    let mut operands: Vec<&Expr> = vec![&args[0]];
    if has_options {
        operands.push(&args[1]);
    }
    if has_callback {
        operands.push(&args[args.len().min(3) - 1]);
    }
    let (operand_values, operand_group) =
        crate::lower_call::lower_operand_list_rooted(ctx, &operands)?;
    let mut operand_values = operand_values.into_iter();
    let first_box = operand_values.next().expect("value operand");
    let options_box = if has_options {
        operand_values.next().expect("options operand")
    } else {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    };
    let callback_box = if has_callback {
        operand_values.next()
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
        let result = blk.call(
            DOUBLE,
            fname,
            &[
                (DOUBLE, &first_box),
                (DOUBLE, &options_box),
                (DOUBLE, &callback_box),
            ],
        );
        operand_group.release(ctx);
        return Ok(result);
    }
    let result = if is_generate {
        blk.call(
            DOUBLE,
            "js_crypto_generate_prime_sync",
            &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
        )
    } else {
        blk.call(
            DOUBLE,
            "js_crypto_check_prime_sync",
            &[(DOUBLE, &first_box), (DOUBLE, &options_box)],
        )
    };
    operand_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let alg_box = arg_values[0].clone();
    let options_box = arg_values
        .get(1)
        .cloned()
        .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    let blk = ctx.block();
    let result = blk.call(
        DOUBLE,
        "js_crypto_get_cipher_info",
        &[(DOUBLE, &alg_box), (DOUBLE, &options_box)],
    );
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(4)])?;
    let alg_box = arg_values[0].clone();
    let data_box = arg_values[1].clone();
    let key_box = arg_values[2].clone();
    let callback_box = if args.len() >= 4 {
        Some(arg_values[3].clone())
    } else {
        None
    };
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let data_handle = unbox_ffi_str_arg(blk, &data_box);
    if let Some(callback_box) = callback_box {
        let result = blk.call(
            DOUBLE,
            "js_crypto_sign_async",
            &[
                (I64, &alg_handle),
                (I64, &data_handle),
                (DOUBLE, &key_box),
                (DOUBLE, &callback_box),
            ],
        );
        arg_group.release(ctx);
        return Ok(result);
    }
    let buf_handle = blk.call(
        I64,
        "js_crypto_sign_rsa_sha256",
        &[(I64, &alg_handle), (I64, &data_handle), (DOUBLE, &key_box)],
    );
    let result = nanbox_pointer_inline(blk, &buf_handle);
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(5)])?;
    let alg_box = arg_values[0].clone();
    let data_box = arg_values[1].clone();
    let key_box = arg_values[2].clone();
    let sig_box = arg_values[3].clone();
    let callback_box = if args.len() >= 5 {
        Some(arg_values[4].clone())
    } else {
        None
    };
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let data_handle = unbox_ffi_str_arg(blk, &data_box);
    let sig_handle = unbox_ffi_str_arg(blk, &sig_box);
    if let Some(callback_box) = callback_box {
        let result = blk.call(
            DOUBLE,
            "js_crypto_verify_async",
            &[
                (I64, &alg_handle),
                (I64, &data_handle),
                (DOUBLE, &key_box),
                (I64, &sig_handle),
                (DOUBLE, &callback_box),
            ],
        );
        arg_group.release(ctx);
        return Ok(result);
    }
    let result = blk.call(
        DOUBLE,
        "js_crypto_verify_rsa_sha256",
        &[
            (I64, &alg_handle),
            (I64, &data_handle),
            (DOUBLE, &key_box),
            (I64, &sig_handle),
        ],
    );
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let key_box = arg_values[0].clone();
    let data_box = arg_values[1].clone();
    let blk = ctx.block();
    let key_converter = match property {
        "publicEncrypt" | "publicDecrypt" => "js_crypto_create_public_key_value",
        "privateDecrypt" | "privateEncrypt" => "js_crypto_create_private_key_value",
        _ => unreachable!(),
    };
    let key_handle = blk.call(I64, key_converter, &[(DOUBLE, &key_box)]);
    let data_handle = unbox_ffi_str_arg(blk, &data_box);
    let fname = match property {
        "publicEncrypt" => "js_crypto_public_encrypt",
        "privateDecrypt" => "js_crypto_private_decrypt",
        "privateEncrypt" => "js_crypto_private_encrypt",
        "publicDecrypt" => "js_crypto_public_decrypt",
        _ => unreachable!(),
    };
    let buf_handle = blk.call(I64, fname, &[(I64, &key_handle), (I64, &data_handle)]);
    let result = nanbox_pointer_inline(blk, &buf_handle);
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let key_box = arg_values[0].clone();
    let enc_box = if args.len() >= 2 {
        Some(arg_values[1].clone())
    } else {
        None
    };
    let blk = ctx.block();
    let key_handle = unbox_ffi_str_arg(blk, &key_box);
    let enc_handle = if let Some(enc) = enc_box {
        unbox_ffi_str_arg(blk, &enc)
    } else {
        "0".to_string()
    };
    let buf_handle = blk.call(
        I64,
        "js_crypto_create_secret_key",
        &[(I64, &key_handle), (I64, &enc_handle)],
    );
    let result = nanbox_pointer_inline(blk, &buf_handle);
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let alg_box = arg_values[0].clone();
    let options_box = arg_values[1].clone();
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let buf_handle = blk.call(
        I64,
        "js_crypto_generate_key_sync",
        &[(I64, &alg_handle), (DOUBLE, &options_box)],
    );
    let result = nanbox_pointer_inline(blk, &buf_handle);
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(3)])?;
    let alg_box = arg_values[0].clone();
    let options_box = arg_values[1].clone();
    let cb_box = arg_values[2].clone();
    let blk = ctx.block();
    let alg_handle = unbox_ffi_str_arg(blk, &alg_box);
    let result = blk.call(
        DOUBLE,
        "js_crypto_generate_key_async",
        &[
            (I64, &alg_handle),
            (DOUBLE, &options_box),
            (DOUBLE, &cb_box),
        ],
    );
    arg_group.release(ctx);
    Ok(result)
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
    let (arg_values, arg_group) =
        crate::lower_call::lower_call_args_rooted(ctx, &args[..args.len().min(2)])?;
    let type_box = arg_values[0].clone();
    let opts_box = if args.len() >= 2 {
        Some(arg_values[1].clone())
    } else {
        None
    };
    let blk = ctx.block();
    let type_handle = unbox_ffi_str_arg(blk, &type_box);
    let opts_handle = match &opts_box {
        Some(b) => unbox_ffi_str_arg(blk, b),
        None => "0".to_string(),
    };
    // Returns an already-NaN-boxed object (POINTER_TAG).
    let result = blk.call(
        DOUBLE,
        "js_crypto_generate_key_pair_sync",
        &[(I64, &type_handle), (I64, &opts_handle)],
    );
    arg_group.release(ctx);
    Ok(result)
}
