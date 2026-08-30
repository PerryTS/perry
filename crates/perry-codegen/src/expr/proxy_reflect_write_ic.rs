//! Stable-tombstone slot validation shared by the static and dynamic write ICs.
//!
//! A stable-tombstone receiver deliberately keeps its ShapeId while deletes
//! leave `TAG_HOLE` in individual slots. Both write ICs therefore need the
//! same cold validation diamond before their ordinary direct-store block.

use crate::types::{DOUBLE, I16, I64};

use super::FnCtx;

/// Runtime `OBJ_FLAG_STABLE_TOMBSTONES`. The bit is already present in each
/// generated write guard's `_reserved` load; on a hit it gates the one deleted
/// slot check required while the receiver keeps a stable ShapeId (#9064).
const STABLE_TOMBSTONES_OBJ_FLAG: u16 = 0x400;

pub(super) struct StableTombstoneSlotCheck {
    validate_idx: usize,
    live_idx: usize,
    validate_label: String,
    live_label: String,
}

impl StableTombstoneSlotCheck {
    /// Create the cold validation block followed by the ordinary live-slot
    /// continuation, preserving the caller's basic-block creation order.
    pub(super) fn new(ctx: &mut FnCtx<'_>, validate_name: &str, live_name: &str) -> Self {
        let validate_idx = ctx.new_block(validate_name);
        let live_idx = ctx.new_block(live_name);
        Self {
            validate_idx,
            live_idx,
            validate_label: ctx.block_label(validate_idx),
            live_label: ctx.block_label(live_idx),
        }
    }

    /// Branch ordinary receivers straight to the direct store. Stable-
    /// tombstone receivers first prove that the cached slot is still live.
    pub(super) fn emit(
        &self,
        ctx: &mut FnCtx<'_>,
        reserved: &str,
        slot_ptr: &str,
        deleted_label: &str,
    ) {
        let stable_tombstones =
            ctx.block()
                .and(I16, reserved, &STABLE_TOMBSTONES_OBJ_FLAG.to_string());
        let needs_slot_validation = ctx.block().icmp_ne(I16, &stable_tombstones, "0");
        ctx.block().cond_br(
            &needs_slot_validation,
            &self.validate_label,
            &self.live_label,
        );

        ctx.current_block = self.validate_idx;
        let old_value = ctx.block().load(DOUBLE, slot_ptr);
        let old_bits = ctx.block().bitcast_double_to_i64(&old_value);
        let deleted = ctx
            .block()
            .icmp_eq(I64, &old_bits, crate::nanbox::TAG_HOLE_I64);
        ctx.block()
            .cond_br(&deleted, deleted_label, &self.live_label);
    }

    pub(super) fn enter_live(&self, ctx: &mut FnCtx<'_>) {
        ctx.current_block = self.live_idx;
    }
}
