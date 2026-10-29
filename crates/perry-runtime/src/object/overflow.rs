//! Object-owned insertion order and dynamic properties (§3.4). Entries retain
//! fixed slot references; the scalar index never contains addresses of moving
//! cells.
//!
//! Storage is columnar: after the header come the values, the attributes,
//! the hash index, and LAST the keys. The header's `length`/`capacity` are an
//! `ArrayHeader` prefix and the keys column ends the allocation, so the keys
//! read as a dense key list (tombstones are `TAG_HOLE`, as in any key list)
//! through the ordinary array element address. Entry `i` is the key, value
//! and attributes at index `i` of the three columns.
use super::{ObjectHeader, ObjectKeys};
use crate::{array::ArrayHeader, value::TAG_HOLE};

#[repr(C)]
pub(crate) struct Table {
    // An `ArrayHeader` prefix over the keys column (see the module docs).
    pub length: u32,
    pub capacity: u32,
    pub fixed_prefix: u32,
    pub deleted: u32,
    pub index_capacity: u32,
    /// The OR of every entry's attribute summary (`key_attrs::entry_summary`)
    /// ever recorded. Kept on each attribute write so `object_summary` is one
    /// load; it may stay set after the entry that set it changes (a wasted
    /// slow-path check, never a wrong answer).
    pub summary: u32,
}

const SLOT_REF: u64 = 1 << 8;

/// Keys a receiver may add one at a time, its list unique to it, before its
/// overflow takes over (`canonical_keys::take_unique_run`).
pub(crate) const UNIQUE_RUN_TO_OVERFLOW: u32 = 1024;
const EMPTY: u32 = u32::MAX;
pub(crate) const SHADOW_BIT: u32 = super::layout_desc::LAYOUT_SHADOW_BIT;

/// The shadow bit (`layout_desc::LAYOUT_SHADOW_BIT`, which documents its one
/// meaning for steps 3 and 4). Clear proves the object has no overflow; set
/// does NOT prove one (a step-3 dynamic write sets it too): use
/// [`has_overflow`] / [`table`] for that.
#[inline(always)]
pub(crate) unsafe fn shadowed(obj: *const ObjectHeader) -> bool {
    (*obj).parent_class_id & SHADOW_BIT != 0
}

#[inline]
pub(crate) unsafe fn is_table(keys: *const ArrayHeader) -> bool {
    (keys as usize) >= perry_abi::RECEIVER_HANDLE_FLOOR
        && (keys as u64) >> 48 == 0
        && (*crate::gc::header_from_trusted_user_ptr(keys.cast())).obj_type
            == crate::gc::GC_TYPE_OVERFLOW
}

/// The object's overflow, or null. `install` sets the shadow bit in the
/// layout word when it installs an overflow and every restamp keeps the bit
/// (`layout_desc::carry_shadow`), so a word without it means no overflow: one
/// load of the word, before any header classification. A word with it still
/// needs the `meta.overflow` load.
#[inline]
pub(crate) unsafe fn table(obj: *const ObjectHeader) -> *mut Table {
    if obj.is_null()
        || !crate::value::addr_class::is_plausible_heap_addr(obj as usize)
        || !shadowed(obj)
        || !super::object_is_shaped(obj)
        || (*obj).meta.is_null()
    {
        return std::ptr::null_mut();
    }
    (*(*obj).meta).overflow as usize as *mut Table
}

#[inline]
pub(crate) unsafe fn has_overflow(obj: *const ObjectHeader) -> bool {
    !table(obj).is_null()
}

#[inline]
pub(crate) unsafe fn keys_array(obj: *const ObjectHeader) -> *mut ArrayHeader {
    table(obj).cast()
}

#[inline(always)]
unsafe fn values(t: *const Table) -> *mut u64 {
    t.add(1).cast_mut().cast()
}

/// Entry `i`'s value (or, with `SLOT_REF`, its fixed slot number).
#[inline(always)]
pub(crate) unsafe fn value_at(t: *const Table, i: u32) -> *mut u64 {
    values(t).add(i as usize)
}

/// Entry `i`'s attributes word (low byte: key attributes; `SLOT_REF`).
#[inline(always)]
pub(crate) unsafe fn attrs_at(t: *const Table, i: u32) -> *mut u64 {
    values(t).add((*t).capacity as usize + i as usize)
}

#[inline(always)]
unsafe fn index(t: *const Table) -> *mut u32 {
    values(t).add(2 * (*t).capacity as usize).cast()
}

/// The keys column: a dense key list of `length` entries.
#[inline(always)]
pub(crate) unsafe fn keys_ptr(t: *const Table) -> *mut u64 {
    values(t).add(2 * (*t).capacity as usize + (*t).index_capacity as usize / 2)
}

/// Entry `i`'s key (`TAG_HOLE` for a tombstone).
#[inline(always)]
pub(crate) unsafe fn key_at(t: *const Table, i: u32) -> *mut u64 {
    keys_ptr(t).add(i as usize)
}

/// Tombstone entry `i`. Its index bucket keeps the probe chain; the next
/// growth compacts dynamic entries (the fixed prefix stays in place).
pub(crate) unsafe fn tombstone(t: *mut Table, i: u32) {
    *key_at(t, i) = TAG_HOLE;
    *value_at(t, i) = crate::value::TAG_UNDEFINED;
    *attrs_at(t, i) = 0;
    (*t).deleted += 1;
}

