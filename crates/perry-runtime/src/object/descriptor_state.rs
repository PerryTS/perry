//! Property / accessor descriptor side-tables and the process-wide hot-path
//! gates that guard them (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use crate::fast_hash::{new_fast_key_hash_map, FastKeyHashMap};
use crate::state::state;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Per-property attribute flags set by `Object.defineProperty` / `Object.freeze` / `Object.seal`.
/// Tracks the JS PropertyDescriptor attributes (writable, enumerable, configurable) for keys
/// that have been customized away from the default `{ writable: true, enumerable: true, configurable: true }`.
/// Keyed by (obj_ptr as usize, key_string) -> attribute bitmask.
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

/// #6759 Phase A: the descriptor side tables and their per-thread fast-path
/// gates, grouped as the `descriptors` field of
/// [`crate::state::RuntimeState`]. Previously four separate `thread_local!`s;
/// reach them via `crate::state::state().descriptors` (one TLS fetch for the
/// whole group).
pub(crate) struct DescriptorTables {
    /// Per-property attribute flags set by `Object.defineProperty` /
    /// `Object.freeze` / `Object.seal`, keyed `(owner_addr, key_string)`.
    ///
    /// Hasher: `FastKeyHasher` (FNV-1a) rather than std's SipHash
    /// `RandomState`. The key is a runtime heap pointer plus a
    /// program-supplied property name, so no external input reaches it and
    /// DoS-resistant hashing buys nothing on this hot property-access path.
    pub(crate) property_descriptors: RefCell<FastKeyHashMap<(usize, String), PropertyAttrs>>,
    /// Accessor descriptor storage: maps `(owner_addr, key_string)` to the
    /// getter/setter closure bits. Same hasher rationale as
    /// `property_descriptors`.
    pub(crate) accessor_descriptors: RefCell<FastKeyHashMap<(usize, String), AccessorDescriptor>>,
    /// Fast-path gate: `false` when no accessor descriptors have ever been
    /// installed on this thread, so hot `js_object_get_field_by_name` /
    /// `set_field_by_name` can skip the `accessor_descriptors` HashMap
    /// lookup entirely.
    pub(crate) accessors_in_use: Cell<bool>,
    /// Fast-path gate for `property_descriptors` — flipped the first time
    /// `Object.defineProperty` (or freeze/seal via `set_property_attrs`)
    /// installs a per-property descriptor. Lets the hot object-write path
    /// skip the `.to_string()` allocation required to look up a descriptor
    /// that almost never exists.
    pub(crate) property_attrs_in_use: Cell<bool>,
    /// Owner index: `owner_addr -> that owner's descriptor keys`, mirroring
    /// the two `(owner, key)`-keyed maps above.
    ///
    /// The maps stay authoritative; these only answer "which keys does THIS
    /// owner have?" without walking every entry in the process. Before this
    /// index, that question was answered by
    /// `map.keys().filter(|(owner, _)| *owner == obj)` — an O(total
    /// descriptors in the program) scan — from three places that run
    /// constantly:
    ///
    ///   * `accessor_descriptor_keys_for_obj`, on the `Object.keys` /
    ///     `getOwnPropertyNames` / `for…in` own-key path;
    ///   * `transfer_descriptor_owner`, on every `ArrayHeader` growth;
    ///   * `scan_descriptor_roots_mut`, on **every GC cycle**.
    ///
    /// Measured cost of the scan (`Object.keys` × 20 000 on a 4-key object,
    /// while unrelated objects hold N descriptors): 26 ms at N=0 rising to
    /// 1628 ms at N=16 000, against a flat 1-3 ms for node — i.e. the cost of
    /// touching one small object grew with descriptors it has nothing to do
    /// with. Profiling `claude -p` put 46.6% of main-thread samples in
    /// shapes/descriptors, with this scan the single hottest entry by 4×.
    ///
    /// `owner_may_have_descriptor_entries` (the per-object `attr_key_bits` /
    /// `accessor_key_bits` Bloom summary) already skipped the scan for owners
    /// with *no* descriptors, which is why this was survivable — but it fails
    /// open for a non-meta-capable owner, and any owner with a single
    /// descriptor paid the full walk.
    pub(crate) attr_keys_by_owner: RefCell<FastKeyHashMap<usize, Vec<String>>>,
    /// Accessor twin of [`Self::attr_keys_by_owner`].
    pub(crate) accessor_keys_by_owner: RefCell<FastKeyHashMap<usize, Vec<String>>>,
}

impl DescriptorTables {
    pub(crate) fn new() -> Self {
        DescriptorTables {
            property_descriptors: RefCell::new(new_fast_key_hash_map()),
            accessor_descriptors: RefCell::new(new_fast_key_hash_map()),
            accessors_in_use: Cell::new(false),
            property_attrs_in_use: Cell::new(false),
            attr_keys_by_owner: RefCell::new(new_fast_key_hash_map()),
            accessor_keys_by_owner: RefCell::new(new_fast_key_hash_map()),
        }
    }
}

/// Record `key` as owned by `owner` in an owner index. Idempotent: a
/// `defineProperty` that overwrites an existing descriptor must not push a
/// duplicate, or the key would be reported twice by `Object.keys`.
fn owner_index_add(index: &RefCell<FastKeyHashMap<usize, Vec<String>>>, owner: usize, key: &str) {
    let mut idx = index.borrow_mut();
    let keys = idx.entry(owner).or_default();
    if !keys.iter().any(|k| k == key) {
        keys.push(key.to_string());
    }
}

/// Drop `key` from `owner`'s index entry, removing the entry entirely once it
/// is empty so a dead owner leaves nothing behind for the GC scan to walk.
fn owner_index_remove(index: &RefCell<FastKeyHashMap<usize, Vec<String>>>, owner: usize, key: &str) {
    let mut idx = index.borrow_mut();
    if let Some(keys) = idx.get_mut(&owner) {
        keys.retain(|k| k != key);
        if keys.is_empty() {
            idx.remove(&owner);
        }
    }
}

/// Move an owner's whole index entry to a new address (array growth, GC
/// evacuation). Merges into any entry already at `new_owner` rather than
/// clobbering it — an address can be recycled by a live tenant.
fn owner_index_transfer(
    index: &RefCell<FastKeyHashMap<usize, Vec<String>>>,
    old_owner: usize,
    new_owner: usize,
) {
    let mut idx = index.borrow_mut();
    let Some(moved) = idx.remove(&old_owner) else {
        return;
    };
    let dest = idx.entry(new_owner).or_default();
    for k in moved {
        if !dest.iter().any(|existing| *existing == k) {
            dest.push(k);
        }
    }
}

/// Accessor descriptor storage: maps (obj_ptr, key) -> (get_closure_bits, set_closure_bits).
/// A zero bits value means "no getter" or "no setter". Entries here represent properties
/// installed via `Object.defineProperty(obj, key, { get, set })` — those must route reads
/// through the getter closure and writes through the setter closure instead of touching
/// the underlying field slot.
#[derive(Clone, Copy, Default)]
pub(crate) struct AccessorDescriptor {
    pub get: u64, // NaN-boxed closure f64 bits, 0 = absent
    pub set: u64, // NaN-boxed closure f64 bits, 0 = absent
}

