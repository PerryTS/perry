//! Charter step 3: a key's ATTRIBUTES live with the key, in the keys array
//! (V8's descriptor arrays).
//!
//! # Representation
//!
//! Every key position carries one *entry* byte. `0` is the default — a
//! writable, enumerable, configurable data property — so a list whose keys
//! are all default carries nothing, hashes and validates exactly as it did
//! before attributes existed, and costs a plain object nothing:
//!
//! | bit | meaning |
//! |---|---|
//! | [`ENTRY_NON_WRITABLE`] | `[[Writable]]` false (for an accessor: the internal writable bit) |
//! | [`ENTRY_NON_ENUMERABLE`] | `[[Enumerable]]` false |
//! | [`ENTRY_NON_CONFIGURABLE`] | `[[Configurable]]` false |
//! | [`ENTRY_ACCESSOR`] | an accessor property |
//! | [`ENTRY_HAS_GET`] / [`ENTRY_HAS_SET`] | which accessor halves exist |
//!
//! A keys array that carries any non-default entry owns an *attributes
//! array*: a pointer-free `GC_TYPE_ARRAY` with one element per key position,
//! `INT32_TAG | cumulative << 8 | entry`. `cumulative` is the OR of
//! [`entry_summary`] over positions `0..=i`, so the summary of any PREFIX of
//! the list — which is what a shape names, since one canonical backing serves
//! a whole growth chain (`canonical_keys.rs`) — is one load:
//! [`keys_summary`]`(keys, count)`.
//!
//! The pointer to the attributes array lives in the keys array's first
//! physical slot, in front of logical element 0, with `GC_ARRAY_NAMED_PROPS`
//! set: exactly the reserve #10166 gives an Array's named properties
//! (`array/named_props.rs`). So nothing new exists for the collector: the
//! reserve word is already emitted as a fixed child slot of every flagged
//! array (`gc::layout_slot_visit`), `js_array_grow` already re-reserves and
//! copies it, and every element reader already adds the front offset
//! (`array_elements_ptr`, #10939). The front slot count is
//! [`KEYS_ATTRS_FRONT_SLOTS`]; nothing may hand-type it.
//!
//! # Why reusing `GC_ARRAY_NAMED_PROPS` on a keys array is sound
//!
//! `GcHeader::_reserved` has no free bit for arrays (`gc/types.rs`). The
//! named-properties bit means "the first physical slot is a traced reserve
//! word" to the collector and to growth, and that is all a keys array needs.
//! Its other meaning — "this Array has expando properties, read them from a
//! pairs array" — is consulted only by the JS-facing Array operations
//! (`arr.foo`, `Object.keys(arr)`, …), and a keys array is NEVER a JS value:
//! it is reached only through a shape record or a dictionary receiver's meta
//! record, and every producer that hands keys to JS (`Object.keys`,
//! `getOwnPropertyNames`, `for…in`) builds a fresh result array. A keys
//! array therefore never meets a reader of the pairs interpretation.
//!
//! # Mutability follows the keys
//!
//! A canonical backing's attributes prefix is as immutable as its key prefix:
//! the only in-place write is the tip append past every published count. An
//! owned list (a dictionary receiver's private list) is mutated in place with
//! its keys; its cumulative words are then recomputed from the edited
//! position, so they stay exact.

use crate::array::ArrayHeader;
use crate::value::{INT32_TAG, POINTER_MASK, POINTER_TAG, TAG_MASK};

/// `[[Writable]]` is false. On an accessor this is the internal writable bit
/// (`install_fresh_accessor_property` keeps it true for a fresh accessor).
pub(crate) const ENTRY_NON_WRITABLE: u8 = 0x01;
/// `[[Enumerable]]` is false.
pub(crate) const ENTRY_NON_ENUMERABLE: u8 = 0x02;
/// `[[Configurable]]` is false.
pub(crate) const ENTRY_NON_CONFIGURABLE: u8 = 0x04;
/// The key is an accessor property; its getter/setter pair lives with the
/// receiver, never with the shape.
pub(crate) const ENTRY_ACCESSOR: u8 = 0x08;
/// The accessor has a getter.
pub(crate) const ENTRY_HAS_GET: u8 = 0x10;
/// The accessor has a setter.
pub(crate) const ENTRY_HAS_SET: u8 = 0x20;
/// The three `PropertyAttrs` bits, inverted.
pub(crate) const ENTRY_ATTR_MASK: u8 =
    ENTRY_NON_WRITABLE | ENTRY_NON_ENUMERABLE | ENTRY_NON_CONFIGURABLE;
/// The accessor half of an entry.
pub(crate) const ENTRY_ACCESSOR_MASK: u8 = ENTRY_ACCESSOR | ENTRY_HAS_GET | ENTRY_HAS_SET;

