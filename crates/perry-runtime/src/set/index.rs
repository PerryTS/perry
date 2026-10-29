//! Owner-local Set index. Buckets contain only a hash and a raw element index;
//! equality always reads the authoritative (GC-rewritten) elements buffer.
//! Strings are never rehashed merely because they move. Pointer keys get a
//! scalar identity token in their own metadata, shared by every Set.
//! Only inserting into an index allocates a token; a probe for a key without
//! one is a definite miss.

use super::*;
use std::cell::Cell;
use std::hash::{BuildHasher, Hash, Hasher};

crate::perry_thread_local! {
    static NEXT_SET_KEY_IDENTITY: Cell<u64> = const { Cell::new(1) };
}

/// A scalar identity belongs to its key's private metadata. Array and
/// closure keys use their existing traced property holders; other public
/// object cells already have a direct metadata edge.
unsafe fn key_identity(addr: usize, create: bool) -> Option<u64> {
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    let _no_move = crate::gc::GcSuppressScope::new();
    let meta = match header.obj_type {
        crate::gc::GC_TYPE_ARRAY => {
            let bag = if create {
                crate::array::array_property_bag_ensure(addr as *mut crate::array::ArrayHeader)
            } else {
                crate::array::array_property_bag(addr as *const crate::array::ArrayHeader)
            };
            if bag.is_null() {
                return None;
            }
            if create {
                crate::object::object_meta_ensure(
                    crate::meta_obj_runtime_born!(bag),
                    crate::object::MetaReason::SetKeyIdentity,
                )
            } else {
                (*bag).meta
            }
        }
        crate::gc::GC_TYPE_CLOSURE => {
            let bag = if create {
                crate::closure::props::bag_ensure(addr)
            } else {
                crate::closure::props::bag_of(addr)
            };
            if bag.is_null() {
                return None;
            }
            if create {
                crate::object::object_meta_ensure(
                    crate::meta_obj_runtime_born!(bag),
                    crate::object::MetaReason::SetKeyIdentity,
                )
            } else {
                (*bag).meta
            }
        }
        _ => {
            let slot = crate::object::cell_meta_slot(addr)?;
            if create {
                crate::object::object_meta_ensure_for_cell(
                    addr,
                    crate::object::MetaReason::SetKeyIdentity,
                )?
            } else {
                *slot
            }
        }
    };
    if meta.is_null() {
        return None;
    }
    let id = &mut (*meta).key_identity;
    if *id == 0 && create {
        // GC_STORE_AUDIT(NONPOINTER): scalar identity counter, never an edge.
        *id = NEXT_SET_KEY_IDENTITY.with(|next| {
            let id = next.get();
            next.set(id.checked_add(1).expect("Set key identity exhausted"));
            id
        });
    }
    (*id != 0).then_some(*id)
}

/// Hash for a value being PUT INTO an index: allocates an identity for a
/// movable key that has none yet.
fn value_hash(value: f64) -> u32 {
    hash_value(value, true).expect("inserting hash always resolves")
}

/// Hash for a PROBE (`has`/`delete`/`remove`): never allocates an identity.
/// `None` means the key is a movable object with no identity, which is a
/// definite miss — every element put into an index went through
/// [`value_hash`], and an identity is dropped only when its object dies.
fn existing_hash(value: f64) -> Option<u32> {
    hash_value(value, false)
}

/// Preserve the legacy raw-pointer key representation (top word zero):
/// it may name a movable object and needs the ordinary identity hash.
#[inline]
fn is_plain_numeric_key(bits: u64) -> bool {
    bits >> 48 != 0 && crate::map::is_plain_nonzero_number_bits(bits)
}

/// Fold all IEEE bits into the bucket hash before truncating to 32 bits.
/// Integer doubles have long zero mantissa tails: the pointer multiply/xor
/// leaves low bucket bits clustered, forcing long probe walks even when
/// the complete hashes differ. This avalanche belongs only to
/// the numeric key domain; movable keys keep their relocation-stable identity.
#[inline(always)]
fn number_hash(mut bits: u64) -> u32 {
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (bits ^ (bits >> 31)) as u32
}