/// Global monotonic flag: set once any accessor or property descriptor is
/// installed.  Checked on every dynamic property write via a single
/// `Relaxed` load (no TLS overhead, no fence on aarch64/x86).
pub(crate) static GLOBAL_DESCRIPTORS_IN_USE: AtomicBool = AtomicBool::new(false);

/// Has any property descriptor or accessor ever been installed in this
/// process? Used by inspect/format code paths to skip per-key
/// descriptor lookups on objects whose enumerability hasn't been
/// touched (the common case). Relaxed load is fine — false positives
/// are harmless (just an extra HashMap lookup) and false negatives
/// can't happen because the store happens before the property is
/// observable.
pub(crate) fn descriptors_in_use() -> bool {
    GLOBAL_DESCRIPTORS_IN_USE.load(Ordering::Relaxed)
}

/// #5093: sticky process-global that disables the codegen-inlined class-field
/// shape-guard fast path. The emitted IR reads this byte directly (a single
/// relaxed load, hoistable out of hot loops) via the
/// `@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` symbol and falls back to the full
/// `js_typed_feedback_class_field_{get,set}_guard` call whenever it is non-zero.
/// It flips to 1 the moment either (a) an accessor / property descriptor is
/// installed on an object the inline path cannot vet per-receiver — a
/// registered class prototype or the canonical `Object.prototype` (#5654;
/// receiver-level descriptors are instead rejected by the emitted
/// `OBJ_FLAG_HAS_DESCRIPTORS` check, so they don't poison the process) — or
/// (b) typed-feedback tracing is enabled, where the guard records observations
/// the inline path would silently skip. Both are monotonic ("in use" never
/// reverts), so the flag is set-only.
#[no_mangle]
pub static PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED: AtomicU8 = AtomicU8::new(0);

/// Disable the codegen-inlined class-field fast path process-wide (see
/// [`PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`]). Idempotent.
pub(crate) fn disable_class_field_inline_guard() {
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.store(1, Ordering::Relaxed);
}

/// True when the inline class-field fast path is still permitted.
pub(crate) fn class_field_inline_guard_enabled() -> bool {
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.load(Ordering::Relaxed) == 0
}

#[cfg(test)]
pub(crate) fn test_reset_class_field_inline_guard() {
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.store(0, Ordering::Relaxed);
    // Also clear the C5a per-key vetting sets (production-monotonic, so
    // without this a key name reused across tests in one process would
    // inherit an earlier test's declared-field / installed-key state and
    // make the disable decision order-dependent (CodeRabbit on #6802).
    if let Ok(mut guard) = DECLARED_FIELD_NAME_HASHES.write() {
        guard.take();
    }
    if let Ok(mut guard) = PROTO_DESCRIPTOR_KEY_HASHES.write() {
        guard.take();
    }
}

/// #5654: flip the process-wide inline gate only when the descriptor target can
/// intercept a `this.field` access that the inline precheck cannot reject on
/// its own. Receiver-level installs are visible to the precheck via
/// `OBJ_FLAG_HAS_DESCRIPTORS` in the receiver's GcHeader (set by
/// [`note_descriptor_target`], checked by the emitted IR), so only
/// prototype-level targets still need the global disable:
///   - a class prototype — either the reflective decl-prototype object that
///     `C.prototype` materializes (`CLASS_DECL_PROTOTYPE_OBJECTS`) or a
///     synthetic `function Base() {}; Base.prototype = obj` prototype
///     (`CLASS_PROTOTYPE_OBJECTS`) — intercepts `this.field` on every instance
///     of that class, which the per-receiver flag cannot see;
///   - the canonical `Object.prototype` sits at the tail of every instance's
///     chain.
/// Any other target (plain object, array, closure, builtin namespace) never
/// appears in the guard's descriptor checks — those walk the receiver and the
/// class-registry prototype chain only — so unrelated installs (the builtin
/// setup that runs during every program's startup, `Object.freeze` on a config
/// object, …) no longer disable the #5093 fast path process-wide.
///
/// The prototype-registry probes scan by value (O(#classes)); descriptor
/// installs are rare and never on the hot property path, so the scan cost is
/// acceptable.
///
/// #6759 C5a — per-KEY refinement (the follow-up the paragraph above used to
/// promise): the inline fast path only ever compiles accesses to DECLARED
/// instance fields, so a prototype-level install whose key names no declared
/// field of any registered class cannot affect anything the inline path
/// handles — babel-style method installs (`defineProperty(C.prototype,
/// "render", …)`) no longer poison the process. The vetting set holds FNV
/// hashes of every declared field name (harvested by
/// `remember_class_keys_array` at class registration); a collision merely
/// disables — never skips — so it stays conservative. Module-init ordering
/// is covered in both directions: installs that precede a class's
/// registration are recorded in [`PROTO_DESCRIPTOR_KEY_HASHES`] and
/// retro-checked by [`note_declared_instance_field_name`] when the class
/// arrives.
pub(crate) fn disable_inline_guards_for_descriptor_target(obj: usize, key: &str) {
    let is_prototype_target = crate::array::object_prototype_addr_matches(obj)
        || class_registry::is_registered_class_prototype_object(obj)
        || class_registry::class_id_for_decl_prototype_object(obj).is_some();
    if is_prototype_target {
        // A prototype descriptor can only change resolution for this key.
        // Retire the matching method-name guard slot across all classes;
        // own-instance installs are still rejected by the receiver's
        // `OBJ_FLAG_HAS_DESCRIPTORS` header bit.
        class_registry::invalidate_class_prototype_fast_guards_for_method(key);
        let hash = super::key_bytes_hash(key.as_ptr(), key.len());
        note_proto_descriptor_key_hash(hash);
        if declared_field_name_hash_exists(hash) {
            disable_class_field_inline_guard();
        }
    }
}

/// #6759 C5a: FNV hashes of every declared instance-field name across all
/// registered classes. Written at class registration (cold), read at
/// prototype-level descriptor installs (rare). Never pruned — class
/// registrations are process-lifetime.
static DECLARED_FIELD_NAME_HASHES: std::sync::RwLock<Option<std::collections::HashSet<u64>>> =
    std::sync::RwLock::new(None);

/// #6759 C5a: FNV hashes of every key installed on a prototype-level
/// descriptor target, so a class that registers AFTER such an install can
/// retro-trigger the disable (see
/// [`disable_inline_guards_for_descriptor_target`]).
static PROTO_DESCRIPTOR_KEY_HASHES: std::sync::RwLock<Option<std::collections::HashSet<u64>>> =
    std::sync::RwLock::new(None);

fn declared_field_name_hash_exists(hash: u64) -> bool {
    DECLARED_FIELD_NAME_HASHES
        .read()
        .map(|g| g.as_ref().is_some_and(|s| s.contains(&hash)))
        // Lock poisoned: be conservative (disable rather than skip).
        .unwrap_or(true)
}

fn note_proto_descriptor_key_hash(hash: u64) {
    if let Ok(mut guard) = PROTO_DESCRIPTOR_KEY_HASHES.write() {
        guard
            .get_or_insert_with(std::collections::HashSet::new)
            .insert(hash);
    } else {
        // Lock poisoned: the retro-check can no longer see this key —
        // take the conservative disable now.
        disable_class_field_inline_guard();
    }
}

