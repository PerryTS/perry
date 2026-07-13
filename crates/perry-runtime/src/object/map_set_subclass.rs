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
pub(crate) const BACKING_KEY: &[u8] = b"__perry_collection_backing__";

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
    // `this` can be a raw, header-less collection/buffer handle (a real Map/Set,
    // a Buffer, or a typed array) when this runs before raw collection dispatch.
    // Those allocations carry no `GcHeader`, so reading `raw - GC_HEADER_SIZE`
    // would crash or misclassify allocator metadata. Magnitude-classify the
    // address (rejecting the handle band + slab allocations) before any header
    // read, and reject registered non-object collections outright.
    if crate::map::is_registered_map(raw)
        || crate::set::is_registered_set(raw)
        || crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(raw)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
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

/// True when a Map/Set subclass INSTANCE carries a USER `[Symbol.iterator]`
/// override anywhere on its class/prototype chain — an own
/// `inst[Symbol.iterator] = …`, a symbol accessor, or a class method
/// `*[Symbol.iterator]()` (registered under the synthetic `@@iterator` name).
/// The backing-store iteration shortcuts must defer to such an override and only
/// synthesize the built-in default iterator when none exists. Returns `false`
/// for non-subclass values.
pub(crate) fn subclass_has_iterator_override(value: f64) -> bool {
    unsafe {
        let Some(obj) = instance_object_ptr(value) else {
            return false;
        };
        let iter_wk = crate::symbol::well_known_symbol("iterator");
        if iter_wk.is_null() {
            return false;
        }
        let iter_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
        // Own symbol property or symbol accessor on the instance.
        if crate::symbol::own_symbol_property(value, iter_f64).is_some() {
            return true;
        }
        // Class-method override `*[Symbol.iterator]()` anywhere on the chain.
        // The built-in Map/Set iterator is a runtime default, NOT a class vtable
        // method, so a hit here means the user declared one.
        let class_id = crate::object::js_object_get_class_id(obj);
        if class_id != 0 && crate::object::method_owner_class_id(class_id, "@@iterator").is_some() {
            return true;
        }
        false
    }
}

/// Like [`subclass_backing_of`] but returns the backing only when there is NO
/// user `[Symbol.iterator]` override — so the iteration fast paths fall through
/// to the normal iterator protocol when the user overrode `@@iterator`.
pub(crate) fn subclass_backing_for_default_iteration(value: f64) -> Option<CollectionBacking> {
    if subclass_has_iterator_override(value) {
        return None;
    }
    subclass_backing_of(value)
}

