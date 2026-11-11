//! Check mode for wpfacts' `Rep::Arr` (DESIGN-p4l1 §2.1-2.6 and §5.1,
//! PR-B).
//!
//! A local the facts give `FactsRep::Arr(r)` holds only plain program
//! Arrays: for its whole life each is a `GC_TYPE_ARRAY` (never a lazy JSON
//! header) with `length >= r.floor` and `capacity >= r.floor`; with
//! `r.fixed` its storage never grows, so its header is never forwarded;
//! with `r.dense` no index below `r.floor` is a hole; and every element is
//! of `r.elem`. PR-B has no consumer: the slot keeps its Boxed word and
//! every read its tier. Under `PERRY_VERIFY_FACTS=1` an element read `x[k]`
//! of such a local first tests the claim on the receiver, and on the slot
//! the key names when the key is an integer below the length and the
//! capacity. A miss reports the false proof and exits 86
//! (`Fact::ArrayRep`).
//!
//! The receiver and the key are evaluated again for the test, so only a
//! local receiver and a local or literal key are tested. A local the
//! codegen keeps in scalar slots (a scalar-replaced literal, an array row
//! alias) holds no array word and is not tested. A forwarded header of a
//! non-`fixed` array is legal (growth forwards it) and its stale fields are
//! not tested.
//!
//! PR-C2 (`proven_array`, the rep-keyed tier) consumes the claim. Its reads
//! are tested again where they load (`witness_read`), on the values they
//! load with: the claim above, that the elements start at the front of the
//! allocation when the tier addresses them there (`floor > 0`), that no
//! element has a descriptor, that an in-floor key is below the floor, and
//! that a slot past the length is a hole (the bound is the capacity).

use anyhow::Result;
use perry_hir::Expr;

use super::guarded_array::{
    emit_array_guard_word, HEAP_POINTER_BAND_BASE_I64, HEAP_POINTER_BAND_SPAN_I64,
};
use super::FnCtx;
use crate::codegen::{FactsArrRep, FactsElemRep, FactsRep};
use crate::nanbox::{POINTER_MASK_I64, TAG_HOLE_I64};
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `perry_abi::GC_TYPE_ARRAY` in the header word's low byte.
const TYPE_MASK_I32: &str = "255";
/// `perry_abi::GC_FLAG_FORWARDED` in the header word's second byte.
const FORWARDED_I32: &str = "32768";

/// Emit the witness for `expr` (an `IndexGet`) when check mode is on and the
/// facts give its local receiver `Arr`.
pub(crate) fn witness(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<()> {
    if !crate::verify_facts::enabled() || ctx.wpfacts_off {
        return Ok(());
    }
    let Expr::IndexGet { object, index, .. } = expr else {
        return Ok(());
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return Ok(());
    };
    if ctx.scalar_replaced_arrays.contains_key(id)
        || ctx.array_row_aliases.contains_key(id)
        || ctx.scalar_replaced.contains_key(id)
    {
        return Ok(());
    }
    let Some(FactsRep::Arr(r)) = crate::codegen::facts_rep::local(ctx, *id) else {
        return Ok(());
    };
    let keyed = matches!(
        index.as_ref(),
        Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_)
    );
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "local": id, "floor": r.floor }),
    );
    let recv = super::lower_expr(ctx, object)?;
    let key = if keyed {
        Some(super::lower_expr(ctx, index)?)
    } else {
        None
    };
    emit(ctx, &recv, key.as_deref(), r, &format!("local {id}"));
    Ok(())
}

/// Check mode at a field read the facts give `Arr(r)` (DESIGN-p4l1 §5.1,
/// PR-C3): the claim on the word `value` the read loaded. `what` names the
/// field for the report.
pub(crate) fn witness_value(ctx: &mut FnCtx<'_>, value: &str, r: FactsArrRep, what: &str) {
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "field": what, "floor": r.floor }),
    );
    emit(ctx, value, None, r, what);
}

