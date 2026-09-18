//! Reflective attributes of DECLARED class accessors (#10480).
//!
//! A ClassBody `get x() {}` / `set x(v) {}` lives in the class vtable
//! (`CLASS_VTABLE_REGISTRY`, or `CLASS_STATIC_ACCESSORS` for `static`), not in
//! the address-keyed descriptor tables `Object.defineProperty` writes. The
//! vtable records only the two function pointers, so the accessor's
//! `[[Enumerable]]` / `[[Configurable]]` were pinned to the ClassBody defaults
//! (`false` / `true`) and nothing could change them.
//!
//! `Object.defineProperties(C.prototype, { x: { enumerable: true } })` is how
//! every WebIDL-generated class (whatwg-url, node-fetch, undici-style
//! polyfills) marks its accessors at module load. That generic descriptor fell
//! through to the ordinary define path, which could not see the class key: it
//! appended a keys-array placeholder with a NEW property's `writable: false`,
//! and that data property on the prototype then rejected every instance write
//! before the class setter could run — while the requested enumerability was
//! never reported.
//!
//! This table holds what a generic descriptor applied, keyed by
//! `(class_id, is_static, name)`. Absence means the ClassBody defaults, so the
//! table stays empty in a program that never redefines a class accessor, and
//! [`class_accessor_attrs_in_use`] lets the enumeration paths skip the lookup
//! with one load. The values are booleans: nothing here is a GC root.

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

