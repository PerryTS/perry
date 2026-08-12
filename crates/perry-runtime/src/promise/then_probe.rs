//! #7910 — a provable-negative fast path for the spec's assimilation probe
//! `Get(resolution, "then")`.
//!
//! ECMA-262 27.2.1.3.2 makes EVERY promise resolution read `then` off the
//! resolution value. For a plain object that read answers `undefined` and does
//! so expensively: a `setjmp` exception frame, a re-intern of the literal
//! `"then"` (`js_string_from_bytes` + `core::str::from_utf8`), the fully
//! generic dynamic getter's preamble (proxy band / JS handle / string tag /
//! class-ref tests), a linear own-key scan with a `js_string_key_matches` per
//! key, and finally a recursive `js_object_get_field_by_name` walk into
//! `Object.prototype` for the miss. Measured on `gc-handoff/apps/asyncpipe.ts`:
//! resolving with an OBJECT costs +78.5 % instructions over resolving with a
//! NUMBER on byte-identical promise/microtask/closure topology, and this probe
//! is ~9 % of the whole program (`gc-handoff/ASYNC2-NOTES.md` §4).
//!
//! # Why this is a correctness problem, not a perf problem
//!
//! A fast path that wrongly answers "no `then`" makes a genuine thenable stop
//! being assimilated, and the failure mode is a **HANG** — the awaiting promise
//! never settles. So this module is written to fail CLOSED: every predicate
//! below returns `false` ("don't know") for anything it cannot prove, and the
//! caller then runs the unmodified spec path.
//!
//! # The two halves of the proof
//!
//! **Half 1 — the receiver, re-proved on every single probe.** Nothing about
//! the resolution value is ever cached. [`definitely_no_then`] proves, from the
//! object's own header and keys array, that its `[[Get]]("then")` is exactly
//! "own data keys, then `Object.prototype`":
//!
//! * arena-classified address ⇒ its `GcHeader` is real, and no malloc-backed
//!   exotic (Buffer / TypedArray / Date cell / RegExp / Temporal) can classify
//!   as arena; proxy ids, stream ids and small handles are all below the arena;
//! * `GC_TYPE_OBJECT` ⇒ not an array, closure, promise, error, Map, Set,
//!   BigInt, string, or native handle;
//! * `OBJECT_TYPE_REGULAR` ⇒ not an Error host and not a per-evaluation class
//!   object (whose statics resolve through the class registry);
//! * no `OBJ_FLAG_HAS_DESCRIPTORS` / `OBJ_FLAG_ARRAY_DESCRIPTORS` /
//!   `OBJ_FLAG_TYPED_ARRAY_PROTO` ⇒ no own accessor or non-default descriptor
//!   can shadow the answer;
//! * an ADMISSIBLE class id ([`class_id_admissible`]): `0` (a plain object
//!   literal), or an anonymous SHAPE id that the class registries provably
//!   know nothing about — no vtable entry, no class prototype object of either
//!   flavour, no parent edge. That excludes every class instance, every
//!   plain-function-constructor instance, the native-module namespace id,
//!   WeakMap/WeakSet, Map/Set iterators, DisposableStack, boxed String,
//!   AbortSignal and TTY hosts, so no `CLASS_VTABLE_REGISTRY` getter or method
//!   named `then` and no class prototype chain is in play;
//! * `native_this_alias::alias_active()` is false — `js_object_get_field_by_name_f64`
//!   forwards a MISSED read to an aliased native handle, a layer above the
//!   lookup this module models;
//! * `PROMISE_SUBCLASS_EVER` is false — the `class X extends Promise` arm is
//!   keyed on `"then"` specifically and probes an INHERITED backing key;
//! * no own `then` key (a direct dense scan of the keys array — this is the
//!   common REAL thenable, `{ then(res, rej) {…} }`, and it must always be
//!   found);
//! * `OBJ_FLAG_NULL_PROTO` ⇒ the own keys ARE the whole chain, answer proven;
//! * otherwise no per-instance `setPrototypeOf` / `__proto__` override
//!   (`object_static_prototype`) ⇒ the chain is exactly
//!   `obj → Object.prototype → null`.
//!
//! Two arms are reachable by such a receiver and are stopped by their own
//! secondary probe rather than by a gate above: `arguments` objects (class 0,
//! but `arguments_object_get_field` answers only for an OWN key, excluded by
//! the own-`then` scan) and `URLSearchParams`-shaped objects (class 0 with
//! `keys[0] == "_entries"`, but its method table has no `then`).
//!
//! **Half 2 — `Object.prototype`, cached under a signature.** The verdict
//! "`Get(Object.prototype, "then")` is `undefined`" is computed by CALLING THE
//! REAL LOOKUP once, so it can never *disagree* with the slow path at compute
//! time. Only staleness is a hazard, and staleness is what
//! [`ProtoVerdict`]'s signature guards:
//!
//! | mutation route into the chain | caught by |
//! |---|---|
//! | `Object.prototype.then = f` (plain assignment, `Reflect.set`, `Object.assign`) | a NEW own data key must land in the keys array: either it grows in place (`keys_len` changes) or the array is reallocated / transitioned (`keys_addr` changes — the fast store lane calls `set_object_keys_array`) |
//! | `Object.defineProperty` / `Object.defineProperties` / `Reflect.defineProperty` | descriptor install ⇒ `prop_plan_epoch_bump()` (`object/descriptor_state.rs`), and/or the keys-array change above |
//! | `delete Object.prototype.then` | `js_object_delete_field` ⇒ `prop_plan_epoch_bump()` (and a delete only ever makes the verdict MORE true) |
//! | `Object.setPrototypeOf(Object.prototype, x)` / `__proto__ =` | instance-override recording ⇒ `prop_plan_epoch_bump()` (`object/prototype_chain.rs`) |
//! | a vtable getter/method named `then` registered for the prototype's class id | `VTABLE_GEN` (`class_registry::vtable_generation`) |
//! | an ACCESSOR `then` on `Object.prototype` (whatever it returns) | never memoized as "no `then`" at all — see [`compute_object_prototype_then`]; the getter is observable and must run on every probe |
//! | a garbage collection RELOCATING `Object.prototype` or its keys array | both addresses are re-derived from live objects on every probe and compared; a relocation is an address mismatch, i.e. a miss |
//! | `Object.prototype` itself being replaced | `proto_addr` is re-derived every probe and compared |
//!
//! The signature keys on `prop_plan_semantic_epoch()`, **not** the full
//! `PROP_PLAN_EPOCH`. That distinction is load-bearing in both directions.
//! Soundness: GC never adds or removes a property, and the only thing it does
//! that this verdict could care about — moving the prototype or its keys array
//! — is caught by the address comparison, because those addresses are re-read
//! from the live object rather than remembered. Performance: the incremental
//! collector's root scan bumps the full epoch at loop-poll cadence, so keying
//! on it made the entry invalid on essentially every probe and turned the cache
//! into an unconditional, MORE expensive recompute — measured at **+35 %**
//! instructions on `asyncpipe` versus **−24.6 %** with the recompute gone.
//!
//! Note what is deliberately NOT on that list: a `then` installed on the
//! resolution object ITSELF, or a `setPrototypeOf` on it. Those are Half 1 and
//! are re-checked from scratch on every probe, never cached.
//!
//! # Instruments
//!
//! * `PERRY_MT_PROFILE=1` prints `thenable_fast=N` plus a bucketed
//!   `[mt-profile] then_probe:` histogram naming the gate that declined and how
//!   often the `Object.prototype` verdict was RECOMPUTED. A run with `fast=0`
//!   measured nothing; a run whose `verdict_compute` tracks `fast` has a cache
//!   that is not caching. Both have already happened once each on this change.
//! * `PERRY_THENABLE_VERIFY=1` re-runs the FULL spec lookup every time the fast
//!   path answers "no `then`" and aborts the process on any disagreement. This
//!   is the sabotage detector: break the invalidation and a verify run dies
//!   loudly instead of hanging.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::object::ObjectHeader;
use crate::value::JSValue;