/// `super.<method>(…)` from inside a `class X extends Map | Set` OVERRIDE
/// (#6325).
///
/// The other native bases perry models — `EventEmitter`, the `node:stream`
/// classes — install their surface as method CLOSURES on the instance, so an
/// override displaces a real value that `super.<m>()` can still reach
/// (`node_stream::displaced_native_base_method`, #6316/#6322). Map/Set have no
/// such closures: their surface is served by redirecting the OPERATION onto the
/// hidden backing collection at each dispatch point. There is therefore nothing
/// for `js_super_method_call_dynamic` to find, and `super.get(k)` returned
/// `undefined` — the base was unreachable from an override. Run the base
/// operation on the backing directly instead.
///
/// Returns `None` for a receiver with no backing, and for any name that is not a
/// base collection method, so an ordinary `super.m()` miss still yields
/// `undefined` (the #774 instance-field-shadow contract).
pub(crate) fn super_collection_method(this_value: f64, name: &str, args: &[f64]) -> Option<f64> {
    let backing = subclass_backing_of(this_value)?;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    // `js_map_set` / `js_set_add` allocate (entry storage, boxed keys) and can
    // therefore GC-move the RECEIVER — which `Map.prototype.set` and
    // `Set.prototype.add` must RETURN, so a stale bit pattern here would hand
    // the override a dead `this`. Root it and re-read from the handle after the
    // call. The backing `MapHeader`/`SetHeader` needs no handle: it is a
    // registered, header-less allocation the GC never moves (the same reason the
    // raw-collection dispatch in `native_call_method` holds it across calls).
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this_value);
    let boxed = |ptr: i64| f64::from_bits(JSValue::pointer(ptr as *const u8).bits());
    let boolean = |b: bool| f64::from_bits(JSValue::bool(b).bits());
    unsafe {
        match backing {
            CollectionBacking::Map(map) => match name {
                "get" => Some(crate::map::js_map_get(map, *args.first()?)),
                "set" => {
                    let key = *args.first()?;
                    let value = args.get(1).copied().unwrap_or(undefined);
                    crate::map::js_map_set(map, key, value);
                    Some(this_handle.get_nanbox_f64())
                }
                "has" => Some(boolean(crate::map::js_map_has(map, *args.first()?) != 0)),
                "delete" => Some(boolean(crate::map::js_map_delete(map, *args.first()?) != 0)),
                "clear" => {
                    crate::map::js_map_clear(map);
                    Some(undefined)
                }
                "forEach" => {
                    let callback = *args.first()?;
                    let this_arg = args.get(1).copied().unwrap_or(undefined);
                    // The callback's 3rd argument must be the SUBCLASS instance,
                    // not the backing — same receiver-identity rule the ordinary
                    // dispatch path applies.
                    crate::map::js_map_foreach_with_collection(
                        map,
                        callback,
                        this_arg,
                        this_handle.get_nanbox_f64(),
                    );
                    Some(undefined)
                }
                "keys" => Some(boxed(crate::collection_iter_object::js_map_keys_iter_obj(
                    map,
                ))),
                "values" => Some(boxed(
                    crate::collection_iter_object::js_map_values_iter_obj(map),
                )),
                "entries" | "Symbol.iterator" | "@@iterator" => Some(boxed(
                    crate::collection_iter_object::js_map_entries_iter_obj(map),
                )),
                _ => None,
            },
            CollectionBacking::Set(set) => match name {
                "add" => {
                    crate::set::js_set_add(set, *args.first()?);
                    Some(this_handle.get_nanbox_f64())
                }
                "has" => Some(boolean(crate::set::js_set_has(set, *args.first()?) != 0)),
                "delete" => Some(boolean(crate::set::js_set_delete(set, *args.first()?) != 0)),
                "clear" => {
                    crate::set::js_set_clear(set);
                    Some(undefined)
                }
                "forEach" => {
                    let callback = *args.first()?;
                    let this_arg = args.get(1).copied().unwrap_or(undefined);
                    crate::set::js_set_foreach_with_collection(
                        set,
                        callback,
                        this_arg,
                        this_handle.get_nanbox_f64(),
                    );
                    Some(undefined)
                }
                // `Set.prototype.keys` is an alias of `values`, and the default
                // iterator is `values` — matching the builtin.
                "keys" | "values" | "Symbol.iterator" | "@@iterator" => Some(boxed(
                    crate::collection_iter_object::js_set_values_iter_obj(set),
                )),
                "entries" => Some(boxed(
                    crate::collection_iter_object::js_set_entries_iter_obj(set),
                )),
                _ => None,
            },
        }
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

    // Allocate the backing collection and keep a RAW pointer root live across
    // the key allocation below: `js_string_from_bytes` can allocate and trigger
    // a GC, which would otherwise reclaim/relocate an unrooted backing store
    // before we stash it on the instance.
    let backing_ptr: *mut u8 = if kind == 0 {
        let map = if has_iter {
            crate::map::js_map_from_iterable(iterable)
        } else {
            crate::map::js_map_alloc(0)
        };
        map as *mut u8
    } else {
        let set = if has_iter {
            crate::set::js_set_from_iterable(iterable)
        } else {
            crate::set::js_set_alloc(0)
        };
        set as *mut u8
    };

    let key = crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32);
    let backing_bits = JSValue::pointer(backing_ptr as *const u8).bits();
    js_object_set_field_by_name(obj, key, f64::from_bits(backing_bits));
    this
}
