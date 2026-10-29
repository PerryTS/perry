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
//! | [`ENTRY_PRIVATE`] | a private field (#11791), never a property |
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
//! physical slot, in front of logical element 0, with `GC_ARRAY_KEY_ATTRS`
//! set: exactly the reserve #10166 gives an Array's named properties
//! (`array/named_props.rs`). So nothing new exists for the collector: the
//! reserve word is already emitted as a fixed child slot of every flagged
//! array (`gc::layout_slot_visit`), `js_array_grow` already re-reserves and
//! copies it, and every element reader already adds the front offset
//! (`array_elements_ptr`, #10939). The front slot count is
//! [`KEYS_ATTRS_FRONT_SLOTS`]; nothing may hand-type it.
//!
//! # Why reusing `GC_ARRAY_KEY_ATTRS` on a keys array is sound
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
/// The key is a class's private field (#11791), named by its qualified
/// private name: an element of the receiver, not a property. A private field
/// is never deleted, so the entry being in the shape's list IS the field being
/// initialized. Always written as [`PRIVATE_FIELD_ENTRY`].
pub(crate) const ENTRY_PRIVATE: u8 = 0x40;
/// A private field's entry: also non-enumerable, so every enumerable-only
/// reader (`Object.keys`, `for…in`, `JSON.stringify`, spread, `assign`)
/// skips it by the attribute it already honors. Readers that list or probe
/// non-enumerable keys ask [`entry_is_private`].
pub(crate) const PRIVATE_FIELD_ENTRY: u8 = ENTRY_PRIVATE | ENTRY_NON_ENUMERABLE;
/// The key's storage slot may hold the hole marker, and then the key is
/// ABSENT (object model §3.2: presence is the hole marker in the slot). Only a
/// layout key a descriptor declares optional or late carries it; every
/// presence, order and value reader asks [`super::key_view`].
pub(crate) const ENTRY_OPTIONAL: u8 = crate::codegen_abi::LAYOUT_KEY_OPTIONAL;
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
/// Some key is a private field ([`ENTRY_PRIVATE`]).
pub(crate) const SUMMARY_PRIVATE: u8 = 0x10;
/// Some key's slot may hold the hole marker ([`ENTRY_OPTIONAL`]): the layout
/// may have holes, and its keys are not all present (`key_view`). A list
/// without it keeps every key present, so no reader pays for holes.
pub(crate) const SUMMARY_HOLES: u8 = 0x20;
/// Every per-key summary bit: a list with none of them is all default (and
/// every key of it is present).
pub(crate) const SUMMARY_KEY_BITS: u8 = SUMMARY_ACCESSOR
    | SUMMARY_NON_WRITABLE
    | SUMMARY_NON_ENUMERABLE
    | SUMMARY_NON_CONFIGURABLE
    | SUMMARY_PRIVATE
    | SUMMARY_HOLES;
/// Attribute-word bit, not a shape summary bit: some key up to here keeps a
/// storage slot other than its position. Such a list comes from a layout
/// descriptor whose slots are a permutation of its key positions. A key
/// added later takes its position as its slot, which is still free; a delete
/// moves the receiver to its ordered overflow table.
const SUMMARY_SLOTS: u8 = 0x40;
/// Attribute-word bit for this position only: the Bloom half holds its slot.
const WORD_SLOT: u8 = 0x80;
/// Bits a plain data store must see clear on every hop of a prototype chain.
pub(crate) const SUMMARY_BLOCKS_STORE: u8 = SUMMARY_ACCESSOR | SUMMARY_NON_WRITABLE;
/// Bits that keep a receiver's own key from a store that overwrites its slot
/// in place (a store cache, an own-store fast lane): [`SUMMARY_BLOCKS_STORE`]
/// and a key whose slot may hold the hole, where the store is an add
/// (`layout_desc::family_store`). See [`entry_is_plain_overwrite`].
pub(crate) const SUMMARY_BLOCKS_OVERWRITE: u8 = SUMMARY_BLOCKS_STORE | SUMMARY_HOLES;

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
    if entry & ENTRY_PRIVATE != 0 {
        s |= SUMMARY_PRIVATE;
    }
    if entry & ENTRY_OPTIONAL != 0 {
        s |= SUMMARY_HOLES;
    }
    s
}

/// Is a key with this entry a private field rather than a property?
#[inline]
pub(crate) const fn entry_is_private(entry: u8) -> bool {
    entry & ENTRY_PRIVATE != 0
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
    entry & (ENTRY_ACCESSOR | ENTRY_NON_WRITABLE | ENTRY_PRIVATE) == 0
}

