//! Agent-local authoritative object-shape descriptors (#8067).
//!
//! A shared `keys_array` already IS a shape (same pointer ⟹ same ordered
//! key list, because mutation always forks a private clone). This module
//! promotes that identity into an explicit per-shape key→slot table,
//! replacing two per-consumer tables that re-derived the same map:
//!
//! * `KEYS_INDEX` — keyed per OBJECT, so 10k same-shape siblings built 10k
//!   private indexes;
//! * `WIDE_KEY_INDEX` — keys-keyed but capped at a 4-entry LRU, so any
//!   working set past 4 wide shapes thrashed.
//!
//! The pointer-keyed key→slot index remains an accelerator: every hit still
//! re-validates the key bytes. Separately, every published `ShapeId` resolves
//! in this agent's `RuntimeState` to a descriptor containing the
//! ordered-keys edge plus the exact logical-key and live-inline-slot bounds.
//! The descriptor table is agent-local while ids are process-global. A live
//! object's ShapeId is authoritative for its ordered keys, logical-key count,
//! live inline-slot bound, and semantic generation. The one deliberately
//! mutable state is an owned ordinary object's #9064 stable-tombstone epoch:
//! per-slot `TAG_HOLE` validation lets its private keys array grow between
//! amortized squeezes without retiring unrelated cached slots.
//!
//! #8113 removed `ObjectHeader::field_count`, so the descriptor's
//! `live_inline_slot_count` is no longer a mirror of a header word — it is the
//! ONLY record of the bound. Every publication below is therefore
//! MINT-THEN-STAMP: the successor descriptor is fully installed while the
//! predecessor stamp is still readable, and the `parent_class_id` store is the
//! single, allocation-free publication point. A stamp-cleared window would be a
//! window in which the collector sees a live bound of 0 (#7154/#7164).
//! #8047 removed `ObjectHeader::keys_array`; consumers derive the edge from
//! this descriptor and no compatibility mirror remains.

use crate::array::ArrayHeader;
use std::cell::RefCell;

#[path = "shapes_own_data.rs"]
mod shapes_own_data;
pub(crate) use shapes_own_data::own_data_shape;

#[path = "shapes_birth_width.rs"]
mod shapes_birth_width;
#[path = "shapes_last_key_rollback.rs"]
mod shapes_last_key_rollback;
pub(crate) use shapes_last_key_rollback::{
    note_last_key_parent, publish_object_shape_last_key_rollback,
};
#[path = "shapes_linked_birth.rs"]
mod shapes_linked_birth;
#[path = "shapes_prototype.rs"]
mod shapes_prototype;
pub(crate) use shapes_linked_birth::stamp_linked_final_shape;
#[path = "shapes_slot_list.rs"]
mod shapes_slot_list;
#[path = "shapes_store.rs"]
mod shapes_store;
#[path = "shapes_worker_seed.rs"]
mod shapes_worker_seed;
pub(crate) use shapes_prototype::{
    identity_word_slot, note_full_trace_begin, proto_id_carries_word, prune_dead_shape_prototypes,
    scan_shape_prototype_words_mut, shape_prototype_word,
};
#[path = "shapes_store_kind.rs"]
pub(crate) mod store_kind;
pub(crate) use shapes_birth_width::{keyless_birth_width, note_spill_width};
#[cfg(test)]
pub(crate) use shapes_slot_list::shape_descriptor_keys_slot;
pub(crate) use shapes_slot_list::shape_id_owns_keys_slot;
pub(crate) use shapes_slot_list::{
    object_shape_hole_count, publish_object_shape_delete_transition, publish_object_shape_holes,
    rekey_stable_tombstone_shape_after_squeeze, retire_owned_shape_history,
    shape_index_migrate_after_delete, shape_index_shift_in_place,
    try_update_stable_tombstone_shape, try_update_stable_tombstone_shape_cached, SlotIndex,
};
pub(crate) use shapes_store::ConstFnSlotInfo;
pub(crate) use shapes_store::PERRY_EMPTY_SHAPE_DIR;
use shapes_store::{
    IdList, ShapeRecord, ShapeSlab, RECORD_FLAG_BIRTH_OWNER, RECORD_FLAG_CACHE_CARRIER,
    RECORD_FLAG_CARRIED_SEEN, RECORD_FLAG_EXTERNAL_CARRIER, RECORD_FLAG_FACTS_INDEXED,
    RECORD_FLAG_OLD_CARRIER, RECORD_FLAG_OLD_CARRIER_SEEN,
};
pub(crate) use shapes_worker_seed::{install_worker_shape_seed, worker_shape_seed};

#[derive(Clone)]
pub(crate) struct ShapeIndex {
    /// Key count covered by `slots`. Longer live array ⟹ catch up
    /// incrementally (append-only while shared); shorter ⟹ a delete
    /// compacted it — drop and rebuild on next lookup_ways.
    indexed_len: u32,
    /// FNV-1a content hash of key bytes → candidate slots (collisions
    /// resolved by the per-hit content validation).
    ///
    /// Keyed with [`crate::fast_hash::PtrHasher`], not the std default: the
    /// key is ALREADY a well-distributed FNV-1a hash, so running SipHash over
    /// it again buys no distribution and costs real time. On
    /// `bench_populated_delete.ts` — perry's worst object-model gap against
    /// node — `hash_one::<&usize>` plus `sip::Hasher::write` were **14.7% of
    /// self time**, second only to the lookup that performs them.
    slots: SlotIndex,
}

/// Immutable facts named by one ShapeId, copied out of the table.
///
/// #8112: the table record's `keys` is the AUTHORITATIVE ordered-keys edge —
/// the collector marks it and rewrites it in place. Before #8112 the header
/// word was the sole strong edge and this field a weak copy that a post-visit
/// callback repaired. The inversion is what #8047 needs, because deleting the
/// header word must not unroot anything.
///
/// The table stores a packed [`ShapeRecord`] in a chunked slab whose record
/// addresses never move (#9706, `shapes_store.rs`); the incremental collector
/// retains enumerated slot addresses across budgeted resumptions, so that
/// stability is load-bearing. This value is the UNPACKED copy the rest of the
/// runtime consumes, and `record` carries the address of the slab record it
/// was lifted from so a traced receiver can hand the collector a rewritable
/// `keys` location without a second table probe (#8122's one-probe rule).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapeDescriptor {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI. Keeping
    /// this u64 preserves identical representation on ILP32/LP64.
    pub(crate) keys: u64,
    /// Address of the slab record this value was lifted from, or 0 for a
    /// descriptor built outside the table (equality comparisons, tests).
    /// Never part of shape IDENTITY — see the hand-written `PartialEq` below.
    pub(crate) record: usize,
    /// Is this shape carried by at least one OLD-generation object?
    ///
    /// #8112's liveness gate. A minor never enumerates old objects, so the
    /// per-receiver edge cannot express "an old object still carries this
    /// shape" — and the record is SHARED, so no per-parent remembered-set
    /// entry can either (one sibling's rewrite creates an old→young edge for a
    /// parent the minor never visits). This flag is what the shape table roots
    /// on. It is sticky within an epoch and recomputed by every full trace, so
    /// it over-approximates by at most one full collection: exactly the
    /// generational contract, and never unconditional rooting.
    ///
    /// The record also keeps the notes accumulated since the last full trace
    /// (`RECORD_FLAG_OLD_CARRIER_SEEN`), adopted into this bit by
    /// [`rotate_old_carrier_epoch_after_full_trace`]; the copy carries only
    /// the adopted gate.
    pub(crate) old_carrier: bool,
    /// A runtime optimization cache can reinstall this historical shape even
    /// while no live object currently carries it. Such a cache is an explicit
    /// strong metadata owner, so collection must root and rewrite `keys` before
    /// weak descriptor pruning.
    pub(crate) cache_carrier: bool,
    pub(crate) logical_key_count: u32,
    pub(crate) live_inline_slot_count: u32,
    /// Zero for ordinary structural shapes. Descriptor/prototype mutations
    /// mint a process-unique nonzero generation so two semantically different
    /// layouts can never compare equal merely because their keys/counts do.
    pub(crate) semantic_generation: u64,
    /// The receiver's [[Prototype]] identity ([`object_proto_id`]): part of
    /// the shape's identity, so one ShapeId names one prototype.
    pub(crate) proto_id: u64,
    /// Semantic receiver kind carried by this exact ShapeId. This is kept in
    /// the authoritative descriptor rather than `GcHeader::_reserved`, whose
    /// bits belong to the GC layout/age protocol and object feature flags.
    pub(crate) object_kind: ShapeObjectKind,
    /// Tombstoned key slots (`TAG_HOLE`) left by O(1) deletes; the live key
    /// count is `logical_key_count - hole_count`. Ordinarily immutable per id;
    /// an owned ordinary receiver carrying `OBJ_FLAG_STABLE_TOMBSTONES`
    /// updates this count in place while per-slot IC validation protects the
    /// stable id (#9064).
    pub(crate) hole_count: u32,
    /// Charter step 3: the attribute summary (`key_attrs::SUMMARY_*`) of the
    /// keys this shape names — what may be an accessor, non-writable,
    /// non-enumerable or non-configurable. Zero for an all-default shape.
    pub(crate) summary: u8,
    /// Charter step 5: the per-slot field representation (`field_rep`).
    /// Compared under [`field_rep::identity`](super::field_rep::identity).
    pub(crate) rep: u64,
    /// For a `REP_SPECIAL` lane, one means ConstFn; zero reserves NoPointer.
    pub(crate) special_constfn_mask: u32,
    /// Borrowed record-owned extension; valid while this descriptor's ShapeId
    /// remains live. Never an independent GC root or a lookup table.
    pub(crate) extras: u64,
}

/// Shape identity is the FACTS, never the storage address. A descriptor value
/// lifted out of the table compares equal to the record it came from.
impl ShapeDescriptor {
    #[inline]
    pub(crate) fn constfn_infos(&self) -> &[shapes_store::ConstFnSlotInfo] {
        if self.extras == 0 {
            &[]
        } else {
            // SAFETY: the live slab record owns the extension; the descriptor
            // is only used while its id is live, like its `record` pointer.
            unsafe { &(*(self.extras as usize as *const shapes_store::ShapeExtras)).constfn_infos }
        }
    }

    /// The private brands every receiver of this shape carries (#11791),
    /// sorted. Empty for every shape no private element was installed on.
    #[inline]
    pub(crate) fn brands(&self) -> &[u64] {
        if self.extras == 0 {
            &[]
        } else {
            // SAFETY: as for `constfn_infos`.
            unsafe { &(*(self.extras as usize as *const shapes_store::ShapeExtras)).brands }
        }
    }

    #[inline]
    pub(crate) fn deprecation_targets(&self) -> (u32, u32) {
        if self.extras == 0 {
            (0, 0)
        } else {
            // SAFETY: a live descriptor borrows the record-owned extension.
            let extras = unsafe { &*(self.extras as usize as *const shapes_store::ShapeExtras) };
            (
                extras
                    .to_nopointer
                    .load(std::sync::atomic::Ordering::Acquire),
                extras.to_any.load(std::sync::atomic::Ordering::Acquire),
            )
        }
    }

    /// This shape's ordered keys: the keys array and the shape's own count,
    /// which is the authority (the array can be a longer shared backing).
    #[inline]
    pub(crate) fn keys_view(&self) -> crate::object::ObjectKeys {
        crate::object::ObjectKeys::new(
            self.keys as usize as *mut ArrayHeader,
            self.logical_key_count,
        )
    }

    /// The one `keys` word the collector rewrites for this shape, or `None`
    /// for a descriptor value that was never lifted out of the table. The
    /// collector itself asks [`ShapeRecordRef::keys_slot`] (#10362).
    #[cfg(test)]
    #[inline]
    pub(crate) fn keys_slot(&self) -> Option<*mut u64> {
        self.record_ref().map(ShapeRecordRef::keys_slot)
    }

    /// The slab record this value was lifted from, or `None` for a descriptor
    /// built outside the table.
    #[inline]
    pub(crate) fn record_ref(&self) -> Option<ShapeRecordRef> {
        std::ptr::NonNull::new(self.record as *mut ShapeRecord).map(ShapeRecordRef)
    }
}

/// One live slab record, borrowed in place rather than lifted (#10362).
///
/// The collector asks three things of a traced receiver's shape: the live
/// inline-slot bound, the record's own `keys` word (the rewritable edge,
/// #8112), and the record's liveness bits. All three live in the record, so it
/// resolves this handle ONCE per receiver (#8122's one-probe rule) and threads
/// it through every step instead of a lifted [`ShapeDescriptor`]. The lifted
/// copy is 40 bytes and rode on the per-object `HeapChildSlotIterator`, which
/// made that iterator too large to move without an out-of-line `memmove`.
///
/// Validity is exactly `ShapeDescriptor::record`'s, which the carrier notes
/// already write through: record addresses never move (#9706), and a record's
/// chunk is released only by `shrink_shape_tables` at the end of a major
/// collection, after every enumeration of the cycle that resolved it.
#[derive(Clone, Copy)]
pub(crate) struct ShapeRecordRef(std::ptr::NonNull<ShapeRecord>);

impl ShapeRecordRef {
    /// The record's live inline-slot bound — the same fact a lifted
    /// descriptor's `live_inline_slot_count` copies.
    #[inline]
    pub(crate) fn live_inline_slot_count(self) -> u32 {
        // SAFETY: a live slab record (type docs).
        unsafe { (*self.0.as_ptr()).live_inline_slot_count }
    }

    /// The record's attribute summary (`key_attrs::SUMMARY_*`): one load,
    /// asked before any per-key attribute lookup.
    #[inline]
    pub(crate) fn summary(self) -> u8 {
        // SAFETY: a live slab record (type docs).
        unsafe { (*self.0.as_ptr()).summary() }
    }

    /// The record's logical key count (how many entries of `keys` the shape
    /// describes).
    #[inline]
    pub(crate) fn logical_key_count(self) -> u32 {
        // SAFETY: a live slab record (type docs).
        unsafe { (*self.0.as_ptr()).logical_key_count }
    }

    /// The record's current `keys` word (0 for a keyless shape).
    #[inline]
    pub(crate) fn keys(self) -> u64 {
        // SAFETY: a live slab record (type docs).
        unsafe { (*self.0.as_ptr()).keys }
    }

    /// The `keys` word's address: the slot the collector marks through and
    /// rewrites in place. `keys` is the first field of the `#[repr(C)]` slab
    /// record, so the record address IS the slot address.
    #[inline]
    pub(crate) fn keys_slot(self) -> *mut u64 {
        self.0.as_ptr() as *mut u64
    }

    /// The record's [[Prototype]] identity word as a GC edge
    /// (`shapes_prototype`): its address when the identity names a heap
    /// object, else `None`. Every shape of one prototype hands the collector
    /// the same word to mark through and rewrite in place.
    #[inline]
    pub(crate) fn prototype_slot(self, dedupe: bool) -> Option<*mut u64> {
        // SAFETY: a live slab record (type docs).
        shapes_prototype::identity_edge_slot(unsafe { (*self.0.as_ptr()).proto_id }, dedupe)
    }

    /// The record's field-representation word (`field_rep`), deprecated
    /// lanes included.
    #[inline]
    pub(crate) fn rep(self) -> u64 {
        self.rep_word().load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Which `REP_SPECIAL` lanes hold closure pointers, so the collector
    /// still visits them. A zero bit reserves NoPointer for a later P5 mint.
    #[inline]
    pub(crate) fn special_constfn_mask(self) -> u32 {
        // SAFETY: a live slab record (type docs); identity is immutable.
        unsafe { (*self.0.as_ptr()).special_constfn_mask() }
    }

    #[inline]
    pub(crate) fn constfn_info(self, slot: u32) -> Option<u64> {
        // SAFETY: a live slab record (type docs).
        unsafe {
            (*self.0.as_ptr())
                .constfn_infos()
                .iter()
                .find(|entry| u32::from(entry.slot) == slot)
                .map(|entry| entry.info)
        }
    }

    #[inline]
    pub(crate) fn deprecate_special_to_any(self, slot: u32) -> bool {
        // SAFETY: a live slab record (type docs), with an atomic learned bit.
        unsafe { (*self.0.as_ptr()).deprecate_special_to_any(slot) }
    }

    #[inline]
    pub(crate) fn has_special_deprecation(self) -> bool {
        // SAFETY: a live slab record (type docs).
        unsafe { (*self.0.as_ptr()).deprecation_targets().1 != 0 }
    }

    /// Mark `slot` deprecated (`F64` -> `10`, `field_rep`): the lineage has
    /// generalized it. A learned fact of the record, masked out of identity,
    /// so the record's facts key and its `by_facts` bucket do not move, and
    /// every object carrying the record still satisfies the `F64` invariant
    /// at `slot`. The word is written atomically because readers of a
    /// published record never take the table borrow.
    #[inline]
    /// Returns whether this call deprecated the lane (it was `F64`).
    pub(crate) fn deprecate_rep_slot(self, slot: u32) -> bool {
        use super::field_rep::{slot_rep, with_slot_rep, REP_F64, REP_F64_DEPRECATED};
        let word = self.rep_word();
        let rep = word.load(std::sync::atomic::Ordering::Relaxed);
        if slot_rep(rep, slot) != REP_F64 {
            return false;
        }
        let next = with_slot_rep(rep, slot, REP_F64_DEPRECATED);
        word.store(next, std::sync::atomic::Ordering::Release);
        true
    }

    #[inline]
    fn rep_word(self) -> &'static std::sync::atomic::AtomicU64 {
        // SAFETY: a live slab record (type docs); `rep` is an 8-aligned u64
        // (the record is `#[repr(C)]`, asserted 8-aligned), and a slab record
        // is never moved while present.
        unsafe {
            &*std::ptr::addr_of_mut!((*self.0.as_ptr()).rep).cast::<std::sync::atomic::AtomicU64>()
        }
    }
}

/// Test instrument: reads the receiver's shape answered at a latched
/// megamorphic site (compiled into test builds only).
#[cfg(test)]
pub(crate) static SHAPE_ANSWERED_READS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Test instrument: SPILL-located reads the receiver's shape answered at a
/// latched megamorphic site (S5; test builds only).
#[cfg(test)]
pub(crate) static SHAPE_ANSWERED_SPILL_READS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

impl ShapeRecordRef {
    /// The INLINE slot at which this shape stores `key`, answered from the
    /// shape's own canonical key list — or `None` when the shape cannot answer
    /// by position alone.
    ///
    /// The shape's [`ShapeRecord::position_bound`] says how many leading key
    /// positions ARE inline slots of every receiver carrying it (0 for a
    /// dictionary, class, descriptor/prototype-generation or tombstoned
    /// shape). The list is bounded by that — never by the backing's length
    /// (#10969: one backing per growth chain).
    ///
    /// Key compares go identity first: canonical lists hold their text's ATOM
    /// (`string::intern::AtomTable`), which is also what a read site's pooled
    /// key is, so the site's guess, and then any position, matches by one
    /// pointer compare. A byte pass remains for a list written before its
    /// atom existed (and for SSO slots) — a pointer MISmatch proves nothing.
    /// Allocation-free, never calls user code.
    #[inline]
    pub(crate) unsafe fn inline_slot_of_key(
        self,
        key: *const crate::StringHeader,
        hint: usize,
    ) -> Option<usize> {
        let r = &*self.0.as_ptr();
        let bound = r.position_bound() as usize;
        if bound == 0 {
            return None;
        }
        let (slots, len) =
            super::keys_array_dense_slots_resolved(r.keys as usize as *const ArrayHeader);
        if slots.is_null() {
            return None;
        }
        let bound = bound.min(len);
        let heap_bits = crate::JSValue::string_ptr(key as *mut crate::StringHeader).bits();
        // The site's slot guess, confirmed by the receiver's own key.
        if hint < bound && (*slots.add(hint)).to_bits() == heap_bits {
            return Some(hint);
        }
        // Identity over the whole list before any byte is compared.
        if let Some(i) = (0..bound).find(|&i| (*slots.add(i)).to_bits() == heap_bits) {
            return Some(i);
        }
        (0..bound).find(|&i| stored_key_matches(key, (*slots.add(i)).to_bits()))
    }

    /// The SPILL position at which this shape stores `key` as an own DATA
    /// property — a position at or past `live_inline_slot_count`, which is
    /// also the key's index in the receiver's spill buffer — or `None` when
    /// the shape cannot answer by position alone (the same refusals as
    /// [`Self::inline_slot_of_key`]), the key is not spill-located, or it is
    /// an accessor.
    ///
    /// Scanned back to front, as the read cache's prime scans (#10595: a
    /// shadowed field's most-derived position wins). Only meaningful after
    /// [`Self::inline_slot_of_key`] declined, which is the one order the
    /// megamorphic read asks in. Allocation-free, never calls user code.
    #[inline]
    pub(crate) unsafe fn spill_position_of_key(
        self,
        key: *const crate::StringHeader,
    ) -> Option<usize> {
        let r = &*self.0.as_ptr();
        if !r.object_kind().is_ordinary_layout()
            || r.semantic_generation != 0
            || r.hole_count != 0
            || r.keys == 0
        {
            return None;
        }
        let keys = r.keys as usize as *const ArrayHeader;
        let (slots, len) = super::keys_array_dense_slots_resolved(keys);
        if slots.is_null() {
            return None;
        }
        let lo = r.live_inline_slot_count as usize;
        let hi = len.min(r.logical_key_count as usize);
        let pos = (lo..hi)
            .rev()
            .find(|&i| stored_key_matches(key, (*slots.add(i)).to_bits()))?;
        let accessor = r.summary() & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
            && crate::object::key_attrs::keys_entry(keys, pos as u32)
                & crate::object::key_attrs::ENTRY_ACCESSOR
                != 0;
        (!accessor).then_some(pos)
    }

    /// The own DATA property named by the string value `key_bits` (whose
    /// text is `key_bytes`) on an ordinary receiver carrying this shape:
    /// `Some(Some(slot))` when it is inline slot `slot`, `Some(None)` when the
    /// shape's own key list does not name it, `None` when the list alone
    /// cannot answer (a class or dictionary layout, an accessor key, a
    /// spilled key). Another key's attributes do not matter. An ordinary layout's key position is its slot in every
    /// generation (a descriptor, prototype or tombstone epoch re-keys the
    /// shape, not the layout).
    ///
    /// Unlike [`Self::inline_slot_of_key`] it answers through tombstones: a
    /// `TAG_HOLE` entry names no key, and every live key keeps its position as
    /// its slot. The caller still reads the slot and treats a `TAG_HOLE` value
    /// as no answer. Identity compares first, then bytes, as for
    /// [`stored_key_matches`]. Allocation-free, never calls user code.
    #[inline]
    pub(crate) unsafe fn own_data_slot_of_value(
        self,
        key_bits: u64,
        key_bytes: &[u8],
    ) -> Option<Option<u32>> {
        let live = (*self.0.as_ptr()).live_inline_slot_count;
        match self.own_data_position_of_value(key_bits, key_bytes)? {
            Some(pos) if pos >= live => None,
            found => Some(found),
        }
    }

    /// [`Self::own_data_slot_of_value`]'s key POSITION, inline or not: a
    /// position at or past the live inline count is the key's index in the
    /// receiver's spill buffer (`object_field_at_with_live` reads either).
    #[inline]
    pub(crate) unsafe fn own_data_position_of_value(
        self,
        key_bits: u64,
        key_bytes: &[u8],
    ) -> Option<Option<u32>> {
        let r = &*self.0.as_ptr();
        if !r.object_kind().is_ordinary_layout() {
            return None;
        }
        if r.keys == 0 {
            return (r.logical_key_count == 0).then_some(None);
        }
        let (slots, len) =
            super::keys_array_dense_slots_resolved(r.keys as usize as *const ArrayHeader);
        let count = r.logical_key_count as usize;
        if slots.is_null() || len < count {
            return None;
        }
        // SSO is canonical (length and bytes in the bits), so an SSO entry
        // names the key exactly when its bits are the key's SSO form.
        let sso = crate::JSValue::try_short_string(key_bytes).map(|value| value.bits());
        let pos = (0..count).find(|&i| {
            let bits = (*slots.add(i)).to_bits();
            bits == key_bits || stored_bits_name(key_bytes, sso, bits)
        });
        match pos {
            None => Some(None),
            // An accessor key's slot holds its pair, not a value.
            Some(i)
                if r.summary() & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
                    && crate::object::key_attrs::keys_entry(
                        r.keys as usize as *mut ArrayHeader,
                        i as u32,
                    ) & crate::object::key_attrs::ENTRY_ACCESSOR
                        != 0 =>
            {
                None
            }
            Some(i) => Some(Some(i as u32)),
        }
    }
}

/// Does the key-list entry `bits` (a heap or SSO string; anything else, a
/// tombstone included, names no key) spell `bytes`, whose SSO form (when it
/// has one) is `sso`?
#[inline]
unsafe fn stored_bits_name(bytes: &[u8], sso: Option<u64>, bits: u64) -> bool {
    match bits >> 48 {
        0x7FFF => {
            let sp = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
            !sp.is_null()
                && (*sp).byte_len as usize == bytes.len()
                && bytes_eq(crate::string::string_data(sp), bytes.as_ptr(), bytes.len())
        }
        0x7FF9 => sso == Some(bits),
        _ => false,
    }
}

/// Does the key-list entry `bits` name `key`? A canonical list holds its
/// text's ATOM where one exists (the site's pooled key), but must not be
/// assumed to: a list written before its atom existed holds another heap
/// string, and a slot may be an SSO immediate. So a stored key matches by
/// identity, by SSO identity, or by (byte length, bytes).
#[inline]
unsafe fn stored_key_matches(key: *const crate::StringHeader, bits: u64) -> bool {
    if bits == crate::JSValue::string_ptr(key as *mut crate::StringHeader).bits() {
        return true;
    }
    let klen = (*key).byte_len as usize;
    match bits >> 48 {
        0x7FFF => {
            let sp = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
            !sp.is_null()
                && (*sp).byte_len as usize == klen
                && bytes_eq(
                    crate::string::string_data(sp),
                    crate::string::string_data(key),
                    klen,
                )
        }
        // An SSO immediate in the list: rare; compare its bytes.
        0x7FF9 => {
            let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut buf)
                == Some(std::slice::from_raw_parts(
                    crate::string::string_data(key),
                    klen,
                ))
        }
        _ => false,
    }
}

/// The position bound of `obj`'s shape (S3c), or `None` when its word names
/// no record in this agent.
#[cfg(test)]
pub(crate) fn test_position_bound_of(obj: *const crate::object::ObjectHeader) -> Option<u32> {
    let id = unsafe { object_shape_stamp(obj) };
    shape_record_by_id(id).map(|r| unsafe { (*r.0.as_ptr()).position_bound() })
}

/// Walk every present record of this agent's slab: `(records, positional,
/// ordinary records with an accessor key, disagreements)`, where a
/// disagreement is a record whose stored POSBOUND differs from
/// [`ShapeRecord::position_bound_by_facts`].
#[cfg(test)]
pub(crate) fn test_positional_census() -> (usize, usize, usize, Vec<u32>) {
    let table = &crate::state::state().shapes;
    let (mut n, mut positional, mut accessor, mut bad) = (0usize, 0usize, 0usize, Vec::new());
    table.slab().for_each(|id, p| {
        let r = unsafe { &*p };
        if !r.present() {
            return;
        }
        n += 1;
        if r.stored_position_bound() > 0 {
            positional += 1;
        }
        if r.object_kind().is_ordinary_layout()
            && r.summary() & crate::object::key_attrs::SUMMARY_ACCESSOR != 0
        {
            accessor += 1;
        }
        if r.stored_position_bound() != r.position_bound_by_facts() {
            bad.push(id);
        }
    });
    (n, positional, accessor, bad)
}

/// `(stored POSBOUND, its definition)` for shape `id`.
#[cfg(test)]
pub(crate) fn test_positional_of_id(id: u32) -> Option<(u32, u32)> {
    shape_record_by_id(id).map(|r| unsafe {
        let r = &*r.0.as_ptr();
        (r.stored_position_bound(), r.position_bound_by_facts())
    })
}

/// The address of this thread's ordinary shape-directory mirror, which
/// [`positional_key_words`] reads through (`agent_ptrs` slot
/// `AGENT_PTR_SHAPE_DIR`).
#[inline]
pub(crate) fn ordinary_dir_addr() -> *const u8 {
    ShapeSlab::ordinary_dir_addr()
}

/// The position bound of shape `id` (S3c), or `None` when it names no record.
#[cfg(test)]
pub(crate) fn test_position_bound_of_id(id: u32) -> Option<u32> {
    shape_record_by_id(id).map(|r| unsafe { (*r.0.as_ptr()).position_bound() })
}

