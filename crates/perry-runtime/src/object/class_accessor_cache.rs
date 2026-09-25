//! Class-accessor resolution cache: `(receiver class id, ShapeId, key) ->` the
//! class-vtable getter / setter the generic path resolved that key to (#10498).
//!
//! # The hole this fills
//!
//! A `get x()` / `set x(v)` declared in a class body lives in the class vtable
//! (`ClassVTable::getters` / `setters`), never as an own key of the instance.
//! Every property inline cache in Perry serves OWN data properties only, so
//! every `obj.x` and `obj.x = v` naming a class accessor missed every cache and
//! re-ran the whole generic path from scratch. Callgrind on the issue's
//! fixture (`get points() { return this._points }` and its setter; release
//! runtime, v0.5.1654, instructions per loop iteration):
//!
//! | loop | own field | class accessor |
//! |---|---:|---:|
//! | two reads (`r.points + r.ms`) | 68 | 7,916 |
//! | two writes + one field read | 207 | 41,823 |
//!
//! A read walked the IC-miss ladder, the generic `[[Get]]` arms, and a
//! SipHash `HashMap<String, usize>` probe per vtable level. A write was worse:
//! `js_put_value_set` ran the spec `OrdinarySet` walk over the prototype
//! OBJECTS (re-deriving each one through `constructor.prototype`), found no
//! descriptor, fell to `CreateDataProperty` on the receiver, and only then
//! reached `js_object_set_field_by_name`'s vtable walk, which finally called
//! the setter.
//!
//! # What an entry claims
//!
//! *A receiver whose (class id, ShapeId) are these, that has no recorded
//! `[[Prototype]]`, no own descriptor, and none of the per-object facts in
//! [`receiver_meta_ok`], resolves key `k` to the vtable getter `get` on a read
//! and to the vtable setter `set` on a write, with `this` = the receiver.*
//!
//! # How an entry is made: recorded, not re-derived
//!
//! The generic resolution has too many arms to re-derive its answer beside it
//! with any confidence, so this module never does. An entry is written at the
//! one point where the generic path has ALREADY committed to calling a vtable
//! accessor for this receiver and key, immediately before it calls it:
//!
//! * **getter** — the two vtable-getter arms of `get_field_by_name_object_tail`
//!   ([`note_class_getter`]). That function is reached only from the full
//!   `[[Get]]` (`js_object_get_field_by_name` and the IC miss handler, both
//!   through `get_field_by_name_past_inherited_cache`) and from a class-OBJECT
//!   arm this module refuses by object kind, so a note there IS the full
//!   `[[Get]]`'s answer. A getter found while an inherited walk has armed the
//!   receiver override is not recorded: its `this` is not the object walked.
//! * **setter** — `js_object_set_field_by_name`'s vtable-setter arm
//!   ([`note_class_setter`]), but ONLY when that call is the
//!   `CreateDataProperty(Receiver)` tail of an ordinary `[[Set]]` walk whose
//!   target was the receiver itself. `js_object_set_field_by_name` has many
//!   other callers whose semantics are not `[[Set]]`'s (it does not look at
//!   prototype descriptors before its vtable walk), so the walk arms a probe
//!   ([`arm_setter_probe`]) around exactly that one `target_set` call. The
//!   vtable arm only captures what it saw into the probe, and only when the
//!   probe names the same receiver and key; the arming frame commits the
//!   capture after `target_set` returns to it ([`finish_setter_probe`]). A
//!   throw out of `target_set` therefore commits nothing, and a probe it
//!   leaves behind is never committed by anyone.
//!
//! Everything the generic path consulted on the way there is a function of the
//! receiver's identity (class id + ShapeId: its own keys, its object kind, its
//! integrity level, its descriptors — every one of which transitions the
//! ShapeId), of per-object facts re-tested on every hit, or of global state
//! covered by the two words below.
//!
//! # What makes a hit valid
//!
//! Re-proved on every hit, cheapest-and-most-selective first:
//!
//! 1. key pointer, class id and ShapeId equal the entry's. Keys are marked
//!    roots of this table (see the GC contract), so an equal pointer is the
//!    same string.
//! 2. `proto_validity()` is unchanged: descriptor installs and clears anywhere
//!    (including `Object.defineProperty(C.prototype, …)`), `delete`,
//!    `setPrototypeOf` recording, class-prototype-object registration and
//!    materialization, and structural mutation of any object marked as a
//!    prototype (`object::proto_validity`).
//! 3. `vtable_generation()` is unchanged: getter / setter / method
//!    registration and parent linking.
//! 4. the receiver is a live, unforwarded `GC_TYPE_OBJECT` whose header and
//!    meta record carry none of the blocking facts.
//!
//! # GC contract
//!
//! The key is the only heap reference an entry holds, and it is MARKED and
//! rewritten by [`scan_class_accessor_cache_roots_mut`], exactly as
//! `inherited_read_cache` does for its keys: a recycled key address can never
//! turn an entry into a false hit, and a moved key is followed. Retention is
//! bounded by the table (256 keys). The accessors are code addresses. The
//! setter probe holds addresses it only COMPARES, never dereferences.

