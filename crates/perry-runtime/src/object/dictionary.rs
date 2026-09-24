//! Dictionary mode: an object that carries its own key list instead of
//! pointing at a layout the shape table owns (#10868 step 2.5, stage 1).
//!
//! # Why this exists
//!
//! Today 97.8 % of shape records are retired, because a private shape dies
//! with the one object that carries it. Step 2.5 makes shape identity
//! canonical: equal layouts intern to one shared `ShapeId`. A shared record
//! cannot be retired by ownership, so a workload that produces unboundedly
//! many distinct key lists — a `Map`-like object built by name with thousands
//! of keys, a per-request object keyed by user input — would accumulate
//! interned shapes for the life of the process: one ShapeId and one trie node
//! per key it ever added. (The storage is linear: the lists of a growth chain
//! share one canonical backing, which a tip append grows in place.)
//!
//! Dictionary mode bounds that. An object whose keys have stopped being worth
//! interning keeps them itself, and stops minting shapes for them.
//!
//! # The representation, and why the shape stays honest
//!
//! A dictionary-mode object is one whose ShapeId describes **no keys at all**:
//!
//! ```text
//! keys = NULL, logical_key_count = 0, hole_count = 0,
//! live_inline_slot_count = the inline bound, object_kind = Ordinary,
//! semantic_generation = a draw from the DICTIONARY namespace below
//! ```
//!
//! and whose real ordered key list is a private, object-owned `GC_TYPE_ARRAY`
//! in [`ObjectMeta::dictionary_keys`]. **Values do not move**: the key at
//! position *i* still reads inline slot *i* below the live bound and the
//! object-owned spill buffer at or above it, exactly as for any other object.
//! Dictionary mode changes where the *names* live, never where the *values*
//! live, which is what lets the existing read, write, delete and enumeration
//! code work on a dictionary object unmodified.
//!
//! The alternative — leaving the object's old key list published on its shape
//! and treating the meta record as an overlay — was rejected. A shape that
//! claims a key list the object no longer matches is a **silent wrong value**
//! in every consumer that trusts it. A shape that claims *nothing* is merely
//! incomplete, and the failure mode of a consumer nobody branched is a missing
//! property, which a differential test against node catches on the first row.
//! `keys = NULL` with a nonzero live bound is an existing, legal fact
//! combination (`shape_descriptor_ensure(std::ptr::null(), 0, field_count)`);
//! `shape_descriptor_ensure_with_holes` rejects only the opposite pairing.
//!
//! # The one branch that does the work
//!
//! [`crate::object::object_keys_array`] is the *sole* runtime derivation of an
//! object's ordered key list — every enumeration walk, the `in`/`hasOwn`
//! predicate, `delete`, `JSON.stringify`, spread and `Object.assign` reach it
//! through that one function. Branching it when the shape publishes no keys
//! gives all of them node-identical behaviour with no second implementation of
//! key order, hole skipping or integer-key ordering. The branch costs nothing
//! on an ordinary object: the descriptor is already loaded, and a nonzero
//! `keys` word returns before this module is consulted.
//!
//! # Identity: one ShapeId per dictionary object, and why not one globally
//!
//! A compiled inline cache compares ShapeIds and nothing else. Two dictionary
//! objects sharing one id would let a cache primed on the first read a slot of
//! the second — a wrong value, not a slow one. So each dictionary object draws
//! its own generation, once, at the latch.
//!
//! That bounds mints at **O(1) per object** against today's O(k), which is the
//! property step 2.5 needs. An append to a dictionary object mints **nothing**:
//! the array grows in place or reallocates, and neither is a fact of a shape
//! whose `keys` word is NULL. Only a change to the live inline bound, or a
//! republication that is not an append (a compacting delete, which moves
//! values and must therefore invalidate caches), draws a fresh generation.
//!
//! # The generation namespace
//!
//! Two namespaces already exist and are disjoint by construction: the
//! `SHAPE_SEMANTIC_NEXT` counter (bit 63 clear, and it aborts far below 2⁶²)
//! and `deterministic_semantic_generation` (bit 63 set). Dictionary draws are
//! **bit 63 clear, bit 62 set**, which is disjoint from both.
//! [`dictionary_generation_namespaces_are_disjoint`] asserts it.
//!
//! # GC
//!
//! `dictionary_keys` is a traced, rewritten child edge exactly like
//! `ObjectMeta::spill` (#6812): one `visit` in the
//! `GcRewriteDescriptorKind::ObjectMeta` arm of
//! `visit_gc_rewrite_slot_descriptors`, which is the single enumerator used by
//! the non-copying minor mark, the full mark, the copying-nursery evacuation,
//! the whole-heap rewrite and the dirty-slot rescan — mark, move and
//! remembered-set coverage from one place. Every store to the word is followed
//! by `runtime_write_barrier_slot`. Membership is proved by sabotage: remove
//! the `visit` and `dictionary_keys_survive_a_moving_collection` fails.

