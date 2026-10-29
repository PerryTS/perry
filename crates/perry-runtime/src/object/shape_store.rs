//! Named [[Set]] answered by the receiver shape, independent of a store site.

use super::{shapes, ObjectHeader};

/// Traced Array/Function named-property holder; ordinary-band ids already prove ObjectHeaders.
pub(crate) unsafe fn named_bag(
    receiver: *const ObjectHeader,
    key_bits: u64,
) -> Option<*const ObjectHeader> {
    let header = crate::value::addr_class::try_read_gc_header(receiver as usize)?;
    let bag = match header.obj_type {
        crate::gc::GC_TYPE_ARRAY => {
            let bag = crate::array::array_property_bag(receiver.cast());
            if bag.is_null() {
                return None;
            }
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let bytes = crate::string::js_string_key_bytes(
                crate::JSValue::from_bits(key_bits),
                &mut scratch,
            )?;
            let name = std::str::from_utf8(bytes).ok()?;
            if name == "length" || super::canonical_array_index(name).is_some() {
                return None;
            }
            bag
        }
        crate::gc::GC_TYPE_CLOSURE => {
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
                return None;
            }
            (*(receiver as *const crate::closure::ClosureHeader)).props
        }
        _ => return None,
    };
    (!bag.is_null()).then_some(bag.cast_const())
}

/// A POINTER-tag receiver's ordinary stamp proves kind (#10828 rule 3).
/// Forwarding writes a high half below the band; the slab needs no allocator classification.
#[inline(always)]
unsafe fn ordinary_receiver_record(obj: *const ObjectHeader) -> Option<shapes::ShapeRecordRef> {
    if !crate::value::addr_class::is_plausible_heap_addr(obj as usize)
        || !(obj as usize).is_multiple_of(std::mem::align_of::<crate::gc::GcHeader>())
    {
        return None;
    }
    let id = shapes::object_shape_stamp(obj);
    if id.0.wrapping_sub(shapes::SHAPE_ID_BASE) >= (2 << crate::codegen_abi::LAYOUT_BAND_SHIFT) {
        return None;
    }
    shapes::shape_record_for_word(id)
}

/// The store twin of the read front's positional shape answer. The receiver
/// kind is a shape fact; integrity changes retire the id or its key attributes.
pub(crate) unsafe fn try_own_store(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    if super::overflow::has_overflow(obj) {
        return false;
    }

    let record = if let Some(record) = ordinary_receiver_record(obj) {
        record
    } else {
        let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        if header.obj_type == crate::gc::GC_TYPE_CLOSURE {
            if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
                || header._reserved & crate::gc::OBJ_FLAG_FROZEN != 0
                || crate::closure::shape::is_class_info(
                    (*(obj as *const crate::closure::ClosureHeader)).info,
                )
            {
                return false;
            }
            return named_bag(obj, crate::JSValue::string_ptr(key as *mut _).bits())
                .is_some_and(|bag| try_own_store(bag.cast_mut(), key, value));
        }
        if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            let arr = crate::array::clean_arr_ptr_mut(obj.cast());
            if crate::array::array_object_flags_resolved(arr) & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return false;
            }
            // Array keys are classified once. An existing dense index is
            // an own data binding too; the shared strict dense setter proves
            // descriptors, integrity, presence and the value barrier. A
            // refusal still needs ordinary Set (holes, growth or accessors).
            let Some(name) = crate::string::header_str_checked(key) else {
                return false;
            };
            if let Some(index) = super::canonical_array_index(name) {
                return crate::array::try_strict_dense_index_set(arr, index, value).is_some();
            }
            if name == "length" {
                return false;
            }
            let bag = crate::array::array_property_bag(arr);
            return !bag.is_null() && try_own_store(bag, key, value);
        }
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        let Some(record) = shapes::object_shape_record(obj) else {
            return false;
        };
        record
    };
    match record_own_overwrite(obj, record, key, value) {
        Some(answer) => answer,
        None => super::try_existing_own_data_overwrite(obj, key, value),
    }
}

/// Where a keyed store's receiver lists the own keys that may answer the
/// store as an overwrite ([`own_overwrite_holder`]).
#[derive(Clone, Copy)]
pub(crate) enum OwnOverwrite {
    /// An ordinary layout with at least one key and no overflow: its shape
    /// record names every own key and slot.
    Record(shapes::ShapeRecordRef),
    /// An overflowed receiver: its overflow lists every own key (§3.4).
    /// [`try_overflow_own_overwrite`] admits the receiver.
    Overflow,
}

