//! Element reads whose receiver wpfacts proves (representation RFC §10.7,
//! phase P4).
//!
//! **The rep-keyed tier** (DESIGN-p4l1 §4.2, PR-C2 and PR-C3; `RepRead`).
//! A local, parameter or module const the facts give `FactsRep::Arr(r)`,
//! and a field read whose every class holds `Arr` in the field
//! (`wpfacts_access::field_array`, `r` their join), holds only
//! plain program Arrays: each is a `GC_TYPE_ARRAY` for its whole life, with
//! no element descriptor and no deleted element (no hard marker), `length`
//! and `capacity` of at least `r.floor`, no hole below the floor when
//! `r.dense`, storage that never moves when `r.unmoved()` (a fixed length,
//! or an at-rest Array's reserved capacity), and, when its length
//! is defined (which `r.floor > 0` implies), elements that start at the
//! front of its allocation. So the read needs no band test and no type
//! test, and:
//! - the forwarding test only when the storage may move (`!r.unmoved()`);
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
//! A field read call (b1, `lower_call::field_read_call`) whose own index
//! is in the floor (wpfacts `FieldReadCall::in_floor`) reads here too
//! (`lower_in_floor`): the method body's read joins every caller's key,
//! the call has its own.
//!
//! **The fixed-storage read** (wpfacts `ModuleBindings::element_reads`,
//! `FactsElementRead`), for a receiver the tier above does not name (a
//! temporary, a field the facts do not give `Arr`).
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
    /// The key is an integer in `[0, key_floor)` at every copy of the read.
    pub(crate) in_floor: bool,
    /// The floor the key is below: `rep.floor`, or a field read call's own
    /// (every Array that call can read has at least as many elements).
    pub(crate) key_floor: u32,
    /// The key is an integer in `[0, length)` of the Array read, whatever
    /// the floor (`walk_read`: the counter of a loop bounded by that
    /// Array's own fixed length).
    pub(crate) below_length: bool,
    /// The read's value is a proven Number: a plain double.
    pub(crate) number: bool,
    /// The receiver, for the check-mode report.
    pub(crate) receiver: Receiver,
    /// The key is the bounded counter of a loop that holds the receiver's
    /// length (`expr::held_len`): an integer in `[0, length)`, so the read
    /// has no bound. `Some(true)`: the length is defined, so the elements
    /// start behind the header whatever `rep.floor` is.
    pub(crate) bounded: Option<bool>,
    /// The in-bounds read (wpfacts `gen/in_bounds.rs`,
    /// `ElisionFacts::inbounds_reads`): the key is an integer in
    /// `[0, length)` of the Array read, by a length test of the program
    /// text and an Array that never shrinks; so the read has no bound.
    /// `Some(true)`: no Array the receiver may hold has a hole, so the slot
    /// is a present element. Never set for an at-rest receiver.
    pub(crate) kept: Option<bool>,
}

/// The receiver expression of a rep-keyed read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Receiver {
    Local(u32),
    /// A field read (`FieldProof::array`).
    Field,
}

impl Receiver {
    pub(crate) fn label(self) -> String {
        match self {
            Receiver::Local(id) => format!("local {id}"),
            Receiver::Field => "a field read".to_string(),
        }
    }
}

impl RepRead {
    /// The key names an own slot without a bound test: it is below the
    /// floor, or below the length of fixed storage.
    fn in_extent(self) -> bool {
        self.in_floor || self.below_length && self.rep.fixed
    }

    /// The slot is below a dense floor: present, never a hole. So is one
    /// below the length of a dense Array of fixed length: each Array's
    /// floor is its own length (`bindings::tok_arr_rep`).
    fn present(self) -> bool {
        self.rep.dense && self.in_extent()
    }

    /// The slot the read loads is a present element: below a dense floor
    /// (`present`), or a kept in-bounds read of Arrays without holes.
    pub(crate) fn element(self) -> bool {
        self.present() || self.kept == Some(true)
    }

    /// The read cannot miss: no test, no cold arm. A present at-rest slot
    /// holds an int32 Number (`expr::rest_elems`).
    pub(crate) fn total(self, numeric: bool) -> bool {
        self.rep.unmoved()
            && (self.in_extent() && self.rep.dense || self.kept == Some(true))
            && (!numeric || self.number || self.rep.at_rest)
    }
}