/// The one key this module is about.
const THEN_KEY: &[u8] = b"then";

/// Times the fast negative fired (reported by `PERRY_MT_PROFILE=1`).
pub static MT_THENABLE_FAST_NEGATIVE: AtomicU64 = AtomicU64::new(0);

/// Times the `Object.prototype` verdict had to be RECOMPUTED by running the
/// real lookup. This should be a handful per process; if it tracks the probe
/// count, the signature is being invalidated by something that is not a
/// property change and the "cache" is a pure cost. That is not hypothetical —
/// it is exactly what keying on the full `PROP_PLAN_EPOCH` did (#7910).
static VERDICT_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

// ── Half 2: the `Object.prototype` verdict and its signature ───────────────

/// Everything about `Object.prototype` (and the process-global counters that
/// track mutations it could hide behind) that the verdict depends on. Every
/// field is an identity token that is only ever COMPARED, never dereferenced,
/// so this is not a GC root: a stale address can at worst fail to match (a
/// miss, which recomputes). Any collection bumps `PROP_PLAN_EPOCH` anyway,
/// which invalidates the entry outright.
///
/// **Adding a mutation route means adding a field here.** `derive(PartialEq)`
/// is deliberate: the whole struct is compared, so a route covered by a new
/// field cannot be forgotten at the comparison site.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ProtoSignature {
    /// Identity of `Object.prototype` itself.
    proto_addr: usize,
    /// Its own-key array — a new own DATA property either grows it in place or
    /// transitions it to a different array.
    keys_addr: usize,
    keys_len: u32,
    /// `OBJ_FLAG_HAS_DESCRIPTORS` &co — an accessor / non-default descriptor.
    obj_flags: u16,
    class_id: u32,
    /// `object::prop_plan::PROP_PLAN_EPOCH` — descriptor installs and clears,
    /// prototype recording, deletes, and every GC collection.
    epoch: u64,
    /// `class_registry::VTABLE_GEN` — getter/setter/method registration.
    vtable_gen: u64,
}