use super::dictionary_counters::*;
use super::{shapes, ObjectHeader, ObjectMeta};
use crate::array::ArrayHeader;
use std::sync::atomic::Ordering;

/// Dictionary generations set bit 62 and clear bit 63. See the module docs.
pub(crate) const DICTIONARY_GENERATION_TAG: u64 = 1 << 62;

/// Monotonic within the dictionary namespace. Starts at 1 so a generation is
/// never bare `DICTIONARY_GENERATION_TAG`, which keeps "tagged" and "drawn"
/// distinguishable in a dump.

/// Is the latch armed at all? Tri-state so the default-off path is ONE relaxed
/// load and a compare — a default-off knob must cost nothing when off.
/// `-1` unresolved, `0` off, `1` on.

/// Lowest key count at which an armed latch fires, so a fixture can trip it
/// part-way through a growth loop and observe both regimes in one run.

/// Remaining canonical LAYOUT IDS, as published by the interning allocator.
///
/// Step 2.5 packs a layout id into 24 bits of `ShapeRecord`'s padding hole, so
/// the id space is ample (16.7 M against tsc's 10,867 distinct layouts) but
/// **bounded**. An object that cannot be given a layout id cannot be interned
/// at all, and dictionary mode is the only place left for it — so exhaustion
/// is a SECOND, independent latch trigger, and unlike the growth trigger it is
/// a correctness requirement rather than a policy.
///
/// `u64::MAX` means "no bound published yet", which is the state on this tree:
/// interning does not exist, so nothing calls [`note_layout_id_budget`] and
/// the trigger is unreachable in production. It is reachable in a TEST,
/// which is the point — an untestable branch in a latch is the
/// check-that-cannot-fail pattern this campaign has now paid for five times.

/// Latches caused by trigger 2 rather than trigger 1, counted separately
/// because the two mean completely different things to an operator: growth is
/// a workload shape, exhaustion is a resource running out.

/// Publish the remaining layout-id budget. Called by the interning allocator
/// (lane 8's side of the seam); `0` means it could not hand out an id.
///
/// Arming is folded in here rather than tested separately in the predicate so
/// the default-off path stays exactly **one relaxed load**: exhaustion arms
/// the same switch the growth trigger uses, with a threshold of zero keys, and
/// the predicate then attributes the latch by reading this budget.
pub fn note_layout_id_budget(remaining: u64) {
    LAYOUT_ID_BUDGET.store(remaining, Ordering::Relaxed);
    if remaining == 0 {
        LATCH_MIN_KEYS.store(0, Ordering::Relaxed);
        LATCH_ARMED.store(1, Ordering::Relaxed);
    }
}

/// Has the layout-id space run out?
pub fn dictionary_layout_ids_exhausted() -> bool {
    LAYOUT_ID_BUDGET.load(Ordering::Relaxed) == 0
}

/// Latches attributed to layout-id exhaustion (trigger 2).
pub fn dictionary_exhaustion_latches() -> u64 {
    EXHAUSTION_LATCHES.load(Ordering::Relaxed)
}

/// Restore the "no bound published" state. Test-only: the budget is a
/// process-global, and a test that leaves it at zero arms the latch for every
/// later test in the same process.
#[cfg(test)]
pub(crate) fn test_clear_layout_id_budget() {
    LAYOUT_ID_BUDGET.store(u64::MAX, Ordering::Relaxed);
    EXHAUSTION_LATCHES.store(0, Ordering::Relaxed);
}

