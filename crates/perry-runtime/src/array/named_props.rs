//! Own non-index ("named") properties of Array exotic objects, stored WITH
//! the array instead of in an address-keyed side table (#10166, brief 4).
//!
//! ## Storage
//!
//! An array that carries named properties has [`crate::gc::GC_ARRAY_NAMED_PROPS`]
//! set in its `GcHeader._reserved` word and reserves the FIRST physical
//! element slots of its backing store, in front of logical element 0. The
//! reserve is expressed through the dense-queue front offset `storage.rs`
//! already defines: `capacity` excludes it, so `array_front_offset(arr)` is at
//! least the reserve size and every logical element address comes out
//! unchanged for every reader — including codegen, which derives the element
//! base from `GcHeader.size` and `capacity` (`perry-codegen/src/array_storage.rs`).
//!
//! Physical slot 0 (`arr + 1`, the *header word*) selects one of two modes:
//!
//! * **Inline** (`INT32_TAG | set | present << 8`): a runtime producer whose
//!   keys are fixed — a regex `exec`/`match` result (`index`, `input`,
//!   `groups`, and `indices` with the `d` flag) — stores the VALUES directly
//!   in physical slots `1..=n`, in key order. `set` names a static key list
//!   ([`InlineKeySet`]) and `present` is a bitmask, so a `delete` just clears
//!   a bit and holes the slot: no allocation, no key strings at all. This is
//!   the whole point for exec results — installing them is `n` slot stores.
//! * **Pairs** (`TAG_HOLE` = none yet, or a NaN-boxed pointer): a *pairs
//!   array* — an ordinary, never user-visible `GC_TYPE_ARRAY` holding
//!   `[key0, value0, key1, value1, …]` in insertion order. Keys are heap
//!   strings, marked shared so an in-place append can never rewrite one. This
//!   is what arbitrary expandos (`arr.foo = v`), sparse far indices and the
//!   template-object `raw` use. The reserve is exactly one slot.
//!
//! An inline array that gains a key outside its set is *materialized* into
//! pairs mode: present keys first, in set order, then the new key — which is
//! exactly insertion order, because a deleted-then-re-added key is absent from
//! the inline set. The vacated inline value slots are holed, so no dead
//! pointer lingers in the front slack.
//!
//! ## Why not the side table
//!
//! `ARRAY_NAMED_PROPS` was a `PtrHashMap<usize, Vec<_>>` keyed by the array
//! ADDRESS. Every exec result paid a hash insert plus a `Vec` allocation, every
//! read a hash probe, every move a rekey (`visit_metadata_usize_slot` + merge),
//! and every collection a `retain` over the whole table. With the storage in
//! the array nothing is keyed by an address: a moved array carries its slots
//! along, and a dead array's properties die with it.
//!
//! ## GC custody
//!
//! The reserve slots are child edges of the array:
//! [`visit_array_named_props_slots`] (called from `gc::layout_slot_visit`'s
//! Array arm, next to the #9304 prototype slot) hands the header word and, in
//! inline mode, every value slot to the collector as fixed slots, so marking
//! retains the values, evacuation and compaction rewrite them, and the
//! remembered-set scan finds them through the pages the store barriers
//! dirtied. Every pointer store into a reserve slot is barriered.
//! `js_array_grow` re-reserves the same number of slots in the replacement
//! allocation ([`carry_named_props_reserve`]) before installing the growth
//! forwarding stub, and `storage::shift_dense`'s empty-queue reset keeps the
//! reserve out of `capacity`.
//!
//! ## Reserving the slot on an existing array
//!
//! Exec results are born with an inline reserve
//! (`js_array_alloc_named_props_reserved`). A user array gains a one-slot
//! pairs reserve on its first expando: if it is a shifted queue
//! (`array_front_offset >= 1`) the dead front slot is taken as is; otherwise
//! the elements move up by one slot inside the allocation (the dense-move
//! funnel `finish_array_dense_move_layout` translates dirty pages / replays
//! barriers), growing first through `js_array_grow` when the allocation is
//! full. Only that last case changes the array's address, exactly as a `push`
//! past capacity does — which is why the setter returns the live head.

use super::header::{
    array_object_flags_from_tag, array_object_flags_resolved, array_receiver_gc_tag, clean_arr_ptr,
    clean_arr_ptr_mut, string_header_as_str, string_header_bytes,
};
use super::{array_elements_ptr, array_front_offset, ArrayHeader};
use crate::gc::RuntimeHandleScope;
use crate::value::{INT32_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_HOLE, TAG_MASK};

/// Static key lists an inline reserve can carry. The discriminant is what the
/// header word stores; `0` is never used so a zeroed word cannot read as one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum InlineKeySet {
    /// A regex `exec`/`match` result.
    ExecResult = 1,
    /// A regex `exec`/`match` result with the `d` flag.
    ExecResultIndices = 2,
    /// The `indices` array of a `d`-flag result.
    IndicesGroups = 3,
}