/// Shape `shape_id`'s positional key words, for the megamorphic read confirm
/// (`ic_miss::read_confirm::js_object_get_field_ic_front`): `(the keys
/// array, POSBOUND)` when the record answers by position — key position
/// `i < POSBOUND` IS inline slot `i` of every receiver carrying the shape —
/// and `None` otherwise (no ordinary record under that id in this agent, or
/// POSBOUND 0: a dictionary, class, descriptor/prototype-generation,
/// tombstoned or accessor-keyed shape).
///
/// A canonical list holds its text's ATOM, boxed exactly as a site's pool
/// entry holds it, so the caller compares key WORDS: equality is identity,
/// and a mismatch proves nothing (a list written before its atom existed, an
/// SSO slot), which the caller answers by declining to the slow entry.
///
/// `dir` is this thread's ordinary directory mirror ([`ordinary_dir_addr`]),
/// as the agent's pointer block holds it.
///
/// Allocation-free, no user code, no formatting path: it is part of a
/// GC-leaf stub.
///
/// # Safety
/// `dir` is this thread's [`ordinary_dir_addr`] or `PERRY_EMPTY_SHAPE_DIR` (never
/// null); any `shape_id`.
/// The words are valid until the next safepoint.
#[inline(always)]
pub(crate) unsafe fn positional_key_words(
    dir: *const u8,
    shape_id: u32,
) -> Option<(PositionalKeys, usize)> {
    let r = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    Some((
        PositionalKeys(r.keys as usize as *const ArrayHeader),
        r.position_bound_raw() as usize,
    ))
}

/// [`positional_key_words`] for the computed-key read
/// (`object::dynamic_key_read`), which also refuses a shape whose
/// [[Prototype]] identity is PER-OBJECT.
///
/// A read site's front is reached only past its own receiver tests; the
/// computed-key read has no site, so it takes this exclusion from the shape
/// itself. A per-object identity is how an exotic read receiver
/// (`process.env`, an arguments object) and a module namespace project "its
/// reads are not answered by its key list" into their shape
/// ([`object_proto_id`]), so such a list naming the key proves nothing.
///
/// # Safety
/// As [`positional_key_words`].
#[inline]
pub(crate) unsafe fn plain_positional_key_words(
    dir: *const u8,
    shape_id: u32,
) -> Option<(PositionalKeys, usize)> {
    let r = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    if r.proto_id == PROTO_ID_PER_OBJECT {
        return None;
    }
    Some((
        PositionalKeys(r.keys as usize as *const ArrayHeader),
        r.position_bound_raw() as usize,
    ))
}

/// Own-key presence of the string key `key_bits` (whose bytes are `key`) on
/// a receiver of shape `shape_id`, for `[[GetOwnProperty]]` existence
/// (`Object.hasOwn`, `Object.prototype.hasOwnProperty`): `Some((present,
/// kind))`, or `None` when the record does not answer for its receivers' own
/// keys and the caller asks the object.
///
/// The record answers when [`plain_positional_key_words`] would: POSBOUND is
/// nonzero (an ordinary layout with a canonical key list, no tombstone holes,
/// no accessor keys, no descriptor generation) and the [[Prototype]] identity
/// is not per-object (no `process.env`, arguments object or module
/// namespace, whose own properties are not their key list). A shape carrying
/// a private key also declines: a private entry sits in the list but is no
/// property. What remains is a list whose first `logical_key_count` entries
/// ARE the receiver's own string keys, so presence is membership.
///
/// Membership is decided in the list itself, never in a side table: the key
/// word is compared first (a canonical list holds its text's atom, which is
/// the very word a pooled key literal evaluates to), and only when no word
/// matches are the texts compared, because a word mismatch proves nothing (a
/// list written before its atom existed, an SSO slot, a key built at run
/// time). A list at or past `KEYS_INDEX_THRESHOLD` asks the shape's key index
/// instead, exactly as every by-name lookup does.
///
/// `kind` is the record's object kind, for a caller whose answer also depends
/// on WHICH object a runtime-born receiver is (`%Function.prototype%`).
///
/// Allocation-free and GC-free.
///
/// # Safety
/// As [`positional_key_words`]; `key` is the text of the string `key_bits`.
#[inline]
pub(crate) unsafe fn plain_own_key_present(
    dir: *const u8,
    shape_id: u32,
    key_bits: u64,
    key: &[u8],
) -> Option<(bool, ShapeObjectKind)> {
    let r = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    if r.position_bound_raw() == 0
        || r.proto_id == PROTO_ID_PER_OBJECT
        || r.summary() & crate::object::key_attrs::SUMMARY_PRIVATE != 0
    {
        return None;
    }
    let kind = r.object_kind();
    let keys = r.keys as usize as *const ArrayHeader;
    let count = r.logical_key_count;
    if count >= super::KEYS_INDEX_THRESHOLD {
        return Some((
            super::keys_find_slot_by_bytes(keys, count, key).is_some(),
            kind,
        ));
    }
    // A live descriptor's keys array is the resolved head (the collector
    // rewrites it on move), holding at least `count` logical keys past its
    // front offset; the min keeps a short array from being over-read anyway.
    let n = (count as usize).min((*keys).length.min((*keys).capacity) as usize);
    let words = crate::array::array_elements_ptr(keys) as *const u64;
    for i in 0..n {
        if *words.add(i) == key_bits {
            return Some((true, kind));
        }
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..n {
        let stored = crate::JSValue::from_bits(*words.add(i));
        if stored.is_string() {
            let h = stored.as_string_ptr();
            if !h.is_null()
                && (*h).byte_len as usize == key.len()
                && bytes_eq(super::string_header_payload(h), key.as_ptr(), key.len())
            {
                return Some((true, kind));
            }
        } else if let Some(text) = crate::string::js_string_key_bytes(stored, &mut sso) {
            if text == key {
                return Some((true, kind));
            }
        }
    }
    Some((false, kind))
}

/// A record's canonical keys array, for [`positional_key_words`]: its words
/// are asked only once POSBOUND is known to be nonzero, so the front-offset
/// arithmetic runs only on the path that reads a key.
pub(crate) struct PositionalKeys(*const ArrayHeader);

impl PositionalKeys {
    /// The first logical key word.
    ///
    /// # Safety
    /// Only when the record's POSBOUND is nonzero: then `keys` names a live
    /// keys array (the collector marks through and rewrites `keys`) holding
    /// at least POSBOUND logical keys. Logical element `i` sits past the
    /// array's FRONT OFFSET, which a canonical list can carry without ever
    /// being shifted (a size-class round-up alone makes the physical capacity
    /// exceed the logical one: `keys_front_offset_tests`), so the accessor is
    /// asked, never `+8`.
    #[inline(always)]
    pub(crate) unsafe fn words(&self) -> *const u64 {
        crate::array::array_elements_ptr(self.0) as *const u64
    }
}

/// Does shape `shape_id` store the key whose NaN-boxed bits are `key_bits`
/// at inline slot `guess`, by position? See [`positional_key_words`].
///
/// # Safety
/// As [`positional_key_words`].
#[cfg(test)]
pub(crate) unsafe fn slot_guess_confirmed(
    dir: *const u8,
    shape_id: u32,
    key_bits: u64,
    guess: usize,
) -> bool {
    positional_key_words(dir, shape_id)
        .is_some_and(|(keys, bound)| guess < bound && *keys.words().add(guess) == key_bits)
}

/// Byte equality without a libc call for the short keys property names are.
#[inline]
unsafe fn bytes_eq(a: *const u8, b: *const u8, n: usize) -> bool {
    let mut i = 0;
    while i + 8 <= n {
        if std::ptr::read_unaligned(a.add(i) as *const u64)
            != std::ptr::read_unaligned(b.add(i) as *const u64)
        {
            return false;
        }
        i += 8;
    }
    while i < n {
        if *a.add(i) != *b.add(i) {
            return false;
        }
        i += 1;
    }
    true
}

impl PartialEq for ShapeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.keys == other.keys
            && self.logical_key_count == other.logical_key_count
            && self.live_inline_slot_count == other.live_inline_slot_count
            && self.semantic_generation == other.semantic_generation
            && self.proto_id == other.proto_id
            && self.object_kind == other.object_kind
            && self.hole_count == other.hole_count
            && self.summary == other.summary
            && super::field_rep::identity_with_special(self.rep)
                == super::field_rep::identity_with_special(other.rep)
            && self.special_constfn_mask == other.special_constfn_mask
            && self.constfn_infos() == other.constfn_infos()
            && shapes_store::brand_lists_equal(self.brands(), other.brands())
    }
}

impl Eq for ShapeDescriptor {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ShapeObjectKind {
    /// An ordinary object whose own slots are proven plain data (a class
    /// instance, or a class-less receiver a birth site marked
    /// `OBJ_FLAG_PLAIN_ORDINARY`) and that carries no Array-subclass numeric
    /// proof. The ONE kind a store may be admitted on by its ShapeId alone
    /// (charter step 3, `shapes_store_kind`).
    Ordinary,
    Class,
    /// #10868: the receiver's keys live per-object, not in a shared keys
    /// array. Held as a KIND rather than a side-table predicate so that
    /// `object_is_regular` declines the fast lanes by construction: a keyless
    /// shape otherwise reads as "this receiver has no own properties" to a
    /// long tail of consumers, four of which return wrong values and one of
    /// which writes out of bounds (#10942). A set whose membership is partly
    /// accidental cannot be secured by enumerating it.
    Dictionary,
    /// A function object (`GC_TYPE_CLOSURE`) whose own properties are exactly
    /// the intrinsic ones (`name`, `length`, `prototype`) and whose
    /// [[Prototype]] is the intrinsic its body kind names (the shape's
    /// `proto_id`). Minted in the exotic band: no per-site own-slot cache may
    /// hold it, because a closure's +16 is not an inline slot.
    Function,
    /// A function object that carries anything else (a user property, a
    /// deleted or redefined intrinsic, an accessor, a recorded
    /// [[Prototype]]): the answer lives on the object, as for `Dictionary`.
    FunctionDictionary,
    /// The ordinary layout of [`ShapeObjectKind::Ordinary`], on a receiver
    /// whose own slots are NOT proven plain data: a class-less receiver no
    /// birth site marked ordinary (`URL`, `Object.prototype`, a typed-array
    /// prototype, or a runtime-born record). Reads
    /// treat it as `Ordinary`; a store is never admitted on it by shape.
    OrdinaryUnmarked,
    /// [`ShapeObjectKind::Ordinary`] carrying the Array-subclass packed-numeric
    /// proof (`array::subclass`). Entered only by publishing the proof and
    /// left by retiring it (or by any other shape change), so the proof is a
    /// shape transition and a store word naming an `Ordinary` id can never
    /// match a proof-carrying receiver.
    OrdinaryNumericProof,
    /// A native-module namespace whose own keys and reads come from its
    /// export surface. The namespace brand is part of shape identity, so an
    /// ordinary record with a module name in a data slot cannot impersonate it.
    NativeNamespace,
}

impl ShapeObjectKind {
    /// A non-`GC_TYPE_OBJECT` receiver kind: minted in the exotic band.
    #[inline]
    pub(crate) fn is_exotic(self) -> bool {
        matches!(
            self,
            ShapeObjectKind::Function | ShapeObjectKind::FunctionDictionary
        )
    }

    /// The ordinary object LAYOUT: every kind whose receiver reads exactly as
    /// an `Ordinary` one. What every layout / read consumer asks; only the
    /// store admission asks `== Ordinary`.
    #[inline]
    pub(crate) fn is_ordinary_layout(self) -> bool {
        matches!(
            self,
            ShapeObjectKind::Ordinary
                | ShapeObjectKind::OrdinaryUnmarked
                | ShapeObjectKind::OrdinaryNumericProof
                | ShapeObjectKind::NativeNamespace
        )
    }

    /// The discriminant `facts_key` folds and `ShapeRecord` stores. Stable:
    /// it is written into a record field, so the values may not be reordered.
    #[inline]
    pub(crate) fn code(self) -> u64 {
        match self {
            ShapeObjectKind::Ordinary => 0,
            ShapeObjectKind::Class => 1,
            ShapeObjectKind::Dictionary => 2,
            ShapeObjectKind::Function => 3,
            ShapeObjectKind::FunctionDictionary => 4,
            ShapeObjectKind::OrdinaryUnmarked => 5,
            ShapeObjectKind::OrdinaryNumericProof => 6,
            ShapeObjectKind::NativeNamespace => 7,
        }
    }
}

/// Per-agent direct cache for the immutable `object_kind` half of a ShapeId.
/// A collision only falls back to the descriptor table. Entries contain no
/// managed address, and descriptor retirement clears a matching id before it
/// can be observed without the authoritative table record.
pub(crate) const SHAPE_KIND_CACHE_SIZE: usize = 16_384;
const SHAPE_KIND_CACHE_MASK: usize = SHAPE_KIND_CACHE_SIZE - 1;
const SHAPE_KIND_ORDINARY: u64 = 1;
const SHAPE_KIND_CLASS: u64 = 2;
const SHAPE_KIND_DICTIONARY: u64 = 3;
const SHAPE_KIND_FUNCTION: u64 = 4;
const SHAPE_KIND_FUNCTION_DICTIONARY: u64 = 5;
const SHAPE_KIND_ORDINARY_UNMARKED: u64 = 6;
const SHAPE_KIND_ORDINARY_NUMERIC_PROOF: u64 = 7;
const SHAPE_KIND_NATIVE_NAMESPACE: u64 = 8;

#[inline(always)]
fn shape_kind_cache_slot(shape_id: u32) -> usize {
    let mixed = u64::from(shape_id).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (mixed ^ (mixed >> 32)) as usize & SHAPE_KIND_CACHE_MASK
}

#[inline]
fn cached_shape_object_kind(shape_id: u32) -> Option<ShapeObjectKind> {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let packed = cache[shape_kind_cache_slot(shape_id)];
    if (packed >> 32) as u32 != shape_id {
        return None;
    }
    match packed & 0xFFFF_FFFF {
        SHAPE_KIND_ORDINARY => Some(ShapeObjectKind::Ordinary),
        SHAPE_KIND_CLASS => Some(ShapeObjectKind::Class),
        SHAPE_KIND_DICTIONARY => Some(ShapeObjectKind::Dictionary),
        SHAPE_KIND_FUNCTION => Some(ShapeObjectKind::Function),
        SHAPE_KIND_FUNCTION_DICTIONARY => Some(ShapeObjectKind::FunctionDictionary),
        SHAPE_KIND_ORDINARY_UNMARKED => Some(ShapeObjectKind::OrdinaryUnmarked),
        SHAPE_KIND_ORDINARY_NUMERIC_PROOF => Some(ShapeObjectKind::OrdinaryNumericProof),
        SHAPE_KIND_NATIVE_NAMESPACE => Some(ShapeObjectKind::NativeNamespace),
        _ => None,
    }
}

#[inline]
fn publish_shape_object_kind(shape_id: u32, kind: ShapeObjectKind) {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let tag = match kind {
        ShapeObjectKind::Ordinary => SHAPE_KIND_ORDINARY,
        ShapeObjectKind::Class => SHAPE_KIND_CLASS,
        ShapeObjectKind::Dictionary => SHAPE_KIND_DICTIONARY,
        ShapeObjectKind::Function => SHAPE_KIND_FUNCTION,
        ShapeObjectKind::FunctionDictionary => SHAPE_KIND_FUNCTION_DICTIONARY,
        ShapeObjectKind::OrdinaryUnmarked => SHAPE_KIND_ORDINARY_UNMARKED,
        ShapeObjectKind::OrdinaryNumericProof => SHAPE_KIND_ORDINARY_NUMERIC_PROOF,
        ShapeObjectKind::NativeNamespace => SHAPE_KIND_NATIVE_NAMESPACE,
    };
    cache[shape_kind_cache_slot(shape_id)] = (u64::from(shape_id) << 32) | tag;
}

#[inline]
fn retire_cached_shape_object_kind(shape_id: u32) {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    let entry = &mut cache[shape_kind_cache_slot(shape_id)];
    if (*entry >> 32) as u32 == shape_id {
        *entry = 0;
    }
}

#[cfg(test)]
#[inline]
fn clear_shape_object_kind_cache() {
    let cache = unsafe { &mut *crate::state::state().object_hot.shape_kind_cache.get() };
    cache.fill(0);
}

struct ShapeTableInner {
    indices: crate::fast_hash::PtrHashMap<usize, ShapeIndex>,
    /// Exact-facts accelerator (#9706): the 64-bit fold of a descriptor's six
    /// identity facts (`shapes_store::facts_key`) -> the ids carrying those
    /// facts. Almost always one id; more than one is legal when a worker
    /// minted a local descriptor before a process-global module id arrived,
    /// or on a 64-bit collision — every hit re-validates the slab record, so
    /// a collision costs a second record read, never a wrong answer. This
    /// replaces the `ShapeFacts`-keyed map whose 32-byte key and 24-byte
    /// `Vec` value made it the largest of the old reverse indices.
    ///
    /// The key is a fold of internal shape state (never program input), so
    /// `PtrHasher` (#8125) is the right hasher: the word is already mixed.
    by_facts: crate::fast_hash::PtrHashMap<u64, IdList>,
    /// Keys-array address -> every descriptor id currently indexed under it.
    /// Same-address retirement, squeeze rekeying and GC relocation all work
    /// per family instead of per descriptor, and the family is what the
    /// metadata scan probes ONCE per keys array.
    ///
    /// A family is small by construction for a SHARED keys array, which is
    /// immutable (mutation forks a private clone): its descriptors differ only
    /// in the birth bound, a semantic generation, the class kind, or a
    /// tombstone count. An OWNED array grows in place, and every same-address
    /// publish retires the predecessor it just superseded
    /// (`retire_owned_shape_siblings`), so its family holds the current
    /// version plus at most the cache-carried ones. Without that retirement a
    /// dictionary built by ten thousand appends kept ten thousand prefix
    /// descriptors alive until the array died.
    ///
    /// Single-word key, so `PtrHasher` (#8125).
    families: crate::fast_hash::PtrHashMap<u64, IdList>,
    /// #9754: keys-array addresses a minor can act on — the families and
    /// slot indices whose keys array is not (yet) old. A minor-scoped
    /// `scan_shape_table_rekey_mut` visits only these; see `gc/young_log.rs`.
    young_keys: crate::gc::young_log::YoungLog<u64>,
}

const SHAPE_YOUNG_LOG_NAME: &str = "shapes.families+indices";

crate::perry_thread_local! {
    /// Carrier notes can be produced while a GC walk already borrows the shape
    /// table. Keep that write-side stream separate and merge it at the next
    /// scanner entry rather than re-borrowing `ShapeTableInner` recursively.
    static SHAPE_CARRIER_YOUNG_KEYS: RefCell<crate::gc::young_log::YoungLog<u64>> =
        const { RefCell::new(crate::gc::young_log::YoungLog::new()) };
    #[cfg(test)]
    static SHAPE_YOUNG_LOG_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn note_shape_carrier_candidate(keys: u64) {
    if !crate::gc::young_log::addr_is_minor_relevant(keys as usize) {
        return;
    }
    #[cfg(test)]
    if SHAPE_YOUNG_LOG_SUPPRESSED.with(std::cell::Cell::get) {
        return;
    }
    SHAPE_CARRIER_YOUNG_KEYS.with(|log| log.borrow_mut().note(keys));
}

/// Re-export of the id-list operation counters' report, so the collector does
/// not have to name a private sibling module. One `[gc-idlist]` line per
/// copying minor under `PERRY_GC_DIAG=1`; `elems_moved` is the falsifier for
/// the swap-remove change.
#[inline]
pub(crate) fn id_list_report() {
    shapes_store::id_list_report();
}

impl ShapeTableInner {
    /// Rule 1 of `gc/young_log.rs`: log a keys address BEFORE a family or a
    /// slot index is published under it, when the keys array is not old.
    /// Every family insert funnels through `family_push_back` /
    /// `family_append_fresh` / `family_push_front`; the slot-index inserts
    /// call this themselves.
    #[inline]
    fn note_young_keys(&mut self, keys: u64) {
        #[cfg(test)]
        if SHAPE_YOUNG_LOG_SUPPRESSED.with(std::cell::Cell::get) {
            return;
        }
        if crate::gc::young_log::addr_is_minor_collectible(keys as usize) {
            self.young_keys.note(keys);
        }
    }

    #[inline]
    // #9976 removed the production rekey caller deliberately (see the
    // scanner-internal rekey note below); `shapes_test_support` is the only
    // remaining consumer, and it is `#[cfg(test)]`.
    #[cfg(test)]
    fn family_push_back(&mut self, keys: u64, id: u32) {
        self.note_young_keys(keys);
        self.families.entry(keys).or_default().push_back(id);
    }

    /// Append a FRESHLY allocated id (see [`IdList::append_unchecked`]): the
    /// id came from `alloc_shape_id`, which never reuses a value, so the
    /// membership scan `family_push_back` would run is dead work that is
    /// linear in the number of descriptors this keys array has ever had.
    #[inline]
    fn family_append_fresh(&mut self, keys: u64, id: u32) {
        self.note_young_keys(keys);
        self.families.entry(keys).or_default().append_unchecked(id);
    }

    #[inline]
    fn family_push_front(&mut self, keys: u64, id: u32) {
        self.note_young_keys(keys);
        self.families.entry(keys).or_default().push_front(id);
    }

    /// Drop `id` from the family under `keys`, removing an emptied family.
    #[inline]
    fn family_remove(&mut self, keys: u64, id: u32) -> bool {
        let Some(ids) = self.families.get_mut(&keys) else {
            return false;
        };
        // UNORDERED: a family's readers are set-valued (see `IdList`'s type
        // doc), and the ordered removal was memmoving the whole tail of a list
        // measured at up to 514,030 entries, from position ~0.31, 3.7 M times
        // per 3300-char reply. The dominant caller is the dead-owner prune
        // (`prune_dead_owner_side_tables_post_trace` ->
        // `remove_descriptor_indexed_under`); `retire_owned_shape_siblings`
        // never sees a family longer than 16.
        let removed = ids.remove_unordered(id);
        if ids.is_empty() {
            self.families.remove(&keys);
        }
        removed
    }

    #[inline]
    fn facts_push_back(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().push_back(id);
    }

    /// Fresh-id twin of [`ShapeTableInner::facts_push_back`]; same argument.
    #[inline(always)]
    fn facts_append_fresh(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().append_unchecked(id);
    }

    #[inline]
    fn facts_push_front(&mut self, facts: u64, id: u32) {
        self.by_facts.entry(facts).or_default().push_front(id);
    }

    /// Drop `id` from the accelerator bucket `facts`, removing it if emptied.
    #[inline]
    fn facts_remove(&mut self, facts: u64, id: u32) -> bool {
        let Some(ids) = self.by_facts.get_mut(&facts) else {
            return false;
        };
        // ORDERED, and it must stay ordered: `facts_push_front` is how an
        // installed process-global id becomes the canonical answer ahead of an
        // equivalent local one, and this list is read first-wins. Measured at
        // max length 1 on cc, so the order costs nothing to keep.
        let removed = ids.remove_ordered(id);
        if ids.is_empty() {
            self.by_facts.remove(&facts);
        }
        removed
    }
}

pub(crate) struct ShapeTable {
    /// The by-id store, outside the `RefCell` on purpose: `shape_descriptor_by_id`
    /// is on the hot property path (profiling a dynamic-property loop put it
    /// and `shape_descriptor_ensure_with_generation` at ~13% of main-thread
    /// samples between them), and the collector reads and writes records
    /// through raw pointers from inside walks that hold `inner` borrowed.
    /// Records are cells; every access goes through a short-lived pointer.
    slab: std::cell::UnsafeCell<ShapeSlab>,
    inner: RefCell<ShapeTableInner>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            slab: std::cell::UnsafeCell::new(ShapeSlab::new_agent()),
            inner: RefCell::new(ShapeTableInner {
                indices: crate::fast_hash::new_ptr_hash_map(),
                by_facts: crate::fast_hash::new_ptr_hash_map(),
                families: crate::fast_hash::new_ptr_hash_map(),
                young_keys: crate::gc::young_log::YoungLog::new(),
            }),
        }
    }

    /// Shared view of the slab. Sound under the single-threaded agent
    /// discipline the whole table relies on; mutation happens only through
    /// [`Self::slab_mut`] in code that holds no other slab reference.
    #[inline]
    fn slab(&self) -> &ShapeSlab {
        // SAFETY: see the field docs — one agent, one thread, no reference
        // held across a call that can insert or remove.
        unsafe { &*self.slab.get() }
    }

    /// # Safety
    ///
    /// The caller holds no other reference into the slab for the duration.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn slab_mut(&self) -> &mut ShapeSlab {
        &mut *self.slab.get()
    }
}

/// #6759 C3c: ShapeIds live in their own u32 range, disjoint from every
/// real class id (user counter tops out far below; builtin reserved
/// ranges sit at `0x7FFF_FF00..=0x7FFF_FFFF` and `0xFFFF_0000..`), so a
/// stamp in a plain object's `parent_class_id` can never be mistaken for
/// inheritance data — and vice versa.
pub(crate) const SHAPE_ID_BASE: u32 = 0x8000_0000;
// Generated code names static ids relative to the ABI's copy.
const _: () = assert!(SHAPE_ID_BASE == crate::codegen_abi::SHAPE_ID_BASE);
/// Exclusive end of the ShapeId range (2^30 ids ≈ one per shape BIRTH,
/// unreachable in practice).
pub(crate) const SHAPE_ID_END: u32 = 0xC000_0000;

/// # The dictionary band: ShapeIds no site can ever hold
///
/// The top quarter of the ShapeId range, `[DICTIONARY_SHAPE_ID_BASE,
/// SHAPE_ID_END)`, holds dictionary-mode shapes and nothing else, and ordinary
/// shapes are minted strictly below it. Membership is a fact of the id's
/// VALUE, fixed when the shape is minted from its generation namespace
/// ([`crate::object::dictionary::DICTIONARY_GENERATION_TAG`]) — not a check any
/// site makes.
///
/// Why a band: a dictionary shape describes no keys (`keys = NULL`), and its
/// receiver KEEPS the id across layout changes — an in-place append or
/// tombstone publishes nothing (`object/dictionary.rs`). A per-site memo
/// `(ShapeId, slot)` is sound only because a ShapeId names ONE immutable key
/// list forever; a dictionary id names none. So every site word is written
/// with an id from the [`is_site_matchable_shape_id`] band, which excludes this
/// one — the same way a spill entry's word is flipped out of the id range by
/// `PACKED_SPILL_FLIP` — and no emitted compare (the compact word, the ways,
/// a region word, a presence or store cache) can equal a dictionary receiver's
/// `+4` word. Everything that reads a shape from an object (`is_shape_id`,
/// the descriptor table, the collector) still sees an ordinary ShapeId.
///
/// A quarter of the range (2^28 ids) is a floor, not an estimate: a
/// dictionary draws one id at its latch and one per compacting delete or
/// inline-bound change, O(1) per object, against the ordinary band's one per
/// shape birth.
pub(crate) const DICTIONARY_SHAPE_ID_BASE: u32 = 0xB000_0000;

/// The EXOTIC band, `[EXOTIC_SHAPE_ID_BASE, SHAPE_ID_END)`: ShapeIds of
/// receivers that are not `GC_TYPE_OBJECT` (function objects today; arrays,
/// Map/Set, ... as their stages land). Like the dictionary band it is outside
/// `is_site_matchable_shape_id`, so no own-inline-slot site word (read PIC,
/// store PIC, key-add memo) can ever hold one: those caches load `recv + 16 +
/// 8*slot`, which is not a slot of these receivers. A consumer that only
/// needs the shape's IDENTITY (its prototype, its absence of own keys) opts in
/// explicitly with [`is_exotic_shape_id`].
pub(crate) const EXOTIC_SHAPE_ID_BASE: u32 = 0xB800_0000;
/// # The static band: ShapeIds the compiler assigned (design step 4)
///
/// `[SHAPE_ID_BASE, STATIC_SHAPE_ID_END)` is never drawn from the counter.
/// The driver assigns an id in it to every compiler-nameable birth shape,
/// by content, before codegen, and generated code embeds that id as an
/// IMMEDIATE. The runtime adopts it at the ordinary mint: a caller that
/// knows the static id passes it as `requested` to
/// [`shape_descriptor_ensure_with_holes`], which mints it on a by-facts miss.
/// The id means the same facts in every agent; each agent gets its own record
/// under it when it first mints those facts with the id requested.
///
/// No id-to-shape table exists: after the seed, the only record of
/// "these facts have this id" is the shape record itself, in the intern
/// structure every later mint probes.
pub(crate) const STATIC_SHAPE_ID_END: u32 =
    SHAPE_ID_BASE + crate::codegen_abi::STATIC_SHAPE_ID_COUNT;
const _: () = assert!(STATIC_SHAPE_ID_END < DICTIONARY_SHAPE_ID_BASE);

/// Is `v` in the compiler-assigned band ([`STATIC_SHAPE_ID_END`])? A fact of
/// the value alone.
#[inline]
pub(crate) fn is_static_shape_id(v: u32) -> bool {
    (SHAPE_ID_BASE..STATIC_SHAPE_ID_END).contains(&v)
}

const _: () = assert!(DICTIONARY_SHAPE_ID_BASE < EXOTIC_SHAPE_ID_BASE);
const _: () = assert!(EXOTIC_SHAPE_ID_BASE < SHAPE_ID_END);