/// Cached "`Object.prototype` has no reachable `then`" verdict plus the
/// signature it was computed under.
#[derive(Clone, Copy)]
struct ProtoVerdict {
    signature: ProtoSignature,
    /// The cached answer: `Get(Object.prototype, "then")` was `undefined` AND
    /// the read had no observable side effect worth repeating.
    no_then: bool,
}

crate::perry_thread_local! {
    static PROTO_VERDICT: std::cell::Cell<Option<ProtoVerdict>> =
        const { std::cell::Cell::new(None) };
    /// Re-entrancy latch. Computing the verdict calls the real property
    /// lookup on `Object.prototype`, which can run a user getter, which can
    /// resolve a promise and land back here. Recursing would be unbounded, so
    /// the inner probe simply declines the fast path.
    static VERDICT_IN_FLIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The mutable state of `Object.prototype` that a `then` could hide in,
/// reduced to a comparable tuple. `None` when the prototype is not in a shape
/// this module is willing to reason about.
unsafe fn proto_signature(proto_addr: usize) -> Option<(usize, u32, u16, u32)> {
    let header = crate::value::addr_class::try_read_gc_header(proto_addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let obj = proto_addr as *const ObjectHeader;
    let keys = (*obj).keys_array;
    let keys_addr = keys as usize;
    let keys_len = if keys.is_null() {
        0
    } else {
        if (keys_addr as u64) >> 48 != 0
            || !crate::value::addr_class::is_above_handle_band(keys_addr)
        {
            return None;
        }
        let kh = crate::value::addr_class::try_read_gc_header(keys_addr)?;
        if kh.obj_type != crate::gc::GC_TYPE_ARRAY
            || kh.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return None;
        }
        let len = (*(keys as *const crate::array::ArrayHeader)).length;
        let cap = (*(keys as *const crate::array::ArrayHeader)).capacity;
        if len > cap {
            return None;
        }
        len
    };
    Some((keys_addr, keys_len, header._reserved, (*obj).class_id))
}

/// `true` when `Get(Object.prototype, "then")` is provably `undefined`.
///
/// Computed by running the REAL lookup and cached under [`ProtoSignature`]. A
/// signature that cannot be taken, or a lookup that moved the world underneath
/// us, declines to cache and reports `false`.
fn object_prototype_has_no_then() -> bool {
    let proto_addr = crate::array::object_prototype_addr();
    if proto_addr == 0 {
        return false;
    }
    let epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
    let vtable_gen = crate::object::vtable_generation();
    let Some((keys_addr, keys_len, obj_flags, class_id)) = (unsafe { proto_signature(proto_addr) })
    else {
        return false;
    };

    let now = ProtoSignature {
        proto_addr,
        keys_addr,
        keys_len,
        obj_flags,
        class_id,
        epoch,
        vtable_gen,
    };
    if let Some(v) = PROTO_VERDICT.with(|c| c.get()) {
        if v.signature == now {
            return v.no_then;
        }
    }

    // Cold: recompute by asking the real lookup. Guard against re-entry — the
    // lookup can run a user getter installed on `Object.prototype`.
    if VERDICT_IN_FLIGHT.with(|c| c.get()) {
        return false;
    }
    VERDICT_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    VERDICT_IN_FLIGHT.with(|c| c.set(true));
    let answer = compute_object_prototype_then(proto_addr);
    VERDICT_IN_FLIGHT.with(|c| c.set(false));
    let Some(no_then) = answer else {
        return false;
    };

    // Only cache when nothing moved across the lookup. A getter that mutated
    // `Object.prototype` (or a collection triggered by one) would otherwise be
    // memoized under a signature that no longer describes the state the answer
    // was computed from.
    let proto_after = crate::array::object_prototype_addr();
    let after = unsafe { proto_signature(proto_after) }.map(|(k, l, f, c)| ProtoSignature {
        proto_addr: proto_after,
        keys_addr: k,
        keys_len: l,
        obj_flags: f,
        class_id: c,
        epoch: crate::object::prop_plan::prop_plan_semantic_epoch(),
        vtable_gen: crate::object::vtable_generation(),
    });
    if after == Some(now) {
        PROTO_VERDICT.with(|c| {
            c.set(Some(ProtoVerdict {
                signature: now,
                no_then,
            }))
        });
    }
    no_then
}

/// Run the real `Get(Object.prototype, "then")`. `None` when the read threw
/// (an accessor on the prototype) — an abrupt completion is emphatically not a
/// "no `then`" answer and must reach the spec path.
fn compute_object_prototype_then(proto_addr: usize) -> Option<bool> {
    // An ACCESSOR `then` on `Object.prototype` whose getter happens to return
    // `undefined` is not "no `then`": the getter is OBSERVABLE (it runs, it
    // sees `this`, it can count calls or mutate). Reading undefined once and
    // memoizing it would suppress every later invocation, which is a real
    // semantic difference, not just a faster answer. Record the CONSERVATIVE
    // verdict for that state so the signature still short-circuits the
    // recompute, while every probe keeps taking the spec path.
    if crate::object::descriptor_state::get_accessor_descriptor(proto_addr, "then").is_some() {
        return Some(false);
    }
    // Likewise if `Object.prototype` itself was given a prototype: the chain
    // above it is not modelled here, and an accessor could live on it.
    if crate::object::prototype_chain::object_static_prototype(proto_addr).is_some() {
        return Some(false);
    }
    let proto_value = crate::value::js_nanbox_pointer(proto_addr as i64);
    let read = super::combinators::combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(
            proto_value,
            THEN_KEY.as_ptr() as *const i8,
            THEN_KEY.len(),
        )
    });
    match read {
        Ok(v) => Some(JSValue::from_bits(v.to_bits()).is_undefined()),
        Err(_) => None,
    }
}