use super::{shapes, ObjectHeader};

/// Direct-mapped, per thread. 256 entries x 48 bytes, boxed for the same
/// reason `inherited_read_cache`'s table is (an oversized inline TLS block
/// overflows the ILP32 TLS layout on arm64_32).
const CACHE_SIZE: usize = 256;
const CACHE_MASK: usize = CACHE_SIZE - 1;

#[derive(Clone, Copy)]
struct Entry {
    /// The key string's address; 0 marks the slot empty. A marked GC root.
    key_ptr: usize,
    /// `proto_validity::proto_validity()` at record time.
    validity: u64,
    /// `class_registry::vtable_generation()` at record time.
    vtable_gen: u64,
    class_id: u32,
    shape: u32,
    /// Vtable getter (`extern "C" fn(this) -> f64`), 0 when not recorded.
    getter: usize,
    /// Vtable setter (`extern "C" fn(this, value) -> f64`), 0 when not recorded.
    setter: usize,
}

const EMPTY_ENTRY: Entry = Entry {
    key_ptr: 0,
    validity: 0,
    vtable_gen: 0,
    class_id: 0,
    shape: 0,
    getter: 0,
    setter: 0,
};

crate::perry_thread_local! {
    static CLASS_ACCESSOR_CACHE: std::cell::UnsafeCell<Box<[Entry]>> =
        std::cell::UnsafeCell::new(vec![EMPTY_ENTRY; CACHE_SIZE].into_boxed_slice());
}

#[inline(always)]
fn entry_index(class_id: u32, shape: u32, key_ptr: usize) -> usize {
    // Same mix as `inherited_read_cache::entry_index`: key pointers are
    // aligned, so fold the middle bits down before masking.
    let h = ((key_ptr >> 4) as u64 ^ ((shape as u64) << 21) ^ ((class_id as u64) << 43))
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 40) as usize & CACHE_MASK
}

/// `PERRY_CLASS_ACCESSOR_IC=0` turns the cache off in a binary that has it, so
/// the same build can be measured with and without it one environment variable
/// apart (the `PERRY_INHERITED_IC` discipline). Both settings answer
/// identically.
#[inline]
fn cache_enabled() -> bool {
    static CLASS_ACCESSOR_IC_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&CLASS_ACCESSOR_IC_ON, || {
        crate::gc::env_default_on_from_value(
            std::env::var("PERRY_CLASS_ACCESSOR_IC").ok().as_deref(),
        )
    })
}

// --- per-object facts -------------------------------------------------------

/// Header flags that refuse a receiver on both halves. An own descriptor makes
/// the resolution depend on WHICH closure is installed, which the ShapeId does
/// not pin (accessor installs mint shapes by descriptor kind, not identity).
const BLOCK_ALWAYS: u16 = crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
    | crate::gc::OBJ_FLAG_NULL_PROTO
    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
/// A non-extensible receiver never reaches the vtable setter: the `[[Set]]`
/// walk's `CreateDataProperty` tail rejects it first. Its ShapeId already
/// differs; refusing it here as well keeps the hit's claim local.
const BLOCK_SET: u16 = BLOCK_ALWAYS
    | crate::gc::OBJ_FLAG_FROZEN
    | crate::gc::OBJ_FLAG_SEALED
    | crate::gc::OBJ_FLAG_NO_EXTEND;

/// Meta-record flags that make a receiver's chain its own rather than its
/// class's, or answer its reads from something other than its shape.
const BLOCK_META_FLAGS: u64 = crate::object::OBJECT_META_FLAG_PROTO_DIVERGED
    | crate::object::OBJECT_META_FLAG_USER_PROTO_OVERRIDE
    | crate::object::OBJECT_META_FLAG_CLASS_EVALUATION_PROTO
    | crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER;