impl InlineKeySet {
    #[inline]
    pub(crate) const fn keys(self) -> &'static [&'static str] {
        match self {
            InlineKeySet::ExecResult => &["index", "input", "groups"],
            InlineKeySet::ExecResultIndices => &["index", "input", "groups", "indices"],
            InlineKeySet::IndicesGroups => &["groups"],
        }
    }

    #[inline]
    const fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(InlineKeySet::ExecResult),
            2 => Some(InlineKeySet::ExecResultIndices),
            3 => Some(InlineKeySet::IndicesGroups),
            _ => None,
        }
    }

    /// Header word for this set with every key present.
    ///
    /// Only the inline reserve builds one, and only the regex results path
    /// installs an inline reserve (`inline_reserve_layout`), so this is gated
    /// the same way: a `regex-engine`-off build has no caller.
    #[cfg(feature = "regex-engine")]
    #[inline]
    const fn header_word(self) -> u64 {
        let n = self.keys().len() as u64;
        INT32_TAG | (self as u64) | (((1u64 << n) - 1) << 8)
    }
}

crate::perry_thread_local! {
    /// Named properties of arrays that were FULL (no front slack, dense prefix
    /// at capacity), or longer than `RESERVE_IN_PLACE_MOVE_LIMIT`, when they
    /// gained their first one — typically an exact-size
    /// literal such as `const t = [a, b]; t.tag = x`. Reserving a slot for such
    /// an array means growing it, which moves it and leaves the program's own
    /// variable pointing at a forwarding stub that every later access has to
    /// follow (+4…6 % on a literal-tagging loop). Those arrays keep the
    /// address-keyed storage instead; everything else uses the reserve.
    ///
    /// An array stays here for its whole life once it has an entry (growth
    /// rekeys the entry, see [`transfer_full_array_named_props_owner`]), so
    /// its properties never live in two stores. Membership is probed only when
    /// the array carries `OBJ_FLAG_ARRAY_DESCRIPTORS` and no reserve, and only
    /// once any array ever used the table.
    ///
    /// GC custody is the former `ARRAY_NAMED_PROPS` contract: values are
    /// traced and owners rekeyed by [`scan_full_array_named_property_roots_mut`]
    /// (reached from the registered `scan_template_raw_roots_mut`), and dead
    /// owners are dropped by [`prune_dead_full_array_named_property_owners`]
    /// on `gc::dead_owner`'s fan-out.
    static FULL_ARRAY_NAMED_PROPS: std::cell::RefCell<
        crate::fast_hash::PtrHashMap<usize, Vec<FallbackProperty>>,
    > = std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Longest dense prefix the first named property may shift up by one slot to
/// open a reserve in place. Beyond it the array uses the fallback table, so
/// tagging `hugeArray.foo = 1` stays O(1) instead of moving every element
/// (and replaying their barriers) once.
const RESERVE_IN_PLACE_MOVE_LIMIT: usize = 64;

/// Has any array on this process ever used [`FULL_ARRAY_NAMED_PROPS`]?
/// Monotone, so a false answer always proves the table empty.
static FULL_ARRAY_NAMED_PROPS_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

struct FallbackProperty {
    name: Box<str>,
    value: f64,
}

#[inline]
fn fallback_ever() -> bool {
    FULL_ARRAY_NAMED_PROPS_EVER.load(std::sync::atomic::Ordering::Acquire)
}

/// Could `arr` (with flag word `flags`) own entries in the fallback table?
#[inline]
fn fallback_possible(flags: u16) -> bool {
    flags & crate::gc::GC_ARRAY_NAMED_PROPS == 0
        && flags & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
        && fallback_ever()
}

fn barrier_fallback_props(owner: usize, props: &mut [FallbackProperty]) {
    for prop in props.iter_mut() {
        crate::gc::runtime_write_barrier_external_slot(
            owner,
            &mut prop.value as *mut f64 as usize,
            prop.value.to_bits(),
        );
    }
}

fn merge_fallback_props(
    table: &mut crate::fast_hash::PtrHashMap<usize, Vec<FallbackProperty>>,
    owner: usize,
    owner_props: Vec<FallbackProperty>,
) {
    let entry = table.entry(owner).or_default();
    for prop in owner_props {
        if let Some(existing) = entry.iter_mut().find(|existing| existing.name == prop.name) {
            existing.value = prop.value;
        } else {
            entry.push(prop);
        }
    }
    barrier_fallback_props(owner, entry);
}

/// Rekey a fallback owner across `js_array_grow`, which is an allocation
/// replacement outside the collector (#9371, #9201).
pub(crate) fn transfer_full_array_named_props_owner(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner || !fallback_ever() {
        return;
    }
    FULL_ARRAY_NAMED_PROPS.with(|m| {
        let mut table = m.borrow_mut();
        if let Some(old_props) = table.remove(&old_owner) {
            merge_fallback_props(&mut table, new_owner, old_props);
        }
    });
}

/// Trace the fallback values and rekey moved owners.
pub(crate) fn scan_full_array_named_property_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    FULL_ARRAY_NAMED_PROPS.with(|m| {
        let mut table = m.borrow_mut();
        let mut moved = Vec::new();
        for (&owner, owner_props) in table.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            for prop in owner_props.iter_mut() {
                visitor.visit_nanbox_f64_slot(&mut prop.value);
            }
        }
        for (old_owner, new_owner) in moved {
            if let Some(old_props) = table.remove(&old_owner) {
                merge_fallback_props(&mut table, new_owner, old_props);
            }
        }
    });
}

/// Remove fallback entries whose array owners are provably dead under the
/// centralized collection-specific liveness policy.
pub(crate) fn prune_dead_full_array_named_property_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    FULL_ARRAY_NAMED_PROPS.with(|m| {
        m.borrow_mut().retain(|owner, _| !is_dead_owner(*owner));
    });
}

