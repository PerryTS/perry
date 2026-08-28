//! Indexing support split out of `indexing.rs` to keep it under the repo's
//! 2000-line cap: the strict-store TypeError throwers, the prototype
//! indexed-property / iterator invalidation latches, and the dense keys-array
//! slot helpers. Pure move except for the `use` lines and `pub(super)`
//! visibility on items `indexing.rs` still calls.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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

/// Sticky summary of the process-wide conditions that invalidate codegen's
/// inline plain-array index guard. The generated guard loads this byte
/// directly; keeping the three rare prototype conditions behind one exported
/// byte avoids an out-of-line runtime call on every array read.
#[no_mangle]
pub static PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED: AtomicU8 = AtomicU8::new(0);

#[inline]
pub(crate) fn invalidate_array_index_fast_path() {
    PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.store(1, Ordering::Relaxed);
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

/// Sticky flag: user code replaced or deleted `Array.prototype[Symbol.iterator]`.
/// `js_get_iterator`'s array short-circuit assumes the builtin values iterator;
/// once this flips, GetIterator on an array must consult the (patched) method
/// per spec — or throw TypeError when it was deleted. Same single-relaxed-load
/// hot-path shape as `ARRAY_PROTO_HAS_INDEX` above.
pub(super) static ARRAY_PROTO_ITERATOR_MODIFIED: AtomicBool = AtomicBool::new(false);

/// The same fact as [`ARRAY_PROTO_ITERATOR_MODIFIED`], exported so GENERATED
/// code can read it (#7760 item 1).
///
/// `for…of` over a statically-proven array desugars to an index loop
/// (`__i < __arr.length` / `__arr[__i]`) in HIR lowering, which never consults
/// the iteration protocol — so a patched `Array.prototype[Symbol.iterator]` was
/// ignored there even after the spread paths were fixed (#7542). The loop now
/// branches on this flag ONCE at entry, which is also what the spec wants:
/// `for…of` performs GetIterator exactly once, so a patch landing mid-loop must
/// not change the iterator already in hand.
///
/// A separate `u8` global rather than exposing the `AtomicBool`: codegen emits
/// a plain volatile `i8` load, the same shape as
/// `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`, so the fast arm pays one load and
/// a predictable branch per LOOP — not per iteration — and the index loop
/// itself is emitted byte-identically to before.
#[no_mangle]
pub static PERRY_ARRAY_PROTO_ITERATOR_PATCHED: AtomicU8 = AtomicU8::new(0);

/// Record (if `obj` is `Array.prototype` and `sym_key` is the well-known
/// `Symbol.iterator`) that the array iteration protocol has been tampered
/// with. Called from the symbol-property set/delete paths.
pub(crate) fn note_array_proto_iterator_write(obj: usize, sym_key: usize) {
    if ARRAY_PROTO_ITERATOR_MODIFIED.load(Ordering::Relaxed) || obj == 0 || sym_key == 0 {
        return;
    }
    if obj == array_prototype_addr()
        && sym_key == crate::symbol::well_known_symbol("iterator") as usize
    {
        ARRAY_PROTO_ITERATOR_MODIFIED.store(true, Ordering::Relaxed);
        // Publish to generated code. Release so a loop that observes the `1`
        // also observes the prototype write that preceded it.
        PERRY_ARRAY_PROTO_ITERATOR_PATCHED.store(1, Ordering::Release);
    }
}

pub(crate) fn array_proto_iterator_modified() -> bool {
    ARRAY_PROTO_ITERATOR_MODIFIED.load(Ordering::Relaxed)
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
        if header.obj_type == crate::gc::GC_TYPE_ARRAY
            && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            && (*arr).length <= (*arr).capacity
        {
            return (*arr).length as usize;
        }
    }
    // A forwarding stub overwrites the old payload's `(length, capacity)`
    // words with the target address. Resolve once, then read BOTH facts from
    // the live header; mixing a resolved length with the stale from-space
    // capacity can truncate an otherwise exact shape count.
    let live = clean_arr_ptr(arr);
    if live.is_null() {
        return js_array_length(arr) as usize;
    }
    let raw = js_array_length(live) as usize;
    raw.min((*live).capacity as usize)
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
            && header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0
            && index < (*keys).length
            && index < (*keys).capacity
        {
            let elements =
                (keys as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
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