#[inline(always)]
fn receiver_header_ok(header: &crate::gc::GcHeader, set: bool) -> bool {
    let block = if set { BLOCK_SET } else { BLOCK_ALWAYS };
    header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && header._reserved & block == 0
}

/// No recorded `[[Prototype]]` (so the chain is the class's), no per-instance
/// chain divergence, not `process.env` / `arguments`, no Array-subclass
/// `elements` store, not in dictionary mode.
///
/// # Safety
/// `meta` is null or the meta record of a live object.
#[inline(always)]
unsafe fn receiver_meta_ok(meta: *const crate::object::ObjectMeta) -> bool {
    meta.is_null()
        || ((*meta).prototype == 0
            && (*meta).elements == 0
            && (*meta).dictionary_keys == 0
            && (*meta).flags & BLOCK_META_FLAGS == 0)
}

/// Keys whose generic resolution consults per-object state beyond the
/// receiver's identity (Array/Map-subclass backings, private names, index
/// keys, synthesized members), or that a special arm answers by name. Never
/// recorded, so never served.
fn key_bytes_cacheable(bytes: &[u8]) -> bool {
    if matches!(bytes.first(), Some(b'#' | b'-' | b'0'..=b'9')) {
        return false;
    }
    !matches!(
        bytes,
        b"constructor"
            | b"__proto__"
            | b"prototype"
            | b"length"
            | b"size"
            | b"name"
            | b"then"
            | b"toJSON"
    )
}

// --- the hits ---------------------------------------------------------------

/// The recorded accessor for `(obj, key)`, after every per-hit check.
///
/// # Safety
/// `obj` is a masked pointer the caller established is POINTER-tagged; it is
/// only dereferenced once it is proved a plausible heap address above the
/// handle band. `key` may be null.
#[inline]
unsafe fn lookup(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    set: bool,
) -> Option<usize> {
    if key.is_null() || !cache_enabled() {
        return None;
    }
    let addr = obj as usize;
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !crate::value::addr_class::is_plausible_heap_addr(addr)
    {
        return None;
    }
    // The identity word pair: class id at +0, ShapeId at +4 — read before the
    // kind is proved, which `inherited_read_cache_lookup` documents as safe
    // for every pointer-tagged value that can reach here (a non-object's +4
    // word cannot be a live ShapeId, and `object_shape_stamp` range-checks).
    let class_id = (*obj).class_id;
    let shape = shapes::object_shape_stamp(obj);
    if shape == 0 || class_id == 0 {
        return None;
    }
    let index = entry_index(class_id, shape, key as usize);
    let (f, validity, vtable_gen) = CLASS_ACCESSOR_CACHE.with(|cell| {
        let entry = &(*cell.get())[index];
        if entry.key_ptr != key as usize || entry.shape != shape || entry.class_id != class_id {
            return (0, 0, 0);
        }
        let f = if set { entry.setter } else { entry.getter };
        (f, entry.validity, entry.vtable_gen)
    });
    if f == 0
        || validity != crate::object::proto_validity::proto_validity()
        || vtable_gen != super::class_registry::vtable_generation()
    {
        return None;
    }
    // Three identities matched. A live ShapeId at +4 is only ever carried by
    // an object cell (#10828's rule 3), so this reads a GC header; its kind
    // and flags are still checked — the argument, and the check, the packed
    // store's runtime ways make (`packed_hit_receiver_ok`) — before the
    // per-object facts the ShapeId does not pin.
    let header = crate::value::addr_class::try_read_gc_header_known_plausible(addr)?;
    if !receiver_header_ok(header, set) || !receiver_meta_ok((*obj).meta) {
        return None;
    }
    Some(f)
}

/// Serve `obj.key` by calling the recorded class getter, exactly as
/// `get_field_by_name_object_tail`'s vtable arm would, or decline.
///
/// # Safety
/// As [`lookup`]. Runs user code: the caller must treat this as a collection
/// point.
#[inline]
pub(crate) unsafe fn class_getter_hit(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let getter = lookup(obj, key, false)?;
    // An armed inherited-walk receiver would become the getter's `this`; the
    // entry was never recorded under one, so leave that case to the walk.
    if super::field_get_set::accessor_receiver_override_armed() {
        return None;
    }
    note_stat(Stat::Hit);
    // `call_class_getter`, minus the override `take` just proved empty.
    let this = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
    let _boundary = super::prototype_chain::UserCodeResolutionBoundary::enter();
    let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter);
    Some(f(this))
}