#[inline]
fn with_fallback<R>(owner: usize, f: impl FnOnce(&mut Vec<FallbackProperty>) -> R) -> Option<R> {
    FULL_ARRAY_NAMED_PROPS.with(|m| m.borrow_mut().get_mut(&owner).map(f))
}

/// Physical slot the header word lives in when `GC_ARRAY_NAMED_PROPS` is set.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head.
#[inline(always)]
pub(crate) unsafe fn array_named_props_slot(arr: *const ArrayHeader) -> *mut u64 {
    arr.add(1) as *mut u64
}

/// Whether the resolved head reserves named-property slots.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline(always)]
pub(crate) unsafe fn array_named_props_flagged_resolved(arr: *const ArrayHeader) -> bool {
    array_object_flags_resolved(arr) & crate::gc::GC_ARRAY_NAMED_PROPS != 0
}

/// The decoded reserve of a flagged head.
#[derive(Clone, Copy)]
enum Reserve {
    /// Not flagged: no reserve.
    None,
    /// Pairs mode; null while no property has been added.
    Pairs(*mut ArrayHeader),
    /// Inline mode: the key set and its present-key bitmask.
    Inline(InlineKeySet, u32),
    /// No reserve, but the array may own `FULL_ARRAY_NAMED_PROPS` entries.
    /// Produced only by [`resolve`], for the readers.
    Fallback,
}

#[inline]
fn decode_header_word(bits: u64) -> Reserve {
    match bits & TAG_MASK {
        POINTER_TAG => Reserve::Pairs((bits & POINTER_MASK) as *mut ArrayHeader),
        INT32_TAG => {
            let low = bits as u32;
            match InlineKeySet::from_id(low & 0xFF) {
                Some(set) => Reserve::Inline(set, (low >> 8) & 0xFF),
                None => Reserve::Pairs(std::ptr::null_mut()),
            }
        }
        _ => Reserve::Pairs(std::ptr::null_mut()),
    }
}

/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head and `flags`
/// its flag word.
#[inline]
unsafe fn reserve_of(arr: *const ArrayHeader, flags: u16) -> Reserve {
    if flags & crate::gc::GC_ARRAY_NAMED_PROPS == 0 {
        return Reserve::None;
    }
    // Invariant: a pairs pointer in the header word is always the LIVE pairs
    // head. Growth writes the replacement back through `store_pairs_pointer`
    // before anything can read it, and every collector move rewrites the slot
    // (it is an enumerated child edge), so no forwarding resolution is needed.
    decode_header_word(*array_named_props_slot(arr))
}

/// Number of physical slots reserved in front of the logical elements: 0
/// without the flag, 1 in pairs mode, `1 + keys` in inline mode.
/// `js_array_grow` sizes the replacement allocation with it and
/// `storage::shift_dense` keeps it out of `capacity`.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline]
pub(crate) unsafe fn array_named_props_reserve(arr: *const ArrayHeader) -> usize {
    match reserve_of(arr, array_object_flags_resolved(arr)) {
        Reserve::None | Reserve::Fallback => 0,
        Reserve::Pairs(_) => 1,
        Reserve::Inline(set, _) => 1 + set.keys().len(),
    }
}

/// Hand every reserve slot of a flagged array to `visit` as a fixed child slot:
/// the header word, and in inline mode each value slot (holed ones included —
/// a non-pointer is ignored by every visitor).
///
/// # Safety
/// `arr` must be a live, non-forwarded `GC_TYPE_ARRAY` head whose header
/// carries `GC_ARRAY_NAMED_PROPS`.
#[inline]
pub(crate) unsafe fn visit_array_named_props_slots(
    arr: *const ArrayHeader,
    mut visit: impl FnMut(*mut u64),
) {
    let slot = array_named_props_slot(arr);
    visit(slot);
    if let Reserve::Inline(set, _) = decode_header_word(*slot) {
        for i in 1..=set.keys().len() {
            visit(slot.add(i));
        }
    }
}

/// Copy the `reserve` front slots of `old` into the same positions of `new`,
/// barriering each store (a pairs pointer or an inline value may be young).
/// Used by `js_array_grow` before the old head becomes a forwarding stub.
///
/// # Safety
/// Both heads must be live arrays with at least `reserve` front slots, and
/// nothing may allocate between the copy and the caller's stub install.
pub(crate) unsafe fn carry_named_props_reserve(
    old: *const ArrayHeader,
    new: *mut ArrayHeader,
    reserve: usize,
) {
    let from = array_named_props_slot(old);
    let to = array_named_props_slot(new);
    for i in 0..reserve {
        let bits = *from.add(i);
        // GC_STORE_AUDIT(BARRIERED): runtime_write_barrier_slot below records
        // any pointer carried into the replacement head's reserve.
        std::ptr::write(to.add(i), bits);
        crate::gc::runtime_write_barrier_slot(new as usize, to.add(i) as usize, bits);
    }
}

/// The flag word of a receiver `clean_arr_ptr` resolved, read through the
/// validated header path. `clean_arr_ptr` waves registered Buffer / TypedArray
/// receivers through, and those carry no `GcHeader`: the eight bytes below
/// their payload are allocator bookkeeping, so a raw `_reserved` read there
/// could invent the flag and dereference the buffer's first data word.
#[inline]
unsafe fn resolved_flags(arr: *const ArrayHeader) -> u16 {
    array_object_flags_from_tag(array_receiver_gc_tag(arr))
}