/// May a store to the own key with `entry` overwrite its slot in place (a
/// store cache's way, an own-store fast lane)? Plain writable data that is
/// not optional: an optional key's slot may hold the hole, and a store then
/// adds the key (`layout_desc::family_store` decides the order).
pub(crate) const fn entry_is_plain_overwrite(entry: u8) -> bool {
    entry_is_plain_writable_data(entry) && entry & ENTRY_OPTIONAL == 0
}

/// One element of an attributes array: the entry at this position, and the
/// CUMULATIVE facts of positions `0..=i` — the summary, and two 16-bit Bloom
/// filters over the keys that carry a non-default entry / an accessor. The
/// cumulative words make a prefix's answers one load: the summary of a shape
/// naming `count` keys is element `count - 1`'s, and so are its filters.
/// A key with its own storage slot ([`WORD_SLOT`]) keeps the slot in the
/// Bloom half instead; the positions after it carry full filters.
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

    /// The storage slot this position keeps, if it is not its position.
    #[inline(always)]
    const fn slot(self) -> Option<u32> {
        if self.summary & WORD_SLOT != 0 {
            Some(self.entry_bloom as u32 | (self.accessor_bloom as u32) << 16)
        } else {
            None
        }
    }

    #[inline]
    fn with_slot(mut self, slot: Option<u32>) -> Word {
        if let Some(slot) = slot {
            self.summary |= WORD_SLOT | SUMMARY_SLOTS;
            self.entry_bloom = slot as u16;
            self.accessor_bloom = (slot >> 16) as u16;
        }
        self
    }

    /// This position's word: `entry` for `key`, after the cumulative `prev`.
    #[inline(always)]
    unsafe fn after(prev: Word, entry: u8, key: impl AttributeKey) -> Word {
        Word::decode(perry_abi::canonical_key_attribute_word(
            prev.encode(),
            entry,
            if entry != 0 { key.bloom_bit() } else { 0 },
            None,
        ))
    }
}

/// The key whose attribute word is being written. A builder that has just
/// stored a string key can pass its original text; both forms feed the same
/// cumulative word, without rereading a fresh array's key slot.
pub(crate) trait AttributeKey {
    fn bloom_bit(self) -> u16;
}

impl AttributeKey for crate::JSValue {
    #[inline]
    fn bloom_bit(self) -> u16 {
        // SAFETY: the writer's key is live, as required by attrs_write.
        unsafe { key_bloom_bit(self) }
    }
}

impl AttributeKey for &str {
    #[inline]
    fn bloom_bit(self) -> u16 {
        bloom_bit_of_bytes(self.as_bytes())
    }
}

