//! Type-erased object / collection property + method dispatchers, plus
//! `js_value_length_f64` which the inline `.length` codegen falls into
//! when the receiver's GC-type byte doesn't statically prove the kind.

use super::*;
use std::sync::atomic::Ordering;

/// Issue #73: safe `.length` lookup by runtime type. Called from the
/// inline PropertyGet length path when the GC-type-byte check at
/// `handle-8` doesn't prove the receiver is a GC_TYPE_ARRAY or
/// GC_TYPE_STRING. Routes by runtime registry / GC header so that a
/// Named-typed receiver that turns out to hold a Buffer, TypedArray,
/// Closure, Error, number, etc. at runtime returns a sensible length
/// instead of dereferencing garbage at `recv & 0xFFFFFFFFFFFF`.
///
/// Returns a double so the inline caller can phi the fast and slow
/// results without another conversion.
#[no_mangle]
pub extern "C" fn js_value_length_f64(value: f64) -> f64 {
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(value) {
        if matches!(
            crate::builtins::boxed_primitive_to_string_tag(value),
            Some("String")
        ) {
            return js_value_length_f64(payload);
        }
        return 0.0;
    }

    let bits = value.to_bits();
    let top16 = bits >> 48;

    // SHORT_STRING_TAG (SSO) — length is the byte count stored in
    // bits 40..=47. Fast path, no heap access. For multibyte UTF-8
    // content the byte length and UTF-16 code-unit count differ,
    // but SSO strings are ≤5 bytes and the vast majority are ASCII
    // where they match. Non-ASCII SSO values go through a slower
    // full-parse path — tolerated because the distinction doesn't
    // come up in practice for 5-byte strings.
    if top16 == 0x7FF9 {
        return ((bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT) as f64;
    }

    // STRING_TAG — length is code-unit count from js_string_length.
    if top16 == 0x7FFF {
        let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x10000 {
            return 0.0;
        }
        return crate::string::js_string_length(ptr) as f64;
    }

    // POINTER_TAG — Buffer / TypedArray via registries first (they
    // don't have GC headers — `buffer_alloc` + `typed_array_alloc`
    // use `std::alloc` directly). Falling through to the GC-header
    // path would read mimalloc bookkeeping as obj_type and return
    // nonsense.
    if top16 == 0x7FFD {
        let handle = (bits & POINTER_MASK) as usize;
        // A POINTER_TAG value in a handle band (Web Fetch
        // Headers/Request/Response/Blob, net/http small handles, zlib stream
        // ids, revocable-proxy ids — all `< 0x100000`) is a registry id, not a
        // heap pointer. None of them carry a `.length`, and dereferencing the
        // raw id (`handle - 8` GcHeader read, or `*handle` u32 below) hits
        // unmapped low memory → SIGSEGV. The macOS 2 TB `heap_min` floor below
        // masks this, but the Linux/Android/iOS `0x1000` floor does not, so
        // reject the band explicitly here. Matches the band gating the inline
        // `.length` codegen fast path now applies before falling in here.
        if crate::value::addr_class::is_handle_band(handle) {
            return 0.0;
        }
        // Heap window: macOS mimalloc lands in 3-5 TB, but Android scudo,
        // Linux glibc, Windows mimalloc, and iOS-family device
        // libsystem_malloc all allocate much lower (often hundreds of GB
        // or less, and on iOS device often in the single-digit GB or even
        // sub-GB range). Using the macOS-tight 2 TB floor on those
        // platforms null-s every real pointer — on iOS device this is the
        // bug behind #1136 (`.length` on an array returned from
        // `String.split()` collapses to 0, so `for…of` loops zero times
        // and `segments.length === 0` is wrongly true). See clean_arr_ptr
        // for the same platform split.
        #[cfg(any(
            target_os = "android",
            target_os = "linux",
            target_os = "windows",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
        ))]
        let heap_min: usize = 0x1000;
        #[cfg(not(any(
            target_os = "android",
            target_os = "linux",
            target_os = "windows",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "visionos",
        )))]
        let heap_min: usize = 0x200_0000_0000;
        if handle < heap_min || (handle as u64) >= 0x8000_0000_0000 {
            return 0.0;
        }
        if let Some(value) = unsafe {
            crate::typedarray_props::typed_array_get_property_value_by_name(handle, "length")
        } {
            return value;
        }
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { (*buf).length as f64 };
        }
        if crate::typedarray::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray::TypedArrayHeader;
            return unsafe { (*ta).length as f64 };
        }
        let gc_header = (handle - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let obj_type = unsafe { (*gc_header).obj_type };
        // Issue #233: a FORWARDED array's first 4 bytes are no longer
        // length but the lower 32 bits of the forwarding pointer.
        // Follow the chain via the unified array-pointer cleaner so
        // `samples.length` after a grow returns the real length.
        if obj_type == crate::gc::GC_TYPE_ARRAY
            && unsafe { (*gc_header).gc_flags } & crate::gc::GC_FLAG_FORWARDED != 0
        {
            let cleaned = crate::array::js_array_get_length(handle as i64);
            return cleaned as f64;
        }
        match obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_STRING => {
                return unsafe { *(handle as *const u32) } as f64;
            }
            // Issue #179 Phase 2: lazy arrays also have `length` at
            // offset 0 (cached_length). The inline-length codegen
            // only recognizes GC_TYPE_ARRAY/STRING in its check so
            // lazy values land here via the slow path — read the
            // u32 from offset 0 just like regular arrays.
            crate::gc::GC_TYPE_LAZY_ARRAY => {
                return unsafe { *(handle as *const u32) } as f64;
            }
            // A closure's `.length` is its spec param count (own-property
            // override first, then the codegen-registered length). Without
            // this arm a `Function`-typed receiver — e.g. a folded
            // `new Function("a,b", body)` — would read 0 here. Mirrors the
            // generic PropertyGet reflection path.
            crate::gc::GC_TYPE_CLOSURE => {
                return crate::closure::closure_length(
                    handle as *const crate::closure::ClosureHeader,
                )
                .unwrap_or(0) as f64;
            }
            // A plain object CAN carry a `length` property — notably a
            // variable whose static type was inferred `Array` but was
            // reassigned to an array-like object (`var x = []; … x = {0:0};
            // x.splice(1,1); x.length` — test262 splice/S15.4.4.12_A4_T1
            // #10). Read it like any field; absent stays the 0 fallback.
            crate::gc::GC_TYPE_OBJECT => {
                let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                let v = crate::object::js_object_get_field_by_name_f64(
                    handle as *const crate::object::ObjectHeader,
                    key,
                );
                if v.to_bits() == crate::value::TAG_UNDEFINED {
                    return 0.0;
                }
                let n = crate::builtins::js_number_coerce(v);
                return if n.is_nan() { 0.0 } else { n };
            }
            // BigInts, Promises, Errors, Maps: no `.length`.
            // Return 0 to match Perry's existing fallback for missing fields
            // (JS would produce `undefined`, but the generic PropertyGet slow
            // path already degrades to 0 here).
            _ => return 0.0,
        }
    }

    // Raw pointer bitcast to f64 (no NaN-box tag — top16 == 0).
    // TypedArrays are allocated via `std::alloc` and the codegen
    // sometimes hands their pointer through as `bitcast i64 → double`
    // without a POINTER_TAG. Without this path, `Int32Array.length`
    // returned 0 because the value's top16 was 0, not 0x7FFD.
    // #1136: mirror the platform split above for raw-pointer-bitcast
    // values too, so a Buffer/TypedArray pointer handed through as
    // `bitcast i64 → double` on iOS device still resolves to its real
    // length via the registry lookups below.
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    let raw_heap_min: u64 = 0x1000;
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
    let raw_heap_min: u64 = 0x200_0000_0000;
    if top16 == 0 && bits >= raw_heap_min && bits < 0x8000_0000_0000 {
        let handle = bits as usize;
        if let Some(value) = unsafe {
            crate::typedarray_props::typed_array_get_property_value_by_name(handle, "length")
        } {
            return value;
        }
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { (*buf).length as f64 };
        }
        if crate::typedarray::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray::TypedArrayHeader;
            return unsafe { (*ta).length as f64 };
        }
    }

    // Everything else — undefined, null, booleans, int32, plain
    // doubles, BigInt pointers — has no `.length`.
    0.0
}

