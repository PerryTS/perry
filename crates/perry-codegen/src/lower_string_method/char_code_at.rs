//! Inline `charCodeAt` and `charAt` on an ASCII string (#7592).
//!
//! Split out of `lower_string_method.rs` to stay under the repo's
//! 2000-line-per-file lint cap. Pure code move plus the `pub(super)`
//! visibility the split requires.

use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

use crate::lower_string_concat::str_operand_handle_tag_dispatched;

// `StringHeader` layout the fast path reads, from
// `crates/perry-runtime/src/string/mod.rs`. That struct is `#[repr(C)]` with
// five `u32` fields; the runtime carries a `const` assertion
// (`STRING_HEADER_ABI_MATCHES_CODEGEN`) pinning exactly these three numbers,
// so a layout change fails the runtime build rather than miscompiling here.
//
// Offset 0 is already an established codegen/runtime contract: the inline
// `.length` fast path in `expr/property_get.rs` loads it, and the struct's
// own doc comment says "`utf16_len` is at offset 0 so codegen can inline
// `.length` as a single i32 load."
const STRING_HEADER_UTF16_LEN_OFFSET: &str = "0";
const STRING_HEADER_BYTE_LEN_OFFSET: &str = "4";
const STRING_HEADER_SIZE: &str = "20";
/// Bit offset of an SSO string's length byte; must match
/// `perry-runtime::value::tags::SHORT_STRING_LEN_SHIFT`.
const SHORT_STRING_LEN_SHIFT: &str = "40";
/// The high bit of each of an SSO string's five payload bytes. Clear in all
/// five means the payload is ASCII.
const SHORT_STRING_HIGH_BITS: &str = "551911719040"; // 0x80_8080_8080

/// Inline `s.charCodeAt(i)` for a heap-tagged, all-ASCII receiver.
///
/// #7592 — the FNV-1a phase of `json_pipeline` spent 85% of its leaf profile
/// in four opaque runtime calls per character over a 68 MB string:
/// `js_string_char_code_at` (31.5%), `js_dynamic_bitxor` (31.0%),
/// `js_string_number_index_to_i32` (13.1%) and `js_get_string_pointer_unified`
/// (9.0%). Only 15% was the JS loop. The individual helpers are each a
/// handful of instructions; the cost is the FFI boundary itself, and — worse
/// — an opaque call on the loop's critical path blocks LICM, so the
/// loop-invariant receiver unbox and header loads could never be hoisted.
///
/// The guard chain reproduces exactly what `js_string_char_code_at` +
/// `js_string_number_index_to_i32` would compute, and anything it cannot prove
/// branches to those same two calls:
/// * `STRING_TAG` receiver — a lying `string` annotation or `undefined`
///   take the slow arm, which still routes through
///   `str_operand_handle_tag_dispatched`. An all-ASCII SSO receiver reads
///   its byte straight out of the value (#10762); a non-ASCII one takes the
///   slow arm;
/// * handle ≥ 4096 — the runtime's `is_valid_string_ptr` magnitude check;
/// * `0.0 <= index < 2^31-1` — ORDERED comparisons, so a NaN-boxed index
///   (a string, a bool, `undefined`, or a genuine NaN) fails both and takes
///   the slow arm, where `js_string_number_index_to_i32` saturates the
///   already-coerced number without invoking user code. Also makes the
///   subsequent `fptosi` in-range, so it can never be poison;
/// * `utf16_len == byte_len` — the runtime's own `is_ascii_string`
///   predicate. Equality implies every byte is one UTF-16 code unit, i.e.
///   every byte is < 0x80, so no WTF-8 / lone-surrogate / astral input can
///   reach the byte load (#6085's bounded walk still owns those);
/// * `index < utf16_len` — out of range returns `NaN` from the slow arm.
///
/// No allocation and no call occur between the receiver re-read and the byte
/// load, so no collection can move the header underneath the fast path.
///
/// The existing receiver tag checks are the admission authority, including
/// calls reached through the tag-guarded Any-string dispatcher. A static type
/// hint must not exclude a runtime-proven string from this same emitter.
///
/// Gated on `static_string_lowering_enabled()` (`PERRY_STATIC_STRING_LOWERING`)
/// — the same knob the sibling inline `.length` fast path uses, so this adds
/// no new mode.
pub(super) fn lower_char_code_at_inline(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
    idx_d: &str,
) -> Option<String> {
    lower_inline(ctx, object, recv_box, idx_d, None, Op::CharCodeAt)
}

