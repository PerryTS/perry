//! Fresh Object.keys results from ordinary shape storage.
use super::*;

/// `Object.keys` of an ordinary object, read from its structure (DYNP E2):
/// one result allocation, filled with the object's own key strings in order
/// (the layout's shared key strings over present slots, or the table's
/// entries in insertion order). Attributes come from the summary and the key
/// entries; nothing is decoded by name except a table entry whose first byte
/// is a digit, `_` or `#` (`key_attrs::key_name_bits`). `None` sends every
/// receiver the structure does not prove to the generic builder
/// (`js_object_keys_shape`): exotic kinds, native and synthetic classes,
/// Array-subclass instances, private names, a possible array-index key (ECMA
/// order), a possible runtime-internal name, and a class instance with a
/// non-enumerable key (class storage resolves repeated declarations by name).
pub(super) fn own_enumerable_keys(obj: *const ObjectHeader) -> Option<*mut ArrayHeader> {
    let addr = strip_nanbox_addr(obj);
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
    unsafe { keys_of_headed(addr as *const ObjectHeader, header.as_ref().obj_type) }
}

/// [`own_enumerable_keys`] of a NaN-boxed value. A `POINTER_TAG` word names a
/// GC-headed allocation by the honest-pointer invariant
/// (`addr_class::pointer_value_header`), so no allocator proof runs.
#[inline]
pub(super) fn own_enumerable_keys_of_value(bits: u64) -> Option<*mut ArrayHeader> {
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    let obj = (bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    unsafe { keys_of_headed(obj, header.obj_type) }
}

/// `Object.values` / `Object.entries` of a NaN-boxed ordinary object from its
/// structure (KEYSARR): taken exactly when [`keys_in_place`] proves the
/// record's key list is the own enumerable key sequence verbatim and the
/// summary has no accessor. Then no [[Get]] runs user code (no key can vanish
/// or change while the values are read), and with no attribute words key `i`
/// lives in storage slot `i`: value `i` is a slot read, never a lookup by
/// name. Entries pair the verbatim key-list copy (the Object.keys funnel) with
/// those values. Fresh arrays every call; nothing is stored. `None`: the
/// generic builder.
pub(super) fn own_enumerable_values_of_value(bits: u64, pairs: bool) -> Option<*mut ArrayHeader> {
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    let obj = (bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    unsafe { values_of_headed(obj, header.obj_type, pairs) }
}

/// [`own_enumerable_values_of_value`] of a raw receiver pointer.
pub(super) fn own_enumerable_values(
    obj: *const ObjectHeader,
    pairs: bool,
) -> Option<*mut ArrayHeader> {
    let addr = strip_nanbox_addr(obj);
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
    unsafe { values_of_headed(addr as *const ObjectHeader, header.as_ref().obj_type, pairs) }
}

unsafe fn values_of_headed(
    obj: *const ObjectHeader,
    obj_type: u8,
    pairs: bool,
) -> Option<*mut ArrayHeader> {
    if crate::gc_type_is_not_object!(obj_type) {
        return None;
    }
    let record = crate::object::shapes::object_shape_record(obj)?;
    if record.summary() & crate::object::key_attrs::SUMMARY_ACCESSOR != 0 {
        return None;
    }
    let (k0, _, n) = keys_in_place(obj, record)?;
    if let Some(out) = values_young(obj, k0, n, pairs) {
        return Some(out);
    }
    // A refused young birth: the collecting allocations, the receiver rooted.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_const_ptr(obj);
    // Values: one birth of capacity `n`, then `n` slot reads (no allocation).
    let (values, obj) = obj_h.across_const::<ObjectHeader, _>(|| crate::array::js_array_alloc(n));
    let live = crate::object::object_live_slot_count(obj);
    let mut out = values;
    for i in 0..n {
        let v = super::object_field_at_with_live(obj, i, live);
        out = crate::array::js_array_push_f64(out, f64::from_bits(v.bits()));
    }
    if !pairs {
        return Some(out);
    }
    let values_h = scope.root_raw_mut_ptr(out);
    // Keys: the verbatim copy of the same key list (`keys_of_record`).
    let (keys, obj) = obj_h.across_const::<ObjectHeader, _>(|| {
        let obj: *const ObjectHeader = obj_h.with_const_ptr(|o| o);
        crate::object::shapes::object_shape_record(obj)
            .and_then(|record| keys_of_record(obj, record))
            .map(|(keys, _)| keys)
    });
    let _ = obj;
    let keys = keys?;
    let keys_h = scope.root_raw_mut_ptr(keys);
    let count = crate::array::js_array_length(keys).min(n);
    let (result, _) = keys_h.across_mut::<ArrayHeader, _>(|| crate::array::js_array_alloc(count));
    let out_h = scope.root_raw_mut_ptr(result);
    for i in 0..count {
        let pair = crate::array::js_array_alloc(2);
        let key = keys_h.with_mut_ptr(|k: *mut ArrayHeader| crate::array::js_array_get(k, i));
        let value = values_h.with_mut_ptr(|v: *mut ArrayHeader| crate::array::js_array_get(v, i));
        let pair = crate::array::js_array_push_f64(pair, f64::from_bits(key.bits()));
        let pair = crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));
        let grown = out_h.with_mut_ptr(|o: *mut ArrayHeader| {
            crate::array::js_array_push_f64(o, f64::from_bits(JSValue::array_ptr(pair).bits()))
        });
        out_h.set_raw_mut_ptr(grown);
    }
    Some(out_h.with_mut_ptr(|p| p))
}

