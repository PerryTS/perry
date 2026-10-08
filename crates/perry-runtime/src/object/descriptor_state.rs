//! Property attributes and accessor pairs, owned by holder shapes and slots.

use super::*;

/// Per-property attribute flags set by `Object.defineProperty` / `Object.freeze` / `Object.seal`.
/// Tracks the JS PropertyDescriptor attributes (writable, enumerable, configurable) for keys
/// that have been customized away from the default `{ writable: true, enumerable: true, configurable: true }`.
/// Encoded in the holder shape's key entry.
///
/// Bit layout: 0x01 = writable, 0x02 = enumerable, 0x04 = configurable.
/// Default (no entry) is `0x07` (all true). An entry of `0x06` means non-writable but enumerable+configurable.
#[derive(Clone, Copy)]
pub(crate) struct PropertyAttrs {
    pub bits: u8,
}
impl PropertyAttrs {
    pub(crate) const WRITABLE: u8 = 0x01;
    pub(crate) const ENUMERABLE: u8 = 0x02;
    pub(crate) const CONFIGURABLE: u8 = 0x04;
    pub const fn new(writable: bool, enumerable: bool, configurable: bool) -> Self {
        let mut bits = 0u8;
        if writable {
            bits |= Self::WRITABLE;
        }
        if enumerable {
            bits |= Self::ENUMERABLE;
        }
        if configurable {
            bits |= Self::CONFIGURABLE;
        }
        Self { bits }
    }
    pub const fn writable(self) -> bool {
        (self.bits & Self::WRITABLE) != 0
    }
    pub const fn enumerable(self) -> bool {
        (self.bits & Self::ENUMERABLE) != 0
    }
    pub const fn configurable(self) -> bool {
        (self.bits & Self::CONFIGURABLE) != 0
    }
}

mod filter;
pub(crate) use filter::may_have_descriptor_entry;
#[cfg(test)]
pub(crate) use filter::test_may_have_descriptor_entry;
use filter::{descriptor_route, DescriptorRoute};
mod holder_edit;
use super::accessor_pair::{descriptor_from, own_accessor, pair_from, store_own_accessor};
pub(crate) use holder_edit::HolderEdit;
#[cfg(test)]
mod native_owner_tests;
#[cfg(test)]
mod holder_route_tests;
#[cfg(test)]
mod tests;

/// Accessor values loaded from the holder's ordinary property slot.
/// A zero bits value means "no getter" or "no setter". Entries here represent properties
/// installed via `Object.defineProperty(obj, key, { get, set })` — those must route reads
/// through the getter closure and writes through the setter closure instead of touching
/// the underlying field slot.
#[derive(Clone, Copy, Default)]
pub(crate) struct AccessorDescriptor {
    pub get: u64, // NaN-boxed closure f64 bits, 0 = absent
    pub set: u64, // NaN-boxed closure f64 bits, 0 = absent
}

/// Retire existing class method guards when a prototype descriptor changes.
pub(crate) fn invalidate_prototype_descriptor_guards(obj: usize, key: &str) {
    if crate::array::object_prototype_addr_matches(obj)
        || class_registry::is_registered_class_prototype_object(obj)
        || class_registry::class_id_for_decl_prototype_object(obj).is_some()
    {
        class_registry::invalidate_class_prototype_fast_guards_for_method(key);
    }
}

/// True when a write of `key` to a plain object whose prototype is the canonical
/// `Object.prototype` might be intercepted there (inherited setter / non-writable
/// data) and must therefore take the slow [[Set]] walk.

pub(crate) fn object_proto_may_intercept_key(key: f64) -> bool {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe {
        crate::string::js_string_key_bytes(
            crate::value::JSValue::from_bits(key.to_bits()),
            &mut scratch,
        )
    }) else {
        return true;
    };
    // The intrinsic Annex-B setter is implemented by the ordinary Set walk.
    if bytes == b"__proto__" {
        return true;
    }
    let proto = crate::array::object_prototype_addr_if_resolved();
    proto != 0
        && unsafe {
            super::key_attrs::object_key_blocks_plain_store(proto as *const ObjectHeader, bytes)
        }
}

