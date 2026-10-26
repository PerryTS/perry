//! One NoCollect storage operation for ordinary fixed slots, including
//! references whose current attributes are owned by an overflow entry.
use super::{ObjectHeader, ShapeRecord, TAG_HOLE};

/// Write a fixed slot after its caller proves presence, or admits its hole
/// fill. Spilled storage still requires an existing present value.
///
/// # Safety
/// obj and r describe the same live receiver; no call here can collect.
#[inline(always)]
pub(super) unsafe fn checked_slot(
    obj: *mut ObjectHeader,
    r: *const ShapeRecord,
    slot: u32,
    bits: u64,
) -> i32 {
    if slot >= (*r).live_inline_slot_count {
        return i32::from(crate::object::spill::spill_overwrite_present(
            obj as usize,
            slot as usize,
            bits,
        ));
    }
    let address = (obj as *mut u8)
        .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
        .cast::<u64>();
    let rep = crate::object::field_rep::slot_rep((*r).rep, slot);
    if bits & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000
        && rep != crate::object::field_rep::REP_SPECIAL
    {
        crate::object::builtin_proto::note_value_store(obj, slot as usize, false, "probe store");
        // GC_STORE_AUDIT(POINTER_FREE): a canonical finite Number.
        address.write(bits);
        return 1;
    }
    if rep == crate::object::field_rep::REP_ANY {
        crate::object::builtin_proto::note_value_store(obj, slot as usize, false, "probe store");
        crate::gc::runtime_store_jsvalue_slot_layout_deferred(
            obj as usize,
            address as usize,
            slot as usize,
            bits,
        );
        return 1;
    }
    0
}

/// The overflow owns membership, attributes and the physical value location.
/// A fixed reference shares the same slot write as a static record.
///
/// # Safety
/// t is obj's current overflow; obj/r remain current through this GC leaf.
#[inline]
pub(super) unsafe fn overflow(
    obj: *mut ObjectHeader,
    r: *const ShapeRecord,
    t: *mut crate::object::overflow::Table,
    key_bits: u64,
    hash: u64,
    bits: u64,
) -> i32 {
    let Some(i) = crate::object::overflow::find_hashed(t, key_bits, hash) else {
        return 0;
    };
    if let Some(slot) = crate::object::overflow::fixed_slot_of(t, i) {
        let attrs = crate::object::overflow::entry_attrs(t.cast(), i);
        if !crate::object::key_attrs::entry_is_plain_writable_data(attrs) {
            return 0;
        }
        if slot < (*r).live_inline_slot_count {
            let address = (obj as *const u8)
                .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
                .cast::<u64>();
            if *address == TAG_HOLE {
                return 0;
            }
        }
        return checked_slot(obj, r, slot, bits);
    }
    // Keep the existing table-owned plain-value probe and barrier.
    if !crate::object::overflow::plain_writable_value(t, i) {
        return 0;
    }
    let slot = crate::object::overflow::value_at(t, i);
    // GC_STORE_AUDIT(BARRIERED): the table owns this value word.
    *slot = bits;
    crate::gc::runtime_write_barrier_slot(t as usize, slot as usize, bits);
    1
}