/// The holder of the own keys that may answer a keyed store as an
/// overwrite. Object facts only, read before the key is resolved, so a
/// store to a fresh keyless object pays nothing more.
///
/// # Safety
/// `obj` is a live object word.
#[inline(always)]
pub(crate) unsafe fn own_overwrite_holder(obj: *mut ObjectHeader) -> Option<OwnOverwrite> {
    if super::overflow::has_overflow(obj) {
        return Some(OwnOverwrite::Overflow);
    }
    let record = ordinary_receiver_record(obj)?;
    (record.logical_key_count() != 0).then_some(OwnOverwrite::Record(record))
}

/// A keyed store `obj[key] = value` on an [`OwnOverwrite::Overflow`]
/// receiver. The key is named by its property atom when that costs no table
/// work beyond an SSO key's one probe: a heap key that is itself a property
/// atom, or an SSO key with an existing atom. A heap string that is not an
/// atom (a computed copy) is left to the complete path, which interns it,
/// so an add of such a key pays no extra hashing here. The receiver is the
/// dynamic add's (`try_dynamic_store`): ordinary, extensible, without own
/// descriptors. An overflow entry holding `key` as a plain writable data
/// value is [[Set]]'s answer whatever the chain holds: the entry store,
/// `Ok`. `Err` carries the key the caller continues with (the atom once
/// resolved: the same property, so the steps after this do not resolve it
/// again).
///
/// # Safety
/// `obj` is a live, overflowed object word; nothing here allocates.
#[inline(always)]
pub(crate) unsafe fn try_overflow_own_overwrite(
    obj: *mut ObjectHeader,
    key: f64,
    value: f64,
) -> Result<f64, f64> {
    let k = crate::JSValue::from_bits(key.to_bits());
    let atom = if k.is_string() {
        let p = k.as_string_ptr() as *const crate::StringHeader;
        if !crate::string::is_property_atom(p) {
            return Err(key);
        }
        p
    } else if k.is_short_string() {
        match crate::string::existing_property_atom_of_value(key.to_bits()) {
            Some(atom) => atom,
            None => return Err(key),
        }
    } else {
        return Err(key);
    };
    let resolved = crate::value::js_nanbox_string(atom as i64);
    // The dynamic add's admission: no private name and no canonical index.
    if (*atom).byte_len == 0 || {
        let first = *crate::string::string_data(atom);
        first == b'#' || first.is_ascii_digit()
    } {
        return Err(resolved);
    }
    // The entry first: an add (no entry) declines before the receiver's
    // admission is read.
    let Some(entry) = super::overflow::own_plain_value_entry(obj, atom) else {
        return Err(resolved);
    };
    if dynamic_store_receiver(obj).is_none() {
        return Err(resolved);
    }
    super::overflow::store_own_value_entry(obj, entry, value);
    Ok(value)
}

/// A keyed store `obj[key] = value` whose receiver's shape record (from
/// [`own_overwrite_holder`]) proves `key` an own writable data slot: the slot
/// store, before any add-path work (family layout, migration, tombstone
/// re-add, overflow, transition). `false` leaves the store to the caller's
/// complete path, unchanged.
///
/// # Safety
/// `obj` is a live object word; `key` is an interned property key.
pub(crate) unsafe fn try_record_own_overwrite(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    record_own_overwrite(obj, record, key, value) == Some(true)
}