// ── Half 1: the receiver ───────────────────────────────────────────────────

enum OwnScan {
    /// The keys array was read end to end and contains no `then`.
    NoThen,
    /// An own `then` key exists.
    HasThen,
    /// The keys array is not a shape this module will scan.
    Unknown,
}

/// Direct dense scan of an object's own key names for `"then"`.
///
/// Deliberately self-contained rather than a call into
/// `own_data_field_by_name`: that helper resolves the VALUE (an overflow-map
/// probe, `js_object_get_field`'s guards), and presence is all this needs. The
/// keys array holds every own data key — inline and overflow slots alike are
/// indexed by position in it — so a full scan is a complete presence test.
unsafe fn own_then_scan(obj: *const ObjectHeader) -> OwnScan {
    let keys = (*obj).keys_array;
    if keys.is_null() {
        // No own named properties at all.
        return OwnScan::NoThen;
    }
    let keys_addr = keys as usize;
    if (keys_addr as u64) >> 48 != 0 || !crate::value::addr_class::is_above_handle_band(keys_addr) {
        return OwnScan::Unknown;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(keys_addr) else {
        return OwnScan::Unknown;
    };
    if header.obj_type != crate::gc::GC_TYPE_ARRAY
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0
    {
        return OwnScan::Unknown;
    }
    let arr = keys as *const crate::array::ArrayHeader;
    let len = (*arr).length;
    if len > (*arr).capacity || len > 4096 {
        return OwnScan::Unknown;
    }
    let elements =
        (keys as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    for i in 0..len as usize {
        let raw = std::ptr::read(elements.add(i));
        let bits = raw.to_bits();
        if bits == crate::value::TAG_HOLE {
            return OwnScan::Unknown;
        }
        if crate::string::js_string_key_matches_bytes(JSValue::from_bits(bits), THEN_KEY) {
            return OwnScan::HasThen;
        }
    }
    OwnScan::NoThen
}

/// `true` when `value`'s `[[Get]]("then")` is PROVABLY `undefined`.
///
/// Conservative in one direction only: a `false` return means "unknown, run the
/// spec path", never "there is a `then`". See the module docs for the full
/// proof obligation each gate discharges.
pub(crate) fn definitely_no_then(value: f64) -> bool {
    let outcome = unsafe { prove_no_then(value) };
    if crate::promise::MT_PROFILE_ENABLED.load(Ordering::Relaxed) {
        note_outcome(outcome);
    }
    if outcome != Outcome::Proved {
        return false;
    }
    if verify_enabled() {
        verify_against_spec_path(value);
    }
    true
}

/// Why a probe did or did not take the fast negative. `PERRY_MT_PROFILE=1`
/// prints the histogram; a fast path that silently stopped applying shows up
/// as a shifted bucket rather than as an unexplained flat A/B.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Proved,
    NotAPointer,
    NotArena,
    NotPlainObject,
    HasDescriptors,
    ClassId,
    NativeAlias,
    PromiseSubclass,
    OwnThen,
    KeysUnscannable,
    InstancePrototype,
    ProtoVerdict,
}

const OUTCOME_NAMES: [&str; 12] = [
    "fast",
    "not_ptr",
    "not_arena",
    "not_plain",
    "descriptors",
    "class_id",
    "native_alias",
    "promise_subclass",
    "own_then",
    "keys_unscannable",
    "instance_proto",
    "proto_verdict",
];

static OUTCOME_COUNTS: [AtomicU64; 12] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

fn note_outcome(outcome: Outcome) {
    OUTCOME_COUNTS[outcome as usize].fetch_add(1, Ordering::Relaxed);
    if outcome == Outcome::Proved {
        MT_THENABLE_FAST_NEGATIVE.fetch_add(1, Ordering::Relaxed);
    }
}

/// `PERRY_MT_PROFILE=1` line: every non-zero decline bucket, so a run that
/// measured nothing says WHY.
pub(crate) fn outcome_histogram() -> String {
    let mut out = String::new();
    for (i, name) in OUTCOME_NAMES.iter().enumerate() {
        let n = OUTCOME_COUNTS[i].load(Ordering::Relaxed);
        if n != 0 {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("{name}={n}"));
        }
    }
    if out.is_empty() {
        out.push_str("(no probes)");
    }
    out.push_str(&format!(
        " verdict_compute={}",
        VERDICT_RECOMPUTES.load(Ordering::Relaxed)
    ));
    out
}