/// Summary bits: what a list (or a prefix of one) MAY contain.
///
/// Some key is an accessor.
pub(crate) const SUMMARY_ACCESSOR: u8 = 0x01;
/// Some DATA key is not writable.
pub(crate) const SUMMARY_NON_WRITABLE: u8 = 0x02;
/// Some key is not enumerable.
pub(crate) const SUMMARY_NON_ENUMERABLE: u8 = 0x04;
/// Some key is not configurable.
pub(crate) const SUMMARY_NON_CONFIGURABLE: u8 = 0x08;
/// Every per-key summary bit: a list with none of them is all default.
pub(crate) const SUMMARY_KEY_BITS: u8 =
    SUMMARY_ACCESSOR | SUMMARY_NON_WRITABLE | SUMMARY_NON_ENUMERABLE | SUMMARY_NON_CONFIGURABLE;
/// Bits a plain data store must see clear on every hop of a prototype chain.
pub(crate) const SUMMARY_BLOCKS_STORE: u8 = SUMMARY_ACCESSOR | SUMMARY_NON_WRITABLE;

/// Physical slots a keys array that carries attributes reserves in front of
/// logical element 0: the attributes pointer. The ONE spelling of this
/// number; generated code that reads a keys array positionally must add it
/// for such an array (it is `array_front_offset`).
pub(crate) const KEYS_ATTRS_FRONT_SLOTS: usize = 1;

/// The summary bits one entry contributes.
#[inline]
pub(crate) const fn entry_summary(entry: u8) -> u8 {
    let mut s = 0;
    if entry & ENTRY_ACCESSOR != 0 {
        s |= SUMMARY_ACCESSOR;
    } else if entry & ENTRY_NON_WRITABLE != 0 {
        s |= SUMMARY_NON_WRITABLE;
    }
    if entry & ENTRY_NON_ENUMERABLE != 0 {
        s |= SUMMARY_NON_ENUMERABLE;
    }
    if entry & ENTRY_NON_CONFIGURABLE != 0 {
        s |= SUMMARY_NON_CONFIGURABLE;
    }
    s
}

/// The entry's data-attribute half from `PropertyAttrs` bits (W=1, E=2, C=4).
#[inline]
pub(crate) const fn attr_bits_to_entry(attrs: u8) -> u8 {
    !attrs & ENTRY_ATTR_MASK
}

/// The `PropertyAttrs` bits (W=1, E=2, C=4) an entry records.
#[inline]
pub(crate) const fn entry_to_attr_bits(entry: u8) -> u8 {
    !entry & ENTRY_ATTR_MASK
}

/// A key with this entry is a plain WRITABLE DATA property — the only kind an
/// inline store may overwrite without the runtime.
#[inline]
pub(crate) const fn entry_is_plain_writable_data(entry: u8) -> bool {
    entry & (ENTRY_ACCESSOR | ENTRY_NON_WRITABLE) == 0
}

/// One element of an attributes array: the entry at this position, and the
/// CUMULATIVE facts of positions `0..=i` — the summary, and two 16-bit Bloom
/// filters over the keys that carry a non-default entry / an accessor. The
/// cumulative words make a prefix's answers one load: the summary of a shape
/// naming `count` keys is element `count - 1`'s, and so are its filters.
#[derive(Clone, Copy)]
struct Word {
    entry: u8,
    summary: u8,
    entry_bloom: u16,
    accessor_bloom: u16,
}

impl Word {
    const EMPTY: Word = Word {
        entry: 0,
        summary: 0,
        entry_bloom: 0,
        accessor_bloom: 0,
    };

    #[inline]
    const fn encode(self) -> u64 {
        INT32_TAG
            | self.entry as u64
            | (self.summary as u64) << 8
            | (self.entry_bloom as u64) << 16
            | (self.accessor_bloom as u64) << 32
    }

    #[inline]
    const fn decode(word: u64) -> Word {
        Word {
            entry: word as u8,
            summary: (word >> 8) as u8,
            entry_bloom: (word >> 16) as u16,
            accessor_bloom: (word >> 32) as u16,
        }
    }

    /// This position's word: `entry` for `key`, after the cumulative `prev`.
    #[inline]
    unsafe fn after(prev: Word, entry: u8, key: crate::JSValue) -> Word {
        let mut w = Word { entry, ..prev };
        if entry != 0 {
            w.summary |= entry_summary(entry);
            let bit = key_bloom_bit(key);
            w.entry_bloom |= bit;
            if entry & ENTRY_ACCESSOR != 0 {
                w.accessor_bloom |= bit;
            }
        }
        w
    }
}

/// The Bloom bit a key contributes: one of 16, from the key's content hash.
/// A key without a string form (a hole, a symbol) contributes none — it
/// carries no entry either.
#[inline]
unsafe fn key_bloom_bit(key: crate::JSValue) -> u16 {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match crate::string::js_string_key_bytes(key, &mut sso) {
        Some(bytes) => bloom_bit_of_bytes(bytes),
        None => 0,
    }
}

