//! Property / accessor descriptor side-tables and the process-wide hot-path
//! gates that guard them (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use crate::arena::arena_alloc_gc;
use crate::ArrayHeader;
use crate::JSValue;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

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

thread_local! {
    pub(crate) static PROPERTY_DESCRIPTORS: RefCell<HashMap<(usize, String), PropertyAttrs>> = RefCell::new(HashMap::new());
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

thread_local! {
    pub(crate) static ACCESSOR_DESCRIPTORS: RefCell<HashMap<(usize, String), AccessorDescriptor>> = RefCell::new(HashMap::new());
    /// Fast-path gate: `false` when no accessor descriptors have ever been installed
    /// on this thread, so hot `js_object_get_field_by_name` / `set_field_by_name`
    /// can skip the `ACCESSOR_DESCRIPTORS` HashMap lookup entirely.
    pub(crate) static ACCESSORS_IN_USE: Cell<bool> = const { Cell::new(false) };
    /// Fast-path gate for `PROPERTY_DESCRIPTORS` — flipped the first time
    /// `Object.defineProperty` (or freeze/seal via `set_property_attrs`)
    /// installs a per-property descriptor. Lets the hot object-write path
    /// skip the `.to_string()` allocation required to look up a descriptor
    /// that almost never exists.
    pub(crate) static PROPERTY_ATTRS_IN_USE: Cell<bool> = const { Cell::new(false) };
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
/// It flips to 1 the moment either (a) any accessor / property descriptor comes
/// into use — the guard then has to perform descriptor-aware dispatch the inline
/// path doesn't model — or (b) typed-feedback tracing is enabled, where the
/// guard records observations the inline path would silently skip. Both are
/// monotonic ("in use" never reverts), so the flag is set-only.
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
            }
        }
    }
}

/// Look up the property descriptor for (obj, key). Returns None if no entry exists,
/// in which case the JS default `{ writable: true, enumerable: true, configurable: true }` applies.
pub(crate) fn get_property_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    PROPERTY_DESCRIPTORS.with(|m| m.borrow().get(&(obj, key.to_string())).copied())
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

/// Store a property descriptor for (obj, key).
pub(crate) fn set_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    note_descriptor_target(obj);
    PROPERTY_ATTRS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_class_field_inline_guard();
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
    });
}

/// Remove a customized property descriptor for (obj, key), restoring default
/// data-property attributes for subsequent writes and reflection.
pub(crate) fn clear_property_attrs(obj: usize, key: &str) {
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().remove(&(obj, key.to_string()));
    });
}

/// Look up the accessor descriptor (get/set) for (obj, key).
pub(crate) fn get_accessor_descriptor(obj: usize, key: &str) -> Option<AccessorDescriptor> {
    ACCESSOR_DESCRIPTORS.with(|m| m.borrow().get(&(obj, key.to_string())).copied())
}

pub(crate) fn accessor_descriptor_keys_for_obj(obj: usize) -> Vec<String> {
    ACCESSOR_DESCRIPTORS.with(|m| {
        let mut keys = m
            .borrow()
            .keys()
            .filter_map(|(owner, key)| (*owner == obj).then(|| key.clone()))
            .collect::<Vec<_>>();
        keys.sort();
        keys
    })
}

/// #2766: resolve an accessor *getter* closure for `(value, key)` if one is
/// installed (e.g. an object-literal `get x() {…}` or
/// `Object.defineProperty(obj, k, { get })`). Returns the NaN-boxed getter
/// closure bits, or `0` when no getter exists. Used by `Reflect.get(target,
/// key, receiver)` so it can rebind the getter's `this` to the receiver before
/// invoking it. Returns `None` (rather than reading the field) when there is no
/// accessor at all, so the caller falls back to an ordinary field read.
pub(crate) fn reflect_getter_closure_bits(value: f64, key: f64) -> Option<u64> {
    if !ACCESSORS_IN_USE.with(|c| c.get()) {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(key);
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
    let mut current = value;
    // Bounded to guard against a cyclic prototype side-table; real chains are
    // a handful of links deep.
    for _ in 0..10_000 {
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
        if obj_value_has_own_key(current, key) {
            return None;
        }
        let proto = crate::object::js_object_get_prototype_of(current);
        if unsafe { extract_obj_ptr(proto) }.is_null() {
            return None;
        }
        current = proto;
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

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    note_descriptor_target(obj);
    ACCESSORS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    disable_class_field_inline_guard();
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), acc);
    });
}

/// Remove an accessor descriptor for (obj, key), letting ordinary data-property
/// reads and writes use the object's stored field again.
pub(crate) fn clear_accessor_descriptor(obj: usize, key: &str) {
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().remove(&(obj, key.to_string()));
    });
}

/// Install a built-in *reflection-only* accessor descriptor for (obj, key)
/// WITHOUT flipping the process-wide `GLOBAL_DESCRIPTORS_IN_USE` /
/// `ACCESSORS_IN_USE` / `PROPERTY_ATTRS_IN_USE` hot-path gates.
///
/// `Object.getOwnPropertyDescriptor` reads `ACCESSOR_DESCRIPTORS` and
/// `PROPERTY_DESCRIPTORS` *unconditionally*, so the descriptor is fully
/// reflectable — but the hot object get/set paths (which only consult the
/// side tables once a gate has flipped) keep skipping the HashMap lookup.
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
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key.clone()), acc);
    });
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
    });
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
    note_descriptor_target(obj);
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
    });
}

/// Walk the keys array of `obj` and apply the given attribute mask AND filter to every existing key.
/// Used by `Object.freeze` (drops `writable` + `configurable`) and `Object.seal` (drops `configurable`).
pub(crate) unsafe fn mark_all_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    _drop_enumerable: bool,
    drop_configurable: bool,
) {
    let keys = (*obj).keys_array;
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
    PROPERTY_DESCRIPTORS.with(|descriptors| {
        let mut descriptors = descriptors.borrow_mut();
        let needs_rebuild = descriptors
            .keys()
            .any(|(owner, _)| rewrite_descriptor_owner(visitor, *owner) != *owner);
        if needs_rebuild {
            let old = std::mem::take(&mut *descriptors);
            for ((owner, key), attrs) in old {
                let owner = rewrite_descriptor_owner(visitor, owner);
                descriptors.insert((owner, key), attrs);
            }
        }
    });

    ACCESSOR_DESCRIPTORS.with(|descriptors| {
        let mut descriptors = descriptors.borrow_mut();
        let needs_rebuild = descriptors
            .keys()
            .any(|(owner, _)| rewrite_descriptor_owner(visitor, *owner) != *owner);
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
    });
}
