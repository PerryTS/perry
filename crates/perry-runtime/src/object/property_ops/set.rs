//! Ordinary receiver-aware property assignment.
use super::*;

/// Extract a raw heap pointer (48-bit) from either a NaN-boxed value
/// (POINTER_TAG / STRING_TAG) or a raw i64/f64-reinterpreted pointer
/// (module-level globals store Arrays/Objects as raw I64s, not NaN-boxed).
pub(crate) fn extract_pointer(bits: u64) -> u64 {
    let top = bits >> 48;
    // A native id reaches the by-name/handle dispatch as its raw id.
    if top == 0x7FFD || top == 0x7FFF || top == (crate::value::HANDLE_TAG >> 48) {
        bits & POINTER_MASK
    } else if top == 0 {
        // Raw untagged pointer (module-level I64 global).
        bits
    } else {
        0
    }
}

pub(crate) fn small_handle_from_value(value: f64) -> Option<i64> {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == (crate::value::HANDLE_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as i64;
        if raw > 0 && (raw as usize) < crate::value::addr_class::HANDLE_BAND_MAX {
            return Some(raw);
        }
    } else if top == 0 && crate::value::addr_class::is_small_handle(bits as usize) {
        return Some(bits as i64);
    }
    None
}

pub(crate) fn set_handle_property(target: f64, key: f64, value: f64) -> Option<bool> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    let handle = small_handle_from_value(target.get_nanbox_f64())?;
    let Some(name) = key_to_rust_string(key.get_nanbox_f64()) else {
        // A SYMBOL-keyed write on a small native handle (e.g. the
        // @hono/node-server `incoming[wrapBodyStream] = true` on the HTTP
        // IncomingMessage handle). The handle is not a heap ObjectHeader, so
        // it has no field storage; route the write to the per-object symbol
        // side table (keyed by the handle pointer, exactly like a plain
        // object) and report success. Returning `Some(false)` here made
        // strict-mode assignment throw `TypeError: Cannot assign to read only
        // property` and 500 every POST/PUT served by Hono's node adapter.
        if unsafe { crate::symbol::js_is_symbol(key.get_nanbox_f64()) } != 0 {
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    target.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    value.get_nanbox_f64(),
                )
            };
            return Some(true);
        }
        return Some(false);
    };
    if let Some(dispatch) = crate::object::handle_property_set_dispatch() {
        unsafe { dispatch(handle, name.as_ptr(), name.len(), value.get_nanbox_f64()) };
    }
    Some(true)
}

pub(crate) fn target_get_property_key(target: f64, property_key: f64) -> f64 {
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        return unsafe { crate::symbol::js_object_get_symbol_property(target, property_key) };
    }
    let key_ptr =
        crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
    if key_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Class constructors use Perry's INT32-tagged ClassRef representation,
    // not a heap pointer. Preserve those bits exactly as the ordinary dynamic
    // class-property path does; pointer extraction would turn the target into
    // null and make `new Proxy(C, {}).staticMethod` read as `undefined`.
    if crate::object::class_ref_id(target).is_some() {
        return crate::object::js_object_get_field_by_name_f64(
            target.to_bits() as *const crate::ObjectHeader,
            key_ptr,
        );
    }
    let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
}

/// `Reflect.set` with an explicit receiver: OrdinarySet(target, P, V,
/// receiver), boolean result NaN-boxed.
pub(crate) fn reflect_ordinary_set_with_receiver(
    target: f64,
    property_key: f64,
    value: f64,
    receiver: f64,
) -> f64 {
    nanbox_bool(ordinary_set_with_receiver(
        target,
        property_key,
        value,
        receiver,
    ))
}

pub(crate) fn target_set(target: f64, key: f64, value: f64) {
    // #6935: `js_to_property_key` runs a user `Symbol.toPrimitive` / `toString`
    // / `valueOf` for an object key (and allocates for every primitive one), so
    // it can trigger a GC that **evacuates** live objects. Both the `target`
    // receiver and the `value` being written into it were raw NaN-boxed Rust
    // locals across the call — a stale target dropped the write onto a
    // forwarding stub, a stale value stored a dangling pointer in a live
    // object. `target_get` already roots; this write sibling did not.
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_u64(target.to_bits());
    let value_handle = scope.root_nanbox_f64(value);
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    let property_key = scope.root_nanbox_f64(property_key).get_nanbox_f64();
    let target = f64::from_bits(target_handle.get_nanbox_u64());
    let value = value_handle.get_nanbox_f64();
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(target, property_key, value);
        }
        return;
    }
    // #6943 audit: this `js_string_coerce` is provably INERT and needs no
    // rooting. `js_to_property_key` returns either a Symbol — taken by the
    // early return above — or `js_nanbox_string(heap_ptr)`, i.e. an
    // already-heap `STRING_TAG` value, which `js_string_coerce` hands straight
    // back without touching the allocator.
    let key_ptr = crate::builtins::js_string_coerce(property_key) as *const crate::StringHeader;
    if crate::object::class_ref_id(target).is_some() {
        // Preserve the INT32-tagged class-ref bits so class dynamic props
        // land in CLASS_DYNAMIC_PROPS instead of being pointer-extracted to 0.
        if !key_ptr.is_null() {
            crate::object::js_object_set_field_by_name(
                target.to_bits() as *mut crate::ObjectHeader,
                key_ptr,
                value,
            );
        }
        return;
    }
    let obj_addr = extract_pointer(target.to_bits()) as usize;
    if crate::closure::is_closure_ptr(obj_addr) {
        if let Some(name) = key_to_rust_string(property_key) {
            crate::closure::closure_set_dynamic_prop(obj_addr, &name, value);
        }
        return;
    }
    let obj_ptr = obj_addr as *mut crate::ObjectHeader;
    if obj_ptr.is_null() || key_ptr.is_null() {
        return;
    }
    crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, value);
}

pub(crate) fn raw_ptr_from_value(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if top16 == (POINTER_TAG >> 48) {
        bits & POINTER_MASK
    } else if top16 == 0 && bits > 0x10000 {
        bits
    } else {
        return None;
    } as usize;
    // #7531: `value` is a Proxy `target`/`key` candidate -- caller-supplied,
    // so it can be a fetch/zlib/proxy/common-registry handle id. The old
    // floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle band; the
    // local name `raw` also made this invisible to
    // `scripts/addr_class_inventory.py`'s `handle-floor` regex (it only
    // matches `ptr`/`addr`/`bits`-shaped identifiers), so this was debt the
    // ratchet could not see. `array_ptr_from_value` derefs
    // `raw - GC_HEADER_SIZE` right after this with only a magnitude-only
    // `is_valid_obj_ptr` guard in between, so a handle id reached that
    // deref.
    if !crate::value::addr_class::is_above_handle_band(raw) {
        return None;
    }
    Some(raw)
}