/// Whether a fast plain-data write of `key` to a CLASS INSTANCE (`class_id != 0`)
/// at `obj_addr` might be intercepted by its prototype chain — i.e. the slow
/// `[[Set]]` walk is required instead of a direct own-data store. Conservative:
/// any uncertainty returns `true` (take the slow path).
///
/// Every interception source is a property of a prototype object. The walk
/// reads accessor and non-writable data attributes from each holder's shape,
/// including declared class accessors and `Object.prototype` at the tail.
///
/// Own-instance descriptors / frozen / sealed are excluded by the caller before
/// this is reached.
pub(crate) unsafe fn class_instance_set_may_intercept(
    obj_addr: usize,
    _class_id: u32,
    key: f64,
) -> bool {
    // Decode the key once — used for both the class-chain and per-prototype
    // accessor probes below.
    let name = match reflect_support::key_to_rust_string(key) {
        Some(n) => n,
        // Non-decodable / non-string key: do not risk the fast path.
        None => return true,
    };
    // Walk the actual prototype objects; their shapes own all accessors.
    let mut proto = js_object_get_prototype_of(crate::value::js_nanbox_pointer(obj_addr as i64));
    let mut depth = 0u32;
    loop {
        depth += 1;
        if depth > 64 {
            // Pathologically deep / cyclic chain — be safe.
            return true;
        }
        let bits = proto.to_bits();
        let top16 = bits >> 48;
        // Classify the prototype value before dereferencing it — mirror the
        // shapes `js_object_get_prototype_of` can hand back:
        //  - 0x7FFD NaN-boxed pointer: a small-handle payload (e.g. a Proxy)
        //    is NOT an ObjectHeader and may carry a trap → be conservative.
        //  - top16 == 0 raw pointer: module-level object literals recorded via
        //    `Object.setPrototypeOf` come back as raw I64 pointers.
        //  - null / undefined: genuine end of chain, nothing to intercept.
        //  - anything else: unknown shape → do not risk the fast path.
        let p = if top16 == 0x7FFD {
            let p = (bits & crate::value::POINTER_MASK) as usize;
            if p == 0 {
                return false;
            }
            if crate::value::addr_class::is_small_handle(p) {
                // Proxy / handle prototype — assume it may intercept the write.
                return true;
            }
            p
        } else if top16 == 0 && bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            bits as usize
        } else if bits == crate::value::TAG_NULL || bits == crate::value::TAG_UNDEFINED {
            return false;
        } else {
            return true;
        };
        if crate::array::object_prototype_addr_matches(p) {
            // Reached the canonical Object.prototype: per-key check, then done.
            return object_proto_may_intercept_key(key);
        }
        // Per-KEY intercepting descriptor on this class prototype. A blanket
        // `object_has_descriptors(p)` bail is too coarse — every class prototype
        // carries descriptors (constructor / method install), which would defeat
        // the fast path entirely. Only an inherited accessor or non-writable data
        // property *named this key* actually intercepts the write.
        // Charter step 3: an ordinary prototype's attributes live with its
        // keys, and its shape's summary proves most hops (methods are
        // non-enumerable, never non-writable) without a key lookup.
        if super::key_attrs::attrs_live_in_keys(p) {
            if super::key_attrs::object_key_blocks_plain_store(
                p as *const ObjectHeader,
                name.as_bytes(),
            ) {
                return true;
            }
        } else if object_has_descriptors(p) {
            if get_accessor_descriptor(p, &name).is_some() {
                return true;
            }
            if let Some(attrs) = get_property_attrs(p, &name) {
                if !attrs.writable() {
                    return true;
                }
            }
        }
        proto = js_object_get_prototype_of(proto);
    }
}

pub(crate) use super::key_attrs::AttrsEdit;

/// A single DATA-descriptor install through the funnel.
pub(crate) fn note_data_descriptor_target(obj: usize, key: &str, attrs: PropertyAttrs) {
    note_descriptor_target_edits(obj, &[AttrsEdit::Data(key.as_bytes(), attrs.bits)]);
}

/// An ACCESSOR install through the funnel. The keys record which halves are
/// present, never the getter/setter identities, which differ per receiver
/// (zod binds a fresh closure per schema) and live with the receiver.
pub(crate) fn note_accessor_descriptor_target(obj: usize, key: &str, acc: &AccessorDescriptor) {
    note_descriptor_target_edits(
        obj,
        &[AttrsEdit::Accessor(
            key.as_bytes(),
            acc.get != 0,
            acc.set != 0,
        )],
    );
}

/// Apply descriptor facts to the ordinary holder shape. Arrays, functions,
/// native handles and exotic cells normalize to their existing property bag.
/// A changed entry publishes a new holder ShapeId and invalidates its memos.
pub(crate) fn note_descriptor_target_edits(obj: usize, edits: &[AttrsEdit<'_>]) {
    let function = HolderEdit::new(obj);
    if let Some(edit) = &function {
        edit.materialize_data_keys(edits);
    }
    let obj = function.as_ref().map_or(obj, |edit| edit.bag as usize);
    if crate::typedarray::lookup_typed_array_kind(obj).is_some() {
        return;
    }
    unsafe {
        // A write: prove the owner is a GC allocation before touching its
        // header or keys (`attrs_live_in_keys_for_install`).
        if let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(obj) {
            let header = header.as_ptr();
            if (*header).obj_type == crate::gc::GC_TYPE_OBJECT {
                (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
                super::key_attrs::apply_edits(obj as *mut crate::object::ObjectHeader, edits);
            }
        }
    }
}

/// The receiver-level bookkeeping of a descriptor install, for an object
/// born with a layout whose keys already carry their attributes (an
/// arguments object's `length`/`callee`): the per-object descriptor bit and
/// its holder shape is already authoritative.
pub(crate) fn note_attrs_born_with_keys(obj: usize) {
    note_descriptor_target_edits(obj, &[]);
}

/// An accessor born in the object's attributed key layout also needs the
/// ordinary descriptor bookkeeping, even though no install runs.
pub(crate) fn note_accessor_born_with_keys(obj: usize) {
    note_attrs_born_with_keys(obj);
}

/// Look up the property descriptor for (obj, key). Returns None if no entry exists,
/// in which case the JS default `{ writable: true, enumerable: true, configurable: true }` applies.
pub(crate) fn get_property_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(obj);
        if !bag.is_null() {
            let entry = super::key_attrs::object_key_entry(bag, key.as_bytes());
            let keys = super::object_keys(bag);
            if entry != 0
                || (bag as usize != obj
                    && super::keys_find_property_slot_by_bytes(
                        keys.arr(),
                        keys.count(),
                        key.as_bytes(),
                    )
                    .is_some())
            {
                return Some(PropertyAttrs {
                    bits: super::key_attrs::entry_to_attr_bits(entry),
                });
            }
        }
    }
    string_wrapper_index_attrs(obj, key)
}

