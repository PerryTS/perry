//! Object-owned overflow storage ("spill") and the legacy thread-local
//! overflow side table.
//!
//! Split out of `object/mod.rs` (which had grown past the 2000-line CI cap).
//! Contents are unchanged; only item visibility was widened to `pub(crate)`
//! so the sibling `object::*` modules that already used these helpers via
//! `use super::*` keep resolving them through the re-exports in `object/mod.rs`.

use super::*;

// Last-accessed overflow Vec cache — one entry, keyed by `obj_ptr`.
// Skips the outer HashMap lookup on consecutive writes to the same
// object (exactly the row-build pattern: a single object gets its
// overflow slots filled back-to-back). Refreshed on every slow-path
// HashMap access; invalidated by `clear_overflow_for_ptr` when GC
// sweep frees the corresponding object.
//
// Safety: the cached pointer references the `Vec<u64>` struct stored
// inside a HashMap bucket. That struct only moves when the HashMap
// resizes, which only happens on `entry().or_default()` inserting a
// fresh key. The slow path below does both the potentially-resizing
// call and the cache refresh while holding the `overflow_fields`
// borrow, so no other thread-local mutation can interleave between
// obtaining `&mut Vec` and caching its address.
// (Storage: `ObjectHotTables::overflow_last`.)

// ---------------------------------------------------------------------------
// #6812: object-owned overflow storage ("spill").
//
// Default-on replacement for the thread-local `overflow_fields` side table:
// values past the inline alloc_limit live in a `GC_TYPE_ARRAY` buffer hung
// off the object's `ObjectMeta` record ([`ObjectMeta::spill`]). Reads are two
// dependent loads instead of a TLS fetch + RefCell + PtrHashMap probe, and
// GC integration is structural — the buffer is a traced child edge (object →
// meta → buffer → elements), so marking, evacuation rewriting, owner moves,
// and death all ride the ordinary object graph. The legacy side-table code
// below stays compiled for one release as a bisection escape hatch
// (`PERRY_OBJECT_SPILL=0`/`off`/`false`); its GC hooks are no-ops while the
// map stays empty.