/// Resolve `arr` and decode its reserve.
#[inline]
unsafe fn resolve(arr: *const ArrayHeader) -> (*const ArrayHeader, Reserve) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return (arr, Reserve::None);
    }
    let flags = resolved_flags(arr);
    match reserve_of(arr, flags) {
        Reserve::None if fallback_possible(flags) => (arr, Reserve::Fallback),
        reserve => (arr, reserve),
    }
}

/// Position of the key with content `wanted` among the PRESENT keys of an
/// inline set.
#[inline]
fn inline_position(set: InlineKeySet, present: u32, wanted: &[u8]) -> Option<usize> {
    set.keys()
        .iter()
        .position(|key| key.as_bytes() == wanted)
        .filter(|&i| present & (1 << i) != 0)
}

/// Index of the key slot whose string content equals `wanted`, if any.
/// Property-name strings from compiled code are interned, so the stored key is
/// usually the very same `StringHeader` as `key_ptr`: that identity is tested
/// before any byte comparison. `key_ptr` may be null when only content is known.
///
/// # Safety
/// `pairs` must be a live pairs array.
#[inline]
unsafe fn find_pair(
    pairs: *const ArrayHeader,
    key_ptr: *const crate::StringHeader,
    wanted: &[u8],
) -> Option<usize> {
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let wanted_bits = crate::value::js_nanbox_string(key_ptr as i64).to_bits();
    let mut i = 0;
    while i + 1 < len {
        if !key_ptr.is_null() && *elems.add(i) == wanted_bits {
            return Some(i);
        }
        i += 2;
    }
    let mut i = 0;
    while i + 1 < len {
        let key_bits = *elems.add(i);
        if key_bits & TAG_MASK == STRING_TAG {
            let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            if string_header_bytes(key) == Some(wanted) {
                return Some(i);
            }
        }
        i += 2;
    }
    None
}

/// Shared body of the named-property reads: `key_ptr` (may be null) enables the
/// identity fast path, `wanted` is the key content. No UTF-8 validation: a key
/// that is not valid UTF-8 was never stored (every store validates), so its
/// bytes cannot match.
#[inline]
unsafe fn lookup(
    arr: *const ArrayHeader,
    key_ptr: *const crate::StringHeader,
    wanted: &[u8],
) -> Option<f64> {
    match resolve(arr) {
        (arr, Reserve::Inline(set, present)) => inline_position(set, present, wanted)
            .map(|i| f64::from_bits(*array_named_props_slot(arr).add(1 + i))),
        (_, Reserve::Pairs(pairs)) if !pairs.is_null() => find_pair(pairs, key_ptr, wanted)
            .map(|i| f64::from_bits(*array_elements_ptr(pairs).add(i + 1))),
        (arr, Reserve::Fallback) => with_fallback(arr as usize, |props| {
            props
                .iter()
                .find(|prop| prop.name.as_bytes() == wanted)
                .map(|prop| prop.value)
        })
        .flatten(),
        _ => None,
    }
}

/// Store `pairs` into the header word of `arr`. The ONE barriered funnel for
/// the pairs edge.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved head with a reserve, `pairs` a
/// live pairs array. Nothing may allocate between the write and the barrier.
unsafe fn store_pairs_pointer(arr: *mut ArrayHeader, pairs: *mut ArrayHeader) {
    store_named_props_word(arr, crate::value::js_nanbox_pointer(pairs as i64).to_bits());
}

/// Store `bits` into the reserve header word of a flagged head, barriered.
/// The pairs pointer's store, shared with the keys-array attributes pointer
/// (`object/key_attrs.rs`), which reuses this reserve on internal key lists.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved head carrying
/// `GC_ARRAY_NAMED_PROPS`.
#[inline]
pub(crate) unsafe fn store_named_props_word(arr: *mut ArrayHeader, bits: u64) {
    let slot = array_named_props_slot(arr);
    // GC_STORE_AUDIT(BARRIERED): the reserved-slot edge is recorded by the
    // slot barrier below; the collector enumerates this exact word as a fixed
    // child slot of the array.
    std::ptr::write(slot, bits);
    crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, bits);
}

/// Store an inline value into physical reserve slot `1 + i` of `arr`.
///
/// # Safety
/// `arr` must be a live head in inline mode with at least `i + 1` keys.
#[inline]
unsafe fn store_inline_value(arr: *mut ArrayHeader, i: usize, bits: u64) {
    let slot = array_named_props_slot(arr).add(1 + i);
    // GC_STORE_AUDIT(BARRIERED): the collector enumerates inline value slots as
    // fixed child slots; runtime_write_barrier_slot below records the edge.
    std::ptr::write(slot, bits);
    crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, bits);
}