/// The in-bounds read fact of the read at `element_offset` of a receiver of
/// rep `rep` (`RepRead::kept`). Not with interpreted code in the program:
/// the facts do not see its writes.
fn kept(ctx: &FnCtx<'_>, rep: FactsArrRep, element_offset: u32) -> Option<bool> {
    if element_offset == 0 || rep.at_rest || ctx.wpfacts.interp_guard {
        return None;
    }
    ctx.wpfacts.inbounds_reads.get(&element_offset).copied()
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
    let (rep, receiver) = receiver_rep(ctx, object)?;
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
        key_floor: rep.floor,
        below_length: false,
        number,
        receiver,
        bounded: None,
        kept: kept(ctx, rep, element_offset),
    })
}

/// THE query for a receiver expression's `Arr` representation
/// (DESIGN-p4l1 §4.2): a local, parameter or module const
/// (`BindingFacts::rep`), or a field read (`FieldProof::array`).
fn receiver_rep(ctx: &FnCtx<'_>, object: &Expr) -> Option<(FactsArrRep, Receiver)> {
    match object {
        Expr::LocalGet(id) => {
            // A local the codegen keeps in scalar slots holds no array
            // word (R6).
            if ctx.scalar_replaced_arrays.contains_key(id)
                || ctx.array_row_aliases.contains_key(id)
                || ctx.scalar_replaced.contains_key(id)
            {
                return None;
            }
            let FactsRep::Arr(rep) = crate::codegen::facts_rep::local(ctx, *id)? else {
                return None;
            };
            Some((rep, Receiver::Local(*id)))
        }
        Expr::PropertyGet {
            object: holder,
            property,
            ..
        } => {
            if !crate::codegen::facts_rep::enabled() || ctx.wpfacts_off {
                return None;
            }
            let rep = crate::expr::wpfacts_access::field_array(ctx, holder, property)?;
            Some((rep, Receiver::Field))
        }
        _ => None,
    }
}

/// Do the facts give the receiver expression an `Arr` representation, i.e.
/// prove every value it holds an Array (a heap pointer)?
pub(crate) fn receiver_is_proven_array(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    receiver_rep(ctx, object).is_some()
}

/// The read of a field read call whose own index is in the floor
/// (`lower_call::field_read_call`), when the tier takes it: the receiver
/// expression's representation, dense, with a floor.
pub(crate) fn in_floor_rep(ctx: &FnCtx<'_>, object: &Expr) -> Option<FactsArrRep> {
    if !tier_enabled() || typed_feedback_emission_enabled() || ctx.wpfacts_off {
        return None;
    }
    let (rep, _) = receiver_rep(ctx, object)?;
    (rep.dense && rep.floor > 0).then_some(rep)
}

/// Read `arr_box[idx_i32]` for a field read call whose index is an integer
/// in `[0, key_floor)` (`in_floor_rep` gave `rep`): a present own element,
/// so the load has no bound and no hole test, and the value is of
/// `rep.elem`.
pub(crate) fn lower_in_floor(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    rep: FactsArrRep,
    key_floor: u32,
) -> String {
    let read = RepRead {
        rep,
        in_floor: true,
        key_floor,
        below_length: false,
        number: rep.elem != crate::codegen::FactsElemRep::Boxed,
        receiver: Receiver::Field,
        bounded: None,
        kept: None,
    };
    lower_rep(ctx, arr_box, idx_i32, read, false, None)
}

/// Read `arr_box[idx_i32]` of local `id` for a key in `[0, length)`
/// (`walk_read`, which gave `rep`: dense, of fixed length, Number
/// elements, not at rest): a present own element, a plain double. No test
/// and no cold arm.
pub(super) fn lower_below_length(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    rep: FactsArrRep,
    id: u32,
) -> String {
    let read = RepRead {
        rep,
        in_floor: false,
        key_floor: rep.floor,
        below_length: true,
        number: true,
        receiver: Receiver::Local(id),
        bounded: None,
        kept: None,
    };
    debug_assert!(read.total(true) && !rep.at_rest);
    lower_rep(ctx, arr_box, idx_i32, read, false, None)
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
    if crate::expr::rest_elems::tiers_enabled()
        && crate::expr::rest_elems::read_floor(ctx, e).is_some()
    {
        // An at-rest read (I32 at rest B2): its own facts decide.
        return crate::expr::rest_elems::number_read(ctx, e);
    }
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
    lower_bounded(ctx, arr_box, idx_i32, fact, numeric, None)
}

