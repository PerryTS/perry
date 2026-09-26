//! The key-add half of the generated static-key store (`o.k = v` where `k` is
//! not yet an own property of `o`).
//!
//! # What the site holds
//!
//! A static-key store site owns one [`PackedSetSite`]: the existing-key word
//! (`packed_set.rs`) and two ADD words. The add words are a memo of a pure
//! function of ONE ShapeId, the PRE-shape `S`:
//!
//! > appending `k` to any receiver carrying `S` yields ShapeId `T`, with the
//! > value at slot `s` (inline, or in overflow storage).
//!
//! `S` names an immutable key list, live inline bound, prototype identity
//! ([[Prototype]] is a shape fact) and descriptor/integrity state, so the
//! successor is determined: [`packed_add_prime`] does not merely observe the
//! runtime's transition, it re-derives it from the two descriptors (`T`'s key
//! list is `S`'s plus `k` at index `s`, same prototype, same semantic
//! generation, same kind, no holes, and `T`'s live bound is exactly what the
//! append rule gives) and publishes nothing otherwise.
//!
//! The one input that is NOT a property of `S` is whether the prototype chain
//! intercepts the write (an inherited setter, an inherited non-writable data
//! property). That verdict is the inherited-access lane's
//! (`object::chain_store`): every prototype hop AND every class id of the
//! chain is MARKED before the verdict is computed, so any later change that
//! could flip it moves the one global word `proto_validity`. The guard word
//! records it, read after the marking and before the predicate, and the
//! emitted hit re-proves the verdict with one load and one compare. Evaluating
//! or registering a class moves nothing unless a verdict walked that class. The prototype the verdict walked is the one
//! the receiver's SHAPE names, because the pre-shape compare is what admitted
//! the receiver.
//!
//! # What the emitted hit re-tests per object
//!
//! (`perry-codegen/src/expr/put_value_store_ic.rs`, `emit_key_add_hit`.)
//! The pre-shape compare proves GC kind, not forwarded, not frozen / sealed /
//! non-extensible, the key absent, no own descriptor, the prototype, the
//! object kind, and that the receiver is neither a marked prototype nor an
//! exotic read receiver (both marks move an object onto a private lineage,
//! and the prime never learns one; a prototype's structural change must go
//! through the stamp funnel, which moves the validity word). It cannot prove per-object facts, which the hit reads:
//!
//! * the receiver kind and the Array-subclass numeric proof, exactly as the
//!   existing-key hit (`_reserved` and `class_id`);
//! * `GC_FLAG_TENURED` clear: the stamp funnel
//!   (`shapes::stamp_object_shape_id_with_carrier_note`) owes an
//!   old-generation receiver a carrier note. `Old => TENURED` (every old-gen
//!   placement ORs the bit in; see `write_barrier.rs`) and objects are only
//!   ever born in the nursery or old space (`arena_alloc_gc`), so a clear bit
//!   proves the receiver young and the note unnecessary;
//! * no stable tombstones and no descriptor flag (both conservative: the
//!   shape already covers them);
//! * the layout state: a `GC_LAYOUT_SIDE_MASK` or typed-layout receiver's
//!   layout record describes the PRE-shape, so the hit first calls
//!   [`js_gc_key_add_layout_unknown`] (the transition lane's
//!   `mark_object_dynamic_shape_unknown`), exactly as the runtime does.
//!
//! Everything else takes the miss, which serves the memo in the runtime
//! ([`packed_add_try`], through the audited stamp funnel and overflow store)
//! before falling back to the full `[[Set]]`.
//!
//! # GC: the memo's ShapeIds stay resolvable
//!
//! An intermediate constructor shape is carried by no object once
//! construction finishes, so a full trace would retire it. A site that stamps
//! `T` must therefore own `T` (and `S`): both are noted as cache carriers when
//! published and re-noted after every full trace from the registry of primed
//! sites ([`note_packed_add_carriers`], called by
//! `shape_carriers::recompute_after_full_trace`). A cache-carried descriptor's
//! keys array is rooted and rewritten by the shape table's scan. The words
//! hold only numbers, never a heap address.
//!
//! # Agents
//!
//! Sites are process-global. Only the primary agent primes (and registers) a
//! site; ShapeIds are process-unique, so a worker's receivers never match a
//! primary-agent pre-shape and a worker only ever takes the miss.
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