const _: () = assert!(SHAPE_ID_BASE < DICTIONARY_SHAPE_ID_BASE);
const _: () = assert!(DICTIONARY_SHAPE_ID_BASE < SHAPE_ID_END);

/// #6759 C3c: PROCESS-GLOBAL allocator (supersedes the per-thread counter
/// C3a landed with). Global uniqueness matters because the worker
/// serializer replays `parent_class_id` verbatim: a deep-copied object's
/// stamp arriving on another thread must never alias an id that thread
/// allocated for a different shape. Monotonic — ids are NEVER reused, so
/// a stale stamp or cache entry can only miss, not falsely hit.
static SHAPE_ID_NEXT: [std::sync::atomic::AtomicU32; 3] = [
    std::sync::atomic::AtomicU32::new(STATIC_SHAPE_ID_END),
    std::sync::atomic::AtomicU32::new(STATIC_SHAPE_ID_END),
    std::sync::atomic::AtomicU32::new(STATIC_SHAPE_ID_END),
];

/// The dictionary band's own monotonic counters (see
/// [`DICTIONARY_SHAPE_ID_BASE`]), one per identity kind
/// ([`SHAPE_ID_KIND_SHIFT`]); never reused, each parks at the band's end.
static DICTIONARY_SHAPE_ID_NEXT: [std::sync::atomic::AtomicU32; 3] = [
    std::sync::atomic::AtomicU32::new(DICTIONARY_SHAPE_ID_BASE),
    std::sync::atomic::AtomicU32::new(DICTIONARY_SHAPE_ID_BASE),
    std::sync::atomic::AtomicU32::new(DICTIONARY_SHAPE_ID_BASE),
];

/// The exotic band's own monotonic counters ([`EXOTIC_SHAPE_ID_BASE`]).
static EXOTIC_SHAPE_ID_NEXT: [std::sync::atomic::AtomicU32; 3] = [
    std::sync::atomic::AtomicU32::new(EXOTIC_SHAPE_ID_BASE),
    std::sync::atomic::AtomicU32::new(EXOTIC_SHAPE_ID_BASE),
    std::sync::atomic::AtomicU32::new(EXOTIC_SHAPE_ID_BASE),
];

/// # The identity kind: a ShapeId says what kind of prototype identity it names
///
/// Bits 20-21 of a ShapeId are its identity KIND ([`proto_id_kind`]):
/// * [`SHAPE_ID_KIND_PLAIN`] (0): an identity that answers by itself and
///   records no prototype: the realm default, a compiled class's declaration
///   prototype, a per-object identity;
/// * [`SHAPE_ID_KIND_WORD`] (1): a LINKED identity with a word naming its
///   prototype (a recorded prototype's serial, `MIXED`, a `UNIQUE` link);
/// * [`SHAPE_ID_KIND_NULL`] (2): a null [[Prototype]].
///
/// Every band draws its ids from one counter per kind, and the counters hand
/// out 2^20-id granules in turn (kind 3 is never minted), so the kind is a
/// fact of the id's VALUE, fixed at the mint (`ShapeSlab::insert` asserts
/// it). The static band (`[SHAPE_ID_BASE, STATIC_SHAPE_ID_END)`, 2^20 ids) is
/// one plain granule: the compiler names only class and literal shapes.
///
/// It is what lets `object_prototype_word` answer the common receiver (a
/// literal, a class instance on its class's prototype) in one compare of the
/// header word, as the receiver's class id did before the prototype moved
/// into the shape, answer a null link from the header, and read the shape
/// record only for a word identity. Granules are page-aligned
/// (`shapes_store` pages hold 2^15 records), so the kinds cost no record
/// memory: a kind's untouched pages are never allocated.
pub(crate) const SHAPE_ID_KIND_SHIFT: u32 = 20;
pub(crate) const SHAPE_ID_KIND_MASK: u32 = 3 << SHAPE_ID_KIND_SHIFT;
pub(crate) const SHAPE_ID_KIND_PLAIN: u32 = 0;
pub(crate) const SHAPE_ID_KIND_WORD: u32 = 1;
pub(crate) const SHAPE_ID_KIND_NULL: u32 = 2;
/// One granule of each kind (and the never-minted fourth).
const SHAPE_ID_KIND_GROUP: u32 = 4 << SHAPE_ID_KIND_SHIFT;
const _: () = assert!(STATIC_SHAPE_ID_END - SHAPE_ID_BASE <= 1 << SHAPE_ID_KIND_SHIFT);
const _: () = assert!(SHAPE_ID_BASE % SHAPE_ID_KIND_GROUP == 0);
const _: () = assert!(DICTIONARY_SHAPE_ID_BASE % SHAPE_ID_KIND_GROUP == 0);
const _: () = assert!(EXOTIC_SHAPE_ID_BASE % SHAPE_ID_KIND_GROUP == 0);
const _: () = assert!(SHAPE_ID_END % SHAPE_ID_KIND_GROUP == 0);

/// The identity kind `header_word` (an `ObjectHeader`'s ShapeId word, or
/// whatever else it holds) carries in its kind bits.
#[inline(always)]
pub(crate) fn shape_word_kind(header_word: u32) -> u32 {
    (header_word & SHAPE_ID_KIND_MASK) >> SHAPE_ID_KIND_SHIFT
}

/// Does `header_word` possibly name a linked prototype identity (a word or
/// null)? `false` proves the receiver's shape identity is plain:
/// `object_prototype_word` answers from its meta record or 0, without
/// reading the shape record.
#[inline(always)]
pub(crate) fn shape_word_may_be_linked(header_word: u32) -> bool {
    header_word & SHAPE_ID_KIND_MASK != 0
}

/// The ShapeId kind ([`SHAPE_ID_KIND_SHIFT`]) of prototype identity
/// `proto_id`.
#[inline]
pub(crate) fn proto_id_kind(proto_id: u64) -> u32 {
    if proto_id == PROTO_ID_NULL {
        SHAPE_ID_KIND_NULL
    } else if shapes_prototype::proto_id_carries_word(proto_id) {
        SHAPE_ID_KIND_WORD
    } else {
        SHAPE_ID_KIND_PLAIN
    }
}

static SHAPE_SEMANTIC_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[inline]
pub(crate) fn is_shape_id(v: u32) -> bool {
    (SHAPE_ID_BASE..SHAPE_ID_END).contains(&v)
}

/// May a per-site cache word hold this id? True exactly for an ORDINARY-band
/// ShapeId: never for a dictionary shape ([`DICTIONARY_SHAPE_ID_BASE`]), never
/// for a class id, never for 0. Every site-word writer publishes only ids this
/// accepts, so an emitted compare can never equal a dictionary receiver's word.
#[inline]
pub(crate) fn is_site_matchable_shape_id(v: u32) -> bool {
    (SHAPE_ID_BASE..DICTIONARY_SHAPE_ID_BASE).contains(&v)
}

/// [`is_site_matchable_shape_id`] for a per-site PIC TOKEN
/// (`PIC_ID_TOKEN_BIT | ShapeId`): the exact token form over a matchable id.
#[inline]
pub(crate) fn is_site_matchable_token(token: u64) -> bool {
    token == (PIC_ID_TOKEN_BIT | u64::from(token as u32))
        && is_site_matchable_shape_id(token as u32)
}

/// Is this a dictionary-mode ShapeId? A fact of the value alone. (The
/// production writers ask the complement, [`is_site_matchable_shape_id`].)
#[cfg(test)]
#[inline]
pub(crate) fn is_dictionary_shape_id(v: u32) -> bool {
    (DICTIONARY_SHAPE_ID_BASE..EXOTIC_SHAPE_ID_BASE).contains(&v)
}

/// Is this an exotic-receiver ShapeId ([`EXOTIC_SHAPE_ID_BASE`])? A fact of
/// the value alone.
#[inline]
pub(crate) fn is_exotic_shape_id(v: u32) -> bool {
    (EXOTIC_SHAPE_ID_BASE..SHAPE_ID_END).contains(&v)
}

/// #6804: classify a WIDENED shape token (`object_shape()`'s usize). Ids
/// stored as usize carry no high bits, so the full-width range test never
/// misclassifies a real heap address whose LOW 32 bits merely fall in the
/// id range (`is_shape_id(v as u32)` would).
#[inline]
pub(crate) fn is_shape_id_token(v: usize) -> bool {
    v >= SHAPE_ID_BASE as usize && v < SHAPE_ID_END as usize
}

/// Lifts a ShapeId into the per-site PIC token space. MUST match the literal
/// the PIC IR emits in
/// `perry-codegen/src/expr/property_get/generic_dispatch.rs`
/// (4611686018427387904 = 1 << 62).
pub(crate) const PIC_ID_TOKEN_BIT: u64 = 1 << 62;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShapeIdExhausted;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShapeDescriptorError {
    IdExhausted,
    InvalidFacts,
}

#[cfg(test)]
fn alloc_shape_id_from(
    next: &std::sync::atomic::AtomicU32,
    end: u32,
) -> Result<u32, ShapeIdExhausted> {
    use std::sync::atomic::Ordering;
    loop {
        let id = next.load(Ordering::Relaxed);
        if id >= end {
            // Park at the exclusive end. In particular, never fetch_add at
            // END: wrapping to zero could eventually alias a live ShapeId, and
            // running past the ordinary band's end would mint into the
            // dictionary band.
            next.store(end, Ordering::Relaxed);
            return Err(ShapeIdExhausted);
        }
        if next
            .compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(id);
        }
    }
}

/// [`alloc_shape_id_from`] for one identity kind ([`SHAPE_ID_KIND_SHIFT`]):
/// an id whose kind bits are `kind`. A counter that reaches another kind's
/// granule skips to its own next one, so a band's counters interleave
/// granule by granule.
fn alloc_shape_id_of_kind(
    next: &std::sync::atomic::AtomicU32,
    end: u32,
    kind: u32,
) -> Result<u32, ShapeIdExhausted> {
    use std::sync::atomic::Ordering;
    loop {
        let current = next.load(Ordering::Relaxed);
        let mut id = current;
        if shape_word_kind(id) != kind {
            // This kind's granule in the current group, or the next group's.
            id = (id & !(SHAPE_ID_KIND_GROUP - 1)) | (kind << SHAPE_ID_KIND_SHIFT);
            if id < current {
                id = id.saturating_add(SHAPE_ID_KIND_GROUP);
            }
        }
        if id >= end {
            next.store(end, Ordering::Relaxed);
            return Err(ShapeIdExhausted);
        }
        if next
            .compare_exchange_weak(current, id + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Ok(id);
        }
    }
}

/// An ORDINARY-band ShapeId for a shape whose identity is `proto_id`: every
/// mint except a dictionary or exotic shape's.
fn alloc_shape_id(proto_id: u64) -> Result<u32, ShapeIdExhausted> {
    let kind = proto_id_kind(proto_id);
    alloc_shape_id_of_kind(
        &SHAPE_ID_NEXT[kind as usize],
        DICTIONARY_SHAPE_ID_BASE,
        kind,
    )
}

/// A dictionary-band ShapeId ([`DICTIONARY_SHAPE_ID_BASE`]).
fn alloc_dictionary_shape_id(proto_id: u64) -> Result<u32, ShapeIdExhausted> {
    let kind = proto_id_kind(proto_id);
    alloc_shape_id_of_kind(
        &DICTIONARY_SHAPE_ID_NEXT[kind as usize],
        EXOTIC_SHAPE_ID_BASE,
        kind,
    )
}

/// An exotic-band ShapeId ([`EXOTIC_SHAPE_ID_BASE`]).
fn alloc_exotic_shape_id(proto_id: u64) -> Result<u32, ShapeIdExhausted> {
    let kind = proto_id_kind(proto_id);
    alloc_shape_id_of_kind(&EXOTIC_SHAPE_ID_NEXT[kind as usize], SHAPE_ID_END, kind)
}

/// The band a new shape's id is drawn from is decided by its generation
/// namespace: a dictionary generation (bit 62 set, bit 63 clear —
/// `dictionary::next_generation`) mints in the dictionary band, everything
/// else in the ordinary band. Its identity decides the kind.
fn alloc_shape_id_for_generation(
    semantic_generation: u64,
    proto_id: u64,
) -> Result<u32, ShapeIdExhausted> {
    const DETERMINISTIC_BIT: u64 = 1 << 63;
    let tag = crate::object::dictionary::DICTIONARY_GENERATION_TAG;
    if semantic_generation & (DETERMINISTIC_BIT | tag) == tag {
        alloc_dictionary_shape_id(proto_id)
    } else {
        alloc_shape_id(proto_id)
    }
}

/// The ordinary-band ids this process has handed out, as a counter.
///
/// Tests assert the DELTA across a workload, because ids come from a 2^30
/// range that is never reused and parks (fail-stop) at the end: a path that
/// mints one id per operation is a process-LIFETIME bug, not merely a memory
/// cost, and nothing in the program's output ever reveals it.
#[cfg(test)]
pub(crate) fn test_shape_id_counter() -> u32 {
    SHAPE_ID_NEXT.iter().fold(0u32, |sum, next| {
        sum.wrapping_add(next.load(std::sync::atomic::Ordering::Relaxed))
    })
}

/// Get or create the exact structural descriptor. The public allocation and
/// mutation paths turn exhaustion into a fail-stop before publishing an
/// untracked layout; the `Result` stays explicit so the allocator boundary and
/// its exhaustion tests remain reviewable.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_ensure_with_generation(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    proto_id: u64,
    receiver: ReceiverFacts,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_holes(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        0,
        proto_id,
        receiver,
        None,
    )
}

/// A static id the driver assigned by content was refused by the ordinary
/// mint (see [`shape_descriptor_ensure_with_holes`]).
#[cold]
fn static_shape_id_refused_abort(requested: u32, proto_id: u64) -> ! {
    eprintln!(
        "Perry internal error: the static ShapeId {requested:#x} (proto {proto_id:#x}) \
         was refused by the shape mint (it is outside the static band or already \
         names other facts in this agent); a static id must name one shape"
    );
    std::process::abort();
}

/// [`shape_descriptor_ensure_with_generation`] with an explicit tombstone
/// count — the publish half of an O(1) hole-delete, which must mint a shape
/// identity distinct from every hole state of the same array. Also the mint
/// for #9019's reserved-floor seed (`object/reserved_floor.rs`), whose keys
/// array is BORN with `floor` leading holes.
///
/// `receiver` is what the shape takes from its receiver rather than from the
/// keys: the attribute summary the keys do not carry themselves (a dictionary
/// receiver's private list, [`receiver_extra_summary`]) and the private
/// brands the receiver carries (#11791). The summary of the published keys is
/// derived here, from the keys, so no caller can publish a shape that
/// under-reports its attributes; a mint for a receiver in hand passes
/// [`receiver_facts`], so no transition can drop a brand.
///
/// `requested` is the compiler-assigned static id of these facts
/// ([`STATIC_SHAPE_ID_END`]), or `None`. It changes only what a by-facts MISS
/// mints: the requested id instead of a counter id, with
/// `RECORD_FLAG_EXTERNAL_CARRIER` set (generated code holds the id as an
/// immediate, so the record must never be pruned while no object carries
/// it). A by-facts HIT returns the existing id whatever was requested — the
/// static id whenever its seed ran first, which the seeds are placed to
/// guarantee; otherwise an immediate compare against it only misses.
///
/// The driver assigns ids BY CONTENT, so a requested id that cannot be
/// adopted on a miss — outside the static band, or already present in this
/// agent under other facts — is an invariant violation, and the mint ABORTS
/// (as the typed install does): generated code compares against the id as an
/// immediate, and a counter fallback would leave it naming whatever else
/// holds it in this agent.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_ensure_with_holes(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    receiver: ReceiverFacts,
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_rep(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        receiver,
        super::field_rep::REP_ANY,
        requested,
    )
}

/// [`shape_descriptor_ensure_with_holes`] with an explicit field
/// representation (charter step 5, `field_rep`). `rep` is identity under
/// [`field_rep::identity`](super::field_rep::identity): a request whose only
/// difference from a live record is a deprecated lane finds that record.
///
/// `requested` is the static id of these facts, exactly as for
/// [`shape_descriptor_ensure_with_holes`]. A static id names a birth
/// content, whose field representation is the birth rep codegen declared
/// (charter step 5, T1: `Any` or `F64` lanes, part of the content), so it is
/// adopted for a rep with no deprecated lane; a request of it with a
/// deprecated lane is refused (and aborts): no birth is born deprecated.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
#[inline(always)]
pub(crate) fn shape_descriptor_ensure_with_rep(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    receiver: ReceiverFacts,
    rep: u64,
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    if !super::field_rep::is_valid(rep) {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    let keys_id = keys as usize as u64;
    if keys_id == 0 && logical_key_count != 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    // SAFETY: a live keys array or 0 (`keys_attrs` resolves through the
    // ownership-checking array resolver, so a test's synthetic address
    // reads as attribute-free).
    let summary = receiver.extra_summary
        | if keys_id == 0 {
            0
        } else {
            unsafe { crate::object::key_attrs::keys_summary_checked(keys, logical_key_count) }
        };
    shape_descriptor_intern_with_rep(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        receiver.brands.as_slice(),
        requested,
    )
}

/// The id of the indexed record with exactly these facts, if the identity
/// table holds one. Probe only: never mints (the lookup half of
/// [`shape_descriptor_intern_with_rep`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_descriptor_find_with_rep(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    brands: &[u64],
) -> Option<u32> {
    if !super::field_rep::is_valid(rep) {
        return None;
    }
    let keys_id = keys as usize as u64;
    let facts = shapes_store::facts_key_proto_with_special(
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        &[],
        brands,
    );
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let ids = inner.by_facts.get(&facts)?;
    let slab = table.slab();
    ids.as_slice().iter().copied().find(|&id| {
        slab.record_ptr(id).is_some_and(|record| {
            // SAFETY: live slab record, read immediately.
            let record = unsafe { *record };
            record.has(RECORD_FLAG_FACTS_INDEXED)
                && record.facts_match_proto_with_special(
                    keys_id,
                    logical_key_count,
                    live_inline_slot_count,
                    semantic_generation,
                    object_kind,
                    hole_count,
                    proto_id,
                    summary,
                    rep,
                    &[],
                    brands,
                )
        })
    })
}

/// The twin of an existing shape that differs only in `object_kind` (charter
/// step 3: the store facts are kinds). Every other fact, the attribute summary
/// included, is copied from `source`'s record, whose summary was derived from
/// the same keys prefix when it was minted, so the twin cannot under-report.
///
/// Unlike [`shape_descriptor_ensure_with_holes`] this never reads the keys
/// array: re-deriving the summary goes through `keys_attrs`, and the static
/// GC call-effects analysis proves that edge can reach a lazy materializer
/// that re-enters JS. A proof retire runs this path from inside
/// the owner store funnel, which must remain non-collecting, so the
/// twin mint must touch only the shape table's own Rust storage.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_kind_twin(source: u32, object_kind: ShapeObjectKind) -> Option<u32> {
    let d = shape_descriptor_by_id(source)?;
    if d.object_kind == object_kind {
        return Some(source);
    }
    shape_descriptor_intern_with_rep(
        d.keys as usize as *const ArrayHeader,
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        d.rep,
        &d.brands().to_vec(),
        // A twin re-kinds an existing record: never a static-id request.
        None,
    )
    .ok()
}

/// [`shape_descriptor_ensure_with_rep`] for a caller that supplies the
/// shape's COMPLETE attribute summary: a lifted record's own `summary`
/// (charter step 5's generalization, which re-interns a live record's facts
/// with another rep). Nothing here reads the keys' attributes.
///
/// `requested` is the static id of these facts, exactly as for
/// [`shape_descriptor_ensure_with_rep`]; a re-intern of a live record's facts
/// names none.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_intern_with_rep(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    brands: &[u64],
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    if !super::field_rep::is_valid(rep) {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    shape_descriptor_intern_with_special(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        &[],
        brands,
        requested,
    )
}

/// The exact shape mint for an optional set of static ConstFn body facts.
/// Existing callers use the wrapper above and preserve their old identity.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_intern_with_special(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    infos: &[shapes_store::ConstFnSlotInfo],
    brands: &[u64],
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_intern_with_special_mode(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        infos,
        brands,
        requested,
        false,
    )
}

/// Only a body-aware static birth/seed may admit a requested ConstFn id.
/// The ordinary interner above keeps refusing it even if a caller passes a
/// requested id. All other validation and by-facts interning is shared.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
fn shape_descriptor_intern_with_special_mode(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    infos: &[shapes_store::ConstFnSlotInfo],
    brands: &[u64],
    requested: Option<u32>,
    static_constfn: bool,
) -> Result<u32, ShapeDescriptorError> {
    let Some(mask) = shapes_store::constfn_mask(infos) else {
        return Err(ShapeDescriptorError::InvalidFacts);
    };
    if !shapes_store::brands_are_sorted(brands) {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    if !super::field_rep::is_valid_with_special(rep, mask) {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    // Every SPECIAL lane in Step 5C names a ConstFn body. P5's optional
    // NoPointer producer needs its own mask and remains gated by census.
    // A requested ConstFn id is only legal through the body-aware static
    // birth/seed entry points; generic dynamic callers fail closed.
    if mask != super::field_rep::special_lane_slots(rep)
        || (requested.is_some() && mask != 0 && !static_constfn)
    {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    let keys_id = keys as usize as u64;
    if keys_id == 0 && logical_key_count != 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    // #10868 attribution, compiled out entirely without `shape-mint-diag`.
    // When it IS compiled in, both halves are gated on one relaxed atomic
    // load, and the key-list hash is resolved BEFORE the table borrow because
    // it reads the keys array and its key strings.
    #[cfg(feature = "shape-mint-diag")]
    let census_on = crate::object::shape_mint_census::armed();
    #[cfg(feature = "shape-mint-diag")]
    let (census_list_hash, census_file, census_line) = if census_on {
        // `#[track_caller]` on this chain is part of the same feature, so
        // `Location::caller()` here names the runtime path that ASKED for a
        // shape rather than this line.
        let loc = std::panic::Location::caller();
        (
            // SAFETY: a live keys array (or 0); no table borrow is held here.
            unsafe {
                crate::object::shape_mint_census::key_list_content_hash(keys_id, logical_key_count)
            },
            loc.file(),
            loc.line(),
        )
    } else {
        (0, "", 0)
    };
    let facts = shapes_store::facts_key_proto_with_special(
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        infos,
        brands,
    );
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    if let Some(ids) = inner.by_facts.get(&facts) {
        let slab = table.slab();
        for &id in ids.as_slice() {
            let Some(record) = slab.record_ptr(id) else {
                continue;
            };
            // SAFETY: live slab record, read immediately.
            let record = unsafe { *record };
            // The bucket is a 64-bit fold: validate the facts on every hit.
            if record.has(RECORD_FLAG_FACTS_INDEXED)
                && record.facts_match_proto_with_special(
                    keys_id,
                    logical_key_count,
                    live_inline_slot_count,
                    semantic_generation,
                    object_kind,
                    hole_count,
                    proto_id,
                    summary,
                    rep,
                    infos,
                    brands,
                )
            {
                #[cfg(feature = "shape-mint-diag")]
                crate::object::shape_mint_census::note_memo_hit();
                return Ok(id);
            }
        }
    }
    drop(inner);
    shape_descriptor_mint_fresh(
        facts,
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        infos,
        brands,
        requested,
        #[cfg(feature = "shape-mint-diag")]
        (census_on, census_list_hash, census_file, census_line),
    )
}

/// The miss half of [`shape_descriptor_intern_with_special_mode`]: allocate
/// the id, then publish the record and its accelerators. Out of line, so the
/// hit path (hash, bucket probe, facts compare) stays small enough to keep its
/// helpers inline: the identity table answers nearly every intern there.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn shape_descriptor_mint_fresh(
    facts: u64,
    keys_id: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    infos: &[shapes_store::ConstFnSlotInfo],
    brands: &[u64],
    requested: Option<u32>,
    #[cfg(feature = "shape-mint-diag")] census: (bool, u64, &'static str, u32),
) -> Result<u32, ShapeDescriptorError> {
    #[cfg(feature = "shape-mint-diag")]
    let (census_on, census_list_hash, census_file, census_line) = census;
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    // A static id is adopted only for facts it can name (ordinary band,
    // generation 0, a birth rep: no deprecated lane, brands of class
    // templates only: a template brand is its class id in every agent, a
    // fresh evaluation's is not) and only if this agent has no record under
    // it yet.
    let adopted = requested.filter(|&id| {
        is_static_shape_id(id)
            && brands
                .iter()
                .all(|&b| b & crate::object::field_get_set::PRIVATE_FRESH_EVALUATION_BRAND == 0)
            && !object_kind.is_exotic()
            && semantic_generation == 0
            && !super::field_rep::has_deprecated(rep)
            && table.slab().record_ptr(id).is_none()
    });
    if let (Some(requested), None) = (requested, adopted) {
        drop(inner);
        static_shape_id_refused_abort(requested, proto_id);
    }
    let id = match adopted {
        Some(id) => id,
        None => if object_kind.is_exotic() {
            alloc_exotic_shape_id(proto_id)
        } else {
            alloc_shape_id_for_generation(semantic_generation, proto_id)
        }
        .map_err(|_| ShapeDescriptorError::IdExhausted)?,
    };
    #[cfg(feature = "shape-mint-diag")]
    if census_on {
        // Every descriptor already indexed under this keys ADDRESS, copied out
        // so no slab reference is alive across `slab_mut()` below.
        let mut family_facts: Vec<(u32, u32, u64, bool, u32)> = Vec::new();
        if let Some(ids) = inner.families.get(&keys_id) {
            let slab = table.slab();
            for &fid in ids.as_slice() {
                if let Some(record) = slab.get(fid) {
                    family_facts.push((
                        record.logical_key_count,
                        record.live_inline_slot_count,
                        record.semantic_generation,
                        record.object_kind() == ShapeObjectKind::Class,
                        record.hole_count,
                    ));
                }
            }
        }
        crate::object::shape_mint_census::note_mint(
            id,
            keys_id,
            census_list_hash,
            logical_key_count,
            live_inline_slot_count,
            semantic_generation,
            object_kind == ShapeObjectKind::Class,
            hole_count,
            &family_facts,
            census_file,
            census_line,
        );
    }
    let record = ShapeRecord::new(
        keys_id,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
    )
    .with_proto_id(proto_id)
    .with_summary(summary)
    .with_special_facts(rep, infos, brands);
    let mut record = record;
    if adopted.is_some() {
        record.set(RECORD_FLAG_EXTERNAL_CARRIER, true);
    }
    // Publish by-id first, then the reverse accelerators. An ObjectHeader is
    // stamped only after this function returns, so a visible id always has a
    // complete descriptor.
    // SAFETY: no slab reference is held; `slab()` above went out of scope.
    unsafe { table.slab_mut().insert(id, record) };
    // `id` was just handed out by `alloc_shape_id`, which never reuses a
    // value, so neither accelerator can already hold it: append without the
    // membership scan, whose cost is linear in this keys array's descriptor
    // history (see `IdList::append_unchecked`).
    inner.facts_append_fresh(facts, id);
    inner.family_append_fresh(keys_id, id);
    // #10905: every shape of the transition tree below a keyless birth shape
    // is minted exactly once, here, so this is where the birth shape learns
    // how wide its descendants grow (`shapes_birth_width`).
    if object_kind.is_ordinary_layout() && semantic_generation == 0 {
        shapes_birth_width::note_descendant_width(
            &inner,
            table.slab(),
            proto_id,
            logical_key_count,
        );
    }
    Ok(id)
}

#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_descriptor_ensure(
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_generation(
        keys,
        logical_key_count,
        live_inline_slot_count,
        0,
        ShapeObjectKind::Ordinary,
        PROTO_ID_DEFAULT,
        ReceiverFacts::NONE,
    )
}

/// [`shape_descriptor_ensure`] for a receiver whose [[Prototype]] identity is
/// read off the receiver itself — every mint that has an object in hand and
/// no lineage to copy it from.
///
/// # Safety
/// `obj` is a live shaped `ObjectHeader`.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn shape_descriptor_ensure_for_object(
    obj: *const crate::object::ObjectHeader,
    keys: *const ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> Result<u32, ShapeDescriptorError> {
    shape_descriptor_ensure_with_generation(
        keys,
        logical_key_count,
        live_inline_slot_count,
        0,
        store_kind::receiver_ordinary_kind(obj),
        object_proto_id(obj),
        // A receiver with no shape record has no private brand: brands are
        // facts of a record (#11791).
        ReceiverFacts::summary(receiver_extra_summary(obj)),
    )
}

/// What a mint for `obj` takes from the receiver rather than from its keys
/// (see [`shape_descriptor_ensure_with_holes`]).
pub(crate) struct ReceiverFacts {
    pub(crate) extra_summary: u8,
    /// The receiver's private brands (#11791), sorted. A brand is only ever
    /// added, so every shape minted for a receiver carries the brands of the
    /// shape it replaces.
    pub(crate) brands: BrandList,
}

/// A receiver's private brands, sorted, copied out of its shape record
/// (#11791). Almost every receiver has none, and then the list is `None`: no
/// allocation, and nothing to free after the mint.
pub(crate) struct BrandList(Option<Box<[u64]>>);

impl BrandList {
    pub(crate) const NONE: BrandList = BrandList(None);

    /// A copy of `brands`, which may borrow a record a mint can retire.
    #[inline]
    pub(crate) fn copy_of(brands: &[u64]) -> BrandList {
        if brands.is_empty() {
            BrandList::NONE
        } else {
            BrandList(Some(brands.into()))
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[u64] {
        self.0.as_deref().unwrap_or(&[])
    }

    /// Add `brand`, keeping the list sorted. `false` when it is already there.
    pub(crate) fn insert(&mut self, brand: u64) -> bool {
        let Err(at) = self.as_slice().binary_search(&brand) else {
            return false;
        };
        let mut brands = self.as_slice().to_vec();
        brands.insert(at, brand);
        self.0 = Some(brands.into_boxed_slice());
        true
    }
}

impl ReceiverFacts {
    /// A shape minted for no receiver: an intrinsic, a birth, a seed.
    pub(crate) const NONE: ReceiverFacts = ReceiverFacts {
        extra_summary: 0,
        brands: BrandList::NONE,
    };

    /// A receiverless mint with an attribute summary its keys do not carry.
    pub(crate) const fn summary(extra_summary: u8) -> ReceiverFacts {
        ReceiverFacts {
            extra_summary,
            brands: BrandList::NONE,
        }
    }

    /// The facts a lifted descriptor carries for its receivers: a mint that
    /// derives one shape from another without a receiver in hand.
    pub(crate) fn of_descriptor(d: &ShapeDescriptor, extra_summary: u8) -> ReceiverFacts {
        ReceiverFacts {
            extra_summary,
            brands: BrandList::copy_of(d.brands()),
        }
    }
}

/// [`ReceiverFacts`] of `obj`: read before the mint, while the predecessor
/// is still stamped, because a mint can collect.
///
/// # Safety
/// `obj` is null or a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_facts(obj: *const crate::object::ObjectHeader) -> ReceiverFacts {
    ReceiverFacts {
        extra_summary: receiver_extra_summary(obj),
        brands: if obj.is_null() {
            BrandList::NONE
        } else {
            BrandList::copy_of(shape_brands_by_id(object_shape_stamp(obj)).unwrap_or(&[]))
        },
    }
}

/// The private brands `obj`'s current shape carries (#11791). Copied out: the
/// slice borrows the predecessor's record, which a mint may retire.
///
/// # Safety
/// `obj` is null or a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_brands(obj: *const crate::object::ObjectHeader) -> Vec<u64> {
    if obj.is_null() {
        return Vec::new();
    }
    shape_brands_by_id(object_shape_stamp(obj)).map_or_else(Vec::new, <[u64]>::to_vec)
}

/// [`receiver_facts`] for a mint that already holds `current`, the
/// descriptor `obj` is stamped with: the brands are read off it, so a shape
/// with no private brand answers with its `extras` word (null), which the
/// mint has in hand.
///
/// # Safety
/// `obj` is a live `ObjectHeader` stamped with `current`.
#[inline]
pub(crate) unsafe fn receiver_facts_of_current(
    obj: *const crate::object::ObjectHeader,
    current: &ShapeDescriptor,
) -> ReceiverFacts {
    debug_assert_eq!(
        current.brands(),
        receiver_brands(obj).as_slice(),
        "`current` is the receiver's own shape"
    );
    ReceiverFacts {
        extra_summary: receiver_extra_summary(obj),
        brands: BrandList::copy_of(current.brands()),
    }
}

/// The brand list of shape `id` in this agent, borrowed from its live record.
/// `None` for an id with no record here.
#[inline]
pub(crate) fn shape_brands_by_id(id: u32) -> Option<&'static [u64]> {
    let record = ShapeSlab::agent_record_present(id)?;
    // SAFETY: a present record of this agent; its extension lives as long as
    // the record, which the caller's receiver keeps stamped.
    Some(unsafe { (*record).brands() })
}

