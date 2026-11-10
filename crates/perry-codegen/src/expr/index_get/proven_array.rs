//! Element reads whose receiver wpfacts proves (representation RFC §10.7,
//! phase P4).
//!
//! **The rep-keyed tier** (DESIGN-p4l1 §4.2, PR-C2; `RepRead`). A local,
//! parameter or module const the facts give `FactsRep::Arr(r)` holds only
//! plain program Arrays: each is a `GC_TYPE_ARRAY` for its whole life, with
//! no element descriptor and no deleted element (no hard marker), `length`
//! and `capacity` of at least `r.floor`, no hole below the floor when
//! `r.dense`, storage that never moves when `r.fixed`, and, when its length
//! is defined (which `r.floor > 0` implies), elements that start at the
//! front of its allocation. So the read needs no band test and no type
//! test, and:
//! - the forwarding test only when the storage may move (`!r.fixed`);
//! - no bound when the key's interval lies in `[0, r.floor)` (wpfacts
//!   `element_keys`), else `idx <u capacity`: the runtime keeps
//!   `[length, capacity)` hole-filled (`array::storage`), so a slot past
//!   the length reads as the hole it is. The floor is the least length
//!   over the Arrays the value may hold, so an index at or past it may
//!   still name an element;
//! - the slot at `h + 24 + 8 * idx` when `r.floor > 0`, else through the
//!   header's capacity and size (a `shift` moves the front of an Array
//!   whose length is not defined);
//! - no hole test below a dense floor, else one `TAG_HOLE` compare;
//! - in a number context, the "is a plain Number" test, unless the slot is
//!   below a dense floor and the read's value is a proven Number (wpfacts
//!   `number_element_reads`, P3 B1).
//!
//! A miss (forwarded, out of bounds, a hole, not a Number) takes the shared
//! cold read with the exact arguments, the generic `Get`. A read below a
//! dense floor of fixed storage has no miss and no cold arm.
//!
//! A scalar-replaced literal has no heap header and is never admitted
//! (REVIEW-p4l1 R6). `PERRY_FACTS_ARRAY_TIER=0` turns the tier off: every
//! read keeps the tier it had.
//!
//! **The fixed-storage read** (wpfacts `ModuleBindings::element_reads`,
//! `FactsElementRead`), for a receiver the tier above does not name (a
//! field read, a temporary).
//!
//! Every receiver of such a read is a program Array that keeps its birth
//! length for the whole program: no store, builtin, `defineProperty` or
//! escape moves it, and no named property is ever written on it. Its
//! storage therefore never grows, so it is never forwarded
//! (`GC_FLAG_FORWARDED` comes only from growth), it holds at least
//! `FactsElementRead::len` slots, and no slot carries a descriptor. The
//! value is a pointer to that Array: no band, type or forwarding test is
//! needed, only the slot.
//!
//! A slot holds its own data or a hole. A hole reads through the prototype
//! chain, an index at or past `len` is absent: both, and in a number
//! context an element that is not a plain double, take the shared cold
//! read with the exact arguments, the generic `Get`. The proof names no
//! runtime state, so there is nothing to invalidate.

use perry_hir::Expr;

use crate::codegen::{FactsArrRep, FactsElementRead, FactsRep};
use crate::nanbox::{POINTER_MASK_I64, TAG_HOLE_I64};
use crate::types::{DOUBLE, I32, I64, I8};

use super::{typed_feedback_emission_enabled, FnCtx};

/// `perry_abi::GC_FLAG_FORWARDED` in the header word's second byte.
const FORWARDED_I32: &str = "32768";

/// A read the facts prove (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Proven {
    /// The receiver's representation.
    Rep(RepRead),
    /// A fixed-storage receiver of any other expression.
    Fixed(FactsElementRead),
}

/// A read of a receiver of rep `Arr(rep)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RepRead {
    pub(crate) rep: FactsArrRep,
    /// The key is an integer in `[0, rep.floor)` at every copy of the read.
    pub(crate) in_floor: bool,
    /// The read's value is a proven Number: a plain double.
    pub(crate) number: bool,
    /// The receiver local, for the check-mode report.
    pub(crate) local: u32,
}

impl RepRead {
    /// The slot is below a dense floor: present, never a hole.
    fn present(self) -> bool {
        self.rep.dense && self.in_floor
    }

    /// The read cannot miss: no test, no cold arm.
    pub(crate) fn total(self, numeric: bool) -> bool {
        self.rep.fixed && self.in_floor && self.rep.dense && (!numeric || self.number)
    }
}

