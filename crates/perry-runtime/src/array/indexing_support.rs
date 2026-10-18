//! Indexing support split out of `indexing.rs` to keep it under the repo's
//! 2000-line cap: the strict-store TypeError throwers, the prototype
//! indexed-property / iterator invalidation latches, sparse index helpers, and
//! the dense keys-array slot helpers. Pure move except for the `use` lines and
//! `pub(super)` visibility on items `indexing.rs` still calls.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

/// Largest hole (`index - length`) an extending write may create while still
/// growing the dense backing store once the array is large. Sparse storage is
/// for jumps far beyond the current length; sequential growth must stay dense
/// because routing it through string-keyed property sets is quadratic.
pub(super) const DENSE_ARRAY_GAP_LIMIT: u32 = 1024;

#[inline]
pub(super) unsafe fn array_sparse_index_property_get(
    arr: *const ArrayHeader,
    index: u32,
) -> Option<f64> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() || index < (*arr).capacity {
        return None;
    }
    let key = index.to_string();
    array_named_property_get_by_name(arr, &key)
}

/// Returns the live head: the first sparse index on a full array reserves the
/// named-property slot by growing it (`named_props.rs`), exactly as a dense
/// extension past capacity would.
pub(super) unsafe fn array_sparse_index_property_set(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    let key = index.to_string();
    // #6935: the key build allocates, so the receiver is reloaded across it
    // and the value re-read from its handle; the setter roots its own
    // allocations before it can collect.
    let (key_ptr, arr) = arr_handle.across_mut::<ArrayHeader, _>(|| {
        crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
    });
    let arr = array_named_property_set(arr, key_ptr, value_handle.get_nanbox_f64());
    if arr.is_null() {
        return arr;
    }
    let new_length = index + 1;
    if (*arr).length < new_length {
        (*arr).length = new_length;
    }
    arr
}

/// Resolve a raw array head a generated loop re-read from its root after a
/// callback returned: the callback may have grown the array, leaving the root
/// on a forwarding stub. Pure `clean_arr_ptr`; null for anything that is not
/// an array. Generated `some` loops call this only when the re-read head's
/// header carries `GC_FLAG_FORWARDED`.
#[no_mangle]
pub extern "C" fn js_array_live_head(arr: i64) -> i64 {
    clean_arr_ptr(arr as *const ArrayHeader) as i64
}

/// A strict-mode element write (`arr[i] = v`) to a **frozen** array's existing
/// index is `[[Set]]` on a non-writable data property with `Throw = true`
/// (ECMA-262 §10.4.2.4 → OrdinarySetWithOwnDescriptor step 2.b.i), so it must
/// throw a **TypeError** rather than silently no-op. Perry compiles everything
/// strict, so the codegen `arr[i] = v` fast paths — which call these
/// `js_array_set_f64*` helpers directly — carry the strict-`Set` contract.
/// Matches V8's message. (test262 built-ins/Array element-write-on-frozen.)
#[cold]
pub(super) fn throw_frozen_array_index_write(index: u32) -> ! {
    crate::collection_iter::throw_type_error(&format!(
        "Cannot assign to read only property '{index}' of object '[object Array]'"
    ));
}

/// A strict-mode write that would *add* a new index to a non-extensible
/// (frozen / sealed / preventExtensions'd) array — `arr[i] = v` with
/// `i >= length` — is `CreateDataProperty` on a non-extensible object with
/// `Throw = true`, so it must throw a **TypeError**. Matches V8's message.
#[cold]
pub(super) fn throw_array_not_extensible_add(index: u32) -> ! {
    crate::collection_iter::throw_type_error(&format!(
        "Cannot add property {index}, object is not extensible"
    ));
}