/// Attribute summary `obj`'s shape must carry beyond what its published keys
/// report. A DICTIONARY receiver publishes no keys (`object/dictionary.rs`),
/// so the attributes of its private list are summarized here —
/// conservatively, every per-key bit, whenever that list carries any: the
/// list is edited in place and a dictionary shape is never shared, so an
/// exact summary would buy nothing a per-key lookup does not.
///
/// # Safety
/// `obj` is null or a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_extra_summary(obj: *const crate::object::ObjectHeader) -> u8 {
    if obj.is_null() || !crate::object::dictionary::is_dictionary(obj) {
        return 0;
    }
    crate::object::dictionary::private_list_summary(obj)
}

#[cold]
#[inline(never)]
fn shape_id_exhausted_abort() -> ! {
    eprintln!("Perry ShapeId space exhausted; refusing to publish an untracked object shape");
    std::process::abort()
}

#[cold]
#[inline(never)]
fn invalid_shape_facts_abort() -> ! {
    eprintln!("Perry internal error: refusing to publish invalid object shape facts");
    std::process::abort()
}

#[inline]
fn shape_descriptor_error_abort(error: ShapeDescriptorError) -> ! {
    match error {
        ShapeDescriptorError::IdExhausted => shape_id_exhausted_abort(),
        ShapeDescriptorError::InvalidFacts => invalid_shape_facts_abort(),
    }
}

#[inline]
pub(crate) fn publish_shape_result(result: Result<u32, ShapeDescriptorError>) -> u32 {
    match result {
        Ok(id) => id,
        Err(error) => shape_descriptor_error_abort(error),
    }
}

/// Compatibility mint for canonical shapes whose key and live-slot counts are
/// identical. New object-aware paths use [`shape_descriptor_ensure`] directly.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) fn shape_id_for_keys_ensure(keys: *const ArrayHeader, key_count: u32) -> u32 {
    publish_shape_result(shape_descriptor_ensure(keys, key_count, key_count))
}

/// One FIELD of a shape's descriptor, without lifting the whole record.
///
/// [`shape_descriptor_by_id`] returns `ShapeDescriptor` **by value**, so every
/// caller that wants a single `u32` still copies the entire record out of the
/// table. That is most of them: `object_live_slot_count` — the slot bound
/// consulted on essentially every property read and write — throws away all
/// of it but `live_inline_slot_count`.
#[inline]
fn shape_descriptor_field_by_id<T>(shape_id: u32, read: impl Fn(&ShapeRecord) -> T) -> Option<T> {
    let record = ShapeSlab::agent_record_present(shape_id)?;
    // SAFETY: `agent_record_present` only returns a live slab record.
    Some(read(unsafe { &*record }))
}

/// The live inline-slot bound for `shape_id`, without copying its descriptor.
pub(crate) fn shape_live_inline_slot_count_by_id(shape_id: u32) -> Option<u32> {
    shape_descriptor_field_by_id(shape_id, |d| d.live_inline_slot_count)
}

/// The descriptor named by `shape_id`, or `None` when the id names no
/// descriptor in this agent.
///
/// #9706: a slab probe — band select, page, chunk, record — with no hash,
/// no `RefCell` borrow and no invalidation epoch; and no runtime-state fetch
/// either: it reads this agent's published directory
/// ([`ShapeSlab::agent_record`]). The direct-mapped way cache
/// that used to front the hash map is gone because the slab IS that cache:
/// a hit was "mask, compare, deref" and a probe is "shift, index, deref".
#[inline]
pub(crate) fn shape_descriptor_by_id(shape_id: u32) -> Option<ShapeDescriptor> {
    let record = ShapeSlab::agent_record_present(shape_id)?;
    // SAFETY: `agent_record_present` only returns a live slab record.
    Some(unsafe { (*record).lift(record) })
}

/// The record named by `shape_id`, borrowed in place: the same slab probe as
/// [`shape_descriptor_by_id`], without lifting a copy (#10362).
#[inline]
pub(crate) fn shape_record_by_id(shape_id: u32) -> Option<ShapeRecordRef> {
    let record = ShapeSlab::agent_record_present(shape_id)?;
    std::ptr::NonNull::new(record).map(ShapeRecordRef)
}

/// The field-representation word (`field_rep`) of `shape_id` in this agent,
/// deprecated lanes included. An id that names no record here reads the
/// absent record's word, 0 — `Any` in every lane, which is exactly the
/// answer for a receiver with no shape record — so there is no presence
/// test: the step 5 store check asks this on every checked store.
#[inline]
pub(crate) fn shape_rep_by_id(shape_id: u32) -> u64 {
    let record = ShapeSlab::agent_record(shape_id);
    // SAFETY: `agent_record` never returns null; `rep` is an 8-aligned u64 of
    // a `#[repr(C)]` record (asserted 8-aligned), read the way
    // `ShapeRecordRef::rep` reads it because a published record's word is
    // rewritten atomically (`deprecate_rep_slot`). The shared empty record
    // is only ever read.
    unsafe {
        (*std::ptr::addr_of!((*record).rep).cast::<std::sync::atomic::AtomicU64>())
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Immutable ordinary-vs-class fact with a pointer-free, per-agent direct
/// cache. The first observation remains the authoritative descriptor lookup_ways;
/// subsequent observations avoid the hot ShapeId HashMap borrow.
#[inline]
pub(crate) fn shape_object_kind_by_id(shape_id: u32) -> Option<ShapeObjectKind> {
    if let Some(kind) = cached_shape_object_kind(shape_id) {
        return Some(kind);
    }
    let kind = shape_descriptor_by_id(shape_id)?.object_kind;
    publish_shape_object_kind(shape_id, kind);
    Some(kind)
}

/// Record that a shape is carried by an OLD-generation receiver.
///
/// Called from the collector's slot visitor, which resolved the record for
/// this receiver already, so the note costs a generation range check and a
/// byte store — no second shape-table probe (#8122's one-probe rule). The
/// store goes straight through the boxed record's address rather than
/// re-borrowing `ShapeTableInner`: the visitor runs inside walks that already
/// hold that borrow.
///
/// # Safety
///
/// `record` is a live slab record owned by this agent's shape table (the
/// contract on [`ShapeRecordRef`]). Records are retired only by the table's
/// own retirement paths, and their chunk is released by
/// `shrink_shape_tables` at the end of a major collection — after every
/// enumeration of the cycle that resolved this record.
#[inline]
pub(crate) unsafe fn note_old_generation_carrier(record: Option<ShapeRecordRef>) {
    let Some(record) = record else {
        return;
    };
    let keys = record.keys();
    let record = record.0.as_ptr();
    let first_note_this_epoch = !(*record).has(RECORD_FLAG_OLD_CARRIER_SEEN);
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping bits, never a heap reference.
    (*record).set(RECORD_FLAG_OLD_CARRIER | RECORD_FLAG_OLD_CARRIER_SEEN, true);
    if first_note_this_epoch {
        note_shape_carrier_candidate(keys);
    }
}

/// Would [`note_old_generation_carrier`] change nothing for `record` right now?
///
/// True when both of its flags are already set — the note then re-sets them
/// and finds it is not the first this epoch — or when there is no record. The
/// copying drain asks this BEFORE classifying the receiver's generation, so a
/// shape whose carrier was already noted this epoch skips that page-map probe
/// entirely (#11549). Exact: it reads the same two flags the note would.
#[inline]
pub(crate) unsafe fn old_generation_carrier_already_noted(record: Option<ShapeRecordRef>) -> bool {
    let Some(record) = record else {
        return true;
    };
    let record = record.0.as_ptr();
    (*record).has(RECORD_FLAG_OLD_CARRIER) && (*record).has(RECORD_FLAG_OLD_CARRIER_SEEN)
}

/// Note that a complete full trace visited a receiver carrying this shape.
/// Unlike the old-generation gate, this answers receiver liveness regardless
/// of generation and is consumed by post-trace descriptor retirement.
#[inline]
pub(crate) unsafe fn note_full_trace_carrier(record: Option<ShapeRecordRef>) {
    let Some(record) = record else {
        return;
    };
    (*record.0.as_ptr()).set(RECORD_FLAG_CARRIED_SEEN, true);
}

#[inline]
pub(crate) unsafe fn note_external_shape_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record != 0 {
        (*(descriptor.record as *mut ShapeRecord)).set(RECORD_FLAG_EXTERNAL_CARRIER, true);
    }
}

/// Retain a descriptor while an agent-local optimization cache can reinstall
/// its ShapeId. Cache tables live with `RuntimeState`; the bit is recomputed
/// from live table occupancy after every full trace
/// (`array_tail_transition::recompute_cache_carriers_after_full_trace`), so a
/// descriptor whose last entry was evicted stops being rooted at the next full
/// trace — the same cadence as `old_carrier`.
#[inline]
pub(crate) unsafe fn note_cache_carrier(descriptor: Option<ShapeDescriptor>) {
    let Some(descriptor) = descriptor else {
        return;
    };
    if descriptor.record == 0 {
        return;
    }
    let record = descriptor.record as *mut ShapeRecord;
    let newly_armed = !(*record).has(RECORD_FLAG_CACHE_CARRIER);
    // GC_STORE_AUDIT(POINTER_FREE): liveness bookkeeping bit, never a heap reference.
    (*record).set(RECORD_FLAG_CACHE_CARRIER, true);
    if newly_armed {
        note_shape_carrier_candidate(descriptor.keys);
    }
}

/// The post-birth publication point for a ShapeId into a receiver's header
/// word: stamp, then register the carrier duty the stamp just created.
///
/// #9200: a receiver a MINOR WILL NOT ENUMERATE (old-gen, `gc_malloc`'d
/// large, immortal bootstrap) can be stamped with a descriptor whose keys
/// array is nursery-young. The receiver is invisible to the next minor, so
/// the descriptor's record is the ONLY path that can keep that keys array
/// alive — and an unarmed record is walked metadata-only by
/// `scan_shape_table_rekey_mut`. The keys array is then swept while live,
/// `prune_dead_shape_keys` drops the descriptor as dead, and the receiver
/// comes back shapeless: `Object.keys()` empty, every fixed-slot read
/// `undefined`. The tombstone-delete publish hit exactly this: it minted a
/// fresh unarmed descriptor for an already-promoted receiver and then
/// retired the armed predecessor in its keys-address sweep.
///
/// Arming here — in the same breath as the header store — makes the
/// old-carrier gate hold BY CONSTRUCTION for every publish routed through
/// this funnel, instead of relying on each publish site to remember the
/// note. The nursery test mirrors `visit_gc_layout_slot_descriptors`'
/// carrier note: "not in the nursery", never "in old-gen".
///
/// Over-approximation is the designed cost model: the gate is sticky within
/// an epoch and recomputed by every full trace, so arming a receiver that
/// dies young roots one record for at most one full collection — exactly
/// the generational contract (#8112).
#[inline]
pub(crate) unsafe fn stamp_object_shape_id_with_carrier_note(
    obj: *mut crate::object::ObjectHeader,
    id: u32,
) {
    // Charter step 3 (R5): a receiver carrying the Array-subclass numeric
    // proof loses it on every stamp but its own proof shape's.
    store_kind::retire_proof_before_stamp(obj, id);
    let previous = (*obj).parent_class_id;
    debug_assert_brands_carried(previous, id);
    (*obj).parent_class_id = id;
    // A structural change to an object somebody INHERITS from is invisible to
    // every instance below it: no instance is touched, no epoch moves, and the
    // instances' own ShapeIds are unchanged. This is the one place every such
    // change publishes, which is why the validity bump lives here rather than
    // in a list of mutation entry points that would have to be kept complete.
    // Until something is marked as a prototype this is one relaxed `bool` load.
    crate::object::proto_validity::note_object_shape_stamped(obj as usize, previous, id);
    if !crate::arena::pointer_in_nursery(obj as usize) {
        let record = shape_record_by_id(id);
        note_old_generation_carrier(record);
        // This stamp is the structural-mutation publication funnel. Re-arm
        // even when the descriptor was already an old carrier: an owned
        // Longlived keys array may have just gained a nursery key at the same
        // address, and its carrier flag alone cannot express that transition.
        if let Some(record) = record {
            note_shape_carrier_candidate(record.keys());
        }
    }
    // Charter step 3 (R6): every publication is checked in debug builds and
    // under `shape-fact-audit`; compiled out otherwise.
    store_kind::check_store_facts(obj);
}

/// Clear every `cache_carrier` bit ahead of the post-full-trace recompute.
pub(crate) fn clear_all_cache_carriers() {
    crate::state::state().shapes.slab().for_each(|_, record| {
        // SAFETY: live slab record, single-threaded agent.
        unsafe { (*record).set(RECORD_FLAG_CACHE_CARRIER, false) };
    });
}

/// Recompute the old-carrier gate from the trace that just finished.
///
/// A FULL trace enumerates every live object, so the notes it accumulated are
/// exactly the shapes old objects still carry; adopt them and clear both the
/// old-carrier accumulator and the all-generation carried note. The latter is
/// consumed by synchronous-full descriptor retirement immediately before this
/// rotation. Budgeted full cycles clear it without retiring because their
/// sliced trace is not a complete carrier census.
pub(crate) fn rotate_old_carrier_epoch_after_full_trace() {
    crate::state::state().shapes.slab().for_each(|_, record| {
        // SAFETY: live slab record, single-threaded agent.
        unsafe {
            let seen = (*record).has(RECORD_FLAG_OLD_CARRIER_SEEN);
            (*record).set(RECORD_FLAG_OLD_CARRIER, seen);
            (*record).set(RECORD_FLAG_OLD_CARRIER_SEEN, false);
            (*record).set(RECORD_FLAG_CARRIED_SEEN, false);
            (*record).set(RECORD_FLAG_BIRTH_OWNER, false);
        }
    });
}

/// Mint (or retrieve) the ShapeId paired with canonical keys and equal
/// key/live-slot counts.
///
/// Codegen calls this once per class during module initialization and stores
/// the result beside `@perry_class_keys_*`. It deliberately takes a raw u64
/// rather than `*const ArrayHeader`: Perry's textual LLVM ABI represents the
/// rooted keys global as an integer heap word on every target.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_keys(keys: u64, key_count: u32) -> u32 {
    let id = shape_id_for_keys_ensure(keys as usize as *const ArrayHeader, key_count);
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { note_external_shape_carrier(shape_descriptor_by_id(id)) };
    id
}

/// [`js_object_shape_id_for_keys`] for a class's birth shape: codegen passes
/// the class id, because the shape names the prototype that class implies,
/// and the birth `rep` (charter step 5, T1): `F64` for exactly the slots the
/// class's typed layout declares raw-f64 at allocation. The rep is codegen's
/// decision, made once from the class source; the runtime never re-derives it.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_class_keys(
    keys: u64,
    key_count: u32,
    class_id: u32,
    rep: u64,
) -> u32 {
    let id = publish_shape_result(class_birth_shape_ensure(
        keys as usize as *const ArrayHeader,
        key_count,
        key_count,
        class_id,
        rep,
        None,
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { note_external_shape_carrier(shape_descriptor_by_id(id)) };
    id
}

/// The birth ShapeId of a class born WIDE: its canonical keys with a live
/// inline bound of `live` (> `key_count`), the in-object slack codegen gives a
/// constructor that adds keys (`lower_call::new_alloc`). The inline allocator
/// stamps it on an object with exactly `max(live, INLINE_SLOT_FLOOR)` slots,
/// and the outlined one matches it for any allocation of that width.
#[no_mangle]
pub extern "C" fn js_object_shape_id_for_class_keys_live(
    keys: u64,
    key_count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
) -> u32 {
    let id = publish_shape_result(class_birth_shape_ensure(
        keys as usize as *const ArrayHeader,
        key_count,
        live,
        class_id,
        rep,
        None,
    ));
    // SAFETY: `id` was resolved from this agent's live slab record above.
    unsafe { note_external_shape_carrier(shape_descriptor_by_id(id)) };
    id
}

/// A class's birth shape with its codegen-declared rep. An `F64` lane past
/// the key count (slack) or on a reserved lane is invalid facts. `requested`
/// is the driver's static id for these facts (design step 4), whose content
/// includes the rep.
pub(crate) fn class_birth_shape_ensure(
    keys: *const ArrayHeader,
    key_count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    let key_lanes = if key_count >= super::field_rep::REP_SLOTS {
        u64::MAX
    } else {
        super::field_rep::lanes_below(key_count)
    };
    if rep & !key_lanes != 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    shape_descriptor_ensure_with_rep(
        keys,
        key_count,
        live.max(key_count),
        0,
        ShapeObjectKind::Ordinary,
        0,
        class_proto_id(class_id),
        // A birth carries no brand: a class brands its instance after its
        // heritage's constructor returns (#11791).
        ReceiverFacts::NONE,
        rep,
        requested,
    )
}

/// Body-aware final-shape mint. This mints facts only; callers must never
/// stamp its result on an allocation with uninitialized closure slots.
/// A seed and post-construction finalizer enter with identical facts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn final_shape_ensure_constfn(
    keys: *const ArrayHeader,
    key_count: u32,
    live: u32,
    class_id: u32,
    rep: u64,
    infos: &[shapes_store::ConstFnSlotInfo],
    requested: Option<u32>,
) -> Result<u32, ShapeDescriptorError> {
    let key_lanes = if key_count >= super::field_rep::REP_SLOTS {
        u64::MAX
    } else {
        super::field_rep::lanes_below(key_count)
    };
    if infos.is_empty() || rep & !key_lanes != 0 || keys.is_null() || key_count == 0 {
        return Err(ShapeDescriptorError::InvalidFacts);
    }
    // As in `shape_descriptor_ensure_with_rep`, derive the summary from the
    // canonical keys rather than trusting a caller-supplied summary.
    let summary = unsafe { crate::object::key_attrs::keys_summary_checked(keys, key_count) };
    shape_descriptor_intern_with_special_mode(
        keys,
        key_count,
        live.max(key_count),
        0,
        ShapeObjectKind::Ordinary,
        0,
        class_proto_id(class_id),
        summary,
        rep,
        infos,
        &[],
        requested,
        true,
    )
}

/// #10123: the inline slot a PLAIN ordinary shape assigns to `key`, or `-1`.
///
/// The element-shape loop clone's shape-keyed arm asks this once per tracked
/// property, in the preheader, and the answer replaces the compile-time packed
/// field index a class-keyed clone bakes in. `key_bits` is the whole NaN-boxed
/// key as codegen loaded it from the string pool — NOT a masked
/// `StringHeader*`, because a short property name ("id") reaches the pool as an
/// SSO immediate whose masked low bits are packed characters, not an address.
///
/// **"Plain" is what makes slot k == key position k.** The four conjuncts
/// below are that claim, and dropping any one of them turns this into a wrong
/// offset rather than a missed optimization:
///
/// * `object_kind == Ordinary` — a class shape's slots are the class's
///   layout, which this function knows nothing about;
/// * `semantic_generation == 0` — a descriptor/prototype mutation minted this
///   layout, so the keys array no longer describes the live slots;
/// * `hole_count == 0` — an O(1) delete tombstones a key IN PLACE, so a later
///   key's position in the keys array is no longer its slot;
/// * `live_inline_slot_count == logical_key_count` — every key is inline; a
///   shape with spilled keys would put later ones outside the inline block.
///
/// Allocation-free and side-effect-free: it reads the descriptor, walks the
/// keys array, and returns. The walk is bounded by the physically present key
/// slots (`length.min(capacity)`), which is why a corrupted or forwarded keys
/// array costs a short scan and a `-1` rather than a spin.
#[no_mangle]
pub extern "C" fn js_shape_ordinary_inline_slot_for_key(shape_id: u32, key_bits: u64) -> i32 {
    let Some(descriptor) = shape_descriptor_by_id(shape_id) else {
        return -1;
    };
    if !descriptor.object_kind.is_ordinary_layout()
        || descriptor.semantic_generation != 0
        || descriptor.hole_count != 0
        || descriptor.live_inline_slot_count != descriptor.logical_key_count
    {
        return -1;
    }
    let mut wanted_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut stored_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    unsafe {
        let Some(wanted) = crate::string::js_string_key_bytes(
            crate::JSValue::from_bits(key_bits),
            &mut wanted_buf,
        ) else {
            return -1;
        };
        let (slots, slot_len) =
            super::keys_array_dense_slots(descriptor.keys as usize as *const ArrayHeader);
        if slots.is_null() {
            return -1;
        }
        let bound = slot_len.min(descriptor.logical_key_count as usize);
        for index in 0..bound {
            let stored = crate::JSValue::from_bits((*slots.add(index)).to_bits());
            // Identical bits is the overwhelmingly common answer for a pooled
            // key against a canonical keys array (both interned), and it is
            // correct for either representation — an SSO immediate and a heap
            // pointer each compare equal to themselves. The byte compare below
            // is what makes a MIXED pair (pool immediate vs heap key, or two
            // separately allocated heap keys) still match.
            if stored.bits() == key_bits {
                return index as i32;
            }
            if crate::string::js_string_key_bytes(stored, &mut stored_buf) == Some(wanted) {
                return index as i32;
            }
        }
    }
    -1
}

/// Step 4b loop regions: where objects of an ORDINARY, generation-0,
/// hole-free shape (the caller checked all three) keep `key` — `(false,
/// slot)` in the inline block, or `(true, position)` in the spill buffer: a
/// position at or past the live inline bound IS the key's index in the
/// buffer (`ShapeRecordRef::spill_position_of_key`). Answered in the same
/// order the megamorphic read asks: the inline range front to back
/// (`inline_slot_of_key`), then the spill range back to front (#10595: a
/// shadowed field's most-derived position wins). The list is bounded by the
/// shape's own key count, never its backing's length (#10969).
/// Allocation-free; never calls user code.
fn region_key_location(descriptor: &ShapeDescriptor, key_bits: u64) -> Option<(bool, usize)> {
    let mut wanted_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let mut stored_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    unsafe {
        let wanted = crate::string::js_string_key_bytes(
            crate::JSValue::from_bits(key_bits),
            &mut wanted_buf,
        )?;
        let (slots, len) =
            super::keys_array_dense_slots_resolved(descriptor.keys as usize as *const ArrayHeader);
        if slots.is_null() {
            return None;
        }
        let hi = len.min(descriptor.logical_key_count as usize);
        let lo = hi.min(descriptor.live_inline_slot_count as usize);
        let mut matches = |i: usize| {
            let stored = crate::JSValue::from_bits((*slots.add(i)).to_bits());
            stored.bits() == key_bits
                || crate::string::js_string_key_bytes(stored, &mut stored_buf) == Some(wanted)
        };
        if let Some(i) = (0..lo).find(|&i| matches(i)) {
            return Some((false, i));
        }
        (lo..hi).rev().find(|&i| matches(i)).map(|i| (true, i))
    }
}

/// Keepalive anchor — `js_shape_ordinary_inline_slot_for_key` is a
/// generated-code-only callee (the element-shape loop clone's shape-keyed
/// preheader), so the auto-optimize whole-program build would otherwise
/// dead-strip it (see the FFI-symbol-link-break class).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SHAPE_ORDINARY_INLINE_SLOT_FOR_KEY: extern "C" fn(u32, u64) -> i32 =
    js_shape_ordinary_inline_slot_for_key;

/// The empty region guard word: its low 32 bits are `u32::MAX`, which is never
/// a live ShapeId, so an unprimed region's shape compare can only miss.
pub const REGION_GUARD_WORD_EMPTY: u64 = 0xFFFF_FFFF;
/// Most distinct keys one region word can carry (6 bits each above the id).
pub const REGION_GUARD_MAX_KEYS: u32 = 5;
const REGION_GUARD_SLOT_BITS: u32 = 6;
const REGION_GUARD_SLOT_MAX: i32 = (1 << REGION_GUARD_SLOT_BITS) - 1;

/// Step 4b: pack a region guard word — one ShapeId and the inline slot of each
/// of the region's keys — or return [`REGION_GUARD_WORD_EMPTY`].
///
/// A read region compares the receiver's ShapeId against the low 32 bits ONCE
/// and then loads every key's slot out of the high 32 bits, so the id and the
/// slots must be published as a single atomic word. Two separately stored
/// words could tear under a concurrent prime and pair one shape's id with
/// another shape's slots — a wrong value, silently.
///
/// Each slot comes from [`js_shape_ordinary_inline_slot_for_key`], which
/// answers only when slot k provably IS key position k (ordinary kind, no
/// semantic generation, no tombstones, every key inline). Anything it refuses —
/// an absent key, an inherited key, an accessor, a spilled key — makes the
/// whole word empty, so the region never takes its fast copy for that shape
/// and every read keeps its ordinary tower. Refusing is always correct.
///
/// `keys` are the whole NaN-boxed key values as codegen loaded them from the
/// string pool, in the region's key order; `n` of them are meaningful.
#[no_mangle]
pub extern "C" fn js_region_guard_pack(
    shape_id: u32,
    n: u32,
    k0: u64,
    k1: u64,
    k2: u64,
    k3: u64,
    k4: u64,
) -> u64 {
    // A region word is a site word: only an ORDINARY-band id may enter it
    // (see `DICTIONARY_SHAPE_ID_BASE`).
    if !is_site_matchable_shape_id(shape_id) || n == 0 || n > REGION_GUARD_MAX_KEYS {
        return REGION_GUARD_WORD_EMPTY;
    }
    let keys = [k0, k1, k2, k3, k4];
    let mut word = u64::from(shape_id);
    for (i, &key) in keys.iter().enumerate().take(n as usize) {
        let slot = js_shape_ordinary_inline_slot_for_key(shape_id, key);
        if !(0..=REGION_GUARD_SLOT_MAX).contains(&slot) {
            return REGION_GUARD_WORD_EMPTY;
        }
        word |= (slot as u64) << (32 + REGION_GUARD_SLOT_BITS * i as u32);
    }
    word
}

/// Compute a region's guard word and publish it, for a read region's miss
/// path (#10884).
///
/// The store lives here rather than in emitted IR for the reason
/// [`crate::object::field_get_set::ic_miss`]'s `prime_get` does it here: a
/// cache word is published by the runtime, which owns its memory ordering.
/// Relaxed is enough — this publishes a numeric layout fact, not an object —
/// and the single word is what makes a concurrent prime unable to pair one
/// shape's id with another shape's slots. (Emitting `store atomic` from
/// codegen also does not survive perry's own native IR construction path,
/// which real modules take.)
///
/// `word` is an aligned, live site word. A shape whose layout the region
/// cannot encode publishes nothing, so the site keeps missing and the bounded
/// attempt counter in the emitted code retires it.
///
/// # Safety
///
/// `word` must be null or point to a live, 8-byte-aligned `AtomicU64`.
#[no_mangle]
pub unsafe extern "C" fn js_region_guard_prime(
    word: *const core::sync::atomic::AtomicU64,
    shape_id: u32,
    n: u32,
    k0: u64,
    k1: u64,
    k2: u64,
    k3: u64,
    k4: u64,
) -> u64 {
    let packed = js_region_guard_pack(shape_id, n, k0, k1, k2, k3, k4);
    if word.is_null() || packed == REGION_GUARD_WORD_EMPTY {
        return REGION_GUARD_WORD_EMPTY;
    }
    (*word).store(packed, core::sync::atomic::Ordering::Relaxed);
    packed
}

/// Keepalive anchor — `js_region_guard_prime` is called only from generated
/// code (a read region's miss path), so the auto-optimize whole-program build
/// would otherwise dead-strip it.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_REGION_GUARD_PRIME: unsafe extern "C" fn(
    *const core::sync::atomic::AtomicU64,
    u32,
    u32,
    u64,
    u64,
    u64,
    u64,
    u64,
) -> u64 = js_region_guard_prime;

