use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

/// `crypto.createSign(alg)` / legacy `crypto.Sign(alg)` and
/// `crypto.createVerify(alg)` / legacy `crypto.Verify(alg)` streaming
/// RSA signature handles.
pub(crate) fn arm_crypto_create_sign_verify_legacy(
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

/// `crypto.createECDH(curve)` — Node-compatible ECDH handle.
pub(crate) fn arm_crypto_create_ecdh(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let curve_box = lower_expr(ctx, &args[0])?;
    let blk = ctx.block();
    let curve_handle = unbox_to_i64(blk, &curve_box);
    Ok(blk.call(DOUBLE, "js_crypto_create_ecdh", &[(I64, &curve_handle)]))
}

/// `crypto.createDiffieHellman(...)` and related DH constructors/getters.
pub(crate) fn arm_crypto_diffie_hellman_ctor(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let property = if let Expr::PropertyGet { property, .. } = callee {
        property.as_str()
    } else {
        unreachable!()
    };
    if property == "getDiffieHellman"
        || property == "createDiffieHellmanGroup"
        || property == "DiffieHellmanGroup"
    {
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

/// `createPrivateKey(pem)` / `createPublicKey(pem)` PEM surrogate path.
pub(crate) fn arm_crypto_create_key(
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

/// `crypto.generateKeyPair(type, options, callback)` — callback form.
pub(crate) fn arm_crypto_generate_key_pair_async(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
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

/// `crypto.generateKeyPairSync("rsa"|"ec"|"ed25519"|"x25519", options)` —
/// returns a plain object with `publicKey`/`privateKey` PEM strings.
pub(crate) fn arm_crypto_generate_key_pair_sync_alg(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    let options = if let Some(arg) = args.get(1) {
        lower_expr(ctx, arg)?
    } else {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    };
    let blk = ctx.block();
    let fname = match args.first() {
        Some(Expr::String(alg)) if alg == "ec" => "js_crypto_generate_key_pair_sync_ec_p256",
        Some(Expr::String(alg)) if alg == "ed25519" => "js_crypto_generate_key_pair_sync_ed25519",
        Some(Expr::String(alg)) if alg == "x25519" => "js_crypto_generate_key_pair_sync_x25519",
        _ => "js_crypto_generate_key_pair_sync_rsa",
    };
    let pair = blk.call(I64, fname, &[(DOUBLE, &options)]);
    Ok(nanbox_pointer_inline(blk, &pair))
}

/// `crypto.diffieHellman({ privateKey, publicKey })` — stateless DH.
pub(crate) fn arm_crypto_diffie_hellman_stateless(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let options = lower_expr(ctx, &args[0])?;
    let blk = ctx.block();
    let secret = blk.call(I64, "js_crypto_diffie_hellman", &[(DOUBLE, &options)]);
    Ok(nanbox_pointer_inline(blk, &secret))
}

/// `crypto.encapsulate(publicKey[, callback])` — X25519 KEM.
pub(crate) fn arm_crypto_encapsulate(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.is_empty() {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let key = lower_expr(ctx, &args[0])?;
    if let Some(callback) = args.get(1) {
        let callback = lower_expr(ctx, callback)?;
        let blk = ctx.block();
        Ok(blk.call(
            DOUBLE,
            "js_crypto_encapsulate_async",
            &[(DOUBLE, &key), (DOUBLE, &callback)],
        ))
    } else {
        let blk = ctx.block();
        let result = blk.call(I64, "js_crypto_encapsulate", &[(DOUBLE, &key)]);
        Ok(nanbox_pointer_inline(blk, &result))
    }
}

/// `crypto.decapsulate(privateKey, ciphertext[, callback])` — X25519.
pub(crate) fn arm_crypto_decapsulate(
    ctx: &mut FnCtx<'_>,
    _callee: &Expr,
    args: &[Expr],
) -> Result<String> {
    if args.len() < 2 {
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    }
    let key = lower_expr(ctx, &args[0])?;
    let ciphertext = lower_expr(ctx, &args[1])?;
    if let Some(callback) = args.get(2) {
        let callback = lower_expr(ctx, callback)?;
        let blk = ctx.block();
        Ok(blk.call(
            DOUBLE,
            "js_crypto_decapsulate_async",
            &[(DOUBLE, &key), (DOUBLE, &ciphertext), (DOUBLE, &callback)],
        ))
    } else {
        let blk = ctx.block();
        let shared = blk.call(
            I64,
            "js_crypto_decapsulate",
            &[(DOUBLE, &key), (DOUBLE, &ciphertext)],
        );
        Ok(nanbox_pointer_inline(blk, &shared))
    }
}
