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
    /// #9754: owners whose entries may hold a pointer a minor can act on —
    /// a young owner, or an accessor whose getter/setter closure is young.
    /// A minor-scoped `scan_descriptor_roots_mut` visits only these; see
    /// `gc/young_log.rs`.
    pub(crate) young_owners: RefCell<crate::gc::young_log::YoungLog<usize>>,
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
            young_owners: RefCell::new(crate::gc::young_log::YoungLog::new()),
        }
    }
}

const DESCRIPTOR_YOUNG_LOG_NAME: &str = "object.descriptors";

#[cfg(test)]
thread_local! {
    static TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE: Cell<bool> = const { Cell::new(false) };
}

mod gc_scan;
mod owner_lifecycle;
mod young;
pub(crate) use gc_scan::{scan_descriptor_owner, scan_descriptor_roots_mut};
pub(crate) use owner_lifecycle::{
    clear_object_descriptors, prune_dead_descriptor_owner_entries,
    prune_dead_descriptor_owner_entries_young, transfer_descriptor_owner,
};
use young::{relevant_descriptor_owners, scan_descriptor_roots_young};

/// Rule 1 of `gc/young_log.rs`: log `owner` BEFORE its descriptor is
/// published when the owner, or the accessor closure being stored, can
/// matter to a minor. Data descriptors carry no pointer, so `acc` is `None`
/// for them and only the owner decides.
#[inline]
fn note_young_descriptor_owner(
    st: &crate::state::RuntimeState,
    owner: usize,
    acc: Option<&AccessorDescriptor>,
) {
    use crate::gc::young_log::{addr_is_minor_collectible, bits_are_minor_relevant};
    if addr_is_minor_collectible(owner)
        || acc
            .is_some_and(|acc| bits_are_minor_relevant(acc.get) || bits_are_minor_relevant(acc.set))
    {
        #[cfg(test)]
        if TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE.with(Cell::get) {
            return;
        }
        st.descriptors.young_owners.borrow_mut().note(owner);
    }
}

#[cfg(test)]
mod young_log_sabotage_tests {
    use super::*;