// A declared key's hash is already static data. Registration uses the
// same Bloom bit as every dynamic writer without hashing its bytes again.
impl AttributeKey for &super::layout_desc::StaticAtom {
    #[inline]
    fn bloom_bit(self) -> u16 {
        if self.kind == crate::codegen_abi::ATOM_KIND_STRING {
            1u16 << (self.hash >> 60)
        } else {
            0
        }
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
#[inline(always)]
pub(crate) unsafe fn keys_attrs(keys: *const ArrayHeader) -> *mut ArrayHeader {
    if keys.is_null() {
        return std::ptr::null_mut();
    }
    let header = crate::gc::header_from_trusted_user_ptr(keys.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return keys_attrs_forwarded(keys);
    }
    keys_attrs_resolved(keys)
}

// A keys list may still name its pre-growth head. Keep all of that resolution
// work on the rare edge so ordinary shape reads need no forwarding frame.
#[cold]
#[inline(never)]
unsafe fn keys_attrs_forwarded(keys: *const ArrayHeader) -> *mut ArrayHeader {
    // Keys metadata is always a real array, never a Proxy or lazy JSON
    // receiver. Follow its GC/growth forwarding words without invoking the
    // arbitrary-array resolver, whose JSON arm can allocate and call JS.
    let mut keys = keys;
    loop {
        let header = crate::gc::header_from_trusted_user_ptr(keys.cast());
        if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
            break;
        }
        keys = crate::gc::forwarding_address(header).cast::<ArrayHeader>();
        if keys.is_null() {
            return std::ptr::null_mut();
        }
    }
    keys_attrs_resolved(keys)
}

#[inline(always)]
unsafe fn keys_attrs_resolved(keys: *const ArrayHeader) -> *mut ArrayHeader {
    if crate::array::array_object_flags_resolved(keys) & crate::gc::GC_ARRAY_KEY_ATTRS == 0 {
        return std::ptr::null_mut();
    }
    let word = *crate::array::key_attrs_slot(keys);
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
    if crate::object::overflow::is_table(keys) {
        return crate::object::overflow::entry_attrs(keys, pos);
    }

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

/// Find an accessor candidate in a live shape's key prefix. Data names need
/// not be decoded on a negative accessor walk. A positive candidate still
/// checks the complete lookup: a later duplicate data key shadows it.
///
/// # Safety
/// `keys` belongs to a live shape, with no intervening allocation or safepoint.
pub(crate) unsafe fn keys_find_accessor_slot_resolved(
    keys: *const ArrayHeader,
    count: u32,
    key: &[u8],
) -> Option<u32> {
    if crate::object::overflow::is_table(keys) {
        let pos = crate::object::overflow::find_bytes(keys.cast(), key)?;
        return (keys_entry(keys, pos) & ENTRY_ACCESSOR != 0).then_some(pos);
    }

    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return None;
    }
    let (entries, entry_len) = words(attrs);
    let (slots, slot_len) = crate::object::keys_array_dense_slots_resolved(keys);
    if slots.is_null() {
        return None;
    }
    let n = (count as usize).min(entry_len).min(slot_len);
    // Prefix summaries are cumulative: everything before the first prefix
    // carrying SUMMARY_ACCESSOR is data. Method-heavy prototypes often put
    // their few accessors last, so find that boundary without scanning every
    // preceding attribute entry.
    let (mut first, mut end) = (0, n);
    while first < end {
        let mid = first + (end - first) / 2;
        if Word::decode(*entries.add(mid)).summary & SUMMARY_ACCESSOR == 0 {
            first = mid + 1;
        } else {
            end = mid;
        }
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in (first..n).rev() {
        if Word::decode(*entries.add(i)).entry & ENTRY_ACCESSOR == 0 {
            continue;
        }
        let value = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if crate::string::js_string_key_bytes(value, &mut sso) == Some(key) {
            return (crate::object::keys_find_slot_by_bytes_resolved(
                crate::object::ObjectKeys::new(keys as *mut _, count),
                count,
                key,
            ) == Some(i as u32))
            .then_some(i as u32);
        }
    }
    None
}

/// Is key position `pos` of `keys` an accessor — does its value slot hold an
/// accessor PAIR rather than a data value (`accessor_pair.rs`)? Every reader
/// that walks an object's slots by position must ask before treating a slot
/// as data. One flag load for a list without attributes.
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn key_is_accessor_at(keys: *const ArrayHeader, pos: u32) -> bool {
    keys_entry(keys, pos) & ENTRY_ACCESSOR != 0
}

/// `obj`'s slot `idx` as a DATA value for a reader that walks slots by
/// position without running user code: `undefined` for an accessor key,
/// whose slot holds its accessor pair (`accessor_pair.rs`) — the value such
/// a slot always read as before the pair moved into it.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_slot_data(
    obj: *const crate::object::ObjectHeader,
    idx: u32,
) -> crate::JSValue {
    let (keys, live) = crate::object::object_keys_and_live_slot_count(obj);
    if key_is_accessor_at(keys.arr(), idx) {
        return crate::JSValue::undefined();
    }
    crate::object::field_get_set::object_key_field_at_with_keys(obj, keys.arr(), idx, live)
}

/// [`object_slot_data`] as the raw `f64` word.
///
/// # Safety
/// As [`object_slot_data`].
#[inline]
pub(crate) unsafe fn object_slot_data_f64(
    obj: *const crate::object::ObjectHeader,
    idx: u32,
) -> f64 {
    f64::from_bits(object_slot_data(obj, idx).bits())
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

/// Does some key among the first `count` keep its own storage slot? Such a
/// list keeps its attributes array even when every entry is default. An
/// ordered overflow table names each entry's slot, so it answers yes.
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_have_slots(keys: *const ArrayHeader, count: u32) -> bool {
    if count == 0 || (keys as usize) < perry_abi::RECEIVER_HANDLE_FLOOR || (keys as u64) >> 48 != 0
    {
        return false;
    }
    // One header load answers a table and a list without attributes.
    let header = &*crate::gc::header_from_trusted_user_ptr(keys.cast());
    if header.obj_type == crate::gc::GC_TYPE_OVERFLOW {
        return true;
    }
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && (header.obj_type != crate::gc::GC_TYPE_ARRAY
            || header._reserved & crate::gc::GC_ARRAY_KEY_ATTRS == 0)
    {
        return false;
    }
    let attrs = keys_attrs(keys);
    !attrs.is_null() && prefix_word(attrs, count).summary & SUMMARY_SLOTS != 0
}

/// Must a list extending the first `count` keys carry an attributes array:
/// does some key carry an entry or its own storage slot?
///
/// # Safety
/// As [`keys_attrs`].
#[inline]
pub(crate) unsafe fn keys_need_attrs(keys: *const ArrayHeader, count: u32) -> bool {
    if count == 0 {
        return false;
    }
    let attrs = keys_attrs(keys);
    !attrs.is_null() && prefix_word(attrs, count).summary & (SUMMARY_KEY_BITS | SUMMARY_SLOTS) != 0
}

/// [`keys_have_slots`] through [`keys_attrs_checked`].
///
/// # Safety
/// `keys` is any address.
#[cfg(test)]
#[inline]
pub(crate) unsafe fn keys_have_slots_checked(keys: *const ArrayHeader, count: u32) -> bool {
    if count == 0 {
        return false;
    }
    let attrs = keys_attrs_checked(keys);
    !attrs.is_null() && prefix_word(attrs, count).summary & SUMMARY_SLOTS != 0
}

/// Debug invariant of every lane that reads key position `i` as storage slot
/// `i`: each of the first `count` positions of `keys` is its own slot. It
/// reads every position's own word, not the cumulative summary a lane's gate
/// trusts, so a reader that misses the slot mapping fails here in debug
/// builds. Release builds compile it away.
///
/// # Safety
/// `keys` is any address.
#[inline(always)]
pub(crate) unsafe fn debug_assert_positional(keys: *const ArrayHeader, count: u32) {
    #[cfg(debug_assertions)]
    assert_positional(keys, count);
    #[cfg(not(debug_assertions))]
    let _ = (keys, count);
}

#[cfg(debug_assertions)]
#[inline(never)]
unsafe fn assert_positional(keys: *const ArrayHeader, count: u32) {
    if count == 0 || crate::object::overflow::is_table(keys) {
        return;
    }
    let attrs = keys_attrs_checked(keys);
    if attrs.is_null() {
        return;
    }
    let (w, len) = words(attrs);
    for pos in 0..(count as usize).min(len) {
        if let Some(slot) = Word::decode(*w.add(pos))
            .slot()
            .filter(|&s| s != pos as u32)
        {
            panic!("positional lane on a layout whose key {pos} keeps storage slot {slot}");
        }
    }
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
    if crate::object::overflow::is_table(keys) {
        return true;
    }

    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return false;
    }
    let w = prefix_word(attrs, count);
    if w.summary & WORD_SLOT != 0 {
        return true;
    }
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
    if crate::object::overflow::is_table(keys) {
        return (0..count).any(|i| crate::object::overflow::entry_attrs(keys, i) != 0);
    }

    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return false;
    }
    let (w, len) = words(attrs);
    (0..(count as usize).min(len)).any(|i| {
        let word = Word::decode(*w.add(i));
        word.entry != 0 || word.slot().is_some()
    })
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
    key: impl AttributeKey,
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

/// Give the key just written at `pos` its own storage `slot`.
///
/// # Safety
/// As [`attrs_write`], after it wrote `pos`.
#[inline]
pub(crate) unsafe fn attrs_write_slot(attrs: *mut ArrayHeader, pos: u32, slot: u32) {
    let (w, _) = words(attrs);
    // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
    *w.add(pos as usize) = Word::decode(*w.add(pos as usize))
        .with_slot(Some(slot))
        .encode();
}

/// The attribute word of key position `pos` of `keys` (0 past its end). Its
/// low byte is the entry, and [`slot_of_word`] reads the storage slot from
/// it: a reader that loads the entry gets the slot from the same load. An
/// ordered overflow table answers with its entry alone: its readers and
/// writers address entries by position (`overflow::get_from`).
///
/// # Safety
/// As [`keys_attrs`].
#[inline(always)]
pub(crate) unsafe fn key_word(keys: *const ArrayHeader, pos: u32) -> u64 {
    if crate::object::overflow::is_table(keys) {
        return crate::object::overflow::entry_attrs(keys, pos) as u64;
    }
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return 0;
    }
    let (w, len) = words(attrs);
    if (pos as usize) < len {
        *w.add(pos as usize)
    } else {
        0
    }
}