/// Allocate a pairs array with room for `pair_capacity` pairs. Its layout is
/// left `GC_LAYOUT_UNKNOWN` (the allocator's zero state): the collector
/// tag-scans its handful of slots, so no store below needs a layout note.
/// `length` is `preset_len` slots of `TAG_HOLE`, so a collection in the middle
/// of a fill still visits every slot written so far.
///
/// Allocates: may collect.
unsafe fn pairs_alloc(pair_capacity: usize, preset_len: usize) -> *mut ArrayHeader {
    // RULE 3: `capacity` lands at payload `+4`. The `as u32` here used to be
    // able to truncate as well as to alias a ShapeId; both are refused now.
    let capacity = super::alloc::array_capacity_or_throw(
        u32::try_from(pair_capacity.max(2).saturating_mul(2)).unwrap_or(u32::MAX),
    );
    let pairs = crate::arena::arena_alloc_gc(
        super::header::array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    (*pairs).length = preset_len as u32;
    (*pairs).capacity = capacity;
    let elems = array_elements_ptr(pairs);
    for i in 0..capacity as usize {
        // GC_STORE_AUDIT(INIT): hole-initialising a just-allocated array that
        // nothing references yet; TAG_HOLE is a non-pointer sentinel.
        std::ptr::write(elems.add(i), TAG_HOLE);
    }
    pairs
}

/// Store one word into slot `i` of a pairs array.
///
/// A pairs array is never raw-f64 and keeps the allocator's
/// `GC_LAYOUT_UNKNOWN` (tag-scanned) state for its whole life — growth copies
/// that state, and the only in-place mutation (`finish_array_dense_move_layout`
/// on delete) leaves it alone — so the layout and numeric notes the generic
/// element store performs have nothing to do here. The slot barrier is the
/// one thing a store owes.
///
/// # Safety
/// `pairs` must be a live pairs array and `i < capacity`.
#[inline]
unsafe fn write_slot(pairs: *mut ArrayHeader, i: usize, bits: u64) {
    let slot = array_elements_ptr(pairs).add(i);
    // GC_STORE_AUDIT(BARRIERED): runtime_write_barrier_slot below records the
    // edge; the pairs array's layout is tag-scanned, so no layout note is owed.
    std::ptr::write(slot, bits);
    crate::gc::runtime_write_barrier_slot(pairs as usize, slot as usize, bits);
}

/// Write one pair into slots `i`, `i + 1` of a pairs array (no length change).
///
/// # Safety
/// `pairs` must be live and `i + 1 < capacity`; `key_bits` a `STRING_TAG` box.
#[inline]
unsafe fn write_pair(pairs: *mut ArrayHeader, i: usize, key_bits: u64, value_bits: u64) {
    write_slot(pairs, i, key_bits);
    write_slot(pairs, i + 1, value_bits);
}

/// Convert an inline reserve into pairs mode (see the module docs), returning
/// the live head. The array does not grow; it can move only through a
/// collection inside the allocations here.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved head in inline mode. Allocates.
unsafe fn materialize_inline(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    let Reserve::Inline(set, present) = reserve_of(arr, array_object_flags_resolved(arr)) else {
        return arr;
    };
    let keys = set.keys();
    let scope = RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let (pairs, _) = arr_handle.across_mut::<ArrayHeader, _>(|| pairs_alloc(keys.len() + 2, 0));
    let pairs_handle = scope.root_raw_mut_ptr(pairs);
    for (i, key) in keys.iter().enumerate() {
        if present & (1 << i) == 0 {
            continue;
        }
        // Interning allocates on a miss; both heads are reloaded after it.
        let key = crate::string::intern_ascii_literal(key.as_bytes());
        let key_bits = crate::value::js_nanbox_string(key as i64).to_bits();
        pairs_handle.with_mut_ptr::<ArrayHeader, _>(|pairs| {
            arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
                let value = *array_named_props_slot(arr).add(1 + i);
                let len = (*pairs).length as usize;
                write_pair(pairs, len, key_bits, value);
                (*pairs).length = (len + 2) as u32;
            })
        });
    }
    // Nothing below allocates.
    pairs_handle.with_mut_ptr::<ArrayHeader, _>(|pairs| {
        arr_handle.with_mut_ptr::<ArrayHeader, _>(|arr| {
            let slot = array_named_props_slot(arr);
            for i in 1..=keys.len() {
                // GC_STORE_AUDIT(POINTER_FREE): holing a vacated inline value
                // slot; the value now lives in the pairs array.
                std::ptr::write(slot.add(i), TAG_HOLE);
            }
            store_pairs_pointer(arr, pairs);
            arr
        })
    })
}

/// Give a resolved head a one-slot pairs reserve, returning the live head —
/// which is a different allocation only when the array was full and had to
/// grow (a forwarding stub then resolves the old address, exactly as after a
/// `push` past capacity). Returns null when the reserve could not be taken (a
/// sealed or frozen full array, which no caller can add a property to anyway).
///
/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head that
/// `resolved_flags` reported as a real array. May allocate (via `js_array_grow`).
pub(crate) unsafe fn ensure_named_props_slot(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    if array_named_props_flagged_resolved(arr) {
        return arr;
    }
    let mut arr = arr;
    if array_front_offset(arr) == 0 {
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        if dense >= capacity {
            // Full: no slack anywhere. Grow exactly like an append would.
            let grown = super::js_array_grow(arr, (capacity as u32).saturating_add(1));
            if grown.is_null() || ((*grown).capacity as usize) <= dense {
                return std::ptr::null_mut();
            }
            arr = grown;
        }
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        let elems = array_elements_ptr(arr);
        // GC_STORE_AUDIT(BARRIERED): the dense prefix moves up by one slot
        // inside its own allocation; `finish_array_dense_move_layout` below
        // translates the old array's dirty-page coverage or replays the
        // barriers for the moved survivors.
        std::ptr::copy(elems, elems.add(1), dense);
        (*arr).capacity -= 1;
        debug_assert_eq!(array_front_offset(arr), 1);
        super::finish_array_dense_move_layout(arr, elems, elems.add(1), dense, elems, 0);
    }
    // GC_STORE_AUDIT(INIT): the reserved word is dead front slack until the
    // flag below publishes it as a child slot; a non-pointer sentinel goes in
    // first so the collector never reads a stale element there.
    std::ptr::write(array_named_props_slot(arr), TAG_HOLE);
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
    (*header)._reserved |= crate::gc::GC_ARRAY_NAMED_PROPS;
    arr
}