/// The tests of `witness` on the lowered receiver `recv` and key `key`.
fn emit(ctx: &mut FnCtx<'_>, recv: &str, key: Option<&str>, r: FactsArrRep, what: &str) {
    let miss = ctx.new_block("arr_rep.verify_miss");
    let next = ctx.new_block("arr_rep.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    // A pointer into the heap band.
    let bits = ctx.block().bitcast_double_to_i64(recv);
    let offset = ctx.block().sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = ctx
        .block()
        .icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    branch(ctx, &heap, "arr_rep.header", &miss_l);
    // Plain: an ordinary Array header, not a lazy one.
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let ty = ctx.block().and(I32, &word, TYPE_MASK_I32);
    let array = crate::target_layout::emit_gc_type_is_array(ctx.block(), I32, &ty);
    branch(ctx, &array, "arr_rep.forwarding", &miss_l);
    // Fixed: never forwarded. Otherwise a forwarded header ends the test.
    let fwd_bit = ctx.block().and(I32, &word, FORWARDED_I32);
    let live = ctx.block().icmp_eq(I32, &fwd_bit, "0");
    if r.fixed {
        branch(ctx, &live, "arr_rep.extent", &miss_l);
    } else {
        branch(ctx, &live, "arr_rep.extent", &next_l);
    }
    // The floor: length and capacity.
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let capacity = ctx.block().load(I32, &cap_ptr);
    let floor = r.floor.to_string();
    let long = ctx.block().icmp_uge(I32, &length, &floor);
    let backed = ctx.block().icmp_uge(I32, &capacity, &floor);
    let extent = ctx.block().and(I1, &long, &backed);
    let Some(key) = key else {
        ctx.block().cond_br(&extent, &next_l, &miss_l);
        finish(ctx, miss, next, recv, r, what);
        return;
    };
    branch(ctx, &extent, "arr_rep.key", &miss_l);
    // The key names a slot when it is an integer below length and capacity.
    let fits = ctx.block().icmp_ult(I32, &length, &capacity);
    let bound = ctx.block().select(I1, &fits, I32, &length, &capacity);
    let bound_f = ctx.block().uitofp(I32, &bound, DOUBLE);
    let low = ctx.block().fcmp("oge", key, "0.0");
    let high = ctx.block().fcmp("olt", key, &bound_f);
    let inside = ctx.block().and(I1, &low, &high);
    branch(ctx, &inside, "arr_rep.integral", &next_l);
    let idx = ctx.block().fptosi(DOUBLE, key, I32);
    let back = ctx.block().sitofp(I32, &idx, DOUBLE);
    let exact = ctx.block().fcmp("oeq", &back, key);
    branch(ctx, &exact, "arr_rep.slot", &next_l);
    let addr = ctx
        .block()
        .array_element_addr_with_capacity(&handle, &capacity, &idx);
    let slot = ctx.block().inttoptr(I64, &addr);
    let raw = ctx.block().load(I64, &slot);
    let hole = ctx.block().icmp_eq(I64, &raw, TAG_HOLE_I64);
    if r.dense {
        // Dense: an index below the floor is no hole.
        let below = ctx.block().icmp_ult(I32, &idx, &floor);
        let bad = ctx.block().and(I1, &below, &hole);
        let good = ctx.block().xor(I1, &bad, "true");
        branch(ctx, &good, "arr_rep.elem", &miss_l);
    }
    // A hole holds no element; any other slot is of `r.elem`.
    let present = ctx.block().xor(I1, &hole, "true");
    branch(ctx, &present, "arr_rep.elem_kind", &next_l);
    elem_test(ctx, &raw, r, r.elem, &next_l, &miss_l);
    finish(ctx, miss, next, recv, r, what);
}

/// Check mode at the rep-keyed tier: what the read `recv[idx]` (`idx` an
/// i32) leans on (module docs).
pub(super) fn witness_read(
    ctx: &mut FnCtx<'_>,
    recv: &str,
    idx: &str,
    read: super::proven_array::RepRead,
    numeric: bool,
) {
    let r = read.rep;
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ArrayRep,
        || serde_json::json!({ "receiver": read.receiver.label(), "floor": r.floor, "read": true }),
    );
    let miss = ctx.new_block("arep.verify_miss");
    let next = ctx.new_block("arep.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    let bits = ctx.block().bitcast_double_to_i64(recv);
    let offset = ctx.block().sub(I64, &bits, HEAP_POINTER_BAND_BASE_I64);
    let heap = ctx
        .block()
        .icmp_ult(I64, &offset, HEAP_POINTER_BAND_SPAN_I64);
    branch(ctx, &heap, "arep.verify_header", &miss_l);
    let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let word = emit_array_guard_word(ctx.block(), &handle);
    let ty = ctx.block().and(I32, &word, TYPE_MASK_I32);
    let array = crate::target_layout::emit_gc_type_is_array(ctx.block(), I32, &ty);
    branch(ctx, &array, "arep.verify_forwarding", &miss_l);
    let fwd_bit = ctx.block().and(I32, &word, FORWARDED_I32);
    let live = ctx.block().icmp_eq(I32, &fwd_bit, "0");
    // The read tests forwarding itself unless the storage is fixed.
    let moved_l = if r.fixed { &miss_l } else { &next_l };
    branch(ctx, &live, "arep.verify_descriptors", moved_l);
    let described =
        crate::expr::array_proto_guard::emit_array_element_descriptors(ctx.block(), &handle);
    let plain = ctx.block().xor(I1, &described, "true");
    branch(ctx, &plain, "arep.verify_extent", &miss_l);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let capacity = ctx.block().load(I32, &cap_ptr);
    if r.floor > 0 {
        // Invariant O: the read addresses slot 0 right behind the header.
        let first = ctx
            .block()
            .array_element_addr_with_capacity(&handle, &capacity, "0");
        let header = crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string();
        let behind = ctx.block().add(I64, &handle, &header);
        let front = ctx.block().icmp_eq(I64, &first, &behind);
        branch(ctx, &front, "arep.verify_floor", &miss_l);
    }
    let floor = r.floor.to_string();
    let long = ctx.block().icmp_uge(I32, &length, &floor);
    let backed = ctx.block().icmp_uge(I32, &capacity, &floor);
    let extent = ctx.block().and(I1, &long, &backed);
    branch(ctx, &extent, "arep.verify_key", &miss_l);
    if read.in_floor {
        // The floor the key was proven below (a field read call's own).
        let below = ctx.block().icmp_ult(I32, idx, &read.key_floor.to_string());
        branch(ctx, &below, "arep.verify_stored", &miss_l);
    }
    // The slot, when the store holds it.
    let stored = ctx.block().icmp_ult(I32, idx, &capacity);
    branch(ctx, &stored, "arep.verify_slot", &next_l);
    let addr = ctx
        .block()
        .array_element_addr_with_capacity(&handle, &capacity, idx);
    let slot = ctx.block().inttoptr(I64, &addr);
    let raw = ctx.block().load(I64, &slot);
    let hole = ctx.block().icmp_eq(I64, &raw, TAG_HOLE_I64);
    // A slot at or past the length is a hole; with `dense`, none below the
    // floor is.
    let past = ctx.block().icmp_uge(I32, idx, &length);
    let below = ctx.block().icmp_ult(I32, idx, &floor);
    let must_hole = past;
    let no_hole = if r.dense { below } else { "false".to_string() };
    let present = ctx.block().xor(I1, &hole, "true");
    let stale = ctx.block().and(I1, &must_hole, &present);
    let gap = ctx.block().and(I1, &no_hole, &hole);
    let bad = ctx.block().or(I1, &stale, &gap);
    let good = ctx.block().xor(I1, &bad, "true");
    branch(ctx, &good, "arep.verify_present", &miss_l);
    branch(ctx, &present, "arep.verify_elem", &next_l);
    // A number-context read of a proven Number takes the slot as a double.
    let elem = if numeric && read.number && r.elem == FactsElemRep::Boxed {
        FactsElemRep::F64
    } else {
        r.elem
    };
    elem_test(ctx, &raw, r, elem, &next_l, &miss_l);
    finish(ctx, miss, next, recv, r, &read.receiver.label());
}

/// The element kind test of a present slot `raw`: an I32 word for an
/// at-rest Array (`r.at_rest`, `expr::rest_elems`), else none for Boxed, a
/// plain Number for F64, an int32 Number (never `-0`) for I32.
fn elem_test(
    ctx: &mut FnCtx<'_>,
    raw: &str,
    r: FactsArrRep,
    elem: FactsElemRep,
    next_l: &str,
    miss_l: &str,
) {
    if r.at_rest {
        crate::expr::rest_elems::witness_raw(ctx, raw, next_l, miss_l);
        return;
    }
    match elem {
        FactsElemRep::Boxed => ctx.block().br(next_l),
        FactsElemRep::F64 => {
            let plain = crate::codegen::emit_plain_number_bits_test(ctx.block(), raw);
            ctx.block().cond_br(&plain, next_l, miss_l);
        }
        FactsElemRep::I32 => {
            let plain = crate::codegen::emit_plain_number_bits_test(ctx.block(), raw);
            branch(ctx, &plain, "arr_rep.int32", miss_l);
            let x = ctx.block().bitcast_i64_to_double(raw);
            let ge = ctx.block().fcmp("oge", &x, "-2147483648.0");
            let le = ctx.block().fcmp("ole", &x, "2147483647.0");
            let in_range = ctx.block().and(I1, &ge, &le);
            branch(ctx, &in_range, "arr_rep.int32_exact", miss_l);
            let int = ctx.block().fptosi(DOUBLE, &x, I32);
            let back = ctx.block().sitofp(I32, &int, DOUBLE);
            let exact = ctx.block().fcmp("oeq", &back, &x);
            let not_negative_zero = ctx.block().icmp_ne(I64, raw, "-9223372036854775808");
            let mut ok = ctx.block().and(I1, &exact, &not_negative_zero);
            // −0 tolerance (wpfacts PR D): a tolerated element may be `-0`.
            if ctx.wpfacts.zero_sign_tolerated {
                ok = ctx.block().or(I1, &ok, &exact);
            }
            ctx.block().cond_br(&ok, next_l, miss_l);
        }
    }
}

/// `cond` goes on to a new block `name`, else to `other`; the new block is
/// current.
fn branch(ctx: &mut FnCtx<'_>, cond: &str, name: &str, other: &str) {
    let on = ctx.new_block(name);
    let on_l = ctx.block_label(on);
    ctx.block().cond_br(cond, &on_l, other);
    ctx.current_block = on;
}

/// The miss block's report, then `next` is current.
fn finish(ctx: &mut FnCtx<'_>, miss: usize, next: usize, recv: &str, r: FactsArrRep, what: &str) {
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!(
            "array rep of {what}: elem {:?}, floor {}, dense {}, fixed {}",
            r.elem, r.floor, r.dense, r.fixed
        ),
        crate::verify_facts::Fact::ArrayRep,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