pub(crate) fn array_ptr_from_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let raw = raw_ptr_from_value(value)?;
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return None;
    }
    unsafe {
        let gc = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
            || (*gc).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
        {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

pub(crate) fn key_equals(key: f64, name: &[u8]) -> bool {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return false;
    };
    if ptr.is_null() || len as usize != name.len() {
        return false;
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) == name }
}

pub(crate) fn key_is_length(key: f64) -> bool {
    key_equals(key, b"length")
}

/// Does deleting `key` off `target` hit a non-configurable exotic own property
/// that lives outside the ordinary descriptor table? Covers an Array's `length`
/// and a plain (non-arrow, non-bound) function's `prototype` — both are
/// non-configurable, so `Reflect.deleteProperty` / `delete` must report failure.
pub(crate) fn is_non_configurable_exotic_own(target: f64, key: f64) -> bool {
    if array_ptr_from_value(target).is_some() && key_is_length(key) {
        return true;
    }
    if key_equals(key, b"prototype") {
        let raw = extract_pointer(target.to_bits()) as usize;
        if raw != 0 && crate::closure::is_closure_ptr(raw) {
            let closure = raw as *const crate::closure::ClosureHeader;
            // Arrow and bound functions have no own `prototype` slot at all, so
            // deleting it succeeds vacuously; only a plain function's `prototype`
            // is a non-configurable own property.
            if !crate::closure::closure_is_arrow(closure)
                && !crate::closure::closure_is_bound_method(closure)
                && !crate::closure::closure_body_is_non_constructor(closure)
            {
                return true;
            }
        }
    }
    false
}

pub(crate) fn parse_canonical_nonnegative_i32(bytes: &[u8]) -> Option<i32> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut value = 0u32;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((byte - b'0') as u32)?;
        if value > i32::MAX as u32 {
            return None;
        }
    }
    Some(value as i32)
}

pub(crate) fn integer_index_key(key: f64) -> Option<i32> {
    let jsval = crate::value::JSValue::from_bits(key.to_bits());
    if jsval.is_int32() {
        let index = jsval.as_int32();
        return (index >= 0).then_some(index);
    }
    if !key.is_nan() {
        return (key.is_finite() && key >= 0.0 && key.fract() == 0.0 && key <= i32::MAX as f64)
            .then_some(key as i32);
    }

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    parse_canonical_nonnegative_i32(bytes)
}

pub(crate) fn set_integer_indexed_exotic(target: f64, key: f64, value: f64) -> bool {
    let Some(index) = integer_index_key(key) else {
        return false;
    };
    let Some(raw) = raw_ptr_from_value(target) else {
        return false;
    };
    // #8149: `ArrayBuffer` / `SharedArrayBuffer` / `DataView` are registered
    // buffers, but they are NOT integer-indexed exotic objects — `dv[0] = 7`
    // creates an ORDINARY own property in node and leaves the byte at 0.
    // Answering `false` here hands the write back to `js_put_value_set`'s
    // ordinary `[[Set]]` walk, which would bit-cast the `BufferHeader`, so
    // store the expando directly and claim the write.
    if crate::buffer::is_non_indexed_buffer_view(raw) {
        crate::buffer::buffer_set_own_prop(raw, &index.to_string(), value);
        return true;
    }
    if crate::buffer::is_registered_buffer(raw) {
        crate::buffer::js_buffer_set(raw as *mut crate::buffer::BufferHeader, index, value as i32);
        return true;
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        crate::typedarray::js_typed_array_set(
            raw as *mut crate::typedarray::TypedArrayHeader,
            index,
            value,
        );
        return true;
    }
    false
}

#[derive(Clone, Copy)]
pub(crate) enum OwnSetDescriptor {
    Data { writable: bool },
    Accessor { setter_bits: u64 },
}