/// #6759 C5a: called by `remember_class_keys_array` for each declared
/// instance-field name of a registering class. Records the name hash and
/// retro-checks it against prototype-level descriptor keys installed
/// earlier (which skipped the disable because no class had declared the
/// name yet).
pub(crate) fn note_declared_instance_field_name(name: &[u8]) {
    let hash = super::key_bytes_hash(name.as_ptr(), name.len());
    if let Ok(mut guard) = DECLARED_FIELD_NAME_HASHES.write() {
        guard
            .get_or_insert_with(std::collections::HashSet::new)
            .insert(hash);
    }
    let installed_earlier = PROTO_DESCRIPTOR_KEY_HASHES
        .read()
        .map(|g| g.as_ref().is_some_and(|s| s.contains(&hash)))
        .unwrap_or(true);
    if installed_earlier {
        disable_class_field_inline_guard();
    }
}

/// #5054: a descriptor (any kind) has been installed on the canonical
/// `Object.prototype` — inherited setters / non-writable data props there
/// must intercept writes of keys missing on the receiver, so the dynamic
/// plain-object write fast path is disabled process-wide once this flips.
static OBJECT_PROTO_DESCRIPTORS: AtomicBool = AtomicBool::new(false);

pub(crate) fn object_proto_descriptors_in_use() -> bool {
    OBJECT_PROTO_DESCRIPTORS.load(Ordering::Relaxed)
}

/// True when a write of `key` to a plain object whose prototype is the canonical
/// `Object.prototype` might be intercepted there (inherited setter / non-writable
/// data) and must therefore take the slow [[Set]] walk.
///
/// `OBJECT_PROTO_DESCRIPTORS` only records that *some* descriptor exists on
/// `Object.prototype`; using it directly forced EVERY dynamic write onto the
/// O(own-key-count) slow path, so a single userland `Object.prototype` accessor
/// made any wide-object build O(n²) (a 20k-property build went 16ms → 42s). The
/// fast plain-data write actually only needs the slow path when `Object.prototype`
/// has an own property for THIS key; an absent key cannot be intercepted, so the
/// fast path stays safe even while unrelated descriptors exist on the prototype.
pub(crate) fn object_proto_may_intercept_key(key: f64) -> bool {
    // #6828: `%Object.prototype%` always owns the Annex-B `__proto__`
    // accessor, even though Perry implements that intrinsic in the ordinary
    // [[Set]] walk rather than materializing a closure-backed descriptor.
    // Treat it as an interceptor so the plain-object direct-store lane cannot
    // create an own enumerable `"__proto__"` property before the walk gets a
    // chance to invoke the intrinsic setter.
    if unsafe { reflect_support::key_to_rust_string(key) }.as_deref() == Some("__proto__") {
        return true;
    }
    if !object_proto_descriptors_in_use() {
        return false;
    }
    let proto_addr = crate::array::object_prototype_addr();
    if proto_addr == 0 {
        return false;
    }
    let proto_value =
        f64::from_bits(crate::value::JSValue::pointer(proto_addr as *const u8).bits());
    reflect_support::obj_value_has_own_key(proto_value, key)
}