unsafe fn allocate(capacity: u32) -> *mut Table {
    let index_capacity = if capacity < 8 {
        0
    } else {
        (capacity * 2).next_power_of_two()
    };
    // Keys last: `array_elements_ptr` of the table is the keys column.
    let size =
        std::mem::size_of::<Table>() + 3 * capacity as usize * 8 + index_capacity as usize * 4;
    let t = crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OVERFLOW) as *mut Table;
    // GC_STORE_AUDIT(POINTER_FREE): Table header has six u32 fields (length,capacity,fixed_prefix,deleted,index_capacity,summary); initialization writes only scalar counts and zeros.
    t.write(Table {
        length: 0,
        capacity,
        fixed_prefix: 0,
        deleted: 0,
        index_capacity,
        summary: 0,
    });
    for i in 0..index_capacity as usize {
        // GC_STORE_AUDIT(POINTER_FREE): index(t) returns *mut u32 scalar slot-index column; writes EMPTY=u32::MAX, not the key/value columns.
        index(t).add(i).write(EMPTY);
    }
    t
}

/// A key atom is canonicalized before entry lookup, including long keys.
#[inline]
unsafe fn key_hash(key: u64) -> u64 {
    if crate::symbol::js_is_symbol(f64::from_bits(key)) != 0 {
        return (*((key & crate::value::POINTER_MASK) as *const crate::symbol::SymbolHeader)).id;
    }
    crate::string::property_atom_hash(
        (key & crate::value::POINTER_MASK) as *const crate::StringHeader,
    )
}

unsafe fn place(t: *mut Table, slot: u32) {
    if (*t).index_capacity == 0 {
        return;
    }
    let key = *key_at(t, slot);
    if key == TAG_HOLE || super::key_attrs::entry_is_private(*attrs_at(t, slot) as u8) {
        return;
    }
    let mask = (*t).index_capacity as usize - 1;
    let mut p = key_hash(key) as usize & mask;
    while *index(t).add(p) != EMPTY {
        let previous = *index(t).add(p);
        if *key_at(t, previous) == key {
            // GC_STORE_AUDIT(POINTER_FREE): place writes u32 slot ordinal into *mut u32 index column after hashing the key; never copies key bits or heap addresses.
            index(t).add(p).write(slot);
            return;
        }
        p = (p + 1) & mask;
    }
    // GC_STORE_AUDIT(POINTER_FREE): place writes u32 slot ordinal into *mut u32 index column after hashing the key; never copies key bits or heap addresses.
    index(t).add(p).write(slot);
}

/// [`find`] with the key's hash supplied by the caller (the probe's
/// compile-time atom hash, `layout_probe.rs`), so no key header is read.
#[inline]
pub(crate) unsafe fn find_hashed(t: *const Table, key: u64, hash: u64) -> Option<u32> {
    if (*t).length < 8 {
        let keys = keys_ptr(t);
        let mut i = (*t).length;
        while i > 0 {
            i -= 1;
            if *keys.add(i as usize) == key {
                return (!super::key_attrs::entry_is_private(*attrs_at(t, i) as u8)).then_some(i);
            }
        }
        return None;
    }
    let mask = (*t).index_capacity as usize - 1;
    let mut p = hash as usize & mask;
    loop {
        let i = *index(t).add(p);
        if i == EMPTY {
            return None;
        }
        if *key_at(t, i) == key {
            return Some(i);
        }
        p = (p + 1) & mask;
    }
}

pub(crate) unsafe fn find(t: *const Table, key: u64) -> Option<u32> {
    if (*t).length < 8 {
        return (0..(*t).length).rev().find(|&i| {
            *key_at(t, i) == key && !super::key_attrs::entry_is_private(*attrs_at(t, i) as u8)
        });
    }
    let mask = (*t).index_capacity as usize - 1;
    let mut p = key_hash(key) as usize & mask;
    loop {
        let i = *index(t).add(p);
        if i == EMPTY {
            return None;
        }
        if *key_at(t, i) == key {
            return Some(i);
        }
        p = (p + 1) & mask;
    }
}

unsafe fn install(obj: *mut ObjectHeader, t: *mut Table) {
    let meta = (*obj).meta;
    (*meta).overflow = t as usize as u64;
    // The slot owner is the live metadata record, whose generation can
    // differ from the receiver's after a collection. Its header decides
    // remembering; an active incremental mark still takes the full barrier.
    crate::gc::runtime_write_barrier_resolved_slot(
        meta as usize,
        &(*meta).overflow as *const _ as usize,
        t as usize as u64,
    );
    (*obj).parent_class_id |= SHADOW_BIT;
    super::proto_validity::note_object_own_keys_changed(obj as usize);
}

/// Establish order authority once, copying keys and recording slot references.
/// No fixed slot or immutable key list is changed.
pub(crate) unsafe fn ensure(obj: *mut ObjectHeader) -> *mut Table {
    ensure_adding(obj, None)
}

