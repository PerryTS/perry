//! `class X extends Array` support — predicates + a dense-snapshot materializer.
//!
//! An Array subclass instance is a plain `ObjectHeader` (perry has no exotic
//! array-object representation), so its inherited `Array.prototype` methods run
//! through the spec-generic array-like engine in [`super::generic`], and
//! iteration / spread materialize a dense snapshot of its indexed elements.
//! Kept out of `generic.rs` so that module stays under the file-size gate.

use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use super::generic::{al_get, al_length, nanbox_arr};
use crate::array::{js_array_alloc_with_length, note_array_slot, ArrayHeader};
use crate::object::ObjectHeader;
use crate::value::JSValue;

#[path = "subclass_loop_guard.rs"]
pub(super) mod loop_guard;
// The loop-guard entry points are exported C symbols; only the unit tests
// reach them through Rust paths.
#[cfg(test)]
pub(super) use loop_guard::{js_packed_arraylike_loop_guard, js_packed_ecs_u32_loop_guard};

// #8690: `ObjectMeta::flags` carries the move-stable scalar payload for a
// numeric packed-prefix proof. The GcHeader authority bit prevents a record
// surviving address reuse: fresh allocations have it clear, and both words
// ride an evacuation without a side-table re-key walk.
//
//     bit 0       existing custom-[[Prototype]] flag
//     bit 1       payload valid
//     bit 2       compact nonnegative-int entity proof (mode 2)
//     bits 8..31  verified prefix bound (24 bits, max 16,000,000)
//     bits 32..63 exact semantic ShapeId
const PACKED_NUMERIC_META_VALID: u64 = 1 << 1;
const PACKED_NUMERIC_META_U32: u64 = 1 << 2;
const PACKED_NUMERIC_META_BOUND_SHIFT: u32 = 8;
const PACKED_NUMERIC_META_BOUND_MASK: u64 = 0x00FF_FFFF << PACKED_NUMERIC_META_BOUND_SHIFT;
const PACKED_NUMERIC_META_SHAPE_MASK: u64 = 0xFFFF_FFFF_0000_0000;
const PACKED_NUMERIC_META_MASK: u64 = PACKED_NUMERIC_META_VALID
    | PACKED_NUMERIC_META_U32
    | PACKED_NUMERIC_META_BOUND_MASK
    | PACKED_NUMERIC_META_SHAPE_MASK;

// #8655: Array-subclass instances use ordinary ObjectHeader property slots,
// but their hot numeric reads have a much stronger invariant than a generic
// object lookup can exploit: `push` appends the own keys `"0"`, `"1"`, ... in
// order, and every structural/descriptor/prototype mutation publishes a new
// ShapeId before it becomes observable. Cache that dense prefix per exact
// (class, shape) pair so a stable `sub[i]` is two field-slot reads (`length`
// and the element) instead of number -> String allocation + hash lookup.
//
// The cache stores no heap pointer, so it is not a GC root. ShapeIds are never
// reused, and the class id prevents an unrelated class with the same ordered
// keys from borrowing the Array-subclass proof.
// Lifecycle-heavy Array subclasses revisit one shape per historical length.
// Keep the common 1k-entity lattice resident so an allocation-free tail
// transition does not fall back to an O(length) ordered-key rescan next cycle.
const DENSE_SUBCLASS_CACHE_SLOTS: usize = 16384;
const ARRAY_SUBCLASS_NAMED_PREFIX_TOKEN_BIT: u64 = 1 << 63;

struct DenseSubclassCacheEntry {
    /// Even while stable, odd while a colliding writer publishes a payload.
    sequence: AtomicU64,
    /// `(class_id << 32) | shape_id`.
    key: AtomicU64,
    /// `(length_slot << 32) | element_base`.
    slots: AtomicU64,
    /// `(live_inline_slots << 32) | dense_prefix_len`.
    bounds: AtomicU64,
}

impl DenseSubclassCacheEntry {
    const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            key: AtomicU64::new(0),
            slots: AtomicU64::new(0),
            bounds: AtomicU64::new(0),
        }
    }
}

static DENSE_SUBCLASS_CACHE: [DenseSubclassCacheEntry; DENSE_SUBCLASS_CACHE_SLOTS] =
    [const { DenseSubclassCacheEntry::new() }; DENSE_SUBCLASS_CACHE_SLOTS];

#[derive(Clone, Copy)]
struct DenseSubclassLayout {
    length_slot: u32,
    element_base: u32,
    dense_prefix_len: u32,
    live_inline_slots: u32,
}

/// A live, non-forwarded ordinary object whose `GcHeader` has already been
/// validated. Keeping the integrity flags beside the pointer lets a hot
/// Array-subclass mutation reuse that single header read for brand, layout,
/// and frozen/sealed/no-extend checks.
#[derive(Clone, Copy)]
pub(super) struct ValidatedObjectReceiver {
    pub(super) object: *const ObjectHeader,
    pub(super) object_flags: u16,
}

/// Read the per-instance prototype-divergence bit after the caller has already
/// proved a live, non-forwarded `GC_TYPE_OBJECT` receiver.
///
/// The public prototype-chain predicate accepts arbitrary addresses and must
/// re-run buffer/heap/header classification before touching `ObjectHeader`.
/// Dense Array-subclass paths have just completed that proof, so repeating it
/// ahead of every receiver-local layout-cache hit is both redundant and hot.
#[inline(always)]
unsafe fn validated_object_has_prototype_override(obj: *const ObjectHeader) -> bool {
    let meta = (*obj).meta;
    !meta.is_null() && (*meta).flags & crate::object::OBJECT_META_FLAG_PROTO_OVERRIDE != 0
}

#[inline(always)]
fn dense_cache_key(class_id: u32, shape_id: u32) -> u64 {
    ((class_id as u64) << 32) | shape_id as u64
}

#[inline(always)]
fn dense_cache_entry(key: u64) -> &'static DenseSubclassCacheEntry {
    let mixed = key ^ (key >> 33) ^ (key >> 17);
    &DENSE_SUBCLASS_CACHE[mixed as usize & (DENSE_SUBCLASS_CACHE_SLOTS - 1)]
}

#[inline]
fn cached_dense_layout(key: u64) -> Option<DenseSubclassLayout> {
    let entry = dense_cache_entry(key);
    let sequence = entry.sequence.load(Ordering::Acquire);
    if sequence & 1 != 0 || entry.key.load(Ordering::Relaxed) != key {
        return None;
    }
    let slots = entry.slots.load(Ordering::Relaxed);
    let bounds = entry.bounds.load(Ordering::Relaxed);
    // Recheck the seqlock before interpreting either word so readers never
    // combine payloads from two colliding publishers. The fence stops the
    // relaxed payload loads above from being reordered past this recheck on
    // weakly ordered targets - an Acquire load alone only orders what comes
    // after it, not what precedes it in program order.
    std::sync::atomic::fence(Ordering::Acquire);
    if entry.sequence.load(Ordering::Acquire) != sequence {
        return None;
    }
    Some(DenseSubclassLayout {
        length_slot: (slots >> 32) as u32,
        element_base: slots as u32,
        dense_prefix_len: bounds as u32,
        live_inline_slots: (bounds >> 32) as u32,
    })
}

#[inline]
fn publish_dense_layout(key: u64, layout: DenseSubclassLayout) {
    let entry = dense_cache_entry(key);
    let mut sequence = entry.sequence.load(Ordering::Relaxed);
    loop {
        if sequence & 1 != 0 {
            std::hint::spin_loop();
            sequence = entry.sequence.load(Ordering::Relaxed);
            continue;
        }
        match entry.sequence.compare_exchange_weak(
            sequence,
            sequence.wrapping_add(1),
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => sequence = observed,
        }
    }
    entry.slots.store(
        ((layout.length_slot as u64) << 32) | layout.element_base as u64,
        Ordering::Relaxed,
    );
    entry.bounds.store(
        ((layout.live_inline_slots as u64) << 32) | layout.dense_prefix_len as u64,
        Ordering::Relaxed,
    );
    entry.key.store(key, Ordering::Relaxed);
    entry
        .sequence
        .store(sequence.wrapping_add(2), Ordering::Release);
}

/// Receiver-local front cache for the current Array-subclass layout. Unlike
/// the process-wide collision cache above, these scalar words move with the
/// owner and need no atomics: Perry heap objects are agent-local, and workers
/// deep-copy rather than concurrently share ObjectHeaders.
#[inline(always)]
unsafe fn owner_cached_dense_layout(obj: *const ObjectHeader) -> Option<DenseSubclassLayout> {
    let meta = (*obj).meta;
    if meta.is_null() {
        return None;
    }
    let key = dense_cache_key((*obj).class_id, (*obj).parent_class_id);
    if key == 0 || (*meta).array_subclass_dense_key != key {
        return None;
    }
    let slots = (*meta).array_subclass_dense_slots;
    let bounds = (*meta).array_subclass_dense_bounds;
    Some(DenseSubclassLayout {
        length_slot: (slots >> 32) as u32,
        element_base: slots as u32,
        dense_prefix_len: bounds as u32,
        live_inline_slots: (bounds >> 32) as u32,
    })
}

