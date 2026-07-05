//! Observable `[[Prototype]]` side-table for ordinary heap objects (#2820).
//!
//! Perry bakes class IDs at allocation time, so it cannot rewrite an object's
//! baked prototype chain. But `Object.setPrototypeOf(obj, proto)` on an
//! *ordinary* object (a `{}` literal, an `Object.create(...)` result, etc.)
//! must be observable: a later `Object.getPrototypeOf(obj)` returns the same
//! `proto`, and an inherited property read (`obj.x` where `x` lives on `proto`)
//! walks to it.
//!
//! We model this with a thread-local map from the object's heap pointer to the
//! NaN-box bits of its recorded prototype. `proto_bits` for an explicit
//! `Object.setPrototypeOf(obj, null)` is `TAG_NULL`, so a recorded-null entry
//! is distinguishable from "no entry recorded" (default prototype).
//!
//! GC correctness: the recorded prototype is a live reference. The map is
//! visited by `visit_object_static_prototype_slot_mut` (wired into the Object
//! rewrite descriptor in `gc/layout.rs`) so a moving collector rewrites the
//! stored bits, and `object_static_prototype_owner_moved` migrates the entry
//! when the *owner* object itself is evacuated.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Set when `Object.setPrototypeOf` has retargeted a REAL ARRAY's
/// [[Prototype]] anywhere in the program. The typed-feedback array guards
/// consult it (one relaxed load) so the inline raw-slot fast path stands
/// down: holes/OOB reads must then walk the custom chain (test262
/// copyWithin/coerced-values-start-change-*).
static ARRAY_TARGET_PROTO_RECORDED: AtomicBool = AtomicBool::new(false);

pub(crate) fn array_static_proto_recorded() -> bool {
    ARRAY_TARGET_PROTO_RECORDED.load(Ordering::Relaxed)
}

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

static OBJECT_PROTOTYPES: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
/// Latched true by the first recorded `Object.setPrototypeOf`. Lets hot
/// per-object probes (e.g. JSON.stringify's `toJSON` fast-negative check,
/// #6009) skip the map mutex entirely in processes that never re-prototype
/// an object — the overwhelmingly common case.
static OBJECT_PROTOTYPES_NONEMPTY: AtomicBool = AtomicBool::new(false);

fn get_object_prototypes() -> &'static Mutex<HashMap<usize, u64>> {
    OBJECT_PROTOTYPES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record `Object.setPrototypeOf(obj_ptr, proto)`. `proto_bits` is the NaN-box
/// bits of the prototype object (POINTER-tagged) or `TAG_NULL`. Idempotent
/// overwrite.
pub fn object_set_static_prototype(obj_ptr: usize, proto_bits: u64) {
    if obj_ptr == 0 {
        return;
    }
    if !ARRAY_TARGET_PROTO_RECORDED.load(Ordering::Relaxed)
        && obj_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
        && crate::value::addr_class::is_above_handle_band(obj_ptr)
        && crate::object::is_valid_obj_ptr(obj_ptr as *const u8)
    {
        let obj_type = unsafe {
            let hdr =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            ARRAY_TARGET_PROTO_RECORDED.store(true, Ordering::Relaxed);
        }
    }
    let mut slot_addr = 0usize;
    // Latch BEFORE the insert: a concurrent `object_static_prototype` that
    // observed the latch after the insert-but-before-the-store window would
    // skip the mutex and miss an already-recorded prototype.
    OBJECT_PROTOTYPES_NONEMPTY.store(true, Ordering::Release);
    if let Ok(mut map) = get_object_prototypes().lock() {
        let slot = map.entry(obj_ptr).or_insert(0);
        *slot = proto_bits;
        slot_addr = slot as *mut u64 as usize;
    }
    if slot_addr != 0 {
        crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, proto_bits);
    }
}

/// Look up the recorded prototype bits for an object, if any. Returns `None`
/// when no explicit prototype has been recorded (the object still has its
/// default prototype); `Some(TAG_NULL)` when it was explicitly set to `null`.
pub fn object_static_prototype(obj_ptr: usize) -> Option<u64> {
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return None;
    }
    get_object_prototypes()
        .lock()
        .ok()
        .and_then(|map| map.get(&obj_ptr).copied())
}

pub(crate) fn default_object_prototype_bits() -> Option<u64> {
    let object_ctor = super::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_bits = object_ctor.to_bits();
    if (ctor_bits >> 48) != 0x7FFD {
        return None;
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_bits = proto.to_bits();
    if (proto_bits >> 48) == 0x7FFD {
        Some(proto_bits)
    } else {
        None
    }
}

pub(crate) unsafe fn default_object_prototype_for_owner(obj_ptr: usize) -> Option<u64> {
    if obj_ptr == 0 {
        return None;
    }
    let obj = obj_ptr as *const crate::ObjectHeader;
    if !super::is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    let gc = super::gc_header_for(obj);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
        return None;
    }
    if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT
        || ((*obj).class_id != 0 && !super::is_anon_shape_class_id((*obj).class_id))
    {
        return None;
    }
    let proto_bits = default_object_prototype_bits()?;
    let proto_ptr = (proto_bits & crate::value::POINTER_MASK) as usize;
    if proto_ptr == 0 || proto_ptr == obj_ptr {
        return None;
    }
    Some(proto_bits)
}

