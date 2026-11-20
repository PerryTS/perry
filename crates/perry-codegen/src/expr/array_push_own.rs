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
//! Every inline push tier admits its receiver only with a null named layout
//! (`ArrayHeader.named`, object model §6a), and every own named property of an
//! array — `array_named_property_set` and every `Object.defineProperty` route —
//! lives in that layout. It is
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

/// The runtime entry every ArrayPush slow arm calls with
/// `PERRY_PUSH_METHOD_TIERS=0`: an own `push` of an ARRAY receiver exits, any
/// other receiver takes the builtin append.
pub(super) const PUSH_SPEC_OR_OWN: &str = "js_array_push_f64_spec_or_own";

/// The same entry, which also performs the real `Get(recv, "push")` for a
/// receiver that is no Array at all (an object behind a `number[]`
/// annotation): its own or inherited user `push` exits like an Array's own.
pub(super) const PUSH_SPEC_OR_METHOD: &str = "js_array_push_f64_spec_or_method";

/// `PERRY_PUSH_METHOD_TIERS=0` (`off`, `false`) keeps the push tiers that call
/// the builtin without resolving `push` on the receiver:
///
/// * the guarded numeric tier trusts `js_typed_feedback_numeric_array_push_guard`,
///   which admits an Array with a named layout (an own `push`, an own accessor)
///   or an own `[[Prototype]]`;
/// * the native `array.push_single` / `array.push` arm (a field receiver in
///   value position, in a constructor, or with several arguments) appends with
///   no test of the receiver;
/// * a slow arm's receiver that is no Array takes the builtin's array-like
///   append.
pub(crate) fn method_tiers_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_PUSH_METHOD_TIERS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The slow arms' runtime entry under the switch.
pub(super) fn push_entry() -> &'static str {
    if method_tiers_enabled() {
        PUSH_SPEC_OR_METHOD
    } else {
        PUSH_SPEC_OR_OWN
    }
}

/// Branch to `heap_label` when the NaN-boxed `recv_box` is a heap pointer
/// above the runtime-id band (one subtract, one unsigned compare: the
/// precondition of reading a header behind it), else to `other_label`.
pub(super) fn emit_heap_pointer_branch(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    heap_label: &str,
    other_label: &str,
) {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(recv_box);
    let biased = blk.sub(
        I64,
        &bits,
        crate::expr::array_proto_guard::HEAP_POINTER_BAND_BASE_I64,
    );
    let heap = blk.icmp_ult(
        I64,
        &biased,
        crate::expr::array_proto_guard::HEAP_POINTER_BAND_SPAN_I64,
    );
    blk.cond_br(&heap, heap_label, other_label);
}

/// The join where an ArrayPush tier's own-method exits meet its ordinary
/// result. Created before the tier's first slow arm, closed by
/// [`OwnPushJoin::finish`] once the tier has computed its ordinary value.
pub(super) struct OwnPushJoin {
    idx: usize,
    label: String,
    /// `(value, predecessor label)` for each own exit.
    incoming: Vec<(String, String)>,
    /// The boxed receiver, when the tier must test that it is a heap pointer
    /// before a runtime entry is handed its unboxed address
    /// (`method_site::inline_tier_needs_pointer_test`).
    receiver_test: Option<String>,
}

impl OwnPushJoin {
    pub(super) fn new(ctx: &mut FnCtx<'_>) -> Self {
        let idx = ctx.new_block("apush.own.join");
        let label = ctx.block_label(idx);
        OwnPushJoin {
            idx,
            label,
            incoming: Vec::new(),
            receiver_test: None,
        }
    }

    /// [`OwnPushJoin::new`] whose slow arms first test that the boxed
    /// receiver `recv_box` is a heap pointer (when `test`): the runtime entry
    /// takes the unboxed ADDRESS, and the low bits of `null`, a Number, a
    /// String or a boolean are no address (a `null` receiver used to come back
    /// as the "head" address 2, written into the local). A miss is the
    /// ordinary `Get(recv, "push")` + `Call`: a primitive's `push` is missing,
    /// so it throws the call's TypeError as node does.
    pub(super) fn with_receiver_test(ctx: &mut FnCtx<'_>, recv_box: &str, test: bool) -> Self {
        let mut join = Self::new(ctx);
        if test {
            join.receiver_test = Some(recv_box.to_string());
        }
        join
    }

    /// The later slow arms are reached only through a header admission that
    /// already proved the receiver a heap pointer: they need no test.
    pub(super) fn receiver_is_heap_pointer(&mut self) {
        self.receiver_test = None;
    }

    /// The ordinary method call on a receiver that is no heap pointer, as an
    /// exit of the join; `ctx` continues in `heap_label`'s block.
    fn emit_non_pointer_exit(&mut self, ctx: &mut FnCtx<'_>, recv_box: &str, v: &str) {
        let heap_idx = ctx.new_block("apush.recv.heap");
        let other_idx = ctx.new_block("apush.recv.other");
        let heap_label = ctx.block_label(heap_idx);
        let other_label = ctx.block_label(other_idx);
        emit_heap_pointer_branch(ctx, recv_box, &heap_label, &other_label);
        ctx.current_block = other_idx;
        let value = crate::builtin_proto::emit_generic_call(
            ctx,
            crate::builtin_proto::GenericRead::Receiver,
            recv_box,
            "push",
            &[v.to_string()],
        );
        let end = ctx.block().label.clone();
        ctx.block().br(&self.label);
        self.incoming.push((value, end));
        ctx.current_block = heap_idx;
    }

    /// Emit a slow arm's push of `v` onto `arr_handle`.
    ///
    /// Returns the new head handle (`i64`) and leaves `ctx` in the builtin
    /// continuation, where the caller does exactly what it did after
    /// `js_array_push_f64_spec`. The own exit is recorded here and branches to
    /// the join.
    pub(super) fn emit_push(&mut self, ctx: &mut FnCtx<'_>, arr_handle: &str, v: &str) -> String {
        if let Some(recv_box) = self.receiver_test.clone() {
            self.emit_non_pointer_exit(ctx, &recv_box, v);
        }
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
                push_entry(),
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