/// ECMA-262 §10.4.3: every in-range integer index of a `String` exotic object
/// (`new String("abc")`, and the wrapper `ToObject` mints for a sloppy method
/// call on a string primitive) has the descriptor
/// `{ writable: false, enumerable: true, configurable: false }`. That is a
/// property of the CLASS and of the boxed length — never of the individual
/// object — so it is answered from the wrapper's own payload instead of being
/// stored once per character in `PROPERTY_DESCRIPTORS`.
///
/// Storing it cost, per boxed character: a `String` key on the Rust heap, a
/// hash-map entry that only a full collection's dead-owner prune can reclaim,
/// an owner-index entry, a meta-descriptor key bit, and one program-wide
/// `prop_plan_epoch_bump()`. On the compiled claude-code TUI, whose render
/// path boxes a receiver per string method call, those entries were the
/// unbounded half of the process's resident growth during a turn.
///
/// A REAL entry still wins (the probe above runs first): `Object.freeze` or
/// an explicit `defineProperty` on a wrapper installs one and is observed.
///
/// The first byte is checked before anything else: an index key starts with an
/// ASCII digit, so every ordinary property name leaves through one compare.
#[inline]
fn string_wrapper_index_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    let bytes = key.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let index = canonical_index_key(bytes)?;
    let len = crate::builtins::boxed_string_wrapper_utf16_len(obj)?;
    (index < len).then(|| PropertyAttrs::new(false, true, false))
}

/// `CanonicalNumericIndexString` for the digits-only case: the key must be the
/// exact `ToString` of the integer it names, so `"0"` is an index but `"01"`,
/// `"1.0"` and `""` are not (mirrors `string::canonical_string_index`).
#[inline]
fn canonical_index_key(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 10 {
        return None;
    }
    if bytes[0] == b'0' {
        return (bytes.len() == 1).then_some(0);
    }
    let mut value: u64 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value * 10 + (b - b'0') as u64;
        if value > u32::MAX as u64 {
            return None;
        }
    }
    u32::try_from(value).ok()
}

/// Does the own holder shape carry customized descriptor facts?
pub(crate) fn object_has_descriptors(obj: usize) -> bool {
    // Ordinary property readers already carry an object holder. Its shape is
    // the answer; normalizing it through closure/byte/exotic storage repeats
    // admission work that belongs only to those other receiver kinds.
    unsafe {
        if super::key_attrs::attrs_live_in_keys(obj) {
            return super::key_attrs::object_summary(obj as *const ObjectHeader) != 0;
        }
    }
    owner_may_have_descriptor_entries(obj, false)
}

/// Does this holder's shape carry customized facts for this key?
unsafe fn own_descriptor_may_cover_key(addr: usize, key: f64) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut sso,
    ) else {
        return true;
    };
    let DescriptorRoute::Keys(bag) = descriptor_route(addr);
    !bag.is_null() && super::key_attrs::object_key_entry(bag, bytes) != 0
}

include!("descriptor_state/skip_key.rs");

