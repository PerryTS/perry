//! Header-based object/function typeof comparisons.

use super::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{I1, I32, I64};

/// Heap object/function comparisons read the GC kind and [[Call]] fact in
/// one word. Primitive and external-handle representations retain the shared
/// classifier; the legacy STRING-kind symbol representation needs its proof.
pub(super) fn lower_typeof_object_inline(
    ctx: &mut FnCtx<'_>,
    value: &str,
    expected: u32,
    negate: bool,
) -> String {
    let heap_idx = ctx.new_block("typeof.obj.heap");
    let answer_idx = ctx.new_block("typeof.obj.answer");
    let other_idx = ctx.new_block("typeof.obj.other");
    let merge_idx = ctx.new_block("typeof.obj.merge");
    let heap_label = ctx.block_label(heap_idx);
    let answer_label = ctx.block_label(answer_idx);
    let other_label = ctx.block_label(other_idx);
    let merge_label = ctx.block_label(merge_idx);
    let bits = ctx.block().bitcast_double_to_i64(value);
    let bias =
        (crate::nanbox::POINTER_TAG | crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64).to_string();
    let span = ((1u64 << 48) - crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64).to_string();
    let biased = ctx.block().sub(I64, &bits, &bias);
    let heap = ctx.block().icmp_ult(I64, &biased, &span);
    ctx.block().cond_br(&heap, &heap_label, &other_label);

    ctx.current_block = heap_idx;
    let addr = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let header = ctx
        .block()
        .sub(I64, &addr, &crate::runtime_abi::GC_HEADER_SIZE.to_string());
    let ptr = ctx.block().inttoptr(I64, &header);
    let word = ctx.block().load(I32, &ptr);
    let kind = ctx.block().and(I32, &word, "255");
    let legacy_symbol =
        ctx.block()
            .icmp_eq(I32, &kind, &crate::runtime_abi::GC_TYPE_STRING.to_string());
    ctx.block()
        .cond_br(&legacy_symbol, &other_label, &answer_label);

    ctx.current_block = answer_idx;
    let closure = ctx
        .block()
        .icmp_eq(I32, &kind, &crate::runtime_abi::GC_TYPE_CLOSURE.to_string());
    let object = crate::target_layout::emit_gc_type_is_object(ctx.block(), I32, &kind);
    let proxy = ctx
        .block()
        .icmp_eq(I32, &kind, &crate::runtime_abi::GC_TYPE_PROXY.to_string());
    let has_fact = ctx.block().or(I1, &object, &proxy);
    let flag = ctx.block().and(
        I32,
        &word,
        &((crate::runtime_abi::GC_OBJECT_CALLABLE as u32) << 16).to_string(),
    );
    let flagged = ctx.block().icmp_ne(I32, &flag, "0");
    let callable = ctx.block().and(I1, &has_fact, &flagged);
    let function = ctx.block().or(I1, &closure, &callable);
    let mut answer = if expected == 5 {
        function
    } else {
        let non_function = ctx.block().xor(I1, &function, "true");
        let non_symbol =
            ctx.block()
                .icmp_ne(I32, &kind, &crate::runtime_abi::GC_TYPE_SYMBOL.to_string());
        ctx.block().and(I1, &non_function, &non_symbol)
    };
    if negate {
        answer = ctx.block().xor(I1, &answer, "true");
    }
    let answer_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = other_idx;
    let tag = ctx
        .block()
        .call(I32, "js_value_typeof_tag", &[(crate::types::DOUBLE, value)]);
    let other = if negate {
        ctx.block().icmp_ne(I32, &tag, &expected.to_string())
    } else {
        ctx.block().icmp_eq(I32, &tag, &expected.to_string())
    };
    let other_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I1, &[(&answer, &answer_end), (&other, &other_end)])
}