/// One static-key store site: the emitted `@perry_ic_N_packed_set`
/// (`[4 x i64]`). **The layout must equal `perry-codegen`'s
/// `PACKED_SET_SITE_WORDS` / `ADD_SHAPES_WORD` / `ADD_GUARD_WORD`**; pinned by
/// `packed_set_site_layout_matches_codegen`.
#[repr(C)]
pub struct PackedSetSite {
    /// The existing-key word (`packed_set.rs`).
    pub set: AtomicU64,
    /// `pre | post << 32`. `pre` is flipped by the read path's spill flip for
    /// an overflow slot, so no emitted compare can match it.
    pub add_shapes: AtomicU64,
    /// `(proto_validity + VTABLE_GEN) << ADD_SLOT_BITS | slot`.
    pub add_guard: AtomicU64,
    /// A `*mut AddWays` (0 = none): the memos of further pre-shapes, served
    /// by [`packed_add_try`]. A base-class constructor's key-add sees one
    /// pre-shape per subclass (the prototype is part of the shape), so such
    /// a site is polymorphic by construction. The emitted hit compares the
    /// ways at the pre-shape's home ([`add_way_home`]) and the one after it,
    /// after the primary words.
    pub add_ways: AtomicU64,
}

/// One further memo, in the primary words' format (`add_shapes`,
/// `add_guard` are a way too: the emitted hit reads either through one
/// pointer).
#[repr(C)]
pub struct AddWay {
    shapes: AtomicU64,
    guard: AtomicU64,
}

/// Further memos per site, never evicted (so a site with more stable
/// pre-shapes than ways settles instead of cycling); a site that overflows
/// them re-primes its primary words. A memo is placed at its pre-shape's HOME
/// way ([`add_way_home`]) when that way is free, and otherwise at the next
/// free way from it; the emitted hit compares the home and the way after it
/// ([`ADD_WAY_PROBES`]), the runtime every way. Placement by pre-shape rather
/// than by arrival matters: on
/// tsc the hot memo of a polymorphic site is typically NOT among its first
/// (8 sites whose hits all land on their 3rd way, 10 on their 15th, behind
/// transient first-instance shapes), so no fixed prefix of an in-order list
/// is where the hits are.
///
/// 64 (a power of two for the home hash), not 8: Zod 3's `ZodType`
/// constructor adds its keys to one pre-shape per subclass (36 of them), and
/// with 8 ways 15,069 of its 78,250 executed key-adds per 200 parses re-ran
/// the full `[[Set]]` and re-primed. Only a polymorphic site allocates them.
pub const ADD_WAYS: usize = 1 << ADD_WAYS_LOG2;
/// `log2(ADD_WAYS)`: the home is the top bits of a 32-bit product.
pub const ADD_WAYS_LOG2: u32 = 6;
/// The multiplier of [`add_way_home`] (2^32 / golden ratio): consecutive
/// ShapeIds, which subclass shapes minted in sequence are, land far apart.
pub const ADD_WAY_HASH: u32 = 0x9E37_79B1;
/// Ways the emitted hit compares from the home on (the home, then the next
/// mod [`ADD_WAYS`]): a memo whose home an earlier memo holds lands on the
/// next free way, which is most often the very next.
#[cfg_attr(not(test), allow(dead_code))]
pub const ADD_WAY_PROBES: usize = 2;
type AddWays = [AddWay; ADD_WAYS];

/// The way the emitted hit compares for a receiver of ShapeId `pre`:
/// the top [`ADD_WAYS_LOG2`] bits of `pre * ADD_WAY_HASH` (mod 2^32).
/// **perry-codegen computes the same (`emit_static_store_ic`).**
#[inline]
pub fn add_way_home(pre: u32) -> usize {
    (pre.wrapping_mul(ADD_WAY_HASH) >> (32 - ADD_WAYS_LOG2)) as usize
}

impl PackedSetSite {
    pub const fn empty() -> Self {
        Self {
            set: AtomicU64::new(PACKED_SET_EMPTY),
            add_shapes: AtomicU64::new(PACKED_SET_EMPTY),
            add_guard: AtomicU64::new(0),
            add_ways: AtomicU64::new(0),
        }
    }
}

