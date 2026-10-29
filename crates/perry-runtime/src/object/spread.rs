//! Fresh literal spreads: one shape transition and one positional copy.
//!
//! The existing weak transition table owns the learned edge. Namespace 255
//! names a SOURCE ShapeId rather than a string; no new table or heap-pointer
//! owner exists. Cold construction uses the canonical key-list authority.
use super::header::meta_form_alloc_bytes;
use super::*;
use crate::codegen_abi::LayoutWord;

pub(super) const SPREAD_TRANSITION_NAMESPACE: u32 = 255;
const NUMBER_COPY: u32 = 1 << 23;
const SOURCE_INLINE_MASK: u32 = NUMBER_COPY - 1;

/// A POINTER value carrying an ordinary-band shape is an ObjectHeader (rule
/// 3). The slab record supplies both layout and the CopyDataProperties proof.
#[inline]
unsafe fn ordinary_shape(value: f64) -> Option<(*const ObjectHeader, u32)> {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr)
        || !addr.is_multiple_of(std::mem::align_of::<crate::gc::GcHeader>())
    {
        return None;
    }
    let object = addr as *const ObjectHeader;
    let id = (*object).shape_word();
    if id.wrapping_sub(shapes::SHAPE_ID_BASE) >= 2 << crate::codegen_abi::LAYOUT_BAND_SHIFT {
        return None;
    }
    Some((object, id))
}

#[inline(never)]
#[optimize(size)]
unsafe fn source_record(value: f64) -> Option<(*const ObjectHeader, u32, shapes::ShapeRecordRef)> {
    let (object, id) = ordinary_shape(value)?;
    let record = shapes::shape_record_for_word(LayoutWord(id))?;
    record.spread_data().then_some((object, id, record))
}

#[inline]
fn spread_key(source: u32) -> usize {
    // The full process-stable id fits both ILP32 and LP64.
    source as usize
}

#[inline]
pub(super) fn cache_slot(prefix: LayoutWord, source: u32) -> usize {
    transition_cache_slot(prefix, spread_key(source), SPREAD_TRANSITION_NAMESPACE)
}

#[inline]
fn lookup(prefix: LayoutWord, source: u32) -> Option<TransitionEntry> {
    let kid = spread_key(source);
    let entry = with_transition_cache(|t| unsafe { (*t)[cache_slot(prefix, source)] });
    (entry.next_keys != 0
        && entry.prev_shape_id == prefix
        && entry.key_ptr == kid
        && entry.slot_idx >> 24 == SPREAD_TRANSITION_NAMESPACE)
        .then_some(entry)
}

/// Used by the existing carrier/prune walks; a source id is weak metadata,
/// never a string address to be rewritten by the collector.
pub(super) fn source_shape(entry: &TransitionEntry) -> Option<LayoutWord> {
    (entry.slot_idx >> 24 == SPREAD_TRANSITION_NAMESPACE)
        .then_some(LayoutWord(entry.key_ptr as u32))
}

fn learn(
    prefix: LayoutWord,
    source: LayoutWord,
    source_inline: u32,
    shape: LayoutWord,
    keys: ObjectKeys,
    numbers: bool,
) {
    if source_inline > SOURCE_INLINE_MASK {
        return;
    }
    let kid = spread_key(source.0);
    let slot = cache_slot(prefix, source.0);
    arm_transition_cache_young(slot, keys.arr() as usize, kid, SPREAD_TRANSITION_NAMESPACE);
    with_transition_cache(|table| unsafe {
        // GC_STORE_AUDIT(ROOT): existing weak transition edge and young log.
        (*table)[slot] = TransitionEntry {
            key_ptr: kid,
            next_keys: keys.arr() as usize,
            prev_shape_id: prefix,
            target_shape_id: shape,
            slot_idx: (SPREAD_TRANSITION_NAMESPACE << 24)
                | source_inline
                | if numbers { NUMBER_COPY } else { 0 },
            target_len: keys.count(),
        };
    });
    // The prefix and source are the lookup key (weak, death-pruned); only the
    // target is owned, as for every transition entry.
    shape_carriers::note_shape_id(shape);
}