/// The storage slot of key position `pos`, from its attribute word.
#[inline(always)]
pub(crate) const fn slot_of_word(word: u64, pos: u32) -> u32 {
    match Word::decode(word).slot() {
        Some(slot) => slot,
        None => pos,
    }
}

/// The canonical key trie's identity of position `pos`'s word: its entry,
/// and its own storage slot when it keeps one. Zero is the default.
#[inline(always)]
pub(crate) const fn token_of_word(word: u64) -> u64 {
    let w = Word::decode(word);
    match w.slot() {
        Some(slot) => w.entry as u64 | (slot as u64 + 1) << 8,
        None => w.entry as u64,
    }
}

/// [`attrs_write`] for a trie token ([`token_of_word`]).
///
/// # Safety
/// As [`attrs_write`].
#[inline]
pub(crate) unsafe fn attrs_write_token(
    attrs: *mut ArrayHeader,
    pos: u32,
    token: u64,
    key: impl AttributeKey,
) {
    attrs_write(attrs, pos, token as u8, key);
    if token >> 8 != 0 {
        attrs_write_slot(attrs, pos, (token >> 8) as u32 - 1);
    }
}

/// The storage slot of key position `pos` of `keys`, or `None` for an
/// ordered overflow table, whose entries are addressed by position
/// (`overflow::get_from`). One header load answers a table and a list without
/// attributes, which keeps no slots.
///
/// # Safety
/// As [`keys_attrs`].
#[inline(always)]
pub(crate) unsafe fn list_storage_slot(keys: *const ArrayHeader, pos: u32) -> Option<u32> {
    if (keys as usize) < perry_abi::RECEIVER_HANDLE_FLOOR || (keys as u64) >> 48 != 0 {
        return Some(pos);
    }
    let header = &*crate::gc::header_from_trusted_user_ptr(keys.cast());
    if header.obj_type == crate::gc::GC_TYPE_OVERFLOW {
        return None;
    }
    if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && (header.obj_type != crate::gc::GC_TYPE_ARRAY
            || header._reserved & crate::gc::GC_ARRAY_KEY_ATTRS == 0)
    {
        return Some(pos);
    }
    Some(attributed_storage_slot(keys, pos))
}

