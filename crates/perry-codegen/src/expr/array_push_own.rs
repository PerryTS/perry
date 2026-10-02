//! The own-`push` exit of the `Expr::ArrayPush` slow tiers (#11021).
//!
//! An own property beats a builtin (#10943), and `arr.push(x)` was the one
//! builtin call the #10943 guard left out: a diamond around it costs the push
//! its inline store (+94 instructions per call, measured, against +7 for
//! `indexOf`). This module closes it WITHOUT a diamond, and at zero cost on the
//! inline store.
//!
//! # Why the inline tier needs no new test
//!
//! Every inline push tier admits its receiver through a mask over
//! `GcHeader::_reserved` that already contains `OBJ_FLAG_ARRAY_DESCRIPTORS`
//! (0x400), and every path that gives an array an own named property arms that
//! bit: both storages of `array_named_property_set` (the pairs reserve and the
//! full-array fallback table) and every `Object.defineProperty` route. It is
//! monotone and survives growth. So an array that owns `push` can never reach
//! the inline store — it fails admission and lands in a slow arm — and the hot
//! path pays nothing: the absence proof rides the instructions it already
//! spends on `_reserved`.
//!
//! # Why every slow arm needs an exit of its own
//!
//! There are five: the spec-order arm, the typed-feedback numeric fallback,
//! the forwarded arm, the realloc arm, and the local tail. Each used to call
//! `js_array_push_f64_spec`, which returns the new HEAD, and computed the
//! expression's value from the array's length afterwards. A receiver that
//! fails the admission mask therefore still ran the builtin: no bail from that
//! call can carry an own method's return value.
//!
//! `js_array_push_f64_spec_or_own` has two exits instead, selected by an i32
//! out-flag: the new head (exactly `js_array_push_f64_spec`, which the tier
//! writes back and measures as before), or the METHOD's return value. The own
//! exit branches straight to [`OwnPushJoin`], whose phi makes that return the
//! expression's value — no head write-back, because nothing was appended, and
//! no recomputed length.
//!
//! > The diamond is not heavy-handed — it is the only instrument that can see
//! > a receiver the flags cannot describe.
//!
//! That sentence from #11021 still holds, and is why this is not a diamond: the
//! flags CAN describe an array that owns `push` — one bit, already tested — so
//! no instrument is needed on the path that cannot hold one. The runtime entry
//! asks the precise question (does this array own `push`, by bytes, without
//! allocating) only once the bit is set.

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

/// The runtime entry every ArrayPush slow arm calls.
pub(super) const PUSH_SPEC_OR_OWN: &str = "js_array_push_f64_spec_or_own";

/// The join where an ArrayPush tier's own-method exits meet its ordinary
/// result. Created before the tier's first slow arm, closed by
/// [`OwnPushJoin::finish`] once the tier has computed its ordinary value.
pub(super) struct OwnPushJoin {
    idx: usize,
    label: String,
    /// `(value, predecessor label)` for each own exit.
    incoming: Vec<(String, String)>,
}

impl OwnPushJoin {
    pub(super) fn new(ctx: &mut FnCtx<'_>) -> Self {
        let idx = ctx.new_block("apush.own.join");
        let label = ctx.block_label(idx);
        OwnPushJoin {
            idx,
            label,
            incoming: Vec::new(),
        }
    }

    /// Emit a slow arm's push of `v` onto `arr_handle`.
    ///
    /// Returns the new head handle (`i64`) and leaves `ctx` in the builtin
    /// continuation, where the caller does exactly what it did after
    /// `js_array_push_f64_spec`. The own exit is recorded here and branches to
    /// the join.
    pub(super) fn emit_push(&mut self, ctx: &mut FnCtx<'_>, arr_handle: &str, v: &str) -> String {
        // An entry-block alloca, never an in-block one: a slow arm inside a
        // loop would otherwise grow the stack every iteration (#10463).
        let own_slot = ctx.func.alloca_entry(I32);
        let own_idx = ctx.new_block("apush.own");
        let builtin_idx = ctx.new_block("apush.own.builtin");
        let own_label = ctx.block_label(own_idx);
        let builtin_label = ctx.block_label(builtin_idx);
        let bits = {
            let blk = ctx.block();
            let bits = blk.call(
                I64,
                PUSH_SPEC_OR_OWN,
                &[(I64, arr_handle), (DOUBLE, v), (PTR, &own_slot)],
            );
            let own = blk.load(I32, &own_slot);
            let took_own = blk.icmp_ne(I32, &own, "0");
            // Unlikely, and said so: without the hint the exit's blocks cost
            // the function's HOT loop a few register moves (+3 instructions
            // per iteration on an object-push loop, shipping profile).
            let took_own = blk.call(I1, "llvm.expect.i1", &[(I1, &took_own), (I1, "false")]);
            blk.cond_br(&took_own, &own_label, &builtin_label);
            bits
        };

        ctx.current_block = own_idx;
        {
            let blk = ctx.block();
            // The call returned the METHOD's result bits on this exit, not a
            // head: it is the expression's value as it stands.
            let value = blk.bitcast_i64_to_double(&bits);
            let end = blk.label.clone();
            blk.br(&self.label);
            self.incoming.push((value, end));
        }

        ctx.current_block = builtin_idx;
        bits
    }

    /// Branch the tier's ordinary result to the join and return the
    /// expression's value: that result, or an own method's return.
    pub(super) fn finish(self, ctx: &mut FnCtx<'_>, value: String) -> String {
        let end = ctx.block().label.clone();
        ctx.block().br(&self.label);
        ctx.current_block = self.idx;
        let mut incoming = self.incoming;
        incoming.push((value, end));
        let refs: Vec<(&str, &str)> = incoming
            .iter()
            .map(|(value, label)| (value.as_str(), label.as_str()))
            .collect();
        ctx.block().phi(DOUBLE, &refs)
    }
}