#[inline]
pub(crate) fn object_spill_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_OBJECT_SPILL").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Raw in-range element access for the spill buffer. The buffer is a plain
/// `GC_TYPE_ARRAY` this module allocated itself, so the user-facing
/// `js_array_get`/`js_array_set` — which classify the receiver against the
/// typed-array/buffer/SAB registries on EVERY call (three TLS probes,
/// measured as the hot leaves of round-robin overflow writes) — are the
/// wrong tool. Store = raw slot write + layout note + generational barrier,
/// the exact triple the retired side-table Vec store performed.
#[inline]
unsafe fn spill_elements(spill: *const crate::array::ArrayHeader) -> *mut u64 {
    (spill as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
}

#[inline]
unsafe fn spill_store_slot(spill: *mut crate::array::ArrayHeader, index: usize, vbits: u64) {
    let slot = spill_elements(spill).add(index);
    *slot = vbits;
    // Length is the buffer's high-water mark: `js_array_alloc_with_length`
    // sets length = REQUESTED capacity while the physical capacity rounds up
    // (MIN_ARRAY_CAPACITY), and the in-capacity fast path stores past the
    // current length. Everything keys off length — `spill_get`'s bounds
    // check, the GC element range (a value past length is invisible to
    // marking/rewriting), and the growth copy — so extend it here. Slots
    // between the old and new length are TAG_HOLE from allocation.
    if index >= (*spill).length as usize {
        (*spill).length = (index + 1) as u32;
    }
    crate::gc::layout_note_slot(spill as usize, index, vbits);
    crate::gc::runtime_write_barrier_slot(spill as usize, slot as usize, vbits);
}

/// Only genuine shaped objects carry a meta record at the ObjectHeader
/// offset. Exotic GC_TYPE_OBJECT aliases (RegExpHeader) and every other
/// GC type (errors, maps, ...) have unrelated bytes there — the legacy
/// side table was address-keyed and safe for ANY owner, so those owners
/// keep it (in both modes) instead of deref'ing garbage. Classification
/// via the canonical header probe, mirroring `gc_object_meta_slot`.
#[inline]
pub(crate) unsafe fn spill_capable_owner(obj_ptr: usize) -> bool {
    if obj_ptr == 0 {
        return false;
    }
    match crate::value::addr_class::try_read_gc_header(obj_ptr) {
        Some(h) => {
            h.obj_type == crate::gc::GC_TYPE_OBJECT
                && !crate::regex::regex_header_has_magic(
                    obj_ptr as *const crate::regex::RegExpHeader,
                )
        }
        None => false,
    }
}

pub(crate) fn spill_get(obj_ptr: usize, field_index: usize) -> Option<u64> {
    unsafe {
        let obj = obj_ptr as *mut ObjectHeader;
        if (*obj).meta.is_null() {
            return None;
        }
        let spill = (*(*obj).meta).spill as *const crate::array::ArrayHeader;
        if spill.is_null() || field_index >= (*spill).length as usize {
            return None;
        }
        let bits = *spill_elements(spill).add(field_index);
        // Never-written positions are TAG_HOLE from allocation (or
        // TAG_UNDEFINED via the legacy-parity fillers); both report as
        // absent, matching the side-table Vec's TAG_UNDEFINED semantics.
        (bits != crate::value::TAG_UNDEFINED && bits != crate::value::TAG_HOLE).then_some(bits)
    }
}

pub(crate) fn spill_set(obj_ptr: usize, field_index: usize, vbits: u64) {
    unsafe {
        let obj = obj_ptr as *mut ObjectHeader;
        // Hot path: meta and buffer already exist with capacity — the
        // in-range barriered store cannot allocate or move anything, so no
        // handle scope is needed. This is every write after the first to a
        // given width (e.g. round-robin updates across an object array).
        let meta = (*obj).meta;
        if !meta.is_null() {
            let spill = (*meta).spill as *mut crate::array::ArrayHeader;
            if !spill.is_null() && ((*spill).capacity as usize) > field_index {
                // Learn the class's true width so FUTURE instances allocate
                // it inline (same hook as the legacy path) — but only when
                // this write raises the buffer's high-water mark. Steady-
                // state writes (index < length: the round-robin update
                // pattern this fast path exists for) skip the TLS probe
                // entirely; the learned maximum is identical because every
                // first write to a new index passes this gate (or the slow
                // path below) with the same `field_index + 1`.
                if field_index >= (*spill).length as usize {
                    note_learned_inline_fields(
                        (*obj).class_id,
                        (field_index as u32).saturating_add(1),
                    );
                }
                spill_store_slot(spill, field_index, vbits);
                return;
            }
        }
        spill_set_slow(obj_ptr, field_index, vbits);
    }
}

/// Reserve exact-width overflow storage for a freshly allocated object whose
/// final key count is already known.
///
/// Dynamic objects normally discover their width one property at a time, so
/// the first overflow store uses an array with general-purpose growth
/// headroom. JSON tapes already encode the matching container boundary and can
/// count an object's top-level keys before materializing it. Keeping the
/// primary object at [`INLINE_SLOT_FLOOR`] avoids the measured regression from
/// widening every object, while an exact spill avoids padding every record's
/// side allocation to [`crate::array::MIN_ARRAY_CAPACITY`].
pub(crate) fn reserve_object_spill(obj_ptr: usize, field_count: u32) {
    if !object_spill_enabled()
        || field_count as usize > SPILL_MAX_FIELD_INDEX
        || unsafe { !spill_capable_owner(obj_ptr) }
    {
        return;
    }

    unsafe {
        let obj = obj_ptr as *mut ObjectHeader;
        let inline_capacity =
            std::cmp::max((*obj).field_count, crate::object::INLINE_SLOT_FLOOR as u32);
        if field_count <= inline_capacity {
            return;
        }

        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        object_meta_ensure(obj);

        let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let meta = (*obj).meta;
        if (*meta).spill != 0 {
            return;
        }

        let spill = crate::array::js_array_alloc_with_length_exact(field_count);
        let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let meta = (*obj).meta;
        if (*meta).spill == 0 {
            (*meta).spill = spill as u64;
            crate::gc::runtime_write_barrier_slot(
                meta as usize,
                &(*meta).spill as *const _ as usize,
                spill as u64,
            );
        }
    }
}

#[cfg(test)]
pub(crate) type SpillSafepointHook = fn(usize);

#[cfg(test)]
thread_local! {
    static SPILL_SAFEPOINT_HOOK: std::cell::Cell<Option<SpillSafepointHook>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn test_set_spill_safepoint_hook(
    hook: Option<SpillSafepointHook>,
) -> Option<SpillSafepointHook> {
    SPILL_SAFEPOINT_HOOK.with(|slot| {
        let previous = slot.get();
        slot.set(hook);
        previous
    })
}

#[cfg(test)]
#[inline]
fn spill_safepoint(obj_ptr: usize) {
    SPILL_SAFEPOINT_HOOK.with(|slot| {
        if let Some(hook) = slot.get() {
            hook(obj_ptr);
        }
    });
}

#[cfg(not(test))]
#[inline]
fn spill_safepoint(_obj_ptr: usize) {}

/// Allocation path: ensure the meta record and a buffer wide enough for
/// `field_index`, then store. Roots the owner AND the incoming value across
/// the allocations.
#[cold]
fn spill_set_slow(obj_ptr: usize, field_index: usize, vbits: u64) {
    unsafe {
        let obj = obj_ptr as *mut ObjectHeader;
        // First write at this width for this object — record the class's
        // high-water mark (the fast path only records when growing an
        // existing buffer's length).
        note_learned_inline_fields((*obj).class_id, (field_index as u32).saturating_add(1));
        // Root the owner: meta/buffer allocation below can trigger a moving
        // minor GC. Reload through the handle after every allocation.
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        // #7538: root the VALUE too. It arrives as plain `u64` bits — a
        // by-value copy of a NaN-boxed pointer the caller may well have
        // rooted, which does the callee no good: `object_meta_ensure` and
        // `js_array_alloc_with_length` below are both collection points, and
        // an evacuating minor at either one rewrites the caller's handle
        // while this local keeps naming the from-space copy. The store at the
        // end then publishes that address into a slot the collector has
        // already finished rewriting, so the stale pointer is never fixed and
        // never reported (its target is live, just relocated). Re-read the
        // bits from the handle immediately before the store.
        let value_handle = scope.root_nanbox_u64(vbits);
        object_meta_ensure(obj);
        // Test-only stand-in for the collection `object_meta_ensure` (and the
        // buffer allocation below) can genuinely take. Same pattern, and same
        // reason, as `json_tape::json_tape_safepoint`: the window is one
        // allocation wide, so a test that waits for the arena trigger to land
        // in it is a coin flip. Compiles to nothing outside `cfg(test)`.
        spill_safepoint(obj_ptr);
        let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let meta = (*obj).meta;
        let spill = (*meta).spill as *mut crate::array::ArrayHeader;
        let needed = field_index + 1;
        if spill.is_null() || ((*spill).capacity as usize) < needed {
            // In range by the SPILL_MAX_FIELD_INDEX dispatch gate, so the
            // conversion is exact (2^24 max); arena allocation panics on
            // genuine OOM (after one emergency reclaim) and never returns
            // null, so the store below always has a live buffer.
            let new_cap =
                u32::try_from(needed.next_power_of_two().max(8)).expect("bounded by dispatch gate");
            // length == capacity and every slot TAG_HOLE from birth, so the
            // GC element range covers the whole buffer and in-range
            // `js_array_set` can never trigger array growth/forwarding —
            // `meta.spill` always points at the live block (GC rewrites it
            // as a child edge on evacuation).
            let new_spill = crate::array::js_array_alloc_with_length(new_cap);
            let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            let meta = (*obj).meta;
            let old = (*meta).spill as *const crate::array::ArrayHeader;
            if !old.is_null() {
                let old_len = (*old).length as usize;
                let elements = (old as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                for i in 0..old_len {
                    let bits = *elements.add(i);
                    if bits != crate::value::TAG_HOLE && bits != crate::value::TAG_UNDEFINED {
                        // In range by construction (old_len <= old cap < new_cap).
                        spill_store_slot(new_spill, i, bits);
                    }
                }
            }
            // GC_STORE_AUDIT(BARRIERED): meta-record slot store + barrier,
            // mirroring the `header.meta` edge install.
            (*meta).spill = new_spill as u64;
            crate::gc::runtime_write_barrier_slot(
                meta as usize,
                &(*meta).spill as *const _ as usize,
                new_spill as u64,
            );
        }
        let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let meta = (*obj).meta;
        let spill = (*meta).spill as *mut crate::array::ArrayHeader;
        spill_store_slot(spill, field_index, value_handle.get_nanbox_u64());
    }
}

/// Read the u64 bits stored at `field_index` for `obj`, or `None` if absent.
/// Positions never written are stored as `TAG_UNDEFINED`; this helper reports
/// them as `None` so callers can return JS `undefined` uniformly with the
/// "no Vec entry at all" case.
/// Spill indices share the runtime's canonical 16M field ceiling (the
/// same cap `layout_note_slot` and the write-loop guard enforce); a larger
/// index would need a >128MB buffer for one property, so it stays on the
/// address-keyed legacy table like exotic owners do.
pub(crate) const SPILL_MAX_FIELD_INDEX: usize = 16_000_000;

#[inline]
pub(crate) fn overflow_get(obj_ptr: usize, field_index: usize) -> Option<u64> {
    if object_spill_enabled()
        && field_index < SPILL_MAX_FIELD_INDEX
        && unsafe { spill_capable_owner(obj_ptr) }
    {
        return spill_get(obj_ptr, field_index);
    }
    crate::state::state()
        .object_hot
        .overflow_fields
        .borrow()
        .get(&obj_ptr)
        .and_then(|v| v.get(field_index).copied())
        .filter(|&bits| bits != crate::value::TAG_UNDEFINED)
}

/// Write `vbits` to the overflow slot `field_index` for `obj`. Grows the
/// per-object `Vec` to `field_index + 1` with `TAG_UNDEFINED` fillers if
/// needed (filler slots correspond to the object's inline region and are
/// never read).
///
/// Fast path skips the outer HashMap when `obj_ptr` matches the last-
/// Learned per-class inline sizing: the dynamic-construct path allocates 8
/// inline slots (it cannot see the constructor body), so a 23-field
/// ES5-pattern object keeps 15 fields in [`OVERFLOW_FIELDS`] — a `Vec<u64>`
/// plus map entry per object (~250B, more than the object payload), visited,
/// rekeyed and finalized by every GC cycle. The FIRST instance that
/// overflows records its class's high-water field index here; every LATER
/// `new` of the same (synthetic or registered) class right-sizes its
/// allocation so all fields land inline. Capped so a pathological dynamic
/// writer can't inflate every future instance.
const LEARNED_INLINE_MAX_FIELDS: u32 = 64;
const LEARNED_INLINE_TABLE_SIZE: usize = 1024;

thread_local! {
    static LEARNED_INLINE_FIELDS: std::cell::UnsafeCell<[(u32, u32); LEARNED_INLINE_TABLE_SIZE]> =
        const { std::cell::UnsafeCell::new([(0u32, 0u32); LEARNED_INLINE_TABLE_SIZE]) };
}

type LearnedInlineTable = std::cell::UnsafeCell<[(u32, u32); LEARNED_INLINE_TABLE_SIZE]>;

/// Address of this thread's `LEARNED_INLINE_FIELDS`. See `crate::tls_hot`.
pub(crate) fn learned_inline_fields_hot_addr() -> *mut u8 {
    LEARNED_INLINE_FIELDS.with(|t| t as *const _ as *mut u8)
}

/// `LEARNED_INLINE_FIELDS` without a TLS resolution.
///
/// [`learned_inline_field_count`] runs on every dynamic construct, so this was
/// the last thread-local left on `churn_alloc`'s allocation path after #7469's
/// structural half: 100% of the residual `_tlv_get_addr` samples attributed to
/// `js_object_alloc_class_inline_keys`, which is this read.
#[inline(always)]
fn hot_learned_inline_fields() -> &'static LearnedInlineTable {
    // SAFETY: paired with `learned_inline_fields_hot_addr` above, and asserted
    // by `tls_hot::tests::cached_addresses_match_thread_locals`.
    unsafe { &*(crate::tls_hot::hot().learned_inline_fields as *const LearnedInlineTable) }
}

#[inline]
fn note_learned_inline_fields(class_id: u32, needed_fields: u32) {
    if class_id == 0 || needed_fields > LEARNED_INLINE_MAX_FIELDS {
        return;
    }
    let slot = (class_id as usize).wrapping_mul(0x9E37_79B1) % LEARNED_INLINE_TABLE_SIZE;
    let t = hot_learned_inline_fields();
    // SAFETY: the table is this thread's own storage and the runtime is
    // single-threaded per arena; `slot` is reduced modulo the table size.
    unsafe {
        let e = &mut (*t.get())[slot];
        if e.0 != class_id {
            *e = (class_id, needed_fields);
        } else if e.1 < needed_fields {
            e.1 = needed_fields;
        }
    }
}

/// Inline field count to pre-size a dynamic construct of `class_id` with —
/// the learned high-water mark, or 0 when nothing was learned (caller keeps
/// its default).
#[inline]
pub(crate) fn learned_inline_field_count(class_id: u32) -> u32 {
    // Bisection kill-switch: PERRY_LEARNED_INLINE=0 disables consumption.
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_LEARNED_INLINE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    }) {
        return 0;
    }
    if class_id == 0 {
        return 0;
    }
    let slot = (class_id as usize).wrapping_mul(0x9E37_79B1) % LEARNED_INLINE_TABLE_SIZE;
    let t = hot_learned_inline_fields();
    // SAFETY: as in `note_learned_inline_fields` above.
    let e = unsafe { (*t.get())[slot] };
    if e.0 == class_id {
        e.1
    } else {
        0
    }
}