#[inline(always)]
unsafe fn publish_owner_dense_layout(obj: *const ObjectHeader, layout: DenseSubclassLayout) {
    let meta = (*obj).meta;
    if meta.is_null() {
        return;
    }
    // Publish the key last. This is single-agent state, but retaining the
    // payload-before-authority ordering also makes an accidental diagnostic
    // read fail closed rather than combine a new key with old bounds.
    (*meta).array_subclass_dense_slots =
        ((layout.length_slot as u64) << 32) | layout.element_base as u64;
    (*meta).array_subclass_dense_bounds =
        ((layout.live_inline_slots as u64) << 32) | layout.dense_prefix_len as u64;
    (*meta).array_subclass_dense_key = dense_cache_key((*obj).class_id, (*obj).parent_class_id);
}

fn decimal_u32<'a>(mut value: u32, buf: &'a mut [u8; 10]) -> &'a [u8] {
    let mut start = buf.len();
    loop {
        start -= 1;
        buf[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return &buf[start..];
        }
    }
}

/// Establish the dense-prefix invariant once for an exact semantic ShapeId.
/// This path may scan the keys array, but it runs only on a cache miss. It
/// allocates nothing and keeps no address into the moving heap.
unsafe fn build_dense_layout(obj: *const ObjectHeader) -> Option<DenseSubclassLayout> {
    let class_id = (*obj).class_id;
    if class_id == 0
        || !is_array_subclass_class_id(class_id)
        || validated_object_has_prototype_override(obj)
    {
        return None;
    }
    let shape = crate::object::shapes::object_shape_descriptor(obj)?;
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
        return None;
    }
    let keys = shape.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return None;
    }
    let (key_slots, physical_len) = crate::object::keys_array_dense_slots(keys);
    let key_count = (shape.logical_key_count as usize).min(physical_len);
    if key_slots.is_null() || key_count == 0 {
        return None;
    }

    let mut length_slot = None;
    let mut element_base = None;
    for slot in 0..key_count {
        let stored = JSValue::from_bits((*key_slots.add(slot)).to_bits());
        if length_slot.is_none() && crate::string::js_string_key_matches_bytes(stored, b"length") {
            length_slot = Some(slot as u32);
        }
        if element_base.is_none() && crate::string::js_string_key_matches_bytes(stored, b"0") {
            element_base = Some(slot as u32);
        }
    }
    let length_slot = length_slot?;
    // A length-only empty subclass has no `"0"` key yet. Its first numeric
    // slot is nevertheless known: it starts immediately after the complete
    // named prefix. Recording that boundary lets named-field PICs prove the
    // prefix while the tail is empty; `dense_prefix_len == 0` still makes
    // every numeric index side-exit.
    let has_element_zero = element_base.is_some();
    let element_base = element_base.unwrap_or(key_count as u32);
    let mut dense_prefix_len = 0u32;
    if has_element_zero {
        while (element_base as usize + dense_prefix_len as usize) < key_count {
            let slot = element_base as usize + dense_prefix_len as usize;
            let stored = JSValue::from_bits((*key_slots.add(slot)).to_bits());
            let mut decimal = [0u8; 10];
            if !crate::string::js_string_key_matches_bytes(
                stored,
                decimal_u32(dense_prefix_len, &mut decimal),
            ) {
                break;
            }
            dense_prefix_len += 1;
        }
    }

    // Class construction installs descriptors for unrelated methods, so the
    // object-wide descriptor bit is too coarse for this proof. Data
    // descriptors do not alter [[Get]]; reject only accessors for the slots the
    // fast path will read. Descriptor mutations publish a new semantic
    // ShapeId, which makes this one-time scan part of the exact-shape proof.
    if crate::object::object_has_descriptors(obj as usize) {
        if crate::object::get_accessor_descriptor(obj as usize, "length").is_some() {
            return None;
        }
        for index in 0..dense_prefix_len {
            let mut decimal = [0u8; 10];
            let bytes = decimal_u32(index, &mut decimal);
            // `decimal_u32` emits ASCII digits only.
            let key = unsafe { std::str::from_utf8_unchecked(bytes) };
            if crate::object::get_accessor_descriptor(obj as usize, key).is_some() {
                return None;
            }
            if crate::object::get_property_attrs(obj as usize, key)
                .is_some_and(|attrs| !attrs.writable())
            {
                return None;
            }
        }
    }

    Some(DenseSubclassLayout {
        length_slot,
        element_base,
        dense_prefix_len,
        live_inline_slots: shape.live_inline_slot_count,
    })
}