/// Word indices of [`PackedSetSite`], for the layout pin.
#[cfg_attr(not(test), allow(dead_code))]
pub const ADD_SHAPES_WORD: usize = 1;
#[cfg_attr(not(test), allow(dead_code))]
pub const ADD_GUARD_WORD: usize = 2;
#[cfg_attr(not(test), allow(dead_code))]
pub const PACKED_SET_SITE_WORDS: usize = 4;
#[cfg_attr(not(test), allow(dead_code))]
pub const ADD_WAYS_WORD: usize = 3;
/// Words of one [`AddWay`].
#[cfg_attr(not(test), allow(dead_code))]
pub const ADD_WAY_WORDS: usize = 2;
/// Low bits of the guard word that hold the slot.
pub const ADD_SLOT_BITS: u32 = 16;
const ADD_SLOT_MASK: u64 = (1 << ADD_SLOT_BITS) - 1;

const SPILL_FLIP: u32 = crate::object::field_get_set::PACKED_SPILL_FLIP;

/// `_reserved` bits that refuse a receiver on the runtime-side hit. The
/// integrity flags are refused as well although the shape proves them.
const ADD_BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
    | crate::gc::OBJ_FLAG_SEALED
    | crate::gc::OBJ_FLAG_NO_EXTEND
    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
    | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF
    | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
    | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;

/// The verdict generation the guard word records and the emitted hit
/// recomputes: `PERRY_PROTO_VALIDITY + PERRY_VTABLE_GEN`.
#[inline]
pub(crate) fn add_generation() -> u64 {
    crate::object::chain_store::verdict_generation()
}

/// `PERRY_KEYADD_IC=0` stops publication (A/B in one binary; both store
/// identically, the off arm through the full `[[Set]]`).
#[inline]
fn lane_enabled() -> bool {
    // A test build never reads the process environment for it: a
    // process-global latch readable from tests is a #10944 hazard.
    #[cfg(test)]
    {
        true
    }
    #[cfg(not(test))]
    {
        static KEYADD_LANE_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *KEYADD_LANE_ON.get_or_init(|| {
            crate::gc::env_default_on_from_value(std::env::var("PERRY_KEYADD_IC").ok().as_deref())
        })
    }
}

crate::perry_thread_local! {
    /// Every site this agent published an add memo into. Sites are module
    /// globals and live for the process, so entries are never removed.
    static ADD_SITES: std::cell::UnsafeCell<Vec<*const PackedSetSite>> =
        const { std::cell::UnsafeCell::new(Vec::new()) };
}

/// The site's further memos, if it has allocated them.
///
/// # Safety
/// `site` is a live site.
#[inline]
unsafe fn site_ways(site: *const PackedSetSite) -> Option<&'static AddWays> {
    let word = (*site).add_ways.load(Ordering::Relaxed) as usize;
    (word != 0).then(|| &*(word as *const AddWays))
}

#[inline]
fn unflip(pre: u32) -> u32 {
    if crate::object::shapes::is_shape_id(pre) {
        pre
    } else {
        pre ^ SPILL_FLIP
    }
}