/// The read under `fact` has an unsigned bound in front of its load: the
/// key is not proven below the floor (`idx <u capacity`) or below the fixed
/// length (`idx <u len`). Such a read admits no negative key, so a caller
/// may hand it one untested (N10, `guarded_array::unsigned_bound_covers`).
pub(super) fn bounds_key(fact: Proven) -> bool {
    match fact {
        Proven::Rep(read) => !read.in_extent(),
        Proven::Fixed(fact) => !fact.key_in_bounds,
    }
}

/// [`lower`] whose key is not sign-tested when `negative_exit` is set
/// (`bounds_key` holds): the miss arm sends a negative key there first
/// (`guarded_array::emit_negative_exit`).
pub(super) fn lower_bounded(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: Proven,
    numeric: bool,
    negative_exit: Option<&str>,
) -> String {
    debug_assert!(negative_exit.is_none() || bounds_key(fact));
    match fact {
        Proven::Rep(read) => lower_rep(ctx, arr_box, idx_i32, read, numeric, negative_exit),
        Proven::Fixed(fact) => lower_fixed(ctx, arr_box, idx_i32, fact, numeric, negative_exit),
    }
}

/// The rep-keyed read at the bounded counter of a loop that holds the
/// receiver's length (`expr::held_len`): `read.bounded` is set.
pub(crate) fn lower_held(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    read: RepRead,
    numeric: bool,
) -> String {
    debug_assert!(read.bounded.is_some() && !read.rep.at_rest);
    lower_rep(ctx, arr_box, idx_i32, read, numeric, None)
}

/// The rep-keyed read of an at-rest Array (`expr::rest_elems`, B2): `read`
/// is `rep_read`'s for that receiver, `read.rep.at_rest`.
pub(crate) fn lower_rest(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    read: RepRead,
    numeric: bool,
) -> String {
    debug_assert!(read.rep.at_rest);
    lower_rep(ctx, arr_box, idx_i32, read, numeric, None)
}