/// Questions asked of [`should_latch_to_dictionary`] WHILE ARMED — the
/// denominator for [`dictionary_latches`].
///
/// Without it a run reporting `latches=0` cannot be told apart from a run in
/// which the predicate was never reached at all, which is the shape of gate
/// that cannot fail (the `gc_schedule_safepoints` precedent in
/// `gc/schedule.rs`). The three states are distinguishable:
///
/// * `armed=false` — the mechanism is off. Nothing is claimed.
/// * `armed=true candidates=0` — armed and NEVER REACHED. This is the bug
///   shape: the knob was set and the call site is not on the path.
/// * `armed=true candidates>0 latches=0` — reached, and every candidate
///   declined.
///
/// The counters are plain atomics with no `#[cfg(feature)]` on them, exactly
/// like `gc/instruments.rs`, so "not compiled in" is not a state this
/// instrument can be in.

/// Objects that actually converted.

/// Key-list republications absorbed by a dictionary object. This is the number
/// that says the mode is doing its job: each of these would have minted a
/// ShapeId on an ordinary object.

/// Of those, the ones that still had to draw a fresh generation (a live-bound
/// change, or a republication that was not an append).

/// Resolve the knob once. Value-parsed, not presence-parsed: #7991 shipped a
/// knob that `PERRY_GC_DIAG=0` turned ON.
/// The compiled-in trigger-1 threshold (a unique RUN, see
/// [`should_latch_to_dictionary`]), armed BY DEFAULT.
///
/// #10868 step 2.5: a receiver with a key list unique to it mints one ShapeId
/// and one canonical trie node per key it adds, retained while its backing
/// lives. When every prefix was its own array it also cost k(k+1)/2 element
/// words (8,192 keys: 461 MB unlatched, 50 MB latched); one backing per
/// growth chain made that storage linear, so what the latch bounds now is the
/// per-key identity. §L8.3.2's rule stands: a bound that is off by default is
/// not a bound.
///
/// On `ts.transpileModule` this threshold latches 2 receivers, as many as the
/// former raw key-count trigger did at the same number. The env var still
/// overrides, in both directions.
const DEFAULT_LATCH_MIN_KEYS: u64 = 1024;

#[cold]
#[inline(never)]
fn resolve_latch_arming() -> bool {
    // Armed by default; the reads below only ADJUST the threshold.
    LATCH_MIN_KEYS.store(DEFAULT_LATCH_MIN_KEYS, Ordering::Relaxed);
    let mut armed = true;
    // Trigger 2, injectable. A fixture that really exhausts a 24-bit layout-id
    // space is impractical, so the budget is a number the allocator PUBLISHES
    // and anyone can inject — which is the only thing that makes the
    // exhaustion arm reachable by a test at all.
    if let Ok(raw) = std::env::var("PERRY_OBJECT_DICTIONARY_LAYOUT_ID_BUDGET") {
        if let Ok(budget) = raw.trim().parse::<u64>() {
            note_layout_id_budget(budget);
            armed |= budget == 0;
        }
    }
    // Trigger 1. Value-parsed, not presence-parsed: #7991 shipped a knob that
    // `PERRY_GC_DIAG=0` turned ON.
    if let Ok(raw) = std::env::var("PERRY_OBJECT_DICTIONARY_MIN_KEYS") {
        if let Ok(min_keys) = raw.trim().parse::<u64>() {
            LATCH_MIN_KEYS.store(min_keys, Ordering::Relaxed);
            armed = true;
        }
    }
    LATCH_ARMED.store(i8::from(armed), Ordering::Relaxed);
    armed
}

/// Was the latch mechanism armed this run? Half of the false-zero verdict.
pub fn dictionary_latch_armed() -> bool {
    match LATCH_ARMED.load(Ordering::Relaxed) {
        -1 => resolve_latch_arming(),
        0 => false,
        _ => true,
    }
}

/// How many `should_latch_to_dictionary` questions were asked while armed.
pub fn dictionary_latch_candidates() -> u64 {
    LATCH_CANDIDATES.load(Ordering::Relaxed)
}

/// How many objects converted to dictionary mode this run.
pub fn dictionary_latches() -> u64 {
    LATCHES.load(Ordering::Relaxed)
}

/// Key-list republications a dictionary object absorbed.
pub fn dictionary_publications() -> u64 {
    PUBLICATIONS.load(Ordering::Relaxed)
}