/// Serve `obj.key = value` (a `[[Set]]` whose target is its receiver) by
/// calling the recorded class setter, exactly as the walk's
/// `CreateDataProperty` tail reaching `js_object_set_field_by_name`'s vtable
/// arm would, or decline. Returns PutValue's result: the assigned value.
///
/// # Safety
/// As [`lookup`]. Runs user code: the caller must treat this as a collection
/// point.
#[inline]
pub(crate) unsafe fn class_setter_hit(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<f64> {
    let setter = lookup(obj, key, true)?;
    note_stat(Stat::Hit);
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
    let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(setter);
    if !nanbox_may_name_heap(value.to_bits()) {
        let _ = f(receiver, value);
        return Some(value);
    }
    // PutValue answers the assigned value; the setter can collect and move it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    let _ = f(receiver, value);
    Some(value_h.get_nanbox_f64())
}

/// Whether `bits` can name a GC allocation, i.e. whether a value held across a
/// collection point needs a root. Numbers, int32s, inline strings and the
/// undefined / null / boolean / hole markers never do; anything else is rooted.
#[inline(always)]
fn nanbox_may_name_heap(bits: u64) -> bool {
    match bits >> 48 {
        // A raw, unboxed pointer (module-level object slots).
        0 => bits != 0,
        tag @ 0x7FF8..=0x7FFF => !matches!(tag, 0x7FF8 | 0x7FF9 | 0x7FFC | 0x7FFE),
        _ => false,
    }
}

/// [`class_setter_hit`] for a NaN-boxed target and a raw key pointer, as the
/// PutValue miss entries receive them. Anything but a POINTER-tagged target
/// declines before a dereference.
///
/// # Safety
/// `key` is null or a live string.
#[inline]
pub(crate) unsafe fn class_setter_hit_value(
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> Option<f64> {
    let bits = target.to_bits();
    if (bits & !crate::value::POINTER_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    class_setter_hit(
        (bits & crate::value::POINTER_MASK) as *const ObjectHeader,
        key,
        value,
    )
}

// --- recording --------------------------------------------------------------

/// What a recording claims, captured while the resolving arm still holds the
/// receiver: its identity and the two validity words at that moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Recording {
    class_id: u32,
    shape: u32,
    validity: u64,
    vtable_gen: u64,
    accessor: usize,
}

/// The receiver's identity if `obj` and `key` are ones a hit may serve.
/// Never allocates and never runs user code.
///
/// # Safety
/// `obj` and `key` are the live pointers the calling resolution arm is about
/// to use.
unsafe fn recordable_identity(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    set: bool,
) -> Option<(u32, u32)> {
    if key.is_null() || !cache_enabled() {
        return None;
    }
    let addr = obj as usize;
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !crate::value::addr_class::is_plausible_heap_addr(addr)
        || crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Unknown
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header_known_plausible(addr)?;
    if !receiver_header_ok(header, set) || !receiver_meta_ok((*obj).meta) {
        return None;
    }
    let class_id = (*obj).class_id;
    if class_id == 0 || class_id == super::NATIVE_MODULE_CLASS_ID {
        return None;
    }
    // Object kind is a pure function of the ShapeId, so it is proved here once
    // and pinned by the identity compare on every hit. This refuses class
    // objects (`ShapeObjectKind::Class`) along with every other exotic kind.
    match shapes::object_shape_descriptor(obj) {
        Some(descriptor) if descriptor.object_kind == shapes::ShapeObjectKind::Ordinary => {}
        _ => return None,
    }
    let shape = shapes::object_shape_stamp(obj);
    if shape == 0 || !key_is_recordable(key) {
        return None;
    }
    Some((class_id, shape))
}

/// A live heap string whose bytes [`key_bytes_cacheable`] admits.
///
/// # Safety
/// `key` is non-null and names a live allocation.
unsafe fn key_is_recordable(key: *const crate::StringHeader) -> bool {
    let key_addr = key as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(key_addr)
        || crate::arena::classify_heap_generation(key_addr) == crate::arena::HeapGeneration::Unknown
    {
        return false;
    }
    let Some(key_header) = crate::value::addr_class::try_read_gc_header_known_plausible(key_addr)
    else {
        return false;
    };
    if key_header.obj_type != crate::gc::GC_TYPE_STRING
        || key_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*key).byte_len > (*key).capacity
        || (*key).byte_len >= 1 << 28
    {
        return false;
    }
    key_bytes_cacheable(std::slice::from_raw_parts(
        crate::string::string_data(key),
        (*key).byte_len as usize,
    ))
}

/// Capture a recording of `(obj, key) -> accessor`, or `None` when this
/// receiver and key are not ones a hit may serve.
///
/// # Safety
/// As [`recordable_identity`].
unsafe fn capture(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    accessor: usize,
    set: bool,
) -> Option<Recording> {
    if accessor == 0 {
        return None;
    }
    let (class_id, shape) = recordable_identity(obj, key, set)?;
    Some(Recording {
        class_id,
        shape,
        validity: crate::object::proto_validity::proto_validity(),
        vtable_gen: super::class_registry::vtable_generation(),
        accessor,
    })
}

/// Write `recording` under `key`. A recording whose validity words have moved
/// on since it was captured describes a world that no longer exists and is
/// dropped rather than written.
fn commit(recording: Recording, key_addr: usize, set: bool) {
    if recording.validity != crate::object::proto_validity::proto_validity()
        || recording.vtable_gen != super::class_registry::vtable_generation()
    {
        return;
    }
    let Recording {
        class_id,
        shape,
        validity,
        vtable_gen,
        accessor,
    } = recording;
    CLASS_ACCESSOR_CACHE.with(|cell| unsafe {
        let entry = &mut (*cell.get())[entry_index(class_id, shape, key_addr)];
        // Keep the other half only when it was recorded against this exact
        // identity under the same two words; otherwise it is stale or foreign.
        if entry.key_ptr != key_addr
            || entry.class_id != class_id
            || entry.shape != shape
            || entry.validity != validity
            || entry.vtable_gen != vtable_gen
        {
            *entry = Entry {
                key_ptr: key_addr,
                validity,
                vtable_gen,
                class_id,
                shape,
                ..EMPTY_ENTRY
            };
        }
        if set {
            entry.setter = accessor;
        } else {
            entry.getter = accessor;
        }
    });
    note_stat(Stat::Record);
}

/// `get_field_by_name_object_tail` is about to call vtable getter `getter` for
/// `obj.key`. See the module docs for why this site is the full `[[Get]]`.
/// Recorded immediately: `obj` and `key` are live here and the getter has not
/// run yet.
///
/// # Safety
/// `obj` / `key` are the tail's live pointers.
#[inline]
pub(crate) unsafe fn note_class_getter(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    getter: usize,
) {
    if super::field_get_set::accessor_receiver_override_armed() {
        return;
    }
    if let Some(recording) = capture(obj, key, getter, false) {
        commit(recording, key as usize, false);
    }
}

/// The setter half's probe: which `CreateDataProperty(receiver, key)` tail a
/// `[[Set]]` walk is inside, and what that tail's vtable arm saw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SetterProbe {
    /// Receiver and key addresses the probe matches; `(0, 0)` once matched or
    /// when nothing is armed. Compared, never dereferenced.
    armed: (usize, usize),
    /// Filled by [`note_class_setter`]; committed only by the frame that armed
    /// the probe, and only when `target_set` returned to it normally.
    seen: Option<Recording>,
}