/// Whether a fast plain-data write of `key` to a CLASS INSTANCE (`class_id != 0`)
/// at `obj_addr` might be intercepted by its prototype chain — i.e. the slow
/// `[[Set]]` walk is required instead of a direct own-data store. Conservative:
/// any uncertainty returns `true` (take the slow path).
///
/// All interception sources are checked so the fast path stays correct:
///   1. A class getter/setter named `key` anywhere in the `extends` chain. These
///      live in the per-class vtable, NOT the address-keyed descriptor tables, so
///      the prototype-object scan in (2) cannot see them.
///   2. An address-keyed accessor / non-writable descriptor on any *class*
///      prototype object (`Object.defineProperty(C.prototype, …)`), detected via
///      `OBJ_FLAG_HAS_DESCRIPTORS` on that prototype object.
///   3. `Object.prototype` at the chain tail — delegated per-key to
///      [`object_proto_may_intercept_key`].
///
/// Own-instance descriptors / frozen / sealed are excluded by the caller before
/// this is reached.
pub(crate) unsafe fn class_instance_set_may_intercept(
    obj_addr: usize,
    class_id: u32,
    key: f64,
) -> bool {
    // Decode the key once — used for both the class-chain and per-prototype
    // accessor probes below.
    let name = match reflect_support::key_to_rust_string(key) {
        Some(n) => n,
        // Non-decodable / non-string key: do not risk the fast path.
        None => return true,
    };
    // (1) A class getter/setter for this exact key anywhere in the class chain.
    if class_registry::class_chain_has_instance_accessor(class_id, &name) {
        return true;
    }
    // (2)/(3) Walk the prototype OBJECTS from the instance's [[Prototype]].
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
        if object_has_descriptors(p) {
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

/// #5054: record descriptor installation on the target object itself —
/// `OBJ_FLAG_HAS_DESCRIPTORS` in its GcHeader (travels with the object on
/// evacuation), plus the `Object.prototype` process-global above. Unlike
/// `GLOBAL_DESCRIPTORS_IN_USE`, neither is poisoned by the runtime
/// installing attrs on unrelated builtins (RegExp prototype etc.), so the
/// dynamic-write fast path stays precise.
/// #6710: set once a native HANDLE-band owner (small id, not a heap object)
/// gets a property-attr / accessor descriptor. Heap owners record this on their
/// GC header (`OBJ_FLAG_HAS_DESCRIPTORS`) but a handle id has no header, so
/// `clear_object_descriptors` uses this flag to skip the O(N) `retain` scans on
/// the common path where no handle was ever `defineProperty`'d.
static HANDLE_HAS_DESCRIPTORS: AtomicBool = AtomicBool::new(false);

pub(crate) fn note_descriptor_target(obj: usize) {
    if crate::value::addr_class::is_handle_band(obj) {
        HANDLE_HAS_DESCRIPTORS.store(true, Ordering::Relaxed);
    }
    if crate::array::object_prototype_addr_matches(obj) {
        OBJECT_PROTO_DESCRIPTORS.store(true, Ordering::Relaxed);
    }
    if crate::typedarray::lookup_typed_array_kind(obj).is_some() {
        return;
    }
    unsafe {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(obj) {
            if header.obj_type == crate::gc::GC_TYPE_OBJECT {
                let header = header as *const crate::gc::GcHeader as *mut crate::gc::GcHeader;
                (*header)._reserved |= crate::gc::OBJ_FLAG_HAS_DESCRIPTORS;
                let object = obj as *mut crate::object::ObjectHeader;
                if crate::object::object_is_shaped(object) {
                    crate::object::shapes::transition_object_shape_semantics(object);
                }
            }
        }
    }
}

/// Look up the property descriptor for (obj, key). Returns None if no entry exists,
/// in which case the JS default `{ writable: true, enumerable: true, configurable: true }` applies.
pub(crate) fn get_property_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    // #6759 Phase C2: the meta-record summary proves most misses without
    // the `String` build + table probe (and shields a fresh object at a
    // recycled address from a dead owner's not-yet-pruned entries).
    if !may_have_descriptor_entry(obj, key, false) {
        return None;
    }
    state()
        .descriptors
        .property_descriptors
        .borrow()
        .get(&(obj, key.to_string()))
        .copied()
}

/// Whether this specific object has ever had a property descriptor installed on
/// it (`OBJ_FLAG_HAS_DESCRIPTORS`, set by [`note_descriptor_target`] for every
/// `PROPERTY_DESCRIPTORS` insertion on a `GC_TYPE_OBJECT`). The flag lives in
/// the GcHeader and travels with the object across evacuation.
///
/// `PROPERTY_DESCRIPTORS` is keyed by raw address, so once a freed object's slot
/// is reused by a fresh object, a stale `(addr, key)` descriptor entry would be
/// read back for the new object — falsely reporting e.g. a `writable: false`
/// `Fragment` on a brand-new `{}` and throwing "Cannot assign to read only
/// property". A fresh allocation's `_reserved` is zeroed, so gating descriptor
/// lookups on this per-object flag avoids the stale-address-reuse false
/// positive (Next.js app-page-turbo runtime's webpack `exports.Fragment = …`).
pub(crate) fn object_has_descriptors(obj: usize) -> bool {
    unsafe {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(obj) {
            return header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0;
        }
    }
    false
}

/// #6759 Phase C2: the summary bit for `key` in the owner's meta-record
/// Bloom words (`ObjectMeta::{attr,accessor}_key_bits`) — same FNV key hash
/// the Phase C1 shape records use. Install and probe must hash identical
/// byte sequences, so both forms go through this one function.
#[inline]
fn descriptor_key_bit_bytes(key: &[u8]) -> u64 {
    1u64 << (super::key_bytes_hash(key.as_ptr(), key.len()) & 63)
}

#[inline]
fn descriptor_key_bit(key: &str) -> u64 {
    descriptor_key_bit_bytes(key.as_bytes())
}

#[cfg(test)]
pub(crate) fn test_descriptor_key_bit(key: &str) -> u64 {
    descriptor_key_bit(key)
}

/// #6759 Phase C2: record `key` in the owner's per-object meta summary so
/// hot-path probes for OTHER keys can skip the descriptor tables. No-op for
/// owners that cannot carry a meta record (handle-band ids, typed arrays,
/// RegExp, non-heap addresses) — probes for those stay conservative.
///
/// Invariant this maintains (relied on by [`may_have_descriptor_entry`]):
/// every insert into `property_descriptors` / `accessor_descriptors` whose
/// owner is meta-capable sets the key's bit first, so for such owners a
/// clear bit — or a still-null meta record — proves the tables hold no
/// entry for that key. The bits travel with the object (the meta record is
/// GC-traced off the header and moves with its owner, exactly when the
/// table entries are rekeyed by `scan_descriptor_roots_mut`), and a fresh
/// object at a recycled address starts meta-null, so stale entries a dead
/// owner left behind can no longer be misread as the new tenant's.
fn note_meta_descriptor_key(owner: usize, key: &str, accessor: bool) {
    unsafe {
        if let Some(obj) = super::prototype_chain::meta_capable_object(owner) {
            // No-move window: `object_meta_ensure` allocates, and a
            // triggered collection could MOVE `owner` — installers
            // (freeze/seal loops, defineProperty) hold raw owner pointers
            // across repeated installs.
            let _no_gc = crate::gc::GcSuppressScope::new();
            let meta = super::object_meta_ensure(obj);
            let bit = descriptor_key_bit(key);
            if accessor {
                (*meta).accessor_key_bits |= bit;
            } else {
                (*meta).attr_key_bits |= bit;
            }
        }
    }
}

/// #6759 Phase C2 per-key fast-path verdict: can the string-keyed
/// descriptor tables hold an entry `(owner, key)`? `false` is
/// authoritative (the probe is skipped); `true` means "probe the table"
/// (a genuine entry, a Bloom collision, or a non-meta-capable owner).
#[inline]
pub(crate) fn may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    unsafe {
        match super::prototype_chain::meta_capable_object(owner) {
            Some(obj) => {
                let meta = (*obj).meta;
                if meta.is_null() {
                    return false;
                }
                let word = if accessor {
                    (*meta).accessor_key_bits
                } else {
                    (*meta).attr_key_bits
                };
                word & descriptor_key_bit(key) != 0
            }
            None => true,
        }
    }
}

/// #6759 Phase C2: can an OWN string-keyed descriptor (attr or accessor)
/// cover the NaN-boxed key `key` on `addr`? Conservative `true` for
/// non-string keys and non-meta-capable owners. Callers pair this with
/// `object_has_descriptors` for the per-key refinement of that flag.
unsafe fn own_descriptor_may_cover_key(addr: usize, key: f64) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(kb) = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut sso,
    ) else {
        return true;
    };
    match super::prototype_chain::meta_capable_object(addr) {
        Some(obj) => {
            let meta = (*obj).meta;
            if meta.is_null() {
                return false;
            }
            let bit = descriptor_key_bit_bytes(kb);
            ((*meta).attr_key_bits | (*meta).accessor_key_bits) & bit != 0
        }
        None => true,
    }
}