#[inline]
fn bloom_bit_of_bytes(bytes: &[u8]) -> u16 {
    let h = crate::object::keys_lookup::key_bytes_hash(bytes.as_ptr(), bytes.len());
    1u16 << (h >> 60)
}

/// The attributes array of `keys`, or null when the list carries none.
///
/// The keys arrays a shape record or a dictionary receiver names are live,
/// resolved heads, so this is one header load and one reserve-word load. A
/// forwarded head (a list read across its own growth) is resolved first.
///
/// # Safety
/// `keys` is null or a live keys array.
#[inline]
pub(crate) unsafe fn keys_attrs(keys: *const ArrayHeader) -> *mut ArrayHeader {
    if keys.is_null() {
        return std::ptr::null_mut();
    }
    let header = crate::gc::header_from_trusted_user_ptr(keys.cast());
    let mut keys = keys;
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        keys = crate::array::clean_arr_ptr(keys);
        if keys.is_null() {
            return std::ptr::null_mut();
        }
    }
    if crate::array::array_object_flags_resolved(keys) & crate::gc::GC_ARRAY_NAMED_PROPS == 0 {
        return std::ptr::null_mut();
    }
    let word = *crate::array::array_named_props_slot(keys);
    if word & TAG_MASK != POINTER_TAG {
        return std::ptr::null_mut();
    }
    (word & POINTER_MASK) as *mut ArrayHeader
}

/// [`keys_attrs`] for a keys word that may not be a real array (a shape
/// record minted by a test from a synthetic address): the header is read
/// through the ownership-checking reader first.
///
/// # Safety
/// `keys` is any address.
#[inline]
pub(crate) unsafe fn keys_attrs_checked(keys: *const ArrayHeader) -> *mut ArrayHeader {
    match crate::value::addr_class::try_read_gc_header(keys as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => keys_attrs(keys),
        _ => std::ptr::null_mut(),
    }
}

/// The raw element words of an attributes array and how many are written.
#[inline]
unsafe fn words(attrs: *const ArrayHeader) -> (*mut u64, usize) {
    (
        crate::array::array_elements_ptr(attrs),
        ((*attrs).length.min((*attrs).capacity)) as usize,
    )
}

/// The cumulative word of the first `count` positions (EMPTY for none).
#[inline]
unsafe fn prefix_word(attrs: *const ArrayHeader, count: u32) -> Word {
    let (w, len) = words(attrs);
    let last = (count as usize).min(len);
    if last == 0 {
        Word::EMPTY
    } else {
        Word::decode(*w.add(last - 1))
    }
}

/// The entry at key position `pos` of `keys` (0 = default, also for a
/// position past the attributes array).
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_entry(keys: *const ArrayHeader, pos: u32) -> u8 {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return 0;
    }
    let (w, len) = words(attrs);
    if (pos as usize) < len {
        Word::decode(*w.add(pos as usize)).entry
    } else {
        0
    }
}

/// The summary of the first `count` keys of `keys`: exact for a canonical
/// list, an over-approximation for an owned list edited in place.
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_summary(keys: *const ArrayHeader, count: u32) -> u8 {
    if count == 0 {
        return 0;
    }
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return 0;
    }
    prefix_word(attrs, count).summary & SUMMARY_KEY_BITS
}

/// [`keys_summary`] through [`keys_attrs_checked`].
///
/// # Safety
/// `keys` is any address.
#[inline]
pub(crate) unsafe fn keys_summary_checked(keys: *const ArrayHeader, count: u32) -> u8 {
    if count == 0 {
        return 0;
    }
    let attrs = keys_attrs_checked(keys);
    if attrs.is_null() {
        return 0;
    }
    prefix_word(attrs, count).summary & SUMMARY_KEY_BITS
}

/// Can `key` carry a non-default entry (`accessor`: an accessor entry) among
/// the first `count` positions of `keys`? `false` is authoritative: the
/// prefix's Bloom filter has no bit for it.
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_may_carry(
    keys: *const ArrayHeader,
    count: u32,
    key: &[u8],
    accessor: bool,
) -> bool {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return false;
    }
    let w = prefix_word(attrs, count);
    let bloom = if accessor {
        w.accessor_bloom
    } else {
        w.entry_bloom
    };
    bloom & bloom_bit_of_bytes(key) != 0
}

/// Does any of the first `count` positions carry a non-default entry?
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_have_entries(keys: *const ArrayHeader, count: u32) -> bool {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return false;
    }
    let (w, len) = words(attrs);
    (0..(count as usize).min(len)).any(|i| Word::decode(*w.add(i)).entry != 0)
}