/// [`ensure`] inside the publication of the key at `adding`: that key's value
/// is stored only after the publication returns, so its slot may still hold
/// the hole. It is a live key, not a tombstone.
pub(crate) unsafe fn ensure_adding(obj: *mut ObjectHeader, adding: Option<u32>) -> *mut Table {
    let existing = table(obj);
    if !existing.is_null() {
        return existing;
    }
    super::layout_desc::diag_note_install(obj);
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    super::object_meta_ensure(obj);
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    let shape = super::shapes::object_shape_descriptor(obj).unwrap();
    let prefix = shape
        .logical_key_count
        .max(shape.live_inline_slot_count)
        .max(super::INLINE_SLOT_FLOOR as u32);
    let t = allocate((prefix + 4).next_power_of_two().max(4));
    let rooted = scope.root_raw_mut_ptr(t);
    let count = shape.logical_key_count;
    for i in 0..count {
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let shape = super::shapes::object_shape_descriptor(obj).unwrap();
        let keys = ObjectKeys::new(shape.keys as usize as *mut ArrayHeader, count);
        let fixed = keys.storage_slot(i);
        // A key whose slot holds the hole (an optional key not written, a
        // tombstone) is a tombstoned entry: no atom, no barrier, no index.
        if adding != Some(i) && slot_holds_hole(obj, fixed, shape.live_inline_slot_count) {
            let attrs = super::key_attrs::keys_entry(shape.keys as usize as *const ArrayHeader, i);
            let t = rooted.get_raw_mut_ptr::<Table>();
            *key_at(t, i) = TAG_HOLE;
            *value_at(t, i) = crate::value::INT32_TAG | fixed as u64;
            *attrs_at(t, i) = attrs as u64 | SLOT_REF;
            (*t).summary |= u32::from(super::key_attrs::entry_summary(attrs));
            (*t).length = i + 1;
            continue;
        }
        let key = keys.get(i).bits();
        let atom = if crate::JSValue::from_bits(key).is_any_string() {
            crate::string::property_atom_from_value(key)
        } else if crate::symbol::js_is_symbol(f64::from_bits(key)) != 0 {
            key
        } else {
            TAG_HOLE
        };
        let t = rooted.get_raw_mut_ptr::<Table>();
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let attrs = super::key_attrs::keys_entry(
            super::shapes::object_shape_descriptor(obj).unwrap().keys as usize
                as *const ArrayHeader,
            i,
        );
        *key_at(t, i) = atom;
        *value_at(t, i) = crate::value::INT32_TAG | fixed as u64;
        *attrs_at(t, i) = attrs as u64 | SLOT_REF;
        (*t).summary |= u32::from(super::key_attrs::entry_summary(attrs));
        (*t).length = i + 1;
        crate::gc::runtime_write_barrier_slot(t as usize, key_at(t, i) as usize, atom);
    }
    // Slots past the layout's keys hold no key: keyless references to their
    // own slot numbers. The hole is no heap reference, so no barrier and
    // no index entry.
    let t = rooted.get_raw_mut_ptr::<Table>();
    for i in count..prefix {
        *key_at(t, i) = TAG_HOLE;
        *value_at(t, i) = crate::value::INT32_TAG | i as u64;
        *attrs_at(t, i) = SLOT_REF;
        (*t).length = i + 1;
    }
    let t = rooted.get_raw_mut_ptr::<Table>();
    (*t).fixed_prefix = prefix;
    for i in 0..prefix {
        place(t, i);
    }
    install(owner.get_raw_mut_ptr::<ObjectHeader>(), t);
    t
}

/// Does storage slot `fixed` of `obj` (live inline slots `live`) hold the
/// hole?
#[inline]
unsafe fn slot_holds_hole(obj: *const ObjectHeader, fixed: u32, live: u32) -> bool {
    if fixed < live {
        super::field_get_set::object_field_at_with_live(obj, fixed, live).bits() == TAG_HOLE
    } else {
        super::spill::spill_get_present(obj as usize, fixed as usize).unwrap_or(TAG_HOLE)
            == TAG_HOLE
    }
}

unsafe fn reserve(obj: *mut ObjectHeader) -> *mut Table {
    let t = ensure(obj);
    if (*t).deleted > 8 && (*t).deleted * 3 > (*t).length {
        let mut out = (*t).fixed_prefix;
        for i in (*t).fixed_prefix..(*t).length {
            if *key_at(t, i) != TAG_HOLE {
                *key_at(t, out) = *key_at(t, i);
                *value_at(t, out) = *value_at(t, i);
                *attrs_at(t, out) = *attrs_at(t, i);
                out += 1;
            }
        }
        (*t).length = out;
        (*t).deleted = 0;
        for i in 0..(*t).index_capacity as usize {
            // GC_STORE_AUDIT(POINTER_FREE): Compaction clears scalar u32 hash-index column to EMPTY; the separate managed key/value columns receive their own visit_slots barrier.
            index(t).add(i).write(EMPTY);
        }
        for i in 0..out {
            place(t, i);
        }
        visit_slots(t, |slot| {
            crate::gc::runtime_write_barrier_slot(t as usize, slot as usize, *slot)
        });
    }
    if (*t).length < (*t).capacity {
        return t;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let old = scope.root_raw_mut_ptr(t);
    let next = allocate((*t).capacity * 2);
    let t = old.get_raw_mut_ptr::<Table>();
    let n = (*t).length as usize;
    std::ptr::copy_nonoverlapping(key_at(t, 0), key_at(next, 0), n);
    std::ptr::copy_nonoverlapping(value_at(t, 0), value_at(next, 0), n);
    std::ptr::copy_nonoverlapping(attrs_at(t, 0), attrs_at(next, 0), n);
    (*next).length = (*t).length;
    (*next).fixed_prefix = (*t).fixed_prefix;
    (*next).deleted = (*t).deleted;
    (*next).summary = (*t).summary;
    for i in 0..(*next).length {
        place(next, i);
    }
    visit_slots(next, |slot| {
        crate::gc::runtime_write_barrier_slot(next as usize, slot as usize, *slot)
    });
    install(owner.get_raw_mut_ptr::<ObjectHeader>(), next);
    next
}

pub(crate) unsafe fn insert(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: u64,
    attrs: u8,
) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_f64(f64::from_bits(value));
    let atom = crate::string::property_atom(key);
    insert_key_bits(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        crate::JSValue::string_ptr(atom as *mut _).bits(),
        val.get_nanbox_f64().to_bits(),
        attrs,
    )
}

