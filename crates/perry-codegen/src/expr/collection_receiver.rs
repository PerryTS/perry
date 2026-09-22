//! The receiver check in front of every static-type `Map` / `Set` fast path
//! (#10446).
//!
//! `s.add(v)`, `m.get(k)`, `this.labels.has(x)`, `m.forEach(cb)`, ... lower to
//! a direct `js_set_*` / `js_map_*` call when the receiver's DECLARED type is
//! `Set<T>` / `Map<K, V>`, and those helpers take the receiver as its unboxed
//! 48-bit payload. A declaration is not a runtime fact. `undefined` (an
//! uninitialized field, a missing option) unboxes to the address `0x1`, which
//! `js_set_add` / `js_map_get` dereferenced: SIGSEGV with no JS stack instead
//! of the catchable `TypeError: Cannot read properties of undefined (reading
//! 'add')` the same call throws when the receiver is untyped. mongodb 7.5.0
//! died that way in `MongoError.addErrorLabel` about 100 ms after connecting.
//!
//! [`unbox_collection_receiver`] replaces the bare `unbox_to_i64` at those
//! sites. The fast path costs one compare of the box's top 16 bits against the
//! object tag, which a genuine `Map` / `Set` (or a subclass instance, or any
//! other object the runtime helpers already brand-check) always passes. The
//! miss path keeps passing through the two word shapes the helpers have always
//! accepted without a tag — an untagged raw word and a JS handle — and throws
//! for every primitive, none of which can carry a collection method.

use crate::expr::FnCtx;
use crate::nanbox::{POINTER_MASK_I64, POINTER_TAG_TOP16_I64};
use crate::types::{DOUBLE, I1, I64, PTR};

/// Top 16 bits of `JS_HANDLE_TAG` (`0x7FFB`).
const JS_HANDLE_TAG_TOP16_I64: &str = "32763";

/// Check that `recv_box` can be a `Map` / `Set` receiver for method `method`,
/// then return its unboxed handle (what `unbox_to_i64` returns).
///
/// Emits at the current block, which is left at the check's success block:
///
/// ```text
///   %bits  = bitcast double %recv to i64
///   %top16 = lshr i64 %bits, 48
///   %obj   = icmp eq i64 %top16, 32765        ; POINTER_TAG
///   br i1 %obj, label %collection_recv.ok, label %collection_recv.miss
/// collection_recv.miss:                        ; untagged raw word or JS handle
///   br i1 (%top16 == 0 | %top16 == 0x7FFB), label %ok, label %throw
/// collection_recv.throw:
///   call void @js_throw_collection_receiver_type_error(%recv, "<method>")
///   unreachable
/// collection_recv.ok:
///   %handle = and i64 %bits, POINTER_MASK
/// ```
///
/// Emit it after the call's operands are lowered, on the box the runtime call
/// consumes (a re-read box on a rooted path): the check never collects, and
/// its throwing arm never returns, so it adds no collection point between the
/// receiver's root and its use.
pub(crate) fn unbox_collection_receiver(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    method: &str,
) -> String {
    let (bits, top16, is_object) = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_object = blk.icmp_eq(I64, &top16, POINTER_TAG_TOP16_I64);
        (bits, top16, is_object)
    };
    let miss_idx = ctx.new_block("collection_recv.miss");
    let throw_idx = ctx.new_block("collection_recv.throw");
    let ok_idx = ctx.new_block("collection_recv.ok");
    let miss_label = ctx.block_label(miss_idx);
    let throw_label = ctx.block_label(throw_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&is_object, &ok_label, &miss_label);

    ctx.current_block = miss_idx;
    {
        let blk = ctx.block();
        let is_raw_word = blk.icmp_eq(I64, &top16, "0");
        let is_js_handle = blk.icmp_eq(I64, &top16, JS_HANDLE_TAG_TOP16_I64);
        let passes = blk.or(I1, &is_raw_word, &is_js_handle);
        blk.cond_br(&passes, &ok_label, &throw_label);
    }

    ctx.current_block = throw_idx;
    let method_idx = ctx.strings.intern(method);
    let method_entry = ctx.strings.entry(method_idx);
    let method_bytes = format!("@{}", method_entry.bytes_global);
    let method_len = method_entry.byte_len.to_string();
    {
        let blk = ctx.block();
        blk.call_void(
            "js_throw_collection_receiver_type_error",
            &[(DOUBLE, recv_box), (PTR, &method_bytes), (I64, &method_len)],
        );
        blk.unreachable();
    }

    ctx.current_block = ok_idx;
    ctx.block().and(I64, &bits, POINTER_MASK_I64)
}
