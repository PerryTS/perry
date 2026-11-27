//! Fresh literal spreads, copied by the source's structure (DESIGN_E E4):
//! no learned edge, no descriptor objects, no by-name define on the
//! structural paths. Prefixed spreads use the canonical key-list authority.
use super::header::meta_form_alloc_bytes;
use super::*;
use crate::codegen_abi::LayoutWord;

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
    if id.wrapping_sub(shapes::SHAPE_ID_BASE) >= crate::codegen_abi::LAYOUT_RECORD_SPAN {
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

/// The own key count of a source whose shape proves every key an enumerable
/// string-keyed data property ([`source_record`]); 0 for any other value.
pub(super) unsafe fn plain_data_key_count(value: f64) -> u32 {
    source_record(value).map_or(0, |(_, _, record)| record.logical_key_count())
}

/// Whether `value` is an ordinary-band object whose shape proves it owns no
/// symbol-keyed property.
pub(super) unsafe fn proves_no_symbols(value: f64) -> bool {
    ordinary_shape(value)
        .and_then(|(_, id)| shapes::shape_record_for_word(LayoutWord(id)))
        .is_some_and(|record| record.proves_no_symbols())
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
    targets: &[u32],
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
    // Each source key's position in the result list, recorded while the
    // list was built (`birth_slow`).
    debug_assert_eq!(targets.len(), record.logical_key_count() as usize);
    for slot in 0..record.logical_key_count() as usize {
        let target = targets[slot] as usize;
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

/// The birth width of a spread result: its own `count` keys plus the `tail`
/// keys the literal defines after this spread part, so those definitions fill
/// inline slots instead of spilling. A later key that repeats a copied key
/// overwrites it and leaves its reserved slot as undefined slack.
#[inline]
fn birth_width(count: u32, tail: u32) -> usize {
    (count as usize)
        .saturating_add(tail as usize)
        .max(INLINE_SLOT_FLOOR)
}

/// The shared prefixed entry retains the complete source-ordered path.
/// `tail` is the number of literal keys after this spread part.
#[no_mangle]
pub unsafe extern "C" fn js_object_spread_fresh(prefix: f64, source: f64, tail: u32) -> f64 {
    spread_general(prefix, source, tail)
}

/// One shared leading-spread entry. Its ignored prefix is the compiler
/// accumulator (undefined). `tail` is the number of literal keys after the spread.
#[no_mangle]
pub unsafe extern "C" fn js_object_spread_fresh_leading(
    _prefix: f64,
    source: f64,
    tail: u32,
) -> f64 {
    spread_leading(source, tail)
}

#[inline]
unsafe fn spread_leading(source: f64, tail: u32) -> f64 {
    match structural(source, tail) {
        Some(result) => result,
        None => match accessor_spread(source, tail) {
            Some(result) => result,
            None => generic(f64::from_bits(crate::value::TAG_UNDEFINED), source, tail),
        },
    }
}

/// The most keys [`accessor_spread`] collects before its result is born.
const ACCESSOR_SPREAD_MAX_KEYS: u32 = 4096;

/// A leading spread of an ordinary source with accessors (a module namespace
/// built by `Object.defineProperty(exports, k, { get, enumerable })`),
/// CopyDataProperties driven by the source's layout (NXPA6 DESIGN.md §1):
/// one walk of the layout's key list, each key's entry and slot read by
/// position, each getter called from the pair in its slot. A getter may
/// change the source; every change to its own keys or their attributes moves
/// its layout word (the shared key list is immutable), so the spec's per-key
/// re-check is "the word is unchanged", and once it moved the remaining keys
/// take the by-name step. The result is born with the collected key list.
/// `None` (nothing ran) for every other source: the generic copy.
#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn accessor_spread(source: f64, tail: u32) -> Option<f64> {
    let (src, word) = ordinary_shape(source)?;
    // A class id is admitted when the identity is a plain record (an object
    // literal's shape class: no class prototype, ordinary kind). Its
    // runtime-internal keys are still hidden, by name, as every enumeration
    // of a class-id object hides them.
    if word & overflow::SHADOW_BIT != 0
        || !overflow::table(src).is_null()
        || !shapes::object_is_plain_record(src)
    {
        return None;
    }
    let header = crate::gc::header_from_trusted_user_ptr(src.cast());
    if !crate::gc_type_is_object!((*header).obj_type)
        || (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*header)._reserved
            & (crate::gc::OBJ_FLAG_STABLE_TOMBSTONES | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        || !key_attrs::attrs_live_in_keys_of_bits(source.to_bits())
    {
        return None;
    }
    let record = shapes::shape_record_for_word(LayoutWord(word))?;
    let summary = record.summary();
    let count = record.logical_key_count();
    let keys = record.keys() as usize as *mut ArrayHeader;
    if !record.kind_is(shapes::ShapeObjectKind::Ordinary)
        || summary & key_attrs::SUMMARY_ACCESSOR == 0
        || summary & (key_attrs::SUMMARY_PRIVATE | key_attrs::SUMMARY_HOLES) != 0
        || count == 0
        || count > ACCESSOR_SPREAD_MAX_KEYS
        || keys.is_null()
    {
        return None;
    }
    // The layout's key list is a shared canonical backing: its keys and
    // attribute entries never change in place.
    if crate::value::addr_class::try_read_gc_header(keys as usize)?.gc_flags
        & crate::gc::GC_FLAG_SHAPE_SHARED
        == 0
        || crate::array::subclass_elements::own_elements(src as usize).is_some()
        || super::string_wrapper::length(src as usize).is_some()
        || crate::process::is_process_env_object(source)
    {
        return None;
    }
    let proof = canonical_keys::SharedLayout::of_receiver(src.cast_mut())?;
    let slots_keyed = key_attrs::keys_have_slots(keys, count);
    let inline = record.live_inline_slot_count();
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_h = scope.root_nanbox_f64(source);
    let src_h = scope.root_raw_const_ptr(src);
    let keys_h = scope.root_raw_mut_ptr(keys);
    // The key list is taken before any getter runs (CopyDataProperties step
    // 3): the string part is the record's list, the symbol part this snapshot.
    let symbols = super::assign::symbol_key_snapshot(&source_h, proves_no_symbols(source));
    let mut taken: Vec<u32> = Vec::with_capacity(count as usize);
    let mut values = Vec::with_capacity(count as usize);
    let mut getter_ran = false;
    let mut by_name = false;
    for position in 0..count {
        let keys = keys_h.get_raw_mut_ptr::<ArrayHeader>();
        let key = *crate::array::array_elements_ptr(keys).add(position as usize);
        if !JSValue::from_bits(key).is_any_string() {
            continue;
        }
        let src = src_h.get_raw_const_ptr::<ObjectHeader>();
        if (*src).class_id != 0 {
            let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            if crate::string::js_string_key_bytes(JSValue::from_bits(key), &mut buf)
                .is_some_and(field_get_set::is_internal_runtime_key_bytes)
            {
                continue;
            }
        }
        // CopyDataProperties step 4.c's re-check, after any getter.
        by_name = by_name || (*src).shape_word() != word;
        let value = if by_name {
            getter_ran = true;
            match super::assign::own_enumerable_value_by_name(src, key) {
                Some(value) => value.to_bits(),
                None => continue,
            }
        } else {
            let entry = key_attrs::keys_entry(keys, position);
            if entry & key_attrs::ENTRY_NON_ENUMERABLE != 0 {
                continue;
            }
            let slot = if slots_keyed {
                key_attrs::key_storage_slot(keys, position)
            } else {
                position
            };
            let bits = field_get_set::object_field_at_with_live(src, slot, inline).bits();
            if entry & key_attrs::ENTRY_ACCESSOR == 0 {
                bits
            } else {
                getter_ran = true;
                let acc = super::accessor_pair::getter_half_of_value(bits);
                match field_get_set::call_getter(src, acc) {
                    Some(value) => value.to_bits(),
                    // A static class entry: the complete read calls it.
                    None => match super::assign::own_enumerable_value_by_name(src, key) {
                        Some(value) => value.to_bits(),
                        None => continue,
                    },
                }
            }
        };
        taken.push(position);
        values.push(scope.root_nanbox_u64(value));
    }
    let result = if taken.is_empty() {
        crate::value::js_nanbox_pointer(js_object_alloc_plain(tail) as i64)
    } else {
        let _suppress = crate::gc::GcSuppressScope::new();
        let census = crate::snap_census::first_use();
        let n = taken.len() as u32;
        let list = key_attrs::alloc_key_list(n, true, false);
        let words = crate::array::array_elements_ptr(keys_h.get_raw_mut_ptr::<ArrayHeader>());
        let dst = crate::array::array_elements_ptr(list);
        for (at, &position) in taken.iter().enumerate() {
            // GC_STORE_AUDIT(INIT): unpublished key list under suppression.
            *dst.add(at) = *words.add(position as usize);
        }
        (*list).length = n;
        super::rebuild_array_layout_from_slots(list);
        let result_keys = canonical_keys::canonicalize(&proof, list, n).view();
        let shape = intern_result(result_keys, field_rep::REP_ANY, tail);
        drop(census);
        let object = allocate_fields(result_keys.count(), tail);
        shapes::store_kind::premark_plain_ordinary(object);
        shapes::stamp_object_shape_id_with_carrier_note(object, LayoutWord(shape));
        for (slot, value) in values.iter().enumerate() {
            copy_value(object, slot, value.get_nanbox_u64());
        }
        crate::value::js_nanbox_pointer(object as i64)
    };
    if symbols.is_empty() {
        return Some(result);
    }
    let target_h = scope
        .root_raw_mut_ptr((result.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader);
    super::assign::copy_symbol_keys(true, &target_h, &src_h, &source_h, symbols, getter_ran);
    Some(target_h.with_mut_ptr::<ObjectHeader, _>(|t| crate::value::js_nanbox_pointer(t as i64)))
}

/// [`ordinary_shape`] for a structural spread source: the layout word
/// without the overflow shadow bit, so a layout object with a table (a `{}`
/// built by adds) is admitted by its band and record like any other.
#[inline]
unsafe fn spread_source(value: f64) -> Option<(*const ObjectHeader, u32)> {
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
    let id = (*object).shape_word() & !overflow::SHADOW_BIT;
    if id.wrapping_sub(shapes::SHAPE_ID_BASE) >= 2 << crate::codegen_abi::LAYOUT_BAND_SHIFT {
        return None;
    }
    Some((object, id))
}

/// One leading source copied by its structure (DESIGN_E 3.3):
/// (a) a default-prototype ordinary source whose layout proves only plain
///     enumerable string data keys and no holes is born on the SAME layout
///     word (immutable layout data, no mint), its inline slots copied in one
///     run; an overflow table whose present entries are all plain
///     enumerable string data is cloned verbatim (keys, values, attributes
///     and the hash index: no re-hash; index keys keep the table's order,
///     which every enumeration orders numerically first);
/// (b) a source of another prototype (class instance) or with holes, and no
///     table: a plain result born with a table sized once, present keys
///     appended in layout order with their atoms' stored hashes.
/// None for everything else (accessors, symbols, non-enumerable, private or
/// internal keys, index keys of a (b) source, exotic kinds, per-object
/// descriptor or stable-tombstone flags): the generic CopyDataProperties.
#[inline]
unsafe fn structural(source: f64, tail: u32) -> Option<f64> {
    let (src, id) = spread_source(source)?;
    let record = shapes::shape_record_for_word(LayoutWord(id))?;
    if (*crate::gc::header_from_trusted_user_ptr(src.cast()))._reserved
        & (crate::gc::OBJ_FLAG_STABLE_TOMBSTONES | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS)
        != 0
    {
        return None;
    }
    let count = record.logical_key_count();
    let inline = record.live_inline_slot_count();
    let table = overflow::table(src);
    if record.spread_same_layout() {
        // Layout keys past the inline slots live in the receiver's spill:
        // the result, born on the same layout word, reads them from its own
        // spill, so it gets a copy of the source's spill words (r2b_order: 9
        // layout keys, 2 inline slots). A table's slot-referencing entries
        // may name spill positions too: that source takes the generic copy.
        if count > inline {
            if !table.is_null()
                || !super::spill::spill_positions_present(src, inline as usize, count as usize)
            {
                return None;
            }
            return Some(same_layout_spilled(src, id, inline, count, tail));
        }
        let copied = if table.is_null() {
            inline
        } else {
            // Slot-referencing table entries may hold values in inline
            // slots past the layout's own bound: copy every slot they reach.
            inline.max(overflow::plain_enumerable_strings(table)?)
        };
        return Some(same_layout(src, id, copied, table, tail));
    }
    if record.spread_plain_slots() && table.is_null() && count <= inline {
        return Some(table_born(src, record, count, tail));
    }
    None
}

#[inline]
unsafe fn same_layout(
    src: *const ObjectHeader,
    id: u32,
    inline: u32,
    table: *const overflow::Table,
    tail: u32,
) -> f64 {
    let width = birth_width(inline, tail);
    let size = meta_form_alloc_bytes(width);
    let _suppress;
    let mut raw = if table.is_null() {
        crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
    } else {
        ptr::null_mut()
    };
    if raw.is_null() {
        // No collection may move `src` (or its table) mid-copy.
        _suppress = crate::gc::GcSuppressScope::new();
        raw = crate::arena::arena_alloc_gc(size, 8, crate::gc::GC_TYPE_OBJECT);
    }
    let object = finish_header(raw.cast(), LayoutWord(id));
    for slot in 0..inline as usize {
        copy_value(object, slot, *fields(src).add(slot));
    }
    initialize_slack(object, inline as usize, width);
    if !table.is_null() {
        overflow::clone_plain_into(object, table);
    }
    crate::value::js_nanbox_pointer(object as i64)
}

/// [`same_layout`] for a source whose layout keys `inline..count` live in
/// its spill: the result's inline run is copied the same way and its own
/// spill holds a copy of the source's spill words, each stored with the
/// spill's layout note and write barrier. No collection runs mid-copy.
#[inline(never)]
unsafe fn same_layout_spilled(
    src: *const ObjectHeader,
    id: u32,
    inline: u32,
    count: u32,
    tail: u32,
) -> f64 {
    let _suppress = crate::gc::GcSuppressScope::new();
    let width = birth_width(inline, tail);
    let raw =
        crate::arena::arena_alloc_gc(meta_form_alloc_bytes(width), 8, crate::gc::GC_TYPE_OBJECT);
    let object = finish_header(raw.cast(), LayoutWord(id));
    for slot in 0..inline as usize {
        copy_value(object, slot, *fields(src).add(slot));
    }
    initialize_slack(object, inline as usize, width);
    super::spill::spill_copy_into_fresh(
        object,
        src,
        inline as usize,
        count as usize,
        tail as usize,
    );
    crate::value::js_nanbox_pointer(object as i64)
}

#[inline(never)]
unsafe fn table_born(
    src: *const ObjectHeader,
    record: shapes::ShapeRecordRef,
    count: u32,
    tail: u32,
) -> f64 {
    let _suppress = crate::gc::GcSuppressScope::new();
    let object = js_object_alloc_plain(0);
    let t = overflow::start_sized(object, 0, count.saturating_add(tail));
    overflow::install(object, t);
    if count == 0 {
        return crate::value::js_nanbox_pointer(object as i64);
    }
    let keys = record.keys() as usize as *const ArrayHeader;
    let source_keys = if key_attrs::keys_have_slots(keys, count) {
        keys
    } else {
        ptr::null()
    };
    let words = crate::array::array_elements_ptr(keys);
    // The source record's names are copied undecoded: the table's copy
    // decodes them (`key_attrs::TABLE_KEY_HIDDEN`).
    (*t).summary |= key_attrs::TABLE_KEY_HIDDEN;
    for position in 0..count as usize {
        let key = *words.add(position);
        let bits = *fields(src).add(source_slot(source_keys, position));
        if key == crate::value::TAG_HOLE || bits == crate::value::TAG_HOLE {
            continue;
        }
        let atom = crate::string::property_atom_from_value(key);
        overflow::append_atom(
            t,
            atom,
            super::layout_probe::store_bits(f64::from_bits(bits)),
        );
    }
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn spread_general(prefix: f64, source: f64, tail: u32) -> f64 {
    if prefix.to_bits() == crate::value::TAG_UNDEFINED {
        return spread_leading(source, tail);
    }
    if let Some((src, _, record)) = source_record(source) {
        let previous = source_record(prefix).and_then(|(object, _, r)| {
            // A prefix is copied slot for slot into a positional list.
            (r.summary() == 0
                && r.proto_id() == shapes::PROTO_ID_DEFAULT
                && !key_attrs::keys_have_slots(
                    r.keys() as usize as *const ArrayHeader,
                    r.logical_key_count(),
                ))
            .then_some((object, r))
        });
        if let Some(previous) = previous {
            return birth_slow(Some(previous), src, record, tail);
        }
    }
    generic(prefix, source, tail)
}

#[cold]
#[inline(never)]
#[optimize(size)]
/// `tail` slots past the keys are the result's reserved inline slack: they
/// are part of its inline bound, as for a keyless birth of that width.
unsafe fn intern_result(keys: ObjectKeys, rep: u64, tail: u32) -> u32 {
    shapes::publish_shape_result(shapes::shape_descriptor_intern_with_rep(
        keys,
        keys.count(),
        keys.count().saturating_add(tail),
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
/// Keep its allocation/initialization sequence out of both cold callers. It
/// initializes every slot of the [`birth_width`], the slack included.
#[cold]
#[inline(never)]
#[optimize(size)]
fn allocate_fields(count: u32, tail: u32) -> *mut ObjectHeader {
    let width = u32::try_from(birth_width(count, tail)).unwrap_or(u32::MAX);
    super::alloc_basic::object_alloc_unpublished(0, width)
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

unsafe fn initialize_slack(object: *mut ObjectHeader, count: usize, width: usize) {
    for slot in count..width {
        // GC_STORE_AUDIT(INIT): untraced birth-width slack is undefined.
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
    source: *const ObjectHeader,
    record: shapes::ShapeRecordRef,
    tail: u32,
) -> f64 {
    // No callbacks in this construction. Suppress collection only while its
    // borrowed inputs and unfinished storage lack independent roots.
    let _suppress = crate::gc::GcSuppressScope::new();
    let prefix_count = prefix.map_or(0, |(_, r)| r.logical_key_count());
    let count = record.logical_key_count();
    // Preserve an existing all-Number representation on a leading copy.
    // Generalization changes the source ShapeId, so an old edge cannot copy
    // a pointer without a barrier. No observed-value or per-site fact exists.
    let numbers = false
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
    // The pair's first use, the same whichever init takes it first: the
    // scratch key list, its canonical list, the shape and (below) the learned
    // edge. The SNAP census counts them as the runtime's; the result object
    // stays the caller's.
    let census = crate::snap_census::first_use();
    let proof = canonical_keys::SharedLayout::of_receiver(source.cast_mut()).unwrap();
    let mut targets: Vec<u32> = Vec::new();
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
        // The source's keys are distinct, so a source key can repeat only a
        // prefix key: it keeps that key's position (its value overwrites it).
        // Every other source key is appended.
        targets.reserve(count as usize);
        for slot in 0..count as usize {
            let word = *slots.add(slot);
            let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
            let bytes =
                crate::string::js_string_key_bytes(JSValue::from_bits(word), &mut scratch).unwrap();
            let target = words[..prefix_count as usize]
                .iter()
                .position(|&old| {
                    old == word
                        || crate::string::js_string_key_matches_bytes(
                            JSValue::from_bits(old),
                            bytes,
                        )
                })
                .unwrap_or_else(|| {
                    words.push(word);
                    words.len() - 1
                });
            targets.push(target as u32);
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
    let shape = intern_result(keys, rep, tail);
    drop(census);
    let object = allocate_fields(keys.count(), tail);
    shapes::store_kind::premark_plain_ordinary(object);
    shapes::stamp_object_shape_id_with_carrier_note(object, LayoutWord(shape));
    let result = shapes::shape_record_for_word(LayoutWord(shape)).unwrap();
    let source_keys = positional_source(record, keys.arr());
    copy_slots(
        object,
        prefix,
        source,
        record,
        Some(result),
        source_keys,
        &targets,
    );
    crate::value::js_nanbox_pointer(object as i64)
}

#[cold]
#[inline(never)]
#[optimize(size)]
unsafe fn generic(prefix: f64, source: f64, tail: u32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    // The source's own key count is unknown here (a proxy, an accessor or
    // dictionary source, an array, a primitive): reserve the literal's tail only.
    let target = if prefix.to_bits() == crate::value::TAG_UNDEFINED {
        crate::value::js_nanbox_pointer(js_object_alloc_plain(tail) as i64)
    } else {
        prefix
    };
    js_object_literal_spread(target, source.get_nanbox_f64())
}
