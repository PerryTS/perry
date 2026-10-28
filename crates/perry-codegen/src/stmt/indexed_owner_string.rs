//! Region-local string admission. No raw storage survives a call or poll.
use super::*;

pub(super) fn read(e: &Expr) -> Option<(u32, &Expr)> {
    let Expr::Call { callee, args, .. } = e else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return None;
    };
    (property == "charCodeAt" && args.len() == 1).then(|| (*id, &args[0]))
}

pub(super) fn prove_pristine(ctx: &mut FnCtx<'_>, slow: &str) {
    // A type selects this operation; the live primitive and prototype prove
    // its semantics. No body call can change the prototype after admission.
    let patched =
        crate::builtin_proto::emit_patched_test(ctx, crate::builtin_proto::BuiltinProto::String);
    let pristine = ctx.block().xor(I1, &patched, "true");
    branch_guard(ctx, &pristine, slow);
}

pub(super) fn admit(ctx: &mut FnCtx<'_>, id: u32, slow: &str) -> Storage {
    let value = boxed_binding(ctx, id);
    let bits = ctx.block().bitcast_double_to_i64(&value);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let short = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let short_check = ctx.new_block("indexed.string.short.check");
    let short_block = ctx.new_block("indexed.string.short");
    let heap_block = ctx.new_block("indexed.string.heap");
    let merge = ctx.new_block("indexed.string.storage");
    let short_check_l = ctx.block_label(short_check);
    let short_l = ctx.block_label(short_block);
    let heap_l = ctx.block_label(heap_block);
    let merge_l = ctx.block_label(merge);
    ctx.block().cond_br(&heap, &heap_l, &short_check_l);
    ctx.current_block = short_check;
    ctx.block().cond_br(&short, &short_l, slow);

    ctx.current_block = short_block;
    let high = ctx.block().and(I64, &bits, "551911719040");
    let ascii = ctx.block().icmp_eq(I64, &high, "0");
    branch_guard(ctx, &ascii, slow);
    let length = ctx.block().lshr(I64, &bits, "40");
    let length = ctx.block().and(I64, &length, "255");
    let short_len = ctx.block().trunc(I64, &length, I32);
    // Materialize immediate bytes in a native stack slot. Only the five
    // payload bytes are read, in the target's little-endian SSO layout.
    let bytes = ctx.func.alloca_entry(I64);
    ctx.block().store(I64, &bits, &bytes);
    let short_data = ctx.block().ptrtoint(&bytes, I64);
    let short_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = heap_block;
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let valid = ctx.block().icmp_uge(I64, &raw, "4096");
    branch_guard(ctx, &valid, slow);
    let ptr = ctx.block().inttoptr(I64, &raw);
    let heap_len = ctx.block().load(I32, &ptr);
    let byte_ptr = ctx.block().gep_inbounds(I8, &ptr, &[(I64, "4")]);
    let byte_len = ctx.block().load(I32, &byte_ptr);
    let flags_ptr = ctx.block().gep_inbounds(I8, &ptr, &[(I64, "16")]);
    let flags = ctx.block().load(I32, &flags_ptr);
    let validated = ctx.block().and(I32, &flags, "4");
    let validated = ctx.block().icmp_ne(I32, &validated, "0");
    let same = ctx.block().icmp_eq(I32, &heap_len, &byte_len);
    // Equal byte/UTF-16 lengths imply ASCII only for validated WTF-8.
    // Raw invalid bytes retain the existing replacement/decoder semantics.
    let ascii = ctx.block().and(I1, &validated, &same);
    branch_guard(ctx, &ascii, slow);
    let heap_data = ctx.block().add(I64, &raw, "20");
    let heap_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge;
    let data = ctx
        .block()
        .phi(I64, &[(&short_data, &short_end), (&heap_data, &heap_end)]);
    let len = ctx
        .block()
        .phi(I32, &[(&short_len, &short_end), (&heap_len, &heap_end)]);
    Storage {
        raw: bits,
        data,
        len,
        kind: STRING_KIND,
        metadata: String::new(),
    }
}