#[inline]
unsafe fn fields(object: *const ObjectHeader) -> *const u64 {
    object
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast()
}

#[inline]
unsafe fn copy_value(object: *mut ObjectHeader, slot: usize, bits: u64) {
    let address = fields(object).cast_mut().add(slot);
    if bits & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000 {
        // GC_STORE_AUDIT(POINTER_FREE): a finite Number in fresh storage.
        address.write(bits);
        return;
    }
    let tag = bits & crate::value::TAG_MASK;
    if tag == crate::value::POINTER_TAG
        || tag == crate::value::STRING_TAG
        || tag == crate::value::BIGINT_TAG
    {
        // Allocate-black and old births still shade/remember their edges.
        crate::gc::runtime_store_jsvalue_slot_layout_deferred(
            object as usize,
            address as usize,
            slot,
            bits,
        );
    } else {
        // GC_STORE_AUDIT(INIT): pointer-free value in fresh object storage.
        address.write(bits);
    }
}

#[inline(never)]
#[optimize(size)]
unsafe fn copy_slots(
    destination: *mut ObjectHeader,
    prefix: Option<(*const ObjectHeader, shapes::ShapeRecordRef)>,
    source: *const ObjectHeader,
    record: shapes::ShapeRecordRef,
    result: Option<shapes::ShapeRecordRef>,
    source_keys: *const ArrayHeader,
) {
    let prefix_count = prefix.map_or(0, |(_, r)| r.logical_key_count());
    if let Some((object, r)) = prefix {
        key_attrs::debug_assert_positional(r.keys() as usize as *const ArrayHeader, prefix_count);
        if let Some(result) = result {
            key_attrs::debug_assert_positional(
                result.keys() as usize as *const ArrayHeader,
                result.logical_key_count(),
            );
        }
        for slot in 0..prefix_count as usize {
            let bits = if slot < r.live_inline_slot_count() as usize {
                *fields(object).add(slot)
            } else {
                js_object_get_field(object, slot as u32).bits()
            };
            copy_value(destination, slot, bits);
        }
    }
    if prefix_count == 0 {
        copy_source_slots(
            destination,
            source,
            record.logical_key_count(),
            record.live_inline_slot_count(),
            source_keys,
        );
        return;
    }
    let keys = record.keys() as usize as *const ArrayHeader;
    let key_words = if prefix_count == 0 || record.logical_key_count() == 0 {
        ptr::null()
    } else {
        crate::array::array_elements_ptr(keys)
    };
    for slot in 0..record.logical_key_count() as usize {
        let target = if prefix_count == 0 {
            slot
        } else {
            let key = JSValue::from_bits(*key_words.add(slot));
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let bytes = crate::string::js_string_key_bytes(key, &mut scratch)
                .expect("spread shape key is a string");
            result
                .expect("a nonempty prefix resolves the result record")
                .own_data_position_of_value(key.bits(), bytes)
                .flatten()
                .expect("combined shape contains every source key") as usize
        };
        let slot = source_slot(source_keys, slot);
        let bits = if slot < record.live_inline_slot_count() as usize {
            *fields(source).add(slot)
        } else {
            js_object_get_field(source, slot as u32).bits()
        };
        copy_value(destination, target, bits);
    }
}

#[inline(never)]
#[optimize(size)]
/// The storage slot of source key `position`. `source_keys` is null when the
/// destination's slots are the source's (a shared list, or no own slots).
unsafe fn source_slot(source_keys: *const ArrayHeader, position: usize) -> usize {
    if source_keys.is_null() {
        position
    } else {
        key_attrs::key_storage_slot(source_keys, position as u32) as usize
    }
}

