//! Region-local string-array admission (JST slice 4, design F): the read
//! `array[i].length` of a declared `string[]`.
//!
//! Admission proves the receiver is an ordinary unforwarded array without
//! element descriptors, then walks its `[0, length)` slots ONCE in the
//! preheader and misses to the original loop unless every slot holds a heap
//! string or an inline ASCII string. Inside the region (no call, no
//! allocation, no poll) nothing can store into the array or move a string, so
//! the proof holds for every iteration and the per-element read is a tag test
//! and one header load. A hole, `undefined`, an object or a non-ASCII inline
//! string anywhere in the array declines the whole region before any body
//! effect.
use super::*;

pub(super) fn read(e: &Expr) -> Option<(u32, &Expr)> {
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return None;
    };
    if property != "length" {
        return None;
    }
    let Expr::IndexGet { object, index, .. } = object.as_ref() else {
        return None;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return None;
    };
    Some((*id, index.as_ref()))
}

/// `true` for a declared `string[]` binding the region may read.
pub(super) fn declared(ctx: &FnCtx<'_>, id: u32) -> bool {
    super::super::string_length_loop::is_declared_string_array(ctx, id)
}

pub(super) fn admit(ctx: &mut FnCtx<'_>, id: u32, slow: &str) -> Storage {
    let boxed = boxed_binding(ctx, id);
    let bits = ctx.block().bitcast_double_to_i64(&boxed);
    // A heap-pointer-tagged value above the runtime-id band: the header
    // behind it is readable.
    let offset = ctx.block().sub(
        I64,
        &bits,
        crate::expr::array_proto_guard::HEAP_POINTER_BAND_BASE_I64,
    );
    let heap = ctx.block().icmp_ult(
        I64,
        &offset,
        crate::expr::array_proto_guard::HEAP_POINTER_BAND_SPAN_I64,
    );
    branch_guard(ctx, &heap, slow);
    let handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let ordinary =
        crate::expr::array_proto_guard::emit_ordinary_array_admission(ctx.block(), &handle);
    branch_guard(ctx, &ordinary, slow);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let len = ctx.block().load(I32, &ptr);
    let cap_ptr = ctx.block().gep_inbounds(I8, &ptr, &[(I64, "4")]);
    let cap = ctx.block().load(I32, &cap_ptr);
    let within = ctx.block().icmp_ule(I32, &len, &cap);
    branch_guard(ctx, &within, slow);
    let data = ctx.block().array_elements_addr_with_capacity(&handle, &cap);
    scan(ctx, &data, &len, slow);
    Storage {
        raw: handle,
        data,
        len,
        kind: SARRAY_KIND,
        metadata: String::new(),
    }
}

/// Miss unless every slot below `len` holds a heap string or an inline ASCII
/// string. One OR-accumulated verdict, one branch after the walk.
fn scan(ctx: &mut FnCtx<'_>, data: &str, len: &str, slow: &str) {
    let index_slot = ctx.func.alloca_entry(I32);
    let bad_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, "0", &index_slot);
    ctx.block().store(I32, "0", &bad_slot);
    let head = ctx.new_block("indexed.sarray.scan");
    let body = ctx.new_block("indexed.sarray.slot");
    let done = ctx.new_block("indexed.sarray.scanned");
    let (head_l, body_l, done_l) = (
        ctx.block_label(head),
        ctx.block_label(body),
        ctx.block_label(done),
    );
    ctx.block().br(&head_l);

    ctx.current_block = head;
    let index = ctx.block().load(I32, &index_slot);
    let more = ctx.block().icmp_ult(I32, &index, len);
    ctx.block().cond_br(&more, &body_l, &done_l);

    ctx.current_block = body;
    let wide = ctx.block().zext(I32, &index, I64);
    let off = ctx.block().shl(I64, &wide, "3");
    let base = ctx.block().inttoptr(I64, data);
    let slot = ctx.block().gep(I8, &base, &[(I64, &off)]);
    let bits = ctx.block().load(I64, &slot);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    let is_short = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
    let high = ctx.block().and(I64, &bits, SHORT_NON_ASCII_MASK);
    let ascii = ctx.block().icmp_eq(I64, &high, "0");
    let short_ok = ctx.block().and(I1, &is_short, &ascii);
    let ok = ctx.block().or(I1, &is_heap, &short_ok);
    let not_ok = ctx.block().xor(I1, &ok, "true");
    let not_ok = ctx.block().zext(I1, &not_ok, I32);
    let bad = ctx.block().load(I32, &bad_slot);
    let next_bad = ctx.block().or(I32, &bad, &not_ok);
    ctx.block().store(I32, &next_bad, &bad_slot);
    let next = ctx.block().add(I32, &index, "1");
    ctx.block().store(I32, &next, &index_slot);
    ctx.block().br(&head_l);

    ctx.current_block = done;
    let bad = ctx.block().load(I32, &bad_slot);
    let clean = ctx.block().icmp_eq(I32, &bad, "0");
    branch_guard(ctx, &clean, slow);
}

/// The inline-string bytes of a slot that are not ASCII payload: bit 7 of each
/// of the five payload bytes (`string::admit` tests the same mask).
const SHORT_NON_ASCII_MASK: &str = "551911719040";

/// The UTF-16 length of the element at `index`, as a u32. The slot was proven
/// by [`scan`] to be a heap string or an inline ASCII string.
pub(super) fn element_length(ctx: &mut FnCtx<'_>, s: &Storage, index: &str) -> String {
    let off = ctx.block().shl(I64, index, "3");
    let base = ctx.block().inttoptr(I64, &s.data);
    let slot = ctx.block().gep(I8, &base, &[(I64, &off)]);
    let bits = ctx.block().load(I64, &slot);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
    // Read through the string's address when it is a heap string; the array's
    // own header (always readable) stands in for an inline string so the
    // load needs no branch.
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let from = ctx.block().select(I1, &is_heap, I64, &raw, &s.raw);
    let from_ptr = ctx.block().inttoptr(I64, &from);
    let heap_len = ctx.block().load(I32, &from_ptr);
    let short = ctx.block().lshr(I64, &bits, "40");
    let short = ctx.block().and(I64, &short, "255");
    let short_len = ctx.block().trunc(I64, &short, I32);
    ctx.block().select(I1, &is_heap, I32, &heap_len, &short_len)
}