/// Re-note every published memo's two ShapeIds as cache-carried. Runs after
/// `clear_all_cache_carriers` in the full-trace recompute, before the
/// uncarried-descriptor prune.
pub(crate) fn note_packed_add_carriers() {
    fn note(shapes: u64) {
        if shapes == PACKED_SET_EMPTY {
            return;
        }
        crate::object::shape_carriers::note_shape_id(unflip(shapes as u32));
        crate::object::shape_carriers::note_shape_id((shapes >> 32) as u32);
    }
    ADD_SITES.with(|cell| unsafe {
        for &site in (*cell.get()).iter() {
            note((*site).add_shapes.load(Ordering::Relaxed));
            if let Some(ways) = site_ways(site) {
                for way in ways.iter() {
                    note(way.shapes.load(Ordering::Relaxed));
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Store census (`PERRY_STORE_CENSUS`): compile-time instrumentation writes
// counters 0..16 from emitted code; the runtime classifies its own paths in
// 16..32. Printed at exit when the variable is set at run time.
// ---------------------------------------------------------------------------

/// The census counters. **Indices below [`CENSUS_RUNTIME_BASE`] are owned by
/// `perry-codegen/src/expr/store_census.rs`.**
#[no_mangle]
pub static PERRY_STORE_CENSUS: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];
#[allow(dead_code)]
pub const CENSUS_RUNTIME_BASE: usize = 16;
pub(crate) const C_ADD_RT_INLINE: usize = 16;
pub(crate) const C_ADD_RT_SPILL: usize = 17;
pub(crate) const C_FULL_KEYADD: usize = 18;
pub(crate) const C_FULL_OTHER: usize = 19;
pub(crate) const C_PRIME_PUBLISHED: usize = 20;
pub(crate) const C_PRIME_INTERCEPTED: usize = 21;
pub(crate) const C_FULL_KEYADD_CLASS: usize = 22;
pub(crate) const C_FULL_KEYADD_SPILL: usize = 23;
pub(crate) const C_PRIME_UNVERIFIED: usize = 24;

#[cfg_attr(test, allow(dead_code))]
const CENSUS_NAMES: [&str; 32] = [
    "emit.pic.word_hit",
    "emit.pic.way_hit",
    "emit.add.inline_hit",
    "emit.pic.miss_call",
    "emit.cfield.shape_proven_store",
    "emit.cfield.guard_store",
    "emit.cfield.guard_fallback_call",
    "emit.cfield.ic_call",
    "emit.cfield.loop_raw_store",
    "emit.cfield.setter_call",
    "emit.cfield.sloppy",
    "emit.by_name.runtime",
    "emit.by_name.put_value",
    "emit.add.layout_forget",
    "emit.add.way_hit",
    "emit.15",
    "rt.add.memo_inline",
    "rt.add.memo_spill",
    "rt.full.key_add",
    "rt.full.other",
    "rt.prime.published",
    "rt.prime.intercepted",
    "rt.full.key_add.class_instance",
    "rt.full.key_add.spill",
    "rt.prime.unverified",
    "rt.25",
    "rt.26",
    "rt.27",
    "rt.28",
    "rt.29",
    "rt.30",
    "rt.31",
];

#[inline]
pub(crate) fn census_enabled() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    {
        static STORE_CENSUS_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *STORE_CENSUS_ON.get_or_init(|| {
            let on = std::env::var_os("PERRY_STORE_CENSUS").is_some();
            if on {
                extern "C" fn report() {
                    let mut line = String::from("[store-census]");
                    for (i, c) in PERRY_STORE_CENSUS.iter().enumerate() {
                        let n = c.load(Ordering::Relaxed);
                        if n != 0 {
                            line.push_str(&format!(" {}={}", CENSUS_NAMES[i], n));
                        }
                    }
                    eprintln!("{line}");
                }
                unsafe { libc::atexit(report) };
            }
            on
        })
    }
}

/// Arm the exit report. A census build calls this from `main` right after
/// `js_gc_init` (`perry-codegen/src/codegen/entry.rs`), so a program whose
/// stores never reach a runtime path still reports its emitted counters.
#[no_mangle]
pub extern "C" fn perry_store_census_arm() {
    let _ = census_enabled();
}

#[inline]
pub(crate) fn census(idx: usize) {
    if census_enabled() {
        PERRY_STORE_CENSUS[idx].fetch_add(1, Ordering::Relaxed);
    }
}

/// Serve `target` from the site's add memo without the full `[[Set]]`, for a
/// receiver the emitted hit refused per object (old, meta-bearing) or a spill
/// slot the emitted hit never takes. `None` = not served, nothing changed.
///
/// # Safety
/// `site` is a live site; `target`/`value` are the caller's live values and
/// nothing has collected since they were read.
pub(crate) unsafe fn packed_add_try(
    site: *const PackedSetSite,
    target: f64,
    value: f64,
) -> Option<f64> {
    if site.is_null() {
        return None;
    }
    let primary = (*site).add_shapes.load(Ordering::Relaxed);
    if primary == PACKED_SET_EMPTY {
        return None;
    }
    let bits = target.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG
        || (bits & POINTER_MASK) < crate::value::addr_class::HANDLE_BAND_MAX as u64
    {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *mut crate::ObjectHeader;
    let sid = crate::object::shapes::object_shape_stamp(obj);
    if !crate::object::shapes::is_shape_id(sid) {
        return None;
    }
    let matches = |shapes: u64| {
        let pre = shapes as u32;
        shapes != PACKED_SET_EMPTY && (pre == sid || pre ^ SPILL_FLIP == sid)
    };
    let (shapes, guard) = if matches(primary) {
        (primary, (*site).add_guard.load(Ordering::Relaxed))
    } else {
        let ways = site_ways(site)?;
        // A memo sits at its home way unless that was taken when it was
        // placed; the emitted hit has already compared the home.
        let home = add_way_home(sid);
        let way = (0..ADD_WAYS)
            .map(|i| &ways[(home + i) % ADD_WAYS])
            .find(|way| matches(way.shapes.load(Ordering::Relaxed)))?;
        (
            way.shapes.load(Ordering::Relaxed),
            way.guard.load(Ordering::Relaxed),
        )
    };
    let spill = shapes as u32 != sid;
    if guard >> ADD_SLOT_BITS != add_generation() {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & ADD_BLOCKING != 0
        || !write_fast_path_receiver_kind_ok(obj, header._reserved)
        || !crate::object::object_is_regular(obj)
    {
        return None;
    }
    let meta = (*obj).meta;
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).flags & crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER != 0)
    {
        return None;
    }
    let post = (shapes >> 32) as u32;
    let post_d = crate::object::shapes::shape_descriptor_by_id(post)?;
    let slot = (guard & ADD_SLOT_MASK) as usize;
    // The audited funnel: layout-unknown marking, the stamp, the prototype
    // validity bump for a marked receiver, the old-generation carrier note.
    if !crate::object::shapes::install_cached_object_shape_version(
        obj,
        sid,
        post,
        post_d.keys as usize as *mut crate::array::ArrayHeader,
        post_d.logical_key_count,
    ) {
        return None;
    }
    let vbits = fixup_null_pointer(value.to_bits());
    if spill {
        census(C_ADD_RT_SPILL);
        // Overflow storage may allocate (spill growth); the value is re-read
        // from a root for the caller, as the transition lane does.
        let scope = crate::gc::RuntimeHandleScope::new();
        let value_h = scope.root_nanbox_f64(f64::from_bits(vbits));
        crate::object::overflow_set(obj as usize, slot, vbits);
        return Some(value_h.get_nanbox_f64());
    }
    census(C_ADD_RT_INLINE);
    crate::object::store_object_field_slot(obj, slot, vbits);
    Some(value)
}

/// The key-add hit's layout retirement: a receiver whose layout record
/// (side mask or typed descriptor) described its PRE-shape. Exactly what the
/// transition lane runs before its stamp. Edits header bits and layout /
/// feedback side tables; allocates nothing a collection could see and never
/// collects (`gc_call_effects.rs` lists it as CannotCollect).
///
/// # Safety
/// `obj` is a live, non-forwarded ordinary object's handle.
#[no_mangle]
pub unsafe extern "C" fn js_gc_key_add_layout_unknown(obj: u64) {
    crate::object::mark_object_dynamic_shape_unknown(obj as usize as *mut crate::ObjectHeader);
}

/// The transition lane's value fix-up (`fast_paths.rs`): a POINTER-tagged
/// null is stored as `undefined`.
#[inline]
fn fixup_null_pointer(vbits: u64) -> u64 {
    if vbits == POINTER_TAG {
        crate::value::TAG_UNDEFINED
    } else {
        vbits
    }
}

/// After the full `[[Set]]`: if it appended `key` to a receiver that carried
/// `pre` before the store, and the chain does not intercept the key, publish
/// the memo.
///
/// # Safety
/// `site` is null or a live site; `target` and `key` are live values read
/// after the last collection point.
pub(crate) unsafe fn packed_add_prime(
    site: *const PackedSetSite,
    target: f64,
    key: *const crate::StringHeader,
    pre: u32,
) {
    let bits = target.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG || key.is_null() {
        census(C_FULL_OTHER);
        return;
    }
    let obj = (bits & POINTER_MASK) as *mut crate::ObjectHeader;
    // Both halves of the memo are site words: an ORDINARY-band ShapeId only
    // (`shapes::is_site_matchable_shape_id`), never a dictionary shape's, so
    // the emitted pre-shape compare can never equal a dictionary receiver's
    // word and the hit can never stamp a dictionary id.
    if !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !crate::object::shapes::is_site_matchable_shape_id(pre)
    {
        census(C_FULL_OTHER);
        return;
    }
    let post = crate::object::shapes::object_shape_stamp(obj);
    if post == pre || !crate::object::shapes::is_site_matchable_shape_id(post) {
        census(C_FULL_OTHER);
        return;
    }
    let (Some(pre_d), Some(post_d)) = (
        crate::object::shapes::shape_descriptor_by_id(pre),
        crate::object::shapes::shape_descriptor_by_id(post),
    ) else {
        census(C_FULL_OTHER);
        return;
    };
    let n = pre_d.logical_key_count;
    if post_d.logical_key_count != n + 1 {
        census(C_FULL_OTHER);
        return;
    }
    census(C_FULL_KEYADD);
    let floor = crate::object::INLINE_SLOT_FLOOR as u32;
    let live = pre_d.live_inline_slot_count;
    let inline = n < live.max(floor);
    if census_enabled() {
        if !inline {
            census(C_FULL_KEYADD_SPILL);
        }
        let cid = (*obj).class_id;
        if cid != 0 && !crate::object::is_anon_shape_class_id(cid) {
            census(C_FULL_KEYADD_CLASS);
        }
    }
    if site.is_null()
        || !lane_enabled()
        || crate::agent::current_agent() != crate::agent::PRIMARY_AGENT
    {
        return;
    }
    // The successor must be exactly the append of `key` to `pre`: the memo
    // is then a function of `pre` alone.
    let expected_live = if !inline || n < live { live } else { n + 1 };
    let verified = pre_d.hole_count == 0
        && post_d.hole_count == 0
        && pre_d.proto_id == post_d.proto_id
        && pre_d.semantic_generation == post_d.semantic_generation
        && pre_d.object_kind == post_d.object_kind
        && post_d.live_inline_slot_count == expected_live
        && n < (1 << ADD_SLOT_BITS) - 1
        && crate::object::object_is_regular(obj)
        && eligible_key(key)
        && crate::object::keys_find_slot_by_key_ptr(
            post_d.keys as usize as *const crate::array::ArrayHeader,
            n + 1,
            key,
        ) == Some(n);
    if !verified {
        census(C_PRIME_UNVERIFIED);
        return;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return;
    };
    if !write_fast_path_receiver_kind_ok(obj, header._reserved) {
        census(C_PRIME_UNVERIFIED);
        return;
    }
    // A marked prototype or exotic read receiver is on a private shape lineage
    // (`proto_validity::ensure_meta_for_mark`); never learn one of its shapes,
    // so the emitted hit's pre-shape compare alone proves the receiver is
    // neither. Its own structural changes must keep going through the stamp
    // funnel, which moves the validity word for a prototype.
    let meta = (*obj).meta;
    if !meta.is_null()
        && (*meta).flags
            & (crate::object::OBJECT_META_FLAG_IS_PROTOTYPE
                | crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER)
            != 0
    {
        census(C_PRIME_UNVERIFIED);
        return;
    }
    // The chain verdict (object::chain_store's discipline): mark every hop,
    // read the generation, then ask the authoritative predicate. Both calls
    // can allocate, so receiver and key live in roots across them.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(target);
    let key_h = scope.root_nanbox_f64(f64::from_bits(
        crate::value::JSValue::string_ptr(key as *mut _).bits(),
    ));
    if !crate::object::chain_store::mark_chain_hops(&scope, recv_h.get_nanbox_f64()) {
        census(C_PRIME_INTERCEPTED);
        return;
    }
    let recv = (recv_h.get_nanbox_f64().to_bits() & POINTER_MASK) as usize;
    let class_id = (*(recv as *const crate::ObjectHeader)).class_id;
    let verdict_class = if crate::object::is_anon_shape_class_id(class_id) {
        0
    } else {
        class_id
    };
    // The class side of the chain, marked like its prototype objects: an
    // accessor registered for any of these classes from now on moves the
    // generation this memo records.
    crate::object::chain_store::mark_verdict_class_chain(verdict_class);
    let generation = add_generation();
    if crate::object::class_instance_set_may_intercept(recv, verdict_class, key_h.get_nanbox_f64())
    {
        census(C_PRIME_INTERCEPTED);
        return;
    }
    let recv = (recv_h.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    if crate::object::shapes::object_shape_stamp(recv) != post {
        census(C_PRIME_UNVERIFIED);
        return;
    }
    // Own both ShapeIds before they can be stamped from the site.
    crate::object::shape_carriers::note_shape_id(pre);
    crate::object::shape_carriers::note_shape_id(post);
    let site_ptr = site;
    let site = &*site;
    if site.add_shapes.load(Ordering::Relaxed) == PACKED_SET_EMPTY
        && site.add_guard.load(Ordering::Relaxed) == 0
    {
        ADD_SITES.with(|cell| (*cell.get()).push(site_ptr));
    }
    let pre_word = if inline { pre } else { pre ^ SPILL_FLIP };
    let shapes = u64::from(pre_word) | (u64::from(post) << 32);
    let guard = (generation << ADD_SLOT_BITS) | u64::from(n);
    let same_pre = |word: u64| word != PACKED_SET_EMPTY && unflip(word as u32) == pre;
    // A way that holds this pre-shape (a stale guard) is superseded.
    if let Some(ways) = site_ways(site_ptr) {
        for way in ways.iter() {
            if same_pre(way.shapes.load(Ordering::Relaxed)) {
                way.shapes.store(PACKED_SET_EMPTY, Ordering::Relaxed);
            }
        }
    }
    // The newest memo takes the primary words, which the emitted hit
    // compares: a site's first receivers are often transient (the first
    // instance of a class allocates before its width is learned, so its
    // pre-shapes differ from every later instance's). The memo it displaces
    // moves to the first way that is free or holds its pre-shape; with every
    // way taken it is dropped. Way hits never re-prime, so a polymorphic site
    // does not cycle its words.
    let primary = site.add_shapes.load(Ordering::Relaxed);
    if primary != PACKED_SET_EMPTY && !same_pre(primary) {
        let displaced_guard = site.add_guard.load(Ordering::Relaxed);
        if site.add_ways.load(Ordering::Relaxed) == 0 {
            let fresh: Box<AddWays> = Box::new(std::array::from_fn(|_| AddWay {
                shapes: AtomicU64::new(PACKED_SET_EMPTY),
                guard: AtomicU64::new(0),
            }));
            site.add_ways
                .store(Box::into_raw(fresh) as usize as u64, Ordering::Relaxed);
        }
        let displaced_pre = unflip(primary as u32);
        if let Some(way) = site_ways(site_ptr).and_then(|ways| {
            // The home way first, then the rest in order from it.
            let home = add_way_home(displaced_pre);
            (0..ADD_WAYS)
                .map(|i| &ways[(home + i) % ADD_WAYS])
                .find(|way| {
                    let word = way.shapes.load(Ordering::Relaxed);
                    word == PACKED_SET_EMPTY || unflip(word as u32) == displaced_pre
                })
        }) {
            way.shapes.store(PACKED_SET_EMPTY, Ordering::Relaxed);
            way.guard.store(displaced_guard, Ordering::Relaxed);
            way.shapes.store(primary, Ordering::Relaxed);
        }
    }
    // Retire the old pair first, so no reader pairs new shapes with an old
    // guard (one thread publishes; this orders it for the emitted reads).
    site.add_shapes.store(PACKED_SET_EMPTY, Ordering::Relaxed);
    site.add_guard.store(guard, Ordering::Relaxed);
    site.add_shapes.store(shapes, Ordering::Relaxed);
    census(C_PRIME_PUBLISHED);
}

/// A key the append may name: a heap string, not a private name, not a
/// canonical-index candidate (those reach Array-subclass / `Object.prototype`
/// index bookkeeping the append does not perform).
unsafe fn eligible_key(key: *const crate::StringHeader) -> bool {
    let Some(gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
        return false;
    };
    if gc.obj_type != crate::gc::GC_TYPE_STRING || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return false;
    }
    if (*key).byte_len == 0 {
        return false;
    }
    let first = *crate::string::string_data(key);
    first != b'#' && !first.is_ascii_digit()
}

#[cfg(test)]
#[path = "packed_add_tests.rs"]
mod tests;