unsafe fn prove_no_then(value: f64) -> Outcome {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return Outcome::NotAPointer;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if addr == 0 {
        return Outcome::NotAPointer;
    }
    // Inside a registered arena page ⇒ the `GcHeader` back-read below is real.
    // Also excludes every malloc-backed exotic and every small-id band.
    if crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown {
        return Outcome::NotArena;
    }
    let Some(gc) = crate::value::addr_class::try_read_gc_header(addr) else {
        return Outcome::NotArena;
    };
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return Outcome::NotPlainObject;
    }
    let flags = gc._reserved;
    // Any own descriptor / accessor, or the TypedArray-prototype host shape,
    // means the answer can come from somewhere this module does not look.
    const BLOCKING: u16 = crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
        | crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS
        | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    if flags & BLOCKING != 0 {
        return Outcome::HasDescriptors;
    }
    let obj = addr as *const ObjectHeader;
    if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR {
        return Outcome::NotPlainObject;
    }
    if !class_id_admissible((*obj).class_id) {
        return Outcome::ClassId;
    }
    // `js_object_get_field_by_name_f64` forwards a MISSED read to an aliased
    // native handle (`http.Server.call(this, …)` inherits pattern) — a layer
    // above `js_object_get_field_by_name` that this module does not model.
    // Aliased receivers are constructor-built and so carry a synthetic class
    // id, but that is incidental; gate on the fact itself. One thread-local
    // `Cell` read, false in every program that does not use the pattern.
    if crate::object::native_this_alias::alias_active() {
        return Outcome::NativeAlias;
    }
    // `class X extends Promise` arms the `then`-keyed subclass probe in
    // `js_object_get_field_by_name`, whose backing-key read is INHERITED and
    // therefore reaches `Object.prototype`. Nothing else in this module models
    // that key, so once any Promise subclass has been constructed, stand down.
    // One relaxed atomic load; false in every program without such a subclass.
    if super::subclass::PROMISE_SUBCLASS_EVER.load(Ordering::Relaxed) {
        return Outcome::PromiseSubclass;
    }
    match own_then_scan(obj) {
        OwnScan::HasThen => return Outcome::OwnThen,
        OwnScan::Unknown => return Outcome::KeysUnscannable,
        OwnScan::NoThen => {}
    }
    // A null-prototype object's own keys are its whole `[[Get]]` chain.
    if flags & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
        return Outcome::Proved;
    }
    // A per-instance `setPrototypeOf` / `__proto__` override puts an arbitrary
    // chain in front of the intrinsic one.
    if crate::object::prototype_chain::object_static_prototype(addr).is_some() {
        return Outcome::InstancePrototype;
    }
    if object_prototype_has_no_then() {
        Outcome::Proved
    } else {
        Outcome::ProtoVerdict
    }
}