/// The existing special-property bit is deliberately conservative and
/// monotone: sharing it with named properties gives the callback-free array
/// consumers an address-local absence proof without a second flag test.
#[inline]
unsafe fn mark_array_descriptors(arr: *mut ArrayHeader) {
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
    (*header)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
}

/// `arr[name] = value` for a non-index string key. Returns the live head: the
/// array moves only when it was full and had to grow (see
/// [`ensure_named_props_slot`]); callers that keep using the receiver must use
/// the returned pointer, as they already do after `js_array_grow`.
///
/// # Safety
/// `key` must be a live heap string. May allocate (a pairs array, growth).
pub(crate) unsafe fn array_named_property_set(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    let flags = resolved_flags(arr);
    let Some(wanted) = string_header_bytes(key) else {
        return arr;
    };
    // A NEW key is stored only when it is valid UTF-8 (the enumeration and
    // attribute tables are `str`-keyed); overwriting an existing key needs no
    // check, because only valid keys were ever stored.
    let new_key_name = || std::str::from_utf8(wanted).ok();
    let reserve = reserve_of(arr, flags);
    match reserve {
        Reserve::None => {
            let owner = arr as usize;
            if fallback_possible(flags)
                && with_fallback(owner, |props| {
                    if let Some(prop) = props.iter_mut().find(|prop| prop.name.as_bytes() == wanted)
                    {
                        prop.value = value;
                    } else if let Some(name) = new_key_name() {
                        props.push(FallbackProperty {
                            name: name.into(),
                            value,
                        });
                    }
                    barrier_fallback_props(owner, props);
                })
                .is_some()
            {
                return arr;
            }
            let Some(name) = new_key_name() else {
                return arr;
            };
            let capacity = (*arr).capacity as usize;
            let dense = ((*arr).length as usize).min(capacity);
            if array_front_offset(arr) == 0
                && (dense >= capacity || dense > RESERVE_IN_PLACE_MOVE_LIMIT)
            {
                // Full, or too long to shift in place: keep the address-keyed
                // store rather than moving the array or its elements (see
                // `FULL_ARRAY_NAMED_PROPS`).
                FULL_ARRAY_NAMED_PROPS_EVER.store(true, std::sync::atomic::Ordering::Release);
                FULL_ARRAY_NAMED_PROPS.with(|m| {
                    let mut table = m.borrow_mut();
                    let props = table.entry(owner).or_default();
                    props.push(FallbackProperty {
                        name: name.into(),
                        value,
                    });
                    barrier_fallback_props(owner, props);
                });
                mark_array_descriptors(arr);
                return arr;
            }
        }
        Reserve::Inline(set, present) => {
            // Overwrite a present inline key in place.
            if let Some(i) = inline_position(set, present, wanted) {
                store_inline_value(arr, i, value.to_bits());
                return arr;
            }
        }
        Reserve::Pairs(pairs) if !pairs.is_null() => {
            // Overwrite in place: no allocation, no move.
            if let Some(i) = find_pair(pairs, key, wanted) {
                write_slot(pairs, i + 1, value.to_bits());
                return arr;
            }
            if new_key_name().is_none() {
                return arr;
            }
            // Append into existing room: still no allocation.
            let len = (*pairs).length as usize;
            if len + 2 <= (*pairs).capacity as usize {
                let key_bits = crate::value::js_nanbox_string(key as i64).to_bits();
                crate::string::js_string_addref_if_heap_string(f64::from_bits(key_bits));
                write_pair(pairs, len, key_bits, value.to_bits());
                (*pairs).length = (len + 2) as u32;
                return arr;
            }
        }
        _ => {}
    }
    if new_key_name().is_none() {
        return arr;
    }
    // A new key that needs materialization, the reserve, a pairs array, or
    // pairs growth: those allocate, so root everything a collection could move.
    let scope = RuntimeHandleScope::new();
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let base_handle = scope.root_raw_mut_ptr(arr);
    let (arr, base) = base_handle.across_mut::<ArrayHeader, _>(|| {
        if matches!(reserve, Reserve::Inline(..)) {
            materialize_inline(arr)
        } else {
            // Taking the reserve may grow (allocate).
            ensure_named_props_slot(arr)
        }
    });
    if arr.is_null() {
        // No reserve could be taken (sealed/frozen full array): the property
        // is not added, exactly as the guard ladder above this call decided.
        return clean_arr_ptr_mut(base);
    }
    // Reuse the base handle for the (possibly materialized) live head instead
    // of pushing another root: handle pushes are a measurable part of this
    // path (#10166 brief 3).
    base_handle.set_raw_mut_ptr(arr);
    let arr_handle = &base_handle;
    let mut pairs = match reserve_of(arr, array_object_flags_resolved(arr)) {
        Reserve::Pairs(pairs) => pairs,
        _ => std::ptr::null_mut(),
    };
    if pairs.is_null() {
        let (fresh, arr) = arr_handle.across_mut::<ArrayHeader, _>(|| pairs_alloc(4, 0));
        store_pairs_pointer(clean_arr_ptr_mut(arr), fresh);
        pairs = fresh;
    }
    let len = (*pairs).length as usize;
    if len + 2 > (*pairs).capacity as usize {
        let (grown, arr) = arr_handle
            .across_mut::<ArrayHeader, _>(|| super::js_array_grow(pairs, (len + 2) as u32));
        store_pairs_pointer(clean_arr_ptr_mut(arr), grown);
        pairs = grown;
    }
    // Nothing below allocates: the final reads are scoped to non-allocating
    // operations.
    let arr = arr_handle.with_mut_ptr::<ArrayHeader, _>(clean_arr_ptr_mut);
    // The key is now shared with the pairs array: an in-place append on a
    // still-unique source string must copy instead of rewriting our key.
    let key_bits = key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
        crate::value::js_nanbox_string(key as i64).to_bits()
    });
    crate::string::js_string_addref_if_heap_string(f64::from_bits(key_bits));
    write_pair(
        pairs,
        len,
        key_bits,
        value_handle.get_nanbox_f64().to_bits(),
    );
    (*pairs).length = (len + 2) as u32;
    mark_array_descriptors(arr);
    arr
}