/// Step 4b loop regions: pack a LOOP region's word, which licenses bare
/// STORES as well as bare reads, or return [`REGION_GUARD_WORD_EMPTY`].
///
/// [`js_region_guard_pack`] proves each key is an ordinary own inline slot of
/// `shape_id` (ordinary kind, semantic generation 0, no tombstones, every key
/// inline). A store additionally needs every covered key to be a WRITABLE DATA
/// property, which is the shape's attribute summary: zero means every key the
/// shape names carries default attributes (data, writable, enumerable,
/// configurable). A nonzero summary refuses the whole word; the loop then runs
/// its generic body, which is today's code. Refusing is always correct.
///
/// The per-object store facts that are NOT shape facts yet (receiver kind,
/// Array-subclass numeric proof: DESIGN §6.5a) are tested by the emitted
/// guard itself, on the object, not here.
///
/// SPILL-located keys (the S5 facts): a key at or past the shape's live
/// inline bound lives at index `position` of the object's spill buffer
/// (`ShapeRecordRef::spill_position_of_key`), and every carrier of the shape
/// has that storage — the same claim
/// the emitted `pic.spill.hit` rests on, published under the same conditions
/// (object-owned spill storage, an index it can address). Such a word carries
/// the ShapeId with `PACKED_SPILL_FLIP` flipped into it — the S5 convention —
/// so the guard's plain compare admits only all-inline words and a second
/// compare, on its miss side, selects the region's spill copy. A spill key is
/// served to READS only: a key in `stored_mask` must be inline (a spill store
/// owes the buffer's own GC bookkeeping, which the bare store does not do).
///
/// A key in `boxed_mask` is one a bare store may write a value the compiler
/// did not prove a canonical double (charter step 5): the bare store runs no
/// field-representation check, so its slot must be an `Any` lane of the
/// shape. A proven canonical double is valid for `Any` and `F64` lanes.
/// Any stored SPECIAL lane is refused: a ConstFn body change requires the
/// checked slot funnel even when the new value is a canonical Number.
/// A read-only numeric region may also use `OrdinaryUnmarked`: the missing
/// birth mark withdraws store permission, not the own-data slot layout.
/// Receivers with virtual read semantics remain refused by their prototype
/// classification, and every covered key must be requested as an inline
/// Number read. A Number read (R) on a lane that is not an identity F64 lane
/// sets [`REGION_LOOP_WORD_VALUE_TEST`]: the guard then tests the value.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn js_region_loop_pack(
    shape_id: u32,
    n: u32,
    k0: u64,
    k1: u64,
    k2: u64,
    k3: u64,
    k4: u64,
    stored_mask: u32,
    boxed_mask: u32,
) -> u64 {
    region_loop_pack(
        shape_id,
        n,
        [k0, k1, k2, k3, k4],
        stored_mask,
        boxed_mask,
        0,
    )
    .unwrap_or(REGION_GUARD_WORD_EMPTY)
}

/// Why [`js_region_loop_pack`] refused a shape — the route census's refusal
/// histogram (DESIGN §9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionRefusal {
    /// Not an ordinary-band ShapeId (dictionary / exotic), or a bad key count.
    Band,
    /// A key is an accessor or not writable/enumerable/configurable data.
    Summary,
    /// Ineligible receiver/read kind, non-zero semantic generation, or tombstones.
    Kind,
    /// A key is not in the shape at all (inherited, or absent).
    Absent,
    /// A key the body STORES is spill-located.
    SpillStored,
    /// A spill-located key the S5 path could not serve (storage disabled,
    /// index past `SPILL_MAX_FIELD_INDEX`, or past the word's 6-bit field).
    SpillUnservable,
    /// An inline slot past the word's 6-bit field (>= 32).
    Range,
    /// A key a bare store may write a non-double into is not an `Any` lane.
    F64Stored,
    /// A requested Number read is spill-located, on a SPECIAL lane, or on a
    /// non-identity lane a bare store may write a non-Number into; or a bare
    /// store targets a SPECIAL lane that requires the checked slot funnel.
    Rep,
}

fn region_loop_pack(
    shape_id: u32,
    n: u32,
    keys: [u64; 5],
    stored_mask: u32,
    boxed_mask: u32,
    r_mask: u32,
) -> Result<u64, RegionRefusal> {
    use RegionRefusal::*;
    if !is_site_matchable_shape_id(shape_id) || n == 0 || n > REGION_GUARD_MAX_KEYS {
        return Err(Band);
    }
    match shape_record_by_id(shape_id) {
        Some(record) if record.summary() == 0 => {}
        Some(_) => return Err(Summary),
        None => return Err(Band),
    }
    let Some(descriptor) = shape_descriptor_by_id(shape_id) else {
        return Err(Band);
    };
    let unmarked_numeric_read = descriptor.object_kind == ShapeObjectKind::OrdinaryUnmarked
        && stored_mask == 0
        && boxed_mask == 0
        && r_mask == (1 << n) - 1;
    if (descriptor.object_kind != ShapeObjectKind::Ordinary && !unmarked_numeric_read)
        || descriptor.proto_id == PROTO_ID_PER_OBJECT
        || descriptor.semantic_generation != 0
        || descriptor.hole_count != 0
    {
        return Err(Kind);
    }
    // (spilled, inline slot or spill index) per key.
    let mut at = [(false, 0usize); REGION_GUARD_MAX_KEYS as usize];
    let mut any_spill = false;
    for (i, &key) in keys.iter().enumerate().take(n as usize) {
        let (spilled, position) = region_key_location(&descriptor, key).ok_or(Absent)?;
        if spilled {
            if stored_mask & (1 << i) != 0 {
                return Err(SpillStored);
            }
            if !super::object_spill_enabled() || position >= super::SPILL_MAX_FIELD_INDEX {
                return Err(SpillUnservable);
            }
            any_spill = true;
        }
        at[i] = (spilled, position);
    }
    // Every field is `slot` (< 32) or `32 + spill index` (< 63), in BOTH
    // kinds of word: a region with two receivers may run its spill copy for
    // one receiver's spill word while the other's word is all-inline, and
    // that copy reads each field the same way.
    let id = if any_spill {
        shape_id ^ crate::object::field_get_set::PACKED_SPILL_FLIP
    } else {
        shape_id
    };
    let mut word = u64::from(id);
    let mut value_test = false;
    for (i, &(spilled, n_at)) in at.iter().enumerate().take(n as usize) {
        // A canonical Number cannot preserve a ConstFn body identity. Every
        // SPECIAL write must use the checked slot funnel before storing;
        // numeric writes to other lanes and read-only SPECIAL keys are safe.
        if !spilled
            && stored_mask & (1 << i) != 0
            && n_at < super::field_rep::REP_SLOTS as usize
            && super::field_rep::slot_rep(descriptor.rep, n_at as u32)
                == super::field_rep::REP_SPECIAL
        {
            return Err(Rep);
        }
        if r_mask & (1 << i) != 0 {
            // R wants the slot's raw bits to be a canonical Number. An inline
            // identity F64 lane guarantees it for every carrier. Any other
            // inline lane that is not SPECIAL (Any, or a deprecated F64) can
            // hold it per OBJECT: the word then carries
            // `REGION_LOOP_WORD_VALUE_TEST` and the emitted guard tests each
            // R slot's value on the object before F runs (and again on every
            // re-check). Inside F nothing writes such a slot except a bare
            // store, so a key a bare store may write a non-Number into
            // (`boxed_mask`) cannot be R.
            if spilled || n_at >= super::field_rep::REP_SLOTS as usize {
                return Err(Rep);
            }
            match super::field_rep::slot_rep(descriptor.rep, n_at as u32) {
                super::field_rep::REP_F64 => {}
                super::field_rep::REP_SPECIAL => return Err(Rep),
                _ if boxed_mask & (1 << i) != 0 => return Err(Rep),
                _ => value_test = true,
            }
        }
        if !spilled
            && boxed_mask & (1 << i) != 0
            && (n_at as u32) < super::field_rep::REP_SLOTS
            && super::field_rep::slot_rep(descriptor.rep, n_at as u32) != super::field_rep::REP_ANY
        {
            return Err(F64Stored);
        }
        let field = match spilled {
            false if n_at < 32 => n_at,
            true if n_at < 31 => 32 + n_at,
            false => return Err(Range),
            true => return Err(SpillUnservable),
        };
        word |= (field as u64) << (32 + REGION_GUARD_SLOT_BITS * i as u32);
    }
    if value_test {
        word |= REGION_LOOP_WORD_VALUE_TEST;
    }
    Ok(word)
}

/// The word a loop region's site holds once its last bounded prime attempt
/// was refused: all ones. Its id half is `REGION_GUARD_WORD_EMPTY`'s, which no
/// object carries, so it can never match; the emitted guard tests for it
/// FIRST and skips the receiver test (DESIGN §4.3).
pub const REGION_LOOP_WORD_RETIRED: u64 = u64::MAX;

/// Bit 63 of a published loop-region word: some key the region reads as a
/// Number (R) sits on a lane that does not guarantee one for every carrier
/// (an `Any` or deprecated lane), so the emitted guard must test each R
/// slot's value on the object itself before F runs. Field indices occupy
/// bits 32..62 at most (five 6-bit fields), so the bit is free; the word's
/// id half is a ShapeId, so a word carrying it is never
/// [`REGION_LOOP_WORD_RETIRED`].
pub const REGION_LOOP_WORD_VALUE_TEST: u64 = 1 << 63;

/// Compute a loop region's word ([`js_region_loop_pack`]) and publish it; the
/// store-side twin of [`js_region_guard_prime`], with its memory ordering.
/// `last` is non-zero on the site's final bounded attempt: a refusal then
/// publishes [`REGION_LOOP_WORD_RETIRED`].
///
/// # Safety
///
/// `word` must be null or point to a live, 8-byte-aligned `AtomicU64`.
#[no_mangle]
pub unsafe extern "C" fn js_region_loop_prime(
    word: *const core::sync::atomic::AtomicU64,
    shape_id: u32,
    n: u32,
    k0: u64,
    k1: u64,
    k2: u64,
    k3: u64,
    k4: u64,
    last: u32,
    stored_mask: u32,
    boxed_mask: u32,
    r_mask: u32,
) -> u64 {
    let verdict = region_loop_pack(
        shape_id,
        n,
        [k0, k1, k2, k3, k4],
        stored_mask,
        boxed_mask,
        r_mask,
    );
    region_loop_prime_census(verdict);
    let packed = verdict.unwrap_or(REGION_GUARD_WORD_EMPTY);
    if word.is_null() {
        return REGION_GUARD_WORD_EMPTY;
    }
    if packed == REGION_GUARD_WORD_EMPTY {
        if last != 0 {
            (*word).store(
                REGION_LOOP_WORD_RETIRED,
                core::sync::atomic::Ordering::Relaxed,
            );
            crate::hot_diag::recv_route_note_runtime(crate::hot_diag::RT_ROUTE_RLOOP_RETIRE);
        }
        return REGION_GUARD_WORD_EMPTY;
    }
    (*word).store(packed, core::sync::atomic::Ordering::Relaxed);
    packed
}

/// The route census's verdict on one loop-region prime (a relaxed load and a
/// not-taken branch outside a census build): accepted, or WHY it was refused
/// ([`RegionRefusal`]).
fn region_loop_prime_census(verdict: Result<u64, RegionRefusal>) {
    use crate::hot_diag::{
        recv_route_note_runtime, RT_ROUTE_RLOOP_PRIME_OK, RT_ROUTE_RLOOP_REFUSE_ABSENT,
        RT_ROUTE_RLOOP_REFUSE_BAND, RT_ROUTE_RLOOP_REFUSE_F64_STORED, RT_ROUTE_RLOOP_REFUSE_KIND,
        RT_ROUTE_RLOOP_REFUSE_RANGE, RT_ROUTE_RLOOP_REFUSE_REP, RT_ROUTE_RLOOP_REFUSE_SPILL_STORED,
        RT_ROUTE_RLOOP_REFUSE_SPILL_UNSERVABLE, RT_ROUTE_RLOOP_REFUSE_SUMMARY,
    };
    let route = match verdict {
        Ok(_) => RT_ROUTE_RLOOP_PRIME_OK,
        Err(RegionRefusal::Band) => RT_ROUTE_RLOOP_REFUSE_BAND,
        Err(RegionRefusal::Summary) => RT_ROUTE_RLOOP_REFUSE_SUMMARY,
        Err(RegionRefusal::Kind) => RT_ROUTE_RLOOP_REFUSE_KIND,
        Err(RegionRefusal::Absent) => RT_ROUTE_RLOOP_REFUSE_ABSENT,
        Err(RegionRefusal::SpillStored) => RT_ROUTE_RLOOP_REFUSE_SPILL_STORED,
        Err(RegionRefusal::SpillUnservable) => RT_ROUTE_RLOOP_REFUSE_SPILL_UNSERVABLE,
        Err(RegionRefusal::Range) => RT_ROUTE_RLOOP_REFUSE_RANGE,
        Err(RegionRefusal::F64Stored) => RT_ROUTE_RLOOP_REFUSE_F64_STORED,
        Err(RegionRefusal::Rep) => RT_ROUTE_RLOOP_REFUSE_REP,
    };
    recv_route_note_runtime(route);
}

/// Keepalive anchor — `js_region_loop_prime` is called only from generated
/// code (a loop region's entry miss).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_REGION_LOOP_PRIME: unsafe extern "C" fn(
    *const core::sync::atomic::AtomicU64,
    u32,
    u32,
    u64,
    u64,
    u64,
    u64,
    u64,
    u32,
    u32,
    u32,
    u32,
) -> u64 = js_region_loop_prime;

// ---------------------------------------------------------------------------
// #8067 — THE SHAPE WORD IS UNIFORM AND AUTHORITATIVE.
//
// `ObjectHeader.parent_class_id` is the shape word. Every shaped object is
// birth-stamped; inheritance lives in the class-id-keyed registry instead.
//
// The gate is gone. The rule is now, for every receiver kind:
//
//     the word is a ShapeId  <=>  is_shape_id(word)
//
// which is exactly what emitted PICs test: the ShapeId range and value, never a
// moving keys address or an ObjectHeader compatibility mirror.
// ---------------------------------------------------------------------------

/// True when `obj` really is an `ObjectHeader` whose word 2 may be written.
///
/// RegExp now has a distinct GC kind, so ShapeId publication never needs to
/// inspect an ObjectHeader payload word to distinguish it.
#[inline]
pub(crate) unsafe fn shape_word_is_writable(obj: *const crate::object::ObjectHeader) -> bool {
    crate::object::object_is_shaped(obj)
}

/// The receiver's ShapeId, or 0 when it is not a shaped object.
#[inline]
pub(crate) unsafe fn object_shape_stamp(obj: *const crate::object::ObjectHeader) -> u32 {
    let word = (*obj).parent_class_id;
    if is_shape_id(word) {
        word
    } else {
        0
    }
}

#[cfg(test)]
thread_local! {
    static TEST_CACHED_TRANSITION_WATCH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TEST_CACHED_TRANSITION_STAMPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Install a transition cache's exact successor without re-hashing descriptor
/// facts. The predecessor ShapeId is the complete semantic guard: it includes
/// the ordered keys edge, live-slot bound, semantic generation, and object
/// kind. A mismatch declines to the ordinary mint-or-find publication path.
///
/// The cache strongly roots `target_keys`, so copying GC rewrites both that
/// edge and the descriptor named by `target_shape_id` while the stable IDs stay
/// unchanged. Entries are learned only after the slow path has published the
/// successor, and ShapeIds are never reused.
#[inline]
pub(crate) unsafe fn install_cached_object_shape_transition(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    target_keys: crate::object::ObjectKeys,
) -> bool {
    install_cached_object_shape_version(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        target_keys.arr(),
        target_keys.count(),
    )
}

/// Install an exact historical shape version whose authoritative keys array
/// may have grown in place since the descriptor was minted. Reflection and
/// field tracing use the descriptor's logical bound, not the backing array's
/// later physical length.
#[inline]
pub(crate) unsafe fn install_cached_object_shape_version(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
) -> bool {
    install_cached_object_shape_version_impl(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        _target_keys,
        _target_key_count,
        false,
    )
}

/// Install a historical shape held by an optimization cache that permanently
/// owns the target descriptor and roots its keys array.
///
/// Unlike the general cached-shape entry, this does not need to probe the
/// shape table merely to note an old-generation carrier: `cache_carrier`
/// already keeps the descriptor and keys live for the lifetime of the cache,
/// which is strictly stronger than the epoch-scoped old-carrier note. The
/// Array-subclass tail cache establishes that ownership before publishing an
/// edge and never returns an unowned entry.
#[inline]
pub(crate) unsafe fn install_cache_carried_object_shape_version(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
) -> bool {
    install_cached_object_shape_version_impl(
        obj,
        expected_predecessor_shape_id,
        target_shape_id,
        _target_keys,
        _target_key_count,
        true,
    )
}

#[inline]
unsafe fn install_cached_object_shape_version_impl(
    obj: *mut crate::object::ObjectHeader,
    expected_predecessor_shape_id: u32,
    target_shape_id: u32,
    _target_keys: *mut ArrayHeader,
    _target_key_count: u32,
    target_is_cache_carried: bool,
) -> bool {
    if obj.is_null()
        || !shape_word_is_writable(obj)
        || object_shape_stamp(obj) != expected_predecessor_shape_id
        || !is_shape_id(target_shape_id)
    {
        return false;
    }

    // Debug/test builds verify the cache-to-table invariant before trusting
    // the constant-time release publication. This lookup_ways is compiled out of
    // optimized release builds, where full-GC pruning validates both ShapeIds
    // and the cache's rooted target edge keeps its descriptor live.
    #[cfg(debug_assertions)]
    {
        if !shape_descriptor_by_id(target_shape_id).is_some_and(|descriptor| {
            descriptor.keys == _target_keys as u64
                && descriptor.logical_key_count == _target_key_count
                && (!target_is_cache_carried || descriptor.cache_carrier)
        }) {
            return false;
        }
    }

    // Match `set_object_keys_array_with_live`: representation feedback must be
    // invalidated while the predecessor stamp is still authoritative.
    if target_is_cache_carried {
        // `cache_carrier` already roots the target descriptor for the
        // cache's lifetime — strictly stronger than the epoch-scoped
        // old-carrier note, so this stamp deliberately skips the funnel's
        // descriptor probe (see the function doc above). The proof retire
        // (R5) is not skipped: it is one header load without a proof.
        store_kind::retire_proof_before_stamp(obj, target_shape_id);
        (*obj).parent_class_id = target_shape_id;
    } else {
        stamp_object_shape_id_with_carrier_note(obj, target_shape_id);
    }

    #[cfg(debug_assertions)]
    debug_assert_object_shape_parity_for_keys(
        obj,
        crate::object::ObjectKeys::new(_target_keys, _target_key_count),
    );
    // The successor was minted from a receiver that carried the same
    // predecessor, so it derived the same F-A (R2); checked, never trusted.
    store_kind::check_store_facts(obj);
    #[cfg(test)]
    TEST_CACHED_TRANSITION_WATCH.with(|watch| {
        if watch.get() == obj as usize {
            TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(hits.get() + 1));
        }
    });
    true
}

#[cfg(test)]
pub(crate) fn test_reset_cached_transition_stamps() {
    TEST_CACHED_TRANSITION_WATCH.with(|watch| watch.set(0));
    TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(0));
}

#[cfg(test)]
pub(crate) fn test_watch_cached_transition_stamps(obj: usize) {
    TEST_CACHED_TRANSITION_WATCH.with(|watch| watch.set(obj));
    TEST_CACHED_TRANSITION_STAMPS.with(|hits| hits.set(0));
}

#[cfg(test)]
pub(crate) fn test_cached_transition_stamps() -> u64 {
    TEST_CACHED_TRANSITION_STAMPS.with(std::cell::Cell::get)
}

/// Stamp `obj` with the exact ShapeId of `keys`, minting the descriptor on
/// first touch. Returns 0 only when the receiver is not a shaped object.
/// Exhaustion fails stop: no live object may depend on the
/// compatibility pointer/count mirrors for its shape.
#[inline]
pub(crate) unsafe fn stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    keys: *const ArrayHeader,
    key_count: u32,
    live_inline_slot_count: u32,
) -> u32 {
    if !shape_word_is_writable(obj) {
        return 0;
    }
    let Some(lineage) = object_shape_descriptor(obj) else {
        crate::array::clear_array_subclass_named_prefix_token(obj);
        let id = shape_descriptor_ensure_for_object(obj, keys, key_count, live_inline_slot_count)
            .unwrap_or_else(|error| shape_descriptor_error_abort(error));
        stamp_object_shape_id_with_carrier_note(obj, id);
        debug_assert_object_shape_parity(obj);
        return id;
    };
    // Charter step 5: a restamp of the SAME keys edge moves no slot, so the
    // lineage's field representation carries (normalized). Any other edge
    // publishes all-`Any`, which is always a valid claim.
    let rep = if lineage.keys == keys as u64 && lineage.logical_key_count == key_count {
        super::field_rep::normalized_without_special(lineage.rep)
    } else {
        super::field_rep::REP_ANY
    };
    // A same-facts republish (the read side's `lookup_ways`) keeps a
    // proof-carrying receiver on its proof shape (charter step 3): it changes
    // nothing the proof depends on, so retiring the proof here would make
    // every read of the receiver re-prove it.
    let kind = if lineage.object_kind == ShapeObjectKind::OrdinaryNumericProof
        && lineage.keys == keys as u64
        && lineage.logical_key_count == key_count
        && store_kind::receiver_carries_numeric_proof(obj)
    {
        ShapeObjectKind::OrdinaryNumericProof
    } else {
        store_kind::mint_kind(lineage.object_kind, obj)
    };
    let id = publish_shape_result(shape_descriptor_ensure_with_rep(
        keys,
        key_count,
        lineage.live_inline_slot_count,
        lineage.semantic_generation,
        kind,
        // Same-array restamp: physical holes persist, so must the count
        // (see the lineage publish below for the churn-growth rationale).
        lineage.hole_count,
        lineage.proto_id,
        receiver_facts_of_current(obj, &lineage),
        rep,
        None,
    ));
    if id != (*obj).parent_class_id {
        // Read-side lookup_ways also calls `stamp_object_shape` to populate its
        // field cache. Preserve a proved Array-subclass prefix when that call
        // merely republishes the exact current descriptor; retire it only for
        // an actual structural identity change.
        crate::array::clear_array_subclass_named_prefix_token(obj);
    }
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Birth-stamp a NEWBORN receiver with an already-minted ShapeId after checking
/// its descriptor against the completed header. A missing, foreign, or
/// count-mismatched id is replaced with an exact local descriptor. A valid
/// process-global id absent from this worker is installed with the worker's
/// local moving keys pointer before it is stamped.
///
/// Every allocator that installs a shape-cached keys array on a fresh
/// `ObjectHeader` must call this so all runtime and emitted guards observe the
/// same descriptor identity from birth.
///
/// `live_inline_slot_count` is the birth bound the allocator sized the object
/// with. #8113: it is a parameter rather than a `(*obj).field_count` read
/// because the header no longer carries the word — the descriptor this
/// publishes is the only record of it.
///
/// No `shape_word_is_writable` check beyond the null test: the callers have just
/// written `class_id` into a header they allocated, so the receiver is a genuine
/// `ObjectHeader` and never the `RegExpHeader` alias.
///
/// `rep` is the newborn's birth rep (charter step 5): its shape is its
/// structural facts WITH that rep, whichever allocator runs. The supplied id is
/// taken only when it names that rep too, a worker's install of it carries it,
/// and the exact fallback is minted with it. So a class or literal born with
/// `F64` lanes gets the one (keys, proto, rep) id on every path, never an
/// all-`Any` twin of it. The caller fills the `F64` lanes
/// (`field_rep_store::birth_fill_f64_lanes`) once the slots are initialized.
#[inline]
pub(crate) unsafe fn birth_stamp_object_shape(
    obj: *mut crate::object::ObjectHeader,
    runtime_shape_id: u32,
    live_inline_slot_count: u32,
    rep: u64,
) {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return;
    }
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        birth_publish_object_shape(obj, live_inline_slot_count);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let keys = current.keys as usize as *mut ArrayHeader;
    let key_count = current.logical_key_count;
    let supplied_id_is_local =
        (descriptor_matches_object(runtime_shape_id, obj, live_inline_slot_count)
            && shape_descriptor_field_by_id(runtime_shape_id, |d| {
                super::field_rep::identity_with_special(d.rep) == rep
            }) == Some(true))
            || shapes_slot_list::install_external_shape_id(
                runtime_shape_id,
                keys,
                key_count,
                live_inline_slot_count,
                object_proto_id(obj),
                store_kind::receiver_ordinary_kind(obj),
                rep,
            );
    if supplied_id_is_local {
        stamp_object_shape_id_with_carrier_note(obj, runtime_shape_id);
        debug_assert_object_shape_parity(obj);
    } else if rep != super::field_rep::REP_ANY {
        // The exact descriptor below is all-`Any`; the newborn's shape is those
        // facts with its birth rep. Minted while that descriptor is stamped
        // (the `publish_object_live_slot_count` discipline).
        let id = publish_shape_result(shape_descriptor_ensure_with_rep(
            keys,
            key_count,
            live_inline_slot_count,
            current.semantic_generation,
            current.object_kind,
            current.hole_count,
            current.proto_id,
            receiver_facts(obj),
            rep,
            None,
        ));
        stamp_object_shape_id_with_carrier_note(obj, id);
        debug_assert_object_shape_parity(obj);
    } else {
        // `current` was just published from the newborn's explicit keys edge
        // and allocation bound, so it is already the exact descriptor.  The
        // cached id can legitimately disagree when an object reserves hidden
        // inline slots that have no public key (fs.Stats has 21 keys and four
        // hidden Date slots).  Before #8047 this fallback rebuilt the same
        // facts from the header's `keys_array` mirror.  With that mirror gone,
        // rebuilding through `birth_publish_object_shape` would instead use a
        // null edge and overwrite the exact 21/25 descriptor with a keyless
        // 0/25 one.  Keep the exact descriptor already stamped by
        // `set_object_keys_array_with_live`.
        debug_assert_object_shape_parity(obj);
    }
}