/// [`list_storage_slot`] for a list with attributes: one shared helper, so
/// each reader inlines only the header test.
#[inline(never)]
unsafe fn attributed_storage_slot(keys: *const ArrayHeader, pos: u32) -> u32 {
    let attrs = keys_attrs(keys);
    if attrs.is_null() {
        return pos;
    }
    let (w, len) = words(attrs);
    if (pos as usize) < len {
        slot_of_word(*w.add(pos as usize), pos)
    } else {
        pos
    }
}

/// The storage slot of key position `pos` of `keys`. An ordered overflow
/// table answers the position: its entries are addressed by position.
///
/// # Safety
/// As [`keys_attrs`].
#[inline(always)]
pub(crate) unsafe fn key_storage_slot(keys: *const ArrayHeader, pos: u32) -> u32 {
    list_storage_slot(keys, pos).unwrap_or(pos)
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
    .with_slot(Word::decode(*w.add(pos as usize)).slot())
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
        let old = Word::decode(*w.add(i));
        prev = Word::after(prev, old.entry, key_at(keys, i as u32)).with_slot(old.slot());
        // GC_STORE_AUDIT(POINTER_FREE): an INT32 box into a pointer-free array.
        *w.add(i) = prev.encode();
    }
}

/// Attach `attrs` to `keys`, whose attributes reserve must already exist.
/// Barriered: an old keys array may take a young attributes array.
///
/// # Safety
/// `keys` is a live, forwarding-resolved keys array carrying
/// `GC_ARRAY_KEY_ATTRS`; `attrs` is a live attributes array.
#[inline]
pub(crate) unsafe fn attach_attrs(keys: *mut ArrayHeader, attrs: *mut ArrayHeader) {
    debug_assert!(
        crate::array::array_object_flags_resolved(keys) & crate::gc::GC_ARRAY_KEY_ATTRS != 0
    );
    crate::array::store_key_attrs_word(
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

/// Allocate both halves of the I3 metadata in stationary old storage.
/// The attributes prefix is immutable with its keys prefix, so keeping a
/// pointer-free young child only made the reserve dirty on every new layout.
pub(crate) unsafe fn alloc_canonical_key_list(
    capacity: u32,
    all_ptr: bool,
    with_attrs: bool,
) -> *mut ArrayHeader {
    if !with_attrs {
        return crate::array::js_array_alloc_canonical_keys(capacity, all_ptr, false);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let attrs = crate::array::js_array_alloc_canonical_keys(capacity.max(1), false, false);
    let handle = scope.root_raw_mut_ptr(attrs);
    let (keys, attrs) = handle.across_mut::<ArrayHeader, _>(|| {
        crate::array::js_array_alloc_canonical_keys(capacity, all_ptr, true)
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
#[inline]
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
        let word = if (pos as usize) < slen {
            Word::decode(*sw.add(pos as usize))
        } else {
            Word::EMPTY
        };
        attrs_write(dst_attrs, pos, word.entry, key_at(src_keys, pos));
        if let Some(slot) = word.slot() {
            attrs_write_slot(dst_attrs, pos, slot);
        }
    }
}

// ---------------------------------------------------------------------------
// Object-level readers: what a receiver's own key's attributes are.
// ---------------------------------------------------------------------------

/// Is the descriptor owner named by `bits` an ordinary property holder whose
/// attributes live in its keys? Every receiver kind normalizes its
/// descriptor storage to such a holder.
///
/// A header is decoded only for a word proven GC-headed, through the one
/// canonical classifier (`addr_class::tracked_header_of_bits`): a
/// `POINTER_TAG` word is honest and answers from its header; a raw untagged
/// word needs the allocator's proof first; anything else has no header.
/// Descriptor owners can be arbitrary native addresses — a native `Box`
/// backing (an `AsyncResource`'s, #11258) is heap-plausible and the bytes
/// before it may decode as an object header (#12513) — so a raw word is
/// never read on faith, on lookups and installs alike. A caller holding the
/// owner's JS value passes its tagged bits and pays one header load.
#[inline]
pub(crate) fn attrs_live_in_keys_of_bits(bits: u64) -> bool {
    let Some(header) = crate::value::addr_class::tracked_header_of_bits(bits) else {
        return false;
    };
    // SAFETY: the classifier proved a live GC header.
    let header = unsafe { header.as_ref() };
    crate::gc_type_is_object!(header.obj_type)
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && crate::typedarray_hook_slots::lookup_typed_array_kind(
            (bits & crate::value::POINTER_MASK) as usize,
        )
        .is_none()
}

/// [`attrs_live_in_keys_of_bits`] for a raw owner address of unknown
/// provenance: the allocator proves the header before an OBJECT answer.
///
/// Most raw owners that reach here are arrays, closures and other kinds
/// whose own bytes already deny being an OBJECT, so the bounded, alignment-
/// and slab-checked read (`try_read_gc_header`) rejects them first. Those
/// bytes can only make the answer `false`, which is also the answer for a
/// word with no header; a positive claim is never trusted without the
/// allocator's proof (#12513).
///
/// # Safety
/// None beyond the classifiers'; `addr` may be any word. Kept `unsafe` for
/// the descriptor-owner call sites' contracts.
#[inline]
pub(crate) unsafe fn attrs_live_in_keys(addr: usize) -> bool {
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(claim) if crate::gc_type_is_object!(claim.obj_type) => {
            attrs_live_in_keys_of_bits(addr as u64)
        }
        _ => false,
    }
}

/// [`attrs_live_in_keys`] at a site that WRITES the owner (its keys, slots or
/// header). Lookups and installs now share the same allocator proof for a raw
/// address; the name stays to mark the write sites.
///
/// # Safety
/// `addr` is any address.
#[inline]
pub(crate) unsafe fn attrs_live_in_keys_for_install(addr: usize) -> bool {
    attrs_live_in_keys(addr)
}

/// The attribute summary of `obj`'s shape (0 when unshaped). One load.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_summary(obj: *const crate::object::ObjectHeader) -> u8 {
    let t = crate::object::overflow::table(obj);
    if !t.is_null() {
        return (*t).summary as u8;
    }

    crate::object::shapes::object_shape_record(obj)
        .map(|r| r.summary())
        .unwrap_or(0)
}

/// Is `obj`'s own key `key` a private field ([`ENTRY_PRIVATE`])? The shape's
/// summary answers every receiver with no private field in one load.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_key_is_private(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> bool {
    if object_summary(obj) & SUMMARY_PRIVATE == 0 {
        return false;
    }
    let keys = crate::object::object_keys(obj);
    !keys.is_null()
        && crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), key).is_some()
        && crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key).is_none()
}

