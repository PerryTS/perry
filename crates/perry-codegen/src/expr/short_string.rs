//! Small strings built at the site, with no runtime call.
//!
//! An inline (SSO) string is its bytes in the value: `SHORT_STRING_TAG`, the
//! length in bits 40..47, up to five bytes below (byte 0 lowest, every byte
//! above the length zero: each constructor in the runtime and here writes
//! them so). Three results that are such a value were still paid for as
//! general string work:
//!
//! * **`a + b` of two strings** called `js_string_concat_box` (about 64
//!   instructions for its short-pair arm, `perry-runtime` `short_pair.rs`).
//!   [`emit_concat_pair`] tests the two operands at the site: both inline,
//!   both ASCII, at most five bytes together. Then the result is the first
//!   operand's bits plus the second's length, or'ed with the second's bytes
//!   shifted past the first's: the very bits the runtime's arm builds. Every
//!   other pair (a heap string, a non-ASCII byte, a longer result, a
//!   non-string behind a wrong annotation) takes the call unchanged.
//! * **`String(n)` of a one-digit integer** went through the five-digit
//!   fixed-point conversion (about 55 instructions). [`emit_digit_arm`] puts
//!   an integral `0 <= n <= 9` first: the digit's byte or'ed into a constant.
//!   When LLVM knows the range (`(x >> 4) + 1` of a masked value) the test
//!   folds away. `-0` prints `"0"` as before; `1.5`, `10`, `-1` and `NaN`
//!   fail the test and keep the existing arms.
//! * **`s.charAt(i)` as an operand of such a `+`** produced the heap
//!   one-character string of the byte, which the concatenation could not
//!   take inline. As a direct operand of a string `+` ([`with_char_operands`])
//!   the inline `charAt` arms answer the same character as an inline string
//!   ([`char_value`]); the slow arm (an out-of-range index, a non-ASCII
//!   receiver, a modified prototype) is the entry call as before. Both are
//!   the same JavaScript string.
//!
//! None of this is a proof about the program: every arm is chosen by a test
//! on the run-time value, so there is nothing for check mode to witness.
//!
//! Probe (`PERRY_SITE_STATS=1`): `PERRY_SITE small-string concat-pair`,
//! `small-string digit`, `small-string char` per lowered site.
//!
//! `PERRY_SMALL_STRINGS=0` (`off`, `false`): none of the three; the lowering
//! of main before this module. A compile-cache input.

use std::cell::RefCell;

use perry_hir::Expr;

use super::FnCtx;
use crate::nanbox::{i64_literal, SHORT_STRING_TAG};
use crate::types::{DOUBLE, I1, I32, I64};

/// Bit offset of an inline string's length byte
/// (`perry-runtime::value::tags::SHORT_STRING_LEN_SHIFT`).
const LEN_SHIFT: u64 = 40;
/// The five payload bytes (`SHORT_STRING_DATA_MASK`).
const PAYLOAD_MASK: u64 = 0x0000_00FF_FFFF_FFFF;
/// `SHORT_STRING_MAX_LEN`.
const MAX_LEN: u64 = 5;
/// Bits that must be clear in `value ^ SHORT_STRING_TAG` for an inline ASCII
/// string: the 16 tag bits and the high bit of each payload byte.
const NOT_INLINE_ASCII: u64 = 0xFFFF_0080_8080_8080;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SMALL_STRINGS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