const IDLE_PROBE: SetterProbe = SetterProbe {
    armed: (0, 0),
    seen: None,
};

crate::perry_thread_local! {
    static SETTER_PROBE: std::cell::Cell<SetterProbe> = const { std::cell::Cell::new(IDLE_PROBE) };
}

/// Arm the setter probe for the `CreateDataProperty(receiver, key)` tail of a
/// `[[Set]]` walk whose target is `receiver`. Returns the displaced probe for
/// [`finish_setter_probe`].
#[inline]
pub(crate) fn arm_setter_probe(receiver: f64, key: f64) -> SetterProbe {
    let (rb, kb) = (receiver.to_bits(), key.to_bits());
    let armed = if (rb & !crate::value::POINTER_MASK) == crate::value::POINTER_TAG
        && (kb & !crate::value::POINTER_MASK) == crate::value::STRING_TAG
    {
        (
            (rb & crate::value::POINTER_MASK) as usize,
            (kb & crate::value::POINTER_MASK) as usize,
        )
    } else {
        (0, 0)
    };
    SETTER_PROBE.with(|probe| probe.replace(SetterProbe { armed, seen: None }))
}

/// `target_set` returned: put back the probe [`arm_setter_probe`] displaced,
/// and commit what this frame's probe saw under the key's CURRENT address
/// (`key_now`, re-read from a root — the setter ran in between and may have
/// moved it).
///
/// A throw out of `target_set` never reaches here, so its probe is never
/// committed: whatever it holds is replaced by the next arm and restored as
/// that arm's `prev`, and nothing but an arming frame ever commits.
#[inline]
pub(crate) fn finish_setter_probe(prev: SetterProbe, key_now: f64) {
    let mine = SETTER_PROBE.with(|probe| probe.replace(prev));
    let Some(recording) = mine.seen else {
        return;
    };
    let bits = key_now.to_bits();
    if (bits & !crate::value::POINTER_MASK) != crate::value::STRING_TAG {
        return;
    }
    let key = (bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
    if unsafe { key_is_recordable(key) } {
        commit(recording, key as usize, true);
    }
}

/// `js_object_set_field_by_name`'s vtable arm is about to call `setter` for
/// `obj.key = …`. Captures a recording only when the armed probe names this
/// exact receiver and key, and disarms the probe first so the setter body
/// cannot match it again.
///
/// # Safety
/// `obj` / `key` are the arm's live pointers.
#[inline]
pub(crate) unsafe fn note_class_setter(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    setter: usize,
) {
    let target = (
        (obj as u64 & crate::value::POINTER_MASK) as usize,
        key as usize,
    );
    let probe = SETTER_PROBE.with(|probe| probe.get());
    if probe.armed.0 == 0 || probe.armed != target {
        return;
    }
    let seen = capture(target.0 as *const ObjectHeader, key, setter, true);
    SETTER_PROBE.with(|probe| {
        probe.set(SetterProbe {
            armed: (0, 0),
            seen,
        })
    });
}

// --- GC ---------------------------------------------------------------------

/// Root scan: every recorded key is MARKED and rewritten, so an entry's key
/// pointer always names the string it was recorded for.
pub(crate) fn scan_class_accessor_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLASS_ACCESSOR_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            if entry.key_ptr != 0 {
                visitor.visit_tagged_usize_slot(&mut entry.key_ptr, crate::value::STRING_TAG);
            }
        }
    });
}

