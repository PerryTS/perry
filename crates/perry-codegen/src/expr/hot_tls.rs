//! Inline access to the runtime's per-thread hot cache on Apple aarch64.
//!
//! The runtime keeps its hottest per-thread state in one `HotTls` block whose
//! address every thread publishes under a pthread key (`tls_hot.rs`); on this
//! platform the pthread TSD array is addressable straight from
//! `TPIDRRO_EL0`, which is how `pthread_getspecific` itself works. Two runtime
//! entries generated code calls on every dynamically-dispatched method call —
//! `js_implicit_this_set` (the save and the restore) — and the once-per-
//! allocating-activation `js_inline_arena_state` do nothing but that lookup
//! plus one load or store, so the lookup is emitted here instead:
//!
//! ```text
//!   key  = load @PERRY_HOT_TSD_KEY          ; -1 = no direct path
//!   tsd  = mrs tpidrro_el0 & ~7             ; this thread's TSD array
//!   hot  = load tsd[key]                    ; null = not published yet
//!   …    = load / store [hot + FIELD_OFFSET]
//! ```
//!
//! Every miss — no key, unpublished slot, or (for the arena state) a state
//! block the runtime has not lazily initialised — takes the runtime call it
//! replaces, so the two paths are equivalent by construction. The `mrs` is an
//! `asm sideeffect`: LLVM must not hoist or CSE a thread pointer across a
//! point where execution can resume on another thread (the same discipline
//! `tls_hot::darwin_tsd::get` records), and the root-lowering passes already
//! treat inline asm as a non-collecting leaf. The field offsets are pinned by
//! `offset_of!` assertions in `tls_hot.rs`.
//!
//! `PERRY_INLINE_HOT_TLS=0` at compile time disables the inline arm for A/B
//! measurement; every other target always takes the runtime call.

use super::FnCtx;
use crate::types::{I64, I8, PTR};

/// `HotTls::inline_state` — see `tls_hot::HOT_TLS_INLINE_STATE_OFFSET`.
pub(crate) const HOT_TLS_INLINE_STATE_OFFSET: &str = "8";
/// `HotTls::implicit_this` — see `tls_hot::HOT_TLS_IMPLICIT_THIS_OFFSET`.
pub(crate) const HOT_TLS_IMPLICIT_THIS_OFFSET: &str = "128";

/// Is the inline hot-cache access available for this compile's target?
pub(crate) fn inline_hot_tls_enabled(ctx: &FnCtx<'_>) -> bool {
    let triple = ctx.target_triple;
    let apple_aarch64 =
        (triple.starts_with("arm64") || triple.starts_with("aarch64")) && triple.contains("apple");
    apple_aarch64
        && std::env::var_os("PERRY_INLINE_HOT_TLS").as_deref() != Some(std::ffi::OsStr::new("0"))
}

/// The emitted lookup. On return the current block is `fast` (the cache
/// address is `hot`); the caller owns `slow` and must terminate both.
pub(crate) struct HotTlsLookup {
    pub(crate) hot: String,
    pub(crate) slow_idx: usize,
}

/// Emit the lookup described in the module doc, ending the current block.
pub(crate) fn emit_hot_tls_lookup(ctx: &mut FnCtx<'_>, stem: &str) -> HotTlsLookup {
    let tsd_idx = ctx.new_block(&format!("{stem}.hot_tls.tsd"));
    let fast_idx = ctx.new_block(&format!("{stem}.hot_tls.fast"));
    let slow_idx = ctx.new_block(&format!("{stem}.hot_tls.slow"));
    let tsd_label = ctx.block_label(tsd_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let key = {
        let blk = ctx.block();
        let key = blk.load_atomic_monotonic(I64, "@PERRY_HOT_TSD_KEY", 8);
        let has_key = blk.icmp_ne(I64, &key, "-1");
        blk.cond_br(&has_key, &tsd_label, &slow_label);
        key
    };
    ctx.current_block = tsd_idx;
    let hot = {
        let blk = ctx.block();
        let tsd = blk.next_reg();
        blk.emit_raw(format!(
            "  {tsd} = call i64 asm sideeffect \"mrs $0, tpidrro_el0\", \"=r\"()"
        ));
        let base = blk.and(I64, &tsd, "-8");
        let offset = blk.shl(I64, &key, "3");
        let slot_addr = blk.add(I64, &base, &offset);
        let slot = blk.inttoptr(I64, &slot_addr);
        let hot = blk.load(PTR, &slot);
        let published = blk.icmp_ne(PTR, &hot, "null");
        blk.cond_br(&published, &fast_label, &slow_label);
        hot
    };
    ctx.current_block = fast_idx;
    HotTlsLookup { hot, slow_idx }
}

/// Address of the cache field at `offset` bytes, in the current (fast) block.
pub(crate) fn hot_tls_field(ctx: &mut FnCtx<'_>, hot: &str, offset: &str) -> String {
    ctx.block().gep(I8, hot, &[(I64, offset)])
}