/// Stamp a newborn compiled-class allocation from the ShapeId installed at
/// module initialization, without re-canonicalizing the same shape facts.
///
/// A hit proves the immutable ordered-keys edge, logical key count, and live
/// inline-slot bound directly from the agent-local descriptor. The id and keys
/// pointer arrive through separate module globals, so every structural fact is
/// checked before the single stamp store. Missing worker-local ids, key-count
/// drift, and learned-width mismatches return `false` for the existing
/// mint-and-validate path to handle.
///
/// # Safety
///
/// `obj` must be a freshly allocated, unpublished `ObjectHeader` and `keys`
/// must be the module-init canonical keys pointer paired with
/// `runtime_shape_id`. No allocation or collection may occur between this
/// function returning `true` and initialization of the newborn's fields.
#[inline]
pub(crate) unsafe fn try_birth_stamp_preinstalled_shape(
    obj: *mut crate::object::ObjectHeader,
    runtime_shape_id: u32,
    keys: crate::object::ObjectKeys,
    live_inline_slot_count: u32,
) -> bool {
    if obj.is_null() {
        return false;
    }
    let Some(descriptor) = shape_descriptor_by_id(runtime_shape_id) else {
        return false;
    };
    if descriptor.keys != keys.arr() as u64
        || descriptor.logical_key_count != keys.count()
        || descriptor.live_inline_slot_count != live_inline_slot_count
        || descriptor.semantic_generation != 0
        || descriptor.object_kind != store_kind::receiver_ordinary_kind(obj)
        || descriptor.proto_id != object_proto_id(obj)
    {
        if descriptor.object_kind.is_ordinary_layout()
            && descriptor.object_kind != store_kind::receiver_ordinary_kind(obj)
        {
            store_kind::audit::note_explicit_decline();
        }
        return false;
    }
    (*obj).parent_class_id = runtime_shape_id;
    if !crate::arena::pointer_in_nursery(obj as usize) {
        note_old_generation_carrier(descriptor.record_ref());
    }
    debug_assert_object_shape_parity(obj);
    store_kind::check_store_facts(obj);
    true
}

/// Publish the exact descriptor for a FRESHLY ALLOCATED header. #8113: the
/// birth live-slot bound must be supplied because no header word carries it.
///
/// Mint-then-stamp: `shape_descriptor_ensure_with_generation` can collect, and
/// at that point the object is still unstamped, which is sound only because it
/// is also still unpublished — the allocator has not returned it and no live
/// edge reaches it. Every LATER bound change goes through
/// [`publish_object_live_slot_count`], which keeps a valid predecessor stamp
/// across the mint.
#[inline]
pub(crate) unsafe fn birth_publish_object_shape(
    obj: *mut crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> u32 {
    synchronize_object_shape_descriptor_from(obj, None, live_inline_slot_count)
}

/// Publish a new live inline-slot bound for an ALREADY PUBLISHED object.
///
/// This is the #8113 replacement for `(*obj).field_count = n`. The successor
/// descriptor is minted while the predecessor stamp is still installed, so a
/// collection inside the mint observes the OLD bound — correct, because the
/// slot the caller is about to expose has not been written yet — and the new
/// bound becomes visible at the single `parent_class_id` store, which cannot
/// allocate and therefore cannot collect.
pub(crate) unsafe fn publish_object_live_slot_count(
    obj: *mut crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> u32 {
    publish_object_live_slot_count_rep(obj, live_inline_slot_count, None)
}

/// [`publish_object_live_slot_count`] whose successor carries `rep` (charter
/// step 5, T2: the key-add that grows the bound publishes its value's lane
/// here). `None` carries the predecessor's lanes below the new bound.
pub(crate) unsafe fn publish_object_live_slot_count_rep(
    obj: *mut crate::object::ObjectHeader,
    live_inline_slot_count: u32,
    rep: Option<u64>,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    let predecessor = object_shape_descriptor(obj);
    if let Some(current) = predecessor {
        if current.live_inline_slot_count == live_inline_slot_count {
            debug_assert_object_shape_parity(obj);
            return object_shape_stamp(obj);
        }
    }
    match predecessor {
        // Charter step 5: a bound change moves no slot, so the predecessor's
        // lanes below the new bound stay valid claims.
        Some(current) => publish_object_shape_from_rep(
            obj,
            Some(current),
            current.keys_view(),
            live_inline_slot_count,
            rep.unwrap_or_else(|| {
                super::field_rep::normalized_without_special(current.rep)
                    & super::field_rep::lanes_below(live_inline_slot_count)
            }),
        ),
        None => synchronize_object_shape_descriptor_from(obj, None, live_inline_slot_count),
    }
}

/// Install the exact descriptor for the object's current authoritative keys
/// edge, preserving the live inline-slot bound the receiver already carries.
/// This is the only structural shape publication operation used by mutations.
/// Keyless objects receive a descriptor too.
///
/// #8113: an UNSTAMPED receiver has no recorded bound anywhere, so this
/// publishes 0 for it rather than inventing one. Callers that know the bound
/// (allocators, the by-name append path) must use
/// [`birth_publish_object_shape`] / [`publish_object_live_slot_count`].
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn synchronize_object_shape_descriptor(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    let predecessor = object_shape_descriptor(obj);
    let live = predecessor
        .map(|descriptor| descriptor.live_inline_slot_count)
        .unwrap_or(0);
    synchronize_object_shape_descriptor_from(obj, predecessor, live)
}

/// Structural synchronization across a keys-edge or slot-bound mutation.
/// `predecessor` carries semantic lineage (including class kind) across the
/// mutation without exposing stale structural facts.
///
/// MINT-THEN-STAMP (#8113): every allocation below happens with the
/// predecessor stamp still installed; the receiver's published shape changes at
/// the final `parent_class_id` store and nowhere else.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn synchronize_object_shape_descriptor_from(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
    live_inline_slot_count: u32,
) -> u32 {
    if obj.is_null() {
        return 0;
    }
    let keys = predecessor
        .map(|descriptor| descriptor.keys_view())
        .unwrap_or(crate::object::ObjectKeys::NONE);
    publish_object_shape_from(obj, predecessor, keys, live_inline_slot_count)
}

/// Publish the exact descriptor for an EXPLICIT keys edge — which may not be
/// the one the header currently holds.
///
/// This is what makes the keys-edge mutation mint-then-stamp (#8113). The
/// caller stamps the successor here, with the predecessor still describing the
/// current edge throughout every allocation inside. The final ShapeId store is
/// the atomic publication point for the new descriptor and its rooted edge.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn publish_object_shape_from(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
    keys_view: crate::object::ObjectKeys,
    live_inline_slot_count: u32,
) -> u32 {
    publish_object_shape_from_rep(
        obj,
        predecessor,
        keys_view,
        live_inline_slot_count,
        super::field_rep::REP_ANY,
    )
}