/// Can anything on the prototype chain of a CLASS-LESS receiver whose
/// `[[Prototype]]` was set explicitly (`new F()`, `Object.create`,
/// `setPrototypeOf`) intercept a plain data write of `key` (#10287)?
///
/// The class-instance twin is [`class_instance_set_may_intercept`]; this is the
/// same per-prototype walk without the class-chain probe for the receiver
/// itself. Without it, EVERY function-constructed receiver — which is what
/// zod's `$constructor` mints — was rejected wholesale by
/// [`plain_data_write_may_intercept`], so no store fast path could serve it.
///
/// Conservative in every uncertain case (returns `true` = take the slow path):
/// proxies and handle prototypes, non-object prototypes (functions, arrays),
/// typed arrays, exotic expando hosts, class objects, native-module receivers,
/// frozen prototypes, and any chain deeper than [`CUSTOM_PROTO_WALK_LIMIT`].
pub(crate) unsafe fn plain_custom_prototype_may_intercept(obj_addr: usize, key: f64) -> bool {
    const CUSTOM_PROTO_WALK_LIMIT: u32 = 8;
    let mut name_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(name_bytes) = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut name_buf,
    ) else {
        return true;
    };
    let Ok(name) = std::str::from_utf8(name_bytes) else {
        return true;
    };
    // `Object.prototype`'s Annex-B accessor is implemented in the walk itself,
    // never as a materialized descriptor (see `object_proto_may_intercept_key`).
    if name == "__proto__" {
        return true;
    }
    let mut proto = js_object_get_prototype_of(crate::value::js_nanbox_pointer(obj_addr as i64));
    let mut depth = 0u32;
    loop {
        depth += 1;
        if depth > CUSTOM_PROTO_WALK_LIMIT {
            return true;
        }
        let bits = proto.to_bits();
        let top16 = bits >> 48;
        // Same classification as the class-instance walk: a NaN-boxed small
        // handle is a Proxy (trap), a raw pointer is a recorded literal
        // prototype, null/undefined ends the chain.
        let p = if top16 == 0x7FFD {
            let p = (bits & crate::value::POINTER_MASK) as usize;
            if p == 0 {
                return false;
            }
            if crate::value::addr_class::is_small_handle(p) {
                return true;
            }
            p
        } else if top16 == 0 && bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            bits as usize
        } else if bits == crate::value::TAG_NULL || bits == crate::value::TAG_UNDEFINED {
            return false;
        } else {
            return true;
        };
        if crate::array::object_prototype_addr_matches(p) {
            return object_proto_may_intercept_key(key);
        }
        let Some(header) = crate::value::addr_class::try_read_gc_header(p) else {
            return true;
        };
        // A non-object prototype (function, array, …) may carry semantics this
        // walk does not model; a frozen one makes every inherited data property
        // non-writable whether or not a per-key entry records it.
        if header.obj_type != crate::gc::GC_TYPE_OBJECT
            || header._reserved
                & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
                != 0
        {
            return true;
        }
        if crate::typedarray::lookup_typed_array_kind(p).is_some() {
            return true;
        }
        let proto_value = crate::value::js_nanbox_pointer(p as i64);
        if super::exotic_expando::exotic_expando_kind_of_value(proto_value).is_some() {
            return true;
        }
        let proto_obj = p as *mut crate::object::ObjectHeader;
        if class_registry::is_class_object_ptr(proto_obj.cast()) {
            return true;
        }
        let class_id = (*proto_obj).class_id;
        if class_id == crate::object::NATIVE_MODULE_CLASS_ID {
            return true;
        }
        if super::key_attrs::attrs_live_in_keys(p) {
            if super::key_attrs::object_key_blocks_plain_store(proto_obj, name.as_bytes()) {
                return true;
            }
        } else if object_has_descriptors(p) {
            if get_accessor_descriptor(p, name).is_some() {
                return true;
            }
            if let Some(attrs) = get_property_attrs(p, name) {
                if !attrs.writable() {
                    return true;
                }
            }
        }
        proto = js_object_get_prototype_of(proto);
    }
}

/// Can this holder shape carry an accessor or customized attribute entry?
pub(crate) fn owner_may_have_descriptor_entries(owner: usize, accessor: bool) -> bool {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(owner);
        if bag.is_null() {
            return false;
        }
        let summary = super::key_attrs::object_summary(bag);
        if accessor {
            summary & super::key_attrs::SUMMARY_ACCESSOR != 0
        } else {
            summary != 0
        }
    }
}

/// #6084 (item 6): can anything intercept a plain-data write of `key` to the
/// `GC_TYPE_OBJECT` at `addr` (own accessor / non-writable descriptor, or an
/// inherited setter / non-writable data property), so the dynamic-write
/// transition-cache fast path must be skipped for THIS write?
///
/// Replaces the process-global `GLOBAL_DESCRIPTORS_IN_USE` latch that used to
/// gate both dynamic-write fast paths. That latch flips on *any* descriptor
/// install anywhere — so a single `Object.freeze` on a completely unrelated
/// object (or any library that freezes one config object at import time)
/// permanently pushed EVERY dynamic property write in the process onto the
/// O(own-key-count) slow walk. Measured: 1M objects × 3 new props = 5281 ms;
/// the identical loop after one unrelated `Object.freeze` = 6807 ms (+29%,
/// and it never recovers).
///
/// The vetting here is the same predicate `ordinary_set`'s #5054 fast path
/// (`proxy.rs`) already applies per receiver, and the same receiver-level /
/// prototype-level split as the #5654 read-side guard:
///   - own descriptors are visible per-object in `OBJ_FLAG_HAS_DESCRIPTORS`
///     (set by [`note_descriptor_target`], travels with the object on
///     evacuation, and is clear on every fresh allocation);
///   - only *prototype*-level installs can intercept a write to an object whose
///     own flag is clear, and those are checked against the actual prototype
///     chain — `Object.prototype` per-key via [`object_proto_may_intercept_key`]
///     (a blanket check made wide dynamic builds O(n²), see #5054), a recorded
///     `setPrototypeOf` target, or the class chain via
///     [`class_instance_set_may_intercept`].
///
/// Conservative in every uncertain case (returns `true` = take the slow path).
/// `caller` must have already established that `addr` is a `GC_TYPE_OBJECT`
/// whose frozen/sealed/non-extensible flags are clear.
pub(crate) unsafe fn plain_data_write_may_intercept(addr: usize, class_id: u32, key: f64) -> bool {
    // The own holder shape first proves whether this key is ordinary data.
    if object_has_descriptors(addr) && !own_descriptors_skip_key(addr, key) {
        return true;
    }

    // `note_descriptor_target` cannot record the per-object flag for typed
    // arrays (small ones are plain-alloc'd without a GcHeader) or for exotic
    // expando hosts, so their descriptors are invisible to the flag check
    // above — never fast-path them once any descriptor exists.
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return true;
    }
    let value = crate::value::js_nanbox_pointer(addr as i64);
    if super::exotic_expando::exotic_expando_kind_of_value(value).is_some() {
        return true;
    }

    if class_id == 0 {
        // Plain object. Its prototype is exactly `Object.prototype` unless a
        // `setPrototypeOf` target was recorded for it — #10287: a recorded
        // prototype is vetted per key instead of rejecting the receiver.
        if super::prototype_chain::object_static_prototype(addr).is_some() {
            plain_custom_prototype_may_intercept(addr, key)
        } else if crate::value::addr_class::try_read_gc_header(addr)
            .is_some_and(|header| header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0)
        {
            // Born with a null `[[Prototype]]` and never relinked: no chain.
            false
        } else {
            object_proto_may_intercept_key(key)
        }
    } else {
        // Class instance: an inherited accessor / non-writable data property
        // anywhere in the chain intercepts the write.
        class_instance_set_may_intercept(addr, class_id, key)
    }
}