crate::perry_thread_local! {
    static CLASS_ACCESSOR_ATTRS: std::cell::RefCell<HashMap<(u32, bool, String), (bool, bool)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Sticky: set by the first [`class_set_accessor_attrs`]. Only a hint that
/// the table may be non-empty — never cleared, so a stale `true` merely costs
/// a lookup.
static CLASS_ACCESSOR_ATTRS_IN_USE: AtomicBool = AtomicBool::new(false);

/// ClassBody defaults for an accessor: `(enumerable, configurable)`.
const CLASS_ACCESSOR_DEFAULT_ATTRS: (bool, bool) = (false, true);

#[inline]
pub(crate) fn class_accessor_attrs_in_use() -> bool {
    CLASS_ACCESSOR_ATTRS_IN_USE.load(Ordering::Relaxed)
}

/// `(enumerable, configurable)` of the declared accessor `name`.
pub(crate) fn class_accessor_attrs(class_id: u32, is_static: bool, name: &str) -> (bool, bool) {
    if !class_accessor_attrs_in_use() {
        return CLASS_ACCESSOR_DEFAULT_ATTRS;
    }
    CLASS_ACCESSOR_ATTRS.with(|table| {
        table
            .borrow()
            .get(&(class_id, is_static, name.to_string()))
            .copied()
            .unwrap_or(CLASS_ACCESSOR_DEFAULT_ATTRS)
    })
}

pub(crate) fn class_set_accessor_attrs(
    class_id: u32,
    is_static: bool,
    name: &str,
    enumerable: bool,
    configurable: bool,
) {
    CLASS_ACCESSOR_ATTRS_IN_USE.store(true, Ordering::Relaxed);
    CLASS_ACCESSOR_ATTRS.with(|table| {
        table.borrow_mut().insert(
            (class_id, is_static, name.to_string()),
            (enumerable, configurable),
        );
    });
}

/// Raw `(getter, setter)` func_ptrs of a live own declared accessor — `None`
/// for a method, a field, an inherited accessor, or one `delete` removed.
pub(crate) fn class_declared_accessor_ptrs(
    class_id: u32,
    is_static: bool,
    name: &str,
) -> Option<(usize, usize)> {
    if class_is_key_deleted(class_id, name) {
        return None;
    }
    if is_static {
        class_own_static_accessor_ptrs(class_id, name)
    } else {
        class_own_accessor_ptrs(class_id, name)
    }
}

/// `Object.getOwnPropertyDescriptor` for a declared accessor. The getter value
/// is rooted across the setter value's allocation.
pub(crate) unsafe fn class_accessor_descriptor(
    class_id: u32,
    is_static: bool,
    name: &str,
    getter: usize,
    setter: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let get = scope.root_nanbox_f64(class_accessor_function_value(getter, false, name));
    let set = class_accessor_function_value(setter, true, name);
    let (enumerable, configurable) = class_accessor_attrs(class_id, is_static, name);
    crate::object::descriptors::build_accessor_descriptor(
        get.get_nanbox_f64(),
        set,
        enumerable,
        configurable,
    )
}

/// The class's own declared accessors that are currently enumerable, in
/// ClassBody order. Empty (without walking the class) unless some accessor of
/// this class was made enumerable.
pub(crate) fn class_enumerable_accessor_names(class_id: u32, is_static: bool) -> Vec<String> {
    if !class_accessor_attrs_in_use() {
        return Vec::new();
    }
    let any = CLASS_ACCESSOR_ATTRS.with(|table| {
        table
            .borrow()
            .iter()
            .any(|(&(cid, st, _), &(enumerable, _))| {
                cid == class_id && st == is_static && enumerable
            })
    });
    if !any {
        return Vec::new();
    }
    class_own_string_member_names(class_id, is_static)
        .into_iter()
        .filter(|name| {
            class_declared_accessor_ptrs(class_id, is_static, name).is_some()
                && class_accessor_attrs(class_id, is_static, name).0
        })
        .collect()
}

/// `Object.keys(C.prototype)` when `physical` is the enumerable key list of the
/// declared-class prototype object of `class_id`: splice in the enumerable
/// declared accessors (which have no physical slot) in [[OwnPropertyKeys]]
/// order — `constructor`, then ClassBody members, then keys added later.
/// Returns `physical` itself when the class has no enumerable accessor.
pub(crate) unsafe fn decl_prototype_keys_with_enumerable_accessors(
    class_id: u32,
    physical: *mut crate::array::ArrayHeader,
) -> *mut crate::array::ArrayHeader {
    let accessors = class_enumerable_accessor_names(class_id, false);
    if accessors.is_empty() || physical.is_null() {
        return physical;
    }
    // Copy the physical names out before the first allocation below moves
    // anything; `physical` is not read again.
    let mut physical_names = Vec::new();
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..crate::array::js_array_length(physical) {
        let key = crate::array::js_array_get(physical, i);
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            if let Ok(name) = std::str::from_utf8(bytes) {
                physical_names.push(name.to_string());
            }
        }
    }
    let mut names: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    };
    if physical_names.iter().any(|name| name == "constructor") {
        push("constructor");
    }
    for name in class_own_string_member_names(class_id, false) {
        if accessors.contains(&name) || physical_names.contains(&name) {
            push(&name);
        }
    }
    for name in &physical_names {
        push(name);
    }
    crate::object::descriptors::sort_property_names_ecma(&mut names);
    let scope = crate::gc::RuntimeHandleScope::new();
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(names.len() as u32));
    for name in names {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let updated = out.with_mut_ptr(|out| {
            crate::array::js_array_push(out, crate::value::JSValue::string_ptr(key))
        });
        out.set_raw_mut_ptr(updated);
    }
    out.with_mut_ptr(|out: *mut crate::array::ArrayHeader| out)
}

/// True when `name` is an enumerable declared accessor of the declared-class
/// prototype at `obj_addr` — the enumeration paths' test for a key with no
/// physical slot. Cheap (one atomic load) until some accessor is redefined.
pub(crate) fn class_prototype_enumerable_accessor(obj_addr: usize, name: &str) -> bool {
    if !class_accessor_attrs_in_use() {
        return false;
    }
    let Some(class_id) = class_id_for_decl_prototype_object(obj_addr) else {
        return false;
    };
    class_declared_accessor_ptrs(class_id, false, name).is_some()
        && class_accessor_attrs(class_id, false, name).0
}