/// Sticky flag: someone installed an indexed property on `Array.prototype`.
/// An out-of-bounds element read on an ordinary array must fall through to
/// `Array.prototype[index]` (ECMA-262 OrdinaryGet -> prototype chain), but in
/// real code nobody adds numeric indices there, so the hot OOB path stays a
/// single relaxed atomic load until the (rare) write flips this. The address
/// it is compared against lives in [`super::prototype_addr`], which also owns
/// the GC hazard that address carries (#6981).
pub(super) static ARRAY_PROTO_HAS_INDEX: AtomicBool = AtomicBool::new(false);

/// Same idea for `Object.prototype`: a numeric index installed there
/// (`Object.prototype[2] = 2`, or a defineProperty accessor) shows through
/// array HOLES and OOB reads (chain: arr -> Array.prototype ->
/// Object.prototype; test262 concat/S15.4.4.4_A3_T3). Flipped by the object
/// index-write/defineProperty hooks; consulted by the typed-feedback guards
/// and the hole/OOB read fallbacks.
pub(super) static OBJECT_PROTO_HAS_INDEX: AtomicBool = AtomicBool::new(false);

per_test_global! {
    /// Sticky summary of the process-wide conditions that invalidate codegen's
    /// inline plain-array index guard. The generated guard loads this byte
    /// directly; keeping the three rare prototype conditions behind one exported
    /// byte avoids an out-of-line runtime call on every array read.
    ///
    /// Outside a test build this is the plain exported `AtomicU8` generated
    /// code links against by name. In a test build it is per-thread, so a test
    /// asserting that one retarget did (or did not) flip it cannot be broken
    /// by a sibling test retargeting an array on another libtest thread (#7672).
    #[no_mangle]
    pub static PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED: AtomicU8 = AtomicU8::new(0)
}

/// [`PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`] bit: an indexed property (or a
/// retarget) on the default chain. Index fast paths test this bit.
pub(crate) const INDEX_INVALIDATED: u8 = 1;
/// [`PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`] bit (#11915): `%Array.prototype%`
/// or `%Object.prototype%` was modified — the mirror of `builtin_proto`'s
/// Array byte in the byte the inline `push`/`pop` admission already loads,
/// which therefore tests the whole byte and proves `Pristine(Array)` with no
/// load of its own. Index fast paths mask it out.
pub(crate) const ARRAY_METHODS_MODIFIED: u8 = 2;

#[inline]
pub(crate) fn invalidate_array_index_fast_path() {
    PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.fetch_or(INDEX_INVALIDATED, Ordering::Relaxed);
}

/// `%Array.prototype%` / `%Object.prototype%` modified (#11915): withdraw the
/// inline array-method admission. Set-only.
#[inline]
pub(crate) fn note_array_methods_modified() {
    PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.fetch_or(ARRAY_METHODS_MODIFIED, Ordering::Relaxed);
}

/// Must an index fast path decline for the `GC_TYPE_ARRAY` whose header
/// `_reserved` word is `reserved`, because an inherited indexed property could
/// intercept the access?
///
/// #10593: the process-wide byte covers the default chain (`Array.prototype` /
/// `Object.prototype` indices, a retargeted `Array.prototype`); a retargeted
/// ordinary array is answered by its OWN `GC_ARRAY_CUSTOM_PROTO` bit, so one
/// `Object.setPrototypeOf(arr, p)` no longer moves every other array's element
/// stores off the fast path and onto the full write barrier. The generated
/// guards test the same two conditions inline.
#[inline]
pub(crate) fn array_index_fast_path_invalid_for(reserved: u16) -> bool {
    reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0
        || PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) & INDEX_INVALIDATED != 0
}

/// Test-only companion to
/// `prototype_chain::test_swap_array_static_proto_recorded`: swap the summary
/// byte generated code reads, returning the previous value. Only for a test
/// that knowingly set it and is putting the process back as it found it.
#[cfg(test)]
pub(crate) fn test_swap_array_index_fast_path_invalidated(value: u8) -> u8 {
    PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.swap(value, Ordering::Relaxed)
}