pub(crate) unsafe fn insert_key_bits(
    obj: *mut ObjectHeader,
    key: u64,
    value: u64,
    attrs: u8,
) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_f64(f64::from_bits(value));
    let key = scope.root_nanbox_u64(key);
    let t = reserve(obj);
    let key = key.get_nanbox_u64();
    let hit = if super::key_attrs::entry_is_private(attrs) {
        (0..(*t).length).find(|&i| {
            *key_at(t, i) == key && super::key_attrs::entry_is_private(*attrs_at(t, i) as u8)
        })
    } else {
        find(t, key)
    };
    let slot = hit.unwrap_or((*t).length);
    if slot == (*t).length {
        *key_at(t, slot) = key;
        *value_at(t, slot) = val.get_nanbox_f64().to_bits();
        *attrs_at(t, slot) = attrs as u64;
        (*t).summary |= u32::from(super::key_attrs::entry_summary(attrs));
        (*t).length += 1;
        place(t, slot);
        super::proto_validity::note_object_own_keys_changed(
            owner.get_raw_mut_ptr::<ObjectHeader>() as usize,
        );
    } else if *attrs_at(t, slot) & SLOT_REF != 0 {
        store_index(
            owner.get_raw_mut_ptr::<ObjectHeader>(),
            slot,
            val.get_nanbox_f64().to_bits(),
        );
        set_entry_attrs(owner.get_raw_mut_ptr::<ObjectHeader>(), t, slot, attrs);
    } else {
        *value_at(t, slot) = val.get_nanbox_f64().to_bits();
        *attrs_at(t, slot) = attrs as u64;
        (*t).summary |= u32::from(super::key_attrs::entry_summary(attrs));
    }
    crate::gc::runtime_write_barrier_resolved_slot(t as usize, key_at(t, slot) as usize, key);
    if *attrs_at(t, slot) & SLOT_REF == 0 {
        crate::gc::runtime_write_barrier_resolved_slot(
            t as usize,
            value_at(t, slot) as usize,
            *value_at(t, slot),
        );
    }
    slot
}

pub(crate) unsafe fn get(obj: *const ObjectHeader, slot: u32) -> Option<u64> {
    let t = table(obj);
    if t.is_null() {
        return None;
    }
    get_from(t, obj, slot)
}

/// The fixed storage slot entry `slot` of `t` refers to: a layout key's
/// entry names the slot its value lives in. `None` for a tombstone and for
/// an entry whose value the table holds.
///
/// # Safety
/// `t` is a live table.
pub(crate) unsafe fn fixed_slot_of(t: *const Table, slot: u32) -> Option<u32> {
    (slot < (*t).length && *key_at(t, slot) != TAG_HOLE && *attrs_at(t, slot) & SLOT_REF != 0)
        .then(|| *value_at(t, slot) as u32)
}

pub(crate) unsafe fn get_from(t: *const Table, obj: *const ObjectHeader, slot: u32) -> Option<u64> {
    if slot >= (*t).length {
        return None;
    }
    if *key_at(t, slot) == TAG_HOLE {
        return None;
    }
    read_found_with_live(t, obj, slot, || super::object_live_slot_count(obj))
}

/// Read an entry just found in the actual table, using the receiver's current
/// record rather than resolving that same published inline bound again.
///
/// # Safety
/// `slot` is a successful lookup in this live, unmodified `t`; `record` is the
/// current present record of `obj`. No callback or collection separates those
/// proofs from this NoCollect read. The caller retains its property-kind checks.
#[inline(always)]
pub(crate) unsafe fn get_found_from_record(
    t: *const Table,
    obj: *const ObjectHeader,
    slot: u32,
    record: super::shapes::ShapeRecordRef,
) -> Option<u64> {
    debug_assert!(slot < (*t).length && *key_at(t, slot) != TAG_HOLE);
    read_found_with_live(t, obj, slot, || record.live_inline_slot_count())
}

/// The single entry-value operation. General callers validate the entry above
/// and resolve a bound only for a fixed reference; a proved lookup supplies its
/// current record's same bound. No state is learned by either caller.
#[inline(always)]
unsafe fn read_found_with_live(
    t: *const Table,
    obj: *const ObjectHeader,
    slot: u32,
    inline_bound: impl FnOnce() -> u32,
) -> Option<u64> {
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        let fixed = *value_at(t, slot) as u32;
        let live = inline_bound();
        if fixed < live {
            return Some(super::field_get_set::object_field_at_with_live(obj, fixed, live).bits());
        }
        return super::spill::spill_get_present(obj as usize, fixed as usize);
    }
    Some(*value_at(t, slot))
}

pub(crate) unsafe fn entry_attrs(keys: *const ArrayHeader, slot: u32) -> u8 {
    let t = keys as *const Table;
    if slot >= (*t).length {
        return 0;
    }
    *attrs_at(t, slot) as u8
}

pub(crate) unsafe fn visit_slots(t: *mut Table, mut visit: impl FnMut(*mut u64)) {
    for i in 0..(*t).length {
        visit(key_at(t, i));
        if *attrs_at(t, i) & SLOT_REF == 0 {
            visit(value_at(t, i));
        }
    }
}