/// How many of those still drew a generation.
pub fn dictionary_regenerations() -> u64 {
    REGENERATIONS.load(Ordering::Relaxed)
}

/// One line for the exit dump. Always emitted when diagnostics are on — a
/// present-and-zero row, never an absent one.
pub fn dictionary_counters_line() -> String {
    format!(
        "[object-dictionary] armed={} layout_ids_exhausted={} candidates={} latches={} \
         exhaustion_latches={} publications={} regenerations={}",
        dictionary_latch_armed(),
        dictionary_layout_ids_exhausted(),
        dictionary_latch_candidates(),
        dictionary_latches(),
        dictionary_exhaustion_latches(),
        dictionary_publications(),
        dictionary_regenerations()
    )
}

/// The CURRENT arming, resolved — for a scope guard that must restore what
/// it found instead of assuming the default is off. Once #10868 armed the
/// latch by default, "disarm on exit" stopped being "restore on exit".
#[cfg(test)]
pub(crate) fn test_latch_state() -> Option<u64> {
    if dictionary_latch_armed() {
        Some(LATCH_MIN_KEYS.load(Ordering::Relaxed))
    } else {
        None
    }
}

/// Arm or disarm the latch from a test. Returns the previous minimum, if armed.
#[cfg(test)]
pub(crate) fn test_arm_latch(min_keys: Option<u64>) -> Option<u64> {
    // RESOLVE before saving. `LATCH_ARMED` starts at -1 = unresolved, and
    // reading the raw atomic sees that as "not armed" — so a test that saved
    // before anything had queried the latch restored `None`, which STORES 0
    // and disarms it for every later test in the process. Harmless while the
    // default was off; fatal once #10868 armed it, because the 65,536-key
    // membership test runs later in the same binary and its key list is
    // unique to it. It passed standalone and OOM'd in the suite, which is the
    // signature of exactly this.
    let was = if dictionary_latch_armed() {
        Some(LATCH_MIN_KEYS.load(Ordering::Relaxed))
    } else {
        None
    };
    match min_keys {
        Some(n) => {
            LATCH_MIN_KEYS.store(n, Ordering::Relaxed);
            LATCH_ARMED.store(1, Ordering::Relaxed);
        }
        None => LATCH_ARMED.store(0, Ordering::Relaxed),
    }
    was
}

#[cfg(test)]
pub(crate) fn test_reset_counters() {
    LATCH_CANDIDATES.store(0, Ordering::Relaxed);
    LATCHES.store(0, Ordering::Relaxed);
    PUBLICATIONS.store(0, Ordering::Relaxed);
    REGENERATIONS.store(0, Ordering::Relaxed);
}

