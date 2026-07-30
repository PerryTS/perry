//! Inline shadow-slot stores (#7086).
//!
//! # What this replaces
//!
//! Every store to a GC-rooted local used to be an `extern "C"` call —
//! `js_shadow_slot_bind(idx, &local)` for a pointer-capable value,
//! `js_shadow_slot_set(idx, 0)` for the "dead from here" clear. #7079 made
//! those functions cheap internally, but a call costs twice: the call itself,
//! and the fact that it is **opaque to LLVM**, which forces a spill of every
//! live value around it and blocks hoisting across it.
//!
//! Read out of the shipped `aarch64` archive, `js_shadow_slot_bind`'s fast path
//! was ~35 instructions: a 4-instruction prologue/epilogue pair, a **TLSDESC
//! indirect call** (`adrp`/`ldr`/`add`/`blr` plus the resolver) to reach the
//! thread-local, a lazy destructor-registration check, then the six or so
//! instructions that actually do the work. The inline form below is those six
//! plus two guards, with no call and no TLS sequence at all.
//!
//! # How the thread-local block is reached
//!
//! Not by re-deriving the TLS address in generated code — that would have to
//! model Rust's TLS model per platform (TLSDESC on this Linux build, `tlv` on
//! macOS) and would be a second, unverified path to the same memory.
//!
//! Instead the address is *obtained from the runtime* and cached for the
//! activation. `js_shadow_frame_enter` is `js_shadow_frame_push` with the
//! address of this thread's `ShadowStackState` as its return value, so codegen
//! pays exactly the one TLS lookup per activation that the push already paid.
//! The pointer goes into an entry alloca; every slot store loads it back.
//! Because it comes from the runtime's own `SHADOW.with`, it is the same
//! memory `js_shadow_slot_set` writes, by construction rather than by
//! coincidence — and `js_shadow_state_addr` lets a Rust test poke the buffer
//! through these very offsets and read it back through the runtime accessor.
//!
//! Caching it is sound because it is the address of a `const`-initialized,
//! drop-free `thread_local!`: fixed for the thread's lifetime, never
//! reallocated. The *buffer* it points at does move when a deeper frame grows
//! it, which is exactly why `ptr`, `len` and `frame_top` are re-loaded from the
//! state at every store instead of a frame base being cached. One activation of
//! a compiled function runs entirely on one thread, so the cached pointer never
//! escapes to another.
//!
//! # The three root properties
//!
//! * **Liveness** — the inline store writes the same `ShadowEntry.value` and
//!   sets the same `SLOT_ACTIVE` bit in `meta` that the runtime function does,
//!   at the same index, so `visit_shadow_stack_root_slots` marks it identically.
//! * **Rewritability** — `meta` still carries the bound compiled-local address,
//!   with the same alignment fallback, so an evacuating collection rewrites the
//!   alloca the mutator reads after the safepoint, not just the mirror.
//! * **The value the mutator stored** — the value is read from the local slot
//!   at the store site, in the same position the call occupied, and written
//!   immediately. Nothing re-reads the slot at a later safepoint.
//!
//! The incremental-mark root shading barrier is emitted inline too, behind the
//! same `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` gate the runtime and
//! `emit_persistent_shadow_root_barrier` already use.

use super::*;

use crate::types::{I1, I32, I64, PTR};

/// Byte offset of `ShadowStackState::ptr`.
///
/// This block mirrors `perry_runtime::gc::roots::SHADOW_STATE_*`. `perry-codegen`
/// deliberately does not depend on `perry-runtime`, so the two copies are
/// pinned together by `perry`'s `shadow_layout_contract` test, which does
/// depend on both and fails if either moves.
pub const SHADOW_STATE_PTR_OFFSET: u64 = 0;
/// Byte offset of `ShadowStackState::len`.
pub const SHADOW_STATE_LEN_OFFSET: u64 = 8;
/// Byte offset of `ShadowStackState::frame_top`.
pub const SHADOW_STATE_FRAME_TOP_OFFSET: u64 = 24;
/// `size_of::<ShadowEntry>()`. A power of two, so indexing is a shift.
pub const SHADOW_ENTRY_SIZE: u64 = 16;
/// `log2(SHADOW_ENTRY_SIZE)`.
pub const SHADOW_ENTRY_SHIFT: u64 = 4;
/// Byte offset of `ShadowEntry::meta` within an entry.
pub const SHADOW_ENTRY_META_OFFSET: u64 = 8;
/// Liveness bit in `ShadowEntry::meta`.
pub const SHADOW_SLOT_ACTIVE_BIT: u64 = 1;
/// Header entries a frame reserves; `frame_top == frame_handle + this`.
pub const SHADOW_STACK_HEADER_SLOTS: u64 = 1;