/// Private-field presence, independent of an equally spelled public property.
pub(crate) unsafe fn object_key_has_private_entry(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> bool {
    if object_summary(obj) & SUMMARY_PRIVATE == 0 {
        return false;
    }
    let keys = crate::object::object_keys(obj);
    if keys.is_null() {
        return false;
    }
    let Some(pos) = crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), key)
    else {
        return false;
    };
    // An overflow table lists present keys only.
    crate::object::overflow::has_overflow(obj)
        || private_element_present(obj, keys_entry(keys.arr(), pos), keys.storage_slot(pos))
}

/// The storage slot of private element `key` when `obj`'s key list holds it
/// as optional (a class family's: present per object,
/// `private_element_present`).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn object_optional_private_slot(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> Option<u32> {
    if object_summary(obj) & SUMMARY_PRIVATE == 0 || crate::object::overflow::has_overflow(obj) {
        return None;
    }
    let keys = crate::object::object_keys(obj);
    if keys.is_null() {
        return None;
    }
    crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), key)
        .filter(|&pos| keys_entry(keys.arr(), pos) & ENTRY_OPTIONAL != 0)
        .map(|pos| keys.storage_slot(pos))
}

/// Is the private element with entry `entry` at storage slot `slot` of `obj`
/// present? An element a layout lists as optional (a class family's,
/// `perry_abi::LAYOUT_KEY_PRIVATE_FIELD` with `ENTRY_OPTIONAL`) is absent
/// while its slot holds the hole: the field's definition fills the slot
/// (`js_private_field_add`), and a private field is never deleted. So its
/// presence is a fact of the object, not of its ShapeId: a per-shape answer
/// names the slot, and each use reads this object's word.
///
/// # Safety
/// `obj` is a live `ObjectHeader` without an overflow whose key list holds
/// the entry.
pub(crate) unsafe fn private_element_present(
    obj: *const crate::object::ObjectHeader,
    entry: u8,
    slot: u32,
) -> bool {
    entry & ENTRY_OPTIONAL == 0
        || private_element_word(obj, slot).is_some_and(|w| w != crate::value::TAG_HOLE)
}

