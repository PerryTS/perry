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
        CLASS_ACCESSOR_DEFAULT_ATTRS
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
            let set_length = if is_setter {
                class_own_setter_length(class_id, name, false)
            } else {
                None
            };
            class_accessor_function_value(raw, is_setter, false, name, set_length).to_bits()
        }
    };
    let get = scope.root_nanbox_u64(half(raw_get, existing.raw_get, existing.get, false));
    let set = half(raw_set, existing.raw_set, existing.set, true);
    let pair = Accessor {
        get: get.get_nanbox_u64(),
        set,
        raw_get,
        raw_set,
        static_get: 0,
        static_set: 0,
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
    if decl_prototype_identity_id(class_id) != class_id || name.starts_with('#') {
        return;
    }
    let proto = class_decl_prototype_object(class_id);
    if !proto.is_null() && !class_proto_key_deleted(class_id, name) {
        install_decl_prototype_accessor(proto, class_id, name);
    }
}

/// The object a `C.prototype` ref value (`class_prototype_ref_value`) reflects
/// the accessor key `name` through: the decl prototype of `class_id`
/// (materialized on demand) when the ClassBody declares an accessor `name`
/// that `delete` has not removed, or the prototype holds one now. `getOwnPropertyDescriptor` / `defineProperty`
/// / `delete` on the ref then apply to that object's real property — whatever
/// a `defineProperty` or `delete` has made of it — so the ref and the object
/// never disagree. `None` for every other key.
pub(crate) fn decl_prototype_own_accessor(class_id: u32, name: &str) -> Option<f64> {
    let proto = class_decl_prototype_value(class_id);
    let js = crate::JSValue::from_bits(proto.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let obj = js.as_pointer::<ObjectHeader>();
    // The live shape alone decides whether the key is an accessor. A
    // declaration cannot resurrect a deleted or replaced property.
    let holds = unsafe {
        crate::object::key_attrs::attrs_live_in_keys(obj as usize)
            && crate::object::key_attrs::object_key_is_accessor(obj, name.as_bytes())
    };
    holds.then_some(proto)
}

/// The accessor a read or write of `name` on an instance of `class_id` meets
/// on the prototype chain, with the object that holds it: the class's decl
/// prototype (materialized on demand) and its ancestors, stopping at the first
/// own property named `name` — an accessor answers, a data property shadows
/// (`None`). This is the one lookup the class-accessor readers use (S3).
pub(crate) fn class_proto_accessor(class_id: u32, name: &str) -> Option<(usize, Accessor)> {
    let _no_move = crate::gc::GcSuppressScope::new();
    prototype_accessor(class_decl_prototype_value(class_id), name)
}

/// Resolve an accessor on the actual prototype chain of an instance. The
/// receiver's shape names the start; each holder's shape names its keys and
/// attributes. A data property stops the lookup, including after a relink.
pub(crate) unsafe fn instance_proto_accessor(
    obj: *const ObjectHeader,
    name: &str,
) -> Option<(usize, Accessor)> {
    // The walk runs no user code. Materialization is rare and cannot move
    // these raw pointers; an ordinary miss needs no handle-scope allocation.
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = accessor_prototype(obj)?;
    prototype_accessor(proto, name)
}

/// Follow only the link named by the shape. In particular, a native object's
/// dynamic `constructor` lookup must not re-enter this accessor walk while
/// the realm is still initializing.
unsafe fn accessor_prototype(obj: *const ObjectHeader) -> Option<f64> {
    use crate::object::shapes as sh;
    let word = sh::object_prototype_word(obj);
    if word != 0 {
        let value = crate::JSValue::from_bits(word);
        return (value.is_pointer() && value.as_pointer::<ObjectHeader>() != obj.cast_mut())
            .then_some(f64::from_bits(word));
    }
    match sh::shape_proto_id(sh::object_shape_stamp(obj))? {
        sh::PROTO_ID_NULL => None,
        sh::PROTO_ID_DEFAULT => {
            let addr = crate::array::object_prototype_addr_if_resolved();
            (addr != 0 && addr != obj as usize)
                .then(|| crate::value::js_nanbox_pointer(addr as i64))
        }
        pid if (sh::PROTO_ID_CLASS..sh::PROTO_ID_MIXED).contains(&pid) => {
            let proto = class_decl_prototype_object(pid as u32);
            if proto.is_null() {
                class_decl_prototype_value_for_instance_class(pid as u32)
            } else {
                // The general prototype builder publishes its class link
                // before installing own members and the parent link. Until
                // that link is stamped, the class identity leads back to
                // this same object; it cannot be an ancestor holder.
                (proto != obj.cast_mut()).then(|| crate::value::js_nanbox_pointer(proto as i64))
            }
        }
        _ => None,
    }
}

fn prototype_accessor(start: f64, name: &str) -> Option<(usize, Accessor)> {
    use crate::object::key_attrs as ka;
    let mut cur = start;
    for _ in 0..10_000 {
        let value = cur;
        let js = crate::JSValue::from_bits(value.to_bits());
        if !js.is_pointer() {
            return None;
        }
        let obj = js.as_pointer::<ObjectHeader>();
        if !unsafe { ka::attrs_live_in_keys(obj as usize) } {
            return None;
        }
        // The holder shape owns the logical key count, slot and attributes.
        // Nothing from the class registration participates in the answer.
        let shape = unsafe { crate::object::shapes::object_shape_record(obj) }?;
        let keys = shape.keys() as usize as *const crate::array::ArrayHeader;
        // Most class prototypes contain only data methods. Their shape's
        // summary proves no accessor can answer here, without a name scan.
        // If a farther accessor answers, the second walk proves that none of
        // these skipped data holders shadows it.
        let slot = (shape.summary() & ka::SUMMARY_ACCESSOR != 0)
            .then(|| unsafe {
                ka::keys_find_accessor_slot_resolved(
                    keys,
                    shape.logical_key_count(),
                    name.as_bytes(),
                )
            })
            .flatten();
        if let Some(slot) = slot {
            if unsafe { ka::keys_entry(keys, slot) } & ka::ENTRY_ACCESSOR == 0 {
                return None;
            }
            if unsafe { accessor_is_shadowed(start, obj, name) } {
                return None;
            }
            let acc = unsafe { crate::object::accessor_pair::slot_accessor(obj, slot) };
            return Some((obj as usize, acc));
        }
        let next = unsafe { accessor_prototype(obj) }?;
        cur = next;
    }
    None
}

/// Called only after an accessor was found. No user code ran in either walk;
/// the same shape links lead to its holder. Uncertainty declines the answer.
unsafe fn accessor_is_shadowed(start: f64, holder: *const ObjectHeader, name: &str) -> bool {
    let mut cur = start;
    for _ in 0..10_000 {
        let value = crate::JSValue::from_bits(cur.to_bits());
        if !value.is_pointer() {
            return true;
        }
        let obj = value.as_pointer::<ObjectHeader>();
        if obj == holder {
            return false;
        }
        let Some(shape) = crate::object::shapes::object_shape_record(obj) else {
            return true;
        };
        if crate::object::keys_find_slot_by_bytes_resolved(
            shape.keys() as usize as *const crate::array::ArrayHeader,
            shape.logical_key_count(),
            name.as_bytes(),
        )
        .is_some()
        {
            return true;
        }
        let Some(next) = accessor_prototype(obj) else {
            return true;
        };
        cur = next;
    }
    true
}

/// `[[Get]]` of `name` through the class prototype chain.
/// `None` when no accessor answers (the caller
/// keeps resolving), otherwise the getter's result — `undefined` for a
/// setter-only accessor. `this_of` supplies the receiver and is called only
/// when an accessor answers (it may consume the inherited-read receiver
/// override). A compiled class getter is called directly; a getter installed
/// by `defineProperty` runs as a closure.
///
/// # Safety
/// `this_of` returns a live receiver.
pub(crate) unsafe fn class_chain_getter_value(
    class_id: u32,
    name: &str,
    this_of: impl FnOnce() -> f64,
) -> Option<(crate::JSValue, usize)> {
    let acc = class_proto_accessor(class_id, name)?.1;
    Some(invoke_instance_getter(acc, this_of()))
}

/// Generic instance read, resolved through the receiver's current chain.
/// The receiver override is consumed only when an accessor actually answers.
pub(crate) unsafe fn instance_chain_getter_value(
    obj: *const ObjectHeader,
    name: &str,
    this_of: impl FnOnce() -> f64,
) -> Option<(crate::JSValue, usize)> {
    // Only a bare CLASS link needs this declaration-prototype fallback.
    // Recorded and ordinary prototype links are read by the generic getter's
    // existing inherited-property walk, including their accessor lanes.
    // Asking both paths repeats negative lookups on ordinary shaped objects.
    let pid =
        crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(obj))?;
    if !(crate::object::shapes::PROTO_ID_CLASS..crate::object::shapes::PROTO_ID_MIXED)
        .contains(&pid)
    {
        return None;
    }
    // The generic getter keeps obj raw across its fallback arms. Prototype
    // materialization may allocate but cannot move that receiver here.
    let acc = instance_proto_accessor(obj, name)?.1;
    Some(invoke_instance_getter(acc, this_of()))
}