/// [`find`] for a property key given as a string value (`key_bits`, 0 when
/// only the text is at hand) and its text: a property atom key is resolved
/// to its canonical atom without hashing its bytes.
pub(crate) unsafe fn find_key(t: *const Table, key_bits: u64, bytes: &[u8]) -> Option<u32> {
    let key = crate::JSValue::from_bits(key_bits);
    if key_bits != 0 && key.is_string() {
        let atom = crate::string::existing_property_atom(key.as_string_ptr())?;
        return find(t, crate::JSValue::string_ptr(atom as *mut _).bits());
    }
    find_bytes(t, bytes)
}

/// Is `name` an own property key of `obj`, when its overflow can say: an
/// object with an overflow lists every own key there (§3.4), so its answer
/// is authoritative. `None` when `obj` has no overflow (its layout answers).
pub(crate) unsafe fn own_has_bytes(obj: *const ObjectHeader, name: &[u8]) -> Option<bool> {
    let t = table(obj);
    (!t.is_null()).then(|| find_bytes(t, name).is_some())
}

pub(crate) unsafe fn find_bytes(t: *const Table, bytes: &[u8]) -> Option<u32> {
    let hash = super::key_bytes_hash(bytes.as_ptr(), bytes.len());
    let atom = crate::string::atom_lookup(bytes, hash)?;
    find(t, crate::JSValue::string_ptr(atom as *mut _).bits())
}

/// §3.3 for a define of `key` (an atom or a symbol, compared by identity)
/// on an object with an overflow: a key the layout names that has no live
/// entry (it was deleted) gets its entry back as a reference to its fixed
/// slot, so the definition that follows (`insert_key_bits`) meets the slot
/// every static reader loads, exactly as `store_routed` re-attaches a string
/// key. A key with a live entry, or one the layout does not name, is left
/// unchanged.
pub(crate) unsafe fn reattach_fixed(obj: *mut ObjectHeader, key: u64) {
    let t = table(obj);
    if t.is_null() || find(t, key).is_some() {
        return;
    }
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return;
    };
    let keys = record.keys() as usize as *const ArrayHeader;
    if keys.is_null() {
        return;
    }
    let slots = crate::array::array_elements_ptr(keys);
    let Some(fixed) = (0..record.logical_key_count()).find(|&i| *slots.add(i as usize) == key)
    else {
        return;
    };
    let fixed = super::key_attrs::key_storage_slot(keys, fixed);
    let entry = insert_key_bits(obj, key, crate::value::TAG_UNDEFINED, 0);
    let t = table(obj);
    *value_at(t, entry) = crate::value::INT32_TAG | fixed as u64;
    *attrs_at(t, entry) |= SLOT_REF;
}

/// Is entry `i` a plain writable data property holding its own value (not
/// a fixed-slot reference, an accessor or a private name)?
pub(crate) unsafe fn plain_writable_value(t: *const Table, i: u32) -> bool {
    let attrs = *attrs_at(t, i);
    attrs & SLOT_REF == 0
        && *key_at(t, i) != TAG_HOLE
        && super::key_attrs::entry_is_plain_writable_data(attrs as u8)
}

/// Case 2c (`layout_desc::route_family_add`): a static family object, whose
/// layout word never changes, gains `key` (a string or a symbol) that its
/// layout does not hold. The overflow is installed (it orders the object from
/// now on) and the key's entry references fixed slot `slot`, past the layout's
/// slots, where the caller's add stores the value.
pub(crate) unsafe fn append_fixed_ref(obj: *mut ObjectHeader, key: u64, slot: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let key = if crate::JSValue::from_bits(key).is_any_string() {
        crate::string::property_atom_from_value(key)
    } else {
        key
    };
    let entry = insert_key_bits(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        key,
        crate::value::TAG_UNDEFINED,
        0,
    );
    let t = table(owner.get_raw_mut_ptr::<ObjectHeader>());
    *value_at(t, entry) = crate::value::INT32_TAG | slot as u64;
    *attrs_at(t, entry) |= SLOT_REF;
}

/// A table key's positional field is its value or its fixed-slot reference.
pub(crate) unsafe fn store_index(obj: *mut ObjectHeader, slot: u32, value: u64) -> bool {
    let t = table(obj);
    if t.is_null() || slot >= (*t).length {
        return false;
    }
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        let fixed = *value_at(t, slot) as u32;
        if fixed < super::object_live_slot_count(obj) {
            super::store_object_field_slot(obj, fixed as usize, value);
        } else {
            super::spill::spill_set(obj as usize, fixed as usize, value);
        }
    } else {
        *value_at(t, slot) = value;
        crate::gc::runtime_write_barrier_slot(t as usize, value_at(t, slot) as usize, value);
    }
    true
}