/// The shape record's own-slot answer for a store: `Some(true)` stored,
/// `Some(false)` the record proves the store is not a plain own overwrite,
/// `None` the record cannot tell (the caller asks the object).
#[inline(always)]
unsafe fn record_own_overwrite(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<bool> {
    if !record.object_kind().is_ordinary_layout() {
        return Some(false);
    }
    if record.object_kind() == shapes::ShapeObjectKind::Ordinary {
        if let Some(slot) = record.own_store_slot(key) {
            let Some(slot) = slot else {
                // Shape-proven absence cannot become an overwrite on another own-key lookup.
                return Some(false);
            };
            // Attributes belong to the matched binding. An accessor,
            // readonly intrinsic or private slot elsewhere on this shape
            // cannot invalidate an ordinary writable property here.
            if record.summary()
                & (super::key_attrs::SUMMARY_BLOCKS_OVERWRITE | super::key_attrs::SUMMARY_PRIVATE)
                != 0
                && !super::key_attrs::entry_is_plain_overwrite(super::key_attrs::keys_entry(
                    record.keys() as usize as *const crate::ArrayHeader,
                    slot as u32,
                ))
            {
                return Some(false);
            }
            let slot = super::key_attrs::key_storage_slot(
                record.keys() as usize as *const crate::ArrayHeader,
                slot as u32,
            ) as usize;
            if store_proven_own_slot(obj, record, slot, value) {
                return Some(true);
            }
        }
    }
    None
}

/// Proven writable ordinary own slot: shared representation/barrier funnel for Set and Define.
#[inline(always)]
pub(crate) unsafe fn store_proven_own_slot(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    slot: usize,
    value: f64,
) -> bool {
    let bits = if value.to_bits() == crate::value::POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    if slot < record.live_inline_slot_count() as usize {
        // The two inline stores below bypass `store_object_field_slot`, so
        // they report to the sealed-prototype hook themselves (one meta load
        // and flag test): an own-key overwrite of `%Array.prototype%`'s
        // `join` must set its shadow bit like any other slot store.
        super::builtin_proto::note_value_store(obj, slot, false, "own store");
        let rep = super::field_rep::slot_rep(record.rep(), slot as u32);
        let address = obj
            .cast::<u8>()
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>()
            .add(slot);
        if bits & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000
            && rep != super::field_rep::REP_SPECIAL
        {
            // GC_STORE_AUDIT(POINTER_FREE): a canonical finite Number.
            address.write(bits);
            return true;
        }
        if rep == super::field_rep::REP_ANY {
            // Existing named-Set barrier: pointer traffic needs no extra slow call.
            crate::gc::runtime_store_jsvalue_slot_layout_deferred(
                obj as usize,
                address as usize,
                slot,
                bits,
            );
            return true;
        }
    }
    store_proven_own_slot_slow(obj, record, slot, f64::from_bits(bits))
}

#[cold]
#[inline(never)]
unsafe fn store_proven_own_slot_slow(
    obj: *mut ObjectHeader,
    record: shapes::ShapeRecordRef,
    slot: usize,
    value: f64,
) -> bool {
    let bits = if value.to_bits() == crate::value::POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        value.to_bits()
    };
    if slot < record.live_inline_slot_count() as usize {
        super::store_object_field_slot(obj, slot, bits);
        return true;
    }
    if super::spill_store_would_be_in_capacity(obj as usize, slot) {
        // Existing physical storage makes the ordinary spill barrier a nonmoving leaf.
        super::spill::spill_set(obj as usize, slot, bits);
        return true;
    }
    false
}

/// Can an inherited setter, accessor or non-writable property block adding
/// `key` to `obj`? Null and realm-default chains have an authoritative
/// answer. Known holders and declared class chains answer from their shapes;
/// unknown chains use the generic operation. No chain answer is cached.
#[inline]
unsafe fn inherited_store_blocked(
    record: shapes::ShapeRecordRef,
    obj: *const ObjectHeader,
    key: f64,
    bytes: &[u8],
) -> bool {
    if let Some(blocked) = super::descriptor_state::layout_store_may_intercept(record, obj, bytes) {
        return blocked;
    }
    // Unknown chains use the generic operation. The caller holds an unrooted
    // receiver, so suppress collection over any materialization in that walk.
    let _no_move = crate::gc::GcSuppressScope::new();
    super::plain_data_write_may_intercept(obj as usize, (*obj).class_id, key)
}

/// A warm shape proof needs no outer PutValue scope or key conversion. The
/// existing transition funnel owns its roots and repairs declined operands.
pub(crate) unsafe fn try_warm_transition(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    let record = if let Some(record) = ordinary_receiver_record(obj) {
        record
    } else {
        let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
        if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            return try_warm_array_transition(obj.cast(), key, value, refresh);
        }
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        shapes::object_shape_record(obj)?
    };
    if !record.object_kind().is_ordinary_layout()
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
    {
        return None;
    }
    let bytes = crate::string::header_str_checked(key)?.as_bytes();
    if bytes == b"__proto__"
        || bytes
            .first()
            .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    // The layouts answer first (§3.7: the receiver's complete own list and
    // its prototypes' layouts); the shape-chain answer covers what they
    // cannot read.
    if super::shapes::layout_probe::layouts_admit_add(obj, key) != Some(true)
        && inherited_store_blocked(
            record,
            obj,
            crate::value::js_nanbox_string(key as i64),
            bytes,
        )
    {
        return None;
    }
    super::object_set_field_by_name_transition_chain_proven_value(obj, key, value, refresh)
}