/// `PERRY_FACTS_ARRAY_TIER=0` turns the rep-keyed tier off.
pub(crate) fn tier_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_ARRAY_TIER").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The rep-keyed proof of reading `object` at the site `element_offset`:
/// the one query for the receiver's representation, and the key's interval
/// at the site against its floor.
pub(crate) fn rep_read(ctx: &FnCtx<'_>, object: &Expr, element_offset: u32) -> Option<RepRead> {
    if !tier_enabled() || typed_feedback_emission_enabled() {
        return None;
    }
    let Expr::LocalGet(id) = object else {
        return None;
    };
    // A local the codegen keeps in scalar slots holds no array word (R6).
    if ctx.scalar_replaced_arrays.contains_key(id)
        || ctx.array_row_aliases.contains_key(id)
        || ctx.scalar_replaced.contains_key(id)
    {
        return None;
    }
    let FactsRep::Arr(rep) = crate::codegen::facts_rep::local(ctx, *id)? else {
        return None;
    };
    let in_floor = element_offset != 0
        && ctx
            .wpfacts
            .element_keys
            .get(&element_offset)
            .is_some_and(|&(lo, hi)| lo >= 0 && hi < i64::from(rep.floor));
    let number = crate::expr::facts_canonical::element(ctx, element_offset)
        == Some(crate::expr::facts_canonical::Num::Number);
    Some(RepRead {
        rep,
        in_floor,
        number,
        local: *id,
    })
}

/// `e` is an element read the tier loads below a dense floor and whose
/// value is a proven Number: a present own element, never `undefined` or
/// `null`, so `e ?? d` is `e`.
pub(crate) fn number_in_floor(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::IndexGet {
        object,
        element_offset,
        ..
    } = e
    else {
        return false;
    };
    !ctx.wpfacts_off
        && rep_read(ctx, object, *element_offset)
            .is_some_and(|read| read.in_floor && read.rep.dense && read.number)
}

/// `left ?? d` is `left` (`number_in_floor`): the consumption record, and
/// in check mode the test that `value`, its lowering, is a plain double.
pub(crate) fn coalesce_is_left(ctx: &mut FnCtx<'_>, value: &str) -> anyhow::Result<()> {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::CanonicalNumber,
        || serde_json::json!({ "coalesce": "left" }),
    );
    crate::facts_consumed::used();
    crate::expr::facts_canonical::witness(
        ctx,
        value,
        crate::expr::facts_canonical::Num::Number,
        "coalesce left",
    )
}

/// The wpfacts proof for the read of `object` at `element_offset`, when the
/// facts cover this function and the build emits no observing guards.
pub(super) fn fact(ctx: &FnCtx<'_>, object: &Expr, element_offset: u32) -> Option<Proven> {
    if ctx.wpfacts_off || typed_feedback_emission_enabled() {
        return None;
    }
    if let Some(read) = rep_read(ctx, object, element_offset) {
        return Some(Proven::Rep(read));
    }
    if element_offset == 0 {
        return None;
    }
    ctx.wpfacts
        .element_reads
        .get(&element_offset)
        .copied()
        .map(Proven::Fixed)
}

/// Read `arr_box[idx_i32]` under `fact`. `numeric`: the consumer is a
/// number context, whose hit is a plain double (the guarded numeric tier's
/// contract) and whose cold read coerces.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: Proven,
    numeric: bool,
) -> String {
    match fact {
        Proven::Rep(read) => lower_rep(ctx, arr_box, idx_i32, read, numeric),
        Proven::Fixed(fact) => lower_fixed(ctx, arr_box, idx_i32, fact, numeric),
    }
}

/// The rep-keyed read (module docs).
fn lower_rep(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    read: RepRead,
    numeric: bool,
) -> String {
    let r = read.rep;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "local": read.local, "floor": r.floor, "in_floor": read.in_floor }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        super::array_rep::witness_read(ctx, arr_box, idx_i32, read, numeric);
    }
    let cold_idx = (!read.total(numeric)).then(|| ctx.new_block("arep.cold"));
    let cold_label = cold_idx.map(|i| ctx.block_label(i));
    // `cond` continues in a new block `name`; a miss takes the cold read.
    let step = |ctx: &mut FnCtx<'_>, cond: &str, name: &str| {
        let on = ctx.new_block(name);
        let on_l = ctx.block_label(on);
        let cold_l = cold_label.as_deref().expect("a tested read has a cold arm");
        ctx.block().cond_br(cond, &on_l, cold_l);
        ctx.current_block = on;
    };
    let bits = ctx.block().bitcast_double_to_i64(arr_box);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    if !r.fixed {
        // Growth moves the storage and forwards this header.
        let word = super::guarded_array::emit_array_guard_word(ctx.block(), &handle);
        let moved = ctx.block().and(I32, &word, FORWARDED_I32);
        let live = ctx.block().icmp_eq(I32, &moved, "0");
        step(ctx, &live, "arep.live");
    }
    let mut capacity = None;
    // In the floor `capacity >= floor > idx`: the slot exists.
    if !read.in_floor {
        let ptr = ctx.block().inttoptr(I64, &handle);
        let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
        let cap = ctx.block().load(I32, &cap_ptr);
        let inside = ctx.block().icmp_ult(I32, idx_i32, &cap);
        step(ctx, &inside, "arep.inside");
        capacity = Some(cap);
    }
    let blk = ctx.block();
    let addr = if r.floor > 0 {
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
        let cap = capacity.expect("no key is below a floor of 0");
        blk.array_element_addr_with_capacity(&handle, &cap, idx_i32)
    };
    let slot = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &slot);
    let value = blk.bitcast_i64_to_double(&raw);
    if numeric && !(read.present() && read.number) {
        let plain = crate::codegen::emit_plain_number_bits_test(blk, &raw);
        step(ctx, &plain, "arep.number");
    } else if !numeric && !read.present() {
        let own = blk.icmp_ne(I64, &raw, TAG_HOLE_I64);
        step(ctx, &own, "arep.own");
    }
    let Some(cold_idx) = cold_idx else {
        return value;
    };
    let merge_idx = ctx.new_block("arep.merge");
    let merge_label = ctx.block_label(merge_idx);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = cold_idx;
    let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
    // Mode 2: the exact arguments, no absence proof (`emit_read_cold`).
    let cold = super::guarded_array::emit_read_cold(
        ctx, arr_box, "2", &idx_box, "0", numeric, numeric, None,
    );
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&value, &load_end), (&cold, &cold_end)])
}

