//! The `length` of a typed array or byte view (`Uint8Array`, `Buffer`, any
//! numeric typed array) read from its own header, shared by the annotated
//! `.length` lowering and the unknown-receiver `.length` site.

use crate::types::{I1, I64, I8, PTR};

use super::FnCtx;

/// Branch to `fast_label` when `recv_handle` (a heap address above the handle
/// band, not forwarded per `not_forwarded`, whose GC type byte is `gc_type`)
/// is a typed array or byte view whose `length` is its payload word +0, else
/// to `slow_label`. Emitted into the current block, which it terminates; its
/// blocks are named under `prefix`.
///
/// `views` also admits a fixed view of a plain owner by dereferencing its
/// link. Only a receiver typed as a byte cell may take it: an unproven
/// pointer-tagged native word can carry a byte-family type byte and a wild link.
///
/// Byte views and typed arrays keep a live length at payload +0, including
/// shared storage: detach/resize update that word. Only these proven type
/// bytes are admitted, not native-arena views. Own named metadata or
/// prototype edits withdraw the accessor proof before publication
/// (`PERRY_TYPED_NAMED_PROPS_INVALIDATED`); the slow edge keeps the complete
/// property read.
pub(super) fn emit_byte_cell_length_guard(
    ctx: &mut FnCtx<'_>,
    recv_handle: &str,
    gc_type: &str,
    not_forwarded: &str,
    fast_label: &str,
    slow_label: &str,
    prefix: &str,
    views: bool,
) {
    let owning_byte = ctx.block().icmp_uge(I8, gc_type, "64");
    let indexed_byte = ctx.block().icmp_ule(I8, gc_type, "76");
    let is_typed_array = ctx.block().and(I1, &owning_byte, &indexed_byte);
    let byte_header_idx = ctx.new_block(&format!("{prefix}.byte_header"));
    let byte_header_label = ctx.block_label(byte_header_idx);
    let view_check_idx = views.then(|| ctx.new_block(&format!("{prefix}.view_check")));
    let not_owner_label =
        view_check_idx.map_or_else(|| slow_label.to_string(), |idx| ctx.block_label(idx));
    ctx.block()
        .cond_br(&is_typed_array, &byte_header_label, &not_owner_label);

    if let Some(idx) = view_check_idx {
        // An indexed view keeps its fixed length at payload +0. It is the
        // live length while the view is not length-tracking and its link
        // is a plain owner (no bag, so no own `length`) that is neither
        // RESIZABLE nor DETACHED: only those change an owner's extent.
        ctx.current_block = idx;
        let view_lo = crate::runtime_abi::BYTES_TYPE_BASE | crate::runtime_abi::BYTES_TYPE_VIEW;
        let view_hi = view_lo | 12;
        let is_view_lo = ctx.block().icmp_uge(I8, gc_type, &view_lo.to_string());
        let is_view_hi = ctx.block().icmp_ule(I8, gc_type, &view_hi.to_string());
        let is_view = ctx.block().and(I1, &is_view_lo, &is_view_hi);
        let view_header_idx = ctx.new_block(&format!("{prefix}.view_header"));
        let view_header_label = ctx.block_label(view_header_idx);
        ctx.block()
            .cond_br(&is_view, &view_header_label, slow_label);
        ctx.current_block = view_header_idx;
        let h = crate::expr::byte_cell::header_word(ctx.block(), recv_handle);
        let tracking = ctx.block().and(I64, &h, &(1u64 << 23).to_string());
        let fixed = ctx.block().icmp_eq(I64, &tracking, "0");
        let link_addr = ctx.block().add(
            I64,
            recv_handle,
            &crate::runtime_abi::BYTES_LINK.to_string(),
        );
        let link_ptr = ctx.block().inttoptr(I64, &link_addr);
        let link = ctx.block().load(PTR, &link_ptr);
        let link = ctx.block().ptrtoint(&link, I64);
        let ho = crate::expr::byte_cell::header_word(ctx.block(), &link);
        let mask = 0xe0u64 | (1 << 24) | (1 << 30);
        let state = ctx.block().and(I64, &ho, &mask.to_string());
        let plain_owner = ctx.block().icmp_eq(
            I64,
            &state,
            &crate::runtime_abi::BYTES_TYPE_BASE.to_string(),
        );
        let view_ok = ctx.block().and(I1, &fixed, &plain_owner);
        let view_ok = ctx.block().and(I1, &view_ok, not_forwarded);
        let named_pristine = crate::expr::property_get::typed_named::emit_named_props_pristine(ctx);
        let view_ok = ctx.block().and(I1, &view_ok, &named_pristine);
        ctx.block().cond_br(&view_ok, fast_label, slow_label);
    }

    ctx.current_block = byte_header_idx;
    let link_addr = ctx.block().add(
        I64,
        recv_handle,
        &crate::runtime_abi::BYTES_LINK.to_string(),
    );
    let link_ptr = ctx.block().inttoptr(I64, &link_addr);
    let link = ctx.block().load(PTR, &link_ptr);
    let empty = ctx.block().icmp_eq(PTR, &link, "null");
    let ta_header_ok = ctx.block().and(I1, &empty, not_forwarded);
    let named_pristine = crate::expr::property_get::typed_named::emit_named_props_pristine(ctx);
    let ta_ok = ctx.block().and(I1, &ta_header_ok, &named_pristine);
    ctx.block().cond_br(&ta_ok, fast_label, slow_label);
}