unsafe fn invoke_instance_getter(acc: Accessor, this: f64) -> (crate::JSValue, usize) {
    if acc.raw_get != 0 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this);
        // The compiled getter reads `this` from its parameter.
        let _boundary = crate::object::prototype_chain::UserCodeResolutionBoundary::enter();
        let f = crate::closure::body_call::js_method_body_fn!(acc.raw_get as *const u8;);
        let v = f(this.get_nanbox_f64());
        return (crate::JSValue::from_bits(v.to_bits()), acc.raw_get);
    }
    if acc.get != 0 {
        return (crate::object::invoke_accessor_getter(acc.get, this), 0);
    }
    (crate::JSValue::undefined(), 0)
}

/// `[[Set]]` of `name` through the class prototype chain.
/// `None` when no accessor answers (the caller
/// keeps resolving), `Some(true)` when a setter ran, `Some(false)` when the
/// accessor has no setter — the write must not create a data property.
/// `this` is the receiver the setter sees.
///
/// # Safety
/// `this` and `value` are live values.
pub(crate) unsafe fn class_chain_setter_apply(
    class_id: u32,
    name: &str,
    this: f64,
    value: f64,
) -> Option<bool> {
    let acc = class_proto_accessor(class_id, name)?.1;
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let value_h = scope.root_nanbox_f64(value);
    invoke_instance_setter(acc, this_h.get_nanbox_f64(), value_h.get_nanbox_f64())
}

/// Generic instance write, resolved through the receiver's current chain.
pub(crate) unsafe fn instance_chain_setter_apply(
    obj: *const ObjectHeader,
    name: &str,
    this: f64,
    value: f64,
) -> Option<bool> {
    let acc = instance_proto_accessor(obj, name)?.1;
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let value_h = scope.root_nanbox_f64(value);
    invoke_instance_setter(acc, this_h.get_nanbox_f64(), value_h.get_nanbox_f64())
}

unsafe fn invoke_instance_setter(acc: Accessor, this: f64, value: f64) -> Option<bool> {
    if acc.raw_set != 0 {
        let f = crate::closure::body_call::js_method_body_fn!(acc.raw_set as *const u8; value);
        let _ = f(this, value);
        return Some(true);
    }
    if acc.set != 0 {
        crate::object::invoke_accessor_setter(acc.set, this, value);
        return Some(true);
    }
    Some(false)
}