const _: () = {
    assert!(SHADOW_ENTRY_SIZE == 1 << SHADOW_ENTRY_SHIFT);
    assert!(SHADOW_ENTRY_META_OFFSET < SHADOW_ENTRY_SIZE);
    assert!(SHADOW_SLOT_ACTIVE_BIT == 1);
};

/// What the inline sequence writes into the entry.
enum InlineSlotWrite<'a> {
    /// Mirror `*local_slot` and bind the entry to `local_slot`. Equivalent to
    /// `js_shadow_slot_bind(idx, local_slot)`.
    Bind { local_slot: &'a str },
    /// Zero the value and drop the liveness bit, keeping the binding.
    /// Equivalent to `js_shadow_slot_set(idx, 0)`.
    Clear,
}

/// Emit the inline equivalent of `js_shadow_slot_bind(slot_idx, local_slot)`.
///
/// Returns `false` when this function has no cached state pointer (so the
/// caller must fall back to the `extern "C"` call).
pub(crate) fn emit_inline_slot_bind(
    ctx: &mut FnCtx<'_>,
    slot_idx: u32,
    local_slot: &str,
) -> bool {
    emit_inline_slot_write(ctx, slot_idx, InlineSlotWrite::Bind { local_slot })
}

/// Emit the inline equivalent of `js_shadow_slot_set(slot_idx, 0)`.
pub(crate) fn emit_inline_slot_clear(ctx: &mut FnCtx<'_>, slot_idx: u32) -> bool {
    emit_inline_slot_write(ctx, slot_idx, InlineSlotWrite::Clear)
}