#[inline]
fn validated_object_receiver(raw: usize) -> Option<ValidatedObjectReceiver> {
    let header = unsafe { crate::value::addr_class::try_read_gc_header(raw)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    Some(ValidatedObjectReceiver {
        object: raw as *const ObjectHeader,
        object_flags: header._reserved,
    })
}

#[inline]
fn validated_object_receiver_for_value(value: f64) -> Option<ValidatedObjectReceiver> {
    let js = JSValue::from_bits(value.to_bits());
    js.is_pointer()
        .then(|| validated_object_receiver(js.as_pointer::<ObjectHeader>() as usize))
        .flatten()
}

/// Resolve the cached dense layout after the caller has proved that `obj` is
/// a live, non-forwarded ordinary object. Every rejected Array-subclass brand,
/// descriptor, hole, or prototype case returns `None`.
#[inline]
fn dense_layout_for_validated_object(obj: *const ObjectHeader) -> Option<DenseSubclassLayout> {
    // This is per receiver, not per ShapeId. A cached layout built before
    // Object.setPrototypeOf must not let this object borrow the old proof.
    if unsafe { validated_object_has_prototype_override(obj) } {
        return None;
    }
    if let Some(layout) = unsafe { owner_cached_dense_layout(obj) } {
        return Some(layout);
    }
    let (class_id, shape_id) = unsafe { ((*obj).class_id, (*obj).parent_class_id) };
    let key = dense_cache_key(class_id, shape_id);
    let layout = cached_dense_layout(key).or_else(|| {
        let layout = unsafe { build_dense_layout(obj) }?;
        publish_dense_layout(key, layout);
        Some(layout)
    })?;
    unsafe { publish_owner_dense_layout(obj, layout) };
    Some(layout)
}

/// Resolve a live Array-subclass object and its cached dense layout. Every
/// rejected brand, forwarding, descriptor, hole, or prototype case returns
/// `None`; callers retain their existing fully generic fallback.
#[inline]
fn dense_layout_for_value(value: f64) -> Option<(*const ObjectHeader, DenseSubclassLayout)> {
    let receiver = validated_object_receiver_for_value(value)?;
    let layout = dense_layout_for_validated_object(receiver.object)?;
    Some((receiver.object, layout))
}

/// Return the class-wide identity of a proved Array-subclass named prefix.
///
/// Perry's object-backed Array subclasses append numeric keys to the same
/// ordered keys array that holds their declared fields. Consequently every
/// numeric tail length has a different ShapeId, even though the slots before
/// `"0"` are byte-for-byte the class allocation shape. A property-read PIC
/// can safely keep using one declared-field slot across those tail shapes only
/// after this function proves all of the following:
///
/// - the receiver is a live ordinary Array-subclass instance;
/// - its prefix matches the class's registered allocation keys exactly;
/// - the only additional named keys are the canonical Array-subclass
///   `length` and `fill` slots;
/// - every remaining key is the complete dense numeric suffix already proved
///   by `DenseSubclassLayout`; and
/// - `requested_slot` lies before that numeric suffix.
///
/// The token is stored on the object, not in a pointer-keyed side table, so it
/// moves with the receiver. Generic structural/semantic shape publication
/// clears it via `clear_array_subclass_named_prefix_token`; exact learned
/// numeric-tail transitions deliberately do not.
pub(crate) unsafe fn array_subclass_named_prefix_token_for_slot(
    obj: *const ObjectHeader,
    requested_slot: usize,
) -> u64 {
    if obj.is_null() {
        return 0;
    }
    let header = match crate::value::addr_class::try_read_gc_header(obj as usize) {
        Some(header) => header,
        None => return 0,
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return 0;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return 0;
    }
    let class_id = (*obj).class_id;
    if class_id == 0 || !is_array_subclass_class_id(class_id) {
        return 0;
    }
    let Some((declared_keys, declared_count)) =
        crate::object::registered_class_keys_array(class_id)
    else {
        return 0;
    };
    if declared_keys.is_null() {
        return 0;
    }
    // An elements-backed instance (`super::subclass_elements`) has NO numeric
    // keys and no `length` property in its shape, so `build_dense_layout`
    // (which locates `length` by name) cannot describe it — and without a
    // token the descriptor-bearing IC-miss path below never primes the PIC,
    // leaving every declared-field read to miss forever (measured: 17.8M
    // misses on `change`/`sset`/`mask` in a 2 s wolf-ecs run). Its named
    // prefix is simply the WHOLE shape: the strongest form of the same
    // proof, validated by the identical declared-prefix comparison below.
    let elements_backed = !super::subclass_elements::elements_of(obj).is_null();
    let (element_base, dense_prefix_len, length_slot) = if elements_backed {
        (u32::MAX, 0, u32::MAX)
    } else {
        let shape_id = (*obj).parent_class_id;
        let cache_key = dense_cache_key(class_id, shape_id);
        let layout = cached_dense_layout(cache_key).or_else(|| {
            let layout = build_dense_layout(obj)?;
            publish_dense_layout(cache_key, layout);
            Some(layout)
        });
        let Some(layout) = layout else {
            return 0;
        };
        (
            layout.element_base,
            layout.dense_prefix_len,
            layout.length_slot,
        )
    };
    // Descriptor-bearing Array subclasses cannot use the ordinary exact-shape
    // raw-load PIC even while empty: their unrelated `length` descriptor sends
    // them through the descriptor arm. Admit the fully validated named prefix
    // before the first numeric key exists as well. `element_base` is the first
    // prospective numeric slot and `dense_prefix_len == 0` proves there is no
    // tail yet; the complete-prefix equality below remains the authority. An
    // elements-backed instance has no numeric slot at all (`u32::MAX`), so
    // only the declared-prefix bound below applies to it.
    if requested_slot >= element_base as usize {
        return 0;
    }
    let cached = (*meta).array_subclass_named_prefix_token;
    if cached != 0 {
        return cached;
    }

    let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
        return 0;
    };
    if shape.object_kind != crate::object::shapes::ShapeObjectKind::Ordinary {
        return 0;
    }
    let current_keys = shape.keys as usize as *const ArrayHeader;
    let (current_slots, current_physical_len) = crate::object::keys_array_dense_slots(current_keys);
    let (declared_slots, declared_physical_len) =
        crate::object::keys_array_dense_slots(declared_keys as *const ArrayHeader);
    let current_count = (shape.logical_key_count as usize).min(current_physical_len);
    let declared_count = (declared_count as usize).min(declared_physical_len);
    if current_slots.is_null() || declared_slots.is_null() || declared_count > current_count {
        return 0;
    }
    // Every key is either in the named prefix or in the numeric tail. An
    // elements-backed instance has no tail, so the requested slot must simply
    // be a declared one; the shape-carried form keeps the exact partition.
    if elements_backed {
        if requested_slot >= declared_count {
            return 0;
        }
    } else if element_base as usize + dense_prefix_len as usize != current_count {
        return 0;
    }

    // Declared slots must occupy the identical prefix positions. Stored object
    // keys are heap strings, so string equality is exact even after one keys
    // array was cloned and the two pointer words differ after a moving GC.
    let mut declared_length_slot = None;
    let mut declared_fill = false;
    for slot in 0..declared_count {
        let current_bits = (*current_slots.add(slot)).to_bits();
        let declared_bits = (*declared_slots.add(slot)).to_bits();
        let current_key = (current_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        let declared_key = (declared_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        if current_key.is_null()
            || declared_key.is_null()
            || crate::string::js_string_equals(current_key, declared_key) == 0
        {
            return 0;
        }
        // An unrelated descriptor (notably Array-subclass `length`) must not
        // disable direct reads of class-declared data fields. Prove every key
        // covered by this class-wide token is not an accessor on THIS object;
        // any later descriptor mutation mints a semantic ShapeId and clears
        // the token before it becomes observable.
        let Some(name) = crate::object::has_own_helpers::str_from_string_header(current_key) else {
            return 0;
        };
        if name == "length" {
            declared_length_slot = Some(slot as u32);
        } else if name == "fill" {
            declared_fill = true;
        }
        if crate::object::get_accessor_descriptor(obj as usize, name).is_some() {
            return 0;
        }
    }

    // The legacy shape-carried representation installs `length` and its
    // compatibility `fill` closure after the declared prefix. The default
    // elements-backed representation inherits `fill` from `Array.prototype`
    // and has no runtime names in its shape. Anything else is
    // instance-specific.
    let declared_count = declared_count as u32;
    let mut expected_runtime_names: [&[u8]; 2] = [&[]; 2];
    let mut expected_runtime_count = 0usize;
    if !elements_backed {
        // The shape-carried form carries `length` as an own property, at the
        // declared slot when the class declared that name and appended
        // otherwise.
        let expected_length_slot = declared_length_slot.unwrap_or(declared_count);
        if length_slot != expected_length_slot {
            return 0;
        }
        if declared_length_slot.is_none() {
            expected_runtime_names[expected_runtime_count] = b"length";
            expected_runtime_count += 1;
        }
    } else if declared_length_slot.is_some() {
        // A class that declares its own `length` field is not modelled by the
        // elements store (the store owns `length`); keep it off this token.
        return 0;
    }
    if !elements_backed && !declared_fill {
        expected_runtime_names[expected_runtime_count] = b"fill";
        expected_runtime_count += 1;
    }
    if !elements_backed
        && element_base != declared_count.saturating_add(expected_runtime_count as u32)
    {
        return 0;
    }
    for (offset, expected) in expected_runtime_names[..expected_runtime_count]
        .iter()
        .enumerate()
    {
        let slot = declared_count as usize + offset;
        let bits = (*current_slots.add(slot)).to_bits();
        let key = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        if key.is_null()
            || !crate::string::js_string_key_matches_bytes(JSValue::from_bits(bits), expected)
        {
            return 0;
        }
        let Some(name) = crate::object::has_own_helpers::str_from_string_header(key) else {
            return 0;
        };
        if crate::object::get_accessor_descriptor(obj as usize, name).is_some() {
            return 0;
        }
    }

    let token = ARRAY_SUBCLASS_NAMED_PREFIX_TOKEN_BIT | u64::from(class_id);
    (*meta).array_subclass_named_prefix_token = token;
    token
}

/// Retire the named-prefix proof before any generic shape or semantic
/// publication. Value-only field stores leave slot identity unchanged and do
/// not call this; exact numeric-tail shape installs bypass it deliberately.
#[inline]
pub(crate) unsafe fn clear_array_subclass_named_prefix_token(obj: *mut ObjectHeader) {
    if obj.is_null() {
        return;
    }
    let meta = (*obj).meta;
    if !meta.is_null() {
        (*meta).array_subclass_named_prefix_token = 0;
    }
}

/// Test an already-published Array-subclass named-prefix proof against the
/// class expected by a consumer.
///
/// The token is stronger than an ordinary-object ShapeId-kind query for this
/// purpose: its publisher admitted only a live ordinary instance of this
/// exact Array-subclass class, validated the complete declared prefix and the
/// dense numeric suffix, and stored the class id in the token itself. Generic
/// structural/semantic transitions clear it before publishing a new ShapeId;
/// only the exact learned numeric-tail transitions preserve it.
///
/// # Safety
///
/// `obj` must already have been validated as a live, non-forwarded
/// `GC_TYPE_OBJECT`. The helper reads only its inline `meta` edge and scalar
/// token payload.
#[inline(always)]
pub(crate) unsafe fn array_subclass_named_prefix_token_matches_class(
    obj: *const ObjectHeader,
    class_id: u32,
) -> bool {
    if obj.is_null() || class_id == 0 {
        return false;
    }
    let meta = (*obj).meta;
    !meta.is_null()
        && (*meta).array_subclass_named_prefix_token
            == (ARRAY_SUBCLASS_NAMED_PREFIX_TOKEN_BIT | u64::from(class_id))
}

/// Clear an established Array-subclass numeric-prefix proof before an owner
/// field store. `layout_note_slot` calls this for inline slots; the object-owned
/// spill path calls it against the owner because its physical store is noted
/// on the child Array buffer instead.
#[inline]
pub(crate) unsafe fn clear_packed_subclass_numeric_proof(obj: *mut ObjectHeader) {
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF == 0
    {
        return;
    }
    let header = std::ptr::from_ref(header).cast_mut();
    // Retire the authority first. A missing/moving meta then merely leaves an
    // unreachable payload, never a proof a future query can consume.
    (*header)._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
    let meta = (*obj).meta;
    if !meta.is_null() {
        (*meta).flags &= !PACKED_NUMERIC_META_MASK;
    }
}

/// Owner-side invalidation for an object-owned spill write. The common
/// no-proof case uses the meta pointer the spill path already loaded and pays
/// only one predictable bit test; it does not re-read the owner's GC header.
#[inline]
pub(crate) unsafe fn note_packed_subclass_spill_store(
    obj: *mut ObjectHeader,
    meta: *mut crate::object::ObjectMeta,
) {
    if !meta.is_null() && (*meta).flags & PACKED_NUMERIC_META_VALID != 0 {
        clear_packed_subclass_numeric_proof(obj);
    }
}

#[inline]
unsafe fn subclass_numeric_prefix_is_proven(
    obj: *const ObjectHeader,
    shape_id: u32,
    bound: u32,
    require_u32: bool,
) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return false;
    };
    let header = std::ptr::from_ref(header).cast_mut();
    if (*header)._reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF == 0 {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        (*header)._reserved &= !crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
        return false;
    }
    let flags = (*meta).flags;
    let payload_valid = flags & PACKED_NUMERIC_META_VALID != 0;
    let proven_bound =
        ((flags & PACKED_NUMERIC_META_BOUND_MASK) >> PACKED_NUMERIC_META_BOUND_SHIFT) as u32;
    let exact_u32 = flags & PACKED_NUMERIC_META_U32 != 0;
    let proven_shape = (flags >> 32) as u32;
    if payload_valid
        && proven_shape == shape_id
        && proven_bound >= bound
        && require_u32 == exact_u32
    {
        return true;
    }
    clear_packed_subclass_numeric_proof(obj as *mut ObjectHeader);
    false
}

#[inline]
unsafe fn publish_subclass_numeric_prefix(
    obj: *const ObjectHeader,
    shape_id: u32,
    bound: u32,
    exact_u32: bool,
) -> bool {
    let meta = (*obj).meta;
    if meta.is_null() || bound > 16_000_000 {
        return false;
    }
    let flags = (*meta).flags;
    (*meta).flags = (flags & !PACKED_NUMERIC_META_MASK)
        | PACKED_NUMERIC_META_VALID
        | if exact_u32 {
            PACKED_NUMERIC_META_U32
        } else {
            0
        }
        | (u64::from(bound) << PACKED_NUMERIC_META_BOUND_SHIFT)
        | (u64::from(shape_id) << 32);
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return false;
    };
    let header = std::ptr::from_ref(header).cast_mut();
    (*header)._reserved |= crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;
    true
}