fn hash_value(value: f64, insert: bool) -> Option<u32> {
    #[cfg(test)]
    TEST_HASH_CALLS.with(|count| count.set(count.get() + 1));
    let bits = value.to_bits();
    let mut hasher = crate::fast_hash::PtrHasher.build_hasher();
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let addr = (bits & crate::value::POINTER_MASK) as *const crate::symbol::SymbolHeader;
        unsafe { (*addr).id }.hash(&mut hasher);
        return Some(hasher.finish() as u32);
    }
    if is_string_like(bits) {
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((data, len)) = string_view_from_bits(bits, &mut scratch) {
            // The Map's string-key hash: a property atom answers with the
            // hash it was born with instead of rehashing its bytes.
            // SAFETY: a view that is not `scratch` is a heap string's payload.
            let hash = unsafe { crate::map::string_key_hash(data, len, data != scratch.as_ptr()) };
            return Some(crate::codegen_abi::set_string_index_hash(hash));
        }
    }
    let tag = bits >> 48;
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if matches!(tag, 0 | 0x7FFD | 0x7FFA) {
        if let Some(header) = unsafe { crate::value::addr_class::try_read_gc_header(addr) }
            .filter(|h| crate::gc::gc_type_is_movable(h.obj_type))
        {
            let kind = header.obj_type;
            if kind == crate::gc::GC_TYPE_BIGINT {
                unsafe {
                    crate::bigint::BigIntHeader::all_limbs(
                        addr as *const crate::bigint::BigIntHeader,
                    )
                }
                .hash(&mut hasher);
            } else if matches!(
                kind,
                crate::gc::GC_TYPE_OBJECT
                    | crate::gc::GC_TYPE_ARRAY
                    | crate::gc::GC_TYPE_CLOSURE
                    | crate::gc::GC_TYPE_PROMISE
                    | crate::gc::GC_TYPE_ERROR
                    | crate::gc::GC_TYPE_MAP
                    | crate::gc::GC_TYPE_SET
            ) {
                let id = unsafe { key_identity(addr, insert) }?;
                (tag, id).hash(&mut hasher);
            } else {
                // Internal non-object cells have no public identity slot.
                // A type hash is relocation-stable; authoritative element
                // equality resolves collisions without retaining addresses.
                (tag, kind).hash(&mut hasher);
            }
            return Some(hasher.finish() as u32);
        }
    }
    // String/symbol/identity paths return above. Only the remaining numeric
    // domain needs an avalanche; ordinary string startup probes pay no new
    // numeric admission work.
    if is_plain_numeric_key(bits) {
        return Some(number_hash(bits));
    }
    bits.hash(&mut hasher);
    Some(hasher.finish() as u32)
}

pub(crate) use crate::codegen_abi::OwnedSetIndex as SetIndex;
use crate::codegen_abi::SET_INDEX_EMPTY as EMPTY;

trait SetIndexLookup {
    unsafe fn find(&self, set: *const SetHeader, value: f64, hash: u32) -> Option<usize>;
    unsafe fn find_number_bits(&self, set: *const SetHeader, bits: u64, hash: u32)
        -> Option<usize>;
    unsafe fn find_by(
        &self,
        set: *const SetHeader,
        hash: u32,
        equals: impl Fn(f64) -> bool,
    ) -> Option<usize>;
}
impl SetIndexLookup for SetIndex {
    unsafe fn find(&self, set: *const SetHeader, value: f64, hash: u32) -> Option<usize> {
        self.find_by(set, hash, |candidate| {
            candidate.to_bits() != SET_HOLE_VALUE_BITS && jsvalue_eq(candidate, value)
        })
    }

    /// The caller admits a normalized plain Number. This specialization
    /// contains only bits equality, even in an index containing mixed keys.
    #[inline(always)]
    unsafe fn find_number_bits(
        &self,
        set: *const SetHeader,
        bits: u64,
        hash: u32,
    ) -> Option<usize> {
        self.find_by(set, hash, |candidate| candidate.to_bits() == bits)
    }

