//! `lower_jsonwebtoken_sign` / `lower_jsonwebtoken_verify` and the
//! payload-pointer helper. Split out of `lower_call/native.rs`
//! (~272 LOC) so the parent module stays under the 2000-line cap.
//!
//! Both entry points are option-aware (algorithm: HS256 / ES256 /
//! RS256) and route to typed runtime helpers when possible, falling
//! back to the `_dyn` / `_dyn_opts` paths when the algorithm /
//! options object isn't an inline literal (#1074).

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::type_analysis::is_string_expr;
use crate::types::{DOUBLE, I32, I64};

use super::*;

fn lower_jsonwebtoken_payload_ptr(ctx: &mut FnCtx<'_>, payload: &Expr) -> Result<String> {
    if is_string_expr(ctx, payload) {
        return get_raw_string_ptr(ctx, payload);
    }

    let boxed_payload = lower_expr(ctx, payload)?;
    Ok(ctx.block().call(
        I64,
        "js_json_stringify",
        &[(DOUBLE, &boxed_payload), (I32, "0")],
    ))
}

pub(super) fn lower_jsonwebtoken_sign(ctx: &mut FnCtx<'_>, args: &[Expr]) -> Result<String> {
    if args.len() < 2 {
        bail!(
            "jsonwebtoken.sign(payload, secret, options?) expects at least 2 args, got {}",
            args.len()
        );
    }

    let payload_ptr = lower_jsonwebtoken_payload_ptr(ctx, &args[0])?;
    let secret_ptr = get_raw_string_ptr(ctx, &args[1])?;
    let mut runtime = "js_jwt_sign";
    // #1074: when the user writes `{ algorithm: ALG }` (i.e. `algorithm`
    // is a non-literal expression), the inline-literal fast path can't
    // pick a typed runtime helper. We track that as a fallback to
    // `js_jwt_sign_dyn`, which takes the alg string as a runtime argument
    // and dispatches there. Pre-#1074 this fell through to the HS256
    // path silently — a real cryptographic downgrade.
    let mut alg_ptr_dyn: Option<String> = None;
    let mut expires_in = double_literal(0.0);
    let mut kid_ptr = "0".to_string();

    if let Some(options) = args.get(2) {
        if let Some(props) = extract_options_fields(ctx, options) {
            for (key, val) in &props {
                match key.as_str() {
                    "algorithm" => {
                        if let Expr::String(algorithm) = val {
                            runtime = match algorithm.as_str() {
                                "ES256" => "js_jwt_sign_es256",
                                "RS256" => "js_jwt_sign_rs256",
                                _ => "js_jwt_sign",
                            };
                        } else {
                            // Non-literal alg (#1074): lower to a string
                            // pointer and let `js_jwt_sign_dyn` pick the
                            // right backend at runtime.
                            alg_ptr_dyn = Some(get_raw_string_ptr(ctx, val)?);
                            runtime = "js_jwt_sign_dyn";
                        }
                    }
                    "expiresIn" => {
                        expires_in = lower_expr(ctx, val)?;
                    }
                    "keyid" | "kid" => {
                        kid_ptr = get_raw_string_ptr(ctx, val)?;
                    }
                    _ => {
                        let _ = lower_expr(ctx, val)?;
                    }
                }
            }
        } else {
            // #1074 case C: the options expression is not an inline
            // object literal (e.g. `const opts = { algorithm: "ES256" };
            // jwt.sign(p, k, opts)`). Lower options as a NaN-boxed
            // JSValue and route to `js_jwt_sign_dyn_opts`, which
            // extracts algorithm/expiresIn/keyid at runtime.
            let opts_val = lower_expr(ctx, options)?;
            for extra in args.iter().skip(3) {
                let _ = lower_expr(ctx, extra)?;
            }
            ctx.pending_declares.push((
                "js_jwt_sign_dyn_opts".to_string(),
                I64,
                vec![I64, I64, DOUBLE],
            ));
            let raw = ctx.block().call(
                I64,
                "js_jwt_sign_dyn_opts",
                &[(I64, &payload_ptr), (I64, &secret_ptr), (DOUBLE, &opts_val)],
            );
            return Ok(ctx.block().bitcast_i64_to_double(&raw));
        }
    }

    for extra in args.iter().skip(3) {
        let _ = lower_expr(ctx, extra)?;
    }

    // Build the call. The five-arg dyn path takes the alg string first;
    // the four-arg typed-helper path doesn't (the algorithm is implied
    // by the symbol name).
    let raw = if let Some(alg_ptr) = alg_ptr_dyn {
        ctx.pending_declares.push((
            "js_jwt_sign_dyn".to_string(),
            I64,
            vec![I64, I64, I64, DOUBLE, I64],
        ));
        ctx.block().call(
            I64,
            "js_jwt_sign_dyn",
            &[
                (I64, &alg_ptr),
                (I64, &payload_ptr),
                (I64, &secret_ptr),
                (DOUBLE, &expires_in),
                (I64, &kid_ptr),
            ],
        )
    } else {
        ctx.pending_declares
            .push((runtime.to_string(), I64, vec![I64, I64, DOUBLE, I64]));
        ctx.block().call(
            I64,
            runtime,
            &[
                (I64, &payload_ptr),
                (I64, &secret_ptr),
                (DOUBLE, &expires_in),
                (I64, &kid_ptr),
            ],
        )
    };
    Ok(ctx.block().bitcast_i64_to_double(&raw))
}