/// Establish-or-confirm the numeric prefix used by the call-free loop clone.
/// The first visit scans; later visits are two scalar-word checks. Any owner
/// store retires the record before writing, and semantic shape changes fail
/// the exact ShapeId comparison even if they do not touch a value slot.
#[inline]
unsafe fn ensure_subclass_numeric_prefix(
    obj: *const ObjectHeader,
    layout: DenseSubclassLayout,
    bound: u32,
    require_u32: bool,
) -> bool {
    if bound == 0 {
        return true;
    }
    let shape_id = (*obj).parent_class_id;
    if subclass_numeric_prefix_is_proven(obj, shape_id, bound, require_u32) {
        return true;
    }
    for index in 0..bound {
        let Some(slot) = layout.element_base.checked_add(index) else {
            return false;
        };
        let value_ptr = if slot < layout.live_inline_slots {
            (obj as *mut u8)
                .add(std::mem::size_of::<ObjectHeader>())
                .cast::<u64>()
                .add(slot as usize)
        } else {
            let meta = (*obj).meta;
            if meta.is_null() {
                return false;
            }
            let spill = (*meta).spill as *mut ArrayHeader;
            if spill.is_null() || slot >= (*spill).length {
                return false;
            }
            (spill as *mut u8)
                .add(std::mem::size_of::<ArrayHeader>())
                .cast::<u64>()
                .add(slot as usize)
        };
        let value = JSValue::from_bits(ptr::read(value_ptr));
        let number = if value.is_int32() {
            // `push(i)` commonly stores Perry's compact INT32 Number tag. The
            // direct clone consumes raw doubles, so normalize that Number to
            // its representation-equivalent f64 bits during the one-time
            // verification walk. This is pointer-free -> pointer-free and
            // changes no JS-observable type/value, hence needs neither a GC
            // barrier nor a layout downgrade.
            // GC_STORE_AUDIT(POINTER_FREE): canonical raw-f64 Number bits
            // replace compact int32 Number bits in an already numeric slot.
            let integer = value.as_int32();
            let number = integer as f64;
            if !require_u32 {
                ptr::write(value_ptr, number.to_bits());
            }
            number
        } else if !value.is_number() {
            return false;
        } else {
            value.as_number()
        };
        if require_u32 {
            // ECS entity ids in this tier are normalized to Perry's ordinary
            // compact INT32 Number representation. Generic reads still
            // observe the same JS Number, while generated component access
            // can consume the low native lane without an f64 conversion.
            // Values outside the nonnegative i31 subset retain the generic
            // loop; no public behavior is narrowed.
            if !number.is_finite()
                || number < 0.0
                || number > i32::MAX as f64
                || number.fract() != 0.0
            {
                return false;
            }
            if !value.is_int32() {
                // GC_STORE_AUDIT(POINTER_FREE): compact Number bits replace
                // raw-f64 Number bits in an already numeric slot.
                ptr::write(value_ptr, JSValue::int32(number as i32).bits());
            }
        }
    }
    publish_subclass_numeric_prefix(obj, shape_id, bound, require_u32)
}

#[inline]
fn layout_length_value(obj: *const ObjectHeader, layout: DenseSubclassLayout) -> JSValue {
    layout_field_value(obj, layout.length_slot, layout.live_inline_slots)
}

/// Read a slot already proved live by an exact ShapeId. Wide dynamic objects
/// keep post-inline fields in the object-owned spill Array. Reaching that
/// buffer directly is the essential #8655 hot path: the general field helper
/// reclassifies the owner and probes the overflow abstraction for every ECS
/// element even though the shape proof already established all of it.
#[inline(always)]
fn layout_field_value(obj: *const ObjectHeader, slot: u32, live_inline_slots: u32) -> JSValue {
    unsafe {
        if slot < live_inline_slots {
            let fields =
                (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
            return *fields.add(slot as usize);
        }

        if crate::object::object_spill_enabled() {
            let meta = (*obj).meta;
            if !meta.is_null() {
                let spill = (*meta).spill as *const ArrayHeader;
                if !spill.is_null() && slot < (*spill).length {
                    let elements =
                        (spill as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
                    return JSValue::from_bits(*elements.add(slot as usize));
                }
            }
            return JSValue::undefined();
        }

        crate::object::overflow_get(obj as usize, slot as usize)
            .map(JSValue::from_bits)
            .unwrap_or_else(JSValue::undefined)
    }
}

fn nonnegative_u32_length(value: JSValue) -> Option<u32> {
    let number = if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        return None;
    };
    (number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= u32::MAX as f64)
        .then_some(number as u32)
}

/// Fast own `length` read for an object-backed Array subclass. Returning the
/// stored JSValue (rather than coercing it) preserves source property-read
/// semantics; descriptor/prototype-divergent shapes decline above.
#[inline]
pub(crate) fn array_subclass_fast_length(value: f64) -> Option<f64> {
    if let Some(elements) = validated_object_receiver_for_value(value)
        .and_then(|r| super::subclass_elements::elements_for_validated(&r))
    {
        return Some(f64::from(unsafe { (*elements).length }));
    }
    let (obj, layout) = dense_layout_for_value(value)?;
    Some(f64::from_bits(layout_length_value(obj, layout).bits()))
}

/// Fast own `length` read that also primes a pointer-free generated-code IC.
///
/// The three published words are `(identity, length slot, inline bound)`.
/// `identity` is either the exact `(class_id, ShapeId)` pair or the stable
/// Array-subclass named-prefix token used by the dense indexed-read IC.  The
/// payload is published before the identity, and no managed pointer escapes
/// into the cache, so moving GC needs neither a root nor a rewrite hook.
#[inline]
pub(crate) fn array_subclass_fast_length_with_ic(value: f64, cache: *mut u64) -> Option<f64> {
    if let Some(elements) = validated_object_receiver_for_value(value)
        .and_then(|r| super::subclass_elements::elements_for_validated(&r))
    {
        // No shape layout to publish: the IC words describe inline slots,
        // and an elements-backed receiver has none for `length`.
        return Some(f64::from(unsafe { (*elements).length }));
    }
    let (obj, layout) = dense_layout_for_value(value)?;
    let result = f64::from_bits(layout_length_value(obj, layout).bits());
    if !cache.is_null() {
        let family_token = if crate::object::object_spill_enabled() {
            unsafe { array_subclass_named_prefix_token_for_slot(obj, layout.length_slot as usize) }
        } else {
            0
        };
        unsafe {
            cache.add(1).write(layout.length_slot as u64);
            cache.add(2).write(layout.live_inline_slots as u64);
            cache.write(if family_token != 0 {
                family_token
            } else {
                dense_cache_key((*obj).class_id, (*obj).parent_class_id)
            });
        }
    }
    Some(result)
}

#[inline]
pub(crate) fn array_subclass_fast_length_raw(arr: *const ArrayHeader) -> Option<f64> {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let receiver = validated_object_receiver(raw)?;
    if let Some(elements) = super::subclass_elements::elements_for_validated(&receiver) {
        return Some(f64::from(unsafe { (*elements).length }));
    }
    let layout = dense_layout_for_validated_object(receiver.object)?;
    Some(f64::from_bits(
        layout_length_value(receiver.object, layout).bits(),
    ))
}

/// Guarded dense numeric read for an object-backed Array subclass. The live
/// `length` value is checked on every hit, while `dense_prefix_len` caps the
/// proof when a length-only grow created holes without changing the shape.
#[inline]
pub(crate) fn array_subclass_fast_index_get(value: f64, index: u32) -> Option<f64> {
    if let Some(elements) = validated_object_receiver_for_value(value)
        .and_then(|r| super::subclass_elements::elements_for_validated(&r))
    {
        return super::subclass_elements::elements_index_get(elements, index);
    }
    let (obj, layout) = dense_layout_for_value(value)?;
    dense_index_get_with_layout(obj, layout, index)
}

#[inline]
pub(crate) fn array_subclass_fast_index_get_raw(
    arr: *const ArrayHeader,
    index: u32,
) -> Option<f64> {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let receiver = validated_object_receiver(raw)?;
    if let Some(elements) = super::subclass_elements::elements_for_validated(&receiver) {
        return super::subclass_elements::elements_index_get(elements, index);
    }
    let layout = dense_layout_for_validated_object(receiver.object)?;
    dense_index_get_with_layout(receiver.object, layout, index)
}

#[inline]
unsafe fn dense_slot_exists(obj: *const ObjectHeader, slot: u32, live_inline_slots: u32) -> bool {
    if slot < live_inline_slots {
        return true;
    }
    if !crate::object::object_spill_enabled() {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return false;
    }
    let spill = (*meta).spill as *const ArrayHeader;
    !spill.is_null() && slot < (*spill).length && slot < (*spill).capacity
}

#[inline]
unsafe fn store_dense_slot(
    obj: *mut ObjectHeader,
    slot: u32,
    live_inline_slots: u32,
    value_bits: u64,
) -> bool {
    if slot < live_inline_slots {
        crate::object::store_object_field_slot(obj, slot as usize, value_bits);
        return true;
    }
    if !crate::object::object_spill_enabled() {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return false;
    }
    let spill = (*meta).spill as *mut ArrayHeader;
    if spill.is_null() || slot >= (*spill).length || slot >= (*spill).capacity {
        return false;
    }
    note_packed_subclass_spill_store(obj, meta);
    let elements = (spill as *mut u8)
        .add(std::mem::size_of::<ArrayHeader>())
        .cast::<u64>();
    // GC_STORE_AUDIT(BARRIERED): the `note_array_slot` below records layout and emits the spill slot barrier.
    ptr::write(elements.add(slot as usize), value_bits);
    note_array_slot(spill, slot as usize, value_bits);
    true
}

/// Store a raw Number into a dense Array-subclass slot without changing its
/// pointer-layout metadata.
///
/// This is deliberately narrower than [`store_dense_slot`]. The caller has
/// already proved either that the slot was outside the predecessor shape or
/// that its old value was also pointer-free, that its physical storage exists,
/// and that the new value is a nonnegative `i32` encoded as raw f64 bits.
/// Consequently the store cannot publish or remove a heap edge, cannot demote
/// a unique string, and cannot change a pointer-layout bit. Skipping the
/// general layout note and write barrier here removes two full metadata
/// pipelines from numeric tail mutation and the hot ECS swap-with-last write.
#[inline]
unsafe fn store_dense_nonpointer_number_slot(
    obj: *mut ObjectHeader,
    slot: u32,
    live_inline_slots: u32,
    number: f64,
) -> bool {
    let value_bits = number.to_bits();
    if slot < live_inline_slots {
        let fields = (obj as *mut u8)
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>();
        // GC_STORE_AUDIT(POINTER_FREE): the caller proved `number` is a raw nonnegative i32 Number; no edge changes.
        ptr::write(fields.add(slot as usize), value_bits);
        return true;
    }
    if !crate::object::object_spill_enabled() {
        return false;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return false;
    }
    let spill = (*meta).spill as *mut ArrayHeader;
    if spill.is_null() || slot >= (*spill).length || slot >= (*spill).capacity {
        return false;
    }
    let elements = (spill as *mut u8)
        .add(std::mem::size_of::<ArrayHeader>())
        .cast::<u64>();
    // GC_STORE_AUDIT(POINTER_FREE): raw Number into a spill slot the caller proved pointer-free; no edge changes.
    ptr::write(elements.add(slot as usize), value_bits);
    true
}

#[inline]
unsafe fn clear_retired_dense_slot(
    obj: *mut ObjectHeader,
    slot: u32,
    former_live_inline_slots: u32,
) {
    if slot < former_live_inline_slots {
        let fields = (obj as *mut u8)
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>();
        // The predecessor ShapeId is already installed, so this physical tail
        // is outside the object's traced slot range. Clearing it is storage
        // hygiene, not publication of a new edge.
        // GC_STORE_AUDIT(POINTER_FREE): retiring a physical tail slot to `undefined` publishes no edge.
        ptr::write(fields.add(slot as usize), crate::value::TAG_UNDEFINED);
        return;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return;
    }
    let spill = (*meta).spill as *mut ArrayHeader;
    if spill.is_null() || slot >= (*spill).length {
        return;
    }
    let elements = (spill as *mut u8)
        .add(std::mem::size_of::<ArrayHeader>())
        .cast::<u64>();
    // GC_STORE_AUDIT(POINTER_FREE): retiring a spill tail slot to `undefined` publishes no edge.
    ptr::write(elements.add(slot as usize), crate::value::TAG_UNDEFINED);
    note_array_slot(spill, slot as usize, crate::value::TAG_UNDEFINED);
}

/// Clear a numeric tail slot after its exact predecessor shape has been
/// installed. The removed value was constructively classified as a
/// nonnegative i32 Number, so replacing it with `undefined` cannot remove or
/// add a heap edge. Unlike the generic helper above this may therefore leave
/// both the object's and its spill buffer's pointer-layout metadata untouched.
#[inline]
unsafe fn clear_retired_dense_numeric_tail_slot(
    obj: *mut ObjectHeader,
    slot: u32,
    former_live_inline_slots: u32,
) {
    if slot < former_live_inline_slots {
        let fields = (obj as *mut u8)
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>();
        // GC_STORE_AUDIT(POINTER_FREE): retiring a physical tail slot to `undefined` publishes no edge.
        ptr::write(fields.add(slot as usize), crate::value::TAG_UNDEFINED);
        return;
    }
    let meta = (*obj).meta;
    if meta.is_null() {
        return;
    }
    let spill = (*meta).spill as *mut ArrayHeader;
    if spill.is_null() || slot >= (*spill).length {
        return;
    }
    let elements = (spill as *mut u8)
        .add(std::mem::size_of::<ArrayHeader>())
        .cast::<u64>();
    // GC_STORE_AUDIT(POINTER_FREE): retiring a spill tail slot to `undefined` publishes no edge.
    ptr::write(elements.add(slot as usize), crate::value::TAG_UNDEFINED);
}

/// Prove once, while learning an exact semantic shape transition, that neither
/// `length` nor the appended numeric property has custom mutation semantics.
/// Descriptor installation/removal mints a new ShapeId, so a later exact cache
/// hit can consume this proof without rebuilding decimal keys and probing two
/// descriptor maps on every push/pop.
pub(crate) fn array_subclass_tail_descriptors_are_plain(
    obj: *const ObjectHeader,
    index: u32,
) -> bool {
    if !crate::object::object_has_descriptors(obj as usize) {
        return true;
    }
    if crate::object::get_accessor_descriptor(obj as usize, "length").is_some()
        || crate::object::get_property_attrs(obj as usize, "length")
            .is_some_and(|attrs| !attrs.writable())
    {
        return false;
    }
    let mut decimal = [0u8; 10];
    let bytes = decimal_u32(index, &mut decimal);
    let key = unsafe { std::str::from_utf8_unchecked(bytes) };
    if crate::object::get_accessor_descriptor(obj as usize, key).is_some() {
        return false;
    }
    crate::object::get_property_attrs(obj as usize, key).is_none()
}

#[inline(always)]
pub(super) fn mutation_receiver_allows_plain_tail(object_flags: u16) -> bool {
    object_flags
        & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
        == 0
        && super::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed) == 0
}