/// Record (if `obj` is the canonical `Object.prototype`) that it now carries
/// an indexed property. Called from the object index-write / numeric
/// defineProperty paths; cheap (relaxed loads + compare).
#[inline]
pub(crate) fn note_object_prototype_index_write(obj: usize) {
    if !OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed) && obj != 0 && obj == object_prototype_addr()
    {
        OBJECT_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
        invalidate_array_index_fast_path();
    }
}

pub(crate) fn object_prototype_has_index_flag() -> bool {
    OBJECT_PROTO_HAS_INDEX.load(Ordering::Relaxed)
}

/// The program's array-iteration facts (object model step 7), fixed by the
/// compiler from the whole-program write census and handed over before any
/// user code runs; never learned from stores. Bit 0
/// ([`ITERATION_MEMBER_WRITABLE`]): a write may reach a `Symbol.iterator`
/// member or re-parent an array, so iteration reads the member's one slot.
/// Bit 1 ([`ITERATION_NEXT_REACHABLE`]): `%ArrayIteratorPrototype%` may be
/// named, so iteration also checks its `next`. A runtime with no compiled
/// program (the Rust tests) assumes both.
static ARRAY_ITERATION_FACTS: AtomicU32 = AtomicU32::new(
    ITERATION_MEMBER_WRITABLE | ITERATION_NEXT_REACHABLE | ELEMENT_DESCRIPTORS_REACHABLE,
);
pub(crate) const ITERATION_MEMBER_WRITABLE: u32 = 1;
pub(crate) const ITERATION_NEXT_REACHABLE: u32 = 2;
pub(crate) const ELEMENT_DESCRIPTORS_REACHABLE: u32 = 4;

#[no_mangle]
pub extern "C" fn perry_array_iteration_init(facts: u32) {
    ARRAY_ITERATION_FACTS.store(facts, Ordering::Relaxed);
}

/// The program's array-iteration facts (see [`ARRAY_ITERATION_FACTS`]).
#[inline]
pub(crate) fn array_iteration_facts() -> u32 {
    ARRAY_ITERATION_FACTS.load(Ordering::Relaxed)
}

/// Record (if `arr` is `Array.prototype`) that the prototype now carries an
/// indexed property, so subsequent out-of-bounds reads consult it. Called from
/// the array element-write paths; cheap (two relaxed atomic loads + compare).
#[inline]
pub(crate) fn note_array_index_write(arr: usize) {
    if !ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) && arr != 0 && arr == array_prototype_addr() {
        ARRAY_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
        invalidate_array_index_fast_path();
    }
}

#[no_mangle]
/// Reported length of an object's keys/property array, capped at its physical
/// capacity.
///
/// Object property walks (the wide-key field-get index and `Object.assign`'s
/// source enumeration) size their work by the keys array's length. A dense
/// keys array's logical length can never exceed its capacity, so for a
/// well-formed array this is a no-op. But when a keys array is malformed and
/// `js_array_length` reports a bogus, oversized value (observed: a pointer-
/// sized length ~= the keys pointer's own low bits, far beyond the real key
/// count), an unclamped `for i in 0..len` / `HashMap::with_capacity(len)` turns
/// a single missing-property read or `Object.assign` into a multi-GB / minutes-
/// long spin. Capping to capacity bounds that work to physically-present slots.
///
/// FOR DENSE KEYS/PROPERTY ARRAYS ONLY — general JS arrays may have
/// `length > capacity` (sparse), where this cap would be incorrect.
pub(crate) unsafe fn keys_array_len_capped_to_capacity(arr: *const ArrayHeader) -> usize {
    // #7765: a well-formed dense keys array answers from its own two words.
    // `js_array_length` re-derives the same number through a proxy probe, a
    // second header read for its lazy/object arms, and a `clean_arr_ptr`
    // forwarding walk — once per property read on the field-get funnel.
    // `length <= capacity` is exactly the well-formed case; the sparse and
    // corrupted shapes this cap exists for fall through unchanged.
    if let Some(header) = crate::value::addr_class::try_read_gc_header(arr as usize) {
        if matches!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_OVERFLOW
        ) && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && (*arr).length <= (*arr).capacity
        {
            return (*arr).length as usize;
        }
    }
    // A forwarding stub overwrites the old payload's `(length, capacity)`
    // words with the target address. Resolve once, then read BOTH facts from
    // the live header; mixing a resolved length with the stale from-space
    // capacity can truncate an otherwise exact shape count.
    //
    // A keys array is an internal dense array, never a Proxy, Set/Map,
    // array-like object or lazy JSON array, so neither the generic
    // `js_array_length` nor `clean_arr_ptr` (whose exotic arms run user JS
    // and can throw) has anything to add here: the owned-array resolver
    // follows forwarding only. A pointer that does not resolve to an array
    // has no keys (#12743).
    let live = clean_owned_arr_ptr(arr);
    if live.is_null() {
        return 0;
    }
    ((*live).length as usize).min((*live).capacity as usize)
}