/// Install the values of a FRESHLY built array born with an inline reserve
/// (`js_array_alloc_named_props_reserved`), one per key of its set, in key
/// order. Nothing here allocates, so the caller's raw values stay current. A
/// fresh array has no accessor descriptors, attributes, or freeze/seal state,
/// which is what makes bypassing `js_array_set_string_key`'s guard ladder
/// sound.
///
/// # Safety
/// `arr` must be a live head born with an inline reserve whose set has exactly
/// `values.len()` keys.
#[cfg(feature = "regex-engine")]
pub(crate) unsafe fn array_named_props_install_inline(arr: *mut ArrayHeader, values: &[f64]) {
    debug_assert!(matches!(
        reserve_of(arr, array_object_flags_resolved(arr)),
        Reserve::Inline(set, _) if set.keys().len() == values.len()
    ));
    for (i, value) in values.iter().enumerate() {
        store_inline_value(arr, i, value.to_bits());
    }
    // Deliberately NOT `mark_array_descriptors`: `OBJ_FLAG_ARRAY_DESCRIPTORS`
    // is also the index-accessor / non-writable-length / sparse-index gate
    // that sends element reads, writes and loops to the slow paths, and an
    // exec result's keys are never indices or accessors. Every reader of
    // named properties (enumeration, `in`, JSON `toJSON`, species, length
    // truncation) consults them without the bit, and a key outside the set
    // materializes into pairs mode, whose store sets the bit (#10166 audit of
    // every 0x400 reader).
}

/// Header word and reserve size for a freshly allocated inline reserve.
#[cfg(feature = "regex-engine")]
pub(crate) const fn inline_reserve_layout(set: InlineKeySet) -> (u64, usize) {
    (set.header_word(), 1 + set.keys().len())
}

/// Does this (already resolved) array head carry at least one named property?
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline]
pub(crate) unsafe fn array_has_named_properties_resolved(arr: *const ArrayHeader) -> bool {
    let flags = array_object_flags_resolved(arr);
    match reserve_of(arr, flags) {
        Reserve::None | Reserve::Fallback => {
            fallback_possible(flags)
                && with_fallback(arr as usize, |props| !props.is_empty()).unwrap_or(false)
        }
        Reserve::Inline(_, present) => present != 0,
        Reserve::Pairs(pairs) => !pairs.is_null() && (*pairs).length >= 2,
    }
}

/// Whether an already-resolved array owns numeric indices among its named
/// properties. Those indices live beyond the dense allocation; growing the
/// allocation across one without migrating it would hide the property because
/// indexed reads consult the named properties only at `index >= capacity`.
/// Inline key sets never contain an index.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
pub(crate) unsafe fn array_has_sparse_index_properties_resolved(arr: *const ArrayHeader) -> bool {
    let flags = array_object_flags_resolved(arr);
    let pairs = match reserve_of(arr, flags) {
        Reserve::Pairs(pairs) => pairs,
        Reserve::None if fallback_possible(flags) => {
            return with_fallback(arr as usize, |props| {
                props
                    .iter()
                    .any(|prop| crate::object::canonical_array_index(&prop.name).is_some())
            })
            .unwrap_or(false);
        }
        _ => return false,
    };
    if pairs.is_null() {
        return false;
    }
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let mut i = 0;
    while i + 1 < len {
        let key_bits = *elems.add(i);
        if key_bits & TAG_MASK == STRING_TAG {
            let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            if string_header_as_str(key)
                .is_some_and(|name| crate::object::canonical_array_index(name).is_some())
            {
                return true;
            }
        }
        i += 2;
    }
    false
}

pub(crate) unsafe fn array_named_property_get_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> Option<f64> {
    lookup(arr, std::ptr::null(), name.as_bytes())
}

pub(crate) unsafe fn array_named_property_get(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    lookup(arr, key, string_header_bytes(key)?)
}

pub(crate) unsafe fn array_named_property_has(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    string_header_bytes(key).is_some_and(|wanted| lookup(arr, key, wanted).is_some())
}