/// Delete keeps fixed slot numbers intact. Re-add appends a reference to the
/// fixed binding, so subsequent static and dynamic writes meet at that slot.
pub(crate) unsafe fn delete(obj: *mut ObjectHeader, key: &[u8]) -> bool {
    let hash = super::key_bytes_hash(key.as_ptr(), key.len());
    if table(obj).is_null() {
        // Class-family births have hole-only placeholders, so an absent
        // delete preserves their birth proof. Ordinary shapes retain their
        // existing overflow/order path; this is a typed family operation.
        // A layout key list may hold a spelling that is not yet its atom (a
        // runtime key added by bytes). The table interns every key it
        // orders, so it is established before the atom is looked up.
        if let Some(record) = super::shapes::object_shape_record(obj) {
            let keys = record.keys() as usize as *const ArrayHeader;
            if let Some(slot) =
                super::keys_find_property_slot_by_bytes(record.keys_view(), record.logical_key_count(), key)
            {
                // A hole is an absent property, including allocator-owned
                // placeholders before a derived constructor's super call.
                // Deleting it changes neither membership nor insertion order.
                // The immutable summary proves solid records have no
                // holes; avoid building a key view on their ordinary deletes.
                let hole = super::key_view::summary_may_have_holes(record.summary())
                    && super::key_view::position_is_hole(
                        obj,
                        ObjectKeys::new(keys.cast_mut(), record.logical_key_count()),
                        slot,
                    );
                if hole && super::layout_desc::is_family_object(obj) {
                    return true;
                }
                // A refused delete changes nothing, so it establishes no
                // overflow (and leaves the layout word as it was).
                if super::key_attrs::keys_entry(keys, slot)
                    & super::key_attrs::ENTRY_NON_CONFIGURABLE
                    != 0
                {
                    return false;
                }
                super::layout_desc::note_install_cause(if hole {
                    super::layout_desc::install_cause::DELETE_HOLE
                } else {
                    super::layout_desc::install_cause::DELETE
                });
                ensure(obj);
            } else if super::layout_desc::is_family_object(obj) {
                // An absent family spelling has no order entry to retire.
                return true;
            }
        }
    }
    // A key that was never interned names no entry and no fixed slot.
    let Some(atom) = crate::string::atom_lookup(key, hash) else {
        return true;
    };
    delete_key_bits(obj, crate::JSValue::string_ptr(atom as *mut _).bits())
}

/// Delete the entry keyed `key` (an atom or a symbol). A fixed-slot entry's
/// slot becomes the hole marker. False only for a non-configurable key.
pub(crate) unsafe fn delete_key_bits(obj: *mut ObjectHeader, key: u64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let key = scope.root_nanbox_u64(key);
    let t = ensure(obj);
    let Some(slot) = find(t, key.get_nanbox_u64()) else {
        return true;
    };
    if *attrs_at(t, slot) as u8 & super::key_attrs::ENTRY_NON_CONFIGURABLE != 0 {
        return false;
    }
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let fixed = *value_at(t, slot) as u32;
        if fixed < super::object_live_slot_count(obj) {
            super::store_object_field_slot(obj, fixed as usize, TAG_HOLE);
        } else {
            super::spill::spill_set(obj as usize, fixed as usize, TAG_HOLE);
        }
    }
    tombstone(table(owner.get_raw_mut_ptr::<ObjectHeader>()), slot);
    super::proto_validity::note_object_own_keys_changed(
        owner.get_raw_mut_ptr::<ObjectHeader>() as usize
    );
    true
}

/// Record entry `i`'s key attributes. An accessor never stays a fixed-slot
/// reference: its pair moves into the entry and the fixed slot gets the hole
/// marker. A read that answers from the layout's fixed slot (an own-slot IC
/// way, which compares the layout id without the shadow bit) then sees the
/// hole and misses, because the attributes of an object with an overflow
/// live in its entries, not in its layout. Never allocates.
pub(crate) unsafe fn set_entry_attrs(obj: *mut ObjectHeader, t: *mut Table, i: u32, attrs: u8) {
    let word = attrs_at(t, i);
    *word = (*word & !0xff) | attrs as u64;
    (*t).summary |= u32::from(super::key_attrs::entry_summary(attrs));
    if *word & SLOT_REF != 0 && attrs & super::key_attrs::ENTRY_ACCESSOR != 0 {
        let fixed = *value_at(t, i) as u32;
        let value = if fixed < super::object_live_slot_count(obj) {
            let v = super::field_get_set::object_field_at_with_live(
                obj,
                fixed,
                super::object_live_slot_count(obj),
            )
            .bits();
            super::store_object_field_slot(obj, fixed as usize, TAG_HOLE);
            v
        } else {
            let v =
                super::spill::spill_get_present(obj as usize, fixed as usize).unwrap_or(TAG_HOLE);
            super::spill::spill_set(obj as usize, fixed as usize, TAG_HOLE);
            v
        };
        *word &= !SLOT_REF;
        *value_at(t, i) = value;
        crate::gc::runtime_write_barrier_slot(t as usize, value_at(t, i) as usize, value);
    }
}