/// #6759 Phase C2 owner-level verdict: can the tables hold ANY entry owned
/// by `owner`? Gates the O(table-size) owner scans (`Object.keys` fast
/// path, `accessor_descriptor_keys_for_obj`). Same trust model as the
/// per-key form.
#[inline]
pub(crate) fn owner_may_have_descriptor_entries(owner: usize, accessor: bool) -> bool {
    unsafe {
        match super::prototype_chain::meta_capable_object(owner) {
            Some(obj) => {
                let meta = (*obj).meta;
                if meta.is_null() {
                    return false;
                }
                if accessor {
                    (*meta).accessor_key_bits != 0
                } else {
                    (*meta).attr_key_bits != 0
                }
            }
            None => true,
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
    // Nothing has ever installed a descriptor or accessor: no per-object work at
    // all, just the one relaxed load the old gate did.
    if !descriptors_in_use() {
        return false;
    }

    // A descriptor exists SOMEWHERE. Vet this receiver and its prototype chain
    // instead of latching the whole process onto the slow path.

    // Own accessor / non-writable descriptor on this exact object. #6759
    // Phase C2: the flag is object-level; the meta summary refines it
    // per-KEY, so an object with a descriptor on one key (webpack's
    // `defineProperty(exports, "__esModule", …)`) keeps the fast path for
    // writes to its other keys. A clear pair of bits proves no own
    // string-keyed entry covers THIS key (an own symbol-keyed descriptor
    // cannot intercept a string-keyed write); prototype-level interception
    // is still vetted below.
    if object_has_descriptors(addr) && own_descriptor_may_cover_key(addr, key) {
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
        // `setPrototypeOf` target was recorded for it.
        super::prototype_chain::object_static_prototype(addr).is_some()
            || object_proto_may_intercept_key(key)
    } else {
        // Class instance: an inherited accessor / non-writable data property
        // anywhere in the chain intercepts the write.
        class_instance_set_may_intercept(addr, class_id, key)
    }
}

/// Store a property descriptor for (obj, key).
pub(crate) fn set_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    super::prop_plan::prop_plan_epoch_bump();
    note_descriptor_target(obj);
    let st = state();
    st.descriptors.property_attrs_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_inline_guards_for_descriptor_target(obj, &key);
    note_meta_descriptor_key(obj, &key, false);
    owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .insert((obj, key), attrs);
}

/// Remove a customized property descriptor for (obj, key), restoring default
/// data-property attributes for subsequent writes and reflection.
pub(crate) fn clear_property_attrs(obj: usize, key: &str) {
    let removed = state()
        .descriptors
        .property_descriptors
        .borrow_mut()
        .remove(&(obj, key.to_string()))
        .is_some();
    if !removed {
        return;
    }
    owner_index_remove(&state().descriptors.attr_keys_by_owner, obj, key);
    super::prop_plan::prop_plan_epoch_bump();
    unsafe {
        let object = obj as *mut crate::object::ObjectHeader;
        if crate::object::object_is_shaped(object) {
            crate::object::shapes::transition_object_shape_semantics(object);
        }
    }
}

/// Look up the accessor descriptor (get/set) for (obj, key).
pub(crate) fn get_accessor_descriptor(obj: usize, key: &str) -> Option<AccessorDescriptor> {
    // #6759 Phase C2: see `get_property_attrs`.
    if !may_have_descriptor_entry(obj, key, true) {
        return None;
    }
    state()
        .descriptors
        .accessor_descriptors
        .borrow()
        .get(&(obj, key.to_string()))
        .copied()
}

/// Does `owner` hold ANY property (data) descriptor?
///
/// O(1) via the owner index. Callers on the `Object.keys` / `for…in` array
/// path used to answer this with
/// `property_descriptors.keys().any(|(ptr, _)| *ptr == owner)` — an O(total
/// descriptors in the program) walk, per enumeration, to decide whether a
/// per-index `enumerable` check was needed at all.
pub(crate) fn owner_has_property_descriptors(owner: usize) -> bool {
    // Cheap authoritative "no" first: the per-object Bloom summary.
    if !owner_may_have_descriptor_entries(owner, false) {
        return false;
    }
    state()
        .descriptors
        .attr_keys_by_owner
        .borrow()
        .contains_key(&owner)
}

pub(crate) fn accessor_descriptor_keys_for_obj(obj: usize) -> Vec<String> {
    // #6759 Phase C2: skip the lookup entirely when the owner's meta summary
    // proves it owns no accessor entries.
    if !owner_may_have_descriptor_entries(obj, true) {
        return Vec::new();
    }
    // O(own keys) via the owner index. This used to walk every entry in
    // `accessor_descriptors` filtering on `owner` — O(total descriptors in the
    // program) — on the `Object.keys` / `getOwnPropertyNames` / `for…in` path.
    // See `DescriptorTables::attr_keys_by_owner` for the measurements.
    let mut keys = state()
        .descriptors
        .accessor_keys_by_owner
        .borrow()
        .get(&obj)
        .cloned()
        .unwrap_or_default();
    keys.sort();
    keys
}

/// #2766: resolve an accessor *getter* closure for `(value, key)` if one is
/// installed (e.g. an object-literal `get x() {…}` or
/// `Object.defineProperty(obj, k, { get })`). Returns the NaN-boxed getter
/// closure bits, or `0` when no getter exists. Used by `Reflect.get(target,
/// key, receiver)` so it can rebind the getter's `this` to the receiver before
/// invoking it. Returns `None` (rather than reading the field) when there is no
/// accessor at all, so the caller falls back to an ordinary field read.
pub(crate) fn reflect_getter_closure_bits(value: f64, key: f64) -> Option<u64> {
    if !state().descriptors.accessors_in_use.get() {
        return None;
    }
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
    let prev = js_implicit_this_set(receiver);
    let result = crate::closure::js_closure_call0(closure);
    js_implicit_this_set(prev);
    Some(result)
}

/// Monotonic (#6386): has an accessor descriptor keyed `"constructor"` ever
/// been installed on ANY object? While false, `ArraySpeciesCreate`'s
/// own-`constructor`-accessor probe on a plain array cannot hit, so the
/// species fast path skips the `(addr, String)` descriptor-table lookup (a
/// per-call `String` allocation + SipHash probe). Set (release) before the
/// insert, so a false (acquire) read can't race a completed install.
static CONSTRUCTOR_ACCESSOR_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn constructor_accessor_ever_installed() -> bool {
    CONSTRUCTOR_ACCESSOR_EVER.load(Ordering::Acquire)
}

fn note_accessor_descriptor_key(key: &str) {
    if key == "constructor" {
        CONSTRUCTOR_ACCESSOR_EVER.store(true, Ordering::Release);
    }
}

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    super::prop_plan::prop_plan_epoch_bump();
    note_descriptor_target(obj);
    let st = state();
    st.descriptors.accessors_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_inline_guards_for_descriptor_target(obj, &key);
    note_accessor_descriptor_key(&key);
    note_meta_descriptor_key(obj, &key, true);
    owner_index_add(&st.descriptors.accessor_keys_by_owner, obj, &key);
    st.descriptors
        .accessor_descriptors
        .borrow_mut()
        .insert((obj, key), acc);
}

/// Remove an accessor descriptor for (obj, key), letting ordinary data-property
/// reads and writes use the object's stored field again.
pub(crate) fn clear_accessor_descriptor(obj: usize, key: &str) {
    let removed = state()
        .descriptors
        .accessor_descriptors
        .borrow_mut()
        .remove(&(obj, key.to_string()))
        .is_some();
    if !removed {
        return;
    }
    owner_index_remove(&state().descriptors.accessor_keys_by_owner, obj, key);
    super::prop_plan::prop_plan_epoch_bump();
    unsafe {
        let object = obj as *mut crate::object::ObjectHeader;
        if crate::object::object_is_shaped(object) {
            crate::object::shapes::transition_object_shape_semantics(object);
        }
    }
}

/// Install a built-in *reflection-only* accessor descriptor for (obj, key)
/// WITHOUT flipping the process-wide `GLOBAL_DESCRIPTORS_IN_USE` /
/// `ACCESSORS_IN_USE` / `PROPERTY_ATTRS_IN_USE` hot-path gates.
///
/// `Object.getOwnPropertyDescriptor` reads `ACCESSOR_DESCRIPTORS` and
/// `PROPERTY_DESCRIPTORS` *unconditionally*, so the descriptor is fully
/// reflectable. The owning object's `OBJ_FLAG_HAS_DESCRIPTORS` bit lets direct
/// reads/writes consult the side tables without flipping a process-wide gate;
/// unrelated objects keep skipping the HashMap lookup.
/// This matters because built-in prototype accessors such as
/// `%TypedArray%.prototype.length` are installed lazily at globalThis
/// init for *every* program that merely touches a builtin global; flipping
/// the gate there would slow the property-write fast path process-wide for
/// no behavioral gain (these accessors have no setter and are never written
/// in real workloads — they exist purely so reflection sees them). See #2060.
pub(crate) fn set_builtin_accessor_descriptor(
    obj: usize,
    key: String,
    acc: AccessorDescriptor,
    attrs: PropertyAttrs,
) {
    super::prop_plan::prop_plan_epoch_bump();
    note_descriptor_target(obj);
    note_accessor_descriptor_key(&key);
    // #6759 Phase C2: the meta summary must over-approximate the tables
    // even for gate-neutral builtin installs — the (unconditionally
    // consulted) reflection reads now trust a clear bit.
    note_meta_descriptor_key(obj, &key, true);
    note_meta_descriptor_key(obj, &key, false);
    let st = state();
    owner_index_add(&st.descriptors.accessor_keys_by_owner, obj, &key);
    owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
    st.descriptors
        .accessor_descriptors
        .borrow_mut()
        .insert((obj, key.clone()), acc);
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .insert((obj, key), attrs);
}