/// Store a property descriptor for (obj, key).
pub(crate) fn set_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    let byte_edit = HolderEdit::new(obj);
    let obj = byte_edit.as_ref().map_or(obj, |edit| edit.bag as usize);
    crate::typedarray_named::note_named_mutation(obj, key.as_bytes());
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    note_data_descriptor_target(obj, &key, attrs);
    invalidate_prototype_descriptor_guards(obj, &key);
    // Charter step 3: an ordinary object's attributes live with its keys
    // (recorded by the funnel above) and nowhere else.
    if unsafe {
        crate::closure::is_closure_ptr(obj) || super::key_attrs::attrs_live_in_keys_for_install(obj)
    } {
        return;
    }
}

/// Install a group of data descriptors without exposing intermediate states.
/// No JS runs between entries, so one plan invalidation and semantic shape
/// transition retire all prior observations just as repeated installs would.
/// Each key updates the holder shape and the existing prototype guards.
#[cfg(test)]
pub(crate) fn set_property_attrs_batch(obj: usize, entries: &[(&str, PropertyAttrs)]) {
    for (key, _) in entries {
        crate::typedarray_named::note_named_mutation(obj, key.as_bytes());
    }
    if entries.is_empty() {
        return;
    }
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    let edits: Vec<AttrsEdit<'_>> = entries
        .iter()
        .map(|&(key, attrs)| AttrsEdit::Data(key.as_bytes(), attrs.bits))
        .collect();
    note_descriptor_target_edits(obj, &edits);
    for &(key, _) in entries {
        invalidate_prototype_descriptor_guards(obj, key);
    }
}

/// Remove a customized property descriptor for (obj, key), restoring default
/// data-property attributes for subsequent writes and reflection.
pub(crate) fn clear_property_attrs(obj: usize, key: &str) {
    let bag = unsafe { descriptor_holder(obj) };
    if bag.is_null()
        || unsafe { super::key_attrs::object_key_entry(bag, key.as_bytes()) }
            & super::key_attrs::ENTRY_ATTR_MASK
            == 0
    {
        return;
    }
    let edit = HolderEdit::new(obj);
    let obj = edit.as_ref().map_or(obj, |e| e.bag as usize);
    if unsafe { super::key_attrs::object_key_entry(obj as *const ObjectHeader, key.as_bytes()) }
        & super::key_attrs::ENTRY_ATTR_MASK
        != 0
    {
        super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
        note_descriptor_target_edits(obj, &[AttrsEdit::ClearData(key.as_bytes())]);
    }
}

/// Look up the accessor descriptor (get/set) for (obj, key).
pub(crate) fn get_accessor_descriptor(obj: usize, key: &str) -> Option<AccessorDescriptor> {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(obj);
        if bag.is_null() {
            return None;
        }
        own_accessor(bag as usize, key.as_bytes()).map(descriptor_from)
    }
}

/// Descriptor lookups for a native HANDLE owner (`handle_expando`): a
/// small-band id OR a never-freed native `Box` backing such as an
/// `AsyncResource`'s (#10926). Neither is a GC cell, so these probe the tables
/// directly and never consult the per-cell meta summary. That summary reads
/// `owner - 8` as a `GcHeader` behind only a magnitude check, and a `Box`
/// address passes it: the preceding allocator slot's bytes were classified as
/// an object header and a field past the `Box`'s end dereferenced as its
/// `ObjectMeta` -- an intermittent SIGSEGV (or, in a debug build, a
/// "misaligned pointer dereference" of string bytes) on every
/// `resource.eventEmitter` read that happened to sit behind a matching byte.
/// For a small-band id the summary already answered "probe" without reading
/// memory, so the verdicts are unchanged there.
pub(crate) fn get_handle_accessor_descriptor(
    handle: usize,
    key: &str,
) -> Option<AccessorDescriptor> {
    let bag = super::handle_expando::handle_property_bag(handle as i64);
    if bag.is_null() {
        None
    } else {
        unsafe { own_accessor(bag as usize, key.as_bytes()).map(descriptor_from) }
    }
}

/// Handle-owner twin of [`get_property_attrs`]; see
/// [`get_handle_accessor_descriptor`]. No `String`-wrapper index synthesis: a
/// handle is never a boxed string, and that probe reads the owner's header too.
pub(crate) fn get_handle_property_attrs(handle: usize, key: &str) -> Option<PropertyAttrs> {
    let bag = super::handle_expando::handle_property_bag(handle as i64);
    if bag.is_null() {
        None
    } else {
        get_property_attrs(bag as usize, key)
    }
}