/// Allocate an attributes array with room for `capacity` positions.
/// Pointer-free: its elements are INT32 boxes.
///
/// # Safety
/// May collect: the caller roots what it holds.
pub(crate) unsafe fn alloc_attrs(capacity: u32) -> *mut ArrayHeader {
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_global_n(
        "bytes.attrs_array",
        (capacity.max(1) as u64 + 1) * 8 + std::mem::size_of::<ArrayHeader>() as u64,
    );
    crate::array::js_array_alloc_key_list(capacity.max(1), false)
}

/// Write `entry` for `key` at `pos`, whose predecessors `0..pos` are already
/// written, and extend the written length to cover it. Allocation-free.
///
/// # Safety
/// `attrs` is a live attributes array with capacity past `pos`, `pos <=` its
/// written length, and `key` is the key at `pos` (live).
#[inline]
pub(crate) unsafe fn attrs_write(
    attrs: *mut ArrayHeader,
    pos: u32,
    entry: u8,
    key: crate::JSValue,
) {
    let (w, len) = words(attrs);
    debug_assert!((pos as usize) <= len && pos < (*attrs).capacity);
    let before = if pos == 0 {
        Word::EMPTY
    } else {
        Word::decode(*w.add(pos as usize - 1))
    };
    // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
    *w.add(pos as usize) = Word::after(before, entry, key).encode();
    if pos as usize == len {
        (*attrs).length = pos + 1;
    }
}

/// The key slot at `pos` of `keys`, or `undefined` past its initialized end.
#[inline]
unsafe fn key_at(keys: *const ArrayHeader, pos: u32) -> crate::JSValue {
    let (slots, len) = crate::object::keys_array_dense_slots(keys);
    if (pos as usize) < len {
        crate::JSValue::from_bits((*slots.add(pos as usize)).to_bits())
    } else {
        crate::JSValue::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// Rewrite the entry at `pos` of an OWNED list's attributes array and
/// recompute every cumulative word from there. O(length - pos).
///
/// # Safety
/// `keys` is live and exclusively owned, `attrs` its attributes array, and
/// `pos` is below the attributes' written length.
pub(crate) unsafe fn attrs_set_owned(
    keys: *const ArrayHeader,
    attrs: *mut ArrayHeader,
    pos: u32,
    entry: u8,
) {
    let (w, len) = words(attrs);
    debug_assert!((pos as usize) < len);
    // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
    *w.add(pos as usize) = Word {
        entry,
        ..Word::EMPTY
    }
    .encode();
    recompute_cumulative(keys, attrs, pos);
}

/// Recompute the cumulative words of `attrs` from position `from` on, reading
/// the keys from `keys`.
///
/// # Safety
/// `keys` is live and `attrs` its attributes array.
pub(crate) unsafe fn recompute_cumulative(
    keys: *const ArrayHeader,
    attrs: *mut ArrayHeader,
    from: u32,
) {
    let (w, len) = words(attrs);
    let mut prev = if from == 0 {
        Word::EMPTY
    } else {
        Word::decode(*w.add(from as usize - 1))
    };
    for i in from as usize..len {
        let entry = Word::decode(*w.add(i)).entry;
        prev = Word::after(prev, entry, key_at(keys, i as u32));
        // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
        *w.add(i) = prev.encode();
    }
}

/// Attach `attrs` to `keys`, whose attributes reserve must already exist.
/// Barriered: an old keys array may take a young attributes array.
///
/// # Safety
/// `keys` is a live, forwarding-resolved keys array carrying
/// `GC_ARRAY_NAMED_PROPS`; `attrs` is a live attributes array.
#[inline]
pub(crate) unsafe fn attach_attrs(keys: *mut ArrayHeader, attrs: *mut ArrayHeader) {
    debug_assert!(
        crate::array::array_object_flags_resolved(keys) & crate::gc::GC_ARRAY_NAMED_PROPS != 0
    );
    crate::array::store_named_props_word(
        keys,
        crate::value::js_nanbox_pointer(attrs as i64).to_bits(),
    );
}

/// Allocate a fresh key list with room for `capacity` keys, and — when
/// `with_attrs` — its attributes array, attached, with nothing written. The
/// canonical trie's and the copy sites' one allocator for a list that must
/// carry attributes.
///
/// # Safety
/// May collect: the caller roots everything it holds. Nothing is held across
/// the two allocations here except through a handle.
pub(crate) unsafe fn alloc_key_list(
    capacity: u32,
    all_ptr: bool,
    with_attrs: bool,
) -> *mut ArrayHeader {
    if !with_attrs {
        return crate::array::js_array_alloc_key_list(capacity, all_ptr);
    }
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_global_n(
        "bytes.key_list_with_attrs",
        (capacity as u64 + 2) * 8 + std::mem::size_of::<ArrayHeader>() as u64,
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let attrs = alloc_attrs(capacity);
    let attrs_handle = scope.root_raw_mut_ptr(attrs);
    let (keys, attrs) = attrs_handle.across_mut::<ArrayHeader, _>(|| {
        crate::array::js_array_alloc_key_list_reserved(capacity, all_ptr)
    });
    attach_attrs(keys, attrs);
    keys
}

/// Copy the entries of `src_keys` positions `from..from + n` into `dst_attrs`
/// at the SAME positions, which must be the written end of `dst_attrs`. A
/// source without attributes contributes default entries.
///
/// # Safety
/// Both arrays are live, `dst_attrs` has the capacity, and nothing allocates.
pub(crate) unsafe fn copy_entries(
    src_keys: *const ArrayHeader,
    from: u32,
    dst_attrs: *mut ArrayHeader,
    n: u32,
) {
    let src = keys_attrs(src_keys);
    let (sw, slen) = if src.is_null() {
        (std::ptr::null_mut(), 0)
    } else {
        words(src)
    };
    for pos in from..from + n {
        let entry = if (pos as usize) < slen {
            Word::decode(*sw.add(pos as usize)).entry
        } else {
            0
        };
        attrs_write(dst_attrs, pos, entry, key_at(src_keys, pos));
    }
}

// ---------------------------------------------------------------------------
// Object-level readers: what a receiver's own key's attributes are.
// ---------------------------------------------------------------------------

/// Is `addr` a heap object whose attributes live with its keys? The ONE
/// predicate that routes a descriptor operation to the keys instead of the
/// owner-keyed tables, so installs and reads cannot disagree about where an
/// owner's attributes are. Every other cell kind (arrays, closures, exotic
/// cells, typed arrays) keeps the tables for now.
///
/// # Safety
/// `addr` is any address; it is classified through the ownership-checking
/// header reader.
#[inline]
pub(crate) unsafe fn attrs_live_in_keys(addr: usize) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && crate::typedarray::lookup_typed_array_kind(addr).is_none()
}

/// The attribute summary of `obj`'s shape (0 when unshaped). One load.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_summary(obj: *const crate::object::ObjectHeader) -> u8 {
    crate::object::shapes::object_shape_record(obj)
        .map(|r| r.summary())
        .unwrap_or(0)
}

/// The entry `obj`'s own key `key` carries: 0 when the key is default or
/// absent. Three filters answer most keys without a lookup: the shape's
/// summary (an all-default receiver), then the key-list prefix's Bloom filter
/// (a key no entry was ever written for).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_key_entry(obj: *const crate::object::ObjectHeader, key: &[u8]) -> u8 {
    if object_summary(obj) & SUMMARY_KEY_BITS == 0 {
        return 0;
    }
    object_key_entry_filtered(obj, key, false)
}

