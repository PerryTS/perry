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
use std::sync::{Mutex, OnceLock};

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

static OBJECT_PROTOTYPES: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

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
    let mut slot_addr = 0usize;
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
    get_object_prototypes()
        .lock()
        .ok()
        .and_then(|map| map.get(&obj_ptr).copied())
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
    let Some(proto_bits) = object_static_prototype(obj_ptr) else {
        return None;
    };
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
    let proto = proto_ptr as *const crate::ObjectHeader;
    if !super::is_valid_obj_ptr(proto as *const u8) {
        return None;
    }
    // `js_object_get_field_by_name` handles its own further prototype hops
    // (recorded protos on the proto object), so this is the full walk.
    let v = super::js_object_get_field_by_name(proto, key);
    if v.bits() == 0x7FFC_0000_0000_0001 {
        // undefined — treat as "not present" so callers fall back cleanly.
        None
    } else {
        Some(v)
    }
}
