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
//! - `idx <u capacity`, unless the facts bound the key to `[0, floor)`
//!   (`read.in_floor`): wpfacts `ArrRep` (C) gives every Array of the set
//!   `capacity >= floor` for its whole life, so such a key is always inside
//!   the storage;
//! - the slot is no hole: with the `[length, capacity)` hole invariant an
//!   own slot is below the length, and overwriting an own element asks
//!   nothing of the prototype chain. A key in `[0, floor)` of a `dense` rep
//!   needs no test: `ArrRep` (D) says no index below the floor is ever a
//!   hole, and (L) `length >= floor` makes it an own element;
//! - the value is a plain double: tested at run time unless it is widened
//!   from a native integer or the caller proves it (`value_plain`: native
//!   fp arithmetic, `type_analysis::expr_produces_canonical_raw_f64`, or a
//!   value the facts prove a Number, `facts_canonical::of`, never an INT32
//!   box). The element facts say a Number, but a Number's encoding may still
//!   be an INT32 box, so they alone do not prove the VALUE's word;
//! - the raw store, with no NaN select: a value that is a plain double
//!   (proven, or tested above) is outside the tag band, and the program's
//!   NaN invariant (`perry-runtime array::canonical_raw_f64`, #10779) keeps
//!   every NaN in circulation the hardware one (`0x7FF8_…`, or `0xFFF8_…`
//!   from `fneg` or the x86 default NaN): every raw-byte ingress (typed
//!   arrays, DataView, Buffer, wasm, FFI, structured clone) collapses a NaN
//!   payload, and arithmetic on such NaNs yields such NaNs. A slot then
//!   reads back as the same Number through any read; the raw stores of
//!   `expr_produces_canonical_raw_f64` values rest on the same invariant.
//! - No pointer is retired or stored: no barrier, no layout
//!   note, and the element kind (F64 or Any) admits a plain double.
//!
//! With every test proven (storage that never moves, a dense key below the
//! floor, a proven plain value) there is no miss and no miss arm.
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
    // `in_floor`: the facts bound the store's key to `[0, floor)` at every
    // copy. The bound is the key signs' (`ElisionFacts::key_signs`, wpfacts
    // `element_keys::collect_signs`): the hull of the key node's integer
    // interval over every copy of every site at this offset, stores
    // included. `element_keys` hulls only the reads there, which may be
    // other expressions (`a[i][0] = v`: the read `a[i]` starts at the
    // same offset), so it never bounds a store's key.
    let in_floor = ctx
        .wpfacts
        .key_signs
        .get(element_offset)
        .is_some_and(|&(lo, hi)| lo >= 0 && hi < i64::from(read.rep.floor));
    (!read.rep.at_rest && read.rep.elem != FactsElemRep::Boxed).then_some(RepRead {
        in_floor,
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
    value_plain: bool,
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
    if value_plain && !int_value && crate::verify_facts::enabled() {
        super::facts_canonical::witness(
            ctx,
            val_double,
            super::facts_canonical::Num::Number,
            "array rep store value",
        )?;
    }
    let plain_value = int_value || value_plain;
    diag(ctx, read, !plain_value);
    let floor_key = read.in_floor && r.floor > 0;
    if r.unmoved() && floor_key && r.dense && plain_value {
        // No test is left: the store is the address and the raw store.
        super::store_census::bump(ctx, super::store_census::ELEM_STORE_INBOUNDS);
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(arr_box);
        let handle = blk.and(I64, &bits, POINTER_MASK_I64);
        let idx = blk.zext(I32, idx_i32, I64);
        let offset = blk.shl(I64, &idx, "3");
        let base = blk.add(
            I64,
            &handle,
            &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
        );
        let addr = blk.add(I64, &base, &offset);
        let slot = blk.inttoptr(I64, &addr);
        // GC_STORE_AUDIT(POINTER_FREE): as below.
        blk.store(DOUBLE, val_double, &slot);
        return Ok(());
    }
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
    // A key in `[0, floor)` is inside the storage (`ArrRep` C:
    // `capacity >= floor`); `floor > 0` then also fixes the element base.
    let capacity = if floor_key {
        None
    } else {
        let capacity = {
            let blk = ctx.block();
            let ptr = blk.inttoptr(I64, &handle);
            let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
            blk.load(I32, &cap_ptr)
        };
        let inside = ctx.block().icmp_ult(I32, idx_i32, &capacity);
        step(ctx, &inside, "arep.set.inside");
        Some(capacity)
    };
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
            let capacity = capacity.expect("a key outside the floor loads the capacity");
            blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32)
        }
    };
    let slot = ctx.block().inttoptr(I64, &addr);
    // The slot holds an own element: tested unless the facts prove it
    // (`ArrRep` D, L: a dense key below the floor).
    let mut own = if floor_key && r.dense {
        None
    } else {
        let old = ctx.block().load(I64, &slot);
        Some(ctx.block().icmp_ne(I64, &old, TAG_HOLE_I64))
    };
    if !plain_value {
        let blk = ctx.block();
        let val_bits = blk.bitcast_double_to_i64(val_double);
        let plain = crate::codegen::emit_plain_number_bits_test(blk, &val_bits);
        own = Some(match own {
            Some(own) => blk.and(I1, &own, &plain),
            None => plain,
        });
    }
    if let Some(own) = own {
        step(ctx, &own, "arep.set.store");
    }
    super::store_census::bump(ctx, super::store_census::ELEM_STORE_INBOUNDS);
    {
        let blk = ctx.block();
        let stored = val_double.to_string();
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
