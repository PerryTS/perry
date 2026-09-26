//! Reflective attributes of DECLARED STATIC class accessors (#10480).
//!
//! A ClassBody `static get x() {}` is an own property of the constructor `C`,
//! and a class constructor is a ClassRef value, not an object: its accessors
//! live in `CLASS_STATIC_ACCESSORS`, which records only the two function
//! pointers. This table holds what a generic descriptor
//! (`Object.defineProperty(C, "x", { enumerable: true })`) applied, keyed by
//! `(class_id, name)`; absence means the ClassBody defaults
//! (`enumerable: false`, `configurable: true`).
//!
//! Instance accessors are not here: they are real accessor properties of the
//! class's decl prototype (`decl_accessors.rs`), whose attributes live with
//! the prototype's keys like any other property's.
//!
//! [`static_accessor_attrs_in_use`] lets the enumeration paths skip the
//! lookup with one load. The values are booleans: nothing here is a GC root.

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

crate::perry_thread_local! {
    static STATIC_ACCESSOR_ATTRS: std::cell::RefCell<HashMap<(u32, String), (bool, bool)>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Sticky: set by the first [`set_static_accessor_attrs`]. Only a hint that
/// the table may be non-empty — never cleared, so a stale `true` merely costs
/// a lookup.
static STATIC_ACCESSOR_ATTRS_IN_USE: AtomicBool = AtomicBool::new(false);

/// ClassBody defaults for an accessor: `(enumerable, configurable)`.
pub(crate) const CLASS_ACCESSOR_DEFAULT_ATTRS: (bool, bool) = (false, true);

#[inline]
pub(crate) fn static_accessor_attrs_in_use() -> bool {
    STATIC_ACCESSOR_ATTRS_IN_USE.load(Ordering::Relaxed)
}

/// `(enumerable, configurable)` of the declared static accessor `name`.
pub(crate) fn static_accessor_attrs(class_id: u32, name: &str) -> (bool, bool) {
    if !static_accessor_attrs_in_use() {
        return CLASS_ACCESSOR_DEFAULT_ATTRS;
    }
    STATIC_ACCESSOR_ATTRS.with(|table| {
        table
            .borrow()
            .get(&(class_id, name.to_string()))
            .copied()
            .unwrap_or(CLASS_ACCESSOR_DEFAULT_ATTRS)
    })
}

pub(crate) fn set_static_accessor_attrs(
    class_id: u32,
    name: &str,
    enumerable: bool,
    configurable: bool,
) {
    STATIC_ACCESSOR_ATTRS_IN_USE.store(true, Ordering::Relaxed);
    STATIC_ACCESSOR_ATTRS.with(|table| {
        table
            .borrow_mut()
            .insert((class_id, name.to_string()), (enumerable, configurable));
    });
}

/// Raw `(getter, setter)` func_ptrs of a live own declared static accessor —
/// `None` for a method, a field, an inherited accessor, or one `delete`
/// removed.
pub(crate) fn static_declared_accessor_ptrs(class_id: u32, name: &str) -> Option<(usize, usize)> {
    if class_is_key_deleted(class_id, name) {
        return None;
    }
    class_own_static_accessor_ptrs(class_id, name)
}

/// `Object.getOwnPropertyDescriptor(C, name)` for a declared static accessor.
/// The getter value is rooted across the setter value's allocation.
pub(crate) unsafe fn static_accessor_descriptor(
    class_id: u32,
    name: &str,
    getter: usize,
    setter: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let get = scope.root_nanbox_f64(class_accessor_function_value(getter, false, name));
    let set = class_accessor_function_value(setter, true, name);
    let (enumerable, configurable) = static_accessor_attrs(class_id, name);
    crate::object::descriptors::build_accessor_descriptor(
        get.get_nanbox_f64(),
        set,
        enumerable,
        configurable,
    )
}

/// The class's own declared static accessors that are currently enumerable,
/// in ClassBody order. Empty (without walking the class) unless some static
/// accessor of this class was made enumerable.
pub(crate) fn static_enumerable_accessor_names(class_id: u32) -> Vec<String> {
    if !static_accessor_attrs_in_use() {
        return Vec::new();
    }
    let any = STATIC_ACCESSOR_ATTRS.with(|table| {
        table
            .borrow()
            .iter()
            .any(|((cid, _), &(enumerable, _))| *cid == class_id && enumerable)
    });
    if !any {
        return Vec::new();
    }
    class_own_string_member_names(class_id, true)
        .into_iter()
        .filter(|name| {
            static_declared_accessor_ptrs(class_id, name).is_some()
                && static_accessor_attrs(class_id, name).0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn getter() -> f64 {
        0.0
    }

    extern "C" fn setter(_value: f64) -> f64 {
        0.0
    }

    unsafe fn register(class_id: u32, name: &str, with_setter: bool, order: i64) {
        js_register_class_static_getter(
            class_id as i64,
            name.as_ptr(),
            name.len() as i64,
            getter as *const () as usize as i64,
        );
        if with_setter {
            js_register_class_static_setter(
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
            1,
            order,
        );
    }

    #[test]
    fn unrecorded_static_accessor_keeps_classbody_defaults() {
        assert_eq!(static_accessor_attrs(0x7c48_0001, "never"), (false, true));
    }

    #[test]
    fn static_attrs_are_keyed_by_class_and_name() {
        let cid = 0x7c48_0002;
        set_static_accessor_attrs(cid, "x", true, false);
        assert!(static_accessor_attrs_in_use());
        assert_eq!(static_accessor_attrs(cid, "x"), (true, false));
        assert_eq!(static_accessor_attrs(cid, "y"), (false, true));
        assert_eq!(static_accessor_attrs(cid + 1, "x"), (false, true));
    }

    /// Only live declared static accessors qualify: a deleted one, a name the
    /// class never declared, and a non-enumerable one are all excluded, and
    /// the survivors come back in ClassBody order rather than insertion order.
    #[test]
    fn static_enumerable_accessor_names_follow_classbody_order() {
        let cid = 0x7c48_0003;
        unsafe {
            register(cid, "b", true, 10);
            register(cid, "a", false, 20);
            register(cid, "c", true, 30);
            register(cid, "gone", true, 40);
        }
        set_static_accessor_attrs(cid, "a", true, true);
        set_static_accessor_attrs(cid, "b", true, true);
        set_static_accessor_attrs(cid, "c", false, true);
        set_static_accessor_attrs(cid, "gone", true, true);
        set_static_accessor_attrs(cid, "undeclared", true, true);
        class_mark_key_deleted(cid, "gone");
        assert_eq!(
            static_enumerable_accessor_names(cid),
            vec!["b".to_string(), "a".to_string()]
        );
        assert_eq!(static_declared_accessor_ptrs(cid, "gone"), None);
        assert!(static_declared_accessor_ptrs(cid, "a").is_some_and(|(g, s)| g != 0 && s == 0));
    }
}
