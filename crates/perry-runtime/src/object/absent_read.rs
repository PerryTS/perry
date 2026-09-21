//! The third answer a property read can have: **the key is on nothing**.
//!
//! # Why this exists
//!
//! A read that MISSES — `if (obj.maybeThere)`, `obj.x ?? fallback`, a
//! feature-detection branch, a destructured options bag — costs 3,629
//! instructions on a plain object literal and 32,874 on a three-level
//! `Object.create` chain, against 37.3 for an own-property hit on the same
//! object (#10495, marginal per read, `perf stat instructions:u`, min of 3,
//! fitted 500 k -> 5 M). Callgrind says there is no hot spot in that: the cost
//! is that the WHOLE generic lookup runs again per chain level and per
//! consult route. Per miss on a `new K()` receiver: `try_data_get_bytes` 5x,
//! `object_keys_array` 5x, `keys_find_slot_by_bytes_resolved` 9x, `from_utf8`
//! 10x, `meta_capable_object` 15x; on a three-level chain, 22 SipHash probes.
//!
//! The verdict "this key is on nothing" is a function of (receiver class id,
//! ShapeId, interned key) and is stable until a shape, a prototype or a class
//! registry changes, all three of which Perry already has enforced funnels
//! for. So it is memoizable, and it is recorded in the SAME table as
//! [`super::inherited_read_cache`]'s inherited hits: same key, same slot, same
//! GC contract. A parallel table would double the lookup on the path that is
//! already the problem and would need its own root scanner and dead-owner
//! prune — the hardest part of #10834 — for no gain.
//!
//! # What licenses an `Absent` entry: BOTH halves, structurally
//!
//! An entry is recorded only when two independent facts hold, and the type
//! system is what requires both: [`record_absent`] cannot be called without
//! one [`ChainExhausted`] and one [`ObservedUndefined`], and neither has a
//! constructor that does not carry its evidence.
//!
//! * [`ChainExhausted`] — **derived.** The walk enumerated every object the
//!   lookup can consult, from the receiver to a PROVED end of chain, each hop
//!   an ordinary `GC_TYPE_OBJECT` with a stamped shape and no accessor or
//!   customized descriptor for this key, and found the key in NO hop's key
//!   list. Only [`super::inherited_read_cache`]'s walk can mint one.
//! * [`ObservedUndefined`] — **observed.** The generic tail, run once on the
//!   priming read, actually returned `undefined` for this pair.
//!
//! **Neither half is sufficient, and this is not belt-and-braces.**
//!
//! Observation alone is a silent wrong value. `P.a = undefined` puts the key
//! in `P`'s key list with an `undefined` value; the tail returns `undefined`,
//! which is indistinguishable from absence by observation — but `"a" in o` is
//! `true`, and a later `P.a = 5` is a plain VALUE store to an EXISTING key: it
//! transitions no shape, installs no descriptor, touches no registry, and
//! moves not one of the guards below. An entry recorded on the observation
//! would answer `undefined` for the life of the process. The walk is what
//! tells absent from present-and-`undefined`, because it looks at the key
//! LIST rather than at the value. `absent_read_tests::a_prototype_key_holding_undefined_is_not_absent`
//! is the named regression.
//!
//! Derivation alone is also insufficient, for the opposite reason: a key on no
//! hop's key list can still be answered by one of the synthesizers the generic
//! tail consults — a class vtable getter, the prototype-method registry, a
//! `class X extends Request` handle surface, a builtin reflection accessor,
//! `constructor`, a Temporal or URLSearchParams cell. The observation excludes
//! every one of them by construction rather than by an audit that a later
//! commit can fall out of date with: if the tail answers such a key, it did
//! not answer `undefined`, so no token exists and nothing is recorded.
//!
//! # Guards
//!
//! An entry re-proves itself on every read. The receiver identity
//! `(class_id, ShapeId)`, the recorded prototype bits and the one
//! `proto_validity::proto_validity()` word are [`super::inherited_read_cache`]'s
//! existing compares, unchanged. Between them they cover a key ADDED anywhere
//! on the chain (on the receiver it is a shape transition; on a hop it is a
//! structural change to a MARKED prototype, which bumps the validity word), a
//! shadowing own key, a prototype swap at any level (`setPrototypeOf` bumps
//! the semantic epoch, which the validity word folds in), every descriptor
//! install and clear, `delete`, and — since #10842 routed
//! `class_lookup_surface_gen_bump` into the same word — the four
//! class-registry writes `VTABLE_GEN` deliberately does not cover.
//!
//! On top of those, [`AbsentGuards`] carries the ONE word that is none of the
//! above: `class_registry::vtable_generation()`, bumped by method / getter /
//! setter registration. That is a side-table write which transitions no
//! shape, marks no prototype and installs no descriptor, and it is what
//! covers `C.prototype.m = fn` AFTER a miss was cached
//! (`class_registry/prototype_methods.rs`). One relaxed load and one compare
//! on top of what a hit already pays, against 3,629-32,874.
//!
//! **An absent verdict depends on #10842's marking discipline.** The validity
//! word bumps only for objects MARKED as somebody's prototype, so an absent
//! entry is sound only if every hop on the exhausted chain was marked when
//! the verdict was recorded. It was: the walk can only proceed PAST a marked
//! hop — an unmarked one is marked and the walk abandoned (`hop_unmarked`) —
//! so proving exhaustion implies every traversed hop, the terminal
//! `Object.prototype` included, was already marked. If that abandon is ever
//! removed as an optimisation, absent entries go stale silently. The walk
//! says so at the arm.
//!
//! # What is refused at prime time
//!
//! Receivers whose read surface is an address-keyed side table rather than a
//! shape: a `class X extends Request/Response` fetch handle, a Temporal
//! subclass cell, a `URLSearchParams`-shaped object. Those are probes the miss
//! path already pays on every read, so asking them once at prime time costs
//! nothing at steady state, and refusing is always safe: the read stays on the
//! path it is on today.