/// Is `obj`'s own key `key` an accessor? The accessor filters answer most
/// keys alone: a receiver with a few accessors pays a lookup only for them.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_key_is_accessor(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> bool {
    if object_summary(obj) & SUMMARY_ACCESSOR == 0 {
        return false;
    }
    object_key_entry_filtered(obj, key, true) & ENTRY_ACCESSOR != 0
}

#[inline(never)]
unsafe fn object_key_entry_filtered(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
    accessor: bool,
) -> u8 {
    let keys = crate::object::object_keys(obj);
    if keys.is_null() || !keys_may_carry(keys.arr(), keys.count(), key, accessor) {
        return 0;
    }
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_global("read.key_entry_lookup");
    match crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), key) {
        Some(pos) => keys_entry(keys.arr(), pos),
        None => 0,
    }
}

/// [`object_key_entry`] for a key held as a string header. An undecodable key
/// reads as an accessor — the conservative answer for every caller (a read
/// declines to prime, a store declines).
///
/// # Safety
/// `obj` is a live `ObjectHeader`; `key` is null or a live string header.
#[inline]
pub(crate) unsafe fn object_key_entry_for_string(
    obj: *const crate::object::ObjectHeader,
    key: *const crate::StringHeader,
) -> u8 {
    if object_summary(obj) & SUMMARY_KEY_BITS == 0 {
        return 0;
    }
    if key.is_null() {
        return 0;
    }
    let boxed =
        crate::value::JSValue::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match crate::string::js_string_key_bytes(boxed, &mut sso) {
        Some(bytes) => object_key_entry_filtered(obj, bytes, false),
        None => ENTRY_ACCESSOR,
    }
}

/// Can a plain data store of `key` be intercepted by `obj` — is the key an
/// accessor or a non-writable data property there? A prototype whose summary
/// has neither answers without a key lookup: every class prototype whose
/// members are only non-enumerable methods.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_key_blocks_plain_store(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> bool {
    if object_summary(obj) & SUMMARY_BLOCKS_STORE == 0 {
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global("read.store_check_summary_clear");
        return false;
    }
    !entry_is_plain_writable_data(object_key_entry_filtered(obj, key, false))
}

// ---------------------------------------------------------------------------
// Mutation: every attribute change of a receiver whose attributes live with
// its keys goes through [`apply_edits`].
// ---------------------------------------------------------------------------