/// [`publish_object_shape_from`] with the successor's field representation
/// (charter step 5). A caller passes a rep only when it knows no slot moved
/// relative to the predecessor it carries lanes from; `REP_ANY` is always a
/// valid claim.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn publish_object_shape_from_rep(
    obj: *mut crate::object::ObjectHeader,
    predecessor: Option<ShapeDescriptor>,
    keys_view: crate::object::ObjectKeys,
    live_inline_slot_count: u32,
    rep: u64,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    // Generic structural publication may add/delete/reorder a named field.
    // The learned exact numeric-tail installer has its own entry point and
    // intentionally preserves this Array-subclass family proof.
    crate::array::clear_array_subclass_named_prefix_token(obj);
    // The caller states the count: a keys array's header length is not a
    // receiver's key count (see `ObjectKeys`).
    let keys = keys_view.arr();
    let key_count = keys_view.count();

    // A same-address length change is legal only for an owned keys array. A
    // shared array must have cloned before push; otherwise siblings already
    // observe mutated bytes and no descriptor can make that state sound.
    let old_id = object_shape_stamp(obj);
    let mut retire_owned_history = false;
    if let Some(old) = shape_descriptor_by_id(old_id) {
        // #9064: an owned ordinary receiver that already entered stable-
        // tombstone mode keeps its id across same-allocation tail appends and
        // live-bound growth. Cached slots validate `TAG_HOLE`, so the deleted
        // slot stays a miss while every surviving slot remains valid. A keys
        // reallocation declines inside the helper and takes the ordinary
        // mint-then-stamp path below.
        if let Some(id) = try_update_stable_tombstone_shape(
            obj,
            keys,
            key_count,
            live_inline_slot_count,
            old.hole_count,
        ) {
            return id;
        }
        if old.keys == keys as u64 && old.logical_key_count != key_count {
            // #8113: these three arms are unreachable-by-construction defenses
            // (`debug_assert!` below). They deliberately leave the receiver
            // STAMPED with its predecessor rather than clearing: an unstamped
            // object now has no live-slot bound at all, so clearing would turn
            // a shape-identity fault into heap-payload loss.
            let Some(gc) = crate::value::addr_class::try_read_tracked_gc_header(keys as usize)
            else {
                return old_id;
            };
            if (*gc.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
                return old_id;
            }
            let shared = (*gc.as_ptr()).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0;
            // A SHARED keys array is a canonical backing: the lists on one
            // growth chain are prefixes of it, each with its own descriptor,
            // and a prefix never changes (only the tip is appended, past every
            // published count). Moving to a longer or shorter prefix of it is
            // an ordinary transition with no history to retire. An OWNED
            // array has one carrier, whose earlier same-address versions this
            // publish supersedes.
            if !shared {
                // An Array-subclass receiver is the one owner whose history IS
                // reinstalled: `array_tail_transition` learns the (predecessor,
                // successor) pair right after this publish returns and its
                // reverse edge stamps the predecessor back on `pop`. That cache
                // takes ownership through `cache_carrier`, but only once the
                // learner has run, so the gate here is the receiver kind the
                // learner is scoped to (`record_array_tail` in the append tail).
                retire_owned_history = !crate::array::is_array_subclass_class_id((*obj).class_id);
            }
        }
    }

    // A caller-supplied predecessor was captured before it temporarily
    // cleared the stamp to mutate structural facts, so it is the semantic
    // authority for this transition. A re-entrant observer can defensively
    // self-heal the zero stamp in that window; never let that interim
    // descriptor replace the saved class/semantic lineage.
    let lineage = predecessor.or_else(|| shape_descriptor_by_id(old_id));
    let semantic_generation = lineage
        .map(|descriptor| descriptor.semantic_generation)
        .unwrap_or(0);
    // Charter step 3 (R2): an ordinary-family kind is the RECEIVER's, never
    // the lineage's — a lineage cannot carry a stale F-A or a numeric proof.
    let object_kind = store_kind::mint_kind(
        lineage
            .map(|descriptor| descriptor.object_kind)
            .unwrap_or(ShapeObjectKind::Ordinary),
        obj,
    );
    // Tombstones (#9029): an append or grow-realloc keeps every hole slot
    // physically in the array, so the successor must inherit the count — a
    // reset would let delete/re-add churn dodge the squeeze threshold
    // forever and grow the array unbounded. Only the squeeze itself (which
    // physically removes the holes) publishes 0, explicitly.
    let hole_count = lineage.map(|descriptor| descriptor.hole_count).unwrap_or(0);
    // The prototype identity is carried like the other semantic facts; a
    // receiver with no lineage (an unstamped newborn) reads its own.
    let proto_id = match lineage {
        Some(descriptor) => descriptor.proto_id,
        None => object_proto_id(obj),
    };
    let id = publish_shape_result(shape_descriptor_ensure_with_rep(
        keys,
        key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        // The brands are the lineage's, which is the receiver's shape even
        // while a caller holds the stamp cleared (#11791).
        match lineage {
            Some(descriptor) => {
                ReceiverFacts::of_descriptor(&descriptor, receiver_extra_summary(obj))
            }
            None => ReceiverFacts::summary(receiver_extra_summary(obj)),
        },
        rep,
        None,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    if retire_owned_history {
        // #9706: the array is OWNED, so this receiver was the only carrier of
        // every earlier same-address version, and the stamp above just
        // superseded the last of them. Retire the growth history now rather
        // than leaving one prefix descriptor per append alive until the
        // array itself dies: on the compiled claude-code TUI that history was
        // most of the descriptor table. Ordered after the stamp for the same
        // reason as the tombstone publish (#9200) — the successor must be
        // armed before the armed predecessor goes.
        retire_owned_shape_siblings(keys as u64, id);
    }
    debug_assert_object_shape_parity_for_keys(obj, keys_view);
    id
}

/// Retire every descriptor of an OWNED keys array other than `keep`.
///
/// Sound because `GC_FLAG_SHAPE_SHARED` is sticky: an array without it has
/// had exactly one owner for its whole life, and that owner now carries
/// `keep`. A stale IC token already misses on the stamp compare and
/// `shape_descriptor_by_id` of a retired id is `None`, so nothing can observe
/// the retired versions — with one exception: a descriptor an optimization
/// cache permanently owns (`cache_carrier`) may be reinstalled by that cache
/// while no live object carries it, so it stays.
fn retire_owned_shape_siblings(keys: u64, keep: u32) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let stale: Vec<u32> = inner
        .families
        .get(&keys)
        .map(|ids| {
            ids.as_slice()
                .iter()
                .copied()
                .filter(|&id| {
                    id != keep
                        && table.slab().get(id).is_some_and(|record| {
                            !record.has(RECORD_FLAG_CACHE_CARRIER | RECORD_FLAG_EXTERNAL_CARRIER)
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// RULE 1 for an accessor whose FUNCTION was replaced while its attributes
/// did not change (`Object.defineProperty(o, k, { get: other })` over an
/// accessor `k`). The attributes live with the keys and are unchanged, so
/// the key list — and with it every other identity fact — is the same; but
/// the getter/setter lives with the receiver, not the shape, and a cache
/// keyed on the ShapeId may have captured the old one. The successor's
/// generation is a pure function of (predecessor ShapeId, key), so receivers
/// replacing the same accessor from the same predecessor keep sharing a
/// shape (#10287: zod replaces a lazily installed accessor per schema).
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn transition_object_shape_accessor_replaced(
    obj: *mut crate::object::ObjectHeader,
    key_bytes: &[u8],
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    if crate::object::dictionary::is_dictionary(obj) {
        return transition_object_shape_semantics(obj);
    }
    let prev = object_shape_stamp(obj);
    let Some(current) = object_shape_descriptor(obj) else {
        return transition_object_shape_semantics(obj);
    };
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let key_hash = crate::object::key_bytes_hash(key_bytes.as_ptr(), key_bytes.len());
    // SplitMix64 over (predecessor, key, a tag no other transition uses).
    // Bit 63 keeps it disjoint from the counter namespace (which aborts far
    // below 2^62) and from the dictionary namespace (bit 62 alone).
    let mut x = key_hash ^ (u64::from(prev) << 32 | u64::from(prev)) ^ 0xACCE_5500_0000_0000;
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    let generation = x | (1 << 63);
    let id = publish_shape_result(shape_descriptor_ensure_with_holes(
        current.keys as usize as *mut ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        generation,
        store_kind::mint_kind(current.object_kind, obj),
        current.hole_count,
        current.proto_id,
        receiver_facts_of_current(obj, &current),
        None,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Mint an exact successor for a descriptor/prototype semantic transition.
/// The structural facts remain unchanged, but the process-unique generation
/// prevents a cache trained before the transition from comparing equal after
/// it. Shared siblings retain their immutable predecessor descriptor.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn transition_object_shape_semantics(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let keys = current.keys as usize as *mut ArrayHeader;
    let key_count = current.logical_key_count;
    // A dictionary receiver's identity lives in its own namespace, disjoint
    // from both ordinary ones (see `object/dictionary.rs`).
    let generation = if crate::object::dictionary::is_dictionary(obj) {
        crate::object::dictionary::next_generation()
    } else {
        SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        keys,
        key_count,
        current.live_inline_slot_count,
        generation,
        store_kind::mint_kind(current.object_kind, obj),
        current.proto_id,
        receiver_facts_of_current(obj, &current),
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Give `obj`'s current shape a ConstFn lane for every inline slot `claims`
/// selects whose value is a closure of one permanent body (the store check's
/// own test, `field_rep_store::constfn_store_info`), keeping the lanes it
/// already has. For an object whose members were installed by a path that
/// cannot carry a lane (a key claimed with its attributes, then stored), so
/// that its shape names each member's body as an ordinary key-add would have.
/// Returns whether a lane was added. Nothing happens to a dictionary, a
/// holey or deprecated layout, or when no selected slot qualifies.
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
pub(crate) unsafe fn learn_object_constfn_lanes(
    obj: *mut crate::object::ObjectHeader,
    claims: impl Fn(u32, u64) -> bool,
) -> bool {
    if obj.is_null()
        || !shape_word_is_writable(obj)
        || crate::object::dictionary::is_dictionary(obj)
    {
        return false;
    }
    let Some(current) = object_shape_descriptor(obj) else {
        return false;
    };
    if current.hole_count != 0
        || current.deprecation_targets() != (0, 0)
        || super::field_rep::has_deprecated(current.rep)
    {
        return false;
    }
    let base = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>());
    let read = |slot: u32| std::ptr::read(base.add(slot as usize * 8) as *const u64);
    let lanes = current
        .live_inline_slot_count
        .min(current.logical_key_count)
        .min(super::field_rep::REP_SLOTS);
    let mut infos: Vec<shapes_store::ConstFnSlotInfo> = Vec::new();
    let mut added = false;
    for slot in 0..lanes {
        let bits = read(slot);
        let existing = current
            .constfn_infos()
            .iter()
            .find(|i| u32::from(i.slot) == slot);
        let info = super::field_rep_store::constfn_store_info(bits);
        match (existing, info) {
            (Some(e), Some(info)) if e.info == info => infos.push(*e),
            (None, Some(info))
                if super::field_rep::slot_rep(current.rep, slot) == super::field_rep::REP_ANY
                    && claims(slot, bits) =>
            {
                infos.push(shapes_store::ConstFnSlotInfo {
                    slot: slot as u8,
                    info,
                });
                added = true;
            }
            _ => {}
        }
    }
    if !added {
        return false;
    }
    // The other lanes keep their representation; every ConstFn lane is SPECIAL.
    let mut rep = current.rep;
    for slot in 0..lanes {
        if super::field_rep::slot_rep(rep, slot) == super::field_rep::REP_SPECIAL {
            rep = super::field_rep::with_slot_rep(rep, slot, super::field_rep::REP_ANY);
        }
    }
    let rep = infos.iter().fold(rep, |rep, i| {
        super::field_rep::with_slot_rep(rep, u32::from(i.slot), super::field_rep::REP_SPECIAL)
    });
    let keys = current.keys as usize as *const ArrayHeader;
    let summary = receiver_extra_summary(obj)
        | if keys.is_null() {
            0
        } else {
            crate::object::key_attrs::keys_summary_checked(keys, current.logical_key_count)
        };
    // The receiver's private brands carry over (#11791).
    let brands = current.brands().to_vec();
    let Ok(id) = shape_descriptor_intern_with_special(
        keys,
        current.logical_key_count,
        current.live_inline_slot_count,
        current.semantic_generation,
        store_kind::mint_kind(current.object_kind, obj),
        0,
        current.proto_id,
        summary,
        rep,
        &infos,
        &brands,
        None,
    ) else {
        return false;
    };
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    true
}

/// [`transition_object_shape_semantics`] for a change that writes no slot and
/// no descriptor: marking an object as a prototype. The fresh semantic
/// generation still says "a structural change happened here" to every memo
/// keyed on the old ShapeId; what it must not do is forget a ConstFn lane the
/// object still satisfies, because a prototype whose methods are ConstFn lanes
/// is exactly what an inherited method site wants to call directly (step 5C).
///
/// A lane is carried only after re-reading its slot: the inline value must be
/// a closure whose permanent body is the lane's body
/// (`field_rep_store::constfn_store_info`, the store check's own test). F64
/// lanes are not carried (as before, the new shape's other lanes are `Any`).
/// Anything unusual (no lane survives, a dictionary, holes, a deprecated
/// record, a refused mint) takes the plain transition.
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
pub(crate) unsafe fn transition_object_shape_semantics_keeping_constfn(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    if obj.is_null()
        || !shape_word_is_writable(obj)
        || crate::object::dictionary::is_dictionary(obj)
    {
        return transition_object_shape_semantics(obj);
    }
    let Some(current) = object_shape_descriptor(obj) else {
        return transition_object_shape_semantics(obj);
    };
    if current.special_constfn_mask == 0
        || current.hole_count != 0
        || current.deprecation_targets() != (0, 0)
    {
        return transition_object_shape_semantics(obj);
    }
    let base = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>());
    let infos: Vec<shapes_store::ConstFnSlotInfo> = current
        .constfn_infos()
        .iter()
        .copied()
        .filter(|i| {
            u32::from(i.slot) < current.live_inline_slot_count
                && super::field_rep_store::constfn_store_info(std::ptr::read(
                    base.add(usize::from(i.slot) * 8) as *const u64,
                )) == Some(i.info)
        })
        .collect();
    if infos.is_empty() {
        return transition_object_shape_semantics(obj);
    }
    let rep = infos.iter().fold(super::field_rep::REP_ANY, |rep, i| {
        super::field_rep::with_slot_rep(rep, u32::from(i.slot), super::field_rep::REP_SPECIAL)
    });
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let keys = current.keys as usize as *const ArrayHeader;
    let generation = SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        shape_id_exhausted_abort();
    }
    let receiver = receiver_facts_of_current(obj, &current);
    let summary = receiver.extra_summary
        | if keys.is_null() {
            0
        } else {
            crate::object::key_attrs::keys_summary_checked(keys, current.logical_key_count)
        };
    let Ok(id) = shape_descriptor_intern_with_special(
        keys,
        current.logical_key_count,
        current.live_inline_slot_count,
        generation,
        store_kind::mint_kind(current.object_kind, obj),
        0,
        current.proto_id,
        summary,
        rep,
        &infos,
        receiver.brands.as_slice(),
        None,
    ) else {
        return transition_object_shape_semantics(obj);
    };
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// [`transition_object_shape_semantics`] for a PROTOTYPE divergence whose
/// prototype has a stable serial. Falls back to the unique-generation
/// transition, which is always correct, when there is no predecessor.
/// Move `obj` to the shape whose prototype identity is `proto_id`: the ONE
/// transition a [[Prototype]] change makes, from the link funnel
/// (`prototype_chain::object_set_static_prototype_impl`) for every link kind
/// and from the post-birth class-id rewrites.
///
/// Layout facts are carried unchanged and the prototype identity is replaced,
/// so every receiver that makes the same change from the same predecessor
/// reaches the same shape through exact-facts interning: no generation hash,
/// no per-object id. A dictionary receiver keeps its own namespace — it gets
/// a unique generation, as for every other change to it.
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn transition_object_shape_prototype(
    obj: *mut crate::object::ObjectHeader,
    proto_id: u64,
    proto_bits: u64,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    // The identity's word names the prototype before any shape names the
    // identity (`shapes_prototype`). The word is a root of every rewrite, so
    // a mint below that collects repairs it with everything else.
    shapes_prototype::write_identity_word(proto_id, proto_bits);
    if current.proto_id == proto_id {
        return object_shape_stamp(obj);
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let generation = if crate::object::dictionary::is_dictionary(obj) {
        crate::object::dictionary::next_generation()
    } else {
        current.semantic_generation
    };
    // The lanes describe the receiver's OWN slots, which a new [[Prototype]]
    // leaves as they are: carry them (normalized, as a key-add carries the
    // lanes below it), so a class instance linked to its evaluation's
    // prototype keeps its class's numeric lanes.
    let id = publish_shape_result(shape_descriptor_ensure_with_rep(
        current.keys as usize as *mut ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        generation,
        store_kind::mint_kind(current.object_kind, obj),
        current.hole_count,
        proto_id,
        receiver_facts_of_current(obj, &current),
        super::field_rep::normalized_without_special(current.rep),
        None,
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// [`transition_object_shape_prototype`] whose result the caller already
/// holds: `obj`, meta-less and still in the shape `linked` was derived from,
/// moves to `linked`, the shape whose identity `proto_id` names the
/// prototype `proto_bits`. A per-evaluation class template remembers the
/// link its instances take (`class_object_template::record_instance_link`),
/// so the evaluation's next instance moves in one stamp, without a mint.
///
/// # Safety
/// `obj` is a live `ObjectHeader` carrying `linked`'s predecessor; `linked`
/// is a present record of this agent whose identity is `proto_id`.
pub(crate) unsafe fn stamp_known_prototype_transition(
    obj: *mut crate::object::ObjectHeader,
    linked: u32,
    proto_id: u64,
    proto_bits: u64,
) {
    // The recorded link already made the identity name this prototype; an
    // identity names one object for its whole life (a collection rewrites
    // the word through forwarding), so only an emptied word is refilled.
    if shapes_prototype::identity_prototype_word(proto_id) != proto_bits {
        shapes_prototype::write_identity_word(proto_id, proto_bits);
    }
    stamp_object_shape_id_with_carrier_note(obj, linked);
    debug_assert_object_shape_parity(obj);
}

/// Is `obj` a class object (`ShapeObjectKind::Class`) whose first own key,
/// in inline slot 0, is `key`? One directory read and one key compare, no
/// descriptor copy: the question a per-evaluation class object answers for
/// its template key (`class_object_template::class_object_template_cell`).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn class_object_first_key_is(
    obj: *const crate::object::ObjectHeader,
    key: &[u8],
) -> bool {
    let Some(record) = ShapeSlab::agent_record_present(object_shape_stamp(obj)) else {
        return false;
    };
    let record = &*record;
    record.object_kind() == ShapeObjectKind::Class
        && record.logical_key_count > 0
        && record.live_inline_slot_count > 0
        && record.hole_count == 0
        && record.keys != 0
        && {
            // The record's keys are its resolved canonical list: read slot 0
            // of its dense storage directly.
            let (keys, len) = crate::object::keys_lookup::keys_array_dense_slots_resolved(
                record.keys as usize as *const ArrayHeader,
            );
            len > 0
                && crate::string::js_string_key_matches_bytes(
                    crate::value::JSValue::from_bits((*keys).to_bits()),
                    key,
                )
        }
}

/// Is `linked` the identity an instance of the class whose own identity is
/// `class_default` (`CLASS | class`) takes when it is linked to a recorded
/// prototype (`MIXED | class | serial`, a per-evaluation class's prototype)?
/// Such an instance keeps its class's own keys in its class's slots: only
/// what it inherits differs.
pub(crate) fn proto_id_links_class_instance(linked: u64, class_default: u64) -> bool {
    const TAG: u64 = 3 << PROTO_ID_TAG_SHIFT;
    class_default & TAG == PROTO_ID_CLASS
        && linked != PROTO_ID_NULL
        && linked & TAG == PROTO_ID_MIXED
        && (linked & !TAG) >> PROTO_ID_MIXED_SERIAL_BITS == class_default & !TAG
        && linked & ((1 << PROTO_ID_MIXED_SERIAL_BITS) - 1) != 0
}

/// Re-derive `obj`'s prototype identity after a write the identity is read
/// from (a post-birth `class_id` rewrite) and move it to the matching shape.
///
/// # Safety
/// `obj` is a live `ObjectHeader`, or null.
pub(crate) unsafe fn restamp_object_proto_id(obj: *mut crate::object::ObjectHeader) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) || object_shape_stamp(obj) == 0 {
        return 0;
    }
    transition_object_shape_prototype(obj, object_proto_id(obj), object_prototype_word(obj));
    // A `class_id` rewrite is also an F-A input (charter step 3, R4): a
    // prototype transition re-derives it, but an unchanged prototype
    // identity mints nothing, so re-derive explicitly.
    store_kind::restamp_object_store_kind(obj);
    object_shape_stamp(obj)
}

// ---------------------------------------------------------------------------
// [[Prototype]] identity — a SHAPE fact.
//
// A ShapeId names one prototype: every object carrying it inherits from the
// same object (or the same class-implied prototype), so a site may key what it
// learned about inherited behaviour on the receiver's shape alone.
//
//   0                      the realm's default `Object.prototype` (class id 0,
//                          a closed-literal anon shape)
//   PROTO_ID_NULL          a null [[Prototype]]
//   serial (1 .. 2^62)     a recorded prototype object: its stable serial
//                          (`proto_validity::mark_object_as_prototype`), for
//                          `setPrototypeOf`, `__proto__`, `new F()`,
//                          `Object.create`
//   CLASS | class          a compiled class instance whose prototype its class
//                          implies (generic specializations share the origin)
//   MIXED | class | serial a compiled class instance with a recorded prototype
//                          (a per-evaluation class, `setPrototypeOf` on an
//                          instance): Perry keeps class accessors in the
//                          class's vtable, so the vtable is part of what the
//                          receiver inherits
//   UNIQUE | n             a prototype with no serial (a function, array or
//                          typed array used as a prototype): one fresh
//                          identity per link, carried by lineage
// ---------------------------------------------------------------------------

/// The default prototype identity: the realm's `Object.prototype`.
pub(crate) const PROTO_ID_DEFAULT: u64 = 0;
/// A null [[Prototype]].
pub(crate) const PROTO_ID_NULL: u64 = u64::MAX;
const PROTO_ID_TAG_SHIFT: u32 = 62;
pub(crate) const PROTO_ID_CLASS: u64 = 1 << PROTO_ID_TAG_SHIFT;
pub(crate) const PROTO_ID_MIXED: u64 = 2 << PROTO_ID_TAG_SHIFT;
pub(crate) const PROTO_ID_UNIQUE: u64 = 3 << PROTO_ID_TAG_SHIFT;
/// The prototype identity of a shape that answers nothing about its receiver
/// (a dictionary-kind shape shared by many receivers): `UNIQUE | 0`, which
/// [`fresh_unique_proto_id`] never hands out (its counter starts at 1).
pub(crate) const PROTO_ID_PER_OBJECT: u64 = PROTO_ID_UNIQUE;
/// Serial bits a MIXED identity can carry beside a 32-bit class id.
const PROTO_ID_MIXED_SERIAL_BITS: u32 = 30;

static PROTO_ID_UNIQUE_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A prototype identity no other link has: for a prototype with no serial.
pub(crate) fn fresh_unique_proto_id() -> u64 {
    let n = PROTO_ID_UNIQUE_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Stay clear of PROTO_ID_NULL at the very top of the UNIQUE band. Reduce
    // modulo the band size minus one rather than masking with `...FE`: that
    // mask dropped bit 0, so serials 2k and 2k+1 collapsed onto one identity
    // and two distinct prototypes could share a shape.
    PROTO_ID_UNIQUE | (n % ((1 << PROTO_ID_TAG_SHIFT) - 1))
}

/// The class whose vtable an instance of `class_id` inherits through, or 0 for
/// a class id with none: a plain object, a closed-literal anon shape, or a
/// synthetic id (`Object.create`, an ES5 constructor), whose prototype is an
/// ordinary object.
fn vtable_class(class_id: u32) -> u32 {
    if class_id == 0
        || class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
        || crate::object::is_anon_shape_class_id(class_id)
    {
        return 0;
    }
    crate::object::class_generic_origin(class_id).unwrap_or(class_id)
}

/// The prototype identity an instance of `class_id` is BORN with, before any
/// prototype is recorded on it.
pub(crate) fn class_proto_id(class_id: u32) -> u64 {
    match vtable_class(class_id) {
        0 => PROTO_ID_DEFAULT,
        class => PROTO_ID_CLASS | u64::from(class),
    }
}

/// The stable serial of the prototype object `bits` names, or 0.
///
/// # Safety
/// `bits` are a recorded `ObjectMeta.prototype` word.
unsafe fn prototype_serial(bits: u64) -> u64 {
    let value = crate::value::JSValue::from_bits(bits);
    if !value.is_pointer() {
        return 0;
    }
    let addr = value.as_pointer::<u8>() as usize;
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
            let meta = (*(addr as *const crate::object::ObjectHeader)).meta;
            if meta.is_null() {
                0
            } else {
                (*meta).proto_serial
            }
        }
        _ => 0,
    }
}

/// The [[Prototype]] identity of an ordinary object of class `class_id` whose
/// meta record links prototype `bits` (NaN-boxed, or `TAG_NULL`): the rule
/// [`object_proto_id`] applies to a recorded prototype. `None` when that link
/// has no stable identity (a prototype with no serial, or a serial past the
/// mixed band), which `object_proto_id` answers with a fresh unique id.
///
/// # Safety
/// `bits` is a live prototype value or `TAG_NULL`.
pub(crate) unsafe fn stable_linked_proto_id(class_id: u32, bits: u64) -> Option<u64> {
    if bits == crate::value::TAG_NULL {
        return Some(PROTO_ID_NULL);
    }
    let serial = prototype_serial(bits);
    if serial == 0 {
        return None;
    }
    let class = vtable_class(class_id);
    if class == 0 {
        return Some(serial);
    }
    if serial >= 1 << PROTO_ID_MIXED_SERIAL_BITS {
        return None;
    }
    Some(PROTO_ID_MIXED | u64::from(class) << PROTO_ID_MIXED_SERIAL_BITS | serial)
}

/// `obj`'s [[Prototype]] identity, read off the object: what a mint with no
/// lineage to copy stamps into the shape. Allocation-free.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn object_proto_id(obj: *const crate::object::ObjectHeader) -> u64 {
    object_proto_id_for(obj, object_prototype_word(obj))
}

/// Does `id` name a present, keyless, generation-0, hole-free shape at
/// prototype identity `proto_id` with `slots` live inline slots — the facts
/// of a construction's birth shape (#10507's birth record)? One directory
/// read; no descriptor copy.
#[inline]
pub(crate) fn shape_is_keyless_birth(id: u32, proto_id: u64, slots: u32) -> bool {
    let Some(record) = ShapeSlab::agent_record_present(id) else {
        return false;
    };
    // SAFETY: a present record of this agent, read immediately.
    unsafe {
        (*record).proto_id == proto_id
            && (*record).logical_key_count == 0
            && (*record).semantic_generation == 0
            && (*record).hole_count == 0
            && (*record).live_inline_slot_count == slots
    }
}

/// [`shape_is_keyless_birth`] for a birth that stamps `id` on a fresh object
/// of kind `kind` in place of publishing its birth descriptor: also the
/// receiver kind, the default (empty) attribute summary and all-`Any` lanes,
/// the remaining facts that publication derives from a newborn.
#[inline]
pub(crate) fn shape_is_keyless_birth_of(
    id: u32,
    proto_id: u64,
    slots: u32,
    kind: ShapeObjectKind,
) -> bool {
    if !shape_is_keyless_birth(id, proto_id, slots) {
        return false;
    }
    let Some(record) = ShapeSlab::agent_record_present(id) else {
        return false;
    };
    // SAFETY: a present record of this agent, read immediately.
    unsafe {
        let r = &*record;
        r.object_kind() == kind && r.summary() == 0 && r.rep == 0 && r.special_constfn_mask() == 0
    }
}

/// Does `id` name a present shape a construction may stamp on a fresh,
/// unpublished object of kind `kind` and then fill slot by slot through the
/// store funnel: an ordinary layout of exactly that kind at prototype
/// identity `proto_id`, whose `count` keys are its `count` live inline
/// slots, generation 0, no tombstones, default attributes and no special
/// (ConstFn) lane? That is the construction's recorded FINAL shape (the one
/// its full sequence of transitions reached). ShapeIds are never reused, so a
/// present record still names the key list it was recorded with. One
/// directory read; no descriptor copy.
#[inline]
pub(crate) fn shape_is_filled_birth(
    id: u32,
    proto_id: u64,
    count: u32,
    kind: ShapeObjectKind,
) -> bool {
    let Some(record) = ShapeSlab::agent_record_present(id) else {
        return false;
    };
    // SAFETY: a present record of this agent, read immediately.
    unsafe {
        let r = &*record;
        r.proto_id == proto_id
            && r.object_kind() == kind
            && kind.is_ordinary_layout()
            && r.logical_key_count == count
            && r.live_inline_slot_count == count
            && r.semantic_generation == 0
            && r.hole_count == 0
            && r.summary() == 0
            && r.special_constfn_mask() == 0
            && r.keys != 0
    }
}

/// The prototype identity `obj`'s ShapeId names, read through the agent
/// directory (an unstamped or unknown id reads the default identity).
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_shape_identity(obj: *const crate::object::ObjectHeader) -> u64 {
    (*ShapeSlab::agent_record(object_shape_stamp(obj))).proto_id
}

/// The first synthetic class id (`class_registry::prototype_objects`): a
/// plain function constructor's instances.
#[cfg(test)]
pub(crate) const SYNTHETIC_CLASS_ID_BASE: u32 =
    crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE;

/// `obj`'s recorded [[Prototype]] bits, 0 when nothing is recorded (the
/// prototype is the default or the class's). A receiver that has a meta
/// record has it there (the prototype funnel writes both, and a
/// `PROTO_ID_PER_OBJECT` receiver's shape answers nothing); a meta-less
/// receiver reads it from its shape's identity word (`shapes_prototype`).
/// Only a word identity has one, and the ShapeId says which those are
/// ([`SHAPE_ID_KIND_SHIFT`]): every other receiver answers 0 (or null) from
/// its header word, with no shape-record read. Allocation-free.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
///
/// A null [[Prototype]] is the identity `PROTO_ID_NULL` and reads back as
/// `TAG_NULL` — except on a cell BORN null (`Object.create(null)`,
/// `OBJ_FLAG_NULL_PROTO`), which answers 0 as it always has: every reader
/// tests that header bit for the born-null case, and a recorded null would
/// send it down the re-prototyped-receiver paths instead.
#[inline(always)]
pub(crate) unsafe fn object_prototype_word(obj: *const crate::object::ObjectHeader) -> u64 {
    let meta = (*obj).meta;
    if !meta.is_null() && (*meta).prototype != 0 {
        return (*meta).prototype;
    }
    // A default, class or per-object identity answers 0 from the id alone,
    // and a null link from the header.
    let word = (*obj).parent_class_id;
    match shape_word_kind(word) {
        SHAPE_ID_KIND_PLAIN => 0,
        SHAPE_ID_KIND_NULL if is_shape_id(word) => null_linked_prototype_word(obj),
        _ => linked_object_prototype_word(obj),
    }
}

/// [`object_prototype_word`] of a meta-less receiver whose ShapeId names the
/// null identity: `TAG_NULL`, or 0 on a cell born null.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline(never)]
unsafe fn null_linked_prototype_word(obj: *const crate::object::ObjectHeader) -> u64 {
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(header) if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 => 0,
        _ => crate::value::TAG_NULL,
    }
}

/// [`object_prototype_word`] of a meta-less receiver whose ShapeId may name
/// a linked identity: the shape record's identity, and that identity's word.
/// Out of line, so every caller's common answer stays a few inlined compares.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline(never)]
unsafe fn linked_object_prototype_word(obj: *const crate::object::ObjectHeader) -> u64 {
    // The agent directory read: never null, an absent id reads the empty
    // record (identity 0, the default).
    let proto_id = (*ShapeSlab::agent_record(object_shape_stamp(obj))).proto_id;
    if proto_id != PROTO_ID_NULL {
        return shapes_prototype::identity_prototype_word(proto_id);
    }
    match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(header) if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 => 0,
        _ => crate::value::TAG_NULL,
    }
}

/// The prototype identity `obj` has when its recorded [[Prototype]] is
/// `recorded` (0 = none recorded): [`object_proto_id`] for a prototype being
/// linked, before any shape names it.
///
/// # Safety
/// `obj` is a live `ObjectHeader`; `recorded` is 0, `TAG_NULL` or a value's
/// bits.
pub(crate) unsafe fn object_proto_id_for(
    obj: *const crate::object::ObjectHeader,
    recorded: u64,
) -> u64 {
    // A namespace's vtable/override registry can answer before its physical
    // own slots. Project that read classification into the shape, as for
    // process.env and arguments below; an own-slot region cannot admit it.
    if (*obj).class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return PROTO_ID_PER_OBJECT;
    }
    let meta = (*obj).meta;
    // Read semantics take precedence over every prototype link, including
    // null. Changing an exotic receiver's prototype cannot turn its virtual
    // reads into physical own-slot reads.
    if !meta.is_null() && (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0
    {
        return PROTO_ID_PER_OBJECT;
    }
    // A recorded prototype outranks the born-null header bit, which is
    // sticky: `Object.setPrototypeOf(Object.create(null), p)` keeps the bit
    // and its shape must still name `p`.
    if recorded == 0 {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) {
            if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                return PROTO_ID_NULL;
            }
        }
    }
    let class_id = (*obj).class_id;
    let class = vtable_class(class_id);
    if recorded != 0 {
        // A compiled class instance linked to its own class's declaration
        // prototype (runtime wiring of a native-base subclass instance) has
        // exactly the prototype its class implies: the class identity, so it
        // shares its class's shapes and its class surface stays exact. A
        // declaration prototype is an object of its own class
        // (`class_decl_prototype_value`), so only such a prototype can be
        // it: the registry is asked only then.
        if class != 0 && bits_name_object_of_class(recorded, class) {
            let decl = crate::object::class_registry::class_decl_prototype_object(class);
            if !decl.is_null() && crate::value::js_nanbox_pointer(decl as i64).to_bits() == recorded
            {
                return PROTO_ID_CLASS | u64::from(class);
            }
        }
        if let Some(id) = stable_linked_proto_id(class_id, recorded) {
            return id;
        }
        // A prototype with no stable identity gets one per link, carried by
        // lineage: the receiver's own shape already has one for these bits
        // when they are the ones its word holds.
        let stamp = object_shape_stamp(obj);
        let current = shape_proto_id(stamp).unwrap_or(PROTO_ID_DEFAULT);
        if class == 0
            && current & PROTO_ID_UNIQUE == PROTO_ID_UNIQUE
            && proto_id_carries_word(current)
            && shape_prototype_word(stamp) == recorded
        {
            return current;
        }
        return fresh_unique_proto_id();
    }
    if class != 0 {
        return PROTO_ID_CLASS | u64::from(class);
    }
    if class_id >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE {
        // `Object.create(p)`: the prototype lives in the synthetic class's
        // registry slot, fixed for that id's life.
        let proto = crate::object::class_prototype_object(class_id);
        if !proto.is_null() {
            let serial = prototype_serial(crate::value::js_nanbox_pointer(proto as i64).to_bits());
            if serial != 0 {
                return serial;
            }
        }
        return PROTO_ID_CLASS | u64::from(class_id);
    }
    PROTO_ID_DEFAULT
}

/// Do `bits` name a live ordinary object whose class id is `class`?
///
/// # Safety
/// `bits` are a recorded [[Prototype]] word.
#[inline]
unsafe fn bits_name_object_of_class(bits: u64, class: u32) -> bool {
    let value = crate::value::JSValue::from_bits(bits);
    if !value.is_pointer() {
        return false;
    }
    let addr = value.as_pointer::<u8>() as usize;
    matches!(
        crate::value::addr_class::try_read_gc_header(addr),
        Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT
            && (*(addr as *const crate::object::ObjectHeader)).class_id == class
    )
}

/// The prototype identity recorded in shape `id`, or `None` for an id with no
/// descriptor. One slab read; no descriptor copy.
#[inline]
pub(crate) fn shape_proto_id(id: u32) -> Option<u64> {
    let table = &crate::state::state().shapes;
    let record = table.slab().record_ptr(id)?;
    // SAFETY: live slab record, read immediately on this agent.
    Some(unsafe { (*record).proto_id })
}

/// Turn a class-expression object into a class receiver. The kind is part of
/// the exact immutable descriptor, so it cannot alias GC layout bits and every
/// pre-mark ShapeId guard permanently misses afterward.
///
/// Unlike a general semantic transition, changing `object_kind` already makes
/// the descriptor facts distinct. Preserve the predecessor generation so
/// repeated evaluations of the same class expression reuse one class-shaped
/// descriptor. Minting a fresh generation here retained one descriptor per
/// evaluation as long as their shared keys array stayed live (one million
/// evaluations consumed hundreds of MB).
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn transition_object_shape_to_class(
    obj: *mut crate::object::ObjectHeader,
) -> u32 {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return 0;
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    if current.object_kind == ShapeObjectKind::Class {
        return object_shape_stamp(obj);
    }
    let id = publish_shape_result(shape_descriptor_ensure_with_generation(
        current.keys as usize as *const ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        current.semantic_generation,
        ShapeObjectKind::Class,
        current.proto_id,
        receiver_facts_of_current(obj, &current),
    ));
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Add private brand `brand` (#11791) to `obj`: move it to the shape with
/// the same facts and `brand` in its brand list. `false` when `obj` already
/// carries the brand (PrivateMethodOrAccessorAdd's "twice" error is the
/// caller's) or is not a shaped object; nothing changes then.
///
/// A class brands its instance after the heritage's constructor returned, so
/// this is a transition, never a birth fact. Every later transition reads the
/// receiver's brands back off its shape (`receiver_facts`), so the brand is
/// carried by every shape the receiver moves to.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[cfg_attr(feature = "shape-mint-diag", track_caller)]
pub(crate) unsafe fn transition_object_shape_add_brand(
    obj: *mut crate::object::ObjectHeader,
    brand: u64,
) -> bool {
    if obj.is_null() || !shape_word_is_writable(obj) {
        return false;
    }
    let current = object_shape_descriptor(obj).unwrap_or_else(|| {
        synchronize_object_shape_descriptor(obj);
        object_shape_descriptor(obj).expect("shape synchronization must publish a descriptor")
    });
    let mut receiver = receiver_facts_of_current(obj, &current);
    if !receiver.brands.insert(brand) {
        return false;
    }
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let kind = store_kind::mint_kind(current.object_kind, obj);
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj);
    // Mint-then-stamp: the mint can collect, so the receiver is re-resolved
    // through its handle before the stamp.
    let (id, obj) = handle.across_mut::<crate::object::ObjectHeader, _>(|| {
        publish_shape_result(shape_descriptor_ensure_with_rep(
            current.keys as usize as *const ArrayHeader,
            current.logical_key_count,
            current.live_inline_slot_count,
            current.semantic_generation,
            kind,
            current.hole_count,
            current.proto_id,
            receiver,
            // The keys and slots are unchanged, so the representation carries.
            super::field_rep::normalized_without_special(current.rep),
            None,
        ))
    });
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    true
}

/// Does `obj`'s shape carry private brand `brand` (#11791)?
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn object_has_brand(obj: *const crate::object::ObjectHeader, brand: u64) -> bool {
    shape_brands_by_id(object_shape_stamp(obj))
        .is_some_and(|brands| brands.binary_search(&brand).is_ok())
}

/// A brand is never removed from a receiver: every shape an object moves to
/// carries the brands of the one it leaves (#11791). Checked at the one stamp
/// funnel, so a mint site that drops `receiver_facts` fails here.
#[inline]
fn debug_assert_brands_carried(previous: u32, next: u32) {
    if cfg!(debug_assertions) && previous != next && is_shape_id(previous) {
        let (Some(before), Some(after)) = (shape_brands_by_id(previous), shape_brands_by_id(next))
        else {
            return;
        };
        debug_assert!(
            before
                .iter()
                .all(|brand| after.binary_search(brand).is_ok()),
            "shape {next:#x} dropped a private brand of {previous:#x}: {before:?} -> {after:?}"
        );
    }
}

/// Authoritative descriptor for a genuine shaped object.
#[inline]
pub(crate) unsafe fn object_shape_descriptor(
    obj: *const crate::object::ObjectHeader,
) -> Option<ShapeDescriptor> {
    shape_descriptor_by_id(object_shape_stamp(obj))
}

/// [`object_shape_descriptor`]'s record, borrowed in place (#10362).
#[inline]
pub(crate) unsafe fn object_shape_record(
    obj: *const crate::object::ObjectHeader,
) -> Option<ShapeRecordRef> {
    shape_record_by_id(object_shape_stamp(obj))
}

#[inline]
pub(crate) unsafe fn object_shape_id(obj: *const crate::object::ObjectHeader) -> u32 {
    object_shape_descriptor(obj)
        .map(|_| object_shape_stamp(obj))
        .unwrap_or(0)
}

/// Retire `id` from the by-id store and the family index (#9706).
///
/// The family index is keyed by the address the descriptor was indexed under.
/// Between a live receiver rewriting the record's `keys` word and the
/// metadata scan moving the family, the two can name different addresses; a
/// removal in that window leaves the id in the stale family, where every
/// walk skips it (`record_ptr` is `None`) and the next scan drops it.
fn remove_descriptor_and_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    let table = &crate::state::state().shapes;
    let Some(indexed) = table.slab().get(id).map(|record| record.keys) else {
        return;
    };
    remove_descriptor_indexed_under(inner, id, indexed);
}

/// [`remove_descriptor_and_reverse_indices`] for a caller that knows the
/// address the id is indexed under — the metadata scan, which retires a
/// family whose keys address was recycled while a live edge may already have
/// rewritten the records to the forwarded address.
fn remove_descriptor_indexed_under(inner: &mut ShapeTableInner, id: u32, indexed: u64) {
    let table = &crate::state::state().shapes;
    // SAFETY: no slab reference is held by the caller across this call.
    let Some(record) = (unsafe { table.slab_mut().remove(id) }) else {
        return;
    };
    #[cfg(feature = "shape-mint-diag")]
    crate::object::shape_mint_census::note_retire(id);
    retire_cached_shape_object_kind(id);
    if record.has(RECORD_FLAG_FACTS_INDEXED) {
        inner.facts_remove(record.facts_key_with_keys(indexed), id);
    }
    inner.family_remove(indexed, id);
    // Unlike the two rekey paths, retirement does not transfer this record.
    // SAFETY: slab removal returned the unique owner of the extension.
    unsafe { record.release_extras() };
}

/// Exact-facts test for a candidate id against the receiver's authoritative
/// header facts. #8113: the live bound is a PARAMETER — the header no longer
/// mirrors it, so the caller supplies the bound it is claiming.
fn descriptor_matches_object(
    shape_id: u32,
    obj: *const crate::object::ObjectHeader,
    live_inline_slot_count: u32,
) -> bool {
    let Some(d) = shape_descriptor_by_id(shape_id) else {
        return false;
    };
    unsafe {
        let keys = crate::object::object_keys(obj);
        d.keys == keys.arr() as u64
            && d.logical_key_count == keys.count()
            && d.live_inline_slot_count == live_inline_slot_count
            && d.proto_id == object_proto_id(obj)
            // Charter step 3 (R3): a supplied id is the receiver's only if
            // it names the receiver's store facts too.
            && d.object_kind == store_kind::receiver_ordinary_kind(obj)
    }
}

/// #8113: the live-slot bound is no longer independently observable, so parity
/// is now exactly "the stamp resolves, and its structural keys facts match the
/// keys edge the receiver is about to carry". The bound cannot disagree with
/// itself.
#[inline]
pub(crate) unsafe fn debug_assert_object_shape_parity(obj: *const crate::object::ObjectHeader) {
    // The facts only feed a `debug_assert!`, but the descriptor probe and the
    // out-of-line keys-length read are not provably pure to LLVM, so without
    // this gate release builds executed both on every object birth and shape
    // publish (~80 instructions per `new C()`).
    if cfg!(debug_assertions) {
        debug_assert_object_shape_parity_for_keys(obj, crate::object::object_keys(obj));
    }
}

/// Parity against an EXPLICIT keys edge.
///
/// `publish_object_shape_from` stamps the successor before the header store
/// (that is what makes the keys mutation mint-then-stamp), so for that one
/// window the authoritative edge is the caller's argument, not the header word.
#[inline]
pub(crate) unsafe fn debug_assert_object_shape_parity_for_keys(
    obj: *const crate::object::ObjectHeader,
    keys: crate::object::ObjectKeys,
) {
    if !cfg!(debug_assertions) {
        return;
    }
    // #10868 step 2.5 stage 1: a dictionary-mode receiver deliberately
    // publishes NO keys while `object_keys_array` answers with the private
    // list in its `ObjectMeta`, so the comparison below is false by
    // construction for it. Its invariant is stricter, and lives with the mode
    // that owns it.
    if crate::object::dictionary::is_dictionary(obj) {
        crate::object::dictionary::debug_assert_dictionary_parity(obj);
        return;
    }
    let id = object_shape_stamp(obj);
    if id != 0 {
        debug_assert!(
            shape_descriptor_by_id(id).is_some_and(|d| {
                d.keys == keys.arr() as u64 && d.logical_key_count == keys.count()
            }),
            "published ShapeId disagrees with authoritative ObjectHeader facts"
        );
        // The count names a prefix of the array; it never reaches past it.
        debug_assert!(
            keys.is_null()
                || keys.count() as usize
                    <= crate::array::keys_array_len_capped_to_capacity(keys.arr()),
            "a receiver's key count runs past its keys array"
        );
    }
}

/// Drop the stamp iff the word currently holds one, leaving a real
/// `parent_class_id` untouched. Returns true when a stamp was cleared.
///
/// # TEST-ONLY since #8113
///
/// Production code must never clear a stamp. The descriptor is now the sole
/// record of the live inline-slot bound, so an unstamped receiver reports a
/// bound of ZERO — its payload stops being traced, rewritten, and writable.
/// Every mutation that used to clear-then-re-mint is mint-then-stamp instead
/// (`publish_object_live_slot_count`, `publish_object_shape_from`), which has no
/// window at all. This survives only so tests can MANUFACTURE the unstamped
/// state and assert what the runtime does with it.
#[cfg(test)]
#[inline]
pub(crate) unsafe fn clear_object_shape_stamp(obj: *mut crate::object::ObjectHeader) -> bool {
    if is_shape_id((*obj).parent_class_id) {
        (*obj).parent_class_id = 0;
        true
    } else {
        false
    }
}

/// A shape-shared keys array's prefix never changes: the only in-place write
/// any such array sees is a canonical backing's tip append, past every
/// published count (`canonical_keys::append_at_tip`); every other writer
/// copies first. So one index serves every list on it.
unsafe fn keys_prefix_is_immutable(keys: *const ArrayHeader) -> bool {
    crate::value::addr_class::try_read_gc_header(keys as usize).is_some_and(|gc| {
        gc.obj_type == crate::gc::GC_TYPE_ARRAY
            && gc.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
    })
}

/// Build (or extend) the slot map for `keys` covering `key_count` keys.
unsafe fn index_range(shape: &mut ShapeIndex, keys: *const ArrayHeader, key_count: u32) {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for i in shape.indexed_len..key_count.min(slot_len as u32) {
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(b) = crate::string::js_string_key_bytes(v, &mut sso) {
            let h = super::key_bytes_hash(b.as_ptr(), b.len());
            shape.slots.push(h, i);
        }
    }
    shape.indexed_len = key_count;
}

/// Look up `key_bytes` in the shape of `keys`. Returns a slot whose stored
/// key has been re-validated against `key_bytes`; `None` means "not found
/// via the shape" (caller falls back to its linear scan / append path).
///
/// `build` gates first-time index construction (callers keep their
/// historical thresholds: write path ≥ `KEYS_INDEX_THRESHOLD`, read path
/// ≥ `WIDE_KEY_INDEX_MIN_KEYS`) — but an entry that already exists is
/// consulted regardless, so a read may reuse the index a write built.
/// A key-index consultation's answer, distinguishing "this COMPLETE index
/// proves the key absent" from "the index cannot answer".
pub(crate) enum KeysIndexVerdict {
    Found(u32),
    /// The index covers every slot of the array (`indexed_len == key_count`)
    /// and holds no entry for this key: the key is not present, and the
    /// caller may skip its linear backstop scan. Trusting absence is what
    /// makes tombstone-delete churn O(1) — the re-add's find-before-append
    /// otherwise pays a full scan per delete, measured at 60.4% of the
    /// flag-on `bench_populated_delete` profile.
    Absent,
    /// No index, a partial build, or a declined consult — scan.
    Unindexed,
}

pub(crate) unsafe fn shape_slot_lookup(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> Option<u32> {
    match shape_slot_lookup_verdict(keys, key_bytes, key_hash, key_count, build) {
        KeysIndexVerdict::Found(slot) => Some(slot),
        _ => None,
    }
}

pub(crate) unsafe fn shape_slot_lookup_verdict(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> KeysIndexVerdict {
    let keys_id = keys as usize;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let shape = match inner.indices.get_mut(&keys_id) {
        Some(s) => {
            if s.indexed_len > key_count && !keys_prefix_is_immutable(keys) {
                // An owned array that shrank (delete/compaction): slots are
                // untrustworthy.
                inner.indices.remove(&keys_id);
                return KeysIndexVerdict::Unindexed;
            }
            // A canonical backing's index covers its longest list, and a
            // shorter list on it answers from the same index: every candidate
            // below is kept only if its slot is below THIS list's count, and
            // is content-validated, so a slot of a longer list can only miss.
            s
        }
        None => {
            if !build {
                return KeysIndexVerdict::Unindexed;
            }
            inner.note_young_keys(keys_id as u64);
            inner.indices.entry(keys_id).or_insert(ShapeIndex {
                indexed_len: 0,
                slots: SlotIndex::new(),
            })
        }
    };
    if shape.indexed_len < key_count {
        index_range(shape, keys, key_count);
    }
    // Complete for THIS list when it covers at least its `key_count` slots:
    // a shorter list on a canonical backing is a prefix of what was indexed.
    let complete = shape.indexed_len >= key_count;
    let absent = if complete {
        KeysIndexVerdict::Absent
    } else {
        KeysIndexVerdict::Unindexed
    };
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    // #10595: keep scanning past the first content match and keep the
    // HIGHEST slot index among them, not the probe order's first hit.
    // A field name that a subclass re-declares (`class Sub extends Base {
    // tag = ... }` where `Base` also declares `tag`) is NOT deduplicated in
    // the packed keys — `codegen/mod.rs` lists ancestor fields first, then
    // the class's own, so a genuine duplicate always has the most-derived
    // declaration at the HIGHER slot index, regardless of this table's probe
    // order (which is insertion order for a fresh table, but open-addressing
    // growth/rehash can reshuffle it). `class_field_global_index` — the
    // compile-time-typed read's index resolver — already picks the
    // most-derived declaration ("TS shadowing"); this dynamic by-name lookup
    // must agree, or a receiver whose static type is unknown (an inherited
    // accessor's `this.field`, a computed `obj[key]`) sees the ancestor's
    // stale slot instead of the override. A name with only one candidate
    // (the common, non-shadowing case) is unaffected.
    let mut found: Option<u32> = None;
    for i in shape.slots.candidates(key_hash) {
        if (i as usize) >= slot_len || i >= key_count {
            continue;
        }
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                found = Some(found.map_or(i, |prev| prev.max(i)));
            }
        }
    }
    if let Some(i) = found {
        return KeysIndexVerdict::Found(i);
    }
    // Hash-bucket candidates existed but none matched: with a complete index
    // that still proves absence (the bucket held colliding OTHER keys).
    absent
}

/// Total slots covered by every live slot index (test instrumentation).
#[cfg(test)]
pub(crate) fn indexed_slots_for_test() -> u64 {
    let inner = crate::state::state().shapes.inner.borrow();
    inner
        .indices
        .values()
        .map(|ix| u64::from(ix.indexed_len))
        .sum()
}

/// Record a freshly appended key: `keys` (the POST-append array — a clone
/// or grow-realloc lands under its new identity, or nowhere if no entry
/// exists yet) grew to `new_count` with `key_hash` at `slot`.
pub(crate) fn shape_note_append(
    keys: *const ArrayHeader,
    new_count: u32,
    key_hash: u64,
    slot: u32,
) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.get_mut(&(keys as usize)) {
        if shape.indexed_len + 1 == new_count {
            shape.indexed_len = new_count;
            shape.slots.push(key_hash, slot);
        }
    }
}

/// Back-fill a linear-scan hit (no-op when the shape has no entry — the
/// next lookup_ways builds it wholesale at the caller's threshold).
pub(crate) fn shape_note_hit(keys: *const ArrayHeader, key_hash: u64, slot: u32) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.get_mut(&(keys as usize)) {
        shape.slots.push(key_hash, slot);
    }
}

/// An OWNED (non-`GC_FLAG_SHAPE_SHARED`) keys array was reallocated while
/// `js_array_push` appended a key. Migrate only the validated slot-index
/// accelerator so it survives capacity growth. The weak old descriptor is not
/// eagerly deleted: even if a release-only invariant regression left a sibling
/// naming it, that sibling must continue to resolve. Post-trace dead-key
/// pruning retires it once no live owner reaches the old array.
///
/// Callers must pass the OWNED-grow pair only: a shared array's fork is a
/// genuine transition (the clone starts a NEW identity and the old address
/// still describes the siblings' live shape — migrating it would corrupt
/// them). Safety net: a wrong or stale migration cannot produce wrong
/// results — every hit re-validates key bytes against the live array —
/// it only wastes the rebuild this exists to save.
pub(crate) fn shape_keys_grown(old_keys: usize, new_keys: *const ArrayHeader) {
    let new_id = new_keys as usize;
    if old_keys == 0 || new_id == 0 || old_keys == new_id {
        return;
    }
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(shape) = inner.indices.remove(&old_keys) {
        inner.note_young_keys(new_id as u64);
        inner.indices.insert(new_id, shape);
    }
}

/// Drop only the validated slot-index accelerator for a keys array that was
/// compacted/retired (delete path). Descriptors are weak and exact-fact gated,
/// but are not eagerly removed: another live sibling may still name one. The
/// post-trace dead-key fan-out retires them when the array is actually dead.
pub(crate) fn shape_drop(keys: *const ArrayHeader) {
    let keys = keys as usize;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    inner.indices.remove(&keys);
}

/// True when a shape's keys address is currently occupied by another GC type.
/// A tracked non-array allocation proves that the original keys array died and
/// its address was recycled; unreadable/off-arena addresses remain governed by
/// the collector's ordinary dead-owner predicate.
#[inline]
fn shape_keys_address_is_recycled(addr: usize) -> bool {
    #[cfg(test)]
    if RECYCLED_KEYS_CHECK_SUPPRESSED.with(std::cell::Cell::get) {
        return false;
    }

    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(addr).is_some_and(|header| {
            let obj_type = (*header.as_ptr()).obj_type;
            obj_type != crate::gc::GC_TYPE_ARRAY && obj_type != crate::gc::GC_TYPE_LAZY_ARRAY
        })
    }
}