/// Drop entries whose key the collector reports dead. Marking above means
/// there should never be one; registered in `DEAD_KEY_PRUNES` anyway, for the
/// reason `inherited_read_cache` gives: it is the only thing between a
/// recycled address and a false hit should a collection ever sweep without
/// running the scanner.
pub(crate) fn prune_dead_class_accessor_cache_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    CLASS_ACCESSOR_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            if entry.key_ptr != 0 && is_dead_owner(entry.key_ptr) {
                *entry = EMPTY_ENTRY;
            }
        }
    });
}

// --- test counters ----------------------------------------------------------
//
// A cache that silently declines is correct-but-slow and invisible in a
// program's output, so the tests assert hits and records, not just values.

#[derive(Clone, Copy)]
enum Stat {
    Hit,
    Record,
}

#[cfg(test)]
thread_local! {
    static STATS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

#[inline(always)]
fn note_stat(_stat: Stat) {
    #[cfg(test)]
    STATS.with(|s| {
        let (hits, records) = s.get();
        s.set(match _stat {
            Stat::Hit => (hits + 1, records),
            Stat::Record => (hits, records + 1),
        });
    });
}

/// `(hits, records)` on this thread since the last [`test_reset`].
#[cfg(test)]
pub(crate) fn test_stats() -> (u64, u64) {
    STATS.with(|s| s.get())
}

/// The recorded half for `(obj, key)`, with every per-hit check, without
/// calling it.
#[cfg(test)]
pub(crate) unsafe fn test_lookup(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    set: bool,
) -> Option<usize> {
    lookup(obj, key, set)
}

/// Every recorded key address, for the GC tests.
#[cfg(test)]
pub(crate) fn test_recorded_keys() -> Vec<usize> {
    CLASS_ACCESSOR_CACHE.with(|cell| unsafe {
        (*cell.get())
            .iter()
            .filter(|entry| entry.key_ptr != 0)
            .map(|entry| entry.key_ptr)
            .collect()
    })
}

#[cfg(test)]
pub(crate) fn test_reset() {
    STATS.with(|s| s.set((0, 0)));
    SETTER_PROBE.with(|probe| probe.set(IDLE_PROBE));
    CLASS_ACCESSOR_CACHE.with(|cell| unsafe {
        for entry in (*cell.get()).iter_mut() {
            *entry = EMPTY_ENTRY;
        }
    });
}

#[cfg(test)]
#[path = "class_accessor_cache_tests.rs"]
mod tests;
