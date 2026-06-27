//! `class X extends Map` / `class X extends Set` — subclass backing support.
//!
//! Perry models a class instance as a plain `ObjectHeader`, not a real exotic
//! Map/Set (`MapHeader`/`SetHeader` are separate, header-less-class allocations).
//! So `super()` to a `Map`/`Set` parent used to be a best-effort no-op, leaving
//! the subclass instance with no collection storage and no `has`/`get`/`set`/…
//! methods — `m.has(k)` threw "has is not a function". NestJS's
//! `ModulesContainer extends Map` (and any user `class … extends Map`) hit this.
//!
//! Fix: `super()` calls `js_map_set_subclass_init`, which allocates a real
//! `MapHeader`/`SetHeader`, optionally seeds it from the constructor's iterable
//! argument, and stashes its NaN-boxed pointer on the instance under a hidden
//! field. Because it is a normal object field, the GC traces + relocates it.
//!
//! The collection method/iterator/`.size` surface is then served by checking
//! for this backing field at the runtime dispatch points (see
//! `subclass_backing_of` callers in `native_call_method`, `for_of`, and
//! `field_get_set`). This is more robust than installing per-instance method
//! closures: it covers method calls, `for…of`, and `.size` reads uniformly.

use crate::map::MapHeader;
use crate::object::{js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader};
use crate::set::SetHeader;
use crate::value::{JSValue, POINTER_MASK};

/// Hidden field on a Map/Set subclass instance holding the NaN-boxed backing
/// `MapHeader`/`SetHeader` pointer.
const BACKING_KEY: &[u8] = b"__perry_collection_backing__";

#[derive(Clone, Copy)]
pub(crate) enum CollectionBacking {
    Map(*mut MapHeader),
    Set(*mut SetHeader),
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn instance_object_ptr(this: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(this);
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    Some(raw as *mut ObjectHeader)
}

/// If `value` is a Map/Set *subclass instance* (a plain object carrying the
/// hidden backing field), return its backing collection. Returns `None` for
/// real Maps/Sets, ordinary objects, and non-objects — so callers fall through
/// to their existing handling.
pub(crate) fn subclass_backing_of(value: f64) -> Option<CollectionBacking> {
    unsafe {
        let obj = instance_object_ptr(value)?;
        let backing = js_object_get_field_by_name_f64(
            obj as *const ObjectHeader,
            crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32),
        );
        let bjs = JSValue::from_bits(backing.to_bits());
        if !bjs.is_pointer() {
            return None;
        }
        let raw = (backing.to_bits() & POINTER_MASK) as usize;
        if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return None;
        }
        if crate::map::is_registered_map(raw) {
            return Some(CollectionBacking::Map(raw as *mut MapHeader));
        }
        if crate::set::is_registered_set(raw) {
            return Some(CollectionBacking::Set(raw as *mut SetHeader));
        }
        None
    }
}

/// `super()` for a `class X extends Map | Set`. `kind`: 0 = Map, 1 = Set.
/// `iterable` is the (optional) first constructor argument; `undefined`/`null`
/// seed an empty collection.
#[no_mangle]
pub extern "C" fn js_map_set_subclass_init(this: f64, kind: i32, iterable: f64) -> f64 {
    let obj = match unsafe { instance_object_ptr(this) } {
        Some(o) => o,
        None => return this,
    };
    let iter_js = JSValue::from_bits(iterable.to_bits());
    let has_iter = !(iter_js.is_undefined() || iter_js.is_null());

    let backing_bits = if kind == 0 {
        let map = if has_iter {
            crate::map::js_map_from_iterable(iterable)
        } else {
            crate::map::js_map_alloc(0)
        };
        JSValue::pointer(map as *const u8).bits()
    } else {
        let set = if has_iter {
            crate::set::js_set_from_iterable(iterable)
        } else {
            crate::set::js_set_alloc(0)
        };
        JSValue::pointer(set as *const u8).bits()
    };

    let key = crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32);
    js_object_set_field_by_name(obj, key, f64::from_bits(backing_bits));
    this
}