/// The fixed-storage read (module docs).
fn lower_fixed(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: FactsElementRead,
    numeric: bool,
) -> String {
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ElementRead,
        || serde_json::json!({ "length": fact.len, "key_in_bounds": fact.key_in_bounds }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        witness(ctx, arr_box, idx_i32, fact);
    }
    let load_idx = ctx.new_block("parr.load");
    let cold_idx = ctx.new_block("parr.cold");
    let merge_idx = ctx.new_block("parr.merge");
    let load_label = ctx.block_label(load_idx);
    let cold_label = ctx.block_label(cold_idx);
    let merge_label = ctx.block_label(merge_idx);
    if fact.key_in_bounds {
        ctx.block().br(&load_label);
    } else {
        let len = fact.len.to_string();
        let inside = ctx.block().icmp_ult(I32, idx_i32, &len);
        ctx.block().cond_br(&inside, &load_label, &cold_label);
    }

    ctx.current_block = load_idx;
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr_box);
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &handle);
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let capacity = blk.load(I32, &cap_ptr);
    let addr = blk.array_element_addr_with_capacity(&handle, &capacity, idx_i32);
    let slot = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &slot);
    let value = blk.bitcast_i64_to_double(&raw);
    let hit = if numeric {
        crate::codegen::emit_plain_number_bits_test(blk, &raw)
    } else {
        blk.icmp_ne(I64, &raw, crate::nanbox::TAG_HOLE_I64)
    };
    let load_end = blk.label.clone();
    blk.cond_br(&hit, &merge_label, &cold_label);

    ctx.current_block = cold_idx;
    let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
    // Mode 2: the exact arguments, no absence proof (`emit_read_cold`).
    let cold = super::guarded_array::emit_read_cold(
        ctx, arr_box, "2", &idx_box, "0", numeric, numeric, None,
    );
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&value, &load_end), (&cold, &cold_end)])
}

/// `PERRY_VERIFY_FACTS=1`: the claim, tested before the proven load. The
/// value is an ordinary heap Array, not forwarded, without element
/// descriptors, of exactly `len` elements over a store of at least `len`
/// slots, and the key is below `len` when
/// the facts say so. A miss reports the false proof and exits 86.
fn witness(ctx: &mut FnCtx<'_>, arr_box: &str, idx_i32: &str, fact: FactsElementRead) {
    use super::guarded_array::{
        emit_array_guard_word, emit_array_guard_word_ok, HEAP_POINTER_BAND_BASE_I64,
        HEAP_POINTER_BAND_SPAN_I64,
    };
    let deref = ctx.new_block("parr.verify_deref");
    let miss = ctx.new_block("parr.verify_miss");
    let next = ctx.new_block("parr.verify_admitted");
    let (deref_l, miss_l, next_l) = (
        ctx.block_label(deref),
        ctx.block_label(miss),
        ctx.block_label(next),
    );
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr_box);
    let offset = blk.sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = blk.icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    blk.cond_br(&heap, &deref_l, &miss_l);
    ctx.current_block = deref;
    let blk = ctx.block();
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let word = emit_array_guard_word(blk, &handle);
    let ordinary = emit_array_guard_word_ok(blk, &word, &handle);
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    let exact = blk.icmp_eq(I32, &length, &fact.len.to_string());
    // The proven load addresses the slot without a capacity test: the store
    // must hold `len` slots (a sparse length over a smaller store does not).
    let cap_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
    let capacity = blk.load(I32, &cap_ptr);
    let backed = blk.icmp_uge(I32, &capacity, &fact.len.to_string());
    let exact = blk.and(crate::types::I1, &exact, &backed);
    let mut ok = blk.and(crate::types::I1, &ordinary, &exact);
    if fact.key_in_bounds {
        let inside = blk.icmp_ult(I32, idx_i32, &fact.len.to_string());
        ok = blk.and(crate::types::I1, &ok, &inside);
    }
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ElementRead,
        || serde_json::json!({ "length": fact.len, "key_in_bounds": fact.key_in_bounds }),
    );
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("proven element read of length {}", fact.len),
        crate::verify_facts::Fact::ElementRead,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, arr_box)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