/// The word stored in storage slot `slot` of `obj`, `None` when the object
/// has none there (past its live inline bound, or no spill word).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn private_element_word(
    obj: *const crate::object::ObjectHeader,
    slot: u32,
) -> Option<u64> {
    if slot < crate::object::object_live_slot_count(obj) {
        return Some(
            *((obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64)
                .add(slot as usize),
        );
    }
    if slot < crate::object::object_inline_capacity(obj) {
        return None;
    }
    crate::object::spill::spill_get_present(obj as usize, slot as usize)
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
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return 0;
    };
    if record.summary() & SUMMARY_KEY_BITS == 0 && !crate::object::overflow::has_overflow(obj) {
        return 0;
    }
    object_key_entry_filtered(
        crate::object::object_keys_from_shape_record(obj, record),
        key,
        false,
    )
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
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return false;
    };
    if record.summary() & SUMMARY_ACCESSOR == 0 && !crate::object::overflow::has_overflow(obj) {
        return false;
    }
    object_key_entry_filtered(
        crate::object::object_keys_from_shape_record(obj, record),
        key,
        true,
    ) & ENTRY_ACCESSOR
        != 0
}

#[inline(never)]
unsafe fn object_key_entry_filtered(
    keys: crate::object::ObjectKeys,
    key: &[u8],
    accessor: bool,
) -> u8 {
    if keys.is_null() || !keys_may_carry(keys.arr(), keys.count(), key, accessor) {
        return 0;
    }
    #[cfg(feature = "attr-census")]
    crate::object::attr_census::note_global("read.key_entry_lookup");
    match crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key) {
        Some(pos) => keys_entry(keys.arr(), pos),
        None => 0,
    }
}

