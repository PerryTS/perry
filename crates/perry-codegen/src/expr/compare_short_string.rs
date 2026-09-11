//! Word-sized ASCII ordering for generic heap-string relations. Bounds and
//! actual bytes are checked; all other strings retain the UTF-16 helper.
use super::{FnCtx, STRING_HEADER_BYTE_LEN_OFFSET, STRING_HEADER_SIZE};
use crate::types::{I1, I32, I64, I8};

pub(super) fn heap_string_order(ctx: &mut FnCtx<'_>, a: &str, b: &str) -> String {
    if !matches!(
        ctx.target_triple.split('-').next().unwrap_or(""),
        "aarch64" | "arm64" | "arm64_32" | "x86_64" | "i686" | "i386" | "riscv64" | "wasm32"
    ) {
        return ctx
            .block()
            .call(I32, "js_string_compare", &[(I64, a), (I64, b)]);
    }
    let header_idx = ctx.new_block("strord.header");
    let words_idx = ctx.new_block("strord.words");
    let ascii_idx = ctx.new_block("strord.ascii");
    let slow_idx = ctx.new_block("strord.utf16");
    let merge_idx = ctx.new_block("strord.merge");
    let header_l = ctx.block_label(header_idx);
    let words_l = ctx.block_label(words_idx);
    let ascii_l = ctx.block_label(ascii_idx);
    let slow_l = ctx.block_label(slow_idx);
    let merge_l = ctx.block_label(merge_idx);
    let a_valid = ctx.block().icmp_ugt(I64, a, "4095");
    let b_valid = ctx.block().icmp_ugt(I64, b, "4095");
    let valid = ctx.block().and(I1, &a_valid, &b_valid);
    ctx.block().cond_br(&valid, &header_l, &slow_l);

    ctx.current_block = header_idx;
    let ap = ctx.block().inttoptr(I64, a);
    let bp = ctx.block().inttoptr(I64, b);
    let alenp = ctx
        .block()
        .gep_inbounds(I8, &ap, &[(I64, STRING_HEADER_BYTE_LEN_OFFSET)]);
    let blenp = ctx
        .block()
        .gep_inbounds(I8, &bp, &[(I64, STRING_HEADER_BYTE_LEN_OFFSET)]);
    let alen = ctx.block().load(I32, &alenp);
    let blen = ctx.block().load(I32, &blenp);
    let a_shorter = ctx.block().icmp_ult(I32, &alen, &blen);
    let common = ctx.block().select(I1, &a_shorter, I32, &alen, &blen);
    let tail = ctx.block().sub(I32, &common, "8");
    let in_range = ctx.block().icmp_ule(I32, &tail, "8");
    ctx.block().cond_br(&in_range, &words_l, &slow_l);

    ctx.current_block = words_idx;
    // 8 <= common <= 16 proves both overlapping eight-byte loads are wholly
    // inside both payloads. StringHeader is 20 bytes, so use alignment one.
    let start = STRING_HEADER_SIZE.to_string();
    let tail64 = ctx.block().zext(I32, &tail, I64);
    let last = ctx.block().add(I64, &tail64, &start);
    let mut words = Vec::with_capacity(4);
    for (ptr, offset) in [(&ap, &start), (&bp, &start), (&ap, &last), (&bp, &last)] {
        let p = ctx.block().gep_inbounds(I8, ptr, &[(I64, offset)]);
        words.push(ctx.block().load_aligned(I64, &p, 1));
    }
    let first_bits = ctx.block().or(I64, &words[0], &words[1]);
    let last_bits = ctx.block().or(I64, &words[2], &words[3]);
    let all_bits = ctx.block().or(I64, &first_bits, &last_bits);
    let high_bits = ctx.block().and(I64, &all_bits, "-9187201950435737472");
    let ascii = ctx.block().icmp_eq(I64, &high_bits, "0");
    ctx.block().cond_br(&ascii, &ascii_l, &slow_l);

    ctx.current_block = ascii_idx;
    // Prefer the first unequal word. Its big-endian integer order is the
    // byte order; ASCII bytes are exactly their UTF-16 code units.
    let first_diff = ctx.block().icmp_ne(I64, &words[0], &words[1]);
    let left = ctx
        .block()
        .select(I1, &first_diff, I64, &words[0], &words[2]);
    let right = ctx
        .block()
        .select(I1, &first_diff, I64, &words[1], &words[3]);
    let left = ctx.block().call(I64, "llvm.bswap.i64", &[(I64, &left)]);
    let right = ctx.block().call(I64, "llvm.bswap.i64", &[(I64, &right)]);
    let equal = ctx.block().icmp_eq(I64, &left, &right);
    let byte_less = ctx.block().icmp_ult(I64, &left, &right);
    let length_equal = ctx.block().icmp_eq(I32, &alen, &blen);
    let length_greater = ctx.block().select(I1, &length_equal, I32, "0", "1");
    let length_order = ctx
        .block()
        .select(I1, &a_shorter, I32, "-1", &length_greater);
    let byte_order = ctx.block().select(I1, &byte_less, I32, "-1", "1");
    let fast = ctx
        .block()
        .select(I1, &equal, I32, &length_order, &byte_order);
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = slow_idx;
    let slow = ctx
        .block()
        .call(I32, "js_string_compare", &[(I64, a), (I64, b)]);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I32, &[(&fast, &fast_pred), (&slow, &slow_pred)])
}