pub(crate) fn key_to_rust_string(value: f64) -> Option<String> {
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    unsafe {
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

pub(crate) fn property_key_to_rust_string(value: f64) -> Option<String> {
    let property_key = unsafe { crate::object::js_to_property_key(value) };
    key_to_rust_string(property_key)
}

pub(crate) fn own_set_descriptor(target: f64, key: f64) -> Option<OwnSetDescriptor> {
    if small_handle_from_value(target).is_some() {
        return None;
    }
    // Object model §3.2: an optional layout key whose slot holds the hole is
    // absent (`key_view`), so the walk goes on to the prototype chain.
    if target.to_bits() & !POINTER_MASK == POINTER_TAG
        && unsafe {
            crate::object::key_view::own_key_is_hole(
                extract_pointer(target.to_bits()) as *const crate::ObjectHeader,
                crate::JSValue::from_bits(key.to_bits()),
            )
        }
    {
        return None;
    }

    // Ordinary prototype holders answer from their shape's key index. A
    // missing key is an indexed absence, not a byte scan of every builtin
    // key followed by a virtual/global property lookup.
    let addr = extract_pointer(target.to_bits()) as usize;
    let indexed_owner = unsafe {
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
                // The indexed answer reads the layout alone; a prototype with
                // an overflow keeps keys and attributes in its entries (object
                // model step 4), which the complete lookup below reads.
                let meta = (*(addr as *const crate::ObjectHeader)).meta;
                (!meta.is_null()
                    && (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0
                    && (*meta).overflow == 0)
                    .then_some(addr)
            }
            Some(header) if header.obj_type == crate::gc::GC_TYPE_ARRAY => {
                crate::object::shape_store::named_bag(
                    addr as *const crate::ObjectHeader,
                    key.to_bits(),
                )
                .map(|bag| bag as usize)
            }
            _ => None,
        }
    };
    if let Some(indexed_owner) = indexed_owner {
        if let Some(shape) = unsafe {
            crate::object::shapes::object_shape_descriptor(
                indexed_owner as *const crate::ObjectHeader,
            )
        } {
            if shape.object_kind.is_ordinary_layout() {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(bytes) = unsafe {
                    crate::string::js_string_key_bytes(
                        crate::JSValue::from_bits(key.to_bits()),
                        &mut scratch,
                    )
                } {
                    let keys = shape.keys as usize as *const crate::ArrayHeader;
                    use crate::object::shapes::KeysIndexVerdict;
                    let verdict = if keys.is_null() {
                        // Keyless dictionary shapes do not answer own absence;
                        // the descriptor path below reads their receiver keys.
                        KeysIndexVerdict::Unindexed
                    } else {
                        unsafe {
                            let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                            let verdict = crate::object::shapes::shape_slot_lookup_verdict(
                                keys,
                                bytes,
                                hash,
                                shape.logical_key_count,
                                true,
                            );
                            // A shortened mutable key list retires its old
                            // index. Rebuild before claiming absence.
                            if matches!(verdict, KeysIndexVerdict::Unindexed) {
                                crate::object::shapes::shape_slot_lookup_verdict(
                                    keys,
                                    bytes,
                                    hash,
                                    shape.logical_key_count,
                                    true,
                                )
                            } else {
                                verdict
                            }
                        }
                    };
                    match verdict {
                        KeysIndexVerdict::Absent => return None,
                        KeysIndexVerdict::Unindexed => {}
                        KeysIndexVerdict::Found(slot) => {
                            let entry = unsafe { crate::object::key_attrs::keys_entry(keys, slot) };
                            if crate::object::key_attrs::entry_is_private(entry) {
                                return None;
                            }
                            if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
                                let name = std::str::from_utf8(bytes).ok()?;
                                let acc =
                                    crate::object::get_accessor_descriptor(indexed_owner, name)?;
                                return Some(OwnSetDescriptor::Accessor {
                                    setter_bits: acc.set,
                                });
                            }
                            return Some(OwnSetDescriptor::Data {
                                writable: entry & crate::object::key_attrs::ENTRY_NON_WRITABLE == 0,
                            });
                        }
                    }
                }
            }
        }
    }

    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        // `undefined` is a valid value for an existing own property. Using a
        // value read as the existence probe made the first overwrite of the
        // symbol metadata installed by an async_hooks `init` callback take the
        // absent-property path and disappear on native AsyncResource handles.
        if !unsafe { crate::symbol::has_own_symbol_property(target, key) } {
            return None;
        }
        // #10481: a symbol ACCESSOR is not a data slot — report its setter so
        // the walk runs it with the receiver instead of shadowing it.
        let (owner, sym_key) = unsafe {
            (
                crate::symbol::obj_key_from_f64(target),
                crate::symbol::sym_key_from_f64(key),
            )
        };
        if let Some((_, setter_bits)) =
            crate::symbol::symbol_accessor_descriptor_bits(owner, sym_key)
        {
            return Some(OwnSetDescriptor::Accessor { setter_bits });
        }
        // An existing symbol-keyed own data property is non-writable when the
        // receiver is frozen or its per-symbol attrs say so — so a strict
        // `obj[sym] = v` is rejected (throws) rather than silently no-op'd
        // (test262 Object/freeze/frozen-object-contains-symbol-properties-strict).
        // Mirrors the string-keyed / `set_symbol_property` guards.
        let writable = !crate::symbol::symbol_property_is_non_writable(target, key);
        return Some(OwnSetDescriptor::Data { writable });
    }

    // The null-receiver guard stays BEFORE the coercion: `key_to_rust_string`
    // can run a user `toString`, and moving it earlier would make that side
    // effect observable on a path that previously short-circuited.
    // ClassRef constructors are non-pointer values with an own prototype.
    if extract_pointer(target.to_bits()) as usize == 0
        && crate::object::class_ref_id(target).is_none()
    {
        return None;
    }
    // #6943: `key_to_rust_string` runs the GC-capable `js_string_coerce`, and
    // `obj_ptr` is BOTH a dereferenced heap address (the typed-array probe) and
    // the raw ADDRESS KEY of the `ACCESSOR_DESCRIPTORS` /
    // `PROPERTY_DESCRIPTORS` side tables. A stale one does not crash: it
    // silently misses, so a non-writable own property or a setter-less accessor
    // reads back as "no descriptor" and the [[Set]] that should have been
    // rejected goes through. Root the receiver across the coercion and derive
    // the address afterwards. (Found in the same review pass as the `key` gap
    // below.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_u64(target.to_bits());
    let key_name = key_to_rust_string(key)?;
    let target = f64::from_bits(target_handle.get_nanbox_u64());
    // Class constructors have an immutable own prototype even though their
    // ClassRef representation has no heap address or descriptor side table.
    if key_name == "prototype"
        && crate::object::class_ref_id(target).is_some()
        && crate::object::class_prototype_ref_id(target).is_none()
    {
        return Some(OwnSetDescriptor::Data { writable: false });
    }
    let obj_ptr = extract_pointer(target.to_bits()) as usize;
    if obj_ptr == 0 {
        return None;
    }
    // A typed array keeps its ordinary (non-index) own expando properties and
    // their descriptors in the typed-array side tables, which the generic
    // address-keyed lookups below skip (`object_has_descriptors` is
    // deliberately gated off for typed arrays). Consult that state directly so
    // a non-writable own data property / setter-less accessor rejects the write
    // (test262 TypedArray internals/Set key-is-not-numeric-index).
    if crate::typedarray_props::is_typed_array_owner(obj_ptr) {
        return match crate::typedarray_props::typed_array_own_set_descriptor(obj_ptr, &key_name) {
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Data { writable }) => {
                Some(OwnSetDescriptor::Data { writable })
            }
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Accessor { setter_bits }) => {
                Some(OwnSetDescriptor::Accessor { setter_bits })
            }
            None => None,
        };
    }
    // `ACCESSOR_DESCRIPTORS` / `PROPERTY_DESCRIPTORS` are keyed by raw address,
    // so a fresh object reusing a freed address would otherwise read back the
    // previous tenant's stale getter-only accessor / non-writable descriptor and
    // report this `obj.k = v` as read-only — falsely throwing "Cannot assign to
    // read only property" on a plain `{}` (Next.js app-page-turbo runtime's
    // `exports.Fragment = …`, reached here once a descriptor on Object.prototype
    // disables the plain-object [[Set]] fast path process-wide, #5054). Gate on
    // the per-object `OBJ_FLAG_HAS_DESCRIPTORS` flag — set reliably for every
    // descriptor installed on a `GC_TYPE_OBJECT`, and clear on a fresh
    // allocation. Closures don't carry the flag, so keep consulting the side
    // tables for them (their `name`/`length` + user `defineProperty` descriptors
    // live there).
    // A class function object's static accessor is an accessor property of
    // its own-property object (#11521: a getter-only one refuses the write).
    // Its own data properties live in the same object; anything else is not
    // own, and the walk continues at the class's [[Prototype]].
    if let Some(class_id) = crate::object::class_value::class_closure_id(obj_ptr) {
        if let Some((acc, _, _)) =
            crate::object::class_value::class_static_own_accessor(class_id, &key_name)
        {
            return Some(OwnSetDescriptor::Accessor {
                setter_bits: acc.set,
            });
        }
        if crate::object::class_value::class_static_get(class_id, &key_name).is_some() {
            let writable = crate::object::class_static_defined_attrs(class_id, &key_name)
                .is_none_or(|(writable, _, _)| writable);
            return Some(OwnSetDescriptor::Data { writable });
        }
        return None;
    }
    if crate::object::object_has_descriptors(obj_ptr)
        || unsafe {
            crate::object::key_attrs::attrs_live_in_keys(obj_ptr)
                && crate::object::key_attrs::object_summary(obj_ptr as *const crate::ObjectHeader)
                    != 0
        }
        || crate::closure::is_closure_ptr(obj_ptr)
    {
        if let Some(acc) = crate::object::get_accessor_descriptor(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Accessor {
                setter_bits: acc.set,
            });
        }
        if let Some(attrs) = crate::object::get_property_attrs(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Data {
                writable: attrs.writable(),
            });
        }
    }
    if crate::closure::is_closure_ptr(obj_ptr) {
        if crate::object::has_own_helpers::closure_own_key_present(obj_ptr, &key_name) {
            // A function's lazily synthesized prototype is already an own
            // property. Preserve its attributes when the first operation is
            // an assignment, before any read materializes the default object.
            if key_name == "prototype" && crate::object::function_would_have_own_prototype(target) {
                crate::object::set_builtin_property_attrs(
                    obj_ptr,
                    key_name.clone(),
                    crate::object::PropertyAttrs::new(true, false, false),
                );
            }
            return Some(OwnSetDescriptor::Data {
                writable: !matches!(key_name.as_str(), "name" | "length"),
            });
        }
        return None;
    }
    if crate::object::obj_value_has_own_key(target, key) {
        return Some(OwnSetDescriptor::Data { writable: true });
    }
    None
}

