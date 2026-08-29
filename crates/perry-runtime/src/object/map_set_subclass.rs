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

/// Has any `class X extends Map | Set` instance EVER stashed a backing
/// collection in this process? Same rationale as
/// `promise::subclass::PROMISE_SUBCLASS_EVER`: `subclass_backing_of` costs a
/// key-string alloc plus a full recursive property read per call, and it is
/// reached from generic iteration/dispatch paths in programs that never
/// subclass a collection. Set at the single stash site (the only writer of
/// `BACKING_KEY`). (#7795)
pub(crate) static MAP_SET_SUBCLASS_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
/// Set once an own `[Symbol.iterator]` write or delete is observed on a
/// registered `Map` or `Set`.
///
/// [`plain_collection_default_iteration`] answers a plain `Map`/`Set` with its
/// builtin iterator WITHOUT consulting the symbol tables, so it must not run
/// once the iteration protocol has been tampered with — a patched
/// `Map.prototype[Symbol.iterator]`, or an own `@@iterator` installed on one
/// instance, both have to be observable.
///
/// Keyed to the RECEIVER, not to a prototype: perry models no `Map.prototype`
/// object at all — `symbol::get` emulates `Map.prototype[Symbol.iterator]` by
/// binding `entries` inside the resolver (#2856) — so an own symbol property
/// on the instance is the only channel that can override default iteration,
/// and that is what [`note_iterator_symbol_write`] watches.
pub static ITERATOR_PROTOCOL_TOUCHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Record that a plain collection's own `[Symbol.iterator]` has been written,
/// so [`plain_collection_default_iteration`] stops claiming receivers.
///
/// Narrowed to writes whose RECEIVER is a registered `Map` or `Set`. The first
/// version flipped on any `@@iterator` write anywhere, which made the lane
/// dead code in practice: a class that merely *defines* `[Symbol.iterator]` as
/// a method — an ordinary iterable class, and the ECS corpus has two — tripped
/// it at class-definition time and disabled the lane process-wide.
///
/// Receiver-narrowing is sound here because perry models no `Map.prototype`
/// OBJECT: `symbol::get` emulates `Map.prototype[Symbol.iterator]` by binding
/// `entries` (and `Set`'s by binding `values`) inside the resolver itself
/// (#2856). There is therefore nothing to patch on a prototype, and the only
/// channel that can override a plain collection's default iteration is an OWN
/// symbol property on that instance — which is exactly what this hook sees.
/// A Map/Set SUBCLASS instance is an ordinary object, not a registered
/// collection, so the lane already declines it and the subclass arm decides.
pub fn note_iterator_symbol_write(obj_key: usize, sym_key: usize) {
    if sym_key == 0
        || obj_key == 0
        || ITERATOR_PROTOCOL_TOUCHED.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    if !crate::map::is_registered_map(obj_key) && !crate::set::is_registered_set(obj_key) {
        return;
    }
    if sym_key == crate::symbol::well_known_symbol("iterator") as usize {
        ITERATOR_PROTOCOL_TOUCHED.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// A PLAIN (non-subclass) `Map` or `Set` receiver's default iteration target.
///
/// `subclass_backing_of` answers only instances that carry a hidden backing
/// field, so a plain collection fell through every arm of `js_get_iterator` to
/// the generic `[Symbol.iterator]` property lookup plus a dynamic method call.
/// On a profile of small-collection `for…of` those two were ~81% of GetIterator,
/// and `for (const v of set)` over a 4-element Set measured 10.6x node.
///
/// Returns `None` once [`ITERATOR_PROTOCOL_TOUCHED`] is set, so a program that
/// patches `@@iterator` keeps the fully general path.
pub(crate) fn plain_collection_default_iteration(value: f64) -> Option<CollectionBacking> {
    if ITERATOR_PROTOCOL_TOUCHED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let jsv = JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return None;
    }
    let raw = (value.to_bits() & POINTER_MASK) as usize;
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

pub(crate) fn subclass_backing_of(value: f64) -> Option<CollectionBacking> {
    // #7795: no Map/Set subclass instance exists, so this cannot return `Some`.
    if !MAP_SET_SUBCLASS_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
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

/// #7570 — resolve a raw Map/Set RECEIVER address that is NOT a genuine
/// `MapHeader`/`SetHeader` to the collection the operation must actually run
/// on. `want` selects which backing kind the caller can use.
///
/// Why this exists: codegen decides "this receiver is a Map" from the
/// **declared** TypeScript type of the binding (`is_map_expr` /
/// `Type::Generic { base: "Map" }`), then emits a raw `js_map_*` call whose
/// first act is to dereference the receiver as a `MapHeader`. A declared type
/// is a hint, never a layout fact (CLAUDE.md, *Known Limitations*: annotations
/// are erased, nothing validates them at runtime), so any binding annotated
/// with the BASE type — `const m: Map<K, V> = new MyMap()`, a parameter, a
/// class field, a return type, an `as Map<…>` cast — can be holding a
/// SUBCLASS instance, which perry models as a plain `ObjectHeader`. The two
/// headers overlay field-for-field, so `entries: *mut f64` reads
/// `parent_class_id ‖ field_count` — two `u32` class ids glued into a pointer
/// — and the first `.set()` stores through it (SIGBUS).
///
/// The unannotated path never had this problem because it dispatches through
/// [`subclass_backing_of`]. This is the same redirect, performed at the raw
/// runtime entry points so it is **fail-closed**: it covers every binding form
/// and every future caller, rather than one predicate at a time.
///
/// Returns `0` for an object that is not a Map/Set subclass instance (a plain
/// object mis-annotated as a native collection), so the caller degrades to its
/// existing null handling — `undefined` / `0` / `false` — instead of
/// dereferencing a forged pointer.
///
/// Marked `#[cold]`/`#[inline(never)]`: the genuine-header fast path never
/// reaches here, and keeping the body out of line preserves the inlined
/// receiver check at the ~57 `js_map_*` / `js_set_*` entry points.
///
/// # This ALLOCATES, and its callers hold unrooted JSValue args
///
/// [`subclass_backing_of`] builds the hidden field's key with
/// `js_string_from_bytes`, so reaching this arm is a collection point — and it
/// runs at the TOP of e.g. `js_map_set`, before that function roots its `key` /
/// `value` params. The exposure is the #7213 shape, and it is closed by the same
/// accident described in `string/alloc.rs`: an allocation here reaches the
/// alloc-point arm of `gc_check_trigger`, which takes
/// `ManualGcScanGuard::force_full_scan`, and a forced conservative stack scan
/// makes the copying minor ineligible. So the collection this can cause never
/// MOVES anything, and the same conservative scan finds the raw args on the
/// native stack.
///
/// Recorded rather than pre-emptively fixed, for two reasons. The shape is
/// already load-bearing on hotter paths — `native_call_method`'s
/// `collection_methods.rs` calls `subclass_backing_of` on every native method
/// call on an object, and `field_get_set/get_field_by_name.rs` on every `.size`
/// read — so this adds no NEW class of exposure. And the obvious fix (a
/// thread-local caching the interned key `StringHeader`) is itself an unrooted
/// runtime cache of a heap pointer, the invisible-root hazard CLAUDE.md warns
/// about, which would have to be registered with
/// `gc_register_mutable_root_scanner` to be sound. If #7213's premise ever
/// changes — if the alloc-point arm stops forcing a conservative scan — this
/// call site must be revisited together with the two above.
#[cold]
#[inline(never)]
pub(crate) fn redirect_collection_receiver(addr: usize, want: CollectionKind) -> usize {
    let boxed = f64::from_bits(JSValue::pointer(addr as *const u8).bits());
    match (subclass_backing_of(boxed), want) {
        (Some(CollectionBacking::Map(m)), CollectionKind::Map) => m as usize,
        (Some(CollectionBacking::Set(s)), CollectionKind::Set) => s as usize,
        _ => 0,
    }
}

/// Which backing kind a [`redirect_collection_receiver`] caller can use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CollectionKind {
    Map,
    Set,
}

/// True when a Map/Set subclass INSTANCE carries a USER `[Symbol.iterator]`
/// override anywhere on its class/prototype chain — an own
/// `inst[Symbol.iterator] = …`, a symbol accessor, or a class method
/// `*[Symbol.iterator]()` (registered under the synthetic `@@iterator` name).
/// The backing-store iteration shortcuts must defer to such an override and only
/// synthesize the built-in default iterator when none exists. Returns `false`
/// for non-subclass values.
pub(crate) fn subclass_has_iterator_override(value: f64) -> bool {
    // #7795: only ever asked about Map/Set SUBCLASS instances; with no subclass
    // in the process the answer is `false` without the symbol lookups below.
    if !MAP_SET_SUBCLASS_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
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

    // #7795: arm the probe gate before the field exists, so no reader can
    // observe a stashed backing while the flag still says "never".
    MAP_SET_SUBCLASS_EVER.store(true, std::sync::atomic::Ordering::Relaxed);
    let key = crate::string::js_string_from_bytes(BACKING_KEY.as_ptr(), BACKING_KEY.len() as u32);
    let backing_bits = JSValue::pointer(backing_ptr as *const u8).bits();
    js_object_set_field_by_name(obj, key, f64::from_bits(backing_bits));
    this
}

/// #7570 — the receiver-resolution contract for the raw `js_map_*` / `js_set_*`
/// entry points.
///
/// These are *sabotage* tests, not smoke tests: each one first asserts that the
/// header word the pre-fix code would have misread is still sitting there at
/// `MapHeader.size`'s offset, and only then that the entry point returns the
/// resolved answer instead. A green run therefore proves the redirect fired,
/// not merely that nothing crashed.
///
/// #8113 moved which word that is: `ObjectHeader::object_type` is gone, so
/// offset 0 — `MapHeader.size` / `SetHeader.size` — is now `class_id`. The
/// misread value changed from a constant 1 to the receiver's class id; the
/// hazard, and therefore the sabotage, is identical.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::js_object_alloc;

    fn boxed(obj: *mut ObjectHeader) -> f64 {
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    }

    fn undefined() -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    /// A `class X extends Map {}` instance, built the way `super()` builds it.
    fn map_subclass_instance() -> *mut ObjectHeader {
        let obj = js_object_alloc(9001, 2);
        js_map_set_subclass_init(boxed(obj), 0, undefined());
        obj
    }

    /// A `class X extends Set {}` instance.
    fn set_subclass_instance() -> *mut ObjectHeader {
        let obj = js_object_alloc(9002, 2);
        js_map_set_subclass_init(boxed(obj), 1, undefined());
        obj
    }

    #[test]
    fn a_genuine_map_takes_the_fast_path_and_is_never_redirected() {
        let map = crate::map::js_map_alloc(4);
        assert_eq!(
            crate::map::resolve_map_receiver(map) as usize,
            map as usize,
            "a real MapHeader must resolve to itself"
        );
        // Assert the subject was live: the redirect is what would have to have
        // produced this answer if the GC_TYPE_MAP fast path had NOT fired, and
        // it cannot — it yields 0 for a non-object. So the identity above came
        // from the fast path, not from a redirect that happened to agree.
        assert_eq!(
            redirect_collection_receiver(map as usize, CollectionKind::Map),
            0,
            "the redirect must not claim a genuine Map"
        );

        let set = crate::set::js_set_alloc(4);
        assert_eq!(
            crate::set::resolve_set_receiver(set) as usize,
            set as usize,
            "a real SetHeader must resolve to itself"
        );
        assert_eq!(
            redirect_collection_receiver(set as usize, CollectionKind::Set),
            0,
            "the redirect must not claim a genuine Set"
        );
    }

    #[test]
    fn a_map_subclass_instance_is_redirected_onto_its_backing() {
        let obj = map_subclass_instance();
        let backing = match subclass_backing_of(boxed(obj)) {
            Some(CollectionBacking::Map(m)) => m,
            _ => panic!("super() should have installed a Map backing"),
        };
        assert_ne!(backing as usize, obj as usize);

        // The pre-fix hazard, still present in the bytes: `MapHeader.size`
        // overlays `ObjectHeader.class_id` (#8113), so `js_map_size` used to
        // report the class id for an EMPTY subclass instance and
        // `MapHeader.entries` was the shape word.
        assert_eq!(unsafe { (*obj).class_id }, 9001);
        assert_eq!(
            js_map_size_of(obj),
            0,
            "an empty Map subclass instance must report size 0, not class_id"
        );

        // Writes land in the backing; the receiver is what comes back.
        let returned = crate::map::js_map_set(obj as *mut crate::map::MapHeader, 1.0, 2.0);
        assert_eq!(
            returned as usize, obj as usize,
            "Map.prototype.set returns the RECEIVER, not the hidden backing"
        );
        assert_eq!(crate::map::js_map_size(backing), 1);
        assert_eq!(js_map_size_of(obj), 1);
        assert_eq!(
            crate::map::js_map_get(obj as *const crate::map::MapHeader, 1.0),
            2.0
        );
        // The instance header is untouched — no forged-pointer store landed in
        // it, and it is still an ordinary object.
        assert_eq!(unsafe { (*obj).class_id }, 9001);
        assert!(unsafe { crate::object::object_is_regular(obj) });
    }

    #[test]
    fn a_set_subclass_instance_is_redirected_onto_its_backing() {
        let obj = set_subclass_instance();
        let backing = match subclass_backing_of(boxed(obj)) {
            Some(CollectionBacking::Set(s)) => s,
            _ => panic!("super() should have installed a Set backing"),
        };
        assert_ne!(backing as usize, obj as usize);
        assert_eq!(unsafe { (*obj).class_id }, 9002);
        assert_eq!(
            crate::set::js_set_size(obj as *const crate::set::SetHeader),
            0,
            "an empty Set subclass instance must report size 0, not class_id"
        );

        let returned = crate::set::js_set_add(obj as *mut crate::set::SetHeader, 7.0);
        assert_eq!(
            returned as usize, obj as usize,
            "Set.prototype.add returns the RECEIVER, not the hidden backing"
        );
        assert_eq!(crate::set::js_set_size(backing), 1);
        assert_eq!(
            crate::set::js_set_has(obj as *const crate::set::SetHeader, 7.0),
            1
        );
        assert_eq!(
            crate::set::js_set_has(obj as *const crate::set::SetHeader, 8.0),
            0
        );
        assert_eq!(unsafe { (*obj).class_id }, 9002);
    }

    /// A plain object merely ANNOTATED `Map<K, V>` / `Set<T>` — the second way
    /// a declared type lies about layout. There is nothing to redirect to, so
    /// the entry points must degrade through their null branch rather than
    /// treat `parent_class_id ‖ field_count` as an `entries` pointer.
    #[test]
    fn a_plain_object_annotated_as_a_collection_forges_no_pointer() {
        let obj = js_object_alloc(9003, 3);
        assert!(subclass_backing_of(boxed(obj)).is_none());
        assert_eq!(
            redirect_collection_receiver(obj as usize, CollectionKind::Map),
            0
        );
        assert_eq!(
            redirect_collection_receiver(obj as usize, CollectionKind::Set),
            0
        );

        // Pre-fix these read the ObjectHeader as a MapHeader: `size` was
        // `class_id` (#8113; `object_type` before that) and the very next
        // `.set()` stored through the shape word.
        assert_eq!(unsafe { (*obj).class_id }, 9003);
        assert_eq!(js_map_size_of(obj), 0);
        assert_eq!(
            crate::map::js_map_get(obj as *const crate::map::MapHeader, 1.0).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        let returned = crate::map::js_map_set(obj as *mut crate::map::MapHeader, 1.0, 2.0);
        assert_eq!(returned as usize, obj as usize);
        assert_eq!(
            crate::set::js_set_size(obj as *const crate::set::SetHeader),
            0
        );
        assert_eq!(
            crate::set::js_set_has(obj as *const crate::set::SetHeader, 1.0),
            0
        );
        crate::set::js_set_clear(obj as *mut crate::set::SetHeader);

        // Nothing wrote into the object's header.
        assert_eq!(unsafe { (*obj).class_id }, 9003);
        assert!(crate::object::shapes::is_shape_id(unsafe {
            (*obj).parent_class_id
        }));
        assert_eq!(unsafe { crate::object::object_live_slot_count(obj) }, 3);
    }

    fn js_map_size_of(obj: *mut ObjectHeader) -> u32 {
        crate::map::js_map_size(obj as *const crate::map::MapHeader)
    }
}

#[cfg(test)]
mod plain_collection_lane_tests {
    use super::*;

    /// The lane must claim a plain Map and a plain Set — those are the
    /// receivers `for…of` hands `js_get_iterator` — and refuse everything it
    /// cannot prove, so the general path still decides those.
    #[test]
    fn lane_claims_plain_collections_only() {
        ITERATOR_PROTOCOL_TOUCHED.store(false, std::sync::atomic::Ordering::Relaxed);

        let map = crate::map::js_map_alloc(4);
        let map_value = crate::value::js_nanbox_pointer(map as i64);
        assert!(
            matches!(
                plain_collection_default_iteration(map_value),
                Some(CollectionBacking::Map(_))
            ),
            "a plain Map must take the builtin-iterator lane"
        );

        let set = crate::set::js_set_alloc(4);
        let set_value = crate::value::js_nanbox_pointer(set as i64);
        assert!(
            matches!(
                plain_collection_default_iteration(set_value),
                Some(CollectionBacking::Set(_))
            ),
            "a plain Set must take the builtin-iterator lane"
        );

        // Not a collection, and not a pointer: both decline.
        let plain = crate::object::js_object_alloc(0, 0);
        assert!(
            plain_collection_default_iteration(crate::value::js_nanbox_pointer(plain as i64))
                .is_none(),
            "an ordinary object is not a plain collection"
        );
        assert!(plain_collection_default_iteration(37.0).is_none());
    }

    /// The whole safety argument: once any `@@iterator` write is observed the
    /// lane must stop claiming receivers, so a patched iteration protocol stays
    /// observable through the general lookup. The latch is monotone.
    #[test]
    fn a_touched_iterator_protocol_disables_the_lane() {
        ITERATOR_PROTOCOL_TOUCHED.store(false, std::sync::atomic::Ordering::Relaxed);
        let map = crate::map::js_map_alloc(4);
        let map_value = crate::value::js_nanbox_pointer(map as i64);
        assert!(plain_collection_default_iteration(map_value).is_some());

        note_iterator_symbol_write(
            map as usize,
            crate::symbol::well_known_symbol("iterator") as usize,
        );
        assert!(
            ITERATOR_PROTOCOL_TOUCHED.load(std::sync::atomic::Ordering::Relaxed),
            "an own @@iterator write on a Map must flip the latch"
        );
        assert!(
            plain_collection_default_iteration(map_value).is_none(),
            "with the protocol touched, the lane must decline and let the \
             general [Symbol.iterator] lookup decide"
        );

        // An unrelated symbol must not flip it (checked from the clean state).
        ITERATOR_PROTOCOL_TOUCHED.store(false, std::sync::atomic::Ordering::Relaxed);
        note_iterator_symbol_write(
            map as usize,
            crate::symbol::well_known_symbol("asyncIterator") as usize,
        );
        assert!(
            !ITERATOR_PROTOCOL_TOUCHED.load(std::sync::atomic::Ordering::Relaxed),
            "only @@iterator disables the lane"
        );

        // The regression this narrowing fixes: an ORDINARY class that merely
        // DEFINES `[Symbol.iterator]` as a method must not disable the lane.
        // Its receiver is a class prototype object, not a registered
        // collection. The first version flipped on any receiver, which made
        // the lane dead code in every program containing an iterable class.
        let ordinary = crate::object::js_object_alloc(0, 0);
        note_iterator_symbol_write(
            ordinary as usize,
            crate::symbol::well_known_symbol("iterator") as usize,
        );
        assert!(
            !ITERATOR_PROTOCOL_TOUCHED.load(std::sync::atomic::Ordering::Relaxed),
            "defining @@iterator on a non-collection must leave the lane armed"
        );
        assert!(
            plain_collection_default_iteration(map_value).is_some(),
            "and the lane must still claim a plain Map afterwards"
        );
        ITERATOR_PROTOCOL_TOUCHED.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