    #[test]
    fn descriptor_log_rederivation_rejects_a_suppressed_setter() {
        let _lock = crate::gc::global_side_table_test_lock();
        let owner = crate::object::js_object_alloc(0, 0) as usize;
        state().descriptors.young_owners.borrow_mut().clear();
        TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE.with(|flag| flag.set(true));
        set_property_attrs(
            owner,
            "sabotage".to_string(),
            PropertyAttrs::new(true, true, true),
        );
        TEST_SUPPRESS_DESCRIPTOR_YOUNG_NOTE.with(|flag| flag.set(false));
        let missed = std::panic::catch_unwind(|| {
            state()
                .descriptors
                .young_owners
                .borrow()
                .debug_assert_logged(
                    DESCRIPTOR_YOUNG_LOG_NAME,
                    &relevant_descriptor_owners(state()),
                );
        });
        clear_property_attrs(owner, "sabotage");
        state().descriptors.young_owners.borrow_mut().clear();
        assert!(
            missed.is_err(),
            "sabotage: suppressing set_property_attrs' note must trip completeness"
        );
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
fn owner_index_remove(
    index: &RefCell<FastKeyHashMap<usize, Vec<String>>>,
    owner: usize,
    key: &str,
) {
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
///
/// Sets the latch AND poisons every registered guard-expectation slot. The two
/// are one decision with two carriers: sites that still read the latch keep
/// working unchanged, while `emit_class_field_inline_precheck` — the per-access
/// guard on every static-key read — gets the same authority for free out of the
/// expectation it already loads. Poison first, so no thread can observe a set
/// latch beside a live expectation.
pub(crate) fn disable_class_field_inline_guard() {
    super::class_guard_shape::poison_class_guard_shapes();
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.store(1, Ordering::Relaxed);
}

/// True when the inline class-field fast path is still permitted.
pub(crate) fn class_field_inline_guard_enabled() -> bool {
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.load(Ordering::Relaxed) == 0
}

#[cfg(test)]
pub(crate) fn test_reset_class_field_inline_guard() {
    PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED.store(0, Ordering::Relaxed);
    // Unpoison every registered expectation back to the ShapeId it was seeded
    // with. Production never does this — the disable decision is monotonic —
    // but a test that flips the latch must not leave later tests guarding
    // against `CLASS_GUARD_SHAPE_POISON`.
    super::class_guard_shape::restore_class_guard_shapes_for_test();
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
/// The prototype-registry probe used to scan by value (O(#classes)). This
/// comment used to add "descriptor installs are rare and never on the hot
/// property path, so the scan cost is acceptable", and that is false for every bundle:
/// esbuild's `__export(exports, { … })` makes `Object.defineProperty` a
/// module-init primitive — claude-code's bundle contains 1,526 of them — so
/// this function runs 26,290 times on `claude --help` and
/// `is_registered_class_prototype_object`'s scan alone was 0.46% of the run.
/// It is now served by the exact inverse prototype-address index, including GC
/// rekeys, so each negative probe stays O(1) even when a class-heavy graph
/// saturates the older monotone address filter (#9225, #10106).
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
pub(crate) fn note_descriptor_target(obj: usize) {
    note_descriptor_target_keyed(obj, None);
}

/// [`note_descriptor_target`] for a single DATA-descriptor install (#10287):
/// the semantic shape transition is keyed on `(key, attrs)`, so receivers that
/// repeat the same install over the same predecessor share the successor shape
/// instead of each minting a private lineage. Every other descriptor mutation
/// (accessors, batches, clears, prototype changes) keeps a unique generation.
pub(crate) fn note_data_descriptor_target(obj: usize, key: &str, attrs: PropertyAttrs) {
    note_descriptor_target_keyed(obj, Some((key.as_bytes(), attrs.bits)));
}

/// [`note_descriptor_target`] for an ACCESSOR install (#10287).
///
/// Keyed on the descriptor's SHAPE — which halves are present — never on the
/// getter/setter identities, which differ per receiver (zod binds a fresh
/// closure per schema). That is sound for the same reason the data form is:
/// the closures live in the per-object accessor table, and every cache that
/// could serve this key refuses a receiver carrying
/// `OBJ_FLAG_HAS_DESCRIPTORS` — the emitted read IC's `data_only` test, the
/// write PIC's blocking mask, the runtime read stub, and the per-key gates
/// here, which report an accessor key as covered. Without this, one lazily
/// installed accessor (zod's `defineLazy`) put every receiver on a private
/// lineage: +41% instructions on a 2,000-receiver fixture.
///
/// The high bit keeps this encoding disjoint from [`PropertyAttrs`]' three.
pub(crate) fn note_accessor_descriptor_target(obj: usize, key: &str, acc: &AccessorDescriptor) {
    let shape = 0x80u8 | u8::from(acc.get != 0) | (u8::from(acc.set != 0) << 1);
    note_descriptor_target_keyed(obj, Some((key.as_bytes(), shape)));
}

/// The ONE funnel every descriptor install goes through, and therefore the
/// one place RULE 1 ("every descriptor change changes the ShapeId of an
/// ordinary object") is implemented.
///
/// What it covers, and what it deliberately does not — this is the exact
/// scope any shape-only read guard inherits:
///
/// * `GC_TYPE_OBJECT`: sets `OBJ_FLAG_HAS_DESCRIPTORS` **and**, when the
///   receiver is shaped, transitions the shape. A cache entry keyed on the
///   old ShapeId can no longer match, so for these receivers the shape
///   compare subsumes the flag test.
/// * **typed arrays**: early return, before either. A small typed array is
///   plain-alloc'd without a `GcHeader`, so there is no flag bit to set and
///   no `ObjectHeader` to stamp.
/// * **every other cell kind** (array, closure, Map/Set, RegExp, Error,
///   Promise, native handles, handle-band ids): the `obj_type` test below
///   rejects them, so they get neither the flag nor a shape transition.
///
/// Consequence for the emitted read path: for a non-`GC_TYPE_OBJECT`
/// receiver the descriptor flag is *never set*, so dropping the flag test
/// loses nothing — but the SHAPE is equally uninformative, so such receivers
/// must still be rejected by KIND. Removing the GC-header load from the read
/// path needs their descriptor state carried in the shape word first
/// (`rule1_funnel_does_not_cover_non_object_receivers` pins this).
fn note_descriptor_target_keyed(obj: usize, data_install: Option<(&[u8], u8)>) {
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
                    match data_install {
                        Some((key_bytes, attrs)) => {
                            crate::object::shapes::
                                transition_object_shape_semantics_for_data_descriptor(
                                    object, key_bytes, attrs,
                                );
                        }
                        None => {
                            crate::object::shapes::transition_object_shape_semantics(object);
                        }
                    }
                }
            }
        }
    }
}