/// Migrate the side-table entry when the owner object is evacuated by a moving
/// GC. Mirrors `closure_dynamic_props_owner_moved`.
pub(crate) fn object_static_prototype_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    if let Ok(mut map) = get_object_prototypes().lock() {
        if let Some(proto_bits) = map.remove(&old_owner) {
            map.insert(new_owner, proto_bits);
        }
    }
}

/// GC scanner: visit the stored prototype-value slot for `owner` so a moving
/// collector can rewrite a forwarded prototype pointer. A `TAG_NULL` entry is
/// not a pointer, so the collector simply leaves it unchanged.
pub(crate) fn visit_object_static_prototype_slot_mut(
    owner: usize,
    mut visit: impl FnMut(*mut u64),
) {
    if owner == 0 {
        return;
    }
    if let Ok(mut map) = get_object_prototypes().lock() {
        if let Some(proto_bits) = map.get_mut(&owner) {
            visit(proto_bits as *mut u64);
        }
    }
}

/// Resolve an inherited property read for an object whose own keys did not
/// contain `key`. Walks the recorded prototype chain (bounded to guard against
/// user-induced cycles). Returns `Some(value)` when a prototype in the chain
/// has the key as an own property, else `None` (caller returns `undefined`).
///
/// `key` is the lookup key already known not to be an own property of the
/// starting object. Each hop reads via `js_object_get_field_by_name`, which is
/// the generic own+inherited getter — but because we only enter this walk after
/// an own-key miss, and the proto's own keys are what matters, re-entering the
/// generic getter on the proto naturally continues the chain.
pub(crate) fn resolve_inherited_field(
    obj_ptr: usize,
    key: *const crate::StringHeader,
) -> Option<crate::value::JSValue> {
    let proto_bits = object_static_prototype(obj_ptr)?;
    if proto_bits == TAG_NULL {
        return None;
    }
    let top16 = proto_bits >> 48;
    let proto_ptr = if top16 == 0x7FFD {
        (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && proto_bits > 0x10000 {
        proto_bits as usize
    } else {
        return None;
    };
    if proto_ptr == 0 || proto_ptr == obj_ptr {
        return None;
    }
    // A Proxy prototype (`Object.create(proxy).x`) is a small fake pointer in
    // the proxy id band, which passes the loose `is_valid_obj_ptr` heap-range
    // check below and would then be dereferenced as an `ObjectHeader` — a
    // SIGSEGV. Route the inherited read through the proxy's `[[Get]]` (which
    // fires the get trap or forwards to the target), binding the original
    // instance as the receiver. (test262
    // Proxy/get/trap-is-{null,undefined}-target-is-proxy via
    // `Object.create(proxy)[k]`.)
    {
        let proto_val = f64::from_bits(proto_bits);
        if crate::proxy::js_proxy_is_proxy(proto_val) != 0 {
            if key.is_null() {
                return None;
            }
            let key_val = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
            let receiver =
                f64::from_bits(crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits());
            let previous_this = super::js_implicit_this_set(receiver);
            let v = crate::proxy::js_proxy_get(proto_val, key_val);
            super::js_implicit_this_set(previous_this);
            if v.to_bits() == crate::value::TAG_UNDEFINED {
                return None;
            }
            return Some(crate::value::JSValue::from_bits(v.to_bits()));
        }
    }
    let proto = proto_ptr as *const crate::ObjectHeader;
    if !super::is_valid_obj_ptr(proto as *const u8) {
        return None;
    }
    // `js_object_get_field_by_name` handles its own further prototype hops
    // (recorded protos on the proto object), so this is the full walk. Bind
    // accessor getters to the original receiver while walking inherited
    // properties; otherwise prototype accessors would observe the prototype
    // object instead of the instance.
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits());
    let previous_this = super::js_implicit_this_set(receiver);
    // The recursive `get_field(proto, key)` re-derives the accessor receiver
    // from `proto`; stash the real instance so an inherited getter binds `this`
    // to it, not to the prototype.
    let prev_override = super::field_get_set::accessor_receiver_override_begin(receiver);
    let v = super::js_object_get_field_by_name(proto, key);
    super::field_get_set::accessor_receiver_override_end(prev_override);
    super::js_implicit_this_set(previous_this);
    if v.bits() == 0x7FFC_0000_0000_0001 {
        // undefined — treat as "not present" so callers fall back cleanly.
        None
    } else {
        Some(v)
    }
}