/// Install a built-in *reflection-only* data-property descriptor for (obj, key)
/// WITHOUT flipping the process-wide `GLOBAL_DESCRIPTORS_IN_USE` /
/// `PROPERTY_ATTRS_IN_USE` hot-path gates — the data-property analogue of
/// [`set_builtin_accessor_descriptor`].
///
/// Built-in prototype methods are spec'd as `{ writable: true,
/// enumerable: false, configurable: true }`, but `install_proto_method`
/// stores them via the ordinary field-set path (default all-true), so
/// `Object.getOwnPropertyDescriptor(Array.prototype, "map").enumerable` and a
/// `for (k in Array.prototype)` scan both reported them as enumerable —
/// failing Test262's pervasive `verifyProperty` checks. Recording a
/// non-enumerable descriptor here fixes all three observation paths
/// (`getOwnPropertyDescriptor`, `Object.keys`, `for-in`), each of which reads
/// `PROPERTY_DESCRIPTORS` per-object and unconditionally. The gate stays
/// down, so the object get/set hot path is unaffected for every program.
pub(crate) fn set_builtin_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    super::prop_plan::prop_plan_epoch_bump();
    note_descriptor_target(obj);
    // #6759 Phase C2: see `set_builtin_accessor_descriptor`.
    note_meta_descriptor_key(obj, &key, false);
    let st = state();
    owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .insert((obj, key), attrs);
}

/// Walk the keys array of `obj` and apply the given attribute mask AND filter to every existing key.
/// Used by `Object.freeze` (drops `writable` + `configurable`) and `Object.seal` (drops `configurable`).
pub(crate) unsafe fn mark_all_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    _drop_enumerable: bool,
    drop_configurable: bool,
) {
    let keys = crate::object::object_keys_array(obj);
    if keys.is_null() {
        return;
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count == 0 || key_count > 65536 {
        return;
    }
    let obj_addr = obj as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let stored_key = key_val.as_string_ptr();
        if stored_key.is_null() {
            continue;
        }
        let name_ptr = (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*stored_key).byte_len as usize;
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_str = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };
        // Start from existing attrs (or default `{w:true, e:true, c:true}`) and clear bits.
        let mut attrs =
            get_property_attrs(obj_addr, &key_str).unwrap_or(PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
        set_property_attrs(obj_addr, key_str, attrs);
    }
}

/// Death pruning for the two descriptor side tables (2026-07-09 GC audit
/// wave 2). Entries are keyed by `(owner_addr, key)` and were never removed
/// when the owner died: `Object.freeze(perRequestObj)` leaked one entry per
/// key per request, accessor closures were immortalized by the root scanner
/// below, and a fresh object at a recycled address inherited the dead
/// owner's descriptors (stale "read only property" throws). `is_dead_owner`
/// is one of the GC's post-trace / copied-minor deadness predicates
/// (`gc::dead_owner`); each distinct owner is probed once.
pub(crate) fn prune_dead_descriptor_owner_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut verdicts: HashMap<usize, bool> = HashMap::new();
    let mut is_dead = |owner: usize| -> bool {
        *verdicts
            .entry(owner)
            .or_insert_with(|| is_dead_owner(owner))
    };
    let st = state();
    {
        let mut m = st.descriptors.property_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| !is_dead(*owner));
        }
    }
    {
        let mut m = st.descriptors.accessor_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| !is_dead(*owner));
        }
    }
    // Keep the owner index in step: a dead owner left here would keep
    // reporting keys through `accessor_descriptor_keys_for_obj` after its
    // entries were reaped, and would be re-walked by every later GC scan.
    for index in [
        &st.descriptors.attr_keys_by_owner,
        &st.descriptors.accessor_keys_by_owner,
    ] {
        let mut idx = index.borrow_mut();
        if !idx.is_empty() {
            idx.retain(|owner, _| !is_dead(*owner));
        }
    }
}

/// #6710: drop every property-attr + accessor descriptor owned by `obj`.
///
/// The generic descriptor tables are keyed by owner address; for a native
/// handle that address is its (recycled) handle id. `gc_sweep_dead_descriptors`
/// only reaps entries whose owner is a dead *heap* object, so a recycled handle
/// id's descriptors survive into the next owner. Called from
/// `handle_expando_clear` when perry-ffi hands a freed handle id back out.
pub(crate) fn clear_object_descriptors(obj: usize) {
    // Fast path: if no handle-band owner ever received a descriptor, these
    // tables hold only heap owners — none of whose keys can match `obj` (a
    // handle id) — so the O(N) `retain` scans would remove nothing. Skip them.
    if !HANDLE_HAS_DESCRIPTORS.load(Ordering::Relaxed) {
        return;
    }
    let st = state();
    {
        let mut m = st.descriptors.property_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| *owner != obj);
        }
    }
    {
        let mut m = st.descriptors.accessor_descriptors.borrow_mut();
        if !m.is_empty() {
            m.retain(|(owner, _), _| *owner != obj);
        }
    }
    st.descriptors.attr_keys_by_owner.borrow_mut().remove(&obj);
    st.descriptors
        .accessor_keys_by_owner
        .borrow_mut()
        .remove(&obj);
}

