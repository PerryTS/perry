//! Object-owned overflow storage ("spill") and the legacy thread-local
//! overflow side table.
//!
//! Split out of `object/mod.rs` (which had grown past the 2000-line CI cap).
//! Contents are unchanged; only item visibility was widened to `pub(crate)`
//! so the sibling `object::*` modules that already used these helpers via
//! `use super::*` keep resolving them through the re-exports in `object/mod.rs`.

use super::*;

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_LAYOUT_NOTE_SLOT_CALLS: Cell<usize> = const { Cell::new(0) };
}

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
    *crate::once_init::get_or_init(&ON, || {
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
    crate::array::array_elements_ptr(spill as *const crate::array::ArrayHeader) as *mut u64
}

#[inline]
unsafe fn spill_store_slot(spill: *mut crate::array::ArrayHeader, index: usize, vbits: u64) {
    let elements = spill_elements(spill);
    let slot = elements.add(index);
    let length = (*spill).length as usize;
    // Length is the buffer's high-water mark: `js_array_alloc_with_length`
    // sets length = REQUESTED capacity while the physical capacity rounds up
    // (MIN_ARRAY_CAPACITY), and the in-capacity fast path stores past the
    // current length. Everything keys off length — `spill_get`'s bounds
    // check, the GC element range (a value past length is invisible to
    // marking/rewriting), and the growth copy — so extend it here.
    //
    // #11550: a slot at or past `length` holds NO value. The allocator
    // initializes only the requested prefix, so the rounded-up tail still
    // holds whatever that arena memory last held — very often a NaN-boxed
    // heap pointer from a dead object. Reading it as the "old value" made a
    // first store of a pointer look like a pointer-over-pointer overwrite,
    // which `layout_note_slot_aware` answers WITHOUT setting the slot's GC
    // mask bit. The collector then skipped the slot: the stored value was
    // neither marked nor rewritten, and the object kept a from-space address
    // (qs's `{ __proto__: null }` accumulator, 12 keys, 10 of them spilled).
    // Treat the tail as holes, and hole-fill any gap the new length exposes,
    // so the element range the GC walks never contains stale bits.
    let old_bits = if index < length {
        *slot
    } else {
        for gap in length..index {
            // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is a non-pointer
            // sentinel for a never-written spill slot.
            elements.add(gap).write(crate::value::TAG_HOLE);
        }
        (*spill).length = (index + 1) as u32;
        crate::value::TAG_HOLE
    };
    // GC_STORE_AUDIT(BARRIERED): layout note + slot barrier below.
    *slot = vbits;
    // Spill elements are boxed JS values just like ordinary array slots. The
    // old value is already in hand, so preserve the same overwrite invariant
    // as array stores: scalar -> scalar and pointer -> pointer cannot change
    // the GC slot mask and need no full layout note. Transitions in either
    // direction still take the complete path below.
    crate::gc::layout_note_slot_aware(spill as usize, index, vbits, old_bits);
    crate::gc::runtime_write_barrier_slot(spill as usize, slot as usize, vbits);
}

/// Only genuine shaped objects carry a meta record at the ObjectHeader offset.
/// Every other GC type (RegExp, errors, maps, ...) has unrelated bytes there —
/// the legacy side table was address-keyed and safe for ANY owner, so those
/// owners keep it instead of dereferencing garbage. Classification uses the
/// canonical GcHeader kind, mirroring `gc_object_meta_slot`.
#[inline]
pub(crate) unsafe fn spill_capable_owner(obj_ptr: usize) -> bool {
    if obj_ptr == 0 {
        return false;
    }
    match crate::value::addr_class::try_read_gc_header(obj_ptr) {
        Some(h) => h.obj_type == crate::gc::GC_TYPE_OBJECT,
        None => false,
    }
}

/// The stored bits of a spill-located key, INCLUDING a stored `undefined`,
/// or `None` when the position has no storage (no meta record, no buffer,
/// past its length, a `TAG_HOLE`) or the value does not live in object-owned
/// spill storage at all (spill disabled, index past
/// [`SPILL_MAX_FIELD_INDEX`], an exotic owner).
///
/// This is the read the emitted `pic.spill.hit` performs, with its checks
/// spelled out: `Some` here is exactly "the compact word may name this
/// receiver's ShapeId flipped". [`overflow_get`] keeps the legacy table's
/// convention that `undefined` reads as absent, which cost every read of an
/// own spill key holding `undefined` the full by-name walk — its site could
/// never prime.
pub(crate) fn spill_get_present(obj_ptr: usize, field_index: usize) -> Option<u64> {
    if !object_spill_enabled()
        || field_index >= SPILL_MAX_FIELD_INDEX
        || unsafe { !spill_capable_owner(obj_ptr) }
    {
        return None;
    }
    unsafe {
        let obj = obj_ptr as *const ObjectHeader;
        let meta = (*obj).meta;
        if meta.is_null() {
            return None;
        }
        let spill = (*meta).spill as *const crate::array::ArrayHeader;
        if spill.is_null() || field_index >= (*spill).length as usize {
            return None;
        }
        let bits = *spill_elements(spill).add(field_index);
        (bits != crate::value::TAG_HOLE).then_some(bits)
    }
}

/// Give a spill-located key that was CLAIMED without a value (an accessor
/// install, a generic descriptor, a keys list installed wholesale) real
/// storage holding `undefined`.
///
/// # Why a keys-only claim must reserve
///
/// A ShapeId names the key list and the live inline-slot bound, so it fixes
/// every key's position; a position at or past the bound lives in the spill
/// buffer at that same index. The emitted spill read loads
/// `meta -> spill -> [index]` on nothing but a ShapeId match, so EVERY
/// carrier of a shape with a spill-located key must have that storage — not
/// just the one that primed the site. A claim that grows the key list without
/// writing a value produces the same ShapeId as a data write of the same key
/// (the canonical list is the same), so without this it would carry the
/// shape and no storage.
///
/// No-op when the position already has storage, and when the key's value
/// does not live in spill storage (spill disabled, an exotic owner): the
/// prime never publishes a spill entry for those (see
/// [`spill_get_present`]).
///
/// May allocate (meta record, buffer): the caller must hold `obj_ptr` in a
/// handle and re-read it afterwards.
pub(crate) fn spill_reserve_claimed(obj_ptr: usize, field_index: usize) {
    if !object_spill_enabled()
        || field_index >= SPILL_MAX_FIELD_INDEX
        || unsafe { !spill_capable_owner(obj_ptr) }
    {
        return;
    }
    if spill_get_present(obj_ptr, field_index).is_none() {
        spill_set(obj_ptr, field_index, crate::value::TAG_UNDEFINED);
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

/// True when a spill write at `field_index` would take `spill_set`'s
/// in-capacity hot path — meta and buffer exist and the index is within the
/// buffer's physical capacity — i.e. the write cannot grow, allocate on the
/// GC heap, or move anything. The emitted transition-IC's spill-append helper
/// gates on this so it stays a non-collecting leaf; growth falls back to the
/// full miss path, which is built for it.
pub(crate) fn spill_store_would_be_in_capacity(obj_ptr: usize, field_index: usize) -> bool {
    unsafe {
        let obj = obj_ptr as *const ObjectHeader;
        let meta = (*obj).meta;
        if meta.is_null() {
            return false;
        }
        let spill = (*meta).spill as *const crate::array::ArrayHeader;
        !spill.is_null() && ((*spill).capacity as usize) > field_index
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
        // The physical write below belongs to the spill Array, so its layout
        // note cannot identify the owning Array-subclass object. Retire that
        // owner's cached numeric-prefix proof before changing any spill slot.
        crate::array::note_packed_subclass_spill_store(obj, meta);
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
                        obj as usize,
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
        let inline_capacity = std::cmp::max(
            crate::object::object_live_slot_count(obj),
            crate::object::INLINE_SLOT_FLOOR as u32,
        );
        if field_count <= inline_capacity {
            return;
        }

        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let (_, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| object_meta_ensure(obj));
        let meta = (*obj).meta;
        if (*meta).spill != 0 {
            return;
        }

        let (spill, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
            crate::array::js_array_alloc_with_length_exact(field_count)
        });
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
        note_learned_inline_fields(
            obj_ptr,
            (*obj).class_id,
            (field_index as u32).saturating_add(1),
        );
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
            let (new_spill, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
                crate::array::js_array_alloc_with_length(new_cap)
            });
            let meta = (*obj).meta;
            let old = (*meta).spill as *const crate::array::ArrayHeader;
            if !old.is_null() {
                let old_len = (*old).length as usize;
                let elements =
                    crate::array::array_elements_ptr(old as *const crate::array::ArrayHeader)
                        as *const u64;
                for i in 0..old_len {
                    let bits = *elements.add(i);
                    // S5: a stored `undefined` is a VALUE and is carried over.
                    // Dropping it left the new buffer's slot `TAG_HOLE`, and
                    // the emitted spill read (`pic.spill.hit`) loads a spill
                    // slot with no hole test — its ShapeId proves the key is
                    // present, so the slot must hold its value.
                    if bits != crate::value::TAG_HOLE {
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
fn note_learned_inline_fields(obj_ptr: usize, class_id: u32, needed_fields: u32) {
    // #10905: a spill also teaches the object's keyless birth shape, if it has
    // one, how wide its descendants grow (`shapes_birth_width`).
    // SAFETY: every caller passes a live shaped object it is storing into.
    unsafe {
        crate::object::shapes::note_spill_width(obj_ptr as *const ObjectHeader, needed_fields)
    };
    if class_id == 0 || needed_fields > LEARNED_INLINE_MAX_FIELDS {
        return;
    }
    // Declared and function-class prototype objects carry the INSTANCE class
    // id so reflection and prototype dispatch can recover their owner. Their
    // own storage width is unrelated to an instance's field layout: a class
    // with eight instance fields can easily have dozens of prototype methods.
    // Learning from that spill makes later instances allocate at the
    // prototype width and stamps a different birth ShapeId, permanently
    // defeating exact-shape field/method guards. Both prototype registries are
    // keyed by the same class id, so pointer equality is an exact, O(1) veto.
    let obj = obj_ptr as *mut ObjectHeader;
    if crate::object::class_decl_prototype_object(class_id) == obj
        || crate::object::class_prototype_object(class_id) == obj
    {
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
    if !*crate::once_init::get_or_init(&ON, || {
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
    unsafe {
        crate::object::proto_validity::note_marked_value_write(obj_ptr as *const ObjectHeader)
    };
    if object_spill_enabled()
        && field_index < SPILL_MAX_FIELD_INDEX
        && unsafe { spill_capable_owner(obj_ptr) }
    {
        return spill_set(obj_ptr, field_index, vbits);
    }
    // Learn the class's true width so FUTURE instances allocate it inline.
    unsafe {
        let hdr = obj_ptr as *const ObjectHeader;
        note_learned_inline_fields(
            obj_ptr,
            (*hdr).class_id,
            (field_index as u32).saturating_add(1),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_spills_do_not_teach_instance_inline_width() {
        let _lock = crate::gc::global_side_table_test_lock();
        const DECLARED_CID: u32 = 0x6B45_5A11;
        const FUNCTION_CID: u32 = 0x6B45_5A12;

        let declared_proto = js_object_alloc(DECLARED_CID, 0);
        crate::object::class_decl_prototype_object_root_store(DECLARED_CID, declared_proto);
        overflow_set(declared_proto as usize, 12, crate::value::TAG_UNDEFINED);
        assert_eq!(
            learned_inline_field_count(DECLARED_CID),
            0,
            "declared Class.prototype storage is not an instance-layout sample"
        );

        let function_proto = js_object_alloc(FUNCTION_CID, 0);
        crate::object::class_prototype_object_root_store(FUNCTION_CID, function_proto);
        overflow_set(function_proto as usize, 15, crate::value::TAG_UNDEFINED);
        assert_eq!(
            learned_inline_field_count(FUNCTION_CID),
            0,
            "function prototype storage is not an instance-layout sample"
        );

        let instance = js_object_alloc(DECLARED_CID, 0);
        overflow_set(instance as usize, 12, crate::value::TAG_UNDEFINED);
        assert_eq!(
            learned_inline_field_count(DECLARED_CID),
            13,
            "a real instance overflow must still teach the class high-water mark"
        );
    }

    #[test]
    fn spill_overwrites_only_note_pointer_kind_transitions() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _trigger_guard = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let owner = js_object_alloc(0x6B45_5A13, 0);
        let slot = 18;

        TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(0));
        spill_set(owner as usize, slot, 1.0f64.to_bits());
        assert_eq!(TEST_LAYOUT_NOTE_SLOT_CALLS.with(Cell::get), 0);

        TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(0));
        spill_set(owner as usize, slot, 2.0f64.to_bits());
        assert_eq!(
            TEST_LAYOUT_NOTE_SLOT_CALLS.with(Cell::get),
            0,
            "a scalar overwrite must not enter the full layout hook"
        );

        let child_a = js_object_alloc(0x6B45_5A14, 0);
        let child_a_bits =
            crate::value::POINTER_TAG | (child_a as u64 & crate::value::POINTER_MASK);
        TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(0));
        spill_set(owner as usize, slot, child_a_bits);
        assert_eq!(
            TEST_LAYOUT_NOTE_SLOT_CALLS.with(Cell::get),
            1,
            "a scalar-to-pointer transition must update the slot layout"
        );

        let spill = crate::object::test_spill_buffer_addr(owner as usize);
        assert_eq!(
            crate::gc::test_layout_pointer_slot_count(spill, slot + 1),
            Some(1)
        );

        let child_b = js_object_alloc(0x6B45_5A15, 0);
        let child_b_bits =
            crate::value::POINTER_TAG | (child_b as u64 & crate::value::POINTER_MASK);
        TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(0));
        spill_set(owner as usize, slot, child_b_bits);
        assert_eq!(
            TEST_LAYOUT_NOTE_SLOT_CALLS.with(Cell::get),
            0,
            "a pointer overwrite must preserve the existing mask bit"
        );
        assert_eq!(
            crate::gc::test_layout_pointer_slot_count(spill, slot + 1),
            Some(1)
        );

        TEST_LAYOUT_NOTE_SLOT_CALLS.with(|calls| calls.set(0));
        spill_set(owner as usize, slot, 3.0f64.to_bits());
        assert_eq!(
            TEST_LAYOUT_NOTE_SLOT_CALLS.with(Cell::get),
            1,
            "a pointer-to-scalar transition must clear the slot layout"
        );
        assert_eq!(
            crate::gc::test_layout_pointer_slot_count(spill, slot + 1),
            Some(0)
        );
    }

    /// #11559: a spill store at or past the buffer's high-water mark must not
    /// read the headroom word as the value it overwrites.
    ///
    /// `js_array_alloc_with_length(8)` initializes eight `TAG_HOLE` slots in a
    /// sixteen-slot allocation; the other eight hold whatever the memory held
    /// before. The poison below stands in for that previous tenant: a live
    /// pointer, so it is exactly the pointer-shaped word that made the
    /// pointer-over-pointer layout shortcut skip the note. The assertion is
    /// the collector's own question — does it enumerate (and so mark and
    /// rewrite) the slot the new child lives in?
    #[test]
    fn spill_store_past_high_water_ignores_headroom_bits() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _trigger_guard = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let owner = js_object_alloc(0x6B45_5A16, 0);
        let first = js_object_alloc(0x6B45_5A17, 0);
        let first_bits = crate::value::POINTER_TAG | (first as u64 & crate::value::POINTER_MASK);
        spill_set(owner as usize, 2, first_bits);

        let spill = crate::object::test_spill_buffer_addr(owner as usize);
        let header = spill as *mut crate::array::ArrayHeader;
        let (length, capacity) =
            unsafe { ((*header).length as usize, (*header).capacity as usize) };
        assert_eq!(
            length, 8,
            "fixture: the first spill buffer requests 8 slots"
        );
        assert!(
            capacity > 9,
            "fixture: index 9 must take the in-capacity path past the high-water mark"
        );

        let stale = js_object_alloc(0x6B45_5A18, 0);
        let stale_bits = crate::value::POINTER_TAG | (stale as u64 & crate::value::POINTER_MASK);
        unsafe {
            let elements = spill_elements(header);
            for i in length..capacity {
                // GC_STORE_AUDIT(INIT): test poison past `length`, standing in
                // for a previous tenant's leftover word; nothing reads it as a value.
                *elements.add(i) = stale_bits;
            }
        }

        let child = js_object_alloc(0x6B45_5A19, 0);
        let child_bits = crate::value::POINTER_TAG | (child as u64 & crate::value::POINTER_MASK);
        spill_set(owner as usize, 9, child_bits);

        assert_eq!(
            crate::object::test_spill_buffer_addr(owner as usize),
            spill,
            "fixture: the store must not have grown the buffer"
        );
        let slot_addr = unsafe { spill_elements(header).add(9) as usize };
        let rewrite = crate::gc::test_gc_rewrite_slot_addresses(spill).unwrap();
        assert!(
            rewrite.contains(&slot_addr),
            "the collector must enumerate a pointer stored past the high-water mark"
        );
        let gap_addr = unsafe { spill_elements(header).add(8) as usize };
        assert_eq!(
            unsafe { *(gap_addr as *const u64) },
            crate::value::TAG_HOLE,
            "the gap the store brings inside `length` must not expose the headroom word"
        );
        assert_eq!(unsafe { (*header).length }, 10);
    }
}