/// Should this receiver stop interning its key list?
///
/// TWO independent triggers, not one:
///
/// 1. **Unique key growth.** An interned shape is shared and cannot be
///    retired by ownership the way today's private ones are (97.8% of records
///    are retired today), so a receiver whose key list is unique to it and
///    grows without bound must stop interning. Policy. The argument is the
///    list's UNIQUE RUN (`canonical_keys::take_unique_run`): how many keys the
///    lineage grew by, one receiver's append at a time, since another arrival
///    last reached it — not the key count, which would also latch every
///    member of a family of objects that merely share a long list.
/// 2. **Layout-id exhaustion** ([`note_layout_id_budget`]). The canonical
///    layout id is 24 bits; when none is left the receiver cannot be interned
///    at all and dictionary mode is the only place for it. Correctness, not
///    policy — which is why it ignores the key-count threshold.
///
/// Uniqueness is answerable now that shape identity is content-keyed: the
/// canonical trie sees every list a receiver creates by appending, and every
/// arrival at an existing one except through the transition cache.
/// `PERRY_OBJECT_DICTIONARY_MIN_KEYS` sets the minimum run.
///
/// Off, this is one relaxed load and a compare.
#[inline]
pub(crate) fn should_latch_to_dictionary(unique_run: u32) -> bool {
    match LATCH_ARMED.load(Ordering::Relaxed) {
        0 => return false,
        -1 => {
            if !resolve_latch_arming() {
                return false;
            }
        }
        _ => {}
    }
    LATCH_CANDIDATES.fetch_add(1, Ordering::Relaxed);
    // Trigger 2: no layout id is available, so this receiver cannot be
    // interned at all. It latches whatever its key count is.
    if LAYOUT_ID_BUDGET.load(Ordering::Relaxed) == 0 {
        EXHAUSTION_LATCHES.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    // Trigger 1: growth of a key list unique to this receiver. A run of 0 is a
    // list this publish did not create, which trigger 1 never latches.
    unique_run != 0 && u64::from(unique_run) >= LATCH_MIN_KEYS.load(Ordering::Relaxed)
}

/// The object's private key list, or null when it has none.
///
/// Only reached from [`crate::object::object_keys_array`], and only once the
/// shape has already said it publishes no keys, so the validation below is
/// off the path every ordinary receiver takes.
///
/// The header check is not defensive padding. `object_keys_array` takes a
/// `*const ObjectHeader` that may in fact alias an `ErrorHeader`,
/// `RegExpHeader`, `MapHeader` or `DateCell` — `shape_word_is_writable`
/// exists for exactly that aliasing — and those cells carry their metadata
/// edge at their OWN offset, not at `ObjectHeader`'s +8. Reading `.meta`
/// blind would classify another layout's bytes as a pointer and dereference
/// it, which is `try_read_gc_header`'s whole reason for existing (#340/#341).
#[inline]
pub(crate) unsafe fn keys_array(obj: *const ObjectHeader) -> *mut ArrayHeader {
    match meta_of(obj) {
        Some(meta) => (*meta).dictionary_keys as usize as *mut ArrayHeader,
        None => std::ptr::null_mut(),
    }
}

/// The receiver's `ObjectMeta`, or `None` unless this really is a live
/// `GC_TYPE_OBJECT` cell that has one. See [`keys_array`] for why the type
/// check is load-bearing rather than defensive.
#[inline]
unsafe fn meta_of(obj: *const ObjectHeader) -> Option<*mut ObjectMeta> {
    if obj.is_null() {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        None
    } else {
        Some(meta)
    }
}

/// Is this receiver in dictionary mode?
///
/// The discriminator is a SHAPE fact first — "my shape publishes no keys" —
/// and the meta word second. Both are required. An ordinary keyless receiver
/// (`{}` before its first key) satisfies the first and not the second. A
/// receiver whose shape has REGAINED a key list satisfies the second and not
/// the first, and is correctly no longer a dictionary: that is what a
/// compacting delete does when it republishes through the ordinary path, so
/// the mode un-latches itself rather than needing a separate exit.
#[inline]
pub(crate) unsafe fn is_dictionary(obj: *const ObjectHeader) -> bool {
    let Some(meta) = meta_of(obj) else {
        return false;
    };
    if (*meta).dictionary_keys == 0 {
        return false;
    }
    shapes::object_shape_descriptor(obj).is_some_and(|descriptor| descriptor.keys == 0)
}

/// Store the private key list, with the barrier that makes it a remembered
/// old→young edge. Never allocates.
#[inline]
unsafe fn store_keys_array(meta: *mut ObjectMeta, keys: *mut ArrayHeader) {
    let bits = keys as usize as u64;
    // GC_STORE_AUDIT(BARRIERED): metadata-record slot store + object barrier,
    // exactly as `object/spill.rs` does for `ObjectMeta::spill`.
    (*meta).dictionary_keys = bits;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).dictionary_keys as *const _ as usize,
        bits,
    );
}

/// A fresh generation in the dictionary namespace. Every semantic transition
/// of a dictionary receiver draws from here (`shapes::transition_object_shape_semantics`),
/// so its identity never leaves the namespace.
pub(crate) fn next_generation() -> u64 {
    let n = DICTIONARY_GENERATION_NEXT.fetch_add(1, Ordering::Relaxed);
    debug_assert!(
        n < DICTIONARY_GENERATION_TAG,
        "dictionary generation counter left its namespace"
    );
    DICTIONARY_GENERATION_TAG | n
}

