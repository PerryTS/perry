//! Charter step 3, accessor stage S2: a declared class's instance accessors
//! (`get x() {}` / `set x(v) {}` in a ClassBody) are REAL accessor properties
//! of its declared prototype object.
//!
//! The decl prototype (`class_decl_prototype_value`) is built at first demand
//! — a dynamic heritage and the per-agent realm both need the class
//! definition to have run — and installs every own member in ClassBody
//! order: methods as data properties, accessors as accessor properties whose
//! pair (`accessor_pair.rs`) holds both forms of each half: the closure
//! reflection hands out and the compiled `fn(this)` / `fn(this, v)` entry an
//! inline cache calls with the receiver as `this`.
//!
//! A registration that arrives after the prototype exists (a computed key is
//! registered when the class definition evaluates) installs onto it here, so
//! the prototype and the registration never disagree.

use super::*;
use crate::object::accessor_pair::{own_accessor, Accessor};

/// Install — or refresh, when a half arrives later — the accessor `name` of
/// `class_id` on its decl prototype `proto`, from the class's registration.
/// A half whose compiled entry is unchanged keeps its closure, so reflection
/// hands out the same function object every time.
pub(crate) fn install_decl_prototype_accessor(proto: *mut ObjectHeader, class_id: u32, name: &str) {
    if proto.is_null() {
        return;
    }
    let Some((raw_get, raw_set)) = class_own_accessor_ptrs(class_id, name) else {
        return;
    };
    // A half arriving later keeps whatever attributes the accessor has now
    // (a `defineProperty` may have changed them); a fresh one takes the
    // ClassBody defaults.
    use crate::object::key_attrs as ka;
    let entry = unsafe { ka::object_key_entry(proto, name.as_bytes()) };
    let (enumerable, configurable) = if entry & ka::ENTRY_ACCESSOR != 0 {
        (
            entry & ka::ENTRY_NON_ENUMERABLE == 0,
            entry & ka::ENTRY_NON_CONFIGURABLE == 0,
        )
    } else {
        class_accessor_attrs(class_id, false, name)
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_raw_mut_ptr(proto);
    let existing = proto_h
        .with_mut_ptr(|p: *mut ObjectHeader| unsafe { own_accessor(p as usize, name.as_bytes()) })
        .unwrap_or_default();
    let half = |raw: usize, have_raw: usize, have: u64, is_setter: bool| -> u64 {
        if raw == 0 {
            0
        } else if raw == have_raw && have != 0 {
            have
        } else {
            class_accessor_function_value(raw, is_setter, name).to_bits()
        }
    };
    let get = scope.root_nanbox_u64(half(raw_get, existing.raw_get, existing.get, false));
    let set = half(raw_set, existing.raw_set, existing.set, true);
    let pair = Accessor {
        get: get.get_nanbox_u64(),
        set,
        raw_get,
        raw_set,
    };
    proto_h.with_mut_ptr(|p: *mut ObjectHeader| {
        crate::object::set_builtin_accessor_pair(
            p as usize,
            name.to_string(),
            pair,
            PropertyAttrs::new(true, enumerable, configurable),
        )
    });
}

/// A getter or setter of `class_id` was registered: when this realm already
/// built the class's decl prototype, install it there too.
pub(crate) fn note_instance_accessor_registered(class_id: u32, name: &str) {
    // A specialization shares its generic's prototype, whose accessors are
    // the generic's own registrations.
    if decl_prototype_identity_id(class_id) != class_id {
        return;
    }
    let proto = class_decl_prototype_object(class_id);
    if !proto.is_null() && !class_is_key_deleted(class_id, name) {
        install_decl_prototype_accessor(proto, class_id, name);
    }
}

/// The accessor a read or write of `name` on an instance of `class_id` meets
/// on the prototype chain, with the object that holds it: the class's decl
/// prototype (materialized on demand) and its ancestors, stopping at the first
/// own property named `name` — an accessor answers, a data property shadows
/// (`None`). This is the one lookup the class-accessor readers use (S3).
pub(crate) fn class_proto_accessor(class_id: u32, name: &str) -> Option<(usize, Accessor)> {
    use crate::object::key_attrs as ka;
    let scope = crate::gc::RuntimeHandleScope::new();
    let cur = scope.root_nanbox_f64(class_decl_prototype_value(class_id));
    for _ in 0..10_000 {
        let value = cur.get_nanbox_f64();
        let js = crate::JSValue::from_bits(value.to_bits());
        if !js.is_pointer() {
            return None;
        }
        let obj = js.as_pointer::<ObjectHeader>();
        if !unsafe { ka::attrs_live_in_keys(obj as usize) } {
            return None;
        }
        let keys = crate::object::object_keys(obj);
        if crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), name.as_bytes())
            .is_some()
        {
            if unsafe { ka::object_key_entry(obj, name.as_bytes()) } & ka::ENTRY_ACCESSOR == 0 {
                return None;
            }
            let acc = unsafe { own_accessor(obj as usize, name.as_bytes()) }?;
            return Some((obj as usize, acc));
        }
        let next = crate::object::js_object_get_prototype_of(cur.get_nanbox_f64());
        cur.set_nanbox_f64(next);
    }
    None
}