pub(crate) fn prototype_of_for_set(value: f64) -> Option<f64> {
    if !reflect_value_is_object(value) {
        return None;
    }
    // A tagged Proxy receiver must dispatch before an ObjectHeader read.
    // Its own target/handler fields provide the prototype operation.
    if lookup(value).is_some() {
        let proto = proxy_get_prototype_of_impl(value);
        let proto_bits = proto.to_bits();
        return if proto_bits == TAG_NULL
            || proto_bits == TAG_UNDEFINED
            || proto_bits == value.to_bits()
        {
            None
        } else {
            Some(proto)
        };
    }
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as usize;
        // #7531: `lookup(value)` above only recognizes an already-registered
        // revocable-Proxy id; a fetch/zlib/common-registry handle id (or a
        // Proxy id from a DIFFERENT realm/registry) reaches here instead.
        // The old floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle
        // band, and the local name `raw` made it invisible to
        // `scripts/addr_class_inventory.py`'s `handle-floor` regex. Below,
        // a rejected `object_static_prototype` lookup falls through to
        // `is_valid_obj_ptr(obj)` -- a magnitude-only check whose own floor
        // is 0x1000 -- followed by an unconditional `(*obj).class_id` read,
        // so an admitted handle id reached that deref.
        // A class function object is a closure, not an ObjectHeader: its
        // [[Prototype]] is the class's (the parent class for `extends`).
        if let Some(class_id) = crate::object::class_value::class_closure_id(raw) {
            let proto = crate::object::class_value::class_prototype_addr(class_id);
            return (proto != 0 && proto != raw)
                .then(|| f64::from_bits(POINTER_TAG | proto as u64));
        }
        if crate::value::addr_class::is_above_handle_band(raw) {
            if let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(raw) {
                if proto_bits == TAG_NULL || proto_bits == TAG_UNDEFINED || proto_bits == bits {
                    return None;
                }
                return Some(f64::from_bits(proto_bits));
            }
            let obj = raw as *const crate::ObjectHeader;
            if crate::object::is_valid_obj_ptr(obj as *const u8) {
                unsafe {
                    let class_id = (*obj).class_id;
                    if class_id != 0 {
                        let proto = crate::object::class_prototype_object(class_id);
                        if !proto.is_null() && proto as usize != raw {
                            return Some(crate::value::js_nanbox_pointer(proto as i64));
                        }
                    }
                }
            }
        }
    }
    let proto = crate::object::js_object_get_prototype_of(value);
    let bits = proto.to_bits();
    if bits == TAG_NULL || bits == TAG_UNDEFINED || bits == value.to_bits() {
        None
    } else {
        Some(proto)
    }
}

pub(crate) fn reflect_target_get_prototype_of(value: f64) -> f64 {
    prototype_of_for_set(value).unwrap_or_else(|| crate::object::js_object_get_prototype_of(value))
}

pub(crate) fn call_setter_with_receiver(setter_bits: u64, receiver: f64, value: f64) -> bool {
    if setter_bits == 0 {
        return false;
    }
    let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
    let receiver = this_scope.root_nanbox_f64(receiver);
    let value = this_scope.root_nanbox_f64(value);
    let rebound = crate::closure::clone_closure_rebind_this(setter_bits, receiver.get_nanbox_f64());
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return false;
    }
    let _ = crate::closure::js_closure_call1(
        closure,
        crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
        value.get_nanbox_f64(),
    );
    true
}

