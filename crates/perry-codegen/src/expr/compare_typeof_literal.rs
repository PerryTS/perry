//! `typeof value === <literal>` lowered to an `i1` (split from `compare.rs`
//! to keep that file under the 2000-line cap; bodies moved verbatim).

use super::*;

/// `typeof v === "number"` (or `!==`) with the definitely-a-Number cases
/// decided inline and everything else deferred to the shared classifier.
///
/// The classifier answers `Number` only on its final fallthrough, after every
/// NaN-box tag has been excluded and one raw-bit exception has been checked:
/// an untagged typed-array pointer (`top16 == 0 && bits >= 0x10000`, #654). A
/// value is therefore a Number by construction when its top 16 bits are
/// outside the tag range `0x7FF9..=0x7FFF` (that covers every negative double
/// as well) and it is not that raw-pointer shape. INT32-tagged values (which
/// may be class references) and the raw-pointer window keep the complete
/// classifier, so the two routes can never disagree. No double VALUE is ever
/// special: Web Streams handles are `POINTER_TAG` registry ids, so 1048576 is
/// a Number like any other. The inline arm is the ECS `_validID` entity-id
/// check.
fn lower_typeof_number_inline(ctx: &mut FnCtx<'_>, value: &str, negate: bool) -> String {
    let fast_idx = ctx.new_block("typeof.num.fast");
    let slow_idx = ctx.new_block("typeof.num.slow");
    let merge_idx = ctx.new_block("typeof.num.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        // Outside the tag band, sign-closed so HANDLE_TAG (0xFFF9) is
        // excluded with the positive tags 0x7FF9..=0x7FFF.
        let magnitude = blk.and(I64, &bits, "9223372036854775807");
        let untagged = blk.icmp_ult(
            I64,
            &magnitude,
            &crate::nanbox::i64_literal(crate::nanbox::SHORT_STRING_TAG),
        );
        // Not an untagged raw typed-array pointer.
        let top16 = blk.lshr(I64, &bits, "48");
        let top_nonzero = blk.icmp_ne(I64, &top16, "0");
        let below_pointer_floor = blk.icmp_ult(I64, &bits, "65536");
        let not_raw_pointer = blk.or(I1, &top_nonzero, &below_pointer_floor);
        let definitely_number = blk.and(I1, &untagged, &not_raw_pointer);
        blk.cond_br(&definitely_number, &fast_label, &slow_label);
    }
    ctx.current_block = fast_idx;
    let fast_end = {
        let blk = ctx.block();
        let label = blk.label.clone();
        blk.br(&merge_label);
        label
    };
    ctx.current_block = slow_idx;
    let (slow_bit, slow_end) = {
        let tag = ctx
            .block()
            .call(I32, "js_value_typeof_tag", &[(crate::types::DOUBLE, value)]);
        let bit = if negate {
            ctx.block().icmp_ne(I32, &tag, "3")
        } else {
            ctx.block().icmp_eq(I32, &tag, "3")
        };
        let blk = ctx.block();
        let label = blk.label.clone();
        blk.br(&merge_label);
        (bit, label)
    };
    ctx.current_block = merge_idx;
    let fast_bit = if negate { "false" } else { "true" };
    ctx.block()
        .phi(I1, &[(fast_bit, &fast_end), (&slow_bit, &slow_end)])
}

/// `typeof value === <literal>` as an `i1`.
///
/// The classifier (`classify_value_typeof` in `perry-runtime/src/builtins/
/// arithmetic.rs`) decides `undefined` (including the hole sentinel), the two
/// booleans and both string tags with exact bit tests before any registry or
/// heap lookup, so `"undefined"`, `"boolean"` and `"string"` never need the
/// call, and `"number"` keeps its inline definitely-a-Number arm. `"object"` and
/// `"function"` read heap callability inline; `"symbol"` and `"bigint"` use the
/// shared classifier.
pub(super) fn lower_typeof_literal_inline(
    ctx: &mut FnCtx<'_>,
    value: &str,
    expected_tag: u32,
    negate: bool,
) -> String {
    let exact = {
        let blk = ctx.block();
        match expected_tag {
            0 => {
                let bits = blk.bitcast_double_to_i64(value);
                let undefined = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
                let hole = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_HOLE_I64);
                Some(blk.or(I1, &undefined, &hole))
            }
            2 => Some(crate::codegen::emit_typed_i1_guard(blk, value)),
            4 => Some(crate::codegen::emit_typed_string_guard(blk, value)),
            _ => None,
        }
    };
    if let Some(bit) = exact {
        return if negate {
            ctx.block().xor(I1, &bit, "true")
        } else {
            bit
        };
    }
    if expected_tag == 1 || expected_tag == 5 {
        return lower_typeof_object_inline(ctx, value, expected_tag, negate);
    }
    if expected_tag == 3 {
        return lower_typeof_number_inline(ctx, value, negate);
    }
    let tag = ctx
        .block()
        .call(I32, "js_value_typeof_tag", &[(crate::types::DOUBLE, value)]);
    let expected = expected_tag.to_string();
    if negate {
        ctx.block().icmp_ne(I32, &tag, &expected)
    } else {
        ctx.block().icmp_eq(I32, &tag, &expected)
    }
}