/// Move string-keyed descriptor ownership when `ArrayHeader` growth replaces
/// one live allocation with another. Array growth is not a GC collection, so
/// the metadata-rewrite scanner below does not run; without this explicit
/// transfer, descriptors installed before a later grow remain keyed to the
/// forwarding stub and disappear from reads through the canonical array head.
pub(crate) fn transfer_descriptor_owner(old_owner: usize, new_owner: usize) {
    if old_owner == new_owner {
        return;
    }
    let st = state();
    // The owner index names exactly this owner's keys, so neither table is
    // walked in full any more. Array growth calls this on every reallocation.
    {
        let moved = st
            .descriptors
            .attr_keys_by_owner
            .borrow()
            .get(&old_owner)
            .cloned()
            .unwrap_or_default();
        let mut attrs = st.descriptors.property_descriptors.borrow_mut();
        for key in moved {
            if let Some(value) = attrs.remove(&(old_owner, key.clone())) {
                attrs.insert((new_owner, key), value);
            }
        }
    }
    {
        let moved = st
            .descriptors
            .accessor_keys_by_owner
            .borrow()
            .get(&old_owner)
            .cloned()
            .unwrap_or_default();
        let mut accessors = st.descriptors.accessor_descriptors.borrow_mut();
        for key in moved {
            if let Some(value) = accessors.remove(&(old_owner, key.clone())) {
                accessors.insert((new_owner, key), value);
            }
        }
    }
    owner_index_transfer(&st.descriptors.attr_keys_by_owner, old_owner, new_owner);
    owner_index_transfer(
        &st.descriptors.accessor_keys_by_owner,
        old_owner,
        new_owner,
    );

    // Carry the per-object Bloom summary across too. Every descriptor read is
    // gated on the owner's `attr_key_bits` / `accessor_key_bits`
    // (`owner_may_have_descriptor_entries`), and a freshly grown array has a
    // null `meta` — for which that gate answers **false**, authoritatively.
    // Without this the entries move correctly and then read back as absent:
    // `Object.keys` / `getOwnPropertyDescriptor` silently lose every accessor
    // an array had before it grew. (Pre-existing: the gate sat in front of the
    // old full-table scan as well, so the scan never ran for the new owner.)
    //
    // Done after the borrows above are released — `note_meta_descriptor_key`
    // allocates via `object_meta_ensure`.
    let moved_attr = st
        .descriptors
        .attr_keys_by_owner
        .borrow()
        .get(&new_owner)
        .cloned()
        .unwrap_or_default();
    let moved_acc = st
        .descriptors
        .accessor_keys_by_owner
        .borrow()
        .get(&new_owner)
        .cloned()
        .unwrap_or_default();
    for key in &moved_attr {
        note_meta_descriptor_key(new_owner, key, false);
    }
    for key in &moved_acc {
        note_meta_descriptor_key(new_owner, key, true);
    }
}

/// Rewrite a descriptor table's owner ADDRESS during the GC metadata-rewrite
/// phase (evacuation moved the owning object), mirroring the symbol-keyed
/// twin tables' owner rekey (`symbol/gc_roots.rs`). Outside that phase the
/// owner is returned unchanged.
fn rewrite_descriptor_owner(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) -> usize {
    if !visitor.is_metadata_rewrite_phase() {
        return owner;
    }
    let mut addr = owner;
    visitor.visit_metadata_usize_slot(&mut addr);
    addr
}

/// GC scanner for the string-keyed descriptor side tables (2026-07-02 audit
/// P0; ported from the stranded be73b4f8d): `ACCESSOR_DESCRIPTORS` holds the
/// ONLY reference to `Object.defineProperty` getter/setter closures (the
/// accessor install path stores no field-slot copy), so without visiting
/// them a minor GC sweeps or moves the closure out from under the next
/// property read. Owner keys are `(obj_addr, key)` — rekeyed when the owning
/// object moves, exactly like the symbol-keyed twins, so frozen/non-writable
/// attrs and accessors don't silently detach (or fire on a new tenant at a
/// reused address).
pub(crate) fn scan_descriptor_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let st = state();
    {
        // Probe DISTINCT OWNERS via the index, not every `(owner, key)` pair.
        // This runs on every GC cycle, and since the moving young-gen scavenge
        // became the default (#7019) that is often — so an O(total descriptors)
        // probe here was a per-collection tax proportional to the whole
        // program's descriptor count rather than to what actually moved.
        let needs_rebuild = st
            .descriptors
            .attr_keys_by_owner
            .borrow()
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        let mut descriptors = st.descriptors.property_descriptors.borrow_mut();
        if needs_rebuild {
            let old = std::mem::take(&mut *descriptors);
            for ((owner, key), attrs) in old {
                let owner = rewrite_descriptor_owner(visitor, owner);
                descriptors.insert((owner, key), attrs);
            }
        }
    }

    {
        let needs_rebuild = st
            .descriptors
            .accessor_keys_by_owner
            .borrow()
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        let mut descriptors = st.descriptors.accessor_descriptors.borrow_mut();
        if needs_rebuild {
            let old = std::mem::take(&mut *descriptors);
            for ((owner, key), mut acc) in old {
                if acc.get != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.get);
                }
                if acc.set != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.set);
                }
                let owner = rewrite_descriptor_owner(visitor, owner);
                descriptors.insert((owner, key), acc);
            }
        } else {
            for acc in descriptors.values_mut() {
                if acc.get != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.get);
                }
                if acc.set != 0 {
                    visitor.visit_nanbox_u64_slot(&mut acc.set);
                }
            }
        }
    }

    // Rekey the owner index itself. Evacuation moved the owning objects, so
    // the tables above were rebuilt under new addresses; an index still keyed
    // by the OLD addresses would report no keys for the moved object (silently
    // dropping its accessors from `Object.keys`) and would keep a dead address
    // alive in every later scan. Merge on collision: an address freed by one
    // object can be reused by another in the same cycle.
    for index in [
        &st.descriptors.attr_keys_by_owner,
        &st.descriptors.accessor_keys_by_owner,
    ] {
        let mut idx = index.borrow_mut();
        if idx.is_empty() {
            continue;
        }
        let needs_rekey = idx
            .keys()
            .any(|owner| rewrite_descriptor_owner(visitor, *owner) != *owner);
        if !needs_rekey {
            continue;
        }
        let old = std::mem::take(&mut *idx);
        for (owner, keys) in old {
            let owner = rewrite_descriptor_owner(visitor, owner);
            let dest = idx.entry(owner).or_default();
            for k in keys {
                if !dest.iter().any(|existing| *existing == k) {
                    dest.push(k);
                }
            }
        }
    }
}

/// The owner index (`attr_keys_by_owner` / `accessor_keys_by_owner`) exists
/// only to answer "which keys does this owner have?" without walking every
/// descriptor in the process. It is a mirror, so the one way it can break is
/// **drift** from the tables it mirrors — which would not crash, it would
/// silently drop keys from `Object.keys` or resurrect deleted ones.
///
/// These tests therefore assert the mirror invariant directly (index ==
/// what a full scan of the table would return) across install, redefine,
/// delete, bulk-clear and owner-transfer.
#[cfg(test)]
mod owner_index_tests {
    use super::*;
    use std::collections::BTreeSet;

    /// What the pre-index implementation would have computed: a full scan of
    /// the table filtered by owner. The index must always agree with this.
    fn scan_table_keys(accessor: bool, owner: usize) -> BTreeSet<String> {
        let st = state();
        if accessor {
            st.descriptors
                .accessor_descriptors
                .borrow()
                .keys()
                .filter(|(o, _)| *o == owner)
                .map(|(_, k)| k.clone())
                .collect()
        } else {
            st.descriptors
                .property_descriptors
                .borrow()
                .keys()
                .filter(|(o, _)| *o == owner)
                .map(|(_, k)| k.clone())
                .collect()
        }
    }

    fn index_keys(accessor: bool, owner: usize) -> BTreeSet<String> {
        let st = state();
        let idx = if accessor {
            &st.descriptors.accessor_keys_by_owner
        } else {
            &st.descriptors.attr_keys_by_owner
        };
        idx.borrow()
            .get(&owner)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    fn assert_mirrors(owner: usize, ctx: &str) {
        for (accessor, label) in [(false, "property"), (true, "accessor")] {
            assert_eq!(
                index_keys(accessor, owner),
                scan_table_keys(accessor, owner),
                "{label} owner index drifted from the table it mirrors ({ctx}); \
                 a drift here silently corrupts Object.keys / for-in output"
            );
        }
    }

    #[test]
    fn index_mirrors_tables_across_install_redefine_and_delete() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0);
        let addr = obj as usize;