/// Which character read the inline body answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    /// The byte as a number; an out-of-range index is `NaN`.
    CharCodeAt,
    /// The byte's one-character string: the runtime's image string
    /// `PERRY_ASCII_CHARS[byte]` (the agent's atom of that text, the same
    /// pointer `js_string_char_at` returns), STRING-tagged. An out-of-range
    /// index takes the entry, which returns `""`.
    CharAt,
}

/// `s.charAt(i)` through its entry (#11915), inline on an ASCII receiver:
/// the same guards and proof as [`lower_char_code_at_inline_entry`], with
/// the byte turned into its canonical one-character string by one load from
/// the runtime's static table. No call, allocation or thread-local access.
pub(super) fn lower_char_at_inline_entry(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
    idx_d: &str,
    entry: &str,
    args: &[String],
    prove_here: bool,
) -> Option<String> {
    lower_inline(
        ctx,
        object,
        recv_box,
        idx_d,
        Some((entry, args, prove_here)),
        Op::CharAt,
    )
}

/// The STRING-tagged canonical one-character string of the ASCII `byte_i64`.
fn ascii_char_value(ctx: &mut FnCtx<'_>, byte_i64: &str) -> String {
    let slot = ctx.block().gep_inbounds(
        crate::types::PTR,
        "@PERRY_ASCII_CHARS",
        &[(I64, byte_i64)],
    );
    let string = ctx.block().load(crate::types::PTR, &slot);
    let handle = ctx.block().ptrtoint(&string, I64);
    crate::expr::nanbox_string_inline(ctx.block(), &handle)
}

/// [`lower_char_code_at_inline`] proving `Pristine(String)` (#11915): the
/// byte joins the index test every inline arm (heap, SSO, out-of-bounds) is
/// dominated by, so a modified prototype reaches only the slow arm — which
/// is the checked entry `entry(recv, args…)`, replacing the unbox, index
/// coercion and decoder calls it had.
pub(super) fn lower_char_code_at_inline_entry(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
    idx_d: &str,
    entry: &str,
    args: &[String],
    prove_here: bool,
) -> Option<String> {
    lower_inline(
        ctx,
        object,
        recv_box,
        idx_d,
        Some((entry, args, prove_here)),
        Op::CharCodeAt,
    )
}