/// In-bounds indexed write for an exact dense Array-subclass shape. The dense
/// layout builder proves every numeric prefix property is a writable data
/// property; descriptor or prototype mutations publish a different ShapeId
/// and therefore miss that cached proof.
#[inline]
pub(crate) fn array_subclass_fast_index_set(receiver: f64, index: u32, value: f64) -> bool {
    let Some(receiver) = validated_object_receiver_for_value(receiver) else {
        return false;
    };
    array_subclass_fast_index_set_validated(receiver, index, value)
}

#[inline]
pub(crate) fn array_subclass_fast_index_set_raw(
    arr: *const ArrayHeader,
    index: u32,
    value: f64,
) -> bool {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let Some(receiver) = validated_object_receiver(raw) else {
        return false;
    };
    array_subclass_fast_index_set_validated(receiver, index, value)
}

#[inline]
fn array_subclass_fast_index_set_validated(
    receiver: ValidatedObjectReceiver,
    index: u32,
    value: f64,
) -> bool {
    if let Some(done) = super::subclass_elements::elements_index_set(&receiver, index, value) {
        return done;
    }
    let obj = receiver.object;
    let Some(layout) = dense_layout_for_validated_object(obj) else {
        return false;
    };
    let Some(length) = nonnegative_u32_length(layout_length_value(obj, layout)) else {
        return false;
    };
    let Some(slot) = layout.element_base.checked_add(index) else {
        return false;
    };
    if index >= length || index >= layout.dense_prefix_len {
        return false;
    }
    if receiver.object_flags & crate::gc::OBJ_FLAG_FROZEN != 0
        || !unsafe { dense_slot_exists(obj, slot, layout.live_inline_slots) }
    {
        return false;
    }
    let obj = obj as *mut ObjectHeader;
    unsafe {
        let old_value = layout_field_value(obj, slot, layout.live_inline_slots);
        let numeric_u31 = |bits| {
            crate::array::value_bits_to_number(bits).filter(|number| {
                number.is_finite()
                    && *number >= 0.0
                    && *number <= i32::MAX as f64
                    && number.fract() == 0.0
            })
        };
        if let (Some(_old), Some(new)) =
            (numeric_u31(old_value.bits()), numeric_u31(value.to_bits()))
        {
            // Both sides are pointer-free Numbers, so neither the pointer mask
            // nor an established packed-u32 prefix changes. Keep that proof
            // live and overwrite the slot without the general metadata path.
            store_dense_nonpointer_number_slot(obj, slot, layout.live_inline_slots, new)
        } else {
            clear_packed_subclass_numeric_proof(obj);
            store_dense_slot(obj, slot, layout.live_inline_slots, value.to_bits())
        }
    }
}

/// Allocation-free `Array.prototype.push` for a previously observed dense
/// Array-subclass tail transition. The first append at each length learns the
/// ordinary object transition; later cycles reuse it under exact shape and
/// descriptor guards.
#[inline]
pub(crate) fn array_subclass_fast_push_one(receiver: f64, value: f64) -> Option<f64> {
    let receiver = validated_object_receiver_for_value(receiver)?;
    array_subclass_fast_push_one_validated(receiver, value, None)
}

/// Raw-entry counterpart to [`array_subclass_fast_push_one`]. The pointer is
/// magnitude- and header-validated exactly once before the dense mutation.
#[inline]
pub(crate) fn array_subclass_fast_push_one_raw(arr: *const ArrayHeader, value: f64) -> Option<f64> {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let receiver = validated_object_receiver(raw)?;
    array_subclass_fast_push_one_validated(receiver, value, None)
}