    #[inline(always)]
    unsafe fn find_by(
        &self,
        set: *const SetHeader,
        hash: u32,
        equals: impl Fn(f64) -> bool,
    ) -> Option<usize> {
        let mask = self.buckets().len() - 1;
        let mut pos = hash as usize & mask;
        loop {
            let bucket = self.buckets()[pos];
            if bucket.index == EMPTY {
                return None;
            }
            if bucket.index < (*set).used && bucket.hash == hash {
                let candidate = *(*set).elements.add(bucket.index as usize);
                if equals(candidate) {
                    return Some(pos);
                }
            }
            pos = (pos + 1) & mask;
        }
    }
}

/// A static index remains shared and untouched on reads. Its first mutation
/// takes ownership through the same SetStore slot used by every native index.
unsafe fn mutable_index(set: *mut SetHeader) -> *mut SetIndex {
    let source = index_ptr(set);
    if source.is_null() {
        return ptr::null_mut();
    }
    if (*(*set).store).index.is_none() {
        install_owned_index(set, Box::new((*source).clone_owned()));
    }
    (*(*set).store).index.as_deref_mut().unwrap()
}

unsafe fn index_ptr(set: *const SetHeader) -> *mut SetIndex {
    let meta = (*set).meta;
    if meta.is_null() {
        ptr::null_mut()
    } else {
        (*meta).native_state as *mut SetIndex
    }
}

/// Direct indexed probe for an actual nonzero Number. The canonical index
/// already proves bucket occupancy; its entry offsets read GC-rewritten values.
/// No generic hash/equality frame or JSValue conversion is needed on this path.
// Keep the indexed loop out of the generic callers' frames: otherwise its
// registers force a callee-save prologue even for string/object probes.
#[inline(never)]
pub(super) unsafe fn lookup_plain_number(set: *const SetHeader, bits: u64) -> i32 {
    debug_assert!(is_plain_numeric_key(bits));
    let Some(index) = index_ptr(set).as_ref() else {
        // Shrinking during an active walk can drop the index while holes
        // keep used above the small-scan extent. Bits equality is still a
        // complete answer for this admitted Number, without a generic exit.
        return (0..(*set).used)
            .find(|&i| (*(*set).elements.add(i as usize)).to_bits() == bits)
            .map_or(-1, |i| i as i32);
    };
    #[cfg(test)]
    TEST_HASH_CALLS.with(|count| count.set(count.get() + 1));
    let hash = number_hash(bits);
    index
        .find_number_bits(set, bits, hash)
        .map_or(-1, |pos| index.buckets()[pos].index as i32)
}

pub(super) unsafe fn lookup_value(set: *const SetHeader, value: f64) -> Option<i32> {
    let index = index_ptr(set).as_ref()?;
    // `Some(-1)`, not `None`: a probe key with no identity is a definite miss
    // (`None` would send the caller to a linear scan of the elements).
    let Some(hash) = existing_hash(value) else {
        return Some(-1);
    };
    Some(
        index
            .find(set, value, hash)
            .map_or(-1, |pos| index.buckets()[pos].index as i32),
    )
}

pub(super) unsafe fn insert_value(set: *mut SetHeader, value: f64, raw: u32) {
    let index = mutable_index(set);
    if index.is_null() {
        if (*set).size > SMALL_SET_SCAN_MAX {
            rebuild_index(set);
        }
    } else {
        let before = (*index).byte_len();
        (*index).insert_hash(value_hash(value), raw);
        let added = (*index).byte_len() - before;
        if added != 0 {
            crate::gc::gc_note_external_side_alloc(added);
        }
    }
}

pub(super) unsafe fn rebuild_index(set: *mut SetHeader) {
    if (*set).size <= SMALL_SET_SCAN_MAX {
        clear_index(set);
        return;
    }
    let mut index = Box::new(SetIndex::new((*set).size as usize));
    for i in 0..(*set).used {
        let value = *(*set).elements.add(i as usize);
        if value.to_bits() != SET_HOLE_VALUE_BITS {
            index.insert_hash(value_hash(value), i);
        }
    }
    install_owned_index(set, index);
}

