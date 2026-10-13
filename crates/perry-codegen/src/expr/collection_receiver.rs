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

/// A builtin collection kind the compiler proves by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConstructedKind {
    Map,
    Set,
}

/// The kind of a receiver the compiler itself allocated: `new Map(...)` /
/// `new Set(...)` lower to `js_map_alloc` / `js_set_alloc`, whose result is a
/// `GC_TYPE_MAP` / `GC_TYPE_SET` object; an immutable local bound to one,
/// never written again, keeps it. A declared `Map<K, V>` is NOT such a proof
/// (a subclass instance or a plain object can carry it), and neither is a
/// local the TDZ can expose before its initializer ran.
pub(crate) fn constructed_kind(ctx: &FnCtx<'_>, e: &perry_hir::Expr) -> Option<ConstructedKind> {
    use perry_hir::Expr;
    match e {
        Expr::MapNew | Expr::MapNewFromArray(_) => Some(ConstructedKind::Map),
        Expr::SetNew | Expr::SetNewFromArray(_) => Some(ConstructedKind::Set),
        Expr::LocalGet(id)
            if !ctx.reassigned_locals.contains(id)
                && !ctx.tdz_boxes.contains(id)
                && !ctx.boxed_vars.contains(id) =>
        {
            ctx.constructed_kind_locals.get(id).copied()
        }
        _ => None,
    }
}

/// Entry selection for a `Map` / `Set` operation (design 4.3
/// `select_entry`): when the compiler created the receiver as `kind`, the
/// kind-typed entry takes its payload with no receiver check at the site;
/// every other receiver keeps [`unbox_collection_receiver`] and the generic
/// entry. Returns the unboxed handle and whether the typed entry applies.
/// The typed call replaces the generic one one for one (same arguments).
pub(crate) fn select_collection_receiver(
    ctx: &mut FnCtx<'_>,
    recv_expr: &perry_hir::Expr,
    recv_box: &str,
    method: &str,
    kind: ConstructedKind,
) -> (String, bool) {
    if constructed_kind(ctx, recv_expr) == Some(kind) {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv_box);
        return (blk.and(I64, &bits, POINTER_MASK_I64), true);
    }
    (unbox_collection_receiver(ctx, recv_box, method), false)
}

/// `set.size` / `map.size` on a static `Set` / `Map` receiver: the kind-typed
/// entry when the compiler constructed the receiver, the checked generic
/// entry otherwise. Answers the size as a double.
pub(crate) fn lower_collection_size(
    ctx: &mut FnCtx<'_>,
    object: &perry_hir::Expr,
    kind: ConstructedKind,
) -> anyhow::Result<String> {
    let recv_box = crate::expr::lower_expr(ctx, object)?;
    let (recv_handle, typed) = select_collection_receiver(ctx, object, &recv_box, "size", kind);
    let entry = match (kind, typed) {
        (ConstructedKind::Set, true) => "js_set_size__Set",
        (ConstructedKind::Set, false) => "js_set_size",
        (ConstructedKind::Map, true) => "js_map_size__Map",
        (ConstructedKind::Map, false) => "js_map_size",
    };
    let blk = ctx.block();
    let i32_v = blk.call(crate::types::I32, entry, &[(I64, &recv_handle)]);
    Ok(blk.sitofp(crate::types::I32, &i32_v, DOUBLE))
}