        set_property_attrs(addr, "a".to_string(), PropertyAttrs::new(true, true, true));
        set_property_attrs(addr, "b".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(addr, "g".to_string(), AccessorDescriptor::default());
        assert_mirrors(addr, "after installs");

        // Redefining an existing key must not duplicate it — a duplicate would
        // make `Object.keys` report the key twice.
        set_property_attrs(addr, "a".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(addr, "g".to_string(), AccessorDescriptor::default());
        assert_eq!(
            state()
                .descriptors
                .attr_keys_by_owner
                .borrow()
                .get(&addr)
                .map(|v| v.len()),
            Some(2),
            "redefining an existing descriptor must not push a duplicate key"
        );
        assert_mirrors(addr, "after redefine");

        clear_property_attrs(addr, "a");
        clear_accessor_descriptor(addr, "g");
        assert_mirrors(addr, "after delete");

        // Deleting the last key must drop the owner entry entirely, so a dead
        // owner leaves nothing for later GC scans to walk.
        clear_property_attrs(addr, "b");
        assert!(
            !state()
                .descriptors
                .attr_keys_by_owner
                .borrow()
                .contains_key(&addr),
            "an owner with no remaining descriptors must be removed from the index"
        );
    }

    #[test]
    fn accessor_keys_for_obj_agrees_with_a_full_scan() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0);
        let addr = obj as usize;
        // A second owner with its own accessors: the whole point of the index
        // is that this one's keys never leak into the first one's answer.
        let other = crate::object::js_object_alloc(0, 0);
        let other_addr = other as usize;

        for k in ["z", "m", "a"] {
            set_accessor_descriptor(addr, k.to_string(), AccessorDescriptor::default());
        }
        for k in ["zz", "mm"] {
            set_accessor_descriptor(other_addr, k.to_string(), AccessorDescriptor::default());
        }

        let got = accessor_descriptor_keys_for_obj(addr);
        assert_eq!(
            got,
            vec!["a".to_string(), "m".to_string(), "z".to_string()],
            "keys must be sorted and scoped to the requested owner only"
        );
        assert_eq!(
            got.into_iter().collect::<BTreeSet<_>>(),
            scan_table_keys(true, addr),
            "the index answer must equal what a full table scan would return"
        );
    }

    #[test]
    fn transfer_moves_both_tables_and_the_index() {
        let _lock = crate::gc::global_side_table_test_lock();
        let old = crate::object::js_object_alloc(0, 0) as usize;
        let new = crate::object::js_object_alloc(0, 0) as usize;

        set_property_attrs(old, "p".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(old, "acc".to_string(), AccessorDescriptor::default());

        transfer_descriptor_owner(old, new);

        assert_mirrors(old, "old owner after transfer");
        assert_mirrors(new, "new owner after transfer");
        assert!(
            scan_table_keys(false, old).is_empty() && scan_table_keys(true, old).is_empty(),
            "transfer must leave nothing behind under the old owner address"
        );
        assert_eq!(
            accessor_descriptor_keys_for_obj(new),
            vec!["acc".to_string()],
            "accessors must be readable through the new owner address after growth"
        );
    }

    #[test]
    fn clear_object_descriptors_empties_the_index_too() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = crate::object::js_object_alloc(0, 0) as usize;
        // `clear_object_descriptors` early-returns unless a handle-band owner
        // has ever taken a descriptor; set the latch so the body actually runs.
        HANDLE_HAS_DESCRIPTORS.store(true, Ordering::Relaxed);

        set_property_attrs(obj, "p".to_string(), PropertyAttrs::new(true, true, true));
        set_accessor_descriptor(obj, "acc".to_string(), AccessorDescriptor::default());
        assert_mirrors(obj, "before clear");

        clear_object_descriptors(obj);
        assert_mirrors(obj, "after clear");
        assert!(
            accessor_descriptor_keys_for_obj(obj).is_empty(),
            "a cleared owner must report no accessor keys"
        );
    }
}

#[cfg(test)]
mod c5a_tests {
    use super::*;

    /// #6759 C5a: a prototype-level descriptor whose key names no declared
    /// instance field must NOT flip the process-wide inline-guard disable;
    /// one whose key IS a declared field must.
    #[test]
    fn inline_guard_disable_is_per_declared_field_key() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset_class_field_inline_guard();

        let proto = crate::object::js_object_alloc(0, 0);
        class_registry::class_prototype_object_root_store(0x0666_0001, proto);
        let proto_addr = proto as usize;

        // Method-style install (babel output): key declared by no class.
        set_accessor_descriptor(
            proto_addr,
            "c5a_render_method".to_string(),
            AccessorDescriptor::default(),
        );
        assert!(
            class_field_inline_guard_enabled(),
            "a prototype install keyed by a non-field name must not poison \
             the inline class-field fast path"
        );
        assert!(
            !class_registry::class_prototype_fast_guards_invalidated(),
            "a keyed prototype descriptor must not retire every method guard"
        );
        let render_slot = class_registry::class_prototype_method_guard_slot("c5a_render_method");
        assert!(
            class_registry::class_prototype_fast_guard_invalidated_for_method(render_slot),
            "a prototype descriptor must retire its matching method guard"
        );
        let other_slot = class_registry::class_prototype_method_guard_slot("c5a_other_method");
        assert!(
            !class_registry::class_prototype_fast_guard_invalidated_for_method(other_slot),
            "an unrelated method guard must remain valid"
        );

        // Field-style install: key declared by a registered class.
        note_declared_instance_field_name(b"c5a_field_x");
        assert!(
            class_field_inline_guard_enabled(),
            "declaring the field alone (no matching install) must not disable"
        );
        set_property_attrs(
            proto_addr,
            "c5a_field_x".to_string(),
            PropertyAttrs::new(false, true, true),
        );
        assert!(
            !class_field_inline_guard_enabled(),
            "a prototype install keyed by a DECLARED field must disable"
        );

        test_reset_class_field_inline_guard();
    }

    /// #6759 C5a ordering: an install that precedes the declaring class's
    /// registration is retro-checked when the class arrives.
    #[test]
    fn inline_guard_retro_disable_on_late_class_registration() {
        let _lock = crate::gc::global_side_table_test_lock();
        test_reset_class_field_inline_guard();

        let proto = crate::object::js_object_alloc(0, 0);
        class_registry::class_prototype_object_root_store(0x0666_0002, proto);

        set_accessor_descriptor(
            proto as usize,
            "c5a_late_field".to_string(),
            AccessorDescriptor::default(),
        );
        assert!(
            class_field_inline_guard_enabled(),
            "no class declares the key yet — install must skip the disable"
        );

        // The declaring class registers AFTER the install.
        note_declared_instance_field_name(b"c5a_late_field");
        assert!(
            !class_field_inline_guard_enabled(),
            "late class registration must retro-trigger the disable for \
             prototype keys installed earlier"
        );

        test_reset_class_field_inline_guard();
    }
}
