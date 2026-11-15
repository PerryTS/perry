//! The rep-keyed element store: `a[k] = v` into a receiver the facts give
//! `FactsRep::Arr(r)` with Number elements (`r.elem` F64 or I32, not at
//! rest), through a key that is no proven index (the runtime-key store).
//!
//! Such a receiver holds only plain program Arrays (`index_get::
//! proven_array`, the read side of the same claim): each is a
//! `GC_TYPE_ARRAY` for its whole life with no element descriptor, never
//! frozen, sealed or made non-extensible (wpfacts makes such an Array hard,
//! and a hard Array has no rep), and every present slot holds a plain
//! double. So the store needs no band test, no type or integrity test and no
//! old-value classification:
//! - the forwarding test, unless the storage never moves (`r.unmoved()`);
//! - `idx <u capacity`, then the slot is no hole: with the
//!   `[length, capacity)` hole invariant an own slot is below the length,
//!   and overwriting an own element asks nothing of the prototype chain;
//! - the value is a plain double (tested at run time unless it is widened
//!   from a native integer: the facts say a Number, its encoding may still
//!   be an INT32 box), with a NaN made the canonical one;
//! - the raw store. No pointer is retired or stored: no barrier, no layout
//!   note, and the element kind (F64 or Any) admits a plain double.
//!
//! Every miss (forwarded, out of the capacity, a hole, a growing store, a
//! key at or past 2^31, a boxed value) takes the caller's exact store, which
//! is the store the site had before.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`) tests the receiver's claim and the
//! slot the key names before the store (`array_rep::witness_read`); a miss
//! reports "array rep" and exits 86. `PERRY_FACTS_ARRAY_STORE=0` turns the
//! tier off (`PERRY_FACTS_ARRAY_TIER=0` does too).

use anyhow::Result;
use perry_hir::Expr;

use super::index_get::guarded_array::emit_array_guard_word;
use super::index_get::proven_array::{rep_read, RepRead};
use super::FnCtx;
use crate::codegen::FactsElemRep;
use crate::nanbox::{POINTER_MASK_I64, TAG_HOLE_I64};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `perry_abi::GC_FLAG_FORWARDED` in the header word's second byte.
const FORWARDED_I32: &str = "32768";
/// `0x7FF8_0000_0000_0000`, the one NaN an F64 slot may hold.
const CANONICAL_NAN_I64: &str = "9221120237041090560";

/// `PERRY_FACTS_ARRAY_STORE=0` (`off`, `false`) turns the tier off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_ARRAY_STORE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The receiver's rep for the element store `e` (`object[k] = v`), when the
/// tier takes it (module docs).
pub(crate) fn fact(ctx: &FnCtx<'_>, object: &Expr, e: &Expr) -> Option<RepRead> {
    if !enabled() || ctx.wpfacts_off {
        return None;
    }
    let Expr::IndexSet { element_offset, .. } = e else {
        return None;
    };
    let read = rep_read(ctx, object, *element_offset)?;
    (!read.rep.at_rest && read.rep.elem != FactsElemRep::Boxed).then_some(RepRead {
        // The store bounds its key itself.
        in_floor: false,
        ..read
    })
}

/// `PERRY_STORE_ARM_DIAG=1`: one stderr line per rep-keyed store.
fn diag(ctx: &FnCtx<'_>, read: RepRead, tested: bool) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var("PERRY_STORE_ARM_DIAG").as_deref() == Ok("1")) {
        eprintln!(
            "[perry store arm] {} {} arm=rep elem={:?} floor={} moved={} value={}",
            ctx.func.name,
            read.receiver.label(),
            read.rep.elem,
            read.rep.floor,
            !read.rep.unmoved() as u8,
            if tested { "tested" } else { "int" }
        );
    }
}

/// Store `val_double` at `arr_box[idx_i32]` under `read` (module docs).
/// `fallback` emits the site's exact store into the miss block.
pub(super) fn emit(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    val_double: &str,
    read: RepRead,
    fallback: impl FnOnce(&mut FnCtx<'_>) -> Result<()>,
) -> Result<()> {
    if ctx.block().is_terminated() {
        return Ok(());
    }
    let r = read.rep;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "receiver": read.receiver.label(), "floor": r.floor, "store": true }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        super::index_get::array_rep::witness_read(ctx, arr_box, idx_i32, read, false, false);
    }
    // A value widened from a native i32 is a plain double, never NaN.
    let int_value = ctx
        .func
        .blocks()
        .iter()
        .rev()
        .any(|b| b.widened_i32_source(val_double).is_some());
    diag(ctx, read, !int_value);
    let cold_idx = ctx.new_block("arep.set.cold");
    let merge_idx = ctx.new_block("arep.set.merge");
    let cold_label = ctx.block_label(cold_idx);
    let merge_label = ctx.block_label(merge_idx);
    // `cond` continues in a new block `name`; a miss takes the exact store.
    let step = |ctx: &mut FnCtx<'_>, cond: &str, name: &str| {
        let on = ctx.new_block(name);
        let on_l = ctx.block_label(on);
        ctx.block().cond_br(cond, &on_l, &cold_label);
        ctx.current_block = on;
    };
    let bits = ctx.block().bitcast_double_to_i64(arr_box);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    if !r.unmoved() {
        // Growth moves the storage and forwards this header.
        let word = emit_array_guard_word(ctx.block(), &handle);
        let moved = ctx.block().and(I32, &word, FORWARDED_I32);
        let live = ctx.block().icmp_eq(I32, &moved, "0");
        step(ctx, &live, "arep.set.live");
    }
    let capacity = {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &handle);
        let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        blk.load(I32, &cap_ptr)
    };
    let inside = ctx.block().icmp_ult(I32, idx_i32, &capacity);
    step(ctx, &inside, "arep.set.inside");
    let addr = {
        let blk = ctx.block();
        if r.floor > 0 {
            // A defined length: the elements start behind the header.
            let idx = blk.zext(I32, idx_i32, I64);
            let offset = blk.shl(I64, &idx, "3");
            let base = blk.add(
                I64,
                &handle,
                &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
            );
            blk.add(I64, &base, &offset)
        } else {
            blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32)
        }
    };
    let slot = ctx.block().inttoptr(I64, &addr);
    let old = ctx.block().load(I64, &slot);
    let mut own = ctx.block().icmp_ne(I64, &old, TAG_HOLE_I64);
    if !int_value {
        let blk = ctx.block();
        let val_bits = blk.bitcast_double_to_i64(val_double);
        let plain = crate::codegen::emit_plain_number_bits_test(blk, &val_bits);
        own = blk.and(I1, &own, &plain);
    }
    step(ctx, &own, "arep.set.store");
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_INBOUNDS);
    {
        let blk = ctx.block();
        let stored = if int_value {
            val_double.to_string()
        } else {
            let is_nan = blk.fcmp("uno", val_double, val_double);
            let nan = blk.bitcast_i64_to_double(CANONICAL_NAN_I64);
            blk.select(I1, &is_nan, DOUBLE, &nan, val_double)
        };
        // GC_STORE_AUDIT(POINTER_FREE): a plain double over an own Number
        // slot of an Array whose elements the facts prove Numbers: no edge
        // is created or retired, so no barrier and no layout note.
        blk.store(DOUBLE, &stored, &slot);
        blk.br(&merge_label);
    }
    ctx.current_block = cold_idx;
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_FALLBACK);
    fallback(ctx)?;
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    Ok(())
}