/// accessed Vec — the common row-build pattern where an object's
/// overflow slots fill in sequence.
#[inline]
pub(crate) fn overflow_set(obj_ptr: usize, field_index: usize, vbits: u64) {
    if object_spill_enabled()
        && field_index < SPILL_MAX_FIELD_INDEX
        && unsafe { spill_capable_owner(obj_ptr) }
    {
        return spill_set(obj_ptr, field_index, vbits);
    }
    // Learn the class's true width so FUTURE instances allocate it inline.
    unsafe {
        let hdr = obj_ptr as *const ObjectHeader;
        note_learned_inline_fields((*hdr).class_id, (field_index as u32).saturating_add(1));
    }
    let st = crate::state::state();
    let cached_slot = unsafe {
        let (cached_obj, cached_vec) = st.object_hot.overflow_last.get();
        if cached_obj == obj_ptr && !cached_vec.is_null() {
            let v = &mut *cached_vec;
            if v.len() <= field_index {
                v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
            }
            let slot = v.get_unchecked_mut(field_index);
            *slot = vbits;
            Some(slot as *mut u64 as usize)
        } else {
            None
        }
    };
    if let Some(slot_addr) = cached_slot {
        crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
        crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, vbits);
        return;
    }
    let slot_addr;
    {
        let mut map = st.object_hot.overflow_fields.borrow_mut();
        let v = map.entry(obj_ptr).or_default();
        if v.len() <= field_index {
            v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
        }
        v[field_index] = vbits;
        slot_addr = (&mut v[field_index]) as *mut u64 as usize;
        let vec_ptr = v as *mut Vec<u64>;
        st.object_hot.overflow_last.set((obj_ptr, vec_ptr));
    }
    crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
    crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, vbits);
}