/// The rep-keyed read (module docs). An at-rest receiver
/// (`FactsArrRep::at_rest`) holds I32 words: a slot that is no hole is an
/// int32 Number, decoded by `rest_elems::decode`, and the cold read is the
/// accessor's.
fn lower_rep(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    read: RepRead,
    numeric: bool,
    negative_exit: Option<&str>,
) -> String {
    let r = read.rep;
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "receiver": read.receiver.label(), "floor": r.floor, "in_floor": read.in_floor, "below_length": read.below_length, "kept": read.kept }),
    );
    crate::facts_consumed::used();
    if crate::verify_facts::enabled() {
        // `read.number`: the read's value is a proven Number, never
        // `undefined` (its own claim, or a present slot of Number elements).
        super::array_rep::witness_read(ctx, arr_box, idx_i32, read, numeric, read.number);
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
    if !r.unmoved() {
        // Growth moves the storage and forwards this header.
        let word = super::guarded_array::emit_array_guard_word(ctx.block(), &handle);
        let moved = ctx.block().and(I32, &word, FORWARDED_I32);
        let live = ctx.block().icmp_eq(I32, &moved, "0");
        step(ctx, &live, "arep.live");
    }
    let mut capacity = None;
    // An at-rest Array is dense below its floor and holds an I32 word in
    // every slot there: outside the key's proven floor, `idx <u floor` is the
    // whole test (no capacity load, no hole test), and any other key is the
    // accessor's cold read.
    let rest_bound = r.at_rest && r.dense && r.floor > 0 && !read.in_extent();
    if rest_bound {
        let inside = ctx.block().icmp_ult(I32, idx_i32, &r.floor.to_string());
        step(ctx, &inside, "arep.inside");
    }
    // In the floor `capacity >= floor > idx`: the slot exists. So does one
    // below a fixed length (`capacity >= length`), and one at a held
    // loop's bounded counter (`idx < length <= capacity`).
    let front = r.floor > 0 || read.bounded == Some(true);
    let unbounded = read.in_extent() || read.bounded.is_some() || read.kept.is_some();
    if !rest_bound && !(unbounded && front) {
        let ptr = ctx.block().inttoptr(I64, &handle);
        let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
        let cap = ctx.block().load(I32, &cap_ptr);
        if !unbounded {
            let inside = ctx.block().icmp_ult(I32, idx_i32, &cap);
            step(ctx, &inside, "arep.inside");
        }
        capacity = Some(cap);
    }
    let addr = if front {
        // A defined length: the elements start behind the header.
        let blk = ctx.block();
        let idx = blk.zext(I32, idx_i32, I64);
        let offset = blk.shl(I64, &idx, "3");
        let base = blk.add(
            I64,
            &handle,
            &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
        );
        let addr = blk.add(I64, &base, &offset);
        if r.floor == 0 {
            crate::expr::held_len::witness_front(ctx, arr_box, &handle, idx_i32, &addr);
        }
        addr
    } else {
        let cap = capacity.expect("no key is below a floor of 0");
        ctx.block()
            .array_element_addr_with_capacity(&handle, &cap, idx_i32)
    };
    let blk = ctx.block();
    let slot = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &slot);
    let value = if r.at_rest {
        if !read.present() && !rest_bound {
            let own = ctx.block().icmp_ne(I64, &raw, TAG_HOLE_I64);
            step(ctx, &own, "arep.own");
        }
        crate::expr::rest_elems::decode(ctx, &raw, crate::expr::rest_elems::Want::Value)
    } else {
        let value = blk.bitcast_i64_to_double(&raw);
        if numeric && !(read.element() && read.number) {
            let plain = crate::codegen::emit_plain_number_bits_test(blk, &raw);
            step(ctx, &plain, "arep.number");
        } else if !numeric && !read.element() {
            let own = blk.icmp_ne(I64, &raw, TAG_HOLE_I64);
            step(ctx, &own, "arep.own");
        }
        value
    };
    let Some(cold_idx) = cold_idx else {
        return value;
    };
    let merge_idx = ctx.new_block("arep.merge");
    let merge_label = ctx.block_label(merge_idx);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = cold_idx;
    super::guarded_array::emit_negative_exit(ctx, idx_i32, negative_exit);
    let cold = if r.at_rest {
        rest_cold(ctx, arr_box, idx_i32, numeric)
    } else {
        let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
        // Mode 2: the exact arguments, no absence proof (`emit_read_cold`).
        super::guarded_array::emit_read_cold(
            ctx, arr_box, "2", &idx_box, "0", numeric, numeric, None,
        )
    };
    let cold_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&value, &load_end), (&cold, &cold_end)])
}

/// The cold read of an at-rest Array (I32 at rest B2): a key at or past its
/// length names no own slot, so it is the shared cold read a double
/// Array's miss takes (it can meet no element word); a key below the length
/// is an element, read through the accessor (`rest_elems::cold_read`).
fn rest_cold(ctx: &mut FnCtx<'_>, arr_box: &str, idx_i32: &str, numeric: bool) -> String {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(arr_box);
    let handle = blk.and(I64, &bits, POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    let element = blk.icmp_ult(I32, idx_i32, &length);
    let acc = ctx.new_block("arep.rest_elem");
    let miss = ctx.new_block("arep.rest_miss");
    let done = ctx.new_block("arep.rest_done");
    let (acc_l, miss_l, done_l) = (
        ctx.block_label(acc),
        ctx.block_label(miss),
        ctx.block_label(done),
    );
    ctx.block().cond_br(&element, &acc_l, &miss_l);
    ctx.current_block = acc;
    let got = crate::expr::rest_elems::cold_read(ctx, arr_box, idx_i32, numeric);
    let acc_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = miss;
    let idx_box = ctx.block().sitofp(I32, idx_i32, DOUBLE);
    let absent = super::guarded_array::emit_read_cold(
        ctx, arr_box, "2", &idx_box, "0", numeric, numeric, None,
    );
    let miss_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = done;
    ctx.block()
        .phi(DOUBLE, &[(&got, &acc_end), (&absent, &miss_end)])
}

/// The fixed-storage read (module docs).
fn lower_fixed(
    ctx: &mut FnCtx<'_>,
    arr_box: &str,
    idx_i32: &str,
    fact: FactsElementRead,
    numeric: bool,
    negative_exit: Option<&str>,
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
    super::guarded_array::emit_negative_exit(ctx, idx_i32, negative_exit);
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