/// Mint and stamp a keyless dictionary shape carrying `live_inline_slot_count`.
///
/// Mint-then-stamp, like every other publisher in this tree: the mint inserts
/// into a `HashMap` and can therefore collect and MOVE the receiver, so the
/// predecessor stays stamped across it (and still describes the payload
/// correctly, because the live bound has not changed yet) and the receiver is
/// re-resolved through a handle before the single `parent_class_id` store.
unsafe fn restamp_dictionary_shape(obj: *mut ObjectHeader, live_inline_slot_count: u32) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    // Read before the mint: the prototype identity is a fact of the receiver
    // the dictionary shape must keep naming.
    let proto_id = match shapes::object_shape_descriptor(obj) {
        Some(d) => d.proto_id,
        None => shapes::object_proto_id(obj),
    };
    let handle = scope.root_raw_mut_ptr(obj);
    // The mint is the allocating half; the receiver is never nameable across
    // it (#7341), so there is no pre-call address left to stamp.
    let (id, obj) = handle.across_mut::<ObjectHeader, _>(|| {
        shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_holes(
            std::ptr::null(),
            0,
            live_inline_slot_count,
            next_generation(),
            shapes::ShapeObjectKind::Ordinary,
            0,
            proto_id,
        ))
    });
    shapes::stamp_object_shape_id_with_carrier_note(obj, id);
    id
}

/// Convert `obj` to dictionary mode. Idempotent; returns whether the receiver
/// is in dictionary mode afterwards.
///
/// Refused for anything that is not an ordinary shaped object, and for a
/// receiver that already carries tombstones: a hole is counted in the shape's
/// `hole_count`, a fact a keyless dictionary shape does not carry, so latching
/// over one would drop the count. Compacting the holes away at the latch would
/// have to move values and is not worth it for stage 1 — such a receiver simply
/// stays ordinary. Recorded rather than left implicit because it is a real
/// limit on when the latch can fire.
pub(crate) unsafe fn latch_object_to_dictionary(obj: *mut ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    if is_dictionary(obj) {
        return true;
    }
    if !crate::object::object_is_regular(obj) {
        return false;
    }
    let Some(descriptor) = shapes::object_shape_descriptor(obj) else {
        return false;
    };
    if descriptor.hole_count != 0 {
        return false;
    }
    if descriptor.logical_key_count == 0 {
        // Nothing to carry — and an allocator's FIRST key publication reaches
        // the latch site before the receiver's payload is initialized, so
        // converting here would allocate (and therefore collect) over slots
        // the shape already claims are live.
        return false;
    }
    let live_inline_slot_count = descriptor.live_inline_slot_count;
    let key_count = descriptor.logical_key_count;

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);

    // 1. The typed-layout descriptor is keyed by the keys edge, and this
    //    receiver is about to stop having one. Invalidate while the
    //    predecessor stamp is still authoritative, exactly as
    //    `set_object_keys_array_with_live` does for a pointer change.
    // The reload is discarded: step 2 allocates again and reloads through
    // the same handle, so a name bound here would be stale before it is read.
    let ((), _) = obj_handle
        .across_mut::<ObjectHeader, _>(|| crate::object::mark_object_dynamic_shape_unknown(obj));

    // 2. A PRIVATE copy of the key list, with slack so the first appends
    //    after the latch do not immediately reallocate. The source may be
    //    shared (`GC_FLAG_SHAPE_SHARED`) with every sibling of its layout and
    //    a dictionary receiver mutates its array in place, so the copy is
    //    unconditional rather than conditional on the flag: after the latch
    //    the array has exactly one owner by construction.
    let (cloned, obj) = obj_handle.across_mut::<ObjectHeader, _>(|| {
        crate::array::js_array_alloc_pointer_elements(key_count + 4)
    });
    if cloned.is_null() {
        return false;
    }
    let cloned_handle = scope.root_raw_mut_ptr(cloned);

    // 3. The metadata record, LAST of the three allocating steps.
    //
    //    The order matters and is not cosmetic: a meta record is reachable
    //    only through its owner and is not a thing to hand the root scanner,
    //    so this step must be the one after which nothing allocates. An
    //    earlier draft allocated the record second and rooted it across the
    //    clone's allocation — that SIGSEGV'd the whole test binary.
    let ((meta, obj), cloned) = cloned_handle.across_mut::<ArrayHeader, _>(|| {
        obj_handle.across_mut::<ObjectHeader, _>(|| crate::object::object_meta_ensure(obj))
    });
    if meta.is_null() {
        return false;
    }

    // 4. Fill the copy. Allocates nothing, which is what lets the SOURCE be
    //    resolved here rather than carried across an allocation.
    copy_key_list_into(obj, cloned, key_count);

    // 5. Install the private list BEFORE the shape stops publishing the old
    //    one. There is no window in which the receiver's keys are
    //    unreachable: across step 6's mint the old array is still on the
    //    shape and the new one is already a traced child edge of the record.
    store_keys_array(meta, cloned);

    // 6. Publish the keyless dictionary shape. `meta` is not read after this
    //    point, so the mint's allocation cannot hand us a stale record.
    restamp_dictionary_shape(obj, live_inline_slot_count);

    LATCHES.fetch_add(1, Ordering::Relaxed);
    true
}