/// Raw-entry counterpart for a value constructively proved by generated code
/// to be a nonnegative signed-i32 Number. Besides keeping tagged integers and
/// ClassRefs out of this path, that proof lets the hot Array-subclass append
/// skip `value_bits_to_number`, finiteness, range, and fractional checks.
#[inline]
pub(crate) fn array_subclass_fast_push_u31_raw(arr: *const ArrayHeader, value: u32) -> Option<f64> {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let receiver = validated_object_receiver(raw)?;
    debug_assert!(value <= i32::MAX as u32);
    array_subclass_fast_push_one_validated(receiver, f64::from(value), Some(value))
}

#[inline]
fn array_subclass_fast_push_one_validated(
    receiver: ValidatedObjectReceiver,
    value: f64,
    proven_u31: Option<u32>,
) -> Option<f64> {
    if super::subclass_elements::elements_for_validated(&receiver).is_some() {
        return super::subclass_elements::elements_push(&receiver, value);
    }
    let obj = receiver.object;
    let layout = dense_layout_for_validated_object(obj)?;
    let length = nonnegative_u32_length(layout_length_value(obj, layout))?;
    if length > layout.dense_prefix_len
        || !mutation_receiver_allows_plain_tail(receiver.object_flags)
    {
        return None;
    }
    let predecessor_shape_id = unsafe { (*obj).parent_class_id };
    let transition = crate::object::array_tail_transition::lookup_forward_for_owner(
        obj,
        predecessor_shape_id,
        length,
    )?;
    if length != 0 && transition.slot != layout.element_base.checked_add(length)? {
        return None;
    }
    if layout.live_inline_slots != transition.predecessor_live_inline_slots
        || !unsafe {
            dense_slot_exists(obj, transition.slot, transition.successor_live_inline_slots)
                && dense_slot_exists(
                    obj,
                    layout.length_slot,
                    transition.successor_live_inline_slots,
                )
        }
    {
        return None;
    }

    let obj = obj as *mut ObjectHeader;
    let installed = unsafe {
        crate::object::shapes::install_cache_carried_object_shape_version(
            obj,
            predecessor_shape_id,
            transition.successor_shape_id,
            transition.successor_keys as *mut ArrayHeader,
            transition.slot.saturating_add(1),
        )
    };
    if !installed {
        return None;
    }
    let new_length = length.checked_add(1)?;
    unsafe {
        // ECS entity ids arrive here as exact nonnegative i32 Numbers. Keep
        // this proof constructive and local: tagged class references share the
        // INT32 tag, and arbitrary doubles can look pointer-bearing to the
        // conservative layout classifier, so neither is admitted. The generic
        // barriered store below remains the complete fallback for them.
        let numeric_entity = proven_u31.map(f64::from).or_else(|| {
            crate::array::value_bits_to_number(value.to_bits()).filter(|number| {
                number.is_finite()
                    && *number >= 0.0
                    && *number <= i32::MAX as f64
                    && number.fract() == 0.0
            })
        });
        let (value_stored, length_stored) = if let Some(number) = numeric_entity {
            // `layout_note_slot` used to retire this proof as a side effect.
            // Retire it explicitly before bypassing that general hook.
            clear_packed_subclass_numeric_proof(obj);
            (
                store_dense_nonpointer_number_slot(
                    obj,
                    transition.slot,
                    transition.successor_live_inline_slots,
                    number,
                ),
                store_dense_nonpointer_number_slot(
                    obj,
                    layout.length_slot,
                    transition.successor_live_inline_slots,
                    f64::from(new_length),
                ),
            )
        } else {
            (
                store_dense_slot(
                    obj,
                    transition.slot,
                    transition.successor_live_inline_slots,
                    value.to_bits(),
                ),
                store_dense_slot(
                    obj,
                    layout.length_slot,
                    transition.successor_live_inline_slots,
                    f64::from(new_length).to_bits(),
                ),
            )
        };
        debug_assert!(value_stored && length_stored);
        publish_owner_dense_layout(
            obj,
            DenseSubclassLayout {
                length_slot: layout.length_slot,
                element_base: layout.element_base,
                dense_prefix_len: layout.dense_prefix_len.max(new_length),
                live_inline_slots: transition.successor_live_inline_slots,
            },
        );
    }
    Some(f64::from(new_length))
}

/// Allocation-free `Array.prototype.pop` for an exact learned dense-tail
/// transition. All rejected cases retain the generic observable algorithm.
#[inline]
pub(crate) fn array_subclass_fast_pop(receiver: f64) -> Option<f64> {
    let receiver = validated_object_receiver_for_value(receiver)?;
    array_subclass_fast_pop_validated(receiver)
}

/// Raw-entry counterpart to [`array_subclass_fast_pop`], sharing one validated
/// object-header read across the entire dense-tail mutation.
#[inline]
pub(crate) fn array_subclass_fast_pop_raw(arr: *const ArrayHeader) -> Option<f64> {
    let raw = (arr as u64 & crate::value::POINTER_MASK) as usize;
    let receiver = validated_object_receiver(raw)?;
    array_subclass_fast_pop_validated(receiver)
}

#[inline]
fn array_subclass_fast_pop_validated(receiver: ValidatedObjectReceiver) -> Option<f64> {
    if super::subclass_elements::elements_for_validated(&receiver).is_some() {
        return super::subclass_elements::elements_pop(&receiver);
    }
    let obj = receiver.object;
    let layout = dense_layout_for_validated_object(obj)?;
    let length = nonnegative_u32_length(layout_length_value(obj, layout))?;
    let index = length.checked_sub(1)?;
    if length > layout.dense_prefix_len
        || !mutation_receiver_allows_plain_tail(receiver.object_flags)
    {
        return None;
    }
    let successor_shape_id = unsafe { (*obj).parent_class_id };
    let transition =
        crate::object::array_tail_transition::lookup_reverse_for_owner(obj, successor_shape_id)?;
    if transition.array_index != index
        || transition.slot != layout.element_base.checked_add(index)?
    {
        return None;
    }
    if layout.live_inline_slots != transition.successor_live_inline_slots
        || !unsafe {
            dense_slot_exists(obj, transition.slot, transition.successor_live_inline_slots)
                && dense_slot_exists(
                    obj,
                    layout.length_slot,
                    transition.predecessor_live_inline_slots,
                )
        }
    {
        return None;
    }
    let value = layout_field_value(obj, transition.slot, transition.successor_live_inline_slots);
    let numeric_entity = crate::array::value_bits_to_number(value.bits()).filter(|number| {
        number.is_finite() && *number >= 0.0 && *number <= i32::MAX as f64 && number.fract() == 0.0
    });
    let obj = obj as *mut ObjectHeader;
    unsafe { clear_packed_subclass_numeric_proof(obj) };
    crate::object::prop_plan::prop_plan_epoch_bump();
    let installed = unsafe {
        crate::object::shapes::install_cache_carried_object_shape_version(
            obj,
            successor_shape_id,
            transition.predecessor_shape_id,
            transition.predecessor_keys as *mut ArrayHeader,
            transition.slot,
        )
    };
    if !installed {
        return None;
    }
    unsafe {
        let length_stored = if numeric_entity.is_some() {
            clear_retired_dense_numeric_tail_slot(
                obj,
                transition.slot,
                transition.successor_live_inline_slots,
            );
            store_dense_nonpointer_number_slot(
                obj,
                layout.length_slot,
                transition.predecessor_live_inline_slots,
                f64::from(index),
            )
        } else {
            clear_retired_dense_slot(obj, transition.slot, transition.successor_live_inline_slots);
            store_dense_slot(
                obj,
                layout.length_slot,
                transition.predecessor_live_inline_slots,
                f64::from(index).to_bits(),
            )
        };
        debug_assert!(length_stored);
        publish_owner_dense_layout(
            obj,
            DenseSubclassLayout {
                length_slot: layout.length_slot,
                element_base: layout.element_base,
                dense_prefix_len: index,
                live_inline_slots: transition.predecessor_live_inline_slots,
            },
        );
    }
    Some(f64::from_bits(value.bits()))
}

#[inline(always)]
fn dense_index_get_with_layout(
    obj: *const ObjectHeader,
    layout: DenseSubclassLayout,
    index: u32,
) -> Option<f64> {
    let length = nonnegative_u32_length(layout_length_value(obj, layout))?;
    if index >= length || index >= layout.dense_prefix_len {
        return None;
    }
    let slot = layout.element_base.checked_add(index)?;
    let value = layout_field_value(obj, slot, layout.live_inline_slots);
    Some(f64::from_bits(value.bits()))
}

fn canonical_u32_index(value: f64) -> Option<u32> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return (js.as_int32() >= 0).then_some(js.as_int32() as u32);
    }
    (js.is_number()
        && value.is_finite()
        && value >= 0.0
        && value.fract() == 0.0
        && value <= (u32::MAX - 1) as f64)
        .then_some(value as u32)
}