/// Does `owner` hold ANY property (data) descriptor?
///
/// Read the holder shape summary to decide whether enumeration needs
/// per-index attribute checks. No owner index or descriptor-map scan exists.
pub(crate) fn owner_has_property_descriptors(owner: usize) -> bool {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(owner);
        !bag.is_null()
            && super::key_attrs::object_summary(bag)
                & (super::key_attrs::SUMMARY_NON_WRITABLE
                    | super::key_attrs::SUMMARY_NON_ENUMERABLE
                    | super::key_attrs::SUMMARY_NON_CONFIGURABLE)
                != 0
    }
}

pub(crate) fn accessor_descriptor_keys_for_obj(obj: usize) -> Vec<String> {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(obj);
        if bag.is_null() {
            Vec::new()
        } else {
            super::key_attrs::object_accessor_key_names(bag)
        }
    }
}

/// #2766: resolve an accessor *getter* closure for `(value, key)` if one is
/// installed (e.g. an object-literal `get x() {…}` or
/// `Object.defineProperty(obj, k, { get })`). Returns the NaN-boxed getter
/// closure bits, or `0` when no getter exists. Used by `Reflect.get(target,
/// key, receiver)` so it can rebind the getter's `this` to the receiver before
/// invoking it. Returns `None` (rather than reading the field) when there is no
/// accessor at all, so the caller falls back to an ordinary field read.
pub(crate) fn reflect_getter_closure_bits(value: f64, key: f64) -> Option<u64> {
    // Builtin and user accessors share the holder shape and pair slot.
    // #6943: `js_string_coerce` allocates for every non-heap-string key and can
    // run a user `toString` / `valueOf` for an object key, so it can trigger a
    // GC that **evacuates**. `value` (the prototype-chain walk's starting
    // receiver, dereferenced by `extract_obj_ptr` below) and `key` (re-read at
    // the own-property shadow check inside the loop) were raw Rust locals
    // across it. Both stay rooted for the walk.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_heap_word_u64(value.to_bits());
    let key_handle = scope.root_nanbox_f64(key);
    let key_str = crate::builtins::js_string_coerce(key_handle.get_nanbox_f64());
    let value = f64::from_bits(value_handle.get_heap_word_u64());
    if key_str.is_null() {
        return None;
    }
    let name = unsafe {
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s.to_string(),
            Err(_) => return None,
        }
    };
    // Spec [[Get]] walks the prototype chain: `Reflect.get(target, key,
    // receiver)` must locate an accessor *getter* installed anywhere on
    // `target`'s chain (an inherited `get x() {…}`), so the caller can rebind
    // its `this` to the receiver before invoking it. An own *data* property at
    // some level shadows inherited accessors, so stop the walk there and let
    // the caller fall back to an ordinary (receiver-aware) field read. (test262
    // Reflect/get/return-value-from-receiver: inherited-getter-via-receiver.)
    // `current` walks the chain through its own handle: `obj_value_has_own_key`
    // and `js_object_get_prototype_of` both allocate, so the link a raw local
    // held could be evacuated out from under the next iteration (#6943).
    let current_handle = scope.root_heap_word_u64(value.to_bits());
    // Bounded to guard against a cyclic prototype side-table; real chains are
    // a handful of links deep.
    for _ in 0..10_000 {
        let current = f64::from_bits(current_handle.get_heap_word_u64());
        let obj = unsafe { extract_obj_ptr(current) };
        if obj.is_null() {
            return None;
        }
        if let Some(acc) = get_accessor_descriptor(obj as usize, &name) {
            return if acc.get != 0 {
                Some(acc.get)
            } else {
                // Accessor exists but has no getter → reading yields undefined;
                // signal that via 0 so the caller returns undefined rather than
                // a field read.
                Some(0)
            };
        }
        // An own (data) property at this level shadows any inherited accessor.
        if obj_value_has_own_key(current, key_handle.get_nanbox_f64()) {
            return None;
        }
        let current = f64::from_bits(current_handle.get_heap_word_u64());
        let proto = crate::object::js_object_get_prototype_of(current);
        if unsafe { extract_obj_ptr(proto) }.is_null() {
            return None;
        }
        current_handle.set_heap_word_u64(proto.to_bits());
    }
    None
}

/// `JSON.stringify` helper: if the own key `key_f64` on `obj` is an accessor
/// property, invoke its getter (with `obj` as the `this` receiver) and return
/// the result bits; `None` when there is no own accessor (caller falls back to
/// the data-field slot). An accessor with no getter reads as `undefined`, which
/// `JSON.stringify` then omits. Node serializes a getter's *return value*, not
/// the stored slot (which holds the getter closure or an empty placeholder).
/// Callers gate this on `descriptors_in_use()`.
pub(crate) unsafe fn json_object_getter_value(
    obj: *const ObjectHeader,
    key_f64: f64,
) -> Option<f64> {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let kb = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key_f64.to_bits()),
        &mut sso,
    )?;
    let name = std::str::from_utf8(kb).ok()?;
    let acc = get_accessor_descriptor(obj as usize, name)?;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if acc.get == 0 {
        return Some(f64::from_bits(TAG_UNDEFINED));
    }
    let closure = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure.is_null() {
        return Some(f64::from_bits(TAG_UNDEFINED));
    }
    let receiver = crate::value::js_nanbox_pointer(obj as i64);
    Some(crate::closure::js_closure_call0(
        closure,
        crate::closure::JsThis::from_f64(receiver),
    ))
}