/// Ordinary named-property Set for the BufferHeader-backed objects that are
/// not integer-indexed exotics (ArrayBuffer, SharedArrayBuffer, DataView).
///
/// These values have no ObjectHeader/GcHeader. The generic Set tail eventually
/// asks whether the receiver is a GC heap object before CreateDataProperty;
/// a process-global SharedArrayBuffer can therefore be rejected even though
/// the buffer registries authoritatively classify it as an Object. Walk the
/// descriptors normally, but materialize the final own data property in the
/// buffer expando table instead of the ObjectHeader store.
pub(crate) fn set_nonindexed_buffer_named_self(target: f64, key: f64, value: f64) -> Option<bool> {
    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        return None;
    }
    let raw = raw_ptr_from_value(target)?;
    if !crate::buffer::is_non_indexed_buffer_view(raw) {
        return None;
    }
    let name = key_to_rust_string(key)?;

    // Existing own accessor/data descriptors take the same precedence as in
    // OrdinarySetWithOwnDescriptor. Buffer expandos live in side tables, so
    // inspect those tables without treating the BufferHeader as ObjectHeader.
    if let Some(accessor) = crate::object::get_accessor_descriptor(raw, &name) {
        return Some(call_setter_with_receiver(accessor.set, target, value));
    }
    if crate::buffer::buffer_has_own_prop(raw, &name) {
        if crate::object::get_property_attrs(raw, &name).is_some_and(|attrs| !attrs.writable()) {
            return Some(false);
        }
        crate::buffer::buffer_set_own_prop(raw, &name, value);
        return Some(true);
    }

    // A descriptor on the prototype can reject the write or invoke a setter.
    // A writable inherited data descriptor (including the standard
    // `.constructor`) creates an own property on the original receiver.
    let mut current = prototype_of_for_set(target);
    for _ in 0..64 {
        let Some(proto) = current else {
            break;
        };
        if let Some(desc) = own_set_descriptor(proto, key) {
            match desc {
                OwnSetDescriptor::Data { writable: false } => return Some(false),
                OwnSetDescriptor::Data { writable: true } => break,
                OwnSetDescriptor::Accessor { setter_bits } => {
                    return Some(call_setter_with_receiver(setter_bits, target, value));
                }
            }
        }
        current = prototype_of_for_set(proto);
    }

    crate::buffer::buffer_set_own_prop(raw, &name, value);
    Some(true)
}

/// #5129: build a fresh data property descriptor
/// `{ value, writable: true, enumerable: true, configurable: true }`
/// (the CreateDataProperty shape) for defining a property on a Proxy receiver
/// via its `[[DefineOwnProperty]]`.
pub(crate) unsafe fn build_create_data_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 4);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    for (name, field) in [
        (b"value".as_slice(), value_root.get_nanbox_f64()),
        (b"writable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"enumerable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"configurable".as_slice(), f64::from_bits(TAG_TRUE)),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key,
            field,
        );
    }
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

/// Define a writable, enumerable, configurable own data property using the
/// receiver's `[[DefineOwnProperty]]`. This is the operation used by public
/// class fields: it bypasses inherited setters on ordinary objects and still
/// drives a Proxy's `defineProperty` trap.
pub(crate) fn create_data_property(receiver: f64, key: f64, value: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    let descriptor = unsafe { build_create_data_descriptor(value.get_nanbox_f64()) };
    let descriptor = scope.root_nanbox_f64(descriptor);
    crate::value::js_is_truthy(js_reflect_define_property(
        receiver.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor.get_nanbox_f64(),
    )) != 0
}

/// #5129: build a `{ value }`-only property descriptor — the `valueDesc` of
/// OrdinarySetWithOwnDescriptor step 2.d.iii, used to update an existing
/// writable data property on a Proxy receiver without disturbing its other
/// attributes (`writable`/`enumerable`/`configurable`).
pub(crate) unsafe fn build_value_only_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 1);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key,
        value_root.get_nanbox_f64(),
    );
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

pub(crate) fn create_or_update_receiver_property(receiver: f64, key: f64, value: f64) -> bool {
    if !reflect_value_is_object(receiver) {
        return false;
    }
    // #5129: a Proxy receiver — e.g. a `set` trap forwarding
    // `Reflect.set(target, key, value, proxy)` (the 4-arg form) — must route
    // through the proxy's `[[DefineOwnProperty]]` (its `defineProperty` trap,
    // or, absent a trap, a define on the proxy's target), NOT an ordinary data
    // store. This is OrdinarySetWithOwnDescriptor's tail
    // `CreateDataProperty(Receiver, P, V)`. Treating the proxy id as a heap
    // object (the `target_set` fall-through below) segfaulted; re-invoking the
    // `set` trap would have recursed infinitely.
    if lookup(receiver).is_some() {
        // OrdinarySetWithOwnDescriptor steps 2.c–2.e for a Proxy receiver. We
        // must mirror the ordinary algorithm's receiver-own-descriptor checks,
        // not jump straight to CreateDataProperty:
        //
        //   2.c existingDescriptor = Receiver.[[GetOwnProperty]](P)
        //   2.d if it exists:
        //       i.   accessor descriptor      → return false
        //       ii.  non-writable data        → return false
        //       iii. else redefine `{ value }` only (preserve other attrs)
        //   2.e else CreateDataProperty(Receiver, P, V)
        //
        // `[[GetOwnProperty]]` fires the proxy's getOwnPropertyDescriptor trap
        // (trap-less: reads its target) and returns a completed plain
        // descriptor object, or `undefined` when absent. A throwing/invariant-
        // violating trap unwinds via `js_throw` and never returns here.
        let existing = js_reflect_get_own_property_descriptor(receiver, key);
        let desc = if reflect_value_is_object(existing) {
            let is_accessor = unsafe {
                reflect::descriptor_field_present(existing, b"get")
                    || reflect::descriptor_field_present(existing, b"set")
            };
            // A completed data descriptor always carries `writable`; treat a
            // missing flag as non-writable (reject) to stay on the safe side.
            let writable = unsafe { reflect::descriptor_bool_field(existing, b"writable") };
            if is_accessor || writable != Some(true) {
                return false;
            }
            unsafe { build_value_only_descriptor(value) }
        } else {
            unsafe { build_create_data_descriptor(value) }
        };
        return crate::value::js_is_truthy(js_reflect_define_property(receiver, key, desc)) != 0;
    }
    if let Some(desc) = own_set_descriptor(receiver, key) {
        match desc {
            OwnSetDescriptor::Data { writable } => {
                if !writable {
                    return false;
                }
            }
            OwnSetDescriptor::Accessor { .. } => {
                // OrdinarySetWithOwnDescriptor step 2.d.i: reaching here means
                // the source (target own) descriptor was a data property (or
                // absent -> CreateDataProperty). A RECEIVER own accessor makes
                // the algorithm return false WITHOUT invoking the setter -- the
                // setter only fires when it is the descriptor found by the
                // OrdinarySet walk itself (the Accessor arm in
                // ordinary_set_with_receiver), never through this
                // CreateDataProperty-on-receiver tail.
                return false;
            }
        }
    } else if crate::object::obj_value_no_extend(receiver) {
        return false;
    } else if unsafe { crate::object::layout_desc::family_create(receiver, key, value) } {
        // Case 2c: a family object's late key, or a key outside its layout.
        return true;
    } else {
        // OrdinarySetWithOwnDescriptor creates an OWN data property here.
        // Re-entering [[Set]] would walk the receiver's prototypes again and
        // recurse forever when that chain contains a trapless Proxy.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(receiver);
        let key = scope.root_nanbox_f64(key);
        let descriptor = crate::object::object_ops::DescView::create_data(&scope, value);
        return unsafe {
            crate::object::object_ops::define_own_property_decoded(
                &scope,
                &receiver,
                &key,
                &descriptor,
            )
        };
    }
    target_set(receiver, key, value);
    true
}