/// Copy the receiver's current ordered key list into `dst`. **Allocates
/// nothing**, which is what lets the source be resolved here rather than
/// carried across the allocation that produced `dst`.
///
/// Both sides go through the element accessors, never through
/// `dst as *mut u8 + size_of::<ArrayHeader>()`. A keys array's elements do
/// not necessarily start at its header + 8: `clean_arr_ptr` resolves a
/// grow-forward pointer, and `array_elements_ptr` accounts for the FRONT
/// RESERVE that #9019's reserved-floor arrays are born with. Hand-computing
/// the offset copies header words and reserve slack into the clone as if they
/// were key pointers — which reads as a missing property before the next
/// collection and SIGSEGVs during it, because the clone claims an all-pointer
/// layout and the collector believes it. Both symptoms were observed here
/// before this used the accessor.
unsafe fn copy_key_list_into(obj: *const ObjectHeader, dst: *mut ArrayHeader, key_count: u32) {
    let source = shapes::object_shape_descriptor(obj)
        .map(|descriptor| descriptor.keys as usize as *mut ArrayHeader)
        .unwrap_or(std::ptr::null_mut());
    let (src, src_len) = crate::object::keys_array_dense_slots(source);
    let count = std::cmp::min(key_count as usize, src_len);
    let out = crate::array::array_elements_ptr(dst as *const ArrayHeader);
    for i in 0..count {
        // GC_STORE_AUDIT(INIT): the clone is unpublished and its all-pointer
        // layout covers only the prefix `length` exposes, which is set below.
        *out.add(i) = (*src.add(i)).to_bits();
    }
    (*dst).length = count as u32;
}

/// Absorb a key-list publication for a dictionary receiver.
///
/// This is the whole point of the mode: on an ordinary receiver every one of
/// these calls mints a ShapeId. Here a SAME-ARRAY publication mints nothing —
/// the array's address is not a fact of a shape whose `keys` word is NULL, an
/// in-place append moves no value, and an in-place tombstone leaves every
/// surviving slot where it was, so a cache primed on this receiver stays
/// correct either way.
///
/// A publication that swaps the array in DOES draw a fresh generation. The
/// case that forces it is the compacting delete, which allocates a fresh
/// array and shifts every value after the hole down one slot: a cache holding
/// `(this ShapeId, key) -> slot` would then read the wrong value, and the new
/// identity is what invalidates it. A reallocating `js_array_push` also swaps
/// the array and does not move values, so it pays a generation it does not
/// strictly need — that is O(log k) draws over k appends against O(k), and
/// buying the difference would need the previously published length kept
/// somewhere, which is a word this record does not have to spare. The test
/// asserts the bound is sub-linear, not that it is zero.
pub(crate) unsafe fn publish_keys(
    obj: *mut ObjectHeader,
    keys: *mut ArrayHeader,
    live_inline_slot_count: u32,
) {
    PUBLICATIONS.fetch_add(1, Ordering::Relaxed);
    let meta = (*obj).meta;
    debug_assert!(!meta.is_null(), "publish_keys on a receiver with no meta");
    let previous = (*meta).dictionary_keys as usize as *mut ArrayHeader;
    let swapped = keys != previous;

    // A swap is not automatically a value move. The [[Set]] growth path
    // clones before pushing whenever the array carries `GC_FLAG_SHAPE_SHARED`
    // — and the transition cache STAMPS that flag on any array it caches an
    // edge through, including this receiver's private one — so an ordinary
    // append arrives here as a new pointer. That clone preserves order and
    // indices, so no value moved and no cache is stale; regenerating for it
    // would make the mode draw one identity per append, which is the exact
    // cost it exists to remove (measured: 23 draws for 24 appends).
    //
    // The compacting delete, which allocates a fresh array AND shifts every
    // value after the hole down one slot, is the case that must regenerate.
    // The two are told apart by length, and the length is readable because
    // the predecessor array is still alive and still carries its pre-append
    // length — a clone-and-push does not mutate the array it cloned FROM.
    let previous_len = if previous.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(previous)
    };
    let next_len = if keys.is_null() {
        0
    } else {
        crate::array::keys_array_len_capped_to_capacity(keys)
    };
    // Strictly SHORTER, not "not longer". The [[Set]] growth path publishes
    // the clone BEFORE it pushes, so an ordinary append arrives here twice —
    // once at equal length (the clone) and once one longer (the push) — and
    // treating the equal-length publication as a possible move drew one
    // identity per append, which is what the mode exists to stop (measured:
    // 23 draws for 24 appends). An equal-length swap is clone-before-mutate
    // and preserves every index; the assertion below is what keeps that a
    // checked invariant rather than a comment.
    let values_may_have_moved = swapped && next_len < previous_len;
    debug_assert!(
        !swapped || next_len != previous_len || key_lists_match(previous, keys, next_len),
        "an equal-length keys-array swap reordered a dictionary receiver's \
         keys: every cached (ShapeId, key) -> slot for it is now wrong"
    );

    if swapped {
        store_keys_array(meta, keys);
    }
    let live_changed = shapes::object_shape_descriptor(obj)
        .is_none_or(|descriptor| descriptor.live_inline_slot_count != live_inline_slot_count);
    if values_may_have_moved || live_changed {
        REGENERATIONS.fetch_add(1, Ordering::Relaxed);
        restamp_dictionary_shape(obj, live_inline_slot_count);
    }
}