// ── Admissible receiver class ids ──────────────────────────────────────────

/// Memo for [`class_id_admissible`]. `VTABLE_GEN` covers method/getter/setter
/// registration; the SEMANTIC epoch covers class-prototype-object registration
/// and parent-static linking (both call `prop_plan_epoch_bump`). GC is
/// deliberately not an input: it never adds a registry entry, and the
/// dead-owner prune only ever REMOVES entries for objects that are dead, which
/// can make a stale `false` verdict conservative but never a stale `true` one.
#[derive(Clone, Copy)]
struct AdmissibleEntry {
    class_id: u32,
    vtable_gen: u64,
    epoch: u64,
    admissible: bool,
}

const ADMISSIBLE_SLOTS: usize = 16;

const EMPTY_ADMISSIBLE: AdmissibleEntry = AdmissibleEntry {
    // 0 is answered without consulting the memo, so it is a safe "empty" tag.
    class_id: 0,
    vtable_gen: 0,
    epoch: 0,
    admissible: false,
};

crate::perry_thread_local! {
    /// Per-slot `Cell`s rather than a `Cell<[…; N]>`: the latter copies the
    /// whole table in AND out on every probe, which on this path is per promise
    /// resolution.
    static ADMISSIBLE_MEMO: [std::cell::Cell<AdmissibleEntry>; ADMISSIBLE_SLOTS] =
        const { [const { std::cell::Cell::new(EMPTY_ADMISSIBLE) }; ADMISSIBLE_SLOTS] };
}