/// Unknown-receiver numeric read used by codegen's guarded typed-array miss
/// block. Stable real arrays and Array subclasses terminate here; every other
/// receiver/key keeps the established tag-aware dispatcher as a cold side
/// exit. Keeping that call behind this ABI boundary removes `js_dyn_index_get`
/// from the emitted hot-loop artifact without weakening its semantics.
#[no_mangle]
/// The five optional IC words are
/// scalar layout facts, never heap pointers:
/// `(class_id, ShapeId)`, length slot, element base, dense prefix, inline bound.
/// The emitted hit path reloads the live object/meta/spill pointers, so moving
/// GC never has to trace or rewrite this cache.
pub extern "C" fn js_packed_arraylike_index_get(receiver: f64, index: f64, cache: *mut u64) -> f64 {
    if let Some(index_u32) = canonical_u32_index(index) {
        let js = JSValue::from_bits(receiver.to_bits());
        if js.is_pointer() {
            let raw = js.as_pointer::<u8>();
            if let Some(header) =
                unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }
            {
                if matches!(
                    header.obj_type,
                    crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
                ) {
                    return crate::array::js_array_get_f64(
                        raw as *const crate::array::ArrayHeader,
                        index_u32,
                    );
                }
                if header.obj_type == crate::gc::GC_TYPE_OBJECT
                    && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                {
                    let obj = raw.cast::<ObjectHeader>();
                    // Elements-backed instance: an in-bounds non-hole element
                    // answers directly; a hole continues to the complete
                    // dispatcher (prototype chain).
                    let elements = unsafe { super::subclass_elements::elements_of(obj) };
                    if !elements.is_null() {
                        if let Some(value) =
                            super::subclass_elements::elements_index_get(elements, index_u32)
                        {
                            return value;
                        }
                    } else if let Some(layout) = dense_layout_for_validated_object(obj) {
                        // The codegen hit path handles both inline and
                        // object-owned spill slots.  In spill mode, publish a
                        // class-wide dense-tail identity when the owner has
                        // proved one.  Exact push/pop transitions preserve
                        // that move-stable token, so a lifecycle loop does not
                        // miss once for every historical tail ShapeId.  The
                        // cached dense-prefix word remains the admitted high
                        // water mark: a generic `length` grow beyond it still
                        // side-exits and re-establishes the complete proof.
                        if !cache.is_null() && crate::object::object_spill_enabled() {
                            let family_token = unsafe {
                                array_subclass_named_prefix_token_for_slot(
                                    obj,
                                    layout.length_slot as usize,
                                )
                            };
                            unsafe {
                                cache.add(1).write(layout.length_slot as u64);
                                cache.add(2).write(layout.element_base as u64);
                                cache.add(3).write(layout.dense_prefix_len as u64);
                                cache.add(4).write(layout.live_inline_slots as u64);
                                cache.write(if family_token != 0 {
                                    family_token
                                } else {
                                    dense_cache_key((*obj).class_id, (*obj).parent_class_id)
                                });
                            }
                        }
                        if let Some(value) = dense_index_get_with_layout(obj, layout, index_u32) {
                            return value;
                        }
                    }
                }
            }
        }
    }
    crate::value::js_dyn_index_get(receiver, index)
}
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_INDEX_GET: extern "C" fn(f64, f64, *mut u64) -> f64 =
    js_packed_arraylike_index_get;

/// True when `class_id` is a user class that extends `Array` (the reserved
/// parent id `0xFFFF0024` appears in its class chain), i.e. `class X extends
/// Array`. Such instances are plain `ObjectHeader`s, so the array-like engines
/// must run on them (`x.push(1)`, `x.map(...)`) — they are otherwise excluded by
/// the "plain objects only" guard alongside ordinary user classes. Purely
/// additive: only newly admits Array subclasses, never changes plain-object or
/// ordinary-class-instance behavior.
pub(crate) fn is_array_subclass_class_id(class_id: u32) -> bool {
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == 0 {
        return false;
    }
    let mut cur = class_id;
    // Bounded walk up the parent chain; guards against a corrupt cyclic edge.
    for _ in 0..64 {
        match crate::object::get_parent_class_id(cur) {
            Some(parent) if parent == CLASS_ID_ARRAY => return true,
            Some(parent) => cur = parent,
            None => return false,
        }
    }
    false
}

/// True when `object` is a live `class X extends Array` instance: a heap
/// `GC_TYPE_OBJECT` whose class id chains to the reserved `Array` parent id.
/// Used to route inherited *read* Array methods (`map` / `filter` / `join` /
/// `at` / `indexOf` / …) and iteration/spread over the subclass instance.
/// `try_read_gc_header` magnitude-classifies the address first, so a non-heap
/// handle id is never dereferenced as a `GcHeader`.
pub fn is_array_subclass_instance(object: f64) -> bool {
    let jsv = JSValue::from_bits(object.to_bits());
    if !jsv.is_pointer() {
        return false;
    }
    let raw = jsv.as_pointer::<u8>();
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        return false;
    }
    let obj_type = match unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) } {
        Some(hdr) => hdr.obj_type,
        None => return false,
    };
    if obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    is_array_subclass_class_id(class_id)
}

/// Materialize a `class X extends Array` instance into a fresh dense array by
/// reading its `length` + indexed elements through the array-like accessors.
/// Iteration (`for…of`, spread, `Array.from`, destructuring, `[].concat(sub)`)
/// drives the array iterator / spread, which read a real `ArrayHeader`; an
/// object-backed subclass instance would be misread, so those paths iterate
/// this snapshot instead. Snapshot (not live) semantics — a full fix would need
/// an object-backed array iterator. Absent indices materialize as `undefined`
/// (not preserved holes): correct for iteration/spread (the array iterator
/// yields `undefined` for holes anyway); a sparse subclass fed to `concat`
/// therefore yields `undefined` rather than a preserved hole — an accepted
/// limitation for this rare case.
pub fn array_subclass_dense_snapshot(recv: f64) -> f64 {
    // An elements-backed instance iterates its live inner array, exactly as
    // a plain Array does (no snapshot: a live length, live holes).
    if let Some((_, elements)) = crate::array::subclass_elements::backed_value(recv) {
        return crate::value::js_nanbox_pointer(elements as i64);
    }
    let len = al_length(recv).max(0);
    // ArrayCreate throws a RangeError for len ≥ 2^32 (matching `js_arraylike_map`)
    // — and, critically, this guard prevents the `as u32` truncation below from
    // under-allocating the buffer while the `0..len` loop iterates the full i64
    // count and writes out of bounds.
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let result = js_array_alloc_with_length(len as u32);
    let elems = unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..len {
        let v = al_get(recv, k);
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): note_array_slot re-stores with the barrier.
            ptr::write(elems.add(k as usize), v);
            note_array_slot(result, k as usize, v.to_bits());
        }
    }
    nanbox_arr(result)
}

/// True when an Array-subclass instance carries a USER `[Symbol.iterator]`
/// override — an own `inst[Symbol.iterator] = …` / symbol accessor, or a class
/// method `*[Symbol.iterator]()` (registered under the synthetic `@@iterator`
/// name). The default array iterator is a runtime default (not a class vtable
/// method), so a hit means the user declared their own. The snapshot iteration
/// shortcuts must defer to such an override and only synthesize the default
/// array iterator when none exists. Mirrors
/// `object::map_set_subclass::subclass_has_iterator_override`.
pub fn array_subclass_has_iterator_override(value: f64) -> bool {
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if iter_wk.is_null() {
        return false;
    }
    let iter_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
    if unsafe { crate::symbol::own_symbol_property(value, iter_f64) }.is_some() {
        return true;
    }
    let raw = value.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    class_id != 0 && crate::object::method_owner_class_id(class_id, "@@iterator").is_some()
}

// ---------------------------------------------------------------------------
// #7574 — raw `js_array_*` receiver resolution for an array-like OBJECT.
//
// Codegen decides "this receiver is an Array" from the DECLARED TypeScript type
// of the binding (`is_array_expr` / `Type::Array(_)` / `Generic { base: "Array" }`),
// then emits a raw `js_array_*` call whose first act is to dereference the
// receiver as an `ArrayHeader`. A declared type is a hint, never a layout fact
// (CLAUDE.md, *Known Limitations*: annotations are erased and nothing validates
// them at runtime), so any binding annotated with the BASE type — `const a:
// number[] = new MyArr()`, a parameter, a class field, a return type, an
// `as number[]` cast — can be holding a `class X extends Array` instance, which
// perry models as a plain `ObjectHeader`. The two headers overlay field for
// field:
//
//     ArrayHeader.length   (u32 @0)  <- ObjectHeader.class_id        (#8113)
//     ArrayHeader.capacity (u32 @4)  <- ObjectHeader.parent_class_id  (ShapeId)
//     elements[0]          (@8)      <- keys_array   (a *mut ArrayHeader)
//     elements[1]          (@16)     <- meta         (a *mut ObjectMeta)
//     elements[2]          (@24)     <- inline field slot 0
//
// so element WRITES overwrite two live GC child edges with arbitrary doubles —
// the collector then traces whatever the mutator stored. `a.push(1); a.push(2)`
// SIGSEGVs (exit 139) on the second push.
//
// `clean_arr_ptr` now refuses a `GC_TYPE_OBJECT` allocation outright, which
// makes every one of its ~190 call sites fail-CLOSED. That is the memory-safety
// half. The correctness half is these helpers: the entry points reachable from
// the declared-type codegen tiers re-enter through their EXISTING null branch
// and run the operation on the spec-generic array-like engine
// (`super::generic` / `super::generic_object`), which already models an Array
// subclass correctly — it is the same engine the UNANNOTATED path has always
// used via `js_native_call_method`.
//
// Unlike #7573's Map/Set fix there is nothing to *redirect* to: an Array
// subclass instance has no hidden backing collection (`js_array_subclass_init`
// installs a `length` own property and the elements are ordinary indexed object
// properties — see `node_stream_constructors/builders.rs`). Minting one would
// split element storage in two, since `Object.keys` / `for…in` /
// `JSON.stringify` / the generic engine all read the object's own properties;
// the answer here is therefore "run the generic engine", not "redirect".
// ---------------------------------------------------------------------------