/// Look up the property descriptor for (obj, key). Returns None if no entry exists,
/// in which case the JS default `{ writable: true, enumerable: true, configurable: true }` applies.
pub(crate) fn get_property_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    // A STORED descriptor wins over the synthesized index default:
    // `Object.defineProperty` / `Object.freeze` on a wrapper installs a real
    // entry, and the §10.4.3 default must not shadow it. Synthesis therefore
    // happens in the `string_wrapper_index_attrs` fallback BELOW the table
    // probe, never as an early return above it.
    //
    // #6759 Phase C2: the meta-record summary proves most misses without
    // the `String` build + table probe (and shields a fresh object at a
    // recycled address from a dead owner's not-yet-pruned entries).
    if may_have_descriptor_entry(obj, key, false) {
        if let Some(attrs) = state()
            .descriptors
            .property_descriptors
            .borrow()
            .get(&(obj, key.to_string()))
            .copied()
        {
            return Some(attrs);
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

/// #6759 phase 1 follow-up: the owner's meta record for descriptor-summary
/// purposes, for ANY cell type that owns one.
///
/// [`super::prototype_chain::meta_capable_object`] answers only for
/// `GC_TYPE_OBJECT`, because its other callers need an `ObjectHeader` to work
/// with. The descriptor summary does not — it needs the `ObjectMeta` edge and
/// nothing else — and every exotic cell has carried that edge since #6759
/// phase 1 unified it behind [`super::cell_meta_slot`]. Asking the narrower
/// question is what lets a `RegExp` receiver answer a summary probe at all.
///
/// * `None` — the cell type has no meta edge, so the caller must stay
///   conservative and probe the tables.
/// * `Some(null)` — the cell HAS the edge and no record was ever installed,
///   which proves the tables hold no entry for this owner.
/// * `Some(meta)` — read the summary words.
///
/// The three-way answer is the whole contract: collapsing "no edge" and "edge,
/// but null" into one `None` would turn a conservative *maybe* into a false
/// *no* for the cell types that still lack an edge.
#[inline]
unsafe fn descriptor_summary_meta(owner: usize) -> Option<*mut ObjectMeta> {
    Some(*super::cell_meta_slot(owner)?)
}

/// Installing twin of [`descriptor_summary_meta`]. Install and probe MUST use
/// the same predicate: a probe that admits a cell type whose installs do not
/// set the key bits would answer a proven-absent for an owner that really has
/// a descriptor — e.g. `Object.defineProperty(re, "lastIndex", {writable:false})`
/// would stop throwing (test262 prototype/{exec,test}/y-fail-lastindex-no-write).
#[inline]
unsafe fn descriptor_summary_meta_ensure(owner: usize) -> Option<*mut ObjectMeta> {
    super::object_meta_ensure_for_cell(owner)
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
        // No-move window: the ensure below allocates, and a triggered
        // collection could MOVE `owner` — installers (freeze/seal loops,
        // defineProperty) hold raw owner pointers across repeated installs.
        let _no_gc = crate::gc::GcSuppressScope::new();
        if let Some(meta) = descriptor_summary_meta_ensure(owner) {
            let bit = descriptor_key_bit(key);
            if accessor {
                (*meta).accessor_key_bits |= bit;
            } else {
                (*meta).attr_key_bits |= bit;
            }
            // #10287: exact identity while this owner has descriptors for a
            // single key, so per-key probes need no table lookup at all.
            let hash = super::key_bytes_hash(key.as_ptr(), key.len());
            match (*meta).descriptor_key_count {
                0 => {
                    (*meta).descriptor_key_hash = hash;
                    (*meta).descriptor_key_count = 1;
                }
                1 if (*meta).descriptor_key_hash == hash => {}
                _ => (*meta).descriptor_key_count = 2,
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
        let answer = match descriptor_summary_meta(owner) {
            Some(meta) => {
                if meta.is_null() {
                    false
                } else {
                    let word = if accessor {
                        (*meta).accessor_key_bits
                    } else {
                        (*meta).attr_key_bits
                    };
                    word & descriptor_key_bit(key) != 0
                }
            }
            None => true,
        };
        // Diagnostic only, and only when the instrument is armed: one relaxed
        // load otherwise. Counts the RegExp receivers this filter sees and how
        // many it now proves absent — before the meta edge was wired for
        // RegExp the second number was 0 by construction.
        if crate::hot_diag::regex_on() {
            note_regexp_descriptor_probe(owner, answer);
        }
        answer
    }
}

/// Test-only view of [`may_have_descriptor_entry`], so a test can assert the
/// FILTER's answer rather than only the value it filters to. Without this a
/// test can see that `get_property_attrs` returns `None`, which is equally
/// true when the fast negative never fired — it would pass against a change
/// that did nothing.
#[cfg(test)]
pub(crate) fn test_may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    may_have_descriptor_entry(owner, key, accessor)
}

/// Diagnostic counter for [`may_have_descriptor_entry`]: is this owner a
/// RegExp cell, and did the summary prove the key absent? Split out and marked
/// cold so the armed check costs the hot path a predictable branch and nothing
/// else.
#[cold]
unsafe fn note_regexp_descriptor_probe(owner: usize, answer: bool) {
    let Some(header) = crate::value::addr_class::try_read_gc_header(owner) else {
        return;
    };
    if header.obj_type != crate::gc::GC_TYPE_REGEXP {
        return;
    }
    crate::hot_diag::regex_with(|d| {
        d.desc_regexp_probes += 1;
        if !answer {
            d.desc_regexp_meta_negative += 1;
        }
    });
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
    match descriptor_summary_meta(addr) {
        Some(meta) => {
            if meta.is_null() {
                return false;
            }
            let bit = descriptor_key_bit_bytes(kb);
            ((*meta).attr_key_bits | (*meta).accessor_key_bits) & bit != 0
        }
        None => true,
    }
}

/// Per-key refinement of `OBJ_FLAG_HAS_DESCRIPTORS` for the store fast paths
/// (#10287). `true` proves no OWN string-keyed descriptor (attr or accessor)
/// covers `key` on `addr`, so a store of `key` meets the same own-property
/// preconditions as one on a receiver that never had a descriptor. Prototype
/// vetting stays with the caller.
///
/// zod v4 opens every schema constructor with
/// `Object.defineProperty(inst, "_zod", …)` and then installs ~60 methods by
/// assignment; the object-wide flag sent every one of those stores down the
/// full `OrdinarySet` walk.
///
/// Index-shaped keys stay on the slow walk: a boxed `String` wrapper
/// synthesizes non-writable index attributes that the summary never records.
pub(crate) unsafe fn own_descriptors_skip_key(addr: usize, key: f64) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(
        crate::value::JSValue::from_bits(key.to_bits()),
        &mut sso,
    ) else {
        return false;
    };
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    // Exact when this owner carries descriptors for one key only (zod's
    // `_zod`): a full-width hash compare, no Bloom, no table probe.
    if let Some(meta) = descriptor_summary_meta(addr) {
        if !meta.is_null() && (*meta).descriptor_key_count == 1 {
            return super::key_bytes_hash(bytes.as_ptr(), bytes.len())
                != (*meta).descriptor_key_hash;
        }
    }
    if !own_descriptor_may_cover_key(addr, key) {
        // Clear summary bits are authoritative: no entry can exist.
        return true;
    }
    // A SET bit is a maybe — the summary is 64 bits wide, so roughly one key
    // in 64 collides with a descriptor key. Confirm against the tables rather
    // than surrendering the fast path.
    //
    // This matters far more than the collision rate suggests: a store that
    // takes the slow path appends to a PRIVATE keys array, which takes the
    // receiver off the shared transition chain for good. Every later store on
    // it then misses the lane too. With one descriptor (`_zod`) and keys
    // `p0..p39`, `p17` collides — so every receiver derailed at the same
    // store and lost the chain for its remaining 23 properties.
    let Ok(name) = std::str::from_utf8(bytes) else {
        return false;
    };
    get_accessor_descriptor(addr, name).is_none() && get_property_attrs(addr, name).is_none()
}

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
        // A class instance or class prototype on the chain keeps its accessors
        // in the class registry, not only in the descriptor tables.
        if class_id != 0 && class_registry::class_chain_has_instance_accessor(class_id, name) {
            return true;
        }
        if object_has_descriptors(p) {
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

/// #6759 Phase C2 owner-level verdict: can the tables hold ANY entry owned
/// by `owner`? Gates the O(table-size) owner scans (`Object.keys` fast
/// path, `accessor_descriptor_keys_for_obj`). Same trust model as the
/// per-key form.
#[inline]
pub(crate) fn owner_may_have_descriptor_entries(owner: usize, accessor: bool) -> bool {
    unsafe {
        match descriptor_summary_meta(owner) {
            Some(meta) => {
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
    // The summary is 64 bits wide, so a SET bit is only a maybe: roughly one
    // key in 64 collides with a descriptor key that is actually present.
    // `own_descriptors_skip_key` confirms a positive against the descriptor
    // tables (and stays conservative for keys it cannot decode).
    //
    // Confirming matters out of all proportion to the collision rate. A store
    // sent down the slow path appends to a PRIVATE keys array, which takes the
    // receiver off the shared transition chain permanently, so every LATER
    // store on that object misses the lane too. With zod's single `_zod`
    // descriptor and keys `p0..p39`, `p17` collided — so all 2000 receivers
    // forked at the same store and ran their remaining 22 properties on
    // per-object shapes.
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
    super::prop_plan::prop_plan_epoch_bump();
    note_data_descriptor_target(obj, &key, attrs);
    let st = state();
    st.descriptors.property_attrs_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_inline_guards_for_descriptor_target(obj, &key);
    note_meta_descriptor_key(obj, &key, false);
    note_young_descriptor_owner(st, obj, None);
    owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .insert((obj, key), attrs);
}

/// Install a group of data descriptors without exposing intermediate states.
/// No JS runs between entries, so one plan invalidation and semantic shape
/// transition retire all prior observations just as repeated installs would.
/// The per-key guard, owner index, and GC bookkeeping still run for every key.
pub(crate) fn set_property_attrs_batch(obj: usize, entries: &[(&str, PropertyAttrs)]) {
    if entries.is_empty() {
        return;
    }
    super::prop_plan::prop_plan_epoch_bump();
    note_descriptor_target(obj);
    let st = state();
    st.descriptors.property_attrs_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    for &(key, attrs) in entries {
        disable_inline_guards_for_descriptor_target(obj, key);
        note_meta_descriptor_key(obj, key, false);
        note_young_descriptor_owner(st, obj, None);
        owner_index_add(&st.descriptors.attr_keys_by_owner, obj, key);
        st.descriptors
            .property_descriptors
            .borrow_mut()
            .insert((obj, key.to_string()), attrs);
    }
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
            crate::object::shapes::transition_object_shape_semantics_for_descriptor_removal(
                object,
                key.as_bytes(),
                false,
            );
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
    // Builtin accessors live in the descriptor table without arming the
    // user-accessor fast-path gate. Reflect.get with a distinct receiver must
    // find them too; get_accessor_descriptor uses each owner's key summary.
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
    let key_string = scope.root_string_ptr(key_str);
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
        // ClassBody accessors live in the declared class's vtable, not the
        // ordinary descriptor table. A RegExp subclass's `get flags()` must
        // win before the walk reaches RegExp.prototype's builtin getter.
        if let Some(cid) = class_registry::class_id_for_decl_prototype_object(obj as usize) {
            // A later own data definition replaces the ClassBody accessor.
            if unsafe { own_key_present(obj as *mut ObjectHeader, key_string.get_raw_const_ptr()) }
            {
                return None;
            }
            if let Some((getter, _)) =
                class_registry::class_declared_accessor_ptrs(cid, false, &name)
            {
                return Some(if getter == 0 {
                    0
                } else {
                    class_registry::class_accessor_function_value(getter, false, &name).to_bits()
                });
            }
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
    let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
    let prev = this_scope.root_nanbox_f64(js_implicit_this_set(receiver));
    let result = crate::closure::js_closure_call0(closure);
    js_implicit_this_set(prev.get_nanbox_f64());
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
    note_accessor_descriptor_target(obj, &key, &acc);
    let st = state();
    st.descriptors.accessors_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_inline_guards_for_descriptor_target(obj, &key);
    note_accessor_descriptor_key(&key);
    note_meta_descriptor_key(obj, &key, true);
    note_young_descriptor_owner(st, obj, Some(&acc));
    owner_index_add(&st.descriptors.accessor_keys_by_owner, obj, &key);
    st.descriptors
        .accessor_descriptors
        .borrow_mut()
        .insert((obj, key), acc);
}

/// #9103 follow-up: one-call install of a BRAND-NEW accessor property — the
/// `{ get, enumerable: true }` fast arm's tail
/// (`object_ops/define_get_accessor.rs`), which installs ~1,245 re-export
/// getters at pi startup and previously paid the full
/// `set_accessor_descriptor` + `set_property_attrs` stack twice over.
///
/// Semantically identical to
/// `set_accessor_descriptor(obj, key.clone(), acc);
///  set_property_attrs(obj, key, attrs);`
/// with the duplicated per-call work folded to one occurrence. Each fold is
/// individually equivalence-preserving:
///
/// * **One epoch bump.** The plan epochs are compared against snapshots for
///   equality ("changed since I cached?"); the two halves of the old sequence
///   run back-to-back on the single mutator thread with no reader in
///   between, so one bump invalidates every snapshot exactly as two did.
/// * **One `note_descriptor_target`.** Its flag writes are idempotent, and
///   its `transition_object_shape_semantics` mints a fresh semantic
///   generation whose only consumer contract is "any cached fact keyed by an
///   older ShapeId is now stale" — one fresh generation retires older ids
///   exactly as two consecutive generations did (nothing can observe the
///   intermediate id: no reader runs between the halves).
/// * **One `disable_inline_guards_for_descriptor_target`.** Both old calls
///   passed the identical `(obj, key)`; the body is idempotent (guard-slot
///   retirement plus a hash-set insert).
/// * **One meta access setting BOTH kind bits** (`accessor_key_bits` /
///   `attr_key_bits`), returning each bit's prior state.
/// * **Owner-index dedupe elided when the kind's meta bit was clear.** The
///   summary's own contract (see `note_meta_descriptor_key` /
///   `may_have_descriptor_entry`: "every insert … whose owner is
///   meta-capable sets the key's bit first, so for such owners a clear bit —
///   or a still-null meta record — proves the tables hold no entry for that
///   key") extends to the owner indexes: every `owner_index_add` site in
///   this file is preceded by the matching-kind `note_meta_descriptor_key`,
///   bits are never cleared, and index removals only shrink the index — so a
///   clear prior bit proves the index holds no entry either, and the O(N)
///   `Vec<String>` dedupe scan (the second-largest term of the __export
///   install profile at 500 keys) can be a plain push. A set prior bit (a
///   Bloom collision, a genuine earlier entry) or a non-meta-capable owner
///   keeps the scanning `owner_index_add`.
///
/// Callers must guarantee the property is brand new on `obj` (the fast arm
/// proves absence via `own_key_present_via_index` /
/// `obj_value_has_own_key` immediately before, with no allocation between
/// probe and install); the descriptor-table `insert`s themselves are plain
/// upserts either way, so a violated precondition degrades to the old
/// overwrite behavior, never to corruption.
pub(crate) fn install_fresh_accessor_property(
    obj: usize,
    key: String,
    acc: AccessorDescriptor,
    attrs: PropertyAttrs,
) {
    super::prop_plan::prop_plan_epoch_bump();
    // #10287: one keyed transition covers the pair, exactly as the two-call
    // sequence this folds would have produced (the accessor half runs last
    // there, so its encoding is the one that survives).
    note_accessor_descriptor_target(obj, &key, &acc);
    let st = state();
    st.descriptors.accessors_in_use.set(true);
    st.descriptors.property_attrs_in_use.set(true);
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_inline_guards_for_descriptor_target(obj, &key);
    note_accessor_descriptor_key(&key);
    note_young_descriptor_owner(st, obj, Some(&acc));
    match note_meta_descriptor_key_both(obj, &key) {
        Some((accessor_bit_was_set, attr_bit_was_set)) => {
            if accessor_bit_was_set {
                owner_index_add(&st.descriptors.accessor_keys_by_owner, obj, &key);
            } else {
                owner_index_push_proven_new(&st.descriptors.accessor_keys_by_owner, obj, &key);
            }
            if attr_bit_was_set {
                owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
            } else {
                owner_index_push_proven_new(&st.descriptors.attr_keys_by_owner, obj, &key);
            }
        }
        // Non-meta-capable owner: no summary to consult — keep the scans.
        None => {
            owner_index_add(&st.descriptors.accessor_keys_by_owner, obj, &key);
            owner_index_add(&st.descriptors.attr_keys_by_owner, obj, &key);
        }
    }
    st.descriptors
        .accessor_descriptors
        .borrow_mut()
        .insert((obj, key.clone()), acc);
    st.descriptors
        .property_descriptors
        .borrow_mut()
        .insert((obj, key), attrs);
}

/// [`owner_index_add`] minus the dedupe scan, for a key
/// [`install_fresh_accessor_property`] has PROVEN absent via the meta
/// summary. Never call without that proof — a duplicate push would make
/// enumeration report the key twice.
fn owner_index_push_proven_new(
    index: &RefCell<FastKeyHashMap<usize, Vec<String>>>,
    owner: usize,
    key: &str,
) {
    index
        .borrow_mut()
        .entry(owner)
        .or_default()
        .push(key.to_string());
}

/// [`note_meta_descriptor_key`] for both kinds in ONE meta access, returning
/// each kind bit's PRIOR state `(accessor_bit_was_set, attr_bit_was_set)` —
/// `None` for a non-meta-capable owner (nothing recorded, matching the
/// single-kind form's no-op arm).
fn note_meta_descriptor_key_both(owner: usize, key: &str) -> Option<(bool, bool)> {
    unsafe {
        // No-move window: the ensure allocates (see
        // `note_meta_descriptor_key`).
        let _no_gc = crate::gc::GcSuppressScope::new();
        let meta = descriptor_summary_meta_ensure(owner)?;
        let bit = descriptor_key_bit(key);
        let accessor_bit_was_set = (*meta).accessor_key_bits & bit != 0;
        let attr_bit_was_set = (*meta).attr_key_bits & bit != 0;
        (*meta).accessor_key_bits |= bit;
        (*meta).attr_key_bits |= bit;
        Some((accessor_bit_was_set, attr_bit_was_set))
    }
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
            crate::object::shapes::transition_object_shape_semantics_for_descriptor_removal(
                object,
                key.as_bytes(),
                true,
            );
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
    note_young_descriptor_owner(st, obj, Some(&acc));
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
    note_young_descriptor_owner(st, obj, None);
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