use super::ObjectHeader;
use crate::value::JSValue;

/// Evidence that the prototype walk enumerated the receiver's WHOLE chain and
/// found the key in no hop's key list.
///
/// The constructor is visible only inside [`super::inherited_read_cache`], so
/// this token cannot be minted by anything but the walk that earns it. It is
/// deliberately zero-sized: it must survive a call into the generic tail,
/// which allocates and can collect, and a token carrying a hop address would
/// be a dangling pointer by the time it was used. [`record_absent`] re-derives
/// the chain itself, under the collector's eye, after the tail has returned.
#[must_use]
pub(crate) struct ChainExhausted(());

impl ChainExhausted {
    /// Mint the derivation half. Reachable only from the walk.
    #[inline]
    pub(in crate::object) fn from_proved_chain_end() -> Self {
        ChainExhausted(())
    }
}

/// Evidence that the generic tail ANSWERED `undefined` for this pair.
///
/// There is exactly one way to make one, and it carries the value it was made
/// from: a caller cannot assert this, only observe it.
#[must_use]
pub(crate) struct ObservedUndefined(());

impl ObservedUndefined {
    /// The only constructor. `None` for any other answer, including `null`.
    #[inline]
    pub(crate) fn from_tail_answer(value: JSValue) -> Option<Self> {
        if value.is_undefined() {
            Some(ObservedUndefined(()))
        } else {
            None
        }
    }
}

/// The ONE global word an absent verdict depends on that is neither a shape
/// transition nor a structural change to a marked prototype. See the module
/// docs for what it covers; this struct is the one place that list is kept.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct AbsentGuards {
    vtable_gen: u64,
}

impl AbsentGuards {
    /// The value every non-absent entry carries. `0` is not a live generation
    /// — the counter starts at 1 and only increases — so a zeroed or stale
    /// entry can never satisfy [`still_valid`](Self::still_valid).
    pub(crate) const ZERO: AbsentGuards = AbsentGuards { vtable_gen: 0 };

    #[inline]
    pub(crate) fn capture() -> Self {
        AbsentGuards {
            vtable_gen: crate::object::class_registry::vtable_generation(),
        }
    }

    #[inline]
    pub(crate) fn still_valid(&self) -> bool {
        *self == AbsentGuards::capture()
    }
}