/// [`values_of_headed`] through young births that cannot collect
/// (`js_array_try_alloc_tag_scanned_unfilled`): nothing moves, so `obj` and
/// its key list `k0` stay current, the young results need no barrier, and
/// the words are written in place. A value string is shared the way a push
/// shares it (`js_string_addref_if_heap_string`); key words are shared by P1
/// (`copy_key_is_shared`). `None` when a birth is refused (the partial result
/// is dropped unreachable).
///
/// # Safety
/// `obj` is a live ordinary layout object; `k0` is its record's dense key list
/// of `n` keys (`keys_in_place`), key `i` in storage slot `i`.
#[inline(always)]
unsafe fn values_young(
    obj: *const ObjectHeader,
    k0: *mut ArrayHeader,
    n: u32,
    pairs: bool,
) -> Option<*mut ArrayHeader> {
    let out = crate::array::js_array_try_alloc_tag_scanned_unfilled(n);
    if out.is_null() {
        return None;
    }
    let live = crate::object::object_live_slot_count(obj);
    let dest = crate::array::array_elements_ptr(out) as *mut u64;
    let keys = crate::array::array_elements_ptr(k0) as *const u64;
    for i in 0..n {
        let value = super::object_field_at_with_live(obj, i, live).bits();
        crate::string::js_string_addref_if_heap_string(f64::from_bits(value));
        let word = if pairs {
            let pair = crate::array::js_array_try_alloc_tag_scanned_unfilled(2);
            if pair.is_null() {
                (*out).length = i;
                crate::array::fill_capacity_with_holes_from(out, i);
                return None;
            }
            let pd = crate::array::array_elements_ptr(pair) as *mut u64;
            let key = *keys.add(i as usize);
            debug_assert!(copy_key_is_shared(key));
            // GC_STORE_AUDIT(INIT): young pair before its length is published.
            pd.write(key);
            pd.add(1).write(value);
            (*pair).length = 2;
            crate::array::fill_capacity_with_holes_from(pair, 2);
            JSValue::array_ptr(pair).bits()
        } else {
            value
        };
        // GC_STORE_AUDIT(INIT): young result before its length is published.
        dest.add(i as usize).write(word);
    }
    (*out).length = n;
    crate::array::fill_capacity_with_holes_from(out, n);
    Some(out)
}