/// May a receiver with this `class_id` be reasoned about here?
///
/// `0` — a plain object literal — always. A NONZERO id must clear two
/// independent bars:
///
/// 1. `is_anon_shape_class_id` — positive membership in a set codegen
///    populates only for `__AnonShape_*`, the synthetic ids minted for CLOSED
///    object-literal shapes. This is what excludes the reserved builtin ids
///    (`WeakMap`/`WeakSet`, Map/Set iterators, `DisposableStack`, boxed
///    `String`, `AbortSignal`, TTY …), which are recognised by *constant
///    comparison* in their arms and never appear in any registry.
/// 2. [`class_registry_inert`] — the class registries actually know nothing
///    about the id. Without this, admitting anon shapes would rest on two
///    COMPILE-TIME facts nothing checks at runtime (`is_closed_shape` refusing
///    getters/setters/methods, and codegen skipping `js_register_class_name`
///    for `__AnonShape_*`). A future lowering that admitted an accessor into
///    the closed-shape path would arm a missed thenable — a HANG — silently.
///    Checking the registry converts that assumption into an observed fact.
fn class_id_admissible(class_id: u32) -> bool {
    if class_id == 0 {
        return true;
    }
    if class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return false;
    }
    let vtable_gen = crate::object::vtable_generation();
    let epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
    let slot = (class_id as usize).wrapping_mul(0x9E37_79B1) >> 12 & (ADMISSIBLE_SLOTS - 1);
    let e = ADMISSIBLE_MEMO.with(|t| t[slot].get());
    if e.class_id == class_id && e.vtable_gen == vtable_gen && e.epoch == epoch {
        return e.admissible;
    }
    let admissible =
        crate::object::is_anon_shape_class_id(class_id) && class_registry_inert(class_id);
    ADMISSIBLE_MEMO.with(|t| {
        t[slot].set(AdmissibleEntry {
            class_id,
            vtable_gen,
            epoch,
            admissible,
        })
    });
    admissible
}

/// `true` when every `class_id != 0` arm of the property-read path resolves
/// nothing for `class_id`: no vtable (getters, setters, methods), no class
/// prototype object of either flavour, and no parent edge. Those four
/// registries are the entire input to `resolve_proto_chain_field_with_receiver`,
/// `lookup_prototype_method`, `lookup_class_method_in_chain` and the
/// `CLASS_VTABLE_REGISTRY` getter/method dispatch in the dynamic getter.
fn class_registry_inert(class_id: u32) -> bool {
    if crate::object::get_parent_class_id(class_id).is_some() {
        return false;
    }
    if !crate::object::class_prototype_object(class_id).is_null() {
        return false;
    }
    if !crate::object::class_decl_prototype_object(class_id).is_null() {
        return false;
    }
    match crate::object::CLASS_VTABLE_REGISTRY.read() {
        Ok(guard) => match guard.as_ref() {
            Some(map) => !map.contains_key(&class_id),
            None => true,
        },
        // A poisoned lock is not a proof of anything.
        Err(_) => false,
    }
}

// ── Verification mode ──────────────────────────────────────────────────────