/// Read slot `index` of a dense internal keys/property array.
///
/// The object field-get funnel has already proved `keys` is a live
/// `GC_TYPE_ARRAY` — it reads the `GcHeader` and returns `undefined` otherwise
/// — and has capped `index` below the array's own capacity (see
/// [`keys_array_len_capped_to_capacity`]). Those are precisely the two facts
/// [`js_array_get_f64`] re-establishes from scratch on every call: a
/// `clean_arr_ptr` forwarding walk, a lazy-header probe, the exotic-receiver
/// classifications and a descriptor-flag read — per key examined, per property
/// read. On `gc-handoff/apps/asyncpipe_big.ts` that one funnel was 78% of all
/// `js_array_get_f64` samples.
///
/// Falls back to the general getter for anything it cannot serve on those
/// terms — a forwarded array (which `clean_arr_ptr` would relocate), one
/// carrying index descriptors, an out-of-range index, or a hole (which reads
/// through the prototype chain) — so no general semantics move. Keys arrays
/// are dense and descriptor-free, so the fallback is the cold arm.
#[inline]
pub(crate) unsafe fn keys_array_slot(
    keys: *const ArrayHeader,
    index: u32,
) -> crate::value::JSValue {
    if let Some(header) = crate::value::addr_class::try_read_gc_header(keys as usize) {
        if header.obj_type == crate::gc::GC_TYPE_ARRAY
            && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && !super::array_has_element_descriptors_resolved(keys)
            && index < (*keys).length
            && index < (*keys).capacity
        {
            let elements =
                crate::array::array_elements_ptr(keys as *const ArrayHeader) as *const f64;
            let raw = std::ptr::read(elements.add(index as usize));
            if raw.to_bits() != crate::value::TAG_HOLE {
                return crate::value::JSValue::from_bits(raw.to_bits());
            }
        }
    }
    #[cfg(test)]
    KEYS_ARRAY_SLOT_FALLBACKS.with(|c| c.set(c.get().wrapping_add(1)));
    crate::array::js_array_get(keys, index)
}

#[cfg(test)]
thread_local! {
/// Times [`keys_array_slot`] could NOT serve a slot from the dense words and
/// had to delegate. Asserted in both directions by
/// `array::collection_tag_tests` — zero for the dense keys arrays the fast path
/// exists for, non-zero for every shape it must refuse — so a fast path that
/// silently stopped applying, or one that started swallowing a shape it should
/// have delegated, both go red.
///
/// Per THREAD — `cargo test` runs every case on its own thread in one process,
/// so a process-global counter would be moved by whatever else is running.
    static KEYS_ARRAY_SLOT_FALLBACKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_keys_array_slot_fallbacks() -> u64 {
    KEYS_ARRAY_SLOT_FALLBACKS.with(|c| c.get())
}