unsafe fn install_owned_index(set: *mut SetHeader, mut index: Box<SetIndex>) {
    // This bounded metadata allocation must not collect while `set` is a raw
    // pointer. No user code runs here; normal allocation pacing resumes below.
    let meta = {
        let _no_gc = crate::gc::GcSuppressScope::new();
        crate::object::object_meta_ensure_for_cell(
            set as usize,
            crate::object::MetaReason::SetKeyIdentity,
        )
        .expect("Set has a meta edge")
    };
    let bytes = index.byte_len();
    let index_ptr = &mut *index as *mut SetIndex;
    let old = (*(*set).store).index.replace(index);
    let old_bytes = old.as_ref().map_or(0, |index| index.byte_len());
    drop(old);
    // Native allocation, owned by the traced Set store, not a managed-heap edge.
    (*meta).native_state = index_ptr as u64;
    crate::gc::gc_note_external_side_free(old_bytes);
    crate::gc::gc_note_external_side_alloc(bytes);
}

pub(super) unsafe fn remove_value(set: *mut SetHeader, value: f64, raw: u32) {
    let Some(index) = mutable_index(set).as_mut() else {
        return;
    };
    // The authoritative element was already tombstoned, so match its raw index.
    // An indexed element always has its identity; should one be missing, fall
    // back to a full bucket scan rather than leave a live bucket behind.
    let mask = index.buckets().len() - 1;
    let pos = match existing_hash(value) {
        Some(hash) => {
            let mut pos = hash as usize & mask;
            while index.buckets()[pos].index != EMPTY && index.buckets()[pos].index != raw {
                pos = (pos + 1) & mask;
            }
            Some(pos).filter(|&pos| index.buckets()[pos].index == raw)
        }
        None => index
            .buckets()
            .iter()
            .position(|bucket| bucket.index == raw),
    };
    if let Some(pos) = pos {
        index.remove_bucket(pos);
    }
    if (*set).size <= SMALL_SET_SCAN_MAX {
        clear_index(set);
    }
}

pub(super) unsafe fn clear_index(set: *mut SetHeader) {
    if index_ptr(set).is_null() {
        return;
    }
    (*(*set).meta).native_state = 0;
    let old = (*(*set).store).index.take();
    if let Some(index) = old {
        crate::gc::gc_note_external_side_free(index.byte_len());
    }
}

#[cfg(test)]
crate::perry_thread_local! {
    static TEST_HASH_CALLS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) unsafe fn test_snapshot(set: *const SetHeader) -> (usize, usize, usize) {
    let index = index_ptr(set);
    (
        index as usize,
        index.as_ref().map_or(0, |i| i.byte_len()),
        TEST_HASH_CALLS.with(Cell::get),
    )
}