/// Do two keys arrays hold the same `count` entries in the same order?
///
/// Debug-only support for [`publish_keys`]' equal-length rule. Cheap to state
/// and impossible to get wrong by accident, which is the point: the rule is
/// what lets an append avoid drawing a shape identity, so an unchecked
/// assumption there is a silent wrong VALUE on the next cached read.
#[cfg(debug_assertions)]
unsafe fn key_lists_match(a: *mut ArrayHeader, b: *mut ArrayHeader, count: usize) -> bool {
    if a.is_null() || b.is_null() {
        return a == b;
    }
    let (pa, la) = crate::object::keys_array_dense_slots(a);
    let (pb, lb) = crate::object::keys_array_dense_slots(b);
    if la < count || lb < count {
        return false;
    }
    (0..count).all(|i| (*pa.add(i)).to_bits() == (*pb.add(i)).to_bits())
}

#[cfg(not(debug_assertions))]
unsafe fn key_lists_match(_a: *mut ArrayHeader, _b: *mut ArrayHeader, _count: usize) -> bool {
    true
}

/// The parity invariant for a dictionary receiver, in place of the ordinary
/// one (`shapes::debug_assert_object_shape_parity_for_keys`, which compares the
/// published `keys` word against `object_keys_array` and would see NULL against
/// the private list).
///
/// Debug builds only.
#[inline]
pub(crate) unsafe fn debug_assert_dictionary_parity(obj: *const ObjectHeader) {
    if !cfg!(debug_assertions) {
        return;
    }
    let Some(meta) = meta_of(obj) else {
        debug_assert!(
            false,
            "a dictionary receiver must be a live GC_TYPE_OBJECT with a meta"
        );
        return;
    };
    debug_assert!(
        (*meta).dictionary_keys != 0,
        "a dictionary receiver must carry a key list"
    );
    let Some(descriptor) = shapes::object_shape_descriptor(obj) else {
        debug_assert!(false, "a dictionary receiver must be stamped");
        return;
    };
    debug_assert!(
        descriptor.keys == 0 && descriptor.logical_key_count == 0,
        "a dictionary receiver's shape must publish no keys"
    );
    debug_assert!(
        descriptor.semantic_generation & DICTIONARY_GENERATION_TAG != 0
            && descriptor.semantic_generation & (1u64 << 63) == 0,
        "a dictionary receiver's generation must come from the dictionary namespace \
         ({:#x})",
        descriptor.semantic_generation
    );
}
