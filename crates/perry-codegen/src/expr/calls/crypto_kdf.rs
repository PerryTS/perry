use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

/// crypto.argon2Sync(algorithm, parameters) -> Buffer.
pub(crate) fn arm_crypto_argon2_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.len() < 2 {
        return Ok(double_literal(0.0));
    }
    let alg_box = lower_expr(ctx, &args[0])?;
    let params_box = lower_expr(ctx, &args[1])?;
    let blk = ctx.block();
    let alg_handle = unbox_to_i64(blk, &alg_box);
    let buf_handle = blk.call(
        I64,
        "js_crypto_argon2_sync",
        &[(I64, &alg_handle), (DOUBLE, &params_box)],
    );
    Ok(nanbox_pointer_inline(blk, &buf_handle))
}

/// crypto.argon2(algorithm, parameters, callback).
pub(crate) fn arm_crypto_argon2(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.len() < 3 {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let alg_box = lower_expr(ctx, &args[0])?;
    let params_box = lower_expr(ctx, &args[1])?;
    let cb_box = lower_expr(ctx, &args[2])?;
    let blk = ctx.block();
    let alg_handle = unbox_to_i64(blk, &alg_box);
    Ok(blk.call(
        DOUBLE,
        "js_crypto_argon2_async",
        &[(I64, &alg_handle), (DOUBLE, &params_box), (DOUBLE, &cb_box)],
    ))
}

/// crypto.hkdfSync(algorithm, ikm, salt, info, keylen) -> Buffer.
pub(crate) fn arm_crypto_hkdf_sync_alg(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// crypto.hkdf(algorithm, ikm, salt, info, keylen, callback).
pub(crate) fn arm_crypto_hkdf_async_alg(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// crypto.scrypt(password, salt, keylen[, options], callback).
pub(crate) fn arm_crypto_scrypt(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// crypto.pbkdf2Sync(password, salt, iterations, keylen, digest) -> Buffer.
pub(crate) fn arm_crypto_pbkdf2_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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
    // #2013/#3146: node validates iterations (int >= 1), keylen
    // (int >= 0), then the digest (string) before deriving — and a
    // non-string digest would otherwise mask into a bogus pointer and
    // segfault `bytes_from_ptr`.
    emit_validate_integer_arg(ctx, &iter_box, "iterations", 1.0, i32::MAX as f64);
    emit_validate_integer_arg(ctx, &keylen_box, "keylen", 0.0, i32::MAX as f64);
    if let Some(db) = &digest_box {
        emit_validate_string_arg(ctx, db, "digest");
    }
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

/// crypto.pbkdf2(password, salt, iterations, keylen, algorithm, callback).
pub(crate) fn arm_crypto_pbkdf2_async(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// crypto.scryptSync(password, salt, keylen, options?) -> Buffer.
pub(crate) fn arm_crypto_scrypt_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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
    // #2013/#3146: node validates keylen as an integer in [0, 2^31-1].
    emit_validate_integer_arg(ctx, &keylen_box, "keylen", 0.0, i32::MAX as f64);
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

/// crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
pub(crate) fn arm_crypto_hkdf_sync(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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