/// A per-key holder shape query; no owner index or string allocation.
pub(crate) unsafe fn owner_key_is_accessor(owner: usize, key: &[u8]) -> bool {
    let DescriptorRoute::Keys(bag) = descriptor_route(owner);
    !bag.is_null() && super::key_attrs::object_key_is_accessor(bag, key)
}

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    crate::typedarray_named::note_named_mutation(obj, key.as_bytes());
    crate::closure::shape::note_function_own_state_changed(obj);
    let function = HolderEdit::new(obj);
    let obj = function.as_ref().map_or(obj, |edit| edit.bag as usize);
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    let in_keys = unsafe { super::key_attrs::attrs_live_in_keys_for_install(obj) };
    let previous = if in_keys {
        unsafe { own_accessor(obj, key.as_bytes()) }
    } else {
        None
    };
    note_accessor_descriptor_target(obj, &key, &acc);
    invalidate_prototype_descriptor_guards(obj, &key);
    if in_keys {
        // Charter step 3: the pair lives in the key's slot.
        unsafe { store_own_accessor(obj, &key, Some(pair_from(&acc))) };
        note_accessor_function_replaced(obj, &key, previous.map(descriptor_from), acc);
        return;
    }
}

/// RULE 1 when an accessor's getter or setter is REPLACED under unchanged
/// attributes: its keys (and so its ShapeId) did not change, but a cache
/// keyed on the ShapeId may hold the old function.
fn note_accessor_function_replaced(
    obj: usize,
    key: &str,
    previous: Option<AccessorDescriptor>,
    now: AccessorDescriptor,
) {
    let Some(previous) = previous else {
        return;
    };
    if previous.get == now.get && previous.set == now.set {
        return;
    }
    unsafe {
        if super::key_attrs::attrs_live_in_keys_for_install(obj) {
            let _no_move = crate::gc::GcSuppressScope::new();
            crate::object::shapes::transition_object_shape_accessor_replaced(
                obj as *mut ObjectHeader,
                key.as_bytes(),
            );
        }
    }
}

/// Install a new accessor and its attributes in one holder-shape edit.
/// The getter and setter values occupy the key's ordinary value slot.
pub(crate) fn install_fresh_accessor_property(
    obj: usize,
    key: String,
    acc: AccessorDescriptor,
    attrs: PropertyAttrs,
) {
    crate::closure::shape::note_function_own_state_changed(obj);
    let function = HolderEdit::new(obj);
    let obj = function.as_ref().map_or(obj, |edit| edit.bag as usize);
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    // One edit covers the pair: the keys record both halves, and a key that
    // is not yet own arrives WITH them (one trie edge; an in-place append on
    // the tip of an attribute backing).
    note_descriptor_target_edits(
        obj,
        &[
            AttrsEdit::Accessor(key.as_bytes(), acc.get != 0, acc.set != 0),
            AttrsEdit::Data(key.as_bytes(), attrs.bits),
        ],
    );
    let in_keys = unsafe { super::key_attrs::attrs_live_in_keys_for_install(obj) };
    invalidate_prototype_descriptor_guards(obj, &key);
    if in_keys {
        // The pair lives in the key's ordinary value slot.
        unsafe { store_own_accessor(obj, &key, Some(pair_from(&acc))) };
        return;
    }
}

/// Remove an accessor descriptor for (obj, key), letting ordinary data-property
/// reads and writes use the object's stored field again.
pub(crate) fn clear_accessor_descriptor(obj: usize, key: &str) {
    let bag = unsafe { descriptor_holder(obj) };
    if bag.is_null()
        || unsafe { super::key_attrs::object_key_entry(bag, key.as_bytes()) }
            & super::key_attrs::ENTRY_ACCESSOR
            == 0
    {
        return;
    }
    let edit = HolderEdit::new(obj);
    let obj = edit.as_ref().map_or(obj, |e| e.bag as usize);
    if unsafe {
        super::key_attrs::object_key_is_accessor(obj as *const ObjectHeader, key.as_bytes())
    } {
        super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
        note_descriptor_target_edits(obj, &[AttrsEdit::ClearAccessor(key.as_bytes())]);
        unsafe { store_own_accessor(obj, key, None) };
    }
}

/// Install a built-in accessor using the same holder shape and slots as
/// user-defined accessors. Reflection and ordinary access read those facts.
pub(crate) fn set_builtin_accessor_descriptor(
    obj: usize,
    key: String,
    acc: AccessorDescriptor,
    attrs: PropertyAttrs,
) {
    set_builtin_accessor_pair(obj, key, pair_from(&acc), attrs);
}