/// One attribute change, as the keys record it. Every descriptor mutation
/// describes itself with these, and [`apply_edits`] folds them into the
/// receiver's keys, so the successor layout is a function of WHAT changed.
#[derive(Clone, Copy, Debug)]
pub(crate) enum AttrsEdit<'a> {
    /// `key`'s data attributes become these `PropertyAttrs` bits (W=1, E=2,
    /// C=4). An accessor half is kept.
    Data(&'a [u8], u8),
    /// `key`'s data attributes return to the default. An accessor half is kept.
    ClearData(&'a [u8]),
    /// `key` becomes an accessor with a getter / a setter. Data attributes are
    /// kept.
    Accessor(&'a [u8], bool, bool),
    /// `key` stops being an accessor. Data attributes are kept.
    ClearAccessor(&'a [u8]),
    /// Every string key becomes non-configurable, and with `freeze` also
    /// non-writable (`Object.freeze` / `Object.seal`).
    Integrity { freeze: bool },
    /// Every key returns to the default.
    ClearAll,
}

impl AttrsEdit<'_> {
    /// The single key this edit is about, or `None` for a whole-list edit.
    #[inline]
    pub(crate) fn key(&self) -> Option<&[u8]> {
        match *self {
            AttrsEdit::Data(k, _)
            | AttrsEdit::ClearData(k)
            | AttrsEdit::Accessor(k, _, _)
            | AttrsEdit::ClearAccessor(k) => Some(k),
            AttrsEdit::Integrity { .. } | AttrsEdit::ClearAll => None,
        }
    }

    /// This edit applied to an entry.
    #[inline]
    pub(crate) fn apply(self, old: u8) -> u8 {
        match self {
            AttrsEdit::Data(_, bits) => (old & ENTRY_ACCESSOR_MASK) | attr_bits_to_entry(bits),
            AttrsEdit::ClearData(_) => old & ENTRY_ACCESSOR_MASK,
            AttrsEdit::Accessor(_, get, set) => {
                let mut e = (old & ENTRY_ATTR_MASK) | ENTRY_ACCESSOR;
                if get {
                    e |= ENTRY_HAS_GET;
                }
                if set {
                    e |= ENTRY_HAS_SET;
                }
                e
            }
            AttrsEdit::ClearAccessor(_) => old & ENTRY_ATTR_MASK,
            AttrsEdit::Integrity { freeze } => {
                old | ENTRY_NON_CONFIGURABLE | if freeze { ENTRY_NON_WRITABLE } else { 0 }
            }
            AttrsEdit::ClearAll => 0,
        }
    }
}

/// The key bytes of a keys-array slot, when it holds a string key.
#[inline]
unsafe fn slot_key_bytes<'b>(
    slot: crate::JSValue,
    sso: &'b mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<&'b [u8]> {
    crate::string::js_string_key_bytes(slot, sso)
}

/// The entry position `pos` of a list takes after `edits`, from `old`.
/// Whole-list edits apply to string keys only (a hole or a symbol slot keeps
/// its entry: symbol attributes are the symbol tables').
#[inline]
unsafe fn fold_edits(edits: &[AttrsEdit<'_>], slot: crate::JSValue, old: u8) -> u8 {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = slot_key_bytes(slot, &mut sso) else {
        return old;
    };
    let mut entry = old;
    for edit in edits {
        match edit.key() {
            Some(k) if k != bytes => {}
            _ => entry = edit.apply(entry),
        }
    }
    entry
}

/// Apply `edits` to `obj`'s keys: the one place an attribute change reaches
/// the layout.
///
/// * A key that is not yet own is CLAIMED with its entry — the attribute
///   always has a key, and the key enters the list in insertion order, where
///   a later `[[OwnPropertyKeys]]` expects it. A key arriving with its
///   attributes is one trie edge ([`canonical_keys::extend_key_with_entry`]),
///   so an `exports` object gaining a getter per re-export appends in place.
/// * Existing keys of a shared layout are REBUILT from the first changed
///   position ([`canonical_keys::rebuild_with_entries`]): the prefix is
///   shared, the rest re-appended — the successor layout is canonical, so
///   receivers making the same change share it.
/// * A dictionary receiver edits its private list in place and draws a fresh
///   dictionary generation, as for every other change to it.
///
/// Runs in a no-move window: the allocations here are a few small arrays,
/// and every caller of the descriptor installers holds the receiver as a raw
/// address across the call, as it always could.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` that is not a typed array.
pub(crate) unsafe fn apply_edits(obj: *mut crate::object::ObjectHeader, edits: &[AttrsEdit<'_>]) {
    if edits.is_empty() || !crate::object::object_is_shaped(obj) {
        return;
    }
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_kind("edit.apply", obj as usize);
    let _no_move = crate::gc::GcSuppressScope::new();
    // 1. Claim absent keys, each with its folded entry.
    for (i, edit) in edits.iter().enumerate() {
        let Some(key) = edit.key() else {
            continue;
        };
        if edits[..i].iter().any(|e| e.key() == Some(key)) {
            continue; // folded with its first occurrence
        }
        let keys = crate::object::object_keys(obj);
        if !keys.is_null()
            && crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), key).is_some()
        {
            continue;
        }
        let entry = edits
            .iter()
            .filter(|e| e.key().is_none() || e.key() == Some(key))
            .fold(0u8, |acc, e| e.apply(acc));
        if entry == 0 {
            // A default entry on an absent key changes nothing: the key is not
            // own, and when it becomes own it is born default.
            continue;
        }
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_kind("edit.claim_absent_key", obj as usize);
        let name = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::object_ops::ensure_key_in_keys_array_with_entry(obj, name, entry);
    }
    // 2. Existing keys: which positions change, and to what.
    let keys = crate::object::object_keys(obj);
    if keys.is_null() {
        return;
    }
    let (slots, available) = crate::object::keys_array_dense_slots(keys.arr());
    let count = (keys.count() as usize).min(available);
    let changes_at = |pos: usize| -> bool {
        let slot = crate::JSValue::from_bits((*slots.add(pos)).to_bits());
        let old = keys_entry(keys.arr(), pos as u32);
        fold_edits(edits, slot, old) != old
    };
    let mut first_change: Option<u32> = None;
    if edits.iter().all(|e| e.key().is_some()) {
        // Keyed edits touch only their keys' positions: find them, rather
        // than walk the list (an install on a wide object is one lookup).
        for edit in edits {
            let key = edit.key().unwrap_or_default();
            if let Some(pos) = crate::object::keys_find_slot_by_bytes(keys.arr(), count as u32, key)
            {
                if first_change.map_or(true, |f| pos < f) && changes_at(pos as usize) {
                    first_change = Some(pos);
                }
            }
        }
    } else {
        first_change = (0..count)
            .find(|&pos| changes_at(pos))
            .map(|pos| pos as u32);
    }
    let Some(from) = first_change else {
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global("edit.no_change");
        return;
    };
    if crate::object::dictionary::is_dictionary(obj) {
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global("edit.dictionary_in_place");
        edit_private_list(obj, keys.arr(), from, count as u32, edits);
        crate::object::shapes::transition_object_shape_semantics(obj);
        return;
    }
    let Some(proof) = crate::object::canonical_keys::SharedLayout::of_receiver(obj) else {
        return;
    };
    let rebuilt =
        crate::object::canonical_keys::rebuild_with_entries(&proof, keys, from, |_, slot, old| {
            fold_edits(edits, slot, old)
        });
    crate::object::set_object_keys(obj, rebuilt.view());
}

/// Edit a dictionary receiver's PRIVATE list in place from position `from`.
unsafe fn edit_private_list(
    obj: *mut crate::object::ObjectHeader,
    keys: *mut ArrayHeader,
    from: u32,
    count: u32,
    edits: &[AttrsEdit<'_>],
) {
    let mut keys = ensure_owned_attrs(keys, count);
    if count > 0 {
        // Cover every position, whatever appended to the list before.
        let last = keys_entry(keys, count - 1);
        keys = owned_note_append(keys, count - 1, last);
    }
    crate::object::dictionary::replace_private_keys(obj, keys);
    let attrs = keys_attrs(keys);
    let (slots, _) = crate::object::keys_array_dense_slots(keys);
    let (w, _) = words(attrs);
    for pos in from..count {
        let slot = crate::JSValue::from_bits((*slots.add(pos as usize)).to_bits());
        let old = Word::decode(*w.add(pos as usize)).entry;
        let new = fold_edits(edits, slot, old);
        // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
        *w.add(pos as usize) = Word {
            entry: new,
            ..Word::EMPTY
        }
        .encode();
    }
    recompute_cumulative(keys, attrs, from);
}

/// Give an OWNED key list (a dictionary receiver's private list) an
/// attributes array covering its first `count` positions, all default, if it
/// has none, with room for the list's capacity. Returns the list's live head:
/// taking the reserve slot moves the elements up one slot inside the
/// allocation, or grows it when it is full.
///
/// # Safety
/// `keys` is a live, exclusively owned keys array. May allocate: the caller
/// roots what it holds (or runs in a no-move window).
pub(crate) unsafe fn ensure_owned_attrs(keys: *mut ArrayHeader, count: u32) -> *mut ArrayHeader {
    let keys = crate::array::clean_arr_ptr_mut(keys);
    if !keys_attrs(keys).is_null() {
        return keys;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_handle = scope.root_raw_mut_ptr(keys);
    let capacity = (*keys).capacity.max(count);
    let (attrs, keys) = keys_handle.across_mut::<ArrayHeader, _>(|| alloc_attrs(capacity + 1));
    let attrs_handle = scope.root_raw_mut_ptr(attrs);
    let (keys, attrs) =
        attrs_handle.across_mut::<ArrayHeader, _>(|| crate::array::ensure_named_props_slot(keys));
    assert!(
        !keys.is_null(),
        "an owned key list must be able to take its attributes reserve"
    );
    for pos in 0..count {
        attrs_write(attrs, pos, 0, key_at(keys, pos));
    }
    attach_attrs(keys, attrs);
    keys
}

/// Record the entry of the key an OWNED list just appended at `pos`. A list
/// without attributes stays without them for a default entry. Returns the
/// list's live head (see [`ensure_owned_attrs`]).
///
/// # Safety
/// As [`ensure_owned_attrs`]; `pos` is the appended key's position, and the
/// list holds `pos + 1` keys.
pub(crate) unsafe fn owned_note_append(
    keys: *mut ArrayHeader,
    pos: u32,
    entry: u8,
) -> *mut ArrayHeader {
    let mut keys = crate::array::clean_arr_ptr_mut(keys);
    let mut attrs = keys_attrs(keys);
    if attrs.is_null() {
        if entry == 0 {
            return keys;
        }
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global("keys.owned_attach_attrs");
        keys = ensure_owned_attrs(keys, pos);
        attrs = keys_attrs(keys);
    }
    let written = (*attrs).length;
    if pos >= (*attrs).capacity {
        // Full: copy the prefix into a larger array.
        let scope = crate::gc::RuntimeHandleScope::new();
        let keys_handle = scope.root_raw_mut_ptr(keys);
        let old_handle = scope.root_raw_mut_ptr(attrs);
        let capacity = (pos + 1).max((*keys).capacity).max(pos + pos / 2 + 1);
        let ((fresh, keys_now), old) = old_handle.across_mut::<ArrayHeader, _>(|| {
            keys_handle.across_mut::<ArrayHeader, _>(|| alloc_attrs(capacity))
        });
        let keep = written.min(pos);
        let (src, _) = words(old);
        for i in 0..keep {
            attrs_write(
                fresh,
                i,
                Word::decode(*src.add(i as usize)).entry,
                key_at(keys_now, i),
            );
        }
        keys = keys_now;
        attach_attrs(keys, fresh);
        attrs = fresh;
    }
    if (*attrs).length > pos {
        // Entries past `pos` belonged to keys the list no longer has.
        (*attrs).length = pos;
    }
    for i in (*attrs).length..pos {
        attrs_write(attrs, i, 0, key_at(keys, i));
    }
    attrs_write(attrs, pos, entry, key_at(keys, pos));
    keys
}

/// An OWNED list removed position `pos` by shifting its tail down one slot:
/// shift the entries with it.
///
/// # Safety
/// `keys` is a live, exclusively owned keys array whose key shift is done.
pub(crate) unsafe fn owned_note_remove(keys: *mut ArrayHeader, pos: u32) {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return;
    }
    let (w, len) = words(attrs);
    if pos as usize >= len {
        return;
    }
    // GC_STORE_AUDIT(POINTER_FREE): INT32 boxes moving inside a pointer-free array.
    std::ptr::copy(
        w.add(pos as usize + 1),
        w.add(pos as usize),
        len - pos as usize - 1,
    );
    (*attrs).length = len as u32 - 1;
    recompute_cumulative(keys, attrs, pos);
}

/// An OWNED list's position `pos` became a hole: its entry is the default.
///
/// # Safety
/// `keys` is a live, exclusively owned keys array.
pub(crate) unsafe fn owned_note_hole(keys: *mut ArrayHeader, pos: u32) {
    let attrs = keys_attrs(keys);
    if attrs.is_null() || pos >= (*attrs).length {
        return;
    }
    attrs_set_owned(keys, attrs, pos, 0);
}

/// An OWNED list was compacted in place: the entry of old position
/// `mapping[i]` now belongs to position `i`. `mapping` is increasing.
///
/// # Safety
/// `keys` is a live, exclusively owned keys array.
pub(crate) unsafe fn owned_note_compaction(keys: *mut ArrayHeader, mapping: &[u32]) {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return;
    }
    let (w, len) = words(attrs);
    for (to, &from) in mapping.iter().enumerate() {
        let entry = if (from as usize) < len {
            Word::decode(*w.add(from as usize)).entry
        } else {
            0
        };
        // GC_STORE_AUDIT(POINTER_FREE): INT32 boxes inside a pointer-free array.
        *w.add(to) = Word {
            entry,
            ..Word::EMPTY
        }
        .encode();
    }
    (*attrs).length = mapping.len() as u32;
    recompute_cumulative(keys, attrs, 0);
}

#[cfg(test)]
#[path = "key_attrs_tests.rs"]
mod key_attrs_tests;