pub(crate) fn ordinary_set_with_receiver(target: f64, key: f64, value: f64, receiver: f64) -> bool {
    if let Some(ok) = set_handle_property(target, key, value) {
        return ok;
    }

    // #5054 fast path: the spec walk below probes own_set_descriptor on the
    // target, which ends in a LINEAR keys_array scan — so every dynamic
    // `obj[key] = v` was O(own-key-count) and building a wide dynamic object
    // quadratic (10k props ~ 12s). When nothing the walk models can apply,
    // the write reduces to the ordinary data-property store:
    //   - target written as itself (receiver bits identical),
    //   - plain GC_TYPE_OBJECT with class_id 0 (no class setter machinery),
    //   - no descriptor ever installed on THIS object
    //     (OBJ_FLAG_HAS_DESCRIPTORS) and not frozen/sealed/non-extensible,
    //   - no recorded setPrototypeOf target (prototype chain is exactly
    //     Object.prototype) and no descriptor on Object.prototype,
    //   - string key.
    let target_top16 = target.to_bits() >> 48;
    if target.to_bits() == receiver.to_bits()
        // POINTER_TAG'd heap object, or a module-level slot's raw I64 pointer
        // (top 16 bits zero).
        && (target_top16 == 0x7FFD || target_top16 == 0)
        && unsafe { crate::symbol::js_is_symbol(key) } == 0
    {
        let addr = extract_pointer(target.to_bits()) as usize;
        // Typed arrays must be excluded before the header probe: small TAs
        // are plain-alloc'd without a GcHeader.
        if !crate::typedarray_props::is_typed_array_owner(addr)
            && crate::object::exotic_expando::exotic_expando_kind_of_value(target).is_none()
            && !crate::closure::is_closure_ptr(addr)
        {
            unsafe {
                if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
                    const SLOW_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
                        | crate::gc::OBJ_FLAG_SEALED
                        | crate::gc::OBJ_FLAG_NO_EXTEND;
                    // #10287: an own descriptor is vetted per KEY (the summary
                    // proves absence outright), so one `defineProperty` no
                    // longer sends every later store on this receiver down the
                    // full walk. Read before the coercion below: it reads key
                    // bytes only and cannot allocate.
                    if header.obj_type == crate::gc::GC_TYPE_OBJECT
                        && header._reserved & SLOW_FLAGS == 0
                        && crate::object::own_descriptors_skip_key(addr, key)
                    {
                        let class_id = (*(addr as *const crate::ObjectHeader)).class_id;
                        // #6943: BOTH arms below reach a GC-capable
                        // `js_string_coerce` on `key` — the class arm calls it
                        // directly for the store-plan key, and the plain-object
                        // arm reaches it through
                        // `object_proto_may_intercept_key` ->
                        // `obj_value_has_own_key`. The coercion is inert only
                        // for an already-heap `STRING_TAG` key; an SSO short
                        // key materializes onto the heap, a numeric key builds
                        // its stringification, and an object key runs a user
                        // `toString` / `valueOf`. Any of those can trigger a GC
                        // that **evacuates**.
                        //
                        // The rule is: when the coercion is NOT inert, every
                        // operand that outlives it must be rooted and re-read
                        // afterwards. That is four of them, not three —
                        // `addr` (the receiver, dereferenced for `plan_eligible`
                        // and passed to `class_instance_set_may_intercept`),
                        // `target` and the `value` about to be written INTO it,
                        // and **`key` itself**, which is re-used at the
                        // interception check and again at `target_set`. An
                        // object key is a `POINTER_TAG` heap value and is
                        // exactly the shape that runs the user JS which can
                        // evacuate it. (`key` was missed in the first pass of
                        // this fix; caught in review.)
                        //
                        // Only an already-heap `STRING_TAG` key is inert — it
                        // is handed straight back with no allocation — so that
                        // one shape takes no scope. Every other shape does: an
                        // SSO short key materializes onto the heap, a numeric
                        // key builds its stringification, and an object key
                        // runs a user `toString` / `valueOf`.
                        let scope = (!crate::builtins::string_coerce_is_inert(key))
                            .then(crate::gc::RuntimeHandleScope::new);
                        let roots = scope.as_ref().map(|s| {
                            (
                                s.root_nanbox_u64(target.to_bits()),
                                s.root_nanbox_f64(value),
                                s.root_raw_mut_ptr(addr as *mut u8),
                                s.root_nanbox_f64(key),
                            )
                        });
                        // Re-read an operand through its handle (or pass the
                        // original through untouched on the inert path).
                        let cur_target = || match &roots {
                            Some((t, ..)) => f64::from_bits(t.get_nanbox_u64()),
                            None => target,
                        };
                        let cur_value = || match &roots {
                            Some((_, v, ..)) => v.get_nanbox_f64(),
                            None => value,
                        };
                        let cur_addr = || match &roots {
                            Some((_, _, a, _)) => a.get_raw_mut_ptr::<u8>() as usize,
                            None => addr,
                        };
                        let cur_key = || match &roots {
                            Some((_, _, _, k)) => k.get_nanbox_f64(),
                            None => key,
                        };
                        let fast_safe = if class_id == 0 {
                            // Plain object: prototype is exactly Object.prototype, and
                            // Object.prototype doesn't intercept this key (per-key, not
                            // the coarse process-wide descriptor flag — that made wide
                            // builds O(n²)). #10287: a RECORDED prototype (a
                            // function-constructed receiver, `Object.create`,
                            // `setPrototypeOf`) is walked per key rather than rejected.
                            // A born-null receiver whose shape proves it ordinary
                            // with no chain has nothing to intercept the key.
                            use crate::object::shapes::store_kind;
                            let obj = cur_addr() as *const crate::ObjectHeader;
                            if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
                                && store_kind::born_null_shape_proven(obj, header._reserved)
                            {
                                true
                            } else {
                                match crate::object::prototype_chain::object_static_prototype(
                                    cur_addr(),
                                ) {
                                    None => {
                                        !crate::object::object_proto_may_intercept_key(cur_key())
                                    }
                                    Some(_) => {
                                        !crate::object::plain_custom_prototype_may_intercept(
                                            cur_addr(),
                                            cur_key(),
                                        )
                                    }
                                }
                            }
                        } else {
                            // `DisposableStack#disposed` is a getter-only
                            // builtin accessor on a reserved native prototype.
                            // Those class ids are intentionally absent from the
                            // JS class-vtable registry, so the shared
                            // class-interception plan cannot discover it.
                            // Keep an own descriptor on the instance eligible
                            // for the normal walk; otherwise report the
                            // inherited accessor's rejected [[Set]] here
                            // (silent in sloppy PutValue, TypeError in strict).
                            let inherited_disposed_readonly = matches!(
                                class_id,
                                crate::disposable::CLASS_ID_DISPOSABLE_STACK
                                    | crate::disposable::CLASS_ID_ASYNC_DISPOSABLE_STACK
                            ) && property_key_to_rust_string(key)
                                .as_deref()
                                == Some("disposed")
                                // #6943: `property_key_to_rust_string` runs
                                // `ToPropertyKey` + `js_string_coerce`, so both
                                // operands must be re-read before this probe.
                                && own_set_descriptor(cur_target(), cur_key()).is_none();
                            if inherited_disposed_readonly {
                                return false;
                            }
                            // Class instance: the `class_id == 0` guard previously sent
                            // EVERY wide class-instance build down the O(own-key) slow
                            // walk (O(n²)). Safe to fast-path when no inherited accessor /
                            // non-writable anywhere in the prototype chain could intercept
                            // this key.
                            //
                            // This receiver-based [[Set]] is the dominant real-code store
                            // path (codegen emits js_put_value_set for many `obj.f = v`),
                            // and it re-ran the full O(chain) interception walk on every
                            // call. Consult the SHARED store-plan cache first: the verdict
                            // is the identical `!class_instance_set_may_intercept(class_id,
                            // key)` that `js_object_set_field_by_name` already memoizes, so
                            // a hit on either path serves the other. Only trust a cached
                            // class-chain verdict for instances whose chain matches the
                            // class chain — SLOW_FLAGS above already excluded
                            // frozen/sealed/descriptor bits; add the per-instance
                            // divergence flags (setPrototypeOf override / null proto).
                            let key_ptr = crate::builtins::js_string_coerce(cur_key())
                                as *const crate::StringHeader;
                            // #6943: re-read the receiver AND the key through
                            // their handles — everything below uses both.
                            let addr = cur_addr();
                            let key = cur_key();
                            let interned = crate::object::interned_key_ptr(key_ptr);
                            // #6595: a per-evaluation CLASS OBJECT (what a
                            // capture-carrying class materializes as,
                            // `ShapeObjectKind::Class`) shares its
                            // template cid with its instances, and its own-data
                            // writes must reach the #6530
                            // `mirror_class_object_static_write` hook in
                            // `js_object_set_field_by_name`. Recording a plan
                            // here armed the mirror-free fast lane for the very
                            // store being vetted, so from the second same-shaped
                            // class on (shape-transition cache hit) post-class
                            // statics like bundled zod's `ZodX.create` vanished
                            // from ClassRef static dispatch. Class objects
                            // neither record nor honor store plans.
                            let plan_eligible = class_id != crate::object::NATIVE_MODULE_CLASS_ID
                                // #8113: this asks for ORDINARY specifically —
                                // it must stay FALSE for a class object or
                                // #6595 reopens. `object_is_regular` is exactly
                                // `descriptor.object_kind == Ordinary` since
                                // #8086, so it is the same predicate the
                                // deleted `object_type == OBJECT_TYPE_REGULAR`
                                // word expressed, not the weaker
                                // "is an ObjectHeader" test.
                                && crate::object::object_is_regular(
                                    addr as *const crate::ObjectHeader,
                                )
                                // A born-null receiver's plan is keyed on a
                                // shape that proves it ordinary with no chain.
                                && crate::object::shapes::store_kind::born_null_admits_lane(
                                    addr as *const crate::ObjectHeader,
                                    header._reserved,
                                )
                                && interned != 0;
                            let verdict = if plan_eligible
                                && crate::object::prop_plan::store_plan_check(
                                    LayoutWord(
                                        (crate::object::shapes::object_shape_stamp(
                                            addr as *const crate::ObjectHeader,
                                        ))
                                        .0,
                                    ),
                                    interned,
                                ) {
                                true
                            } else {
                                let clear = !crate::object::class_instance_set_may_intercept(
                                    addr, class_id, key,
                                );
                                if clear && plan_eligible {
                                    crate::object::prop_plan::store_plan_record(
                                        LayoutWord(
                                            (crate::object::shapes::object_shape_stamp(
                                                cur_addr() as *const crate::ObjectHeader
                                            ))
                                            .0,
                                        ),
                                        interned,
                                    );
                                }
                                clear
                            };
                            verdict
                        };
                        // #6943: the store itself takes the refreshed receiver,
                        // KEY and payload — all three were rooted across
                        // whichever coercion the arm above performed.
                        if fast_safe {
                            target_set(cur_target(), cur_key(), cur_value());
                            return true;
                        }
                    }
                }
            }
        }
    }

    // CommonJS native-module namespaces are MUTABLE in Node — monkey-patching
    // like Next.js's `require('node:timers').setImmediate = patched` must
    // store the override (read back through the namespace vtable's
    // `get_own_field`) rather than reporting the built-in member
    // non-writable and throwing under strict mode.
    {
        let jv = crate::value::JSValue::from_bits(target.to_bits());
        if jv.is_pointer() {
            let obj = extract_pointer(target.to_bits()) as *const crate::object::ObjectHeader;
            if !obj.is_null() && unsafe { (*obj).class_id } == crate::object::NATIVE_MODULE_CLASS_ID
            {
                let module_name = unsafe { crate::object::get_module_name_from_namespace(target) };
                if let (false, Some(prop)) =
                    (module_name.is_empty(), property_key_to_rust_string(key))
                {
                    if prop != "__module__" {
                        if module_name == "buffer.Buffer" && prop == "poolSize" {
                            crate::object::set_buffer_pool_size(value);
                        } else {
                            crate::object::native_namespace_prop_override_store(
                                &module_name,
                                &prop,
                                value,
                            );
                        }
                        return true;
                    }
                }
            }
        }
    }

    set_interceptor(target, key, value, receiver)
        .unwrap_or_else(|| create_or_update_receiver_property(receiver, key, value))
}

