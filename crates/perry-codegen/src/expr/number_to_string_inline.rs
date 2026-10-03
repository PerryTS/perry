//! Inline small-integer number-to-string (#10762).
//!
//! `String(n)`, `` `${n}` ``, `n.toString()` and `"" + n` on a number all
//! reach `perry-runtime`'s `small_integer_sso_bits` for an integer in
//! `-9999..=99999`: the answer is an SSO immediate, computed with no
//! allocation. What was left was the call itself and the tag ladder in front
//! of it. This emits the same computation at the call site for operands the
//! type analysis says are numbers, and keeps the runtime call on a cold arm
//! for everything else, including a declared-`number` slot that holds
//! something else at run time.
//!
//! The bits are identical to the runtime's, so the two arms are
//! interchangeable: most significant digit in byte 0, a leading `-` below the
//! digits, and the length at `SHORT_STRING_LEN_SHIFT`. `-0` converts to `0`
//! and compares equal to `0.0`, so it prints `"0"` like the runtime.

use super::FnCtx;
use crate::nanbox::{double_literal, i64_literal, SHORT_STRING_TAG};
use crate::types::{DOUBLE, I1, I32, I64};

/// Bit offset of an SSO string's length byte. Must match
/// `perry-runtime::value::tags::SHORT_STRING_LEN_SHIFT`.
const SHORT_STRING_LEN_SHIFT: u64 = 40;

/// Emit the SSO bits of `v` inline when it is an integral double in
/// `-9999..=99999`, and `slow(ctx)` otherwise. Returns the merged NaN-boxed
/// string. `slow` runs with the builder positioned in the cold block and must
/// return a DOUBLE.
pub(crate) fn emit_number_to_string_inline(
    ctx: &mut FnCtx<'_>,
    v: &str,
    slow: impl FnOnce(&mut FnCtx<'_>) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let int_idx = ctx.new_block("num2str.int");
    let fast_idx = ctx.new_block("num2str.fast");
    let slow_idx = ctx.new_block("num2str.slow");
    let merge_idx = ctx.new_block("num2str.merge");
    let int_label = ctx.block_label(int_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Range first: `fptosi` of an out-of-range value is poison, so it runs
    // only behind this branch. Every comparison with a NaN — a NaN-boxed
    // non-number included — is false.
    {
        let blk = ctx.block();
        let lo = blk.fcmp("oge", v, &double_literal(-9_999.0));
        let hi = blk.fcmp("ole", v, &double_literal(99_999.0));
        let in_range = blk.and(I1, &lo, &hi);
        blk.cond_br(&in_range, &int_label, &slow_label);
    }

    ctx.current_block = int_idx;
    let int = {
        let blk = ctx.block();
        let int = blk.fptosi(DOUBLE, v, I32);
        let back = blk.sitofp(I32, &int, DOUBLE);
        let integral = blk.fcmp("oeq", &back, v);
        blk.cond_br(&integral, &fast_label, &slow_label);
        int
    };

    ctx.current_block = fast_idx;
    let fast = {
        let blk = ctx.block();
        let neg = blk.icmp_slt(I32, &int, "0");
        let negated = blk.sub(I32, "0", &int);
        let abs32 = blk.select(I1, &neg, I32, &negated, &int);
        let abs = blk.zext(I32, &abs32, I64);
        // Five digits, most significant first, by fixed-point division:
        // `abs * ceil(2^32 / 10^4)` puts `abs / 10^4` in the high word and
        // the scaled remainder in the low word, and each `* 10` of the low
        // word shifts the next digit up. Exact for every `abs < 100000`
        // (checked exhaustively), one multiply per digit where the plain
        // `/ 10`, `% 10` pairs cost about five instructions each. Leading
        // zeros are digits too, so they are shifted out below.
        let low_mask = i64_literal(0xFFFF_FFFF);
        let mut scaled = blk.mul(I64, &abs, "429497");
        let mut packed = blk.lshr(I64, &scaled, "32");
        for byte in 1..5u32 {
            let low = blk.and(I64, &scaled, &low_mask);
            scaled = blk.mul(I64, &low, "10");
            let digit = blk.lshr(I64, &scaled, "32");
            let shifted = blk.shl(I64, &digit, &(byte * 8).to_string());
            packed = blk.or(I64, &packed, &shifted);
        }
        let ascii = blk.or(I64, &packed, &i64_literal(0x30_3030_3030));
        let mut ndig = "1".to_string();
        for bound in ["10", "100", "1000", "10000"] {
            let ge = blk.icmp_uge(I64, &abs, bound);
            let one = blk.zext(I1, &ge, I64);
            ndig = blk.add(I64, &ndig, &one);
        }
        let zeros = blk.sub(I64, "5", &ndig);
        let shift = blk.shl(I64, &zeros, "3");
        let payload = blk.lshr(I64, &ascii, &shift);
        let with_sign = blk.shl(I64, &payload, "8");
        let with_sign = blk.or(I64, &with_sign, &(b'-' as u64).to_string());
        let signed_len = blk.add(I64, &ndig, "1");
        let payload = blk.select(I1, &neg, I64, &with_sign, &payload);
        let len = blk.select(I1, &neg, I64, &signed_len, &ndig);
        let len = blk.shl(I64, &len, &SHORT_STRING_LEN_SHIFT.to_string());
        let bits = blk.or(I64, &payload, &len);
        let bits = blk.or(I64, &bits, &i64_literal(SHORT_STRING_TAG));
        let boxed = blk.bitcast_i64_to_double(&bits);
        blk.br(&merge_label);
        boxed
    };

    ctx.current_block = slow_idx;
    let slow_val = slow(ctx)?;
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let fast_label_end = ctx.block_label(fast_idx);
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (fast.as_str(), fast_label_end.as_str()),
            (slow_val.as_str(), slow_end.as_str()),
        ],
    ))
}