/// An own read of a receiver that has an overflow (§3.4): the overflow holds
/// every own key, a fixed key as a slot reference, so one atom-keyed hash hit
/// answers it, a getter included. `None` when `key` is not an own key (the
/// caller continues with the prototype chain) or when the entry needs the
/// complete read (a native alias's undefined).
pub(crate) unsafe fn own_get(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<crate::JSValue> {
    let t = table(obj);
    if t.is_null() || key.is_null() {
        return None;
    }
    let atom = crate::string::existing_property_atom(key)?;
    own_get_entry(obj, t, crate::JSValue::string_ptr(atom as *mut _).bits())
}

/// A static read site's miss (§3.7 case 3): when the receiver is an ordinary
/// object whose shadow bit says its own keys extend past its layout, one
/// atom-keyed overflow probe answers an own key. `None` sends the read on to
/// the complete miss path (the prototype chain, exotics, absence).
#[inline]
pub(crate) unsafe fn site_miss_own_get(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<crate::JSValue> {
    if (obj as u64) >> 48 != 0
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
        || (*crate::gc::header_from_trusted_user_ptr(obj.cast())).obj_type
            != crate::gc::GC_TYPE_OBJECT
        || !shadowed(obj)
    {
        return None;
    }
    own_get(obj, key)
}

/// [`own_get`] for a key given as a value: a string (heap or inline), whose
/// atom is looked up once, or a symbol, which is its own key.
pub(crate) unsafe fn own_get_key(
    obj: *const ObjectHeader,
    key_bits: u64,
) -> Option<crate::JSValue> {
    let t = table(obj);
    if t.is_null() {
        return None;
    }
    own_get_entry(obj, t, overflow_key(key_bits)?)
}

/// Whether `key_bits` (a string or a symbol) is an own key of `obj`, when
/// its overflow can say: an entry answers yes. On an ordinary object with no
/// indexed elements the overflow holds every own key, so a miss (or a string
/// no atom spells) answers no. `None` when `obj` has no overflow, or when a
/// miss is not conclusive.
pub(crate) unsafe fn own_has_key(obj: *const ObjectHeader, key_bits: u64) -> Option<bool> {
    let t = table(obj);
    // A class instance can hold runtime-internal keys that reflection hides
    // (`own_slot_hidden`): the complete check owns those.
    if t.is_null() || (*obj).class_id != 0 {
        return None;
    }
    if overflow_key(key_bits).is_some_and(|key| find(t, key).is_some()) {
        return Some(true);
    }
    let meta = (*obj).meta;
    let conclusive = (*meta).elements == 0
        && super::object_is_regular(obj)
        && super::native_this_alias::object_alias(obj).is_none();
    conclusive.then_some(false)
}

/// The overflow key of a property key value: a string's property atom, if
/// one exists (a string with no atom names no entry), or a symbol itself.
unsafe fn overflow_key(key_bits: u64) -> Option<u64> {
    let key = crate::JSValue::from_bits(key_bits);
    if key.is_string() {
        let atom = crate::string::existing_property_atom(key.as_string_ptr())?;
        Some(crate::JSValue::string_ptr(atom as *mut _).bits())
    } else if key.is_short_string() {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let bytes = crate::string::js_string_key_bytes(key, &mut scratch)?;
        let atom =
            crate::string::atom_lookup(bytes, super::key_bytes_hash(bytes.as_ptr(), bytes.len()))?;
        Some(crate::JSValue::string_ptr(atom as *mut _).bits())
    } else if crate::symbol::js_is_symbol(f64::from_bits(key_bits)) != 0 {
        Some(key_bits)
    } else {
        None
    }
}

unsafe fn own_get_entry(
    obj: *const ObjectHeader,
    t: *mut Table,
    key: u64,
) -> Option<crate::JSValue> {
    let i = find(t, key)?;
    let live = super::object_live_slot_count(obj);
    // A getter with an overridden receiver (Reflect.get, super) takes the
    // complete read, which consumes the override.
    if super::key_attrs::key_is_accessor_at(t.cast(), i)
        && super::field_get_set::accessor_receiver_override_armed()
    {
        return None;
    }
    if let Some(value) = super::field_get_set::object_accessor_at_with_live(obj, t.cast(), i, live)
    {
        return Some(value);
    }
    let value = crate::JSValue::from_bits(get_from(t, obj, i)?);
    if value.bits() == TAG_HOLE
        || (value.is_undefined() && super::native_this_alias::object_alias(obj).is_some())
    {
        return None;
    }
    Some(value)
}

/// The shared dynamic writer. The immutable shape key list is probed first;
/// only an unmatched name becomes a table value. No key-add shape is minted.
pub(crate) unsafe fn store_routed(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_f64(value);
    let atom = crate::string::property_atom(key);
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    // A fixed-slot hit changes no key the layout does not already name; only
    // an overflow entry shadows (`install` sets the bit).
    let record = super::shapes::object_shape_record(obj).unwrap();
    let fixed = super::keys_find_property_slot_by_key_ptr(
        record.keys_view(),
        record.logical_key_count(),
        atom,
    );
    if let Some(position) = fixed {
        let fixed = super::key_attrs::key_storage_slot(
            record.keys() as usize as *const ArrayHeader,
            position,
        );
        let old = if fixed < record.live_inline_slot_count() {
            super::field_get_set::object_field_at_with_live(
                obj,
                fixed,
                record.live_inline_slot_count(),
            )
            .bits()
        } else {
            super::spill::spill_get_present(obj as usize, fixed as usize).unwrap_or(TAG_HOLE)
        };
        let t = table(obj);
        let detached = if old == TAG_HOLE && !t.is_null() {
            find(t, crate::JSValue::string_ptr(atom as *mut _).bits())
                .filter(|&i| *attrs_at(t, i) & SLOT_REF == 0)
        } else {
            None
        };
        if let Some(i) = detached {
            // The key's entry owns its value (`set_entry_attrs`).
            store_index(obj, i, val.get_nanbox_f64().to_bits());
        } else {
            if old == TAG_HOLE {
                let slot = insert(obj, atom, crate::value::TAG_UNDEFINED, 0);
                let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
                let t = table(obj);
                *value_at(t, slot) = crate::value::INT32_TAG | fixed as u64;
                *attrs_at(t, slot) |= SLOT_REF;
            }
            let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
            if fixed < super::object_live_slot_count(obj) {
                super::store_object_field_slot(obj, fixed as usize, val.get_nanbox_f64().to_bits());
            } else {
                super::spill::spill_set(
                    obj as usize,
                    fixed as usize,
                    val.get_nanbox_f64().to_bits(),
                );
            }
        }
    } else {
        store_unfixed(obj, atom, val.get_nanbox_f64().to_bits());
    }
    super::proto_validity::note_object_shape_stamped(
        owner.get_raw_mut_ptr::<ObjectHeader>() as usize,
        0,
        (*owner.get_raw_mut_ptr::<ObjectHeader>()).parent_class_id,
    );
}

/// A write of a key the receiver's layout does not hold: the existing entry,
/// or one append. `atom` is a property atom.
unsafe fn store_unfixed(obj: *mut ObjectHeader, atom: *const crate::StringHeader, value: u64) {
    let key = crate::JSValue::string_ptr(atom as *mut _).bits();
    let t = table(obj);
    match if t.is_null() { None } else { find(t, key) } {
        Some(slot) => {
            store_index(obj, slot, value);
        }
        None => {
            insert_key_bits(obj, key, value, 0);
        }
    }
}

/// The entry of `atom` (a property atom) in `obj`'s overflow when it holds a
/// plain writable data value of its own (not a fixed-slot reference, an
/// accessor, readonly or private entry): an own overwrite's target. `None`
/// for no overflow or no such entry.
///
/// # Safety
/// `obj` is a live object word.
#[inline]
pub(crate) unsafe fn own_plain_value_entry(
    obj: *const ObjectHeader,
    atom: *const crate::StringHeader,
) -> Option<u32> {
    let t = table(obj);
    if t.is_null() {
        return None;
    }
    let i = find(t, crate::JSValue::string_ptr(atom as *mut _).bits())?;
    plain_writable_value(t, i).then_some(i)
}

/// Overwrite entry `i` (from [`own_plain_value_entry`]) of `obj`'s overflow
/// with `value`: the entry store and the publication note
/// [`store_unfixed_routed`] gives an existing entry. Nothing allocates.
///
/// # Safety
/// `obj` is a live ordinary object, `i` names a plain value entry of its
/// overflow.
#[inline]
pub(crate) unsafe fn store_own_value_entry(obj: *mut ObjectHeader, i: u32, value: f64) {
    store_index(obj, i, value.to_bits());
    super::proto_validity::note_object_shape_stamped(obj as usize, 0, (*obj).parent_class_id);
}

/// [`store_routed`] for a dynamic write whose caller has already resolved
/// `atom` (a property atom) and proved the layout does not hold it.
pub(crate) unsafe fn store_unfixed_routed(
    obj: *mut ObjectHeader,
    atom: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    store_unfixed(obj, atom, value.to_bits());
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    super::proto_validity::note_object_shape_stamped(obj as usize, 0, (*obj).parent_class_id);
}

/// The canonical key of a static atom (§3.5): the agent's one string for it.
#[inline]
unsafe fn static_atom_key(
    atom: *const super::layout_desc::StaticAtom,
) -> *const crate::StringHeader {
    crate::string::property_atom((*atom).string)
}

/// §6a overflow API: the value of `atom` in `obj`'s overflow, or the hole
/// marker when the object has no such entry.
///
/// # Safety
/// `obj` is a live ordinary object; `atom` a static atom of this program.
#[no_mangle]
pub unsafe extern "C" fn perry_overflow_get(
    obj: *const ObjectHeader,
    atom: *const super::layout_desc::StaticAtom,
) -> u64 {
    let t = table(obj);
    if t.is_null() || atom.is_null() {
        return TAG_HOLE;
    }
    let key = crate::JSValue::string_ptr(static_atom_key(atom) as *mut _).bits();
    find(t, key)
        .and_then(|slot| get_from(t, obj, slot))
        .unwrap_or(TAG_HOLE)
}

/// §6a overflow API: a dynamic write of `atom`. It meets the layout's slot
/// when the layout names the key (§3.3); otherwise it appends to the overflow.
/// Non-zero `attrs` are recorded on the key's entry.
///
/// # Safety
/// `obj` is a live, extensible ordinary object; `atom` a static atom.
#[no_mangle]
pub unsafe extern "C" fn perry_overflow_set(
    obj: *mut ObjectHeader,
    atom: *const super::layout_desc::StaticAtom,
    value: u64,
    attrs: u8,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let key = static_atom_key(atom);
    store_routed(obj, key, f64::from_bits(value));
    if attrs != 0 {
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let t = ensure(obj);
        if let Some(slot) = find(t, crate::JSValue::string_ptr(key as *mut _).bits()) {
            set_entry_attrs(owner.get_raw_mut_ptr::<ObjectHeader>(), t, slot, attrs);
        }
    }
}

/// §6a overflow API: delete `atom` from `obj`. False only for a
/// non-configurable property.
///
/// # Safety
/// `obj` is a live ordinary object; `atom` a static atom.
#[no_mangle]
pub unsafe extern "C" fn perry_overflow_delete(
    obj: *mut ObjectHeader,
    atom: *const super::layout_desc::StaticAtom,
) -> bool {
    let s = (*atom).string;
    let bytes = std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
    delete(obj, bytes)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_OVERFLOW_GET: unsafe extern "C" fn(
    *const ObjectHeader,
    *const super::layout_desc::StaticAtom,
) -> u64 = perry_overflow_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_OVERFLOW_SET: unsafe extern "C" fn(
    *mut ObjectHeader,
    *const super::layout_desc::StaticAtom,
    u64,
    u8,
) = perry_overflow_set;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PERRY_OVERFLOW_DELETE: unsafe extern "C" fn(
    *mut ObjectHeader,
    *const super::layout_desc::StaticAtom,
) -> bool = perry_overflow_delete;

#[cfg(test)]
pub(crate) unsafe fn establish_order(obj: *mut ObjectHeader) -> bool {
    ensure(obj);
    true
}