/// #11201: `[[Set]]` reaching a class-backed link — a declared or
/// per-evaluation class prototype, or a class instance used as a prototype
/// (`Object.create(new C())`) — on the chain of a receiver with class id 0.
/// The link's accessors live in the class vtable, not in the descriptor tables
/// `own_set_descriptor` reads, so without this the walk ran past an inherited
/// `set x` and created an own data property instead of calling it.
///
/// A receiver WITH a class id never takes this arm: its write reaches
/// `js_object_set_field_by_name`, whose vtable walk is keyed by that id. An
/// own property of the link has already been answered by `own_set_descriptor`.
/// `Some(true)`: the setter ran; `Some(false)`: getter-only, the write is
/// rejected; `None`: no accessor for `key`, continue the walk.
pub(crate) fn class_link_accessor_set(
    current: f64,
    key: f64,
    value: f64,
    receiver: f64,
) -> Option<bool> {
    let link = extract_pointer(current.to_bits()) as *const crate::ObjectHeader;
    let recv = extract_pointer(receiver.to_bits()) as *const crate::ObjectHeader;
    if link.is_null() || crate::object::js_object_get_class_id(recv) != 0 {
        return None;
    }
    // A class function object is a closure (no ObjectHeader class id): its
    // static accessors are own properties `own_set_descriptor` reports.
    if crate::object::class_value::class_closure_id(link as usize).is_some() {
        return None;
    }
    let class_id = crate::object::js_object_get_class_id(link);
    if class_id == 0
        || class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::is_class_object_ptr(link.cast())
    {
        return None;
    }
    class_super_accessor_set(class_id, key, value, receiver)
}