/// Retire descriptors no live receiver carried during the just-completed
/// synchronous full trace and no runtime metadata owner can reinstall.
///
/// The caller must run this while the full trace's `CARRIED_SEEN` notes are
/// intact and only after cache-carrier bits have been rebuilt from live table
/// occupancy. Minor and budgeted cycles are deliberately ineligible: neither
/// provides an exact, stop-the-world enumeration of every live receiver.
pub(crate) fn prune_uncarried_shape_descriptors_after_full_trace() {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let mut stale = Vec::new();
    table.slab().for_each(|id, record| {
        // SAFETY: live slab record, read immediately under agent ownership.
        let record = unsafe { &*record };
        // #10905: a keyless birth shape an allocation consulted this epoch
        // keeps the width it learned, though its births sit on wider shapes.
        if !record.has(RECORD_FLAG_CARRIED_SEEN | RECORD_FLAG_BIRTH_OWNER)
            && !record.cache_carrier()
        {
            stale.push(id);
        }
    });
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// Post-trace weak-table prune: drop slot indices and by-id descriptors whose
/// keys array is dead. A live object has already traced its authoritative
/// header edge and synchronized the descriptor named by its ShapeId, so a
/// descriptor removed here cannot be named by a live object. Correctness fails
/// closed on a missing lookup_ways, independently of pruning.
pub(crate) fn prune_dead_shape_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    // A shape keys entry is keyed by the address of its keys array — a
    // `GC_TYPE_ARRAY` (or `GC_TYPE_LAZY_ARRAY`). When the keys array dies
    // and the arena recycles its address for a different object type
    // (closure, string, …), the `is_dead_owner` predicate sees the NEW
    // object's flags (MARKED/FORWARDED) and reports the address as alive,
    // leaving a stale entry that makes property lookups on objects whose
    // descriptor still points at the old address read the wrong shape.
    // Guard the retain with a type check: if the object at the key address
    // is not an array/lazy-array, the keys array is dead regardless of what
    // `is_dead_owner` says about the recycled tenant.
    if !inner.indices.is_empty() {
        inner.indices.retain(|keys_id, _| {
            !is_dead_owner(*keys_id) && !shape_keys_address_is_recycled(*keys_id)
        });
    }
    let mut stale: Vec<u32> = Vec::new();
    table.slab().for_each(|id, record| {
        // SAFETY: live slab record, read immediately.
        let descriptor = unsafe { *record };
        let keys = descriptor.keys as usize;
        if is_dead_owner(descriptor.keys as usize) || shape_keys_address_is_recycled(keys) {
            stale.push(id);
        }
    });
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// [`prune_dead_shape_keys`] for a MINOR (#9754): only a young keys array can
/// die, and a young keys address is always in the young log (noted at
/// insert, re-logged by every minor-scoped walk while it stays young), so the
/// log is the complete candidate set.
pub(crate) fn prune_dead_shape_keys_young(is_dead_owner: &dyn Fn(usize) -> bool) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let candidates = inner.young_keys.take_sorted();
    let mut kept = Vec::with_capacity(candidates.len());
    for keys in candidates {
        let addr = keys as usize;
        if !is_dead_owner(addr) && !shape_keys_address_is_recycled(addr) {
            kept.push(keys);
            continue;
        }
        inner.indices.remove(&addr);
        let ids: Vec<u32> = inner
            .families
            .get(&keys)
            .map(|ids| ids.as_slice().to_vec())
            .unwrap_or_default();
        for id in ids {
            remove_descriptor_indexed_under(&mut inner, id, keys);
        }
    }
    inner.young_keys.extend(kept);
}

/// Metadata-only forwarding repair for the weak descriptor table and
/// pointer-keyed slot indices. Mark/copy mode does not root anything; live
/// object scans provide descriptor reachability, and post-copy rewrite follows
/// only forwarding records those live edges already created.
///
/// #9706: the walk is per keys-array FAMILY, not per descriptor. Every
/// descriptor of a family shares one keys address, so one probe answers for
/// all of them — the per-address memo the descriptor walk used to keep
/// (`PROBE_MEMO`, a persistent map sized to every distinct address in the
/// table) is now simply the family index itself. A family is probed with the
/// MARKING visit when any of its descriptors is a carrier, which is exactly
/// the rooting duty the #8112 gate assigns: the keys array must survive while
/// an old receiver or a cache still names one of its shapes.
pub(crate) fn scan_shape_table_rekey_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let carrier_notes = SHAPE_CARRIER_YOUNG_KEYS.with(|log| log.borrow_mut().take_sorted());
    inner.young_keys.extend(carrier_notes);
    let rewrite_phase = visitor.is_metadata_rewrite_phase();
    // #9754: a minor-scoped pass visits only the young-logged keys addresses;
    // the full walk below rebuilds the log from what it finds.
    if visitor.young_scope() {
        scan_shape_table_young(visitor, table, &mut inner, rewrite_phase);
        return;
    }
    let table_len = (inner.families.len() + inner.indices.len()) as u64;
    let mut moved_families: Vec<(u64, u64)> = Vec::new();
    let mut dead_descriptor_ids: Vec<(u32, u64)> = Vec::new();
    // The shared slab view is scoped to the probe loop: retirement below
    // takes the slab mutably, and nothing after the loop may still hold it.
    let slab = table.slab();
    for (&indexed, ids) in inner.families.iter() {
        if indexed == 0 {
            // Keyless shapes hold no edge.
            continue;
        }
        // #8112 ephemeron gate. A shape with an OLD carrier is rooted here:
        // the minor that has to keep its keys array alive never enumerates the
        // object that carries it. A shape with only young carriers is NOT —
        // those receivers are traced, and each one emits the edge itself, so
        // rooting them from the table would make every keys array ever minted
        // immortal and turn `prune_dead_shape_keys`'s "is the keys array
        // dead?" into a question it asks of itself.
        //
        // One descriptor stands for the family: a carrier if the family has
        // one (its duty is the strongest), else any present member.
        let mut descriptor: Option<ShapeDescriptor> = None;
        for &id in ids.as_slice() {
            if let Some(lifted) = slab.lift(id) {
                if lifted.old_carrier || lifted.cache_carrier {
                    descriptor = Some(lifted);
                    break;
                }
                descriptor.get_or_insert(lifted);
            }
        }
        let Some(descriptor) = descriptor else {
            // Every id retired under a stale address; the family is empty.
            moved_families.push((indexed, 0));
            continue;
        };
        let mut addr = indexed as usize;
        // The census gate (`scripts/shape_descriptor_census.py`) pins this
        // exact two-armed expression so that a sabotage which widens the gate
        // or swaps the arms is red, and its own self-test sabotages this very
        // literal.
        let moved = if descriptor.old_carrier || descriptor.cache_carrier {
            visitor.visit_usize_slot(&mut addr)
        } else {
            visitor.visit_metadata_usize_slot(&mut addr)
        };
        // Validate the POST-visit address. A stale shape key can follow the
        // forwarding record of the non-array tenant that recycled its address;
        // checking only an unmoved old address misses that case.
        if rewrite_phase && shape_keys_address_is_recycled(addr) {
            dead_descriptor_ids.extend(ids.as_slice().iter().map(|&id| (id, indexed)));
            continue;
        }
        if moved {
            for &id in ids.as_slice() {
                if let Some(record) = slab.record_ptr(id) {
                    // SAFETY: live slab record, single-threaded agent. A live
                    // receiver's edge may already have written the same
                    // forwarded address here; the store is idempotent.
                    unsafe { (*record).keys = addr as u64 };
                }
            }
        }
        if addr as u64 != indexed {
            moved_families.push((indexed, addr as u64));
        }
    }
    for (id, indexed) in dead_descriptor_ids {
        remove_descriptor_indexed_under(&mut inner, id, indexed);
    }
    for (old, new) in moved_families {
        move_shape_family(table, &mut inner, old, new);
    }

    if rewrite_phase && !inner.indices.is_empty() {
        let moved: Vec<(usize, usize)> = inner
            .indices
            .keys()
            .filter_map(|&keys_id| {
                let mut addr = keys_id;
                visitor.visit_metadata_usize_slot(&mut addr);
                (addr != keys_id).then_some((keys_id, addr))
            })
            .collect();
        for (old, new) in moved {
            if let Some(shape) = inner.indices.remove(&old) {
                inner.indices.insert(new, shape);
            }
        }
        // Drop indices entries whose keys-array address was recycled: the
        // forwarding record at the old address points to a DIFFERENT object
        // (not a keys array), so `visit_metadata_usize_slot` either rekeyed
        // it to the wrong address (caught above by the type mismatch on the
        // new address) or returned false because the forwarding walk could
        // not classify the address. Either way the keys array is dead; remove
        // the stale entry so property lookups don't resolve the wrong shape.
        let recycled: Vec<usize> = inner
            .indices
            .keys()
            .filter(|&&keys_id| shape_keys_address_is_recycled(keys_id))
            .copied()
            .collect();
        for old in recycled {
            inner.indices.remove(&old);
        }
    }

    // A full walk is authoritative: rebuild the young log from the tables.
    let kept = relevant_shape_keys(table, &inner);
    let kept_len = kept.len() as u64;
    let _ = inner.young_keys.take_sorted();
    inner.young_keys.extend(kept);
    crate::gc::young_log::note_walk(
        SHAPE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: false,
            logged: table_len,
            visited: table_len,
            kept: kept_len,
            table_len,
        },
    );
}

/// Re-index a family that the collector moved from `old` to `new` (`new ==
/// 0`: every member retired, drop it).
fn move_shape_family(table: &ShapeTable, inner: &mut ShapeTableInner, old: u64, new: u64) {
    let Some(ids) = inner.families.remove(&old) else {
        return;
    };
    if new == 0 {
        return;
    }
    for &id in ids.as_slice() {
        let Some(record) = table.slab().get(id) else {
            continue;
        };
        // The accelerator was keyed with the OLD address; the other five
        // facts never change under the collector.
        if record.has(RECORD_FLAG_FACTS_INDEXED) {
            inner.facts_remove(record.facts_key_with_keys(old), id);
            inner.facts_push_back(record.facts_key_with_keys(new), id);
        }
        // Scanner-internal rekey: the caller keeps `new` from its post-visit
        // relevance result (or the full walk rebuilds the log). Re-entering
        // the writer funnel here would enqueue the same family mid-walk and
        // price it twice in one minor.
        inner.families.entry(new).or_default().push_back(id);
    }
}

/// Every keys address a minor can act on, re-derived from the authoritative
/// tables (families and slot indices whose keys array is not old).
fn relevant_shape_keys(table: &ShapeTable, inner: &ShapeTableInner) -> Vec<u64> {
    let mut relevant: Vec<u64> = inner.families.keys().copied().collect();
    relevant.extend(inner.indices.keys().copied().map(|keys| keys as u64));
    relevant.sort_unstable();
    relevant.dedup();
    relevant.retain(|&keys| shape_keys_entry_is_minor_relevant(table, inner, keys));
    relevant
}

/// Exact minor-work predicate for one shape-table key.
///
/// Nursery addresses must be rekeyed even for weak metadata entries. Malloc
/// arrays must be rooted when a carrier owns the family. A Longlived keys
/// array never moves or dies, so it matters only while a rooted family exposes
/// a collectible property-key leaf from its payload. Property keys are
/// strings/symbol headers and both are GC leaves; tracing through an immortal
/// key cannot discover a younger grandchild.
fn shape_keys_entry_is_minor_relevant(
    table: &ShapeTable,
    inner: &ShapeTableInner,
    keys: u64,
) -> bool {
    if keys == 0 {
        return false;
    }
    let addr = keys as usize;
    match crate::arena::classify_heap_space(addr) {
        crate::arena::HeapSpace::NurseryEden
        | crate::arena::HeapSpace::Survivor0
        | crate::arena::HeapSpace::Survivor1
        | crate::arena::HeapSpace::PromotedYoung => return true,
        crate::arena::HeapSpace::Old => return false,
        crate::arena::HeapSpace::Unknown => {
            return family_has_root_carrier(table, inner, keys)
                && crate::gc::young_log::addr_is_minor_collectible(addr);
        }
        crate::arena::HeapSpace::Longlived => {}
    }
    if !family_has_root_carrier(table, inner, keys) {
        return false;
    }
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(addr) else {
            return false;
        };
        if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
            return false;
        }
        let (slots, len) = super::keys_array_dense_slots(addr as *const ArrayHeader);
        (0..len).any(|index| {
            crate::gc::young_log::bits_are_minor_collectible((*slots.add(index)).to_bits())
        })
    }
}

fn family_has_root_carrier(table: &ShapeTable, inner: &ShapeTableInner, keys: u64) -> bool {
    inner.families.get(&keys).is_some_and(|ids| {
        ids.as_slice().iter().any(|&id| {
            table
                .slab()
                .get(id)
                .is_some_and(|record| record.has(RECORD_FLAG_OLD_CARRIER) || record.cache_carrier())
        })
    })
}

/// The minor-scoped walk (#9754): only the young-logged keys addresses, each
/// visited exactly as the full walk visits it — the family's carrier gate,
/// the record rewrite, the recycled-address retirement, the slot-index
/// re-key — and re-logged iff the keys array is still not old afterwards.
fn scan_shape_table_young(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    table: &ShapeTable,
    inner: &mut ShapeTableInner,
    rewrite_phase: bool,
) {
    let table_len = (inner.families.len() + inner.indices.len()) as u64;
    #[cfg(any(debug_assertions, test))]
    {
        let relevant = relevant_shape_keys(table, inner);
        inner
            .young_keys
            .debug_assert_logged(SHAPE_YOUNG_LOG_NAME, &relevant);
    }
    let mut logged = 0u64;
    let mut visited = 0u64;
    let mut kept = inner.young_keys.take_spare();
    loop {
        let batch = inner.young_keys.take_sorted();
        if batch.is_empty() {
            break;
        }
        logged += batch.len() as u64;
        for keys in batch {
            if keys == 0 {
                continue;
            }
            visited += 1;
            let (post, relevant) =
                scan_shape_keys_address(visitor, table, inner, rewrite_phase, keys);
            if relevant {
                kept.push(post);
            }
            // The family moves in the MARK pass (a carrier's `visit_usize_slot`
            // copies the keys array) while the slot index is re-keyed only in
            // the REWRITE pass, so between the two the index still sits under
            // the from-space address: keep that key logged as well.
            if post != keys && inner.indices.contains_key(&(keys as usize)) {
                kept.push(keys);
            }
        }
    }
    let kept_len = kept.len() as u64;
    inner.young_keys.extend(kept);
    crate::gc::young_log::note_walk(
        SHAPE_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: true,
            logged,
            visited,
            kept: kept_len,
            table_len,
        },
    );
}

/// Visit one keys address — its family and its slot index — with the same
/// per-entry body as the full walk. Returns the post-visit address and
/// whether the keys array can still matter to a minor.
fn scan_shape_keys_address(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    table: &ShapeTable,
    inner: &mut ShapeTableInner,
    rewrite_phase: bool,
    indexed: u64,
) -> (u64, bool) {
    let mut post = indexed;
    let ids: Vec<u32> = inner
        .families
        .get(&indexed)
        .map(|ids| ids.as_slice().to_vec())
        .unwrap_or_default();
    if !ids.is_empty() {
        let slab = table.slab();
        let mut descriptor: Option<ShapeDescriptor> = None;
        for &id in &ids {
            if let Some(lifted) = slab.lift(id) {
                if lifted.old_carrier || lifted.cache_carrier {
                    descriptor = Some(lifted);
                    break;
                }
                descriptor.get_or_insert(lifted);
            }
        }
        match descriptor {
            None => {
                // Every id retired under a stale address; the family is empty.
                move_shape_family(table, inner, indexed, 0);
            }
            Some(descriptor) => {
                let mut addr = indexed as usize;
                let moved = if descriptor.old_carrier || descriptor.cache_carrier {
                    visitor.visit_usize_slot(&mut addr)
                } else {
                    visitor.visit_metadata_usize_slot(&mut addr)
                };
                if rewrite_phase && shape_keys_address_is_recycled(addr) {
                    for id in ids {
                        remove_descriptor_indexed_under(inner, id, indexed);
                    }
                } else {
                    if moved {
                        for &id in &ids {
                            if let Some(record) = slab.record_ptr(id) {
                                // SAFETY: live slab record, single-threaded agent;
                                // the store is idempotent (see the full walk).
                                unsafe { (*record).keys = addr as u64 };
                            }
                        }
                    }
                    if addr as u64 != indexed {
                        move_shape_family(table, inner, indexed, addr as u64);
                        post = addr as u64;
                    }
                }
            }
        }
    }
    if rewrite_phase && inner.indices.contains_key(&(indexed as usize)) {
        let mut addr = indexed as usize;
        visitor.visit_metadata_usize_slot(&mut addr);
        if addr != indexed as usize {
            if let Some(shape) = inner.indices.remove(&(indexed as usize)) {
                inner.indices.insert(addr, shape);
            }
            post = addr as u64;
        }
        if shape_keys_address_is_recycled(addr) {
            inner.indices.remove(&addr);
        }
    }
    (post, shape_keys_entry_is_minor_relevant(table, inner, post))
}

// #8112 sabotage switch. Suppressing the descriptor edge proves the fixture's
// detector distinguishes a rewritten record from a stale one.
//
// Deliberately `#[cfg(test)]` thread-locals and not env knobs: the GC-knob
// kill policy requires every shipped knob's off-state to be exercised by a
// required CI arm, and neither state may be reachable in a shipped binary —
// Only collector-level fixtures may turn it on.
#[cfg(test)]
thread_local! {
    static KEYS_EDGE_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static RECYCLED_KEYS_CHECK_SUPPRESSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only helpers for the shape table, in a sibling file (see the cap note there).
#[cfg(test)]
#[path = "shapes_test_support.rs"]
mod shapes_test_support;

#[cfg(test)]
pub(crate) use shapes_test_support::*;

/// The shape-table unit suites, in a sibling file: `shapes.rs` sits close to
/// the repo's 2000-line-per-file cap and #8112 added the descriptor record's
/// keys slot and old-carrier gate to it. Moved verbatim.
#[cfg(test)]
#[path = "shapes_tests.rs"]
mod shapes_tests;

#[cfg(test)]
#[path = "region_numeric_read_tests.rs"]
mod region_numeric_read_tests;

/// #9612: release the capacity that pruning left behind.
///
/// hashbrown never shrinks on `remove`/`retain`, so the shape tables keep the
/// allocation of their startup PEAK for the life of the process. Measured on
/// the compiled claude-code TUI at idle: `ids_by_facts` held 30.8 MB at 12.3%
/// fill and `descriptors` 13.8 MB at 10.5% fill, i.e. sized for a peak that
/// `prune_dead_shape_keys` had already discarded.
///
/// Called once per MAJOR collection, right after the prune, where a rehash is
/// already amortized against a full heap walk. #9706: the by-id store is a
/// slab now, so this also releases its all-dead chunks; the family and slot
/// index maps are shrunk to `len + len / 4`, one growth step of headroom.
pub(crate) fn shrink_shape_tables() {
    fn worth_shrinking(len: usize, capacity: usize) -> bool {
        // Only when the table is holding real slack and is less than half
        // used; a small or well-packed table is left alone.
        capacity > 4096 && capacity > len.saturating_mul(2)
    }
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if worth_shrinking(inner.indices.len(), inner.indices.capacity()) {
        let target = inner.indices.len() + inner.indices.len() / 4;
        inner.indices.shrink_to(target);
    }
    if worth_shrinking(inner.by_facts.len(), inner.by_facts.capacity()) {
        let target = inner.by_facts.len() + inner.by_facts.len() / 4;
        inner.by_facts.shrink_to(target);
    }
    if worth_shrinking(inner.families.len(), inner.families.capacity()) {
        let target = inner.families.len() + inner.families.len() / 4;
        inner.families.shrink_to(target);
    }
    // SAFETY: the prune that precedes this call holds no slab reference, and
    // neither does anything else while the major collection owns the agent.
    unsafe { table.slab_mut().release_empty_chunks() };
}

/// `PERRY_GC_CENSUS`: the by-id slab, the per-shape key indices, the
/// exact-facts accelerator and the keys-address family index.
pub(crate) fn shape_table_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::map_bytes;
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut rows = Vec::new();
    rows.push(("shapes.descriptors", slab.len(), slab.estimated_bytes()));
    let index_inner: usize = inner.indices.values().map(|ix| ix.slots.heap_bytes()).sum();
    rows.push((
        "shapes.indices",
        inner.indices.len(),
        map_bytes(&inner.indices) + index_inner,
    ));
    let facts_inner: usize = inner.by_facts.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.by_facts",
        inner.by_facts.len(),
        map_bytes(&inner.by_facts) + facts_inner,
    ));
    let families_inner: usize = inner.families.values().map(IdList::heap_bytes).sum();
    rows.push((
        "shapes.families",
        inner.families.len(),
        map_bytes(&inner.families) + families_inner,
    ));
    // Ids ever minted by this process: the slab is indexed by id, so the gap
    // between this and `shapes.descriptors` is what chunk release reclaims.
    let minted = SHAPE_ID_NEXT
        .iter()
        .map(|next| next.load(std::sync::atomic::Ordering::Relaxed) - STATIC_SHAPE_ID_END)
        .sum::<u32>();
    rows.push(("shapes.ids_minted(process)", minted as usize, 0));
    // How the descriptor population splits by [[Prototype]] identity kind,
    // and how many distinct prototype identities it names: what the
    // prototype-in-shape rule costs in shapes.
    let mut by_kind = [0usize; 6];
    let mut distinct = std::collections::HashSet::new();
    slab.for_each(|_, record| {
        // SAFETY: live slab record, read immediately under agent ownership.
        let proto_id = unsafe { (*record).proto_id };
        distinct.insert(proto_id);
        let kind = match proto_id {
            PROTO_ID_DEFAULT => 0,
            PROTO_ID_NULL => 1,
            id if id >> PROTO_ID_TAG_SHIFT == 0 => 2,
            id if id & PROTO_ID_UNIQUE == PROTO_ID_CLASS => 3,
            id if id & PROTO_ID_UNIQUE == PROTO_ID_MIXED => 4,
            _ => 5,
        };
        by_kind[kind] += 1;
    });
    for (name, count) in [
        "shapes.proto(object_prototype)",
        "shapes.proto(null)",
        "shapes.proto(object_serial)",
        "shapes.proto(class_default)",
        "shapes.proto(class_recorded)",
        "shapes.proto(unique)",
    ]
    .into_iter()
    .zip(by_kind)
    {
        rows.push((name, count, 0));
    }
    rows.push(("shapes.proto.distinct_identities", distinct.len(), 0));
    rows
}

/// `PERRY_GC_CENSUS`: how the descriptor population relates to the live heap
/// (#9706). `live_ids` is the sorted, deduplicated set of ShapeIds stamped on
/// live shaped objects, collected by the census walk.
///
/// * `shapes.descriptors.carried` — descriptors some live object is stamped
///   with: the population V8's "object shape" bucket corresponds to.
/// * `shapes.descriptors.uncarried` — descriptors no live object carries:
///   transition history a cache may reinstall (`cache_carrier`), versions
///   kept for an old receiver since the last full trace, and shapes whose
///   keys array is still alive on some other descriptor.
/// * `shapes.families.multi` — keys arrays with more than one descriptor,
///   and the descriptors they hold beyond the first: the duplication the
///   family walk pays for.
pub(crate) fn shape_table_liveness_census(
    live_ids: &[u32],
) -> Vec<crate::gc::census::SideTableRow> {
    let table = &crate::state::state().shapes;
    let inner = table.inner.borrow();
    let slab = table.slab();
    let mut carried = 0usize;
    let mut uncarried = 0usize;
    let mut uncarried_cache = 0usize;
    let mut uncarried_old = 0usize;
    slab.for_each(|id, record| {
        if live_ids.binary_search(&id).is_ok() {
            carried += 1;
            return;
        }
        uncarried += 1;
        // SAFETY: live slab record, read immediately.
        let record = unsafe { *record };
        if record.cache_carrier() {
            uncarried_cache += 1;
        } else if record.has(RECORD_FLAG_OLD_CARRIER) {
            uncarried_old += 1;
        }
    });
    let mut multi_families = 0usize;
    let mut multi_extra = 0usize;
    let mut largest = 0usize;
    for ids in inner.families.values() {
        let n = ids.len();
        largest = largest.max(n);
        if n > 1 {
            multi_families += 1;
            multi_extra += n - 1;
        }
    }
    vec![
        ("shapes.descriptors.carried(live objects)", carried, 0),
        ("shapes.descriptors.uncarried", uncarried, 0),
        (
            "shapes.descriptors.uncarried.cache_carrier",
            uncarried_cache,
            0,
        ),
        ("shapes.descriptors.uncarried.old_carrier", uncarried_old, 0),
        (
            "shapes.families.multi(families,extra descriptors)",
            multi_families,
            multi_extra,
        ),
        ("shapes.families.largest", largest, 0),
    ]
}