/// [`set_builtin_accessor_descriptor`] with the whole pair, so a class
/// accessor's compiled entries travel with its closures (S2).
pub(crate) fn set_builtin_accessor_pair(
    obj: usize,
    key: String,
    pair: super::accessor_pair::Accessor,
    attrs: PropertyAttrs,
) {
    crate::closure::shape::note_function_own_state_changed(obj);
    let function = HolderEdit::new(obj);
    let obj = function.as_ref().map_or(obj, |edit| edit.bag as usize);
    let acc = descriptor_from(pair);
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    let in_keys = unsafe { super::key_attrs::attrs_live_in_keys_for_install(obj) };
    let previous = if in_keys {
        unsafe { own_accessor(obj, key.as_bytes()) }
    } else {
        None
    };
    note_descriptor_target_edits(
        obj,
        &[
            AttrsEdit::Accessor(key.as_bytes(), acc.get != 0, acc.set != 0),
            AttrsEdit::Data(key.as_bytes(), attrs.bits),
        ],
    );
    if in_keys {
        // Charter step 3: the pair lives in the key's slot.
        unsafe { store_own_accessor(obj, &key, Some(pair)) };
        note_accessor_function_replaced(obj, &key, previous.map(descriptor_from), acc);
        return;
    }
}

/// Record a built-in data property's attributes in its holder shape.
pub(crate) fn set_builtin_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    // A function born with this key's attributes (its bag's key entry
    // already says so): nothing changes.
    if unsafe { function_key_has_attrs(obj, &key, attrs) } {
        return;
    }
    super::prop_plan::prop_plan_epoch_bump_for_owner(obj);
    note_descriptor_target_edits(obj, &[AttrsEdit::Data(key.as_bytes(), attrs.bits)]);
    if unsafe {
        crate::closure::is_closure_ptr(obj) || super::key_attrs::attrs_live_in_keys_for_install(obj)
    } {
        return;
    }
}

/// Does the function `obj` own data key `key` with exactly the non-default
/// attributes `attrs`, per its bag's key entry?
///
/// # Safety
/// `obj` is any address; only a proven closure's bag is read.
unsafe fn function_key_has_attrs(obj: usize, key: &str, attrs: PropertyAttrs) -> bool {
    let entry = super::key_attrs::attr_bits_to_entry(attrs.bits);
    if entry == 0 || !crate::closure::is_closure_ptr(obj) {
        return false;
    }
    let bag = crate::closure::props::bag_of(obj);
    !bag.is_null()
        && super::key_attrs::object_key_entry(bag, key.as_bytes()) == entry
        && crate::closure::props::bag_has_own(obj, key.as_bytes())
}

/// Install a built-in data property WITH its attributes: `obj[key] = value`
/// followed by [`set_builtin_property_attrs`], as one step. On an object
/// whose attributes live with its keys (charter step 3) the key is claimed
/// with its attributes before the value is stored, so a prototype or
/// namespace gaining one method after another appends each key in place —
/// set-then-rewrite would copy the object's key list once per member.
/// A function claims the key in its bag the same way, in one step
/// (`closure_define_data_with_attrs`); every other owner takes the two-call
/// path.
///
/// The claim allocates; callers hold raw receivers across builtin installs,
/// so it runs in a no-move window.
pub(crate) fn define_builtin_data_property(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    name: String,
    attrs: PropertyAttrs,
) {
    unsafe {
        if crate::closure::is_closure_ptr(obj as usize) {
            super::own_override::as_builtin_definition(|| {
                super::own_override::note_exotic_named_prop_install(obj as usize);
                crate::closure::closure_define_data_with_attrs(obj as usize, &name, value, attrs);
            });
            return;
        } else if super::key_attrs::attrs_live_in_keys_for_install(obj as usize) {
            let _no_move = crate::gc::GcSuppressScope::new();
            let entry = super::key_attrs::attr_bits_to_entry(attrs.bits);
            if entry != 0 {
                super::object_ops::ensure_key_in_keys_array_with_entry(obj, key, entry);
            }
            super::object_ops::define_property_force_store_value(obj, key, value);
        } else {
            super::own_override::as_builtin_definition(|| {
                js_object_set_field_by_name(obj, key, value);
            });
        }
    }
    set_builtin_property_attrs(obj as usize, name, attrs);
}

mod integrity;
pub(crate) use integrity::mark_all_keys;

/// Names from the same holder keys used by descriptor queries.
pub(crate) unsafe fn holder_key_names(bag: *const ObjectHeader, enumerable: bool) -> Vec<String> {
    if bag.is_null() {
        return Vec::new();
    }
    let keys = super::object_keys(bag);
    let mut result = Vec::new();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..keys.count() {
        let entry = super::key_attrs::keys_entry(keys.arr(), i);
        if super::key_attrs::entry_is_private(entry)
            || (enumerable && entry & super::key_attrs::ENTRY_NON_ENUMERABLE != 0)
        {
            continue;
        }
        if let Some(bytes) = crate::string::js_string_key_bytes(keys.get(i), &mut sso) {
            if let Ok(name) = std::str::from_utf8(bytes) {
                result.push(name.to_owned());
            }
        }
    }
    result
}

/// Bulk reset uses the same shape and slot edits as an individual redefine.
#[cfg(test)]
pub(crate) fn clear_object_descriptors(obj: usize) {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(obj);
        if bag.is_null() {
            return;
        }
        for key in super::key_attrs::object_accessor_key_names(bag) {
            clear_accessor_descriptor(bag as usize, &key);
        }
        note_descriptor_target_edits(bag as usize, &[AttrsEdit::ClearAll]);
    }
}

/// The ordinary holder reached by a value's own-property storage edge.
#[inline]
pub(crate) unsafe fn descriptor_holder(owner: usize) -> *mut ObjectHeader {
    let DescriptorRoute::Keys(bag) = descriptor_route(owner);
    bag as *mut ObjectHeader
}