thread_local! {
    /// The receivers (by address) of the `charAt` calls that are direct
    /// operands of the string `+` being lowered on this thread. A lowering
    /// hint for the length of [`with_char_operands`]' closure only; it holds
    /// no fact and outlives no expression.
    static CHAR_OPERAND_RECEIVERS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// The receiver of `e` when `e` is `recv.charAt(..)`.
fn char_at_receiver(e: &Expr) -> Option<&Expr> {
    let Expr::Call { callee, .. } = e else {
        return None;
    };
    match callee.as_ref() {
        Expr::PropertyGet {
            object, property, ..
        } if property == "charAt" => Some(object.as_ref()),
        _ => None,
    }
}

/// Run `lower` (the lowering of `operands`, the two sides of a string `+`)
/// with each operand that is a `charAt` call marked: its inline arms answer
/// an inline string ([`wants_inline_char`]).
pub(crate) fn with_char_operands<T>(operands: &[&Expr], lower: impl FnOnce() -> T) -> T {
    if !enabled() {
        return lower();
    }
    let marked: Vec<usize> = operands
        .iter()
        .filter_map(|e| char_at_receiver(e))
        .map(|recv| recv as *const Expr as usize)
        .collect();
    if marked.is_empty() {
        return lower();
    }
    let depth = CHAR_OPERAND_RECEIVERS.with(|r| {
        let mut r = r.borrow_mut();
        let depth = r.len();
        r.extend(marked);
        depth
    });
    let out = lower();
    CHAR_OPERAND_RECEIVERS.with(|r| r.borrow_mut().truncate(depth));
    out
}

/// The `charAt` on `receiver` is a direct operand of a string `+`.
pub(crate) fn wants_inline_char(receiver: &Expr) -> bool {
    let addr = receiver as *const Expr as usize;
    CHAR_OPERAND_RECEIVERS.with(|r| r.borrow().contains(&addr))
}

/// The one-character inline string of the ASCII byte `byte_i64` (an i64 below
/// 128).
pub(crate) fn char_value(ctx: &mut FnCtx<'_>, byte_i64: &str) -> String {
    let blk = ctx.block();
    let bits = blk.or(
        I64,
        byte_i64,
        &i64_literal(SHORT_STRING_TAG | (1 << LEN_SHIFT)),
    );
    blk.bitcast_i64_to_double(&bits)
}

/// The digit arm of the inline number-to-string: branches to `digit_label`'s
/// blocks when `v` is an integral double in `0..=9`, to `else_label`
/// otherwise (with the builder left unpositioned: the caller sets its next
/// block). Returns the boxed one-digit string and the label of the block
/// that holds it, which the caller must terminate with a branch to its merge.
pub(crate) fn emit_digit_arm(ctx: &mut FnCtx<'_>, v: &str, else_label: &str) -> (String, usize) {
    super::note_site("small-string digit");
    let int_idx = ctx.new_block("num2str.digit_int");
    let digit_idx = ctx.new_block("num2str.digit");
    let int_label = ctx.block_label(int_idx);
    let digit_label = ctx.block_label(digit_idx);
    {
        // Ordered comparisons: a NaN (a NaN-boxed non-number included) fails
        // both. Range first, so the `fptosi` below is never poison.
        let blk = ctx.block();
        let lo = blk.fcmp("oge", v, "0.0");
        let hi = blk.fcmp("ole", v, "9.0");
        let in_range = blk.and(I1, &lo, &hi);
        blk.cond_br(&in_range, &int_label, else_label);
    }
    ctx.current_block = int_idx;
    let int = {
        let blk = ctx.block();
        let int = blk.fptosi(DOUBLE, v, I32);
        let back = blk.sitofp(I32, &int, DOUBLE);
        let integral = blk.fcmp("oeq", &back, v);
        blk.cond_br(&integral, &digit_label, else_label);
        int
    };
    ctx.current_block = digit_idx;
    let blk = ctx.block();
    let digit = blk.zext(I32, &int, I64);
    // `'0'` is 0x30 and the digit is below 16: the or is the sum.
    let bits = blk.or(
        I64,
        &digit,
        &i64_literal(SHORT_STRING_TAG | (1 << LEN_SHIFT) | u64::from(b'0')),
    );
    (blk.bitcast_i64_to_double(&bits), digit_idx)
}

/// `a + b` of two values the caller lowers as a string concatenation: the
/// inline result when both are inline ASCII strings of at most five bytes
/// together, `js_string_concat_box(a, b)` otherwise (module docs).
pub(crate) fn emit_concat_pair(ctx: &mut FnCtx<'_>, a: &str, b: &str) -> String {
    if !enabled() {
        return ctx
            .block()
            .call(DOUBLE, "js_string_concat_box", &[(DOUBLE, a), (DOUBLE, b)]);
    }
    super::note_site("small-string concat-pair");
    let inline_idx = ctx.new_block("strcat.inline");
    let call_idx = ctx.new_block("strcat.call");
    let merge_idx = ctx.new_block("strcat.merge");
    let inline_label = ctx.block_label(inline_idx);
    let call_label = ctx.block_label(call_idx);
    let merge_label = ctx.block_label(merge_idx);
    let tag = i64_literal(SHORT_STRING_TAG);
    let (a_bits, a_len, b_low) = {
        let blk = ctx.block();
        let a_bits = blk.bitcast_double_to_i64(a);
        let b_bits = blk.bitcast_double_to_i64(b);
        // Tag bits zero exactly for an inline string; the length byte and
        // the payload are untouched.
        let a_low = blk.xor(I64, &a_bits, &tag);
        let b_low = blk.xor(I64, &b_bits, &tag);
        let both = blk.or(I64, &a_low, &b_low);
        let bad = blk.and(I64, &both, &i64_literal(NOT_INLINE_ASCII));
        let inline_ascii = blk.icmp_eq(I64, &bad, "0");
        let a_len = blk.lshr(I64, &a_low, &LEN_SHIFT.to_string());
        let b_len = blk.lshr(I64, &b_low, &LEN_SHIFT.to_string());
        let total = blk.add(I64, &a_len, &b_len);
        let fits = blk.icmp_ule(I64, &total, &MAX_LEN.to_string());
        let ok = blk.and(I1, &inline_ascii, &fits);
        blk.cond_br(&ok, &inline_label, &call_label);
        (a_bits, a_len, b_low)
    };

    // Both lengths are at most five here, so the shift is at most 40.
    ctx.current_block = inline_idx;
    let inline = {
        let blk = ctx.block();
        let b_payload = blk.and(I64, &b_low, &i64_literal(PAYLOAD_MASK));
        // `b`'s length in place: what is left of its tag-free bits.
        let b_len_bits = blk.sub(I64, &b_low, &b_payload);
        let shift = blk.shl(I64, &a_len, "3");
        let moved = blk.shl(I64, &b_payload, &shift);
        // `a`'s tag, length and bytes, with `b`'s length added to the
        // length byte (the sum is at most five: no carry).
        let head = blk.add(I64, &a_bits, &b_len_bits);
        let bits = blk.or(I64, &head, &moved);
        let boxed = blk.bitcast_i64_to_double(&bits);
        blk.br(&merge_label);
        boxed
    };

    ctx.current_block = call_idx;
    let called = ctx
        .block()
        .call(DOUBLE, "js_string_concat_box", &[(DOUBLE, a), (DOUBLE, b)]);
    let call_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (inline.as_str(), inline_label.as_str()),
            (called.as_str(), call_end.as_str()),
        ],
    )
}