pub(crate) fn class_super_accessor_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
) -> Option<bool> {
    let key_name = property_key_to_rust_string(key)?;
    // Charter step 3: the accessor is a property of the parent's prototype
    // chain. `Some(false)`: getter-only, the write is refused.
    unsafe { crate::object::class_chain_setter_apply(parent_class_id, &key_name, receiver, value) }
}

pub(crate) fn receiver_super_parent_class_id(receiver: f64) -> Option<u32> {
    let obj = extract_pointer(receiver.to_bits()) as *const crate::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    let class_id = unsafe { (*obj).class_id };
    if class_id == 0 {
        return None;
    }
    crate::object::get_parent_class_id(class_id)
}

pub(crate) fn normalize_accessor_receiver(receiver: f64) -> f64 {
    let bits = receiver.to_bits();
    if bits != 0 && (bits >> 48) == 0 {
        crate::value::js_nanbox_pointer(bits as i64)
    } else if receiver.is_finite() && receiver > 65_536.0 && receiver.fract() == 0.0 {
        crate::value::js_nanbox_pointer(receiver as i64)
    } else {
        receiver
    }
}

/// `super[key] = value` for class methods. The property lookup starts at the
/// parent prototype, but writes use the current `this` as Receiver.
#[no_mangle]
pub extern "C" fn js_super_put_value_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    let receiver = normalize_accessor_receiver(receiver);
    // Static-context super uses the parent CONSTRUCTOR as the lookup target,
    // while retaining the child constructor as Receiver. A statically-known
    // class parent is represented by a ClassRef; a function-valued parent is
    // the value captured at class-definition time. The previous instance-only
    // path looked at `Parent.prototype` and made valid static writes fail.
    if let Some(child_id) = crate::object::class_ref_id(receiver) {
        // A builtin parent id (`extends Error`) never gets a class function
        // object; its constructor is the class's dynamic parent value.
        let target =
            if parent_class_id != 0 && crate::object::is_class_id_registered(parent_class_id) {
                crate::object::class_value::class_value(parent_class_id)
            } else {
                crate::object::js_get_dynamic_parent_value(child_id)
            };
        let tv = crate::value::JSValue::from_bits(target.to_bits());
        if !tv.is_undefined() && !tv.is_null() {
            return js_put_value_set(target, key, value, receiver, strict);
        }
        // A base class's constructor inherits from Function.prototype. Perry
        // does not materialize that object for this path; when lookup misses,
        // OrdinarySet creates the own property on Receiver.
        return js_put_value_set(receiver, key, value, receiver, strict);
    }
    if parent_class_id == 0 && crate::object::is_class_object_value(receiver) {
        let obj = crate::value::JSValue::from_bits(receiver.to_bits())
            .as_pointer::<crate::ObjectHeader>();
        let child_id = if obj.is_null() {
            0
        } else {
            crate::object::js_object_get_class_id(obj)
        };
        let dynamic_parent = crate::object::js_get_dynamic_parent_value(child_id);
        if crate::value::JSValue::from_bits(dynamic_parent.to_bits()).is_undefined() {
            return js_put_value_set(receiver, key, value, receiver, strict);
        }
        return js_put_value_set(dynamic_parent, key, value, receiver, strict);
    }
    let receiver_parent_class_id = receiver_super_parent_class_id(receiver);
    if let Some(ok) =
        class_super_accessor_set(parent_class_id, key, value, receiver).or_else(|| {
            receiver_parent_class_id
                .filter(|cid| *cid != parent_class_id)
                .and_then(|cid| class_super_accessor_set(cid, key, value, receiver))
        })
    {
        if !ok && strict != 0 {
            let key_name = key_to_rust_string(key).unwrap_or_else(|| "property".to_string());
            crate::error::throw_immutable_write(0, &key_name);
        }
        return value;
    }

    let effective_parent_class_id = if parent_class_id != 0 {
        parent_class_id
    } else {
        receiver_parent_class_id.unwrap_or(0)
    };
    let proto = crate::object::class_prototype_object(effective_parent_class_id);
    if !proto.is_null() {
        let target = crate::value::js_nanbox_pointer(proto as i64);
        return js_put_value_set(target, key, value, receiver, strict);
    }

    // No resolvable parent-class prototype — `super` is `Object.prototype`
    // (e.g. `class A {}` with no `extends`). Per spec `super.x = v` performs
    // the home object's prototype `[[Set]]` with `this` as the receiver, which
    // for a missing key + no inherited setter creates an own data property on
    // the receiver. Do that ordinary set instead of throwing. (Test262
    // syntax/class-body-method-definition-super-property.)
    js_put_value_set(receiver, key, value, receiver, strict)
}