/// Own named-property keys in insertion order (after the integer indices in
/// `[[OwnPropertyKeys]]`), optionally only the enumerable ones.
pub(crate) unsafe fn array_named_property_names(
    arr: *const ArrayHeader,
    enumerable_only: bool,
) -> Vec<String> {
    let (arr, reserve) = resolve(arr);
    let owner = arr as usize;
    let keep = |name: &str| {
        !enumerable_only
            || crate::object::get_property_attrs(owner, name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true)
    };
    match reserve {
        Reserve::Inline(set, present) => set
            .keys()
            .iter()
            .enumerate()
            .filter(|(i, name)| present & (1 << i) != 0 && keep(name))
            .map(|(_, name)| name.to_string())
            .collect(),
        Reserve::Pairs(pairs) if !pairs.is_null() => {
            let len = (*pairs).length as usize;
            let elems = array_elements_ptr(pairs);
            let mut names = Vec::with_capacity(len / 2);
            let mut i = 0;
            while i + 1 < len {
                let key_bits = *elems.add(i);
                i += 2;
                if key_bits & TAG_MASK != STRING_TAG {
                    continue;
                }
                let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
                if let Some(name) = string_header_as_str(key) {
                    if keep(name) {
                        names.push(name.to_string());
                    }
                }
            }
            names
        }
        Reserve::Fallback => with_fallback(owner, |props| {
            props
                .iter()
                .filter(|prop| keep(&prop.name))
                .map(|prop| prop.name.to_string())
                .collect()
        })
        .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub(crate) unsafe fn array_named_property_delete(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(name) = string_header_as_str(key) else {
        return false;
    };
    array_named_property_delete_by_name(arr, name)
}

/// Remove one property. Never allocates: an inline key clears its present bit
/// and holes its slot; a pair closes the gap so survivors keep insertion order.
pub(crate) unsafe fn array_named_property_delete_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> bool {
    match resolve(arr) {
        (arr, Reserve::Inline(set, present)) => {
            let Some(i) = inline_position(set, present, name.as_bytes()) else {
                return false;
            };
            let slot = array_named_props_slot(arr);
            let word = (*slot & !(1u64 << (8 + i))) | INT32_TAG;
            // GC_STORE_AUDIT(POINTER_FREE): the header word is an INT32 box and
            // the deleted value slot becomes a hole; neither is a pointer.
            std::ptr::write(slot, word);
            std::ptr::write(slot.add(1 + i), TAG_HOLE);
            true
        }
        (_, Reserve::Pairs(pairs)) if !pairs.is_null() => {
            let Some(i) = find_pair(pairs, std::ptr::null(), name.as_bytes()) else {
                return false;
            };
            let len = (*pairs).length as usize;
            let elems = array_elements_ptr(pairs);
            let moved = len - i - 2;
            // GC_STORE_AUDIT(BARRIERED): survivors slide down inside the pairs
            // allocation; `finish_array_dense_move_layout` below translates
            // dirty pages or replays their barriers, and the vacated tail holds
            // holes.
            std::ptr::copy(elems.add(i + 2), elems.add(i), moved);
            std::ptr::write(elems.add(len - 2), TAG_HOLE);
            std::ptr::write(elems.add(len - 1), TAG_HOLE);
            (*pairs).length = (len - 2) as u32;
            super::finish_array_dense_move_layout(
                pairs,
                elems.add(i + 2),
                elems.add(i),
                moved,
                elems,
                0,
            );
            true
        }
        (arr, Reserve::Fallback) => with_fallback(arr as usize, |props| {
            let Some(index) = props.iter().position(|prop| &*prop.name == name) else {
                return false;
            };
            props.remove(index);
            true
        })
        .unwrap_or(false),
        _ => false,
    }
}

/// Test-only view of the reserve: `(flagged, pairs head or 0, property count)`.
#[cfg(test)]
pub(crate) unsafe fn test_named_props_state(arr: *const ArrayHeader) -> (bool, usize, usize) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return (false, 0, 0);
    }
    let flags = resolved_flags(arr);
    let flagged = flags & crate::gc::GC_ARRAY_NAMED_PROPS != 0;
    match reserve_of(arr, flags) {
        Reserve::None | Reserve::Fallback => (flagged, 0, 0),
        Reserve::Inline(_, present) => (flagged, 0, present.count_ones() as usize),
        Reserve::Pairs(pairs) if pairs.is_null() => (flagged, 0, 0),
        Reserve::Pairs(pairs) => (flagged, pairs as usize, (*pairs).length as usize / 2),
    }
}

/// Test-only: the inline key set of a head, if it is in inline mode.
#[cfg(test)]
pub(crate) unsafe fn test_named_props_inline_set(arr: *const ArrayHeader) -> Option<InlineKeySet> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return None;
    }
    match reserve_of(arr, resolved_flags(arr)) {
        Reserve::Inline(set, _) => Some(set),
        _ => None,
    }
}

/// Test-only: does the fallback table hold an entry for `owner`?
#[cfg(test)]
pub(crate) fn test_full_array_named_property_owner_exists(owner: usize) -> bool {
    FULL_ARRAY_NAMED_PROPS.with(|m| m.borrow().contains_key(&owner))
}

/// Test-only: drop every fallback entry.
#[cfg(test)]
pub(crate) fn test_clear_full_array_named_property_roots() {
    FULL_ARRAY_NAMED_PROPS.with(|m| m.borrow_mut().clear());
}