#[cfg(test)]
pub(crate) unsafe fn test_index_buckets(set: *const SetHeader) -> (Vec<(u32, u32)>, usize, usize) {
    let index = index_ptr(set).as_ref().expect("fixture must have an index");
    (
        index
            .buckets()
            .iter()
            .map(|bucket| (bucket.hash, bucket.index))
            .collect(),
        index.live_len(),
        index.occupied_len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Force every domain into one probe chain. Numeric admission must still
    /// distinguish non-numbers, honor tombstones, and agree with SameValueZero.
    #[test]
    fn numeric_candidates_in_mixed_collision_chain() {
        let set = js_set_alloc(32);
        let text = crate::string::js_string_from_bytes(b"7".as_ptr(), 1);
        let values = [
            7.0,
            -3.5,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            f64::NAN,
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            boxed_heap_string_value(text),
        ];
        for value in values {
            js_set_add(set, value);
        }
        unsafe {
            let mut index = SetIndex::new(values.len());
            for i in 0..values.len() {
                index.insert_hash(7, i as u32);
            }
            for value in values.into_iter().chain([99.0, -0.0, 1.5]) {
                let value = normalize_zero(value);
                let expected = values.iter().position(|&v| jsvalue_eq(v, value));
                if is_plain_numeric_key(value.to_bits()) {
                    assert_eq!(
                        index
                            .find_number_bits(set, value.to_bits(), 7)
                            .map(|p| index.buckets()[p].index as usize),
                        expected,
                    );
                }
                assert_eq!(
                    index
                        .find(set, value, 7)
                        .map(|p| index.buckets()[p].index as usize),
                    expected,
                );
            }
            let pos = index.find(set, 7.0, 7).unwrap();
            index.remove_bucket(pos);
            assert!(index.find_number_bits(set, 7.0f64.to_bits(), 7).is_none());
            assert!(index
                .find_number_bits(set, (-3.5f64).to_bits(), 7)
                .is_some());
            // Both insertion/general probes and admitted-number probes must
            // use the same numeric hash in a mixed index.
            for value in [7.0f64, -3.5, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(existing_hash(value), Some(number_hash(value.to_bits())));
                assert_eq!(js_set_has(set, value), 1);
            }
            finalize_set_side_allocation_for_gc(set);
        }
    }

    /// Integer IEEE encodings have zero low mantissa bits. Bucket masking
    /// must still use the information carried in the high bits of the key.
    #[test]
    fn numeric_hash_distributes_integer_mantissas_before_bucket_mask() {
        let mut occupied = [false; 1024];
        for integer in 1..=4096 {
            let hash = number_hash((integer as f64).to_bits());
            occupied[hash as usize & 1023] = true;
        }
        assert!(occupied.into_iter().filter(|&bit| bit).count() > 512);
    }

    #[test]
    fn numeric_probe_after_shrink_during_a_live_walk() {
        let set = js_set_alloc(16);
        for i in 1..=12 {
            js_set_add(set, i as f64);
        }
        set_foreach_enter(set);
        for i in 1..=4 {
            assert_eq!(js_set_delete(set, i as f64), 1);
        }
        unsafe {
            assert_eq!((*set).size, 8);
            assert!((*set).used > SMALL_SET_SCAN_MAX);
            assert!(
                index_ptr(set).is_null(),
                "shrink must exercise the indexless path"
            );
            for i in 1..=12 {
                assert_eq!(js_set_has(set, i as f64), i32::from(i > 4));
            }
            assert_eq!(js_set_has(set, 99.0), 0);
            assert!(set_foreach_leave(set));
            compact_if_holey_set(set);
            assert_eq!((*set).used, 8);
            assert_eq!(js_set_has(set, 5.0), 1);
            finalize_set_side_allocation_for_gc(set);
        }
    }

    #[test]
    fn raw_object_keys_keep_the_relocation_stable_hash() {
        unsafe {
            let object = crate::object::js_object_alloc(0, 0) as usize;
            let id = key_identity(object, true).unwrap();
            let mut expected = crate::fast_hash::PtrHasher.build_hasher();
            (0u64, id).hash(&mut expected);
            assert_eq!(
                existing_hash(f64::from_bits(object as u64)),
                Some(expected.finish() as u32)
            );
        }
    }

    #[test]
    fn borrowed_image_index_is_readonly_until_the_first_mutation() {
        let set = js_set_alloc(16);
        for value in 0..12 {
            js_set_add(set, value as f64);
        }
        unsafe {
            // The image owns no native Box. Keep this template alive in the
            // test while the Set borrows the same prefix an image emits.
            let template = (*(*set).store).index.take().unwrap();
            let address = index_ptr(set);
            let snapshot = template
                .buckets()
                .iter()
                .map(|b| (b.hash, b.index))
                .collect::<Vec<_>>();
            for value in 0..12 {
                assert_eq!(js_set_has(set, value as f64), 1);
            }
            assert_eq!(js_set_has(set, 42.0), 0);
            assert!(
                (*(*set).store).index.is_none(),
                "reads must not allocate an owned index"
            );
            assert_eq!(index_ptr(set), address);
            js_set_add(set, 42.0);
            assert_ne!(index_ptr(set), address);
            assert!((*(*set).store).index.is_some());
            assert_eq!(js_set_has(set, 42.0), 1);
            assert_eq!(js_set_delete(set, 4.0), 1);
            assert_eq!(
                template
                    .buckets()
                    .iter()
                    .map(|b| (b.hash, b.index))
                    .collect::<Vec<_>>(),
                snapshot
            );
            assert_eq!(template.live_len(), 12);
            finalize_set_side_allocation_for_gc(set);
            crate::gc::gc_note_external_side_free(template.byte_len());
        }
    }

    #[test]
    fn small_mixed_sets_are_lazy_and_rebuild_after_shrink() {
        let set = js_set_alloc(16);
        let string = crate::string::js_string_from_bytes(b"hello".as_ptr(), 5);
        let values = [
            0.0,
            f64::NAN,
            2.0,
            3.0,
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_UNDEFINED),
            boxed_heap_string_value(string),
        ];
        for value in values {
            js_set_add(set, value);
        }
        unsafe {
            assert!(
                (*set).meta.is_null(),
                "eight mixed values need no metadata or index"
            );
            for value in values {
                assert_eq!(js_set_has(set, value), 1);
            }
            js_set_add(set, 9.0);
            assert!(!index_ptr(set).is_null());
            let equivalent = crate::string::js_string_from_bytes(b"hello".as_ptr(), 5);
            assert_eq!(js_set_has_string(set, equivalent), 1);
            js_set_add_string(set, equivalent);
            assert_eq!((*set).size, 9);
            assert_eq!(js_set_delete(set, 9.0), 1);
            assert!(index_ptr(set).is_null());
            for value in values {
                assert_eq!(js_set_has(set, value), 1);
            }
            js_set_add(set, 10.0);
            assert!(!index_ptr(set).is_null());
            js_set_clear(set);
            assert!(index_ptr(set).is_null());
            js_set_add(set, 11.0);
            assert!(index_ptr(set).is_null());
            finalize_set_side_allocation_for_gc(set);
        }
    }

    #[test]
    fn hash_collisions_use_live_values_and_tombstones_preserve_probe_chains() {
        let set = js_set_alloc(16);
        for i in 0..12 {
            js_set_add(set, i as f64);
        }
        unsafe {
            let mut index = SetIndex::new(12);
            for i in 0..12 {
                index.insert_hash(7, i);
            }
            for i in 0..12 {
                assert!(index.find(set, i as f64, 7).is_some());
            }
            let pos = index.find(set, 3.0, 7).unwrap();
            index.remove_bucket(pos);
            assert!(index.find(set, 3.0, 7).is_none());
            assert!(index.find(set, 11.0, 7).is_some());
            assert!(index.find(set, 99.0, 7).is_none());
            index.insert_hash(7, 3);
            assert!(index.find(set, 3.0, 7).is_some());
            finalize_set_side_allocation_for_gc(set);
        }
    }

    #[test]
    fn churn_matches_ordered_membership_across_growth_and_compaction() {
        let set = js_set_alloc(4);
        let mut expected = Vec::new();
        let mut seed = 12345u32;
        for step in 0..6000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let value = ((seed >> 16) % 128) as f64;
            if step % 97 == 0 {
                js_set_clear(set);
                expected.clear();
            } else if seed & 3 == 0 {
                let present = expected.contains(&value);
                assert_eq!(js_set_delete(set, value), i32::from(present));
                expected.retain(|v| *v != value);
            } else {
                js_set_add(set, value);
                if !expected.contains(&value) {
                    expected.push(value);
                }
            }
            assert_eq!(js_set_size(set) as usize, expected.len());
            for i in 0..128 {
                assert_eq!(
                    js_set_has(set, i as f64),
                    i32::from(expected.contains(&(i as f64)))
                );
            }
            for (i, &v) in expected.iter().enumerate() {
                assert_eq!(js_set_value_at(set, i as u32), v);
            }
        }
        unsafe {
            finalize_set_side_allocation_for_gc(set);
        }
    }
}

#[cfg(test)]
pub(crate) unsafe fn test_key_identity(addr: usize) -> Option<u64> {
    key_identity(addr, false)
}

#[cfg(test)]
pub(crate) unsafe fn test_erase_key_identity(addr: usize) {
    let meta = *crate::object::cell_meta_slot(addr).unwrap();
    (*meta).key_identity = 0;
}