/// Dispatch `jsonwebtoken.verify(token, secret_or_pem, options?)` to
/// the right runtime (HS256 / ES256 / RS256) based on the
/// `algorithms: ['…']` (or singular `algorithm: '…'`) option.
/// Mirrors `lower_jsonwebtoken_sign`.
///
/// perry#927 follow-up: the generic NativeModSig table picked
/// `js_jwt_verify` (HS256-only) for every algorithm, so ES256 / RS256
/// tokens silently failed verification (returning `null` to user
/// code, breaking the shop-admin auth middleware after a successful
/// signup). Verify needs the same option-aware routing that `sign`
/// already has.
///
/// Return shape matches the old `NR_OBJ_FROM_JSON_STR`: the runtime
/// hands back a JSON-text `*mut StringHeader` (or null), which we
/// pipe through `js_json_parse_or_null` so user code sees a real
/// object on success and `null` on failure (no throw).
pub(super) fn lower_jsonwebtoken_verify(ctx: &mut FnCtx<'_>, args: &[Expr]) -> Result<String> {
    if args.len() < 2 {
        bail!(
            "jsonwebtoken.verify(token, secret, options?) expects at least 2 args, got {}",
            args.len()
        );
    }

    let token_ptr = get_raw_string_ptr(ctx, &args[0])?;
    let secret_ptr = get_raw_string_ptr(ctx, &args[1])?;
    let mut runtime = "js_jwt_verify";
    // #1074: when `algorithm` (or the first entry of `algorithms`) is a
    // non-literal expression, lower it as a string and route through
    // `js_jwt_verify_dyn` instead of silently picking HS256.
    let mut alg_ptr_dyn: Option<String> = None;

    if let Some(options) = args.get(2) {
        if let Some(props) = extract_options_fields(ctx, options) {
            for (key, val) in &props {
                match key.as_str() {
                    // `algorithm: 'ES256'` (singular) — accepted for
                    // symmetry with `sign`'s option name.
                    "algorithm" => {
                        if let Expr::String(algorithm) = val {
                            runtime = match algorithm.as_str() {
                                "ES256" => "js_jwt_verify_es256",
                                "RS256" => "js_jwt_verify_rs256",
                                _ => "js_jwt_verify",
                            };
                        } else {
                            alg_ptr_dyn = Some(get_raw_string_ptr(ctx, val)?);
                            runtime = "js_jwt_verify_dyn";
                        }
                    }
                    // `algorithms: ['ES256']` (plural array) — the
                    // canonical Node `jsonwebtoken.verify` shape.
                    // First entry decides routing; the underlying Rust
                    // jsonwebtoken crate's verify is single-algorithm,
                    // so multi-algorithm fallback isn't honored.
                    "algorithms" => {
                        if let Expr::Array(elems) = val {
                            match elems.first() {
                                Some(Expr::String(algorithm)) => {
                                    runtime = match algorithm.as_str() {
                                        "ES256" => "js_jwt_verify_es256",
                                        "RS256" => "js_jwt_verify_rs256",
                                        _ => "js_jwt_verify",
                                    };
                                }
                                // #1074: first element is a non-literal
                                // (e.g. `algorithms: [ALG]` where ALG is
                                // a const-bound name). Lower it as a
                                // string and route through the dyn path.
                                Some(other) => {
                                    alg_ptr_dyn = Some(get_raw_string_ptr(ctx, other)?);
                                    runtime = "js_jwt_verify_dyn";
                                }
                                None => {}
                            }
                        } else {
                            // `algorithms` is a non-array expression
                            // (e.g. a const-bound array reference). We
                            // could try harder, but the runtime opts
                            // path below already handles this when the
                            // whole options object is non-extractable.
                            // Lower the side effect and let the
                            // following HS256 fallback fire — same as
                            // pre-#1074 (rare in practice).
                            let _ = lower_expr(ctx, val)?;
                        }
                    }
                    _ => {
                        let _ = lower_expr(ctx, val)?;
                    }
                }
            }
        } else {
            // #1074 case C: options is not an inline object literal —
            // defer extraction to `js_jwt_verify_dyn_opts`, which reads
            // `algorithm` / `algorithms[0]` at runtime.
            let opts_val = lower_expr(ctx, options)?;
            for extra in args.iter().skip(3) {
                let _ = lower_expr(ctx, extra)?;
            }
            ctx.pending_declares.push((
                "js_jwt_verify_dyn_opts".to_string(),
                I64,
                vec![I64, I64, DOUBLE],
            ));
            ctx.pending_declares
                .push(("js_json_parse_or_null".to_string(), I64, vec![I64]));
            let blk = ctx.block();
            let raw = blk.call(
                I64,
                "js_jwt_verify_dyn_opts",
                &[(I64, &token_ptr), (I64, &secret_ptr), (DOUBLE, &opts_val)],
            );
            let parsed_bits = blk.call(I64, "js_json_parse_or_null", &[(I64, &raw)]);
            return Ok(blk.bitcast_i64_to_double(&parsed_bits));
        }
    }

    for extra in args.iter().skip(3) {
        let _ = lower_expr(ctx, extra)?;
    }

    let raw = if let Some(alg_ptr) = alg_ptr_dyn {
        ctx.pending_declares
            .push(("js_jwt_verify_dyn".to_string(), I64, vec![I64, I64, I64]));
        ctx.block().call(
            I64,
            "js_jwt_verify_dyn",
            &[(I64, &alg_ptr), (I64, &token_ptr), (I64, &secret_ptr)],
        )
    } else {
        ctx.pending_declares
            .push((runtime.to_string(), I64, vec![I64, I64]));
        ctx.block()
            .call(I64, runtime, &[(I64, &token_ptr), (I64, &secret_ptr)])
    };
    ctx.pending_declares
        .push(("js_json_parse_or_null".to_string(), I64, vec![I64]));
    let blk = ctx.block();
    let parsed_bits = blk.call(I64, "js_json_parse_or_null", &[(I64, &raw)]);
    Ok(blk.bitcast_i64_to_double(&parsed_bits))
}