/// `for-in` keys of a NaN-boxed value from structure (DYNP E3): the own
/// enumerable keys ([`own_enumerable_keys_of_value`]) when every prototype
/// level is an ordinary object that structurally has no enumerable string key
/// ([`level_has_no_enumerable_key`]); then no level contributes a key and no
/// own key can be shadowed, so the result is the own key list, the loop's
/// snapshot. The chain is walked before the result is allocated and runs no
/// user code. `None`: the generic walk (`for_in_keys_with`).
#[inline]
/// `Some((keys, fact, len))`: the keys, their loop fact ([`for_in_loop_fact`],
/// read off the copy that made it: no second pass over the keys) and the
/// loop's bound `len`. `in_place`: the caller holds `len` as the loop's bound
/// for this one execution, so the receiver's own key list may be the keys
/// ([`keys_in_place`]); otherwise they are a snapshot of length `len`.
pub(super) fn for_in_keys_of_value(
    bits: u64,
    in_place: bool,
) -> Option<(*mut ArrayHeader, u64, u32)> {
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    if crate::gc_type_is_not_object!(header.obj_type) {
        return None;
    }
    let receiver = (bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    let mut level = receiver;
    // The receiver's record states its link and then names its own keys; each
    // later level's one record answers both its enumerable-key test and its link.
    let receiver_record = unsafe { crate::object::shapes::object_shape_record(level) }?;
    let mut record = receiver_record;
    // Depth cap as in the generic walk.
    for _ in 0..1000 {
        let proto =
            unsafe { crate::object::object_ops::structural_prototype_link_of(level, record) }?;
        let pbits = proto.to_bits();
        if JSValue::from_bits(pbits).is_null() {
            if in_place {
                if let Some(keys) = unsafe { keys_in_place(receiver, receiver_record) } {
                    return Some(keys);
                }
            }
            return unsafe { keys_of_record(receiver, receiver_record) }
                .map(|(keys, fact)| (keys, fact, unsafe { (*keys).length } as u32));
        }
        let ph = crate::value::addr_class::pointer_value_header(pbits)?;
        if crate::gc_type_is_not_object!(ph.obj_type) {
            return None;
        }
        level = (pbits & crate::value::POINTER_MASK) as *const ObjectHeader;
        record = unsafe { crate::object::shapes::object_shape_record(level) }?;
        if !unsafe { level_has_no_enumerable_key(level, record) } {
            return None;
        }
    }
    None
}

/// The for-in keys of `obj` in place (DYNP E6 step 4): the record's own key
/// list `K0` and the bound `n` = its logical key count, with no birth and no
/// copy. Sound because the loop reads only `K0[i]` for `i < n`, and those
/// positions never change (P2): a canonical backing's prefix is immutable (the
/// only in-place write is the tip append past every published count,
/// `canonical_keys`), and a list that is rewritten is replaced, not written
/// (`shapes.rs` rekeys; the squeeze in `delete_rest.rs` rewrites only owned
/// lists, never a record's). Taken when the copy would be `K0[..n]` verbatim:
/// an ordinary layout object without a table, every key an enumerable string
/// (no private, non-enumerable, optional-hole, index, internal or symbol key)
/// and no class refusal of [`keys_of_record`]; the list is the record's own
/// live array (not forwarded) and carries no attribute words. The fact is
/// the layout fact of the whole list ([`for_in_loop_fact`]: key `i < n` is
/// layout key `i` while the word is unchanged).
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell; `record` is its live record.
#[inline(always)]
unsafe fn keys_in_place(
    obj: *const ObjectHeader,
    record: crate::object::shapes::ShapeRecordRef,
) -> Option<(*mut ArrayHeader, u64, u32)> {
    use crate::object::key_attrs as ka;
    if !matches!(
        record.object_kind(),
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
    ) || !table_of_object(obj).is_null()
    {
        return None;
    }
    let word = (*obj).parent_class_id;
    if !crate::object::shapes::is_layout_word(word) {
        return None;
    }
    let summary = record.summary();
    if summary & (ka::SUMMARY_PRIVATE | ka::SUMMARY_NON_ENUMERABLE) != 0
        || crate::object::key_view::summary_may_have_holes(summary)
        || record.enum_bits() & (ka::ENUM_INDEX_KEY | ka::ENUM_INTERNAL_KEY) != 0
        || !record.proves_no_symbols()
    {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id != 0
        && (class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
            || crate::array::subclass_elements::backed_raw(obj as usize).is_some())
    {
        return None;
    }
    let n = record.logical_key_count();
    let k0 = record.keys() as usize as *mut ArrayHeader;
    if n == 0 || n >= 1 << 30 || k0.is_null() {
        return None;
    }
    let (slots, len) = record.keys_view().dense_slots();
    if len != n as usize
        || slots.add(0) as *const u64 != crate::array::array_elements_ptr(k0) as *const u64
        || crate::array::array_object_flags_resolved(k0) & crate::gc::GC_ARRAY_KEY_ATTRS != 0
    {
        return None;
    }
    Some((k0, FACT_SET | (u64::from(n) << 32) | u64::from(word), n))
}

/// Loop fact bit: the fact holds an answer.
const FACT_SET: u64 = 1 << 63;
/// Loop fact bit: the receiver's own keys are an overflow table.
const FACT_TABLE: u64 = 1 << 62;

/// The for-in loop fact (DYNP E3c), derived once at loop start from the
/// receiver `obj_bits` and its just-born snapshot `snap`, held by the loop in
/// a compiled-code local for this one execution. It names the longest prefix
/// `n` of the snapshot that is the receiver's own key sequence in place:
/// - a layout without a table: snapshot key `i < n` is layout key `i`; the
///   fact holds the layout word.
/// - a table: snapshot key `i < n` is the table entry `base + i` (`base` =
///   the leading non-string entries); the fact holds `base`.
/// Bits: [`FACT_SET`], [`FACT_TABLE`], `n` in 32..62, word or base in 0..32.
/// 0 when nothing is answered (the complete `in` runs per key).
///
/// # Safety
/// Any value bits; `snap` is the loop's snapshot or null.
pub(super) unsafe fn for_in_loop_fact(obj_bits: u64, snap: *const ArrayHeader) -> u64 {
    if snap.is_null() {
        return 0;
    }
    let Some(header) = crate::value::addr_class::pointer_value_header(obj_bits) else {
        return 0;
    };
    if crate::gc_type_is_not_object!(header.obj_type) {
        return 0;
    }
    let obj = (obj_bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return 0;
    };
    if !matches!(
        record.object_kind(),
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
    ) {
        return 0;
    }
    let snap_len = (*snap).length as usize;
    let sk = crate::array::array_elements_ptr(snap) as *const u64;
    let t = table_of_object(obj);
    let (n, low, kind) = if t.is_null() {
        let word = (*obj).parent_class_id;
        if !crate::object::shapes::is_layout_word(word) {
            return 0;
        }
        let keys = crate::object::object_keys(obj);
        let (s, len) = keys.dense_slots();
        if len == 0 {
            return 0;
        }
        let src = s.add(0) as *const u64;
        let limit = len.min(snap_len);
        let mut m = 0;
        while m < limit && *src.add(m) == *sk.add(m) {
            m += 1;
        }
        (m, word as u64, FACT_SET)
    } else {
        let src = crate::object::overflow::keys_ptr(t) as *const u64;
        let len = (*t).length as usize;
        let mut base = 0;
        while base < len && !JSValue::from_bits(*src.add(base)).is_any_string() {
            base += 1;
        }
        let limit = (len - base).min(snap_len);
        let mut m = 0;
        while m < limit && *src.add(base + m) == *sk.add(m) {
            m += 1;
        }
        (m, base as u64, FACT_SET | FACT_TABLE)
    };
    if n == 0 || n >= 1 << 30 || low >> 32 != 0 {
        return 0;
    }
    kind | ((n as u64) << 32) | low
}

/// The per-key deletion check under the loop fact `fact`
/// ([`for_in_loop_fact`]): true when snapshot key `key` at index `i` is still
/// an own property of the loop's receiver `obj_bits`. A layout: the receiver
/// still carries the fact's layout word (same key sequence, no delete since). A table: the receiver's current table (reread:
/// growth and compaction replace it) holds exactly `key` at `base + i`, not a
/// hole. `false` = not answered here (the complete `in` runs).
///
/// # Safety
/// `obj_bits` is the receiver the fact was taken from (the loop's own local).
#[inline(always)]
pub(super) unsafe fn key_live_by_fact(obj_bits: u64, key_bits: u64, i: u32, fact: u64) -> bool {
    use crate::object::key_view as kv;
    let n = ((fact >> 32) & 0x3fff_ffff) as u32;
    if i >= n {
        return false;
    }
    let obj = (obj_bits & crate::value::POINTER_MASK) as *const ObjectHeader;
    if fact & FACT_TABLE == 0 {
        // The word alone answers: with no table, every write of the hole
        // into a slot is a delete, and each one moves the word (the delete
        // transitions in `delete_rest.rs`: `js_object_delete_field`, the SSO
        // delete and its squeeze, which rekeys or restores); a table install
        // sets the shadow bit in it (`overflow::install`), so the in-place
        // table deletes never meet an unchanged layout word.
        (*obj).parent_class_id == fact as u32
    } else {
        let t = table_of_object(obj);
        if t.is_null() {
            return false;
        }
        let pos = fact as u32 + i;
        pos < (*t).length
            && *(crate::object::overflow::keys_ptr(t) as *const u64).add(pos as usize) == key_bits
            && (!kv::summary_may_have_holes((*t).summary as u8)
                || !kv::position_is_hole(obj, crate::object::object_keys(obj), pos))
    }
}

/// True when the ordinary object `obj` has no enumerable string key, read from
/// its structure: a layout's record birth bit `ENUM_ANY` (exact for the
/// immutable key prefix; a mutable prefix is born with every bit set), or a
/// table whose live string entries all carry `ENTRY_NON_ENUMERABLE` (a table
/// without `SUMMARY_NON_ENUMERABLE` has an enumerable key as soon as it has a
/// string key). Exotic kinds and element-backed objects answer false.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` cell; `record` is its live record.
#[inline]
unsafe fn level_has_no_enumerable_key(
    obj: *const ObjectHeader,
    record: crate::object::shapes::ShapeRecordRef,
) -> bool {
    use crate::object::key_attrs as ka;
    if !matches!(
        record.object_kind(),
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
    ) {
        return false;
    }
    if (*obj).class_id != 0 && crate::array::subclass_elements::backed_raw(obj as usize).is_some() {
        return false;
    }
    let t = table_of_object(obj);
    if t.is_null() {
        return record.enum_bits() & ka::ENUM_ANY == 0;
    }
    // A table level (Object.prototype, the builtin prototypes) is decided by
    // its summary: no entry was ever written enumerable.
    (*t).summary & ka::TABLE_ENUM_ANY == 0
}

/// [`crate::object::overflow::table`] of an object whose GC header was read
/// (a `GC_TYPE_OBJECT` cell, or the receiver a loop fact was taken from):
/// its address is a heap cell, so only the shadow bit, the shaped test and
/// the meta load remain.
#[inline(always)]
unsafe fn table_of_object(obj: *const ObjectHeader) -> *mut crate::object::overflow::Table {
    if !crate::object::overflow::shadowed(obj) || !crate::object::object_is_shaped(obj) {
        return std::ptr::null_mut();
    }
    let meta = crate::meta_read!(crate::meta_obj_from_kind!(obj as *mut _, ()));
    if meta.is_null() {
        return std::ptr::null_mut();
    }
    (*meta).overflow as usize as *mut crate::object::overflow::Table
}

/// The shared body: `obj` is a GC-headed allocation of type `obj_type`.
#[inline]
unsafe fn keys_of_headed(obj: *const ObjectHeader, obj_type: u8) -> Option<*mut ArrayHeader> {
    if crate::gc_type_is_not_object!(obj_type) {
        return None;
    }
    let record = crate::object::shapes::object_shape_record(obj)?;
    keys_of_record(obj, record).map(|(keys, _)| keys)
}

/// [`keys_of_headed`] of the `GC_TYPE_OBJECT` cell `obj` with its live
/// record `record`: the own enumerable keys and the for-in loop fact of that
/// list ([`for_in_loop_fact`]), derived by the copy.
#[inline(always)]
unsafe fn keys_of_record(
    obj: *const ObjectHeader,
    record: crate::object::shapes::ShapeRecordRef,
) -> Option<(*mut ArrayHeader, u64)> {
    use crate::object::key_attrs as ka;
    if !matches!(
        record.object_kind(),
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
    ) {
        return None;
    }
    let t = table_of_object(obj);
    // A table's name facts are its keys' (tested while copying); a layout's
    // are its record's birth facts.
    let (summary, bits) = if t.is_null() {
        (record.summary(), record.enum_bits())
    } else {
        ((*t).summary as u8, 0)
    };
    if summary & ka::SUMMARY_PRIVATE != 0
        || bits & (ka::ENUM_INDEX_KEY | ka::ENUM_INTERNAL_KEY) != 0
    {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id != 0
        && (class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
            || summary & ka::SUMMARY_NON_ENUMERABLE != 0
            || crate::array::subclass_elements::backed_raw(obj as usize).is_some())
    {
        return None;
    }
    let count = if t.is_null() {
        record.logical_key_count()
    } else {
        (*t).length
    };
    if count == 0 {
        return Some((crate::array::js_array_alloc(0), 0));
    }
    let holes = crate::object::key_view::summary_may_have_holes(summary);
    let non_enumerable = summary & ka::SUMMARY_NON_ENUMERABLE != 0;
    // A young birth that cannot collect (`arena_alloc_gc_no_collect`): nothing
    // has moved, so the receiver, its key list and its table read above are
    // current, and the young result needs no barrier.
    let out = crate::array::js_array_try_alloc_tag_scanned_unfilled(count);
    if !out.is_null() {
        // A table whose key column is its own enumerable keys verbatim past
        // its keyless prefix ([`copy_table_keys_verbatim`]) is copied as one
        // block, out of line: the layout copy below keeps its own shape.
        if !t.is_null()
            && !holes
            && !non_enumerable
            && (*t).summary & ka::TABLE_KEY_HIDDEN == 0
            && (*t).deleted == 0
        {
            return Some((out, copy_table_keys_verbatim(t, out, count)));
        }
        // A table's entry words are read only for attributed keys.
        let keys = if !t.is_null() {
            if holes || non_enumerable {
                crate::object::ObjectKeys::owned(t.cast())
            } else {
                crate::object::ObjectKeys::NONE
            }
        } else if record.keys() != 0 {
            record.keys_view()
        } else {
            crate::object::ObjectKeys::NONE
        };
        let verbatim = t.is_null() && record.proves_no_symbols();
        return copy_own_keys(
            obj,
            keys,
            t,
            out,
            count,
            holes,
            non_enumerable,
            verbatim,
            true,
        )
        .map(|fact| (out, fact));
    }
    // A refused birth takes the collecting allocation: root the receiver and
    // re-read its key list (layout keys or table) after it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_const_ptr(obj);
    // Born tag-scanned at its final capacity: the copy below publishes the
    // length once, with no layout rebuild.
    let out = crate::array::js_array_alloc_tag_scanned(count);
    let complete = owner.with_const_ptr::<ObjectHeader, _>(|obj| {
        let keys = crate::object::object_keys(obj);
        let t = table_of_object(obj);
        copy_own_keys(
            obj,
            keys,
            t,
            out,
            count,
            holes,
            non_enumerable,
            false,
            false,
        )
    });
    complete.map(|fact| (out, fact))
}

/// Copies the own enumerable keys of `obj` (its key list `keys`, its table `t`
/// or null) into the fresh result `out` and publishes its length; `false` when
/// a table entry's name needs the generic builder, else the loop fact of the
/// copied list ([`for_in_loop_fact`]: the in-place prefix ends at the first
/// skipped key; a table's leading non-string entries are its base). `young`: `out` came from
/// [`crate::array::js_array_try_alloc_tag_scanned_unfilled`] (young, its
/// capacity not yet filled): the unwritten capacity gets the hole here, on
/// both exits, and no barrier runs. Runs no allocation.
///
/// # Safety
/// `obj`, `keys` and `t` are current; `out` has capacity for `count` keys.
/// P1: a key word in a key list or table column is never a uniquely owned
/// heap string (refcount 1): layout keys were demoted when the list took them
/// (`ensure_key_in_keys_array_inner`, the string ownership rule) or are
/// interned literals; table keys are property atoms. So the result array
/// shares them with no per-key addref.
#[inline(always)]
unsafe fn copy_key_is_shared(key: u64) -> bool {
    key & crate::value::TAG_MASK != crate::value::STRING_TAG
        || (*((key & crate::value::POINTER_MASK) as *const crate::StringHeader)).refcount != 1
}

/// The own enumerable keys of the table `t` into the young unfilled result
/// `out` ([`crate::array::js_array_try_alloc_tag_scanned_unfilled`], capacity
/// `count`) when its key column is them verbatim past the keyless prefix: no
/// entry was written but by the plain append (no `TABLE_KEY_HIDDEN`: string
/// atoms with plain names), none was tombstoned (`deleted == 0`) and no entry
/// is attributed (the caller's `!holes && !non_enumerable`). The prefix
/// `[0, fixed_prefix)` holds keyless holes and is the fact's base, as the
/// per-key loop of [`copy_own_keys`] counts it; `keys[fixed_prefix..length)`
/// is one block of shared key words (P1: table keys are property atoms).
/// Out of line so the layout copy keeps its shape. Returns the loop fact.
///
/// # Safety
/// `t` is the current table of a live object; `out` is as above.
#[inline(never)]
unsafe fn copy_table_keys_verbatim(
    t: *mut crate::object::overflow::Table,
    out: *mut ArrayHeader,
    count: u32,
) -> u64 {
    let source = crate::object::overflow::keys_ptr(t) as *const u64;
    let available = ((*t).length as usize).min(count as usize);
    let lead = ((*t).fixed_prefix as usize).min(available);
    debug_assert!((0..lead).all(|i| *source.add(i) == crate::value::TAG_HOLE));
    debug_assert!((lead..available).all(|i| {
        JSValue::from_bits(*source.add(i)).is_any_string() && copy_key_is_shared(*source.add(i))
    }));
    let dest = crate::array::array_elements_ptr(out) as *mut u64;
    let written = available - lead;
    // GC_STORE_AUDIT(INIT): the fresh young tag-scanned result before its
    // length is published; no barrier.
    std::ptr::copy_nonoverlapping(source.add(lead), dest, written);
    (*out).length = written as u32;
    crate::array::fill_capacity_with_holes_from(out, written as u32);
    let n = written as u64;
    if n == 0 || n >= 1 << 30 {
        0
    } else {
        FACT_SET | FACT_TABLE | (n << 32) | lead as u64
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn copy_own_keys(
    obj: *const ObjectHeader,
    keys: crate::object::ObjectKeys,
    t: *mut crate::object::overflow::Table,
    out: *mut ArrayHeader,
    count: u32,
    holes: bool,
    non_enumerable: bool,
    verbatim: bool,
    young: bool,
) -> Option<u64> {
    // Read once: every optional-key hole test below compares against it.
    let cap = if holes {
        crate::object::object_inline_capacity(obj)
    } else {
        0
    };
    use crate::object::key_attrs as ka;
    let mut written = 0;
    let mut base: u64 = 0;
    // `written` at the first skip that ends the in-place prefix.
    let mut prefix_end: Option<usize> = None;
    let complete = 'copy: {
        let arr = keys.arr() as *const ArrayHeader;
        let (source, available) = if t.is_null() {
            let (s, n) = keys.dense_slots();
            if n == 0 {
                (std::ptr::null(), 0)
            } else {
                (s.add(0) as *const u64, n.min(count as usize))
            }
        } else {
            (
                crate::object::overflow::keys_ptr(t) as *const u64,
                ((*t).length as usize).min(count as usize),
            )
        };
        let table = !t.is_null();
        let attributed = holes || non_enumerable;
        // A layout's attribute words, read once: each word holds its
        // position's entry byte and its storage slot (`list_storage_slot`).
        let (words, word_count) = if attributed && !table {
            ka::storage_slot_words(arr)
        } else {
            (std::ptr::null(), 0)
        };
        let dest = crate::array::array_elements_ptr(out) as *mut u64;
        // A layout list of string keys only (`verbatim`: its record proves
        // no symbol) with no attributed key is copied as one block: every
        // key word is already shared (P1, `copy_key_is_shared`).
        if verbatim && !attributed && !source.is_null() {
            debug_assert!((0..available).all(|i| copy_key_is_shared(*source.add(i))));
            std::ptr::copy_nonoverlapping(source, dest, available);
            written = available;
        }
        // A table's names are decoded only when its summary says one may be
        // hidden (`key_attrs::TABLE_KEY_HIDDEN`).
        let decode_names = table && (*t).summary & ka::TABLE_KEY_HIDDEN != 0;
        let from = written;
        for i in from..available {
            let key = *source.add(i);
            // Symbols and tombstoned table entries are not string keys. The
            // tag (`TAG_MASK` is the top 16 bits) is compared as a 16-bit
            // value, so no 64-bit mask or tag constant is live in this loop.
            let tag = key >> 48;
            debug_assert_eq!(
                tag == crate::value::STRING_TAG >> 48
                    || tag == crate::value::SHORT_STRING_TAG >> 48,
                JSValue::from_bits(key).is_any_string()
            );
            if tag != crate::value::STRING_TAG >> 48 && tag != crate::value::SHORT_STRING_TAG >> 48
            {
                if prefix_end.is_none() {
                    if table && written == 0 {
                        base += 1;
                    } else {
                        prefix_end = Some(written);
                    }
                }
                continue;
            }
            // A table entry's name facts are read here, not at its append.
            if decode_names && ka::key_name_bits(key) != 0 {
                break 'copy None;
            }
            if attributed {
                let pos = i as u32;
                let (entry, slot) = if table || words.is_null() {
                    (ka::keys_entry(arr, pos), pos)
                } else if i < word_count {
                    let w = *words.add(i);
                    (w as u8, ka::slot_of_word(w, pos))
                } else {
                    (0, pos)
                };
                if non_enumerable && entry & ka::ENTRY_NON_ENUMERABLE != 0 {
                    prefix_end.get_or_insert(written);
                    continue;
                }
                // An optional key whose slot holds the hole is absent
                // (`own_slot_hidden`).
                if holes && entry & ka::ENTRY_OPTIONAL != 0 {
                    let absent = if table {
                        crate::object::key_view::position_is_hole(obj, keys, pos)
                    } else {
                        pos < keys.count()
                            && crate::object::key_view::slot_holds_hole_within(obj, slot, cap)
                    };
                    if absent {
                        prefix_end.get_or_insert(written);
                        continue;
                    }
                }
            }
            debug_assert!(copy_key_is_shared(key));
            // GC_STORE_AUDIT(INIT): the fresh tag-scanned result before its
            // length is published; an old-generation birth gets the slot
            // barriers below.
            dest.add(written).write(key);
            written += 1;
        }
        (*out).length = written as u32;
        if !young && crate::arena::pointer_in_old_gen(out as usize) {
            for i in 0..written {
                let slot = dest.add(i);
                crate::gc::runtime_write_barrier_slot(out as usize, slot as usize, *slot);
            }
        }
        let n = prefix_end.unwrap_or(written) as u64;
        let fact = if n == 0 || n >= 1 << 30 {
            0
        } else if table {
            FACT_SET | FACT_TABLE | (n << 32) | base
        } else {
            let word = (*obj).parent_class_id;
            if crate::object::shapes::is_layout_word(word) {
                FACT_SET | (n << 32) | word as u64
            } else {
                0
            }
        };
        Some(fact)
    };
    if young {
        let from = if complete.is_some() { written } else { 0 };
        crate::array::fill_capacity_with_holes_from(out, from as u32);
    }
    complete
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objops_keys_copy_the_shape_prefix_and_refuse_non_enumerable_or_dictionary() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key =
                |name: &[u8]| crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let proof = crate::object::canonical_keys::SharedLayout::shape_cache_entry();
            let first = crate::object::canonical_keys::extend_key(
                &proof,
                crate::object::canonical_keys::CanonicalKeys::EMPTY,
                key(b"objops_long_first"),
            );
            let second = crate::object::canonical_keys::extend_key(
                &proof,
                first,
                key(b"objops_long_second"),
            );
            assert_eq!(
                first.as_ptr(),
                second.as_ptr(),
                "live shared-backing premise"
            );
            assert_eq!(crate::array::js_array_length(first.as_ptr()), 2);
            crate::object::declarations::test_declare_anonymous(0x12345);
            let obj = crate::object::js_object_alloc(0x12345, 4);
            crate::object::set_object_keys_with_live(obj, first.view(), 4);
            let result = own_enumerable_keys(obj).expect("ordinary shape must take the bulk copy");
            assert_ne!(result, first.as_ptr());
            assert_eq!(crate::array::js_array_length(result), 1);
            assert_eq!(
                crate::array::js_array_get(result, 0).bits(),
                first.view().get(0).bits()
            );
            // Mutating a fresh result cannot mutate the shape's key storage.
            crate::array::js_array_push_f64(result, 42.0);
            assert_eq!(crate::object::object_keys(obj).count(), 1);
            crate::object::set_property_attrs(
                obj as usize,
                "objops_long_first".into(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
            assert!(
                own_enumerable_keys(obj).is_none(),
                "non-enumerable must retain filtering"
            );
            let native = crate::object::js_object_alloc(crate::wasi::CLASS_ID_WASI, 4);
            assert!(
                own_enumerable_keys(native).is_none(),
                "native DEFAULT retains dispatch"
            );
            let class = crate::object::js_object_alloc(0x23456, 4);
            assert_eq!(
                crate::array::js_array_length(
                    own_enumerable_keys(class).expect("class storage reads its layout")
                ),
                0
            );
            let dict = crate::object::js_object_alloc(0, 4);
            crate::object::js_object_set_field_by_name(dict, key(b"dict_key"), 1.0);
            assert!(crate::object::overflow::establish_order(dict));
            assert_eq!(
                crate::array::js_array_length(
                    own_enumerable_keys(dict).expect("a table lists its entries")
                ),
                1
            );
        }
    }
}