/// `PERRY_ABSENT_IC=0` turns the absent answer off in a binary that has it, so
/// one build measures both sides one environment variable apart — the
/// discipline `PERRY_INHERITED_IC` and `PERRY_IC_OUTLINE_FASTPATH` established.
/// Both settings answer identically; only the cost differs.
#[inline]
pub(crate) fn absent_cache_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(std::env::var("PERRY_ABSENT_IC").ok().as_deref())
    })
}

// --- counters ---------------------------------------------------------------

use std::sync::atomic::{AtomicU64, Ordering};

static ABSENT_SERVED: AtomicU64 = AtomicU64::new(0);
static ABSENT_RECORDED: AtomicU64 = AtomicU64::new(0);
static ABSENT_REFUSED: AtomicU64 = AtomicU64::new(0);

#[inline]
pub(crate) fn note_absent_served() {
    ABSENT_SERVED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn absent_served() -> u64 {
    ABSENT_SERVED.load(Ordering::Relaxed)
}

pub(crate) fn absent_recorded() -> u64 {
    ABSENT_RECORDED.load(Ordering::Relaxed)
}

pub(crate) fn absent_refused() -> u64 {
    ABSENT_REFUSED.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn test_reset_absent_counters() {
    ABSENT_SERVED.store(0, Ordering::Relaxed);
    ABSENT_RECORDED.store(0, Ordering::Relaxed);
    ABSENT_REFUSED.store(0, Ordering::Relaxed);
}

/// Counters for a caller outside the runtime. `which`: 0 served, 1 recorded,
/// 2 refused. Armed by `PERRY_INHERITED_IC_STATS` like its siblings.
#[no_mangle]
pub extern "C" fn js_absent_read_cache_stats(which: i32) -> f64 {
    match which {
        0 => absent_served() as f64,
        1 => absent_recorded() as f64,
        2 => absent_refused() as f64,
        _ => -1.0,
    }
}

// --- where a walk stopped ---------------------------------------------------

/// Every arm the chain walk can leave by, so a refusal can be attributed
/// rather than guessed at.
///
/// This exists because a coverage question was answered three times by
/// hypothesis and the measurements agreed with none of them. A per-arm tally
/// costs one byte in a stack-local struct and one array bump per walk, behind
/// the same latch as every other counter here, and it turns "the cache
/// declined" into "the cache declined HERE".
pub(crate) mod walk_stop {
    pub(crate) const RESOLVED: u8 = 0;
    pub(crate) const KEY_ADDR: u8 = 1;
    pub(crate) const KEY_HEADER: u8 = 2;
    pub(crate) const KEY_REFUSED: u8 = 3;
    pub(crate) const RECV_ADDR: u8 = 4;
    pub(crate) const RECV_HEADER: u8 = 5;
    pub(crate) const RECV_KIND: u8 = 6;
    pub(crate) const RECV_NOT_ORDINARY: u8 = 7;
    pub(crate) const RECV_NO_STAMP: u8 = 8;
    pub(crate) const RECV_ADDRESS_FACTS: u8 = 9;
    pub(crate) const RECV_PERF_ENTRY: u8 = 10;
    pub(crate) const RECV_BLOOM: u8 = 11;
    pub(crate) const PROTO_NOT_POINTER: u8 = 12;
    pub(crate) const DECL_PROTOTYPE: u8 = 13;
    pub(crate) const NO_PROTOTYPE_ROUTE: u8 = 14;
    pub(crate) const NEXT_NULL_OR_CYCLE: u8 = 15;
    pub(crate) const MAX_HOPS: u8 = 16;
    pub(crate) const HOP_ADDR: u8 = 17;
    pub(crate) const HOP_HEADER: u8 = 18;
    pub(crate) const HOP_KIND: u8 = 19;
    pub(crate) const HOP_NO_SHAPE: u8 = 20;
    pub(crate) const HOP_NOT_ORDINARY: u8 = 21;
    pub(crate) const HOP_NO_STAMP: u8 = 22;
    pub(crate) const HOP_ADDRESS_FACTS: u8 = 23;
    pub(crate) const HOP_EXOTIC: u8 = 24;
    pub(crate) const HOP_BLOOM: u8 = 25;
    pub(crate) const FOUND_SPILLED: u8 = 26;
    pub(crate) const FOUND_HOLE: u8 = 27;
    pub(crate) const FOUND_UNDEFINED: u8 = 28;
    pub(crate) const END_EXPLICIT_NULL: u8 = 29;
    pub(crate) const END_HEADER_NULL: u8 = 30;
    pub(crate) const END_OBJECT_PROTOTYPE: u8 = 31;
    pub(crate) const RECORD_RECV_KEY_LIST: u8 = 32;
    pub(crate) const RECORD_RECV_REFUSED: u8 = 33;
    pub(crate) const RECORD_WRITTEN: u8 = 34;
    pub(crate) const RECORD_RE_WALK_LOST: u8 = 35;
    /// The five gates of `implicit_object_prototype_hop`, separated because
    /// `no_prototype_route` alone said only "the implicit link was refused"
    /// and the whole coverage question is WHICH refusal.
    pub(crate) const IMPLICIT_SYNTHETIC: u8 = 36;
    pub(crate) const IMPLICIT_CLASS_ID: u8 = 37;
    pub(crate) const IMPLICIT_HEADER: u8 = 38;
    pub(crate) const IMPLICIT_NULL_PROTO_FLAG: u8 = 39;
    pub(crate) const IMPLICIT_NO_MEMO: u8 = 40;
    /// #10842's mark-and-abandon: the hop was not yet marked as a
    /// prototype, so the walk marked it and stopped. The absent verdict
    /// rests on this arm — see the module docs.
    pub(crate) const HOP_UNMARKED: u8 = 41;
    pub(crate) const COUNT: usize = 42;

    pub(crate) fn name(code: u8) -> &'static str {
        match code {
            0 => "resolved",
            1 => "key_addr",
            2 => "key_header",
            3 => "key_refused",
            4 => "recv_addr",
            5 => "recv_header",
            6 => "recv_kind",
            7 => "recv_not_ordinary",
            8 => "recv_no_stamp",
            9 => "recv_address_facts",
            10 => "recv_perf_entry",
            11 => "recv_bloom",
            12 => "proto_not_pointer",
            13 => "decl_prototype",
            14 => "no_prototype_route",
            15 => "next_null_or_cycle",
            16 => "max_hops",
            17 => "hop_addr",
            18 => "hop_header",
            19 => "hop_kind",
            20 => "hop_no_shape",
            21 => "hop_not_ordinary",
            22 => "hop_no_stamp",
            23 => "hop_address_facts",
            24 => "hop_exotic",
            25 => "hop_bloom",
            26 => "found_spilled",
            27 => "found_hole",
            28 => "found_undefined",
            29 => "end_explicit_null",
            30 => "end_header_null",
            31 => "end_object_prototype",
            32 => "record_recv_key_list",
            33 => "record_recv_refused",
            34 => "record_written",
            35 => "record_re_walk_lost",
            36 => "implicit_synthetic",
            37 => "implicit_class_id",
            38 => "implicit_header",
            39 => "implicit_null_proto_flag",
            40 => "implicit_no_memo",
            41 => "hop_unmarked",
            _ => "unknown",
        }
    }
}

static WALK_STOPS: [AtomicU64; walk_stop::COUNT] = [const { AtomicU64::new(0) }; walk_stop::COUNT];

/// Account for one walk. Called once per walk by the two callers, so the
/// buckets sum to the number of walks and an arm that never fires is visible
/// as a zero rather than as an absence.
#[inline]
pub(crate) fn note_walk_stop(code: u8) {
    if (code as usize) < walk_stop::COUNT {
        WALK_STOPS[code as usize].fetch_add(1, Ordering::Relaxed);
    }
}

/// Non-zero buckets, most frequent first, for the `PERRY_IC_DIAG` report.
pub(crate) fn walk_stop_report() -> String {
    let mut rows: Vec<(u64, &'static str)> = (0..walk_stop::COUNT)
        .map(|i| {
            (
                WALK_STOPS[i].load(Ordering::Relaxed),
                walk_stop::name(i as u8),
            )
        })
        .filter(|(n, _)| *n != 0)
        .collect();
    rows.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    rows.iter()
        .map(|(n, nm)| format!("{nm}={n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
pub(crate) fn test_reset_walk_stops() {
    for cell in WALK_STOPS.iter() {
        cell.store(0, Ordering::Relaxed);
    }
}

/// `PERRY_ABSENT_TRACE=1` prints one line per absent RECORD and per absent
/// SERVE, with the receiver identity the entry is keyed on. Off by default and
/// gated at the call site, so it costs one `OnceLock` load when it is off.
///
/// This exists because `fx/inval.ts` route 6 prints a wrong value and the
/// identity of the entry that answers it is the whole question: whether the
/// verdict was recorded against the shape the re-add lands on, or against a
/// different one.
pub(crate) fn trace_absent(
    what: &str,
    class_id: u32,
    shape: u32,
    key: *const crate::StringHeader,
    hops: u8,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("PERRY_ABSENT_TRACE").is_some()) {
        return;
    }
    let name = unsafe { crate::string::header_str_checked(key) }.unwrap_or("<?>");
    eprintln!("[absent] {what} class={class_id:#x} shape={shape:#x} hops={hops} key={name}");
}

// --- the prime-time refusal list --------------------------------------------

/// Receivers whose read surface is an address-keyed side table rather than a
/// shape. Asked ONCE, at prime time, on a path that already pays every one of
/// these probes per read today.
///
/// # Safety
/// `obj` is a masked, non-null heap pointer the caller has proved is an
/// ordinary `GC_TYPE_OBJECT`.
pub(crate) unsafe fn receiver_may_record_absent(obj: *const ObjectHeader) -> bool {
    let addr = obj as usize;
    if crate::object::field_get_set::fetch_subclass_handle_id(addr).is_some() {
        return false;
    }
    if crate::object::temporal_subclass_cell(addr).is_some() {
        return false;
    }
    if crate::url::search_params::shape_is_url_search_params(obj) {
        return false;
    }
    true
}

// --- the one call site ------------------------------------------------------

/// Run the generic tail for a read the caches could not serve, and — when the
/// walk proved the chain exhausted — record the `undefined` it answers.
///
/// This is the ONLY way an absent entry is written. `exhausted` is the walk's
/// derivation; the [`ObservedUndefined`] below is the tail's observation; both
/// are required by [`record_absent`]'s signature.
///
/// `obj` and `key` are rooted across the tail call only on the recording path
/// — the tail allocates and can collect, and the record needs them afterwards.
/// A read with no candidate (the overwhelming majority, once a pair is
/// recorded) takes the plain call and pays nothing.
///
/// # Safety
/// As `get_field_by_name_past_inherited_cache`.
pub(crate) unsafe fn tail_and_maybe_record_absent(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    exhausted: Option<ChainExhausted>,
) -> JSValue {
    let Some(exhausted) = exhausted else {
        return crate::object::field_get_set::get_field_by_name::get_field_by_name_past_inherited_cache(
            obj, key,
        );
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(obj as i64));
    let key_h = scope.root_nanbox_f64(crate::value::nanbox_string_key(key));
    let value =
        crate::object::field_get_set::get_field_by_name::get_field_by_name_past_inherited_cache(
            obj, key,
        );
    if let Some(observed) = ObservedUndefined::from_tail_answer(value) {
        let obj =
            crate::value::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *const ObjectHeader;
        let key = crate::value::js_nanbox_get_pointer(key_h.get_nanbox_f64())
            as *const crate::StringHeader;
        record_absent(obj, key, exhausted, observed);
    } else {
        ABSENT_REFUSED.fetch_add(1, Ordering::Relaxed);
    }
    value
}

/// Write the absent entry.
///
/// Takes both tokens by value so the conjunction is the signature rather than
/// a comment, and then **re-derives the chain itself** rather than trusting
/// the token's provenance: the tail between the two has run user-visible code
/// and may have collected, added the key, or re-parented the chain. A re-walk
/// that no longer proves exhaustion records nothing, which is exactly the
/// right answer in every one of those cases.
///
/// # Safety
/// `obj` and `key` are live, re-read from roots after the tail call.
unsafe fn record_absent(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    _derived: ChainExhausted,
    _observed: ObservedUndefined,
) {
    if super::inherited_read_cache::inherited_read_cache_record_absent(obj, key) {
        ABSENT_RECORDED.fetch_add(1, Ordering::Relaxed);
    } else {
        ABSENT_REFUSED.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "absent_read_tests.rs"]
mod tests;