/// Unified object property access that handles both JS handle objects and native objects.
/// Also handles strings for property access like `.length`.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_object_get_property(
    obj_value: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
) -> f64 {
    // Check if this is a JS handle
    if is_js_handle(obj_value) {
        // Try to use the JS runtime function if it's been registered
        let func_ptr = JS_HANDLE_OBJECT_GET_PROPERTY.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleObjectGetPropertyFn = unsafe { std::mem::transmute(func_ptr) };
            return func(obj_value, property_name_ptr, property_name_len);
        }
        // JS runtime not available - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a NaN-boxed string - handle string properties like .length
    let bits = obj_value.to_bits();
    if (bits & TAG_MASK) == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if !str_ptr.is_null() {
            // Get the property name
            let name_slice = if property_name_ptr.is_null() {
                return f64::from_bits(TAG_UNDEFINED);
            } else if property_name_len > 0 {
                std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
            } else {
                std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
            };

            // Handle string properties
            if name_slice == b"length" {
                let len = crate::string::js_string_length(str_ptr);
                return len as f64;
            }
            // Other string properties return undefined
            return f64::from_bits(TAG_UNDEFINED);
        }
    }

    // A user-class ClassRef (`class X extends Promise` etc.) reaching this
    // generic dynamic getter — `GetPromiseResolve(C)`'s `Get(C, "resolve")`, a
    // `Subclass.resolve.bind(Subclass)` read — must resolve through the
    // class-ref-aware `js_object_get_field_by_name`, which checks a user static
    // override (`Subclass.resolve = fn`) FIRST and only then falls back to the
    // inherited builtin static. The plain pointer path below can't see class
    // refs and would return `undefined`.
    if crate::object::class_ref_id(obj_value).is_some() {
        let name_slice = if property_name_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        } else if property_name_len > 0 {
            std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
        } else {
            std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
        };
        let key = crate::string::js_string_from_bytes(name_slice.as_ptr(), name_slice.len() as u32);
        let class_ref_obj = obj_value.to_bits() as *const crate::object::ObjectHeader;
        return crate::object::js_object_get_field_by_name_f64(class_ref_obj, key);
    }

    // Not a JS handle - it's a native object pointer
    let ptr = js_nanbox_get_pointer(obj_value);

    if ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        if let Some(dispatch) = crate::object::handle_property_dispatch() {
            return dispatch(ptr, property_name_ptr as *const u8, property_name_len);
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Get the key string
    let name_slice = if property_name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    } else if property_name_len > 0 {
        std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
    } else {
        // Null-terminated C string
        std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(TAG_UNDEFINED),
    };

    // Check if this is a ClosureHeader (CLOSURE_MAGIC at offset 12).
    // ClosureHeader layout: func_ptr (8B), capture_count u32 (4B), type_tag u32 (4B), captures at 16+
    // ObjectHeader layout: object_type u32 (4B), class_id u32 (4B), parent_class_id u32 (4B), field_count u32 (4B), keys_array (8B), ...
    // Without this check, the closure's capture[0] at offset 16 would be read as keys_array → crash.
    if crate::closure::is_closure_ptr(ptr as usize) {
        return crate::closure::closure_get_dynamic_prop(ptr as usize, property_name);
    }

    // Handle Buffer/Uint8Array properties (buffer, byteOffset, byteLength, length)
    // BufferHeader has same layout as ArrayHeader (length u32, capacity u32, data...)
    // and doesn't have ObjectHeader fields, so we must check before treating as ObjectHeader.
    if crate::buffer::is_registered_buffer(ptr as usize) {
        let buf = ptr as *const crate::buffer::BufferHeader;
        match property_name {
            "length" | "byteLength" => {
                return (*buf).length as f64;
            }
            "byteOffset" | "offset" => {
                return crate::buffer::buffer_byte_offset(ptr as usize) as f64;
            }
            "buffer" | "parent" => {
                let alias = crate::buffer::buffer_backing_array_buffer(ptr as usize);
                return f64::from_bits(crate::value::js_nanbox_pointer(alias as i64).to_bits());
            }
            _ => {
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }

    // Check if this is a registered Map
    if crate::map::is_registered_map(ptr as usize) {
        let map_ptr = ptr as *const crate::map::MapHeader;
        if name_slice == b"size" {
            return (*map_ptr).size as f64;
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a registered Set
    if crate::set::is_registered_set(ptr as usize) {
        let set_ptr = ptr as *const crate::set::SetHeader;
        if name_slice == b"size" {
            return (*set_ptr).size as f64;
        }
        return f64::from_bits(TAG_UNDEFINED);
    }

    // #1545: Promise `then`/`catch`/`finally` value-reads return a bound
    // function (so `typeof p.then === "function"` and `const f = p.then`
    // work). Call-form `p.then(cb)` is lowered separately by codegen; this
    // covers the value-read path the generic getter previously dropped to
    // `undefined`.
    //
    // #5226: skip for off-GC-heap header-less allocations (small typed arrays
    // — buffers were already handled above): they have no `GcHeader`, so the
    // `ptr - GC_HEADER_SIZE` back-read faults at region boundaries. They are
    // never Promises; fall through to the normal property handling.
    if !crate::typedarray::is_offheap_sidetable_alloc(ptr as usize) {
        let gc_header = (ptr as usize - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        // Typed arrays are `std::alloc`-backed with no GcHeader, so the byte at
        // `ptr - 8` can read as `GC_TYPE_PROMISE` (5) by coincidence; exclude
        // them before acting. (Buffers/maps/sets were already handled above.)
        if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE
            && crate::typedarray::lookup_typed_array_kind(ptr as usize).is_none()
        {
            // A user-attached own expando (`p.status = "pending"`,
            // `Object.assign(p, …)`) wins over the inherited prototype method.
            // #5142: @tanstack/query-core's `pendingThenable()` stores `status`
            // / `value` on the promise and gates its retryer on
            // `thenable.status`; without this the read came back `undefined`,
            // `isResolved()` was permanently true, and the fetch never resolved.
            if let Some(v) = crate::object::exotic_expando::exotic_get_own_property(
                ptr as usize,
                crate::object::exotic_expando::ExoticKind::Promise,
                property_name,
                obj_value,
            ) {
                return v;
            }
            if matches!(property_name, "then" | "catch" | "finally") {
                if let Some(v) = crate::promise::js_promise_bound_method(
                    ptr as *mut crate::promise::Promise,
                    property_name,
                ) {
                    return v;
                }
            }
            // A Promise is a `GC_TYPE_PROMISE` cell, not an `ObjectHeader`;
            // never fall through to the field/vtable path below (it would
            // reinterpret the promise's bytes as object fields).
            return f64::from_bits(TAG_UNDEFINED);
        }
    }

    // Check the object type tag (first u32 field of both ObjectHeader and ErrorHeader)
    let object_type = *(ptr as *const u32);

    // Handle native module namespace objects (e.g., `const fn = fs.lstatSync`)
    // Create a bound method closure so the method reference can be called later
    let obj_header = ptr as *const crate::object::ObjectHeader;
    if (*obj_header).class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return crate::object::js_native_module_bind_method(
            obj_value,
            property_name.as_ptr(),
            property_name.len(),
        );
    }

    // Handle Error objects specially
    if object_type == crate::error::OBJECT_TYPE_ERROR {
        // An own expando / accessor property (installed via defineProperty, or a
        // reassigned `message`/`stack`) lives in the exotic side tables and wins
        // over the builtin slot. The compiled member-get path consults these,
        // but this lower-level dynamic getter — used by
        // `Object.defineProperties` to read each descriptor off the properties
        // bag — historically dropped straight to `undefined` for any key other
        // than the five native slots, so an accessor/data expando on an Error
        // read as `undefined` (and `defineProperties(obj, errObj)` then threw
        // "Property description must be an object: undefined").
        if let Some(v) = crate::object::exotic_expando::exotic_get_own_property(
            ptr as usize,
            crate::object::exotic_expando::ExoticKind::Error,
            property_name,
            obj_value,
        ) {
            return v;
        }
        let error_ptr = ptr as *mut crate::error::ErrorHeader;
        match property_name {
            "message" => {
                let msg = crate::error::js_error_get_message(error_ptr);
                return js_nanbox_string(msg as i64);
            }
            "name" => {
                let name = crate::error::js_error_get_name(error_ptr);
                return js_nanbox_string(name as i64);
            }
            "stack" => {
                let stack = crate::error::js_error_get_stack(error_ptr);
                return js_nanbox_string(stack as i64);
            }
            "cause" => {
                return crate::error::js_error_get_cause(error_ptr);
            }
            "errors" => {
                let arr = crate::error::js_error_get_errors(error_ptr);
                if arr.is_null() {
                    return f64::from_bits(TAG_UNDEFINED);
                }
                return js_nanbox_pointer(arr as i64);
            }
            _ => {
                // Error objects don't have other properties
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }

    // Check vtable for a registered getter or method before falling back to field lookup
    let class_id = (*obj_header).class_id;
    if class_id != 0 {
        if let Ok(registry) = crate::object::CLASS_VTABLE_REGISTRY.read() {
            if let Some(ref reg) = *registry {
                if let Some(vtable) = reg.get(&class_id) {
                    if let Some(&getter_ptr) = vtable.getters.get(property_name) {
                        // Methods take `this` as f64 (NaN-boxed), not i64.
                        // On Windows x64 ABI, i64 and f64 use different registers.
                        let f: extern "C" fn(f64) -> f64 = std::mem::transmute(getter_ptr);
                        return f(obj_value);
                    }
                    if vtable.methods.contains_key(property_name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(property_name.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(
                                property_name.as_ptr(),
                                ptr,
                                property_name.len(),
                            );
                            ptr
                        };
                        return crate::object::js_class_method_bind(
                            obj_value,
                            heap_name,
                            property_name.len(),
                        );
                    }
                }
            }
        }
    }

    // Create a Perry string for the key
    let key_ptr =
        crate::string::js_string_from_bytes(property_name.as_ptr(), property_name.len() as u32);

    // Call native object property access

    crate::object::js_object_get_field_by_name_f64(
        ptr as *const crate::object::ObjectHeader,
        key_ptr,
    )
}

/// Dynamic method dispatch for Map/Set collection types.
/// Checks the magic tag of the object and dispatches known methods.
/// Returns TAG_UNDEFINED if the object is not a Map/Set or method is unknown.
/// This handles cases like `map.get(key).add(value)` where the intermediate
/// result type is unknown at codegen time.
#[no_mangle]
pub unsafe extern "C" fn js_collection_method_dispatch(
    obj_value: f64,
    method_ptr: *const u8,
    method_len: usize,
    arg0: f64,
    arg1: f64,
) -> f64 {
    let ptr = js_nanbox_get_pointer(obj_value);
    if ptr == 0 || ptr < 0x10000 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let method = std::slice::from_raw_parts(method_ptr, method_len);

    // Check if this is a registered Map
    if crate::map::is_registered_map(ptr as usize) {
        let map = ptr as *mut crate::map::MapHeader;
        return match method {
            b"get" => crate::map::js_map_get(map, arg0),
            b"set" => {
                let result = crate::map::js_map_set(map, arg0, arg1);
                js_nanbox_pointer(result as i64)
            }
            b"has" => crate::map::js_map_has(map, arg0) as f64,
            b"delete" => crate::map::js_map_delete(map, arg0) as f64,
            b"size" => crate::map::js_map_size(map) as f64,
            b"clear" => {
                crate::map::js_map_clear(map);
                f64::from_bits(TAG_UNDEFINED)
            }
            b"entries" => {
                let arr = crate::map::js_map_entries(map);
                js_nanbox_pointer(arr as i64)
            }
            b"keys" => {
                let arr = crate::map::js_map_keys(map);
                js_nanbox_pointer(arr as i64)
            }
            b"values" => {
                let arr = crate::map::js_map_values(map);
                js_nanbox_pointer(arr as i64)
            }
            _ => f64::from_bits(TAG_UNDEFINED),
        };
    }

    // Check if this is a registered Set
    if crate::set::is_registered_set(ptr as usize) {
        let set = ptr as *mut crate::set::SetHeader;
        return match method {
            b"add" => {
                let result = crate::set::js_set_add(set, arg0);
                js_nanbox_pointer(result as i64)
            }
            b"has" => crate::set::js_set_has(set, arg0) as f64,
            b"delete" => crate::set::js_set_delete(set, arg0) as f64,
            b"size" => crate::set::js_set_size(set) as f64,
            b"clear" => {
                crate::set::js_set_clear(set);
                f64::from_bits(TAG_UNDEFINED)
            }
            _ => f64::from_bits(TAG_UNDEFINED),
        };
    }

    f64::from_bits(TAG_UNDEFINED)
}

/// Dynamic Object.keys() that handles both regular objects and Error objects.
/// Takes a raw pointer (extracted from NaN-boxed value) and returns array of keys.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_object_keys(ptr: i64) -> *mut crate::array::ArrayHeader {
    if ptr == 0 {
        return crate::array::js_array_alloc(0);
    }

    // Check the object type tag (first u32 field of both ObjectHeader and ErrorHeader)
    let object_type = *(ptr as *const u32);

    // Handle Error objects specially - they have fixed keys
    if object_type == crate::error::OBJECT_TYPE_ERROR {
        // Error objects have keys: "message", "name", "stack"
        let keys = crate::array::js_array_alloc(3);

        let msg_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
        crate::array::js_array_push(keys, JSValue::string_ptr(msg_key));

        let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        crate::array::js_array_push(keys, JSValue::string_ptr(name_key));

        let stack_key = crate::string::js_string_from_bytes(b"stack".as_ptr(), 5);
        crate::array::js_array_push(keys, JSValue::string_ptr(stack_key));

        return keys;
    }

    // Regular object - delegate to js_object_keys
    crate::object::js_object_keys(ptr as *const crate::object::ObjectHeader)
}

/// Get a property from an object by name.
/// This is the main entry point used by codegen for dynamic property access.
/// Delegates to js_dynamic_object_get_property which handles JS handles, native objects,
/// strings, and error objects.
///
/// Parameters:
/// - object: NaN-boxed f64 containing the object
/// - name_ptr: i64 pointer to the property name bytes
/// - name_len: i64 length of the property name
///
/// Returns: NaN-boxed f64 containing the property value (or undefined)
#[no_mangle]
pub unsafe extern "C" fn js_get_property(object: f64, name_ptr: i64, name_len: i64) -> f64 {
    js_dynamic_object_get_property(object, name_ptr as *const i8, name_len as usize)
}

#[cfg(test)]
mod length_handle_band_tests {
    use super::*;
    use crate::value::addr_class;

    /// A POINTER_TAG-boxed *handle-band* value (Web Fetch
    /// Headers/Request/Response/Blob ids, net/http handles, zlib/proxy ids —
    /// all `< 0x100000`) is a registry id, not a heap pointer. Reaching the
    /// `.length` slow path with such a value must return `0.0` WITHOUT
    /// dereferencing the raw id (which would SIGSEGV at the unmapped low
    /// address). Regression for the doctor / mcp-list startup crash where a
    /// Fetch handle (e.g. `0x40005`) flowed into a `.length` site; the inline
    /// codegen fast path now band-gates before falling in here, and this slow
    /// path rejects the band explicitly so it is safe on every platform.
    #[test]
    fn fetch_band_handle_length_is_zero_not_a_deref() {
        // Sample one id from each handle sub-band, including the exact band
        // boundaries the bug touched.
        for &id in &[
            1usize,                                  // common native handle
            addr_class::FETCH_HANDLE_BAND_START,     // 0x40000
            addr_class::FETCH_HANDLE_BAND_START + 5, // 0x40005 — the crash addr
            addr_class::FETCH_HANDLE_BAND_END - 1,   // top of fetch band
            addr_class::PROXY_ID_BAND_START,         // revocable-proxy id
            addr_class::HANDLE_BAND_MAX - 1,         // 0xFFFFF — last handle-band id
        ] {
            assert!(addr_class::is_handle_band(id));
            let boxed = crate::value::js_nanbox_pointer(id as i64);
            // Must not crash and must report no length.
            assert_eq!(
                js_value_length_f64(boxed),
                0.0,
                "handle-band id {id:#x} must yield length 0 without dereferencing"
            );
        }
    }
}