/// `Object.values` / `Object.entries` own-key snapshot for a declared-class
/// prototype with enumerable ClassBody accessors — those keys have no slot in
/// the physical keys array the ordinary snapshot walks. `None` for every other
/// receiver, which keeps the existing walk.
///
/// The list is `Object.keys`', so (unlike the ordinary snapshot) enumerability
/// is settled here rather than re-read per key: a getter that flips a SIBLING
/// accessor's enumerability mid-enumeration is not modelled. Nothing else
/// changes — the per-key `[[Get]]` and its side effects are unaffected.
pub(crate) unsafe fn decl_prototype_enumerable_key_snapshot(
    obj: *const ObjectHeader,
) -> Option<Vec<Vec<u8>>> {
    if !class_accessor_attrs_in_use() {
        return None;
    }
    let class_id = class_id_for_decl_prototype_object(obj as usize)?;
    if class_enumerable_accessor_names(class_id, false).is_empty() {
        return None;
    }
    let keys = crate::object::field_get_set::enumeration::js_object_keys(obj);
    if keys.is_null() {
        return None;
    }
    let mut snapshot = Vec::new();
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..crate::array::js_array_length(keys) {
        let key = crate::array::js_array_get(keys, i);
        if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut scratch) {
            snapshot.push(bytes.to_vec());
        }
    }
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn getter(_this: f64) -> f64 {
        0.0
    }

    extern "C" fn setter(_this: f64, _value: f64) -> f64 {
        0.0
    }

    unsafe fn register(class_id: u32, name: &str, with_setter: bool, order: i64) {
        js_register_class_getter(
            class_id as i64,
            name.as_ptr(),
            name.len() as i64,
            getter as *const () as usize as i64,
        );
        if with_setter {
            js_register_class_setter(
                class_id as i64,
                name.as_ptr(),
                name.len() as i64,
                setter as *const () as usize as i64,
            );
        }
        js_register_class_string_member_order(
            class_id as i64,
            name.as_ptr(),
            name.len() as i64,
            0,
            order,
        );
    }

    #[test]
    fn unrecorded_accessor_keeps_classbody_defaults() {
        assert_eq!(
            class_accessor_attrs(0x7c48_0001, false, "never"),
            (false, true)
        );
    }

    #[test]
    fn attrs_are_keyed_by_static_side_and_name() {
        let cid = 0x7c48_0002;
        class_set_accessor_attrs(cid, false, "x", true, false);
        assert!(class_accessor_attrs_in_use());
        assert_eq!(class_accessor_attrs(cid, false, "x"), (true, false));
        assert_eq!(class_accessor_attrs(cid, true, "x"), (false, true));
        assert_eq!(class_accessor_attrs(cid, false, "y"), (false, true));
    }

    /// Only live declared accessors qualify: a deleted one, a name the class
    /// never declared, and a non-enumerable one are all excluded, and the
    /// survivors come back in ClassBody order rather than insertion order.
    #[test]
    fn enumerable_accessor_names_follow_classbody_order() {
        let cid = 0x7c48_0003;
        unsafe {
            register(cid, "b", true, 10);
            register(cid, "a", false, 20);
            register(cid, "c", true, 30);
            register(cid, "gone", true, 40);
        }
        class_set_accessor_attrs(cid, false, "a", true, true);
        class_set_accessor_attrs(cid, false, "b", true, true);
        class_set_accessor_attrs(cid, false, "c", false, true);
        class_set_accessor_attrs(cid, false, "gone", true, true);
        class_set_accessor_attrs(cid, false, "undeclared", true, true);
        class_mark_key_deleted(cid, "gone");
        assert_eq!(
            class_enumerable_accessor_names(cid, false),
            vec!["b".to_string(), "a".to_string()]
        );
        assert!(class_enumerable_accessor_names(cid, true).is_empty());
        assert_eq!(class_declared_accessor_ptrs(cid, false, "gone"), None);
        assert!(
            class_declared_accessor_ptrs(cid, false, "a").is_some_and(|(g, s)| g != 0 && s == 0)
        );
    }
}