fn lower_inline(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    recv_box: &str,
    idx_d: &str,
    entry: Option<(&str, &[String], bool)>,
    op: Op,
) -> Option<String> {
    debug_assert!(op == Op::CharCodeAt || entry.is_some());
    use crate::nanbox::POINTER_MASK_I64;
    if !crate::expr::static_string_lowering_enabled() {
        return None;
    }

    let bits = ctx.block().bitcast_double_to_i64(recv_box);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    // #11915 / object model step 3: with `prove_here`, both inline arms also
    // prove `Pristine(String)` — the shadow bit of the agent's
    // `%String.prototype%` (`builtin_proto::emit_patched_test`), tested once
    // here. A modified prototype reaches only the slow arm, the checked
    // entry. With `prove_here == false` the caller proved it before the
    // arguments and the slow arm is the unchecked body.
    let prove_here = matches!(entry, Some((_, _, true)));
    let string_pristine = if prove_here {
        let patched = crate::builtin_proto::emit_patched_test(
            ctx,
            crate::builtin_proto::BuiltinProto::String,
        );
        Some(ctx.block().xor(I1, &patched, "true"))
    } else {
        None
    };
    let handle_ok = ctx.block().icmp_uge(I64, &handle, "4096");
    let handle_ok = match &string_pristine {
        Some(p) => ctx.block().and(I1, &handle_ok, p),
        None => handle_ok,
    };
    let idx_ge0 = ctx.block().fcmp("oge", idx_d, "0.0");
    let idx_lt_max = ctx
        .block()
        .fcmp("olt", idx_d, &crate::nanbox::double_literal(2147483647.0));
    let recv_ok = ctx.block().and(I1, &is_heap, &handle_ok);
    let idx_ok = ctx.block().and(I1, &idx_ge0, &idx_lt_max);
    let entry_ok = ctx.block().and(I1, &recv_ok, &idx_ok);

    let hdr_idx = ctx.new_block("cca.hdr");
    let fast_idx = ctx.new_block("cca.fast");
    let sso_check_idx = ctx.new_block("cca.sso_check");
    let sso_idx = ctx.new_block("cca.sso");
    let sso_fast_idx = ctx.new_block("cca.sso_fast");
    let bounds_idx = ctx.new_block("cca.bounds");
    let oob_idx = ctx.new_block("cca.oob");
    let slow_idx = ctx.new_block("cca.slow");
    let merge_idx = ctx.new_block("cca.merge");
    let hdr_label = ctx.block_label(hdr_idx);
    let fast_label = ctx.block_label(fast_idx);
    let sso_check_label = ctx.block_label(sso_check_idx);
    let sso_label = ctx.block_label(sso_idx);
    let sso_fast_label = ctx.block_label(sso_fast_idx);
    let bounds_label = ctx.block_label(bounds_idx);
    let oob_label = ctx.block_label(oob_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&entry_ok, &hdr_label, &sso_check_label);

    // SSO receiver (#10762): `String(n)`, `${n}` and `"" + n` return a short
    // number's text as an SSO immediate, and the slow arm below materialized
    // a heap copy of it through the intern table (about 175 instructions) to
    // read one byte. The bytes are in the value itself. An all-ASCII payload
    // has one UTF-16 code unit per byte, so byte `index` is the answer; a
    // non-ASCII payload (UTF-8 sequences) keeps the slow arm.
    ctx.current_block = sso_check_idx;
    let is_sso = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let sso_ok = ctx.block().and(I1, &is_sso, &idx_ok);
    let sso_ok = match &string_pristine {
        Some(pristine) => ctx.block().and(I1, &sso_ok, pristine),
        None => sso_ok,
    };
    ctx.block().cond_br(&sso_ok, &sso_label, &slow_label);

    // Dominated by the index range test, so `fptosi` is in range.
    ctx.current_block = sso_idx;
    let sso_idx_i32 = ctx.block().fptosi(DOUBLE, idx_d, I32);
    let sso_idx_i64 = ctx.block().zext(I32, &sso_idx_i32, I64);
    let sso_len_word = ctx.block().lshr(I64, &bits, SHORT_STRING_LEN_SHIFT);
    let sso_len = ctx.block().and(I64, &sso_len_word, "255");
    let sso_in_bounds = ctx.block().icmp_ult(I64, &sso_idx_i64, &sso_len);
    let high_bits = ctx.block().and(I64, &bits, SHORT_STRING_HIGH_BITS);
    let sso_ascii = ctx.block().icmp_eq(I64, &high_bits, "0");
    let sso_bounds_idx = ctx.new_block("cca.sso_bounds");
    let sso_bounds_label = ctx.block_label(sso_bounds_idx);
    ctx.block()
        .cond_br(&sso_ascii, &sso_bounds_label, &slow_label);
    ctx.current_block = sso_bounds_idx;
    let sso_oob_label = if op == Op::CharAt {
        &slow_label
    } else {
        &oob_label
    };
    ctx.block()
        .cond_br(&sso_in_bounds, &sso_fast_label, sso_oob_label);

    // `index < len <= 5`, so the shift is at most 32.
    ctx.current_block = sso_fast_idx;
    let sso_shift = ctx.block().shl(I64, &sso_idx_i64, "3");
    let sso_word = ctx.block().lshr(I64, &bits, &sso_shift);
    let sso_byte = ctx.block().and(I64, &sso_word, "255");
    let sso_val = match op {
        Op::CharCodeAt => ctx.block().uitofp(I64, &sso_byte, DOUBLE),
        Op::CharAt => ascii_char_value(ctx, &sso_byte),
    };
    let sso_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Header block: ASCII + in-range test. Dominated by `handle >= 4096`, so
    // the two loads are safe; dominated by the index range test, so `fptosi`
    // is in range.
    ctx.current_block = hdr_idx;
    let hdr_ptr = ctx.block().inttoptr(I64, &handle);
    let u16_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &hdr_ptr,
        &[(I64, STRING_HEADER_UTF16_LEN_OFFSET)],
    );
    let utf16_len = ctx.block().load(I32, &u16_ptr);
    let blen_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &hdr_ptr,
        &[(I64, STRING_HEADER_BYTE_LEN_OFFSET)],
    );
    let byte_len = ctx.block().load(I32, &blen_ptr);
    let same_lengths = ctx.block().icmp_eq(I32, &utf16_len, &byte_len);
    // A malformed raw payload can have equal byte/code-unit lengths. Only
    // the producer's encoding proof makes that equality an ASCII proof.
    let flags_ptr = ctx
        .block()
        .gep_inbounds(crate::types::I8, &hdr_ptr, &[(I64, "16")]);
    let flags = ctx.block().load(I32, &flags_ptr);
    let validated = ctx.block().and(I32, &flags, "4");
    let validated = ctx.block().icmp_ne(I32, &validated, "0");
    let is_ascii = ctx.block().and(I1, &same_lengths, &validated);
    let idx_i32 = ctx.block().fptosi(DOUBLE, idx_d, I32);
    let in_bounds = ctx.block().icmp_ult(I32, &idx_i32, &utf16_len);
    ctx.block().cond_br(&is_ascii, &bounds_label, &slow_label);
    ctx.current_block = bounds_idx;
    let oob_target = if op == Op::CharAt {
        &slow_label
    } else {
        &oob_label
    };
    ctx.block().cond_br(&in_bounds, &fast_label, oob_target);

    // Fast block: one byte load, zero calls.
    ctx.current_block = fast_idx;
    let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
    let data_ptr =
        ctx.block()
            .gep_inbounds(crate::types::I8, &hdr_ptr, &[(I64, STRING_HEADER_SIZE)]);
    let char_ptr = ctx
        .block()
        .gep_inbounds(crate::types::I8, &data_ptr, &[(I64, &idx_i64)]);
    let byte = ctx.block().load(crate::types::I8, &char_ptr);
    let fast_val = match op {
        Op::CharCodeAt => ctx.block().uitofp(crate::types::I8, &byte, DOUBLE),
        Op::CharAt => {
            let byte_i64 = ctx.block().zext(crate::types::I8, &byte, I64);
            ascii_char_value(ctx, &byte_i64)
        }
    };
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Numeric out-of-bounds indices need no conversion or decoder call.
    ctx.current_block = oob_idx;
    let oob_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Slow block: coercing indices and Unicode keep the existing decoder.
    ctx.current_block = slow_idx;
    let slow_val = if let Some((entry, args, _)) = entry {
        let mut operands: Vec<(crate::types::LlvmType, &str)> = vec![(DOUBLE, recv_box)];
        operands.extend(args.iter().map(|a| (DOUBLE, a.as_str())));
        ctx.block().call(DOUBLE, entry, &operands)
    } else {
        let recv_handle = str_operand_handle_tag_dispatched(ctx, object, recv_box);
        let slow_idx_i32 =
            ctx.block()
                .call(I32, "js_string_number_index_to_i32", &[(DOUBLE, idx_d)]);
        ctx.block().call(
            DOUBLE,
            "js_string_char_code_at",
            &[(I64, &recv_handle), (I32, &slow_idx_i32)],
        )
    };
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let nan = crate::nanbox::double_literal(f64::NAN);
    // `charAt` sends an out-of-range index to the entry, so its oob block is
    // unreachable; it still branches here and needs its incoming value.
    Some(ctx.block().phi(
        DOUBLE,
        &[
            (&fast_val, &fast_pred),
            (&sso_val, &sso_pred),
            (&nan, &oob_pred),
            (&slow_val, &slow_pred),
        ],
    ))
}

#[cfg(test)]
#[path = "char_code_at_tests.rs"]
mod tests;