/// The names of `obj`'s own accessor keys, sorted (the owner-index contract
/// of `accessor_descriptor_keys_for_obj`).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn object_accessor_key_names(
    obj: *const crate::object::ObjectHeader,
) -> Vec<String> {
    if object_summary(obj) & SUMMARY_ACCESSOR == 0 {
        return Vec::new();
    }
    let keys = crate::object::object_keys(obj);
    let (slots, available) = crate::object::keys_array_dense_slots(keys.arr());
    let mut out = Vec::new();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for pos in 0..(keys.count() as usize).min(available) {
        if keys_entry(keys.arr(), pos as u32) & ENTRY_ACCESSOR == 0 {
            continue;
        }
        let slot = crate::JSValue::from_bits((*slots.add(pos)).to_bits());
        if let Some(bytes) = crate::string::js_string_key_bytes(slot, &mut sso) {
            out.push(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    out.sort();
    out
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
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return false;
    };
    object_key_blocks_plain_store_from_record(obj, record, key)
}

/// The same attribute operation when the caller already holds the layout.
/// An object with an overflow records its keys' attributes in its entries,
/// whose summary the overflow keeps (object model step 4), so the answer
/// covers the object's whole own contents, not only its layout's keys.
#[inline]
pub(crate) unsafe fn object_key_blocks_plain_store_from_record(
    obj: *const crate::object::ObjectHeader,
    record: crate::object::shapes::ShapeRecordRef,
    key: &[u8],
) -> bool {
    let table = crate::object::overflow::table(obj);
    let summary = if table.is_null() {
        record.summary()
    } else {
        (*table).summary as u8
    };
    if summary & SUMMARY_BLOCKS_STORE == 0 {
        #[cfg(feature = "attr-census")]
        crate::object::attr_census::note_global("read.store_check_summary_clear");
        return false;
    }
    // If every data property is writable, only an accessor can intercept.
    // Use the existing accessor filter: non-enumerable prototype methods
    // need no lookup for a store merely because another key is an accessor.
    let accessor_only = summary & SUMMARY_NON_WRITABLE == 0;
    !entry_is_plain_writable_data(object_key_entry_filtered(
        crate::object::object_keys_from_shape_record(obj, record),
        key,
        accessor_only,
    ))
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
    /// Every property key becomes non-configurable, and with `freeze` also
    /// non-writable (`Object.freeze` / `Object.seal`).
    Integrity { freeze: bool },
    /// Every key returns to the default.
    #[cfg(test)]
    ClearAll,
    /// `key` is claimed as a private field ([`PRIVATE_FIELD_ENTRY`]).
    Private(&'a [u8]),
}

impl AttrsEdit<'_> {
    /// The single key this edit is about, or `None` for a whole-list edit.
    #[inline]
    pub(crate) fn key(&self) -> Option<&[u8]> {
        match *self {
            AttrsEdit::Data(k, _)
            | AttrsEdit::ClearData(k)
            | AttrsEdit::Accessor(k, _, _)
            | AttrsEdit::ClearAccessor(k)
            | AttrsEdit::Private(k) => Some(k),
            AttrsEdit::Integrity { .. } => None,
            #[cfg(test)]
            AttrsEdit::ClearAll => None,
        }
    }

    /// This edit applied to an entry. A private field is not a property, so
    /// no property edit (a define, `freeze`, `seal`) changes its entry.
    #[inline]
    pub(crate) fn apply(self, old: u8) -> u8 {
        if entry_is_private(old) {
            return old;
        }
        match self {
            AttrsEdit::Private(_) => PRIVATE_FIELD_ENTRY,
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
            #[cfg(test)]
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
/// Whole-list integrity edits include symbol property entries. Byte-keyed
/// edits compare string keys only; holes and private entries stay untouched.
#[inline]
unsafe fn fold_edits(edits: &[AttrsEdit<'_>], slot: crate::JSValue, old: u8) -> u8 {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = slot_key_bytes(slot, &mut sso) else {
        if slot.is_pointer() && crate::symbol::js_is_symbol(f64::from_bits(slot.bits())) != 0 {
            return edits
                .iter()
                .filter(|edit| matches!(edit, AttrsEdit::Integrity { .. }))
                .fold(old, |entry, edit| edit.apply(entry));
        }
        return old;
    };
    let mut entry = old;
    for edit in edits {
        match edit.key() {
            Some(k)
                if k != bytes || matches!(edit, AttrsEdit::Private(_)) != entry_is_private(old) => {
            }
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
            && (if matches!(edit, AttrsEdit::Private(_)) {
                crate::object::keys_find_private_slot_by_bytes(keys.arr(), keys.count(), key)
            } else {
                crate::object::keys_find_property_slot_by_bytes(keys, keys.count(), key)
            })
            .is_some()
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
            let position = if matches!(edit, AttrsEdit::Private(_)) {
                crate::object::keys_find_private_slot_by_bytes(keys.arr(), count as u32, key)
            } else {
                crate::object::keys_find_property_slot_by_bytes(keys, count as u32, key)
            };
            if let Some(pos) = position {
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
    if crate::object::overflow::has_overflow(obj) {
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

/// Restrict an ordinary holder's owned entries inside its integrity operation.
/// The caller holds a no-move window and publishes the final shape once after
/// setting its final integrity flags. Other descriptor edits publish normally.
///
/// # Safety
/// obj owns an overflow table and cannot be observed before final publication.
pub(crate) unsafe fn restrict_owned_integrity(obj: *mut crate::object::ObjectHeader, freeze: bool) {
    let table = crate::object::overflow::table(obj);
    debug_assert!(!table.is_null());
    edit_private_list(
        obj,
        table.cast(),
        0,
        (*table).length,
        &[AttrsEdit::Integrity { freeze }],
    );
}

/// Edit a dictionary receiver's PRIVATE list in place from position `from`.
unsafe fn edit_private_list(
    obj: *mut crate::object::ObjectHeader,
    keys: *mut ArrayHeader,
    from: u32,
    count: u32,
    edits: &[AttrsEdit<'_>],
) {
    let table = keys as *mut crate::object::overflow::Table;
    for pos in from..count {
        let key = crate::JSValue::from_bits(*crate::object::overflow::key_at(table, pos));
        let word = crate::object::overflow::attrs_at(table, pos);
        let attrs = fold_edits(edits, key, *word as u8);
        crate::object::overflow::set_entry_attrs(obj, table, pos, attrs);
    }
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
        attrs_handle.across_mut::<ArrayHeader, _>(|| crate::array::ensure_key_attrs_slot(keys));
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
    // EK §3.1 (a removal moves the tail down): only [0, pos) is untouched.
    crate::array::ek_lower_prefix(attrs, pos as u32);
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
        let old = if (from as usize) < len {
            Word::decode(*w.add(from as usize))
        } else {
            Word::EMPTY
        };
        // GC_STORE_AUDIT(POINTER_FREE): INT32 boxes inside a pointer-free array.
        *w.add(to) = Word {
            entry: old.entry,
            ..Word::EMPTY
        }
        .with_slot(old.slot())
        .encode();
    }
    (*attrs).length = mapping.len() as u32;
    recompute_cumulative(keys, attrs, 0);
}

#[cfg(test)]
#[path = "key_attrs_tests.rs"]
mod key_attrs_tests;