/// Debug invariant of a learned edge: it copies slot for slot, so the source
/// keeps the result's storage slots (one shared list) or none of its own.
#[inline(always)]
unsafe fn debug_assert_edge(source: *const ObjectHeader, target_shape: LayoutWord) {
    #[cfg(debug_assertions)]
    {
        let source = crate::object::object_keys(source);
        if let Some(target) = shapes::shape_record_for_word(target_shape) {
            let target_keys = target.keys() as usize as *const ArrayHeader;
            if target_keys != source.arr() {
                key_attrs::debug_assert_positional(source.arr(), source.count());
                key_attrs::debug_assert_positional(target_keys, target.logical_key_count());
            }
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = (source, target_shape);
}

/// Non-null when `source`'s keys keep their own storage slots but the result
/// list `result_keys` is a positional copy. Such a source learns no edge, so
/// the cached copies below never see one.
#[inline]
unsafe fn positional_source(
    record: shapes::ShapeRecordRef,
    result_keys: *const ArrayHeader,
) -> *const ArrayHeader {
    let keys = record.keys() as usize as *const ArrayHeader;
    if keys != result_keys && key_attrs::keys_have_slots(keys, record.logical_key_count()) {
        keys
    } else {
        ptr::null()
    }
}

unsafe fn copy_source_slots(
    destination: *mut ObjectHeader,
    source: *const ObjectHeader,
    count: u32,
    inline: u32,
    source_keys: *const ArrayHeader,
) {
    if inline >= count && source_keys.is_null() {
        for slot in 0..count as usize {
            copy_value(destination, slot, *fields(source).add(slot));
        }
        return;
    }
    for position in 0..count as usize {
        let slot = source_slot(source_keys, position);
        let bits = if slot < inline as usize {
            *fields(source).add(slot)
        } else {
            js_object_get_field(source, slot as u32).bits()
        };
        copy_value(destination, position, bits);
    }
}

/// The shared prefixed entry retains the complete source-ordered path.
#[no_mangle]
pub unsafe extern "C" fn js_object_spread_fresh(prefix: f64, source: f64) -> f64 {
    spread_general(prefix, source)
}

/// One shared leading-spread entry. Its ignored prefix is the compiler
/// accumulator (undefined); the ABI lets the same rooting combinator advance it.
#[no_mangle]
pub unsafe extern "C" fn js_object_spread_fresh_leading(_prefix: f64, source: f64) -> f64 {
    if let Some((src, source_id)) = ordinary_shape(source) {
        if let Some(entry) = lookup(LayoutWord(0), source_id) {
            // The full source ShapeId names the admission learned by
            // birth_slow. Getters, symbols, deletions and attribute edits
            // change that id; dictionary layouts never teach an edge.
            // Reuse the learned bounds, just as a named transition reuses
            // its learned slot, without lifting either descriptor again.
            let count = entry.target_len;
            let size = meta_form_alloc_bytes((count as usize).max(INLINE_SLOT_FLOOR));
            let raw = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT);
            if !raw.is_null() {
                debug_assert_edge(src, entry.target_shape_id);
                let object = finish_header(raw.cast(), entry.target_shape_id);
                if entry.slot_idx & NUMBER_COPY != 0 {
                    // GC_STORE_AUDIT(POINTER_FREE): the learned source
                    // shape proves Number in every copied inline lane.
                    ptr::copy_nonoverlapping(
                        fields(src),
                        fields(object).cast_mut(),
                        count as usize,
                    );
                } else {
                    copy_source_slots(
                        object,
                        src,
                        count,
                        entry.slot_idx & SOURCE_INLINE_MASK,
                        ptr::null(),
                    );
                }
                initialize_slack(object, count as usize);
                return crate::value::js_nanbox_pointer(object as i64);
            }
            return leading_collect(source, entry);
        }
    }
    leading_slow(source)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn spread_general(prefix: f64, source: f64) -> f64 {
    if let Some((src, source_id, record)) = source_record(source) {
        let predecessor = if prefix.to_bits() == crate::value::TAG_UNDEFINED {
            Some((None, 0))
        } else {
            source_record(prefix).and_then(|(object, id, r)| {
                // A prefix is copied slot for slot into a positional list.
                (r.summary() == 0
                    && r.proto_id() == shapes::PROTO_ID_DEFAULT
                    && !key_attrs::keys_have_slots(
                        r.keys() as usize as *const ArrayHeader,
                        r.logical_key_count(),
                    ))
                .then_some((Some((object, r)), id))
            })
        };
        if let Some((previous, prefix_id)) = predecessor {
            if let Some(entry) = lookup(LayoutWord(prefix_id), source_id) {
                // A stabilized edge owns its exact target descriptor, just as
                // the named transition hit does. A leading positional copy
                // needs no second descriptor probe merely to stamp that id.
                let result =
                    previous.and_then(|_| shapes::shape_record_for_word(entry.target_shape_id));
                if previous.is_none() || result.is_some() {
                    let size =
                        meta_form_alloc_bytes((entry.target_len as usize).max(INLINE_SLOT_FLOOR));
                    let raw =
                        crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT);
                    if !raw.is_null() {
                        debug_assert_edge(src, entry.target_shape_id);
                        let object = finish_header(raw.cast(), entry.target_shape_id);
                        copy_slots(object, previous, src, record, result, ptr::null());
                        initialize_slack(object, entry.target_len as usize);
                        return crate::value::js_nanbox_pointer(object as i64);
                    }
                    return birth_cached_collect(prefix, source);
                }
            }
            return birth_slow(
                previous,
                LayoutWord(prefix_id),
                src,
                LayoutWord(source_id),
                record,
            );
        }
    }
    generic(prefix, source)
}

/// A bump refusal is an allocation safepoint, not a new shape construction.
/// Root the borrowed inputs and target keys before the normal allocator can
/// move them, then re-resolve every descriptor and slot from relocated values.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn birth_cached_collect(prefix: f64, source: f64) -> f64 {
    let (_, source_id) = ordinary_shape(source).unwrap();
    let prefix_id = if prefix.to_bits() == crate::value::TAG_UNDEFINED {
        0
    } else {
        ordinary_shape(prefix).unwrap().1
    };
    let entry =
        lookup(LayoutWord(prefix_id), source_id).expect("nonmoving bump refusal retains its edge");
    let scope = crate::gc::RuntimeHandleScope::new();
    let prefix = scope.root_nanbox_f64(prefix);
    let source = scope.root_nanbox_f64(source);
    let _keys = scope.root_raw_mut_ptr(entry.next_keys as *mut ArrayHeader);
    let raw = allocate_fields(entry.target_len);
    let p = prefix.get_nanbox_f64();
    let s = source.get_nanbox_f64();
    let (src, _, record) = source_record(s).expect("rooted spread source retains its shape");
    let previous = if p.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        let (object, _, r) = source_record(p).expect("rooted spread prefix retains its shape");
        Some((object, r))
    };
    let result = shapes::shape_record_for_word(entry.target_shape_id)
        .expect("rooted target keys retain the learned result shape");
    let object = finish_header(raw, entry.target_shape_id);
    if !crate::arena::pointer_in_nursery(object as usize) {
        shapes::stamp_object_shape_id_with_carrier_note(object, entry.target_shape_id);
    }
    copy_slots(object, previous, src, record, Some(result), ptr::null());
    initialize_slack(object, entry.target_len as usize);
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn leading_collect(source: f64, entry: TransitionEntry) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    let _keys = scope.root_raw_mut_ptr(entry.next_keys as *mut ArrayHeader);
    let raw = allocate_fields(entry.target_len);
    let (src, _, record) = source_record(source.get_nanbox_f64()).unwrap();
    let object = finish_header(raw, entry.target_shape_id);
    if !crate::arena::pointer_in_nursery(object as usize) {
        shapes::stamp_object_shape_id_with_carrier_note(object, entry.target_shape_id);
    }
    copy_source_slots(
        object,
        src,
        record.logical_key_count(),
        record.live_inline_slot_count(),
        ptr::null(),
    );
    initialize_slack(object, entry.target_len as usize);
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn leading_slow(source: f64) -> f64 {
    let Some((src, source_id, record)) = source_record(source) else {
        return generic(f64::from_bits(crate::value::TAG_UNDEFINED), source);
    };
    let count = record.logical_key_count();
    if count == 0 {
        // No source edge remains live. Reuse the existing keyless birth
        // authority and normal collection pacing (a null key list cannot
        // teach a weak transition edge).
        return crate::value::js_nanbox_pointer(js_object_alloc_plain(0) as i64);
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    let numbers = count <= field_rep::REP_SLOTS
        && record.live_inline_slot_count() >= count
        && (0..count).all(|i| field_rep::slot_rep(record.rep(), i) == field_rep::REP_F64);
    let rep = if numbers {
        let mask = if count == field_rep::REP_SLOTS {
            u64::MAX
        } else {
            (1u64 << (count * 2)) - 1
        };
        field_rep::identity(record.rep()) & mask
    } else {
        field_rep::REP_ANY
    };
    let keys = if record.summary() == 0 {
        record.keys_view_of(count)
    } else {
        let list = key_attrs::alloc_key_list(count, true, false);
        // GC_STORE_AUDIT(INIT): unpublished default-attribute keys under suppression.
        ptr::copy_nonoverlapping(
            crate::array::array_elements_ptr(record.keys() as usize as *const ArrayHeader),
            crate::array::array_elements_ptr(list),
            count as usize,
        );
        (*list).length = count;
        super::rebuild_array_layout_from_slots(list);
        let proof = canonical_keys::SharedLayout::of_receiver(src.cast_mut()).unwrap();
        canonical_keys::canonicalize(&proof, list, count).view()
    };
    let shape = intern_result(keys, rep);
    let object = allocate_fields(count);
    finish_header(object, LayoutWord(shape));
    shapes::stamp_object_shape_id_with_carrier_note(object, LayoutWord(shape));
    let source_keys = positional_source(record, keys.arr());
    copy_source_slots(
        object,
        src,
        count,
        record.live_inline_slot_count(),
        source_keys,
    );
    if count != 0 && source_keys.is_null() {
        learn(
            LayoutWord(0),
            LayoutWord(source_id),
            record.live_inline_slot_count(),
            LayoutWord(shape),
            keys,
            numbers,
        );
    }
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn intern_result(keys: ObjectKeys, rep: u64) -> u32 {
    shapes::publish_shape_result(shapes::shape_descriptor_intern_with_rep(
        keys,
        keys.count(),
        keys.count(),
        0,
        shapes::ShapeObjectKind::Ordinary,
        0,
        shapes::PROTO_ID_DEFAULT,
        0,
        rep,
        &[],
        None,
        0,
    ))
    .0
}

/// Cold minting and collecting cache hits use the same unpublished allocator.
/// Keep its allocation/initialization sequence out of both cold callers.
#[cold]
#[inline(never)]
#[optimize(size)]
fn allocate_fields(count: u32) -> *mut ObjectHeader {
    super::alloc_basic::object_alloc_unpublished(0, count)
}

#[inline]
unsafe fn finish_header(object: *mut ObjectHeader, shape: LayoutWord) -> *mut ObjectHeader {
    (*object).class_id = 0;
    (*object).parent_class_id = shape.0;
    // GC_STORE_AUDIT(INIT): unpublished fresh object has no metadata.
    *crate::meta_slot_of!(crate::meta_obj_runtime_born!(object)) = ptr::null_mut();
    shapes::store_kind::premark_plain_ordinary(object);
    object
}

unsafe fn initialize_slack(object: *mut ObjectHeader, count: usize) {
    for slot in count..INLINE_SLOT_FLOOR {
        // GC_STORE_AUDIT(INIT): untraced allocation-floor slack is undefined.
        fields(object)
            .cast_mut()
            .add(slot)
            .write(crate::value::TAG_UNDEFINED);
    }
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn birth_slow(
    prefix: Option<(*const ObjectHeader, shapes::ShapeRecordRef)>,
    prefix_id: LayoutWord,
    source: *const ObjectHeader,
    source_id: LayoutWord,
    record: shapes::ShapeRecordRef,
) -> f64 {
    // No callbacks in this construction. Suppress collection only while its
    // borrowed inputs and unfinished storage lack independent roots.
    let _suppress = crate::gc::GcSuppressScope::new();
    let prefix_count = prefix.map_or(0, |(_, r)| r.logical_key_count());
    let count = record.logical_key_count();
    // Preserve an existing all-Number representation on a leading copy.
    // Generalization changes the source ShapeId, so an old edge cannot copy
    // a pointer without a barrier. No observed-value or per-site fact exists.
    let numbers = prefix.is_none()
        && count <= field_rep::REP_SLOTS
        && record.live_inline_slot_count() >= count
        && (0..count).all(|i| field_rep::slot_rep(record.rep(), i) == field_rep::REP_F64);
    let rep = if numbers {
        let mask = if count == field_rep::REP_SLOTS {
            u64::MAX
        } else {
            (1u64 << (count * 2)) - 1
        };
        record.rep() & mask
    } else {
        field_rep::REP_ANY
    };
    let proof = canonical_keys::SharedLayout::of_receiver(source.cast_mut()).unwrap();
    let keys = if prefix_count == 0 && record.summary() == 0 {
        record.keys_view_of(count)
    } else {
        let mut words = Vec::with_capacity((prefix_count + count) as usize);
        if let Some((_, r)) = prefix {
            let slots = crate::array::array_elements_ptr(r.keys() as usize as *const ArrayHeader);
            for i in 0..prefix_count as usize {
                words.push(*slots.add(i));
            }
        }
        // A keyless source (`{ ...undefined }`'s result) has a null key list.
        let slots = if count == 0 {
            ptr::null()
        } else {
            crate::array::array_elements_ptr(record.keys() as usize as *const ArrayHeader)
        };
        for slot in 0..count as usize {
            let word = *slots.add(slot);
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let bytes =
                crate::string::js_string_key_bytes(JSValue::from_bits(word), &mut scratch).unwrap();
            if !words.iter().any(|&old| {
                crate::string::js_string_key_matches_bytes(JSValue::from_bits(old), bytes)
            }) {
                words.push(word);
            }
        }
        let list = super::key_attrs::alloc_key_list(words.len() as u32, true, false);
        // GC_STORE_AUDIT(INIT): unpublished canonical key list under suppression.
        ptr::copy_nonoverlapping(
            words.as_ptr(),
            crate::array::array_elements_ptr(list),
            words.len(),
        );
        (*list).length = words.len() as u32;
        super::rebuild_array_layout_from_slots(list);
        canonical_keys::canonicalize(&proof, list, words.len() as u32).view()
    };
    let shape = intern_result(keys, rep);
    let object = allocate_fields(keys.count());
    shapes::store_kind::premark_plain_ordinary(object);
    shapes::stamp_object_shape_id_with_carrier_note(object, LayoutWord(shape));
    let result = shapes::shape_record_for_word(LayoutWord(shape)).unwrap();
    let source_keys = positional_source(record, keys.arr());
    copy_slots(object, prefix, source, record, Some(result), source_keys);
    if keys.count() != 0 && source_keys.is_null() {
        learn(
            prefix_id,
            source_id,
            record.live_inline_slot_count(),
            LayoutWord(shape),
            keys,
            numbers,
        );
    }
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn generic(prefix: f64, source: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    let target = if prefix.to_bits() == crate::value::TAG_UNDEFINED {
        crate::value::js_nanbox_pointer(js_object_alloc_plain(0) as i64)
    } else {
        prefix
    };
    js_object_literal_spread(target, source.get_nanbox_f64())
}