/// A dynamic-key write (§3.3): a key the receiver's layout names is a slot
/// store, which the caller's ordinary path performs; any other key goes to
/// the receiver's overflow, which takes it with one append. `None` leaves the
/// write to the complete `[[Set]]` (an exotic, frozen, sealed or
/// descriptor-carrying receiver, or a key a prototype could intercept) and
/// guarantees nothing was allocated, so the caller's operands are current.
/// `key` is a heap or inline string.
pub(crate) unsafe fn try_dynamic_store(
    obj: *mut ObjectHeader,
    key: f64,
    value: f64,
) -> Option<f64> {
    let record = dynamic_store_receiver(obj)?;
    // The overflow takes dynamic keys of realm-default and null-prototype
    // bags, as step 4 landed it. A receiver with a class or recorded
    // prototype keeps its layout transitions until the unknown-receiver probe
    // (step 5) serves reads of overflowed objects: an overflowed object
    // primes no own-slot way, so routing those receivers here would send
    // every later read of them to the generic lookup (Effect +92%).
    // A record fact, so it is asked before the key is decoded.
    if !matches!(
        record.proto_id(),
        shapes::PROTO_ID_DEFAULT | shapes::PROTO_ID_NULL
    ) {
        return None;
    }
    let k = crate::JSValue::from_bits(key.to_bits());
    if !k.is_string() && !k.is_short_string() {
        return None;
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(k, &mut scratch)?;
    if bytes == b"__proto__"
        || bytes
            .first()
            .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    let keys = record.keys() as usize as *const crate::ArrayHeader;
    if !keys.is_null()
        && super::keys_find_property_slot_by_bytes(keys, record.logical_key_count(), bytes)
            .is_some()
    {
        return None;
    }
    if inherited_store_blocked(record, obj, key, bytes) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let atom = crate::string::property_atom_from_value(key.to_bits());
    // The layout does not hold the key (probed above): the write is the
    // overflow's existing entry or one append.
    super::overflow::store_unfixed_routed(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        (atom & crate::value::POINTER_MASK) as *const crate::StringHeader,
        value.get_nanbox_f64(),
    );
    Some(value.get_nanbox_f64())
}

/// The receiver a dynamic-key write may extend through its overflow: an
/// ordinary, extensible object with no own descriptors and no exotic surface.
unsafe fn dynamic_store_receiver(obj: *mut ObjectHeader) -> Option<shapes::ShapeRecordRef> {
    if !crate::value::addr_class::is_plausible_heap_addr(obj as usize)
        || !(obj as usize).is_multiple_of(std::mem::align_of::<crate::gc::GcHeader>())
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
    {
        return None;
    }
    let id = super::layout_desc::layout_id_of_word((*obj).parent_class_id);
    if id.wrapping_sub(shapes::SHAPE_ID_BASE) >= (2 << crate::codegen_abi::LAYOUT_BAND_SHIFT) {
        return None;
    }
    // A live object's own header word: the receiver carries its row.
    let record = shapes::shape_record_of_receiver(id)?;
    if !record.object_kind().is_ordinary_layout()
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
    {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags
                & (super::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | super::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return None;
    }
    Some(record)
}

/// A projected array bag already names its prototype chain. Reuse its
/// learned edge without repeating the collecting generic Set setup.
unsafe fn try_warm_array_transition(
    arr: *mut crate::ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
    refresh: &mut Option<(f64, f64, f64)>,
) -> Option<f64> {
    let arr = crate::array::clean_arr_ptr_mut(arr);
    if crate::array::array_object_flags_resolved(arr)
        & (crate::gc::GC_ARRAY_CUSTOM_PROTO
            | crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
    {
        return None;
    }
    let bytes = crate::string::header_str_checked(key)?.as_bytes();
    if bytes == b"length"
        || bytes == b"__proto__"
        || bytes
            .first()
            .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    let bag = crate::array::array_property_bag(arr);
    if bag.is_null() {
        return None;
    }
    let proto = crate::array::array_prototype_addr() as *mut crate::ArrayHeader;
    if proto.is_null()
        || arr == crate::array::clean_arr_ptr_mut(proto)
        || crate::array::array_object_flags_resolved(proto) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
    {
        return None;
    }
    let proto_bag = crate::array::array_property_bag(proto);
    // A freshly allocated bag still has Object.prototype. Its proof cannot
    // stand in for the outer array's chain until the projection is installed.
    if proto_bag.is_null()
        || super::prototype_chain::object_static_prototype(bag as usize)
            != Some(crate::value::js_nanbox_pointer(proto_bag as i64).to_bits())
    {
        return None;
    }
    if array_bag_chain_blocks(bag, bytes) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(arr);
    let key_root = scope.root_string_ptr(key);
    let value_root = scope.root_nanbox_f64(value);
    let stored =
        super::object_set_field_by_name_transition_chain_proven_value(bag, key, value, &mut None);
    // A declined append may have interned a key and collected. Repair the
    // outer receiver, rather than returning the bag as the receiver.
    *refresh = Some((
        crate::value::js_nanbox_pointer(owner.get_raw_mut_ptr::<crate::ArrayHeader>() as i64),
        crate::value::js_nanbox_string(key_root.get_raw_const_ptr::<crate::StringHeader>() as i64),
        value_root.get_nanbox_f64(),
    ));
    stored.map(|_| value_root.get_nanbox_f64())
}

/// Array named properties use the same shaped bag as customized descriptors.
/// Its prototype edge names the array chain; elements and length stay exotic.
unsafe fn try_array_store(
    target: f64,
    key: f64,
    value: f64,
    name: &str,
    may_move: &mut bool,
) -> Option<f64> {
    let key_ptr = (key.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    if name == "length"
        || name == "__proto__"
        || name.starts_with('#')
        || super::canonical_array_index(name).is_some()
    {
        return None;
    }
    let arr = crate::array::clean_arr_ptr_mut(
        (target.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader,
    );
    let flags = crate::array::array_object_flags_resolved(arr);
    if flags & (crate::gc::GC_ARRAY_CUSTOM_PROTO | crate::gc::OBJ_FLAG_FROZEN) != 0 {
        return None;
    }
    let bag = crate::array::array_property_bag(arr);
    let existing = if bag.is_null() {
        crate::array::array_named_property_has(arr, key_ptr)
    } else {
        super::obj_value_has_own_key(crate::value::js_nanbox_pointer(bag as i64), key)
    };
    if !existing && flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0 {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(target);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    let bag = crate::array::array_property_bag_ensure(arr);
    let bag_root = scope.root_raw_mut_ptr(bag);
    if bag.is_null() {
        return None;
    }
    // This internal holder has ordinary data/descriptor semantics; it is
    // not one of the runtime-born special objects the unmarked kind covers.
    if shapes::object_shape_record(bag)?.object_kind() == shapes::ShapeObjectKind::OrdinaryUnmarked
    {
        super::alloc_plain::mark_object_plain_ordinary(bag);
    }
    if !existing {
        // Array.prototype is itself an array. Its named-property bag is
        // the ordinary shape projection of this named-only chain. Marking
        // that bag makes subsequent named descriptor edits invalidate the
        // proof through the existing shape-stamp hook.
        let mut array_proto = crate::array::array_prototype_addr();
        if array_proto == 0 {
            let _ = super::js_get_global_this();
            array_proto = crate::array::array_prototype_addr();
        }
        if array_proto == 0 {
            return None;
        }
        let owner_arr = crate::array::clean_arr_ptr_mut(
            (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK)
                as *mut crate::ArrayHeader,
        );
        let array_proto = crate::array::clean_arr_ptr_mut(array_proto as *mut crate::ArrayHeader);
        let prototype = if owner_arr == array_proto {
            crate::value::js_nanbox_pointer(crate::array::object_prototype_addr() as i64)
        } else {
            crate::value::js_nanbox_pointer(array_proto as i64)
        };
        let proto_root = scope.root_nanbox_f64(prototype);
        let proto_addr = (prototype.to_bits() & crate::value::POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(proto_addr)?;
        let prototype = if header.obj_type == crate::gc::GC_TYPE_ARRAY {
            let proto_arr = crate::array::clean_arr_ptr_mut(proto_addr as *mut crate::ArrayHeader);
            if crate::array::array_object_flags_resolved(proto_arr)
                & crate::gc::GC_ARRAY_CUSTOM_PROTO
                != 0
            {
                return None;
            }
            let proto_bag = crate::array::array_property_bag_ensure(proto_arr);
            if proto_bag.is_null() {
                return None;
            }
            let proto_bag_root = scope.root_raw_mut_ptr(proto_bag);
            let object_proto_addr = crate::array::object_prototype_addr();
            if object_proto_addr == 0 {
                return None;
            }
            let object_proto = crate::value::js_nanbox_pointer(object_proto_addr as i64);
            if super::prototype_chain::object_static_prototype(proto_bag as usize)
                != Some(object_proto.to_bits())
            {
                // Internal construction of `%Array.prototype%`'s bag, not a
                // modification of it (#11915, `builtin_proto`).
                super::own_override::as_builtin_definition(|| {
                    super::prototype_chain::object_link_created_prototype(
                        proto_bag as usize,
                        object_proto.to_bits(),
                    )
                });
            }
            crate::value::js_nanbox_pointer(proto_bag_root.get_raw_mut_ptr::<ObjectHeader>() as i64)
        } else if header.obj_type == crate::gc::GC_TYPE_OBJECT {
            proto_root.get_nanbox_f64()
        } else {
            return None;
        };
        let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
        if super::prototype_chain::object_static_prototype(bag as usize)
            != Some(prototype.to_bits())
        {
            // Internal construction, not observable prototype surgery. The
            // new shape names the edge; no global plan invalidation is needed.
            super::prototype_chain::object_link_created_prototype(
                bag as usize,
                prototype.to_bits(),
            );
        }
    }
    let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
    let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
    let key_value = crate::value::js_nanbox_string(key as i64);
    let result = try_store_with_refresh(
        crate::value::js_nanbox_pointer(bag as i64),
        key_value,
        value_root.get_nanbox_f64(),
        may_move,
    );
    if result.is_some() {
        return Some(value_root.get_nanbox_f64());
    }
    if !existing {
        // The first transition has no edge yet. A proven chain can create it
        // through the ordinary bag store; later receivers reuse that edge.
        let bag = bag_root.get_raw_mut_ptr::<ObjectHeader>();
        let key = key_root.get_raw_const_ptr::<crate::StringHeader>();
        shapes::object_shape_record(bag)?;
        let bytes = crate::string::header_str_checked(key)?.as_bytes();
        if !array_bag_chain_blocks(bag, bytes) {
            super::object_ops::define_property_force_store_value(
                bag,
                key,
                value_root.get_nanbox_f64(),
            );
            return Some(value_root.get_nanbox_f64());
        }
    }
    None
}

#[cfg(test)]
#[path = "shape_store_tests.rs"]
mod tests;

/// No user code runs on a successful own store or transition. An unproven
/// chain, exotic receiver, or exceptional key retains the complete [[Set]].
#[cfg(test)]
pub(crate) unsafe fn try_store(target: f64, key: f64, value: f64) -> Option<f64> {
    try_store_with_refresh(target, key, value, &mut false)
}

/// Report whether this attempt entered an allocating path. A decline before
/// that point keeps the caller's already refreshed operands valid.
pub(crate) unsafe fn try_store_with_refresh(
    target: f64,
    key: f64,
    value: f64,
    may_move: &mut bool,
) -> Option<f64> {
    use crate::value::{POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};
    if target.to_bits() & TAG_MASK != POINTER_TAG || key.to_bits() & TAG_MASK != STRING_TAG {
        return None;
    }
    let key_ptr = (key.to_bits() & POINTER_MASK) as *const crate::StringHeader;
    let name = crate::string::header_str_checked(key_ptr)?;
    let bytes = name.as_bytes();
    if bytes
        .first()
        .is_some_and(|b| *b == b'#' || b.is_ascii_digit())
    {
        return None;
    }
    let addr = (target.to_bits() & POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        return try_array_store(target, key, value, name, may_move);
    }
    if header.obj_type == crate::gc::GC_TYPE_CLOSURE {
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
            return None;
        }
        return try_function_store(target, key, value, name, may_move);
    }
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = addr as *mut ObjectHeader;
    let shape = shapes::object_shape_descriptor(obj)?;
    if !shape.object_kind.is_ordinary_layout()
        || (*obj).class_id == super::NATIVE_MODULE_CLASS_ID
        || super::overflow::has_overflow(obj)
    {
        return None;
    }
    if super::try_existing_own_data_overwrite_by_content(obj, key, bytes, value) {
        return Some(value);
    }
    if header._reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
        != 0
        || !super::own_descriptors_skip_key(addr, key)
        || bytes == b"__proto__"
    {
        return None;
    }
    // A transition edge only exists for an absent own key. The prototype
    // fact is attached to its predecessor shape, never to the call site.
    let record = shape.record_ref()?;
    let blocked = inherited_store_blocked(
        record,
        obj,
        crate::value::js_nanbox_string(key_ptr as i64),
        bytes,
    );
    if blocked {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_root = scope.root_raw_mut_ptr(obj);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    apply_proven_add(&obj_root, &key_root, &value_root)
}

/// A proven absent key can mint its first edge through the same own-data
/// funnel that later transition hits reuse. A missing edge is not a reason
/// to repeat [[Set]]'s descriptor and prototype walk.
unsafe fn apply_proven_add(
    owner: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    value: &crate::gc::RuntimeHandle<'_>,
) -> Option<f64> {
    if super::object_set_field_by_name_transition_chain_proven_value(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_const_ptr::<crate::StringHeader>(),
        value.get_nanbox_f64(),
        &mut None,
    )
    .is_some()
    {
        return Some(value.get_nanbox_f64());
    }
    // A keys-only claim cannot publish the body-specific successor of a
    // permanent-image function. Its first add stays in the existing value
    // publication funnel; subsequent adds use that learned transition above.
    if super::field_rep_store::constfn_store_info(value.get_nanbox_f64().to_bits()).is_some() {
        return None;
    }
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    let key_ptr = key.get_raw_const_ptr::<crate::StringHeader>();
    let record = shapes::object_shape_record(obj)?;
    if record.object_kind() != shapes::ShapeObjectKind::Ordinary
        || record.summary() & super::key_attrs::SUMMARY_PRIVATE != 0
        || super::class_registry::is_class_object_ptr(obj.cast())
        || record.own_store_slot(key_ptr) != Some(None)
        || crate::value::addr_class::try_read_gc_header(obj as usize)?._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
    {
        return None;
    }
    // Absence and the complete ordered key count prove where this append
    // lands. Reuse the existing key-add funnel, then write that slot; the
    // general descriptor helper would scan the whole list again.
    let slot = record.logical_key_count() as usize;
    let predecessor = shapes::object_shape_stamp(obj);
    let predecessor_rep = record.rep();
    let number_lane = slot < super::field_rep::REP_SLOTS as usize
        && slot < super::object_inline_capacity(obj) as usize
        && super::field_rep::f64_slot_bits(value.get_nanbox_f64().to_bits()).is_some();
    if number_lane {
        // Publish the value-bearing edge directly, as the named-store tail
        // does. A keys-only Any intermediary would incorrectly count as a
        // non-Number sibling and deprecate this new F64 lane at convergence.
        let atom = crate::string::js_string_intern(key_ptr, super::key_content_hash(key_ptr));
        if atom.is_null() {
            return None;
        }
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        let proof = super::canonical_keys::SharedLayout::of_receiver(obj)?;
        let keys = super::object_keys(obj);
        let parent = super::canonical_keys::canonical_prefix(&proof, keys, (slot as u32).min(keys.count()));
        let next = super::canonical_keys::extend_key(
            &proof,
            parent,
            key.get_raw_const_ptr::<crate::StringHeader>(),
        );
        super::field_rep_store::publish_key_add_edge(
            owner.get_raw_mut_ptr::<ObjectHeader>(),
            next.view(),
            predecessor_rep,
            slot as u32,
            Some(value.get_nanbox_f64().to_bits()),
            true,
        );
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        if super::overflow::has_overflow(obj) {
            return None;
        }
        super::js_object_set_field(
            obj,
            slot as u32,
            crate::JSValue::from_bits(value.get_nanbox_f64().to_bits()),
        );
        let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
        if let Some(atom) = super::store_key::interned_key_for_store(
            crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64),
        ) {
            super::transition_cache_insert(
                std::ptr::null(),
                predecessor,
                atom,
                super::object_keys(obj).arr() as usize,
                slot as u32,
                shapes::object_shape_stamp(obj),
            );
        }
        return Some(value.get_nanbox_f64());
    }
    super::object_ops::ensure_key_in_keys_array_for_value(
        owner.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_const_ptr::<crate::StringHeader>(),
        value.get_nanbox_f64(),
    );
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    // Appending can transfer the list to the receiver's dictionary. That
    // store no longer has a shape-owned slot; the complete setter writes it.
    if super::overflow::has_overflow(obj) {
        return None;
    }
    let obj = owner.get_raw_mut_ptr::<ObjectHeader>();
    let bits = value.get_nanbox_f64().to_bits();
    if slot < super::object_inline_capacity(obj) as usize {
        super::js_object_set_field(obj, slot as u32, crate::JSValue::from_bits(bits));
    } else {
        super::overflow_set(obj as usize, slot, bits);
    }
    Some(value.get_nanbox_f64())
}

/// Array named properties use the same shaped bag as customized descriptors.
/// Its prototype edge names the array chain; elements and length stay exotic.
unsafe fn try_function_store(
    target: f64,
    key: f64,
    value: f64,
    name: &str,
    may_move: &mut bool,
) -> Option<f64> {
    let ptr = (target.to_bits() & crate::value::POINTER_MASK) as usize;
    let key_ptr = (key.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    // Intrinsic function keys retain their own descriptor/materialization rules.
    if matches!(
        name,
        "name" | "length" | "prototype" | "caller" | "arguments" | "__proto__"
    ) || name.starts_with('#')
        || super::canonical_array_index(name).is_some()
    {
        return None;
    }
    let closure = ptr as *const crate::closure::ClosureHeader;
    let descriptor = shapes::shape_descriptor_of_receiver(((*closure).shape_id).0)?;
    if crate::closure::shape::is_class_info((*closure).info) {
        return None;
    }
    let flags = crate::value::addr_class::try_read_gc_header(ptr)?._reserved;
    if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return None;
    }
    let bag = (*closure).props;
    if !bag.is_null() && crate::closure::props::bag_has_own(ptr, name.as_bytes()) {
        return super::try_existing_own_data_overwrite_by_content(bag, key, name.as_bytes(), value)
            .then_some(value);
    }
    if !matches!(
        descriptor.object_kind,
        shapes::ShapeObjectKind::Function | shapes::ShapeObjectKind::FunctionDictionary
    ) || flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0
    {
        return None;
    }
    *may_move = true;
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(target);
    let key_root = scope.root_string_ptr(key_ptr);
    let value_root = scope.root_nanbox_f64(value);
    let prototype =
        scope.root_nanbox_f64(super::js_object_get_prototype_of(owner.get_nanbox_f64()));
    if prototype.get_nanbox_f64().to_bits() != crate::value::TAG_NULL {
        let bits = prototype.get_nanbox_f64().to_bits();
        if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return None;
        }
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let proto = addr as *const ObjectHeader;
        let record = shapes::object_shape_record(proto)?;
        if !record.object_kind().is_ordinary_layout() {
            return None;
        }
        let name =
            crate::string::header_str_checked(key_root.get_raw_const_ptr::<crate::StringHeader>())?;
        // A FunctionDictionary's prototype is per-object, so its shared
        // ShapeId cannot authorize a chain. Ask the actual prototype shape's
        // own attributes plus its inherited-chain fact. The named bag retains
        // its null prototype for all existing DefineOwnProperty writers.
        if super::key_attrs::object_key_blocks_plain_store(proto, name.as_bytes()) {
            return None;
        }
        // The prototype's own attributes were answered above; walk the rest.
        if chain_blocks(prototype.get_nanbox_f64(), name.as_bytes()) {
            return None;
        }
    }
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    let bag = crate::closure::props::bag_ensure(ptr);
    let bag_root = scope.root_raw_mut_ptr(bag);
    if shapes::object_shape_record(bag)?.object_kind() == shapes::ShapeObjectKind::OrdinaryUnmarked
    {
        super::alloc_plain::mark_object_plain_ordinary(bag);
    }
    let key_ptr = key_root.get_raw_const_ptr::<crate::StringHeader>();
    let mut refresh = None;
    if super::object_set_field_by_name_transition_chain_proven_value(
        bag,
        key_ptr,
        value_root.get_nanbox_f64(),
        &mut refresh,
    )
    .is_none()
    {
        super::object_ops::define_property_force_store_value(
            bag_root.get_raw_mut_ptr::<ObjectHeader>(),
            key_root.get_raw_const_ptr::<crate::StringHeader>(),
            value_root.get_nanbox_f64(),
        );
    }
    let ptr = (owner.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    // The ordinary function writer also revives a deleted configurable key.
    // The bag's new data binding and the function's deletion state must agree.
    let name =
        crate::string::header_str_checked(key_root.get_raw_const_ptr::<crate::StringHeader>())?;
    crate::closure::props::state_clear_deleted(ptr, name);
    crate::closure::shape::refresh_closure_shape(ptr);
    Some(value_root.get_nanbox_f64())
}

/// Prototype key summaries are the proof: ordinary writable data cannot
/// intercept a named add. Unknown kinds and Proxy hops keep the full Set.
/// May the chain of an array's named-property `bag` intercept a store of
/// `key`? The bag inherits from the `%Array.prototype%` bag, then
/// `%Object.prototype%`. While the sealed Array intrinsic's layout word is
/// unshadowed, that bag still holds exactly the runtime's definitions (all
/// writable data) and its own link; `%Object.prototype%` answers from its
/// own word. Anything else walks the chain.
unsafe fn array_bag_chain_blocks(bag: *mut ObjectHeader, key: &[u8]) -> bool {
    let array =
        crate::agent_ptrs::intrinsic_proto(super::builtin_proto::BuiltinProto::Array as usize);
    let word = (*array).parent_class_id;
    if word >= shapes::SHAPE_ID_BASE
        && word < super::layout_desc::UNSHADOWED_WORD_END
        && super::prototype_chain::object_static_prototype(bag as usize)
            == Some(crate::value::js_nanbox_pointer(array as i64).to_bits())
    {
        return super::descriptor_state::object_proto_may_intercept_name(key);
    }
    chain_blocks(crate::value::js_nanbox_pointer(bag as i64), key)
}

pub(crate) unsafe fn chain_blocks(receiver: f64, key: &[u8]) -> bool {
    let mut hop = super::js_object_get_prototype_of(receiver);
    for _ in 0..64 {
        if hop.to_bits() == crate::value::TAG_NULL {
            return false;
        }
        if hop.to_bits() & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return true;
        }
        let addr = (hop.to_bits() & crate::value::POINTER_MASK) as usize;
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return true;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return true;
        }
        let obj = addr as *const ObjectHeader;
        let Some(shape) = shapes::object_shape_descriptor(obj) else {
            return true;
        };
        if !shape.object_kind.is_ordinary_layout() {
            return true;
        }
        // A hop with an overflow keeps its keys' attributes in its entries,
        // not in its layout's summary (object model step 4).
        if (shape.summary & super::key_attrs::SUMMARY_BLOCKS_STORE != 0
            || super::overflow::has_overflow(obj))
            && super::key_attrs::object_key_blocks_plain_store(obj, key)
        {
            return true;
        }
        hop = super::js_object_get_prototype_of(hop);
    }
    true
}