fn verify_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("PERRY_THENABLE_VERIFY").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// `PERRY_THENABLE_VERIFY=1`: re-run the unmodified spec lookup and abort if it
/// disagrees with the fast negative. A wrong fast negative in production is a
/// HANG, which is invisible; under this knob it is an immediate, named abort.
#[cold]
fn verify_against_spec_path(value: f64) {
    let read = super::combinators::combinator_catch_js(|| unsafe {
        crate::value::js_dynamic_object_get_property(
            value,
            THEN_KEY.as_ptr() as *const i8,
            THEN_KEY.len(),
        )
    });
    let disagreement = match read {
        Ok(v) => {
            if JSValue::from_bits(v.to_bits()).is_undefined() {
                None
            } else {
                Some(format!("spec path returned a non-undefined `then` ({v:?})"))
            }
        }
        Err(_) => Some("spec path threw while reading `then`".to_string()),
    };
    if let Some(why) = disagreement {
        eprintln!(
            "[thenable-verify] FAST NEGATIVE IS WRONG for value {:#x}: {}",
            value.to_bits(),
            why
        );
        std::process::abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_signature() -> ProtoSignature {
        ProtoSignature {
            proto_addr: 0x1000,
            keys_addr: 0x2000,
            keys_len: 3,
            obj_flags: 0,
            class_id: 0,
            epoch: 7,
            vtable_gen: 11,
        }
    }

    /// The verdict is only reused when EVERY tracked input is unchanged. Each
    /// field below stands for a mutation route into `Object.prototype`
    /// (module docs), so dropping one from the comparison — or from the struct
    /// — turns a real invalidation into a missed thenable, i.e. a hang. This
    /// test fails if any single field stops participating.
    #[test]
    fn every_signature_field_invalidates_the_verdict() {
        let base = sample_signature();
        assert_eq!(base, sample_signature());

        let mutations: [(&str, fn(&mut ProtoSignature)); 7] = [
            ("proto_addr — Object.prototype replaced", |s| {
                s.proto_addr += 8
            }),
            ("keys_addr — keys array transitioned", |s| {
                s.keys_addr += 8
            }),
            ("keys_len — own data key appended", |s| s.keys_len += 1),
            ("obj_flags — descriptor installed", |s| {
                s.obj_flags |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
            }),
            ("class_id", |s| s.class_id += 1),
            (
                "epoch — defineProperty / delete / setPrototypeOf / any GC",
                |s| s.epoch += 1,
            ),
            ("vtable_gen — getter or method registered", |s| {
                s.vtable_gen += 1
            }),
        ];
        for (why, mutate) in mutations {
            let mut changed = base;
            mutate(&mut changed);
            assert_ne!(
                base, changed,
                "signature must not match after a change to: {why}"
            );
        }
    }

    #[test]
    fn primitives_and_non_pointers_never_take_the_fast_path() {
        // Numbers, undefined, null, booleans, int32s: not POINTER_TAG.
        assert!(!definitely_no_then(1.5));
        assert!(!definitely_no_then(f64::from_bits(
            crate::value::TAG_UNDEFINED
        )));
        assert!(!definitely_no_then(f64::from_bits(crate::value::TAG_NULL)));
        assert!(!definitely_no_then(f64::from_bits(crate::value::TAG_TRUE)));
        assert!(!definitely_no_then(f64::from_bits(
            crate::value::INT32_TAG | 7
        )));
        // A STRING_TAG value is a pointer, but not a POINTER_TAG one.
        assert!(!definitely_no_then(f64::from_bits(
            crate::value::STRING_TAG | 0x1234
        )));
    }

    #[test]
    fn a_null_or_unmapped_pointer_never_takes_the_fast_path() {
        assert!(!definitely_no_then(f64::from_bits(
            crate::value::POINTER_TAG
        )));
        // Small-handle band: not arena-classified.
        assert!(!definitely_no_then(f64::from_bits(
            crate::value::POINTER_TAG | 0x2000
        )));
    }

    /// A real heap object with an own `then` must ALWAYS be reported as
    /// unknown — this is the shape whose miss would hang a program.
    #[test]
    fn an_own_then_key_defeats_the_fast_path() {
        unsafe {
            let obj = crate::object::js_object_alloc(0, 4);
            assert!(!obj.is_null());
            let value = crate::value::js_nanbox_pointer(obj as i64);
            let key = crate::string::js_string_from_bytes(b"then".as_ptr(), 4);
            crate::object::js_object_set_field_by_name(obj, key, 1.0);
            match own_then_scan(obj as *const ObjectHeader) {
                OwnScan::HasThen => {}
                _ => panic!("own `then` key must be found by the dense scan"),
            }
            assert!(
                !definitely_no_then(value),
                "an object carrying an own `then` must never take the fast negative"
            );
        }
    }

    /// The dense scan must find `then` regardless of where it sits among the
    /// keys, and must not be confused by keys that merely share a prefix or
    /// length.
    #[test]
    fn dense_scan_is_exact_about_the_key_name() {
        unsafe {
            for decoys in [
                &["the", "them", "thenx", "hen"][..],
                &["a", "b", "c", "d", "e", "f", "g", "h"][..],
            ] {
                let obj = crate::object::js_object_alloc(0, 8);
                assert!(!obj.is_null());
                for d in decoys {
                    let k = crate::string::js_string_from_bytes(d.as_ptr(), d.len() as u32);
                    crate::object::js_object_set_field_by_name(obj, k, 1.0);
                }
                match own_then_scan(obj as *const ObjectHeader) {
                    OwnScan::NoThen => {}
                    OwnScan::HasThen => panic!("decoy keys {decoys:?} matched `then`"),
                    OwnScan::Unknown => { /* refusing to scan is always safe */ }
                }
                let k = crate::string::js_string_from_bytes(b"then".as_ptr(), 4);
                crate::object::js_object_set_field_by_name(obj, k, 1.0);
                match own_then_scan(obj as *const ObjectHeader) {
                    OwnScan::HasThen => {}
                    _ => panic!("`then` added after {decoys:?} must still be found"),
                }
            }
        }
    }
}