/// The array-like OBJECT receiver a raw `js_array_*` entry point must actually
/// run on, or `None` when the pointer is not one.
///
/// Admits exactly what [`super::generic::plain_object_value`] admits — an
/// object literal, an anonymous shape, or a `class X extends Array` instance —
/// so ordinary user-class instances, real arrays, typed arrays, buffers, and
/// proxies all answer `None` and keep their existing behaviour.
///
/// Marked `#[cold]`/`#[inline(never)]`: every caller reaches it only from a
/// branch `clean_arr_ptr` already refused, so a genuine `ArrayHeader` never
/// executes a byte of this.
/// One-load brand pre-filter: true only when `arr` has a readable `GcHeader`
/// saying `GC_TYPE_OBJECT`. A genuine `ArrayHeader` answers false without
/// touching a side table, so callers can gate the (registry-probing)
/// [`array_object_receiver`] behind it on a hot path. Uses
/// `addr_class::try_read_gc_header`, which magnitude-classifies the address
/// before any dereference.
#[inline]
pub(crate) fn raw_receiver_is_heap_object(arr: *const ArrayHeader) -> bool {
    let raw = ((arr as u64) & 0x0000_FFFF_FFFF_FFFF) as usize;
    validated_object_receiver(raw).is_some()
}

#[cold]
#[inline(never)]
pub(crate) fn array_object_receiver(arr: *const ArrayHeader) -> Option<f64> {
    let raw = (arr as u64) & 0x0000_FFFF_FFFF_FFFF;
    if raw == 0 {
        return None;
    }
    super::generic::plain_object_value(raw as *const ArrayHeader)
}

/// True when `value` is a live `class X extends Array` INSTANCE — the
/// annotation-independent brand test the generic `[[Set]]` funnels use to
/// decide whether the Array-exotic `length` steps apply.
pub(crate) fn is_array_subclass_value(value: f64) -> bool {
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    // `raw_receiver_is_heap_object` magnitude-classifies through
    // `addr_class::try_read_gc_header` and proves `GC_TYPE_OBJECT` before the
    // class-id read below dereferences the header.
    if !raw_receiver_is_heap_object(raw as *const ArrayHeader) {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw);
    class_id != 0 && is_array_subclass_class_id(class_id)
}

/// Run an `Array.prototype` method generically on the array-like object
/// `recv`, covering both the mutating family (`push` / `pop` / `shift` /
/// `unshift` / `reverse` / `splice` / `sort` / `concat`) and the read family
/// (`map` / `filter` / `forEach` / `join` / `slice` / `indexOf` / …).
///
/// Returns `None` only for a method name neither engine implements.
#[cold]
#[inline(never)]
pub(crate) fn array_object_method(recv: f64, method: &str, args: &[f64]) -> Option<f64> {
    if method == "push" && args.len() == 1 {
        if let Some(length) = array_subclass_fast_push_one(recv, args[0]) {
            return Some(length);
        }
    } else if method == "pop" && args.is_empty() {
        if let Some(value) = array_subclass_fast_pop(recv) {
            return Some(value);
        }
    } else if method == "fill" {
        // `Array.prototype.fill` over the receiver's own `length` + indexed
        // properties. An elements-backed instance has no own `fill` method
        // (see `js_array_subclass_init`), so this funnel is where the
        // inherited one is served.
        return Some(crate::array::js_array_fill_generic(
            recv,
            args.first()
                .copied()
                .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED)),
            i32::from(args.len() > 1),
            args.get(1).copied().unwrap_or(0.0),
            i32::from(args.len() > 2),
            args.get(2).copied().unwrap_or(0.0),
        ));
    }
    let (ptr, len) = (args.as_ptr(), args.len());
    if let Some(result) = super::generic::run_object_mutator(recv, method, ptr, len) {
        return Some(result);
    }
    super::generic::dispatch_arraylike_read_method(recv, method, ptr, len)
}

/// `Get(recv, ToString(index))` for an array-like object receiver.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_get(recv: f64, index: u32) -> f64 {
    if let Some(value) = array_subclass_fast_index_get(recv, index) {
        return value;
    }
    al_get(recv, index as i64)
}

/// `Set(recv, ToString(index), value, …)` PLUS the Array-exotic `length`
/// maintenance the receiver's class inherits from `Array`.
///
/// A `class X extends Array` instance is a real Array in JavaScript, so
/// `sub[3] = v` sets `length` to 4 (ECMA-262 §10.4.2.1
/// `ArraySetLength`/`ArrayDefineOwnProperty`). Perry models the instance as a
/// plain object, whose `[[DefineOwnProperty]]` has no such step — pre-fix
/// `sub[0] = 10; sub.length` read back `0`, on the ANNOTATED and unannotated
/// paths alike. Emulate the exotic step here so both agree with node.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_set(recv: f64, index: u32, value: f64) {
    if array_subclass_fast_index_set(recv, index, value) {
        return;
    }
    // The store interns a key string and can allocate, so root the receiver
    // across it — it is a movable `ObjectHeader` and is read again below.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    crate::object::js_object_set_index_polymorphic(
        (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
        index as f64,
        value,
    );
    maintain_array_exotic_length(handle.get_nanbox_f64(), index);
}

/// The Array-exotic post-step for an indexed own-property write, applied by
/// the two generic OBJECT store funnels (`js_put_value_set` and
/// `js_object_set_index_polymorphic`) AFTER the store has landed.
///
/// `sub[3] = v` on a `class X extends Array` instance must leave `length == 4`.
/// Perry models the instance as a plain object, so nothing in its
/// `[[DefineOwnProperty]]` does that — pre-fix `sub[0] = 10; sub.length` read
/// back `0` on the annotated AND unannotated paths alike, which then made the
/// next `sub.push(v)` append at index 0 and overwrite the element.
///
/// Gated on the receiver's class chain reaching `Array`, so an object literal
/// (`class_id == 0`) short-circuits on one load and an ordinary class instance
/// on a bounded parent walk. `key` is a property-key VALUE; a non-canonical
/// array index (`"length"`, `"foo"`, `"01"`, a symbol) is a no-op.
pub(crate) fn note_array_subclass_index_write(recv: f64, key: f64) {
    // An elements-backed instance's `length` is the inner array's: the index
    // store already maintained it, and no numeric proof lives on the shape.
    if crate::array::subclass_elements::backed_value(recv).is_some() {
        return;
    }
    // Stringifying a numeric key can allocate and evacuate the object. Keep
    // both inputs live, then re-read the receiver before retiring its proof.
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_h = scope.root_nanbox_f64(recv);
    let key_h = scope.root_nanbox_f64(key);
    if !is_array_subclass_value(recv_h.get_nanbox_f64()) {
        return;
    }
    let key_ptr = crate::value::js_jsvalue_to_string(key_h.get_nanbox_f64())
        as *const crate::string::StringHeader;
    // The `&str` borrows the heap `StringHeader`'s bytes. `canonical_array_index`
    // only parses digits — it allocates nothing, so the borrow cannot straddle a
    // collection point (the `&[u8]`-into-a-StringHeader hazard in CLAUDE.md).
    let index = unsafe {
        match crate::object::has_own_helpers::str_from_string_header(key_ptr)
            .and_then(crate::object::canonical_array_index)
        {
            Some(i) => i,
            None => return,
        }
    };
    let live_recv = recv_h.get_nanbox_f64();
    let raw = (live_recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    // A successful value overwrite does not necessarily change shape and a
    // pointer-free tag (notably an SSO string) needs no GC layout note. Retire
    // the numeric authority explicitly so neither case can reuse stale proof.
    unsafe { clear_packed_subclass_numeric_proof(raw) };
    maintain_array_exotic_length(live_recv, index);
}

/// The `length`-bumping half of `array_object_index_set`, split out so the
/// generic OBJECT index-store funnels can apply it without re-entering the
/// store.
pub(crate) fn maintain_array_exotic_length(recv: f64, index: u32) {
    if crate::array::subclass_elements::backed_value(recv).is_some() {
        return;
    }
    let current = al_length(recv);
    if (index as i64) < current {
        return;
    }
    // `js_string_from_bytes` ALLOCATES, so it is a collection point: root the
    // receiver and re-read it afterwards rather than deriving the raw pointer
    // first (the #7192 store-after-an-allocating-call shape — a movable
    // `ObjectHeader` written through a pre-allocation address lands on a
    // forwarding stub).
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::js_object_set_field_by_name(raw, key, (index as f64) + 1.0);
}

/// `Set(recv, "length", new_length, true)` for an array-like object receiver:
/// truncating deletes the indices at or above the new length, exactly as the
/// Array-exotic `[[DefineOwnProperty]]` would.
#[cold]
#[inline(never)]
pub(crate) fn array_object_set_length(recv: f64, new_length: f64) {
    if let Some((obj, elements)) = crate::array::subclass_elements::backed_value(recv) {
        unsafe { crate::array::subclass_elements::set_length(obj, elements, new_length) };
        return;
    }
    if !new_length.is_finite() || new_length < 0.0 || new_length.trunc() != new_length {
        crate::array::array_length_range_error();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let target = new_length as i64;
    let current = al_length(handle.get_nanbox_f64());
    for k in target..current {
        let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        crate::object::js_object_delete_dynamic(raw, k as f64);
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::set_field_by_name_object_tail(raw, key, new_length);
}