fn emit_inline_slot_write(
    ctx: &mut FnCtx<'_>,
    slot_idx: u32,
    what: InlineSlotWrite<'_>,
) -> bool {
    if !crate::codegen::helpers::inline_shadow_slot_enabled() {
        return false;
    }
    let Some(state_slot) = ctx.func.shadow_state_slot().map(str::to_owned) else {
        return false;
    };

    // The slow arm is not dead code kept for tidiness: the state alloca is
    // null-initialized in the entry block, so any path that could reach a slot
    // store before the frame push ran still writes the root — through the
    // runtime function, which does its own TLS lookup. Where the push provably
    // dominates (every shipped path), `js_shadow_frame_enter`'s `nonnull`
    // return lets LLVM fold this branch away entirely.
    let slow_idx = ctx.new_block("ss.slow");
    let chk_top_idx = ctx.new_block("ss.chk_top");
    let chk_len_idx = ctx.new_block("ss.chk_len");
    let store_idx = ctx.new_block("ss.store");
    let done_idx = ctx.new_block("ss.done");
    let slow_label = ctx.block_label(slow_idx);
    let chk_top_label = ctx.block_label(chk_top_idx);
    let chk_len_label = ctx.block_label(chk_len_idx);
    let store_label = ctx.block_label(store_idx);
    let done_label = ctx.block_label(done_idx);

    let state = ctx.block().load(PTR, &state_slot);
    let state_null = ctx.block().icmp_eq(PTR, &state, "null");
    ctx.block()
        .cond_br(&state_null, &slow_label, &chk_top_label);

    // --- slow arm: the runtime function, byte-for-byte the old behaviour ---
    ctx.current_block = slow_idx;
    match what {
        InlineSlotWrite::Bind { local_slot } => ctx.block().call_void(
            "js_shadow_slot_bind",
            &[(I32, &slot_idx.to_string()), (PTR, local_slot)],
        ),
        InlineSlotWrite::Clear => ctx.block().call_void(
            "js_shadow_slot_set",
            &[(I32, &slot_idx.to_string()), (I64, "0")],
        ),
    }
    ctx.block().br(&done_label);

    // --- `if top == usize::MAX { return }` ---
    ctx.current_block = chk_top_idx;
    let frame_top_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &state,
        &[(I64, &SHADOW_STATE_FRAME_TOP_OFFSET.to_string())],
    );
    let frame_top = ctx.block().load(I64, &frame_top_ptr);
    // `usize::MAX` is `-1` as an i64 bit pattern.
    let no_frame = ctx.block().icmp_eq(I64, &frame_top, "-1");
    ctx.block()
        .cond_br(&no_frame, &done_label, &chk_len_label);

    // --- `let slot = top + idx; if slot >= len { return }` ---
    //
    // The `usize::MAX` test above is what makes this addition safe to do
    // untrapped: with `top` ruled out as the sentinel it is a real buffer
    // index, so `top + idx` cannot wrap. Testing only `slot < len` would let
    // the sentinel through — `usize::MAX + idx` wraps to `idx - 1`, which is
    // in bounds and would corrupt a *different* frame's entry.
    ctx.current_block = chk_len_idx;
    let slot = ctx
        .block()
        .add(I64, &frame_top, &u64::from(slot_idx).to_string());
    let len_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &state,
        &[(I64, &SHADOW_STATE_LEN_OFFSET.to_string())],
    );
    let len = ctx.block().load(I64, &len_ptr);
    let in_bounds = ctx.block().icmp_ult(I64, &slot, &len);
    ctx.block()
        .cond_br(&in_bounds, &store_label, &done_label);

    // --- the entry write ---
    ctx.current_block = store_idx;
    let buf = if SHADOW_STATE_PTR_OFFSET == 0 {
        ctx.block().load(PTR, &state)
    } else {
        let p = ctx.block().gep_inbounds(
            crate::types::I8,
            &state,
            &[(I64, &SHADOW_STATE_PTR_OFFSET.to_string())],
        );
        ctx.block().load(PTR, &p)
    };
    let byte_off = ctx
        .block()
        .shl(I64, &slot, &SHADOW_ENTRY_SHIFT.to_string());
    let entry = ctx
        .block()
        .gep_inbounds(crate::types::I8, &buf, &[(I64, &byte_off)]);
    let meta_ptr = ctx.block().gep_inbounds(
        crate::types::I8,
        &entry,
        &[(I64, &SHADOW_ENTRY_META_OFFSET.to_string())],
    );

    match what {
        InlineSlotWrite::Bind { local_slot } => {
            // Snapshot the word the mutator just stored. This load sits
            // immediately after the caller's `store` to the same alloca, so
            // LLVM forwards it; it is never re-read at a later safepoint.
            let value = ctx.block().load(I64, local_slot);
            let raw = ctx.block().ptrtoint(local_slot, I64);
            // `bound_slot_meta`: an address whose bit 0 would collide with the
            // liveness tag is recorded active-but-unbound rather than
            // truncated, so the collector is never handed a mis-derived
            // address to write a forwarded pointer into. Compiled local slots
            // are `i64`/`double` allocas and so 8-byte aligned, which lets
            // LLVM fold this select away; it exists for the mis-emitted case.
            let low = ctx
                .block()
                .and(I64, &raw, &SHADOW_SLOT_ACTIVE_BIT.to_string());
            let aligned = ctx.block().icmp_eq(I64, &low, "0");
            let bound = ctx.block().select(I1, &aligned, I64, &raw, "0");
            let meta = ctx
                .block()
                .or(I64, &bound, &SHADOW_SLOT_ACTIVE_BIT.to_string());
            ctx.block().store(I64, &value, &entry);
            ctx.block().store(I64, &meta, &meta_ptr);
            emit_inline_root_shading_barrier(ctx, &value, &done_label);
        }
        InlineSlotWrite::Clear => {
            // Codegen's "dead from here" clear: drop the liveness bit but keep
            // the binding, so a later re-activation still writes through to
            // the same compiled local slot. No shading barrier — a zero value
            // is not a heap reference.
            let old_meta = ctx.block().load(I64, &meta_ptr);
            let cleared = ctx.block().and(
                I64,
                &old_meta,
                &format!("{}", !SHADOW_SLOT_ACTIVE_BIT as i64),
            );
            ctx.block().store(I64, "0", &entry);
            ctx.block().store(I64, &cleared, &meta_ptr);
            ctx.block().br(&done_label);
        }
    }

    ctx.current_block = done_idx;
    true
}

/// The incremental-mark root shading barrier, gated inline on
/// `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`.
///
/// Identical in kind to `emit_persistent_shadow_root_barrier` and to the
/// runtime's own `root_shading_barrier`: a zero count *proves* this thread's
/// `INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null, because
/// `incremental_mark_barrier_enable` installs the thread-local before
/// incrementing the count. Skipping the call on a zero count is therefore
/// observationally identical, not a weaker barrier.
///
/// Terminates the current block with a branch to `done_label`.
fn emit_inline_root_shading_barrier(ctx: &mut FnCtx<'_>, value_bits: &str, done_label: &str) {
    let active =
        ctx.block()
            .load_atomic_seq_cst(I32, "@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT", 4);
    let needed = ctx.block().icmp_ne(I32, &active, "0");
    let barrier_idx = ctx.new_block("ss.barrier");
    let barrier_label = ctx.block_label(barrier_idx);
    ctx.block().cond_br(&needed, &barrier_label, done_label);

    ctx.current_block = barrier_idx;
    ctx.block()
        .call_void("js_write_barrier_root_nanbox", &[(I64, value_bits)]);
    ctx.block().br(done_label);
}
