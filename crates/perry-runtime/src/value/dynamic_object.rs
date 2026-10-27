//! Type-erased object / collection property + method dispatchers, plus
//! `js_value_length_f64` which the inline `.length` codegen falls into
//! when the receiver's GC-type byte doesn't statically prove the kind.

use super::*;
use std::sync::atomic::Ordering;

#[inline(always)]
fn is_length_heap_addr(addr: usize) -> bool {
    crate::value::addr_class::is_plausible_heap_addr(addr)
}

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

    if top16 == (crate::value::HANDLE_TAG >> 48) {
        return 0.0;
    }

    // SHORT_STRING_TAG stores a byte count; JS length counts UTF-16 units.
    if top16 == 0x7FF9 {
        return JSValue::from_bits(bits).short_string_utf16_len() as f64;
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
        // ids, revocable-proxy ids — all `< HANDLE_BAND_MAX`) is a registry id, not a
        // heap pointer. None of them carry a `.length`, and dereferencing the
        // raw id (`handle - 8` GcHeader read, or `*handle` u32 below) hits
        // unmapped low memory → SIGSEGV. Reject the band explicitly before
        // applying the platform heap-range classifier below. Matches the band
        // gating the inline `.length` codegen fast path now applies before
        // falling in here.
        if crate::value::addr_class::is_handle_band(handle) {
            return 0.0;
        }
        // Keep this path on the canonical platform heap range. The duplicated
        // floor here omitted macOS and rejected legitimate low mappings there
        // (#10271); it also could not follow the wider AArch64 Linux ceiling.
        if !is_length_heap_addr(handle) {
            return 0.0;
        }
        if let Some(value) = unsafe {
            crate::typedarray_props::typed_array_get_property_value_by_name(handle, "length")
        } {
            return value;
        }
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { crate::buffer::store::raw_length(buf as usize) as f64 };
        }
        if crate::typedarray_hook_slots::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray_hook_slots::TypedArrayHeader;
            return unsafe { crate::buffer::store::raw_length(ta as usize) as f64 };
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
            // A closure's `.length` is its spec param count. Prefer
            // native/builtin metadata over the codegen-registered physical
            // wrapper length. Without those metadata checks, a named native
            // constructor such as `Console` reports its zero-argument wrapper
            // arity instead of its declared arity 1. This keeps the intrinsic
            // fallback aligned with the generic PropertyGet reflection path.
            crate::gc::GC_TYPE_CLOSURE => {
                if let Some(arity) =
                    unsafe { crate::object::bound_native_callable_value_arity(value) }
                {
                    return arity as f64;
                }
                if let Some(length) = crate::object::builtin_closure_length(handle) {
                    return length as f64;
                }
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
    // #1136: use the same canonical platform range for raw-pointer-bitcast
    // values too, so a Buffer/TypedArray pointer handed through as
    // `bitcast i64 → double` resolves to its real length on every target.
    // A raw word is also what a subnormal number looks like, and the probes
    // below read the cell's header (#10694), so the allocator vouches first.
    if top16 == 0
        && is_length_heap_addr(bits as usize)
        && crate::buffer::header_is_owned(bits as usize)
    {
        let handle = bits as usize;
        if let Some(value) = unsafe {
            crate::typedarray_props::typed_array_get_property_value_by_name(handle, "length")
        } {
            return value;
        }
        if crate::buffer::is_registered_buffer(handle) {
            let buf = handle as *const crate::buffer::BufferHeader;
            return unsafe { crate::buffer::store::raw_length(buf as usize) as f64 };
        }
        if crate::typedarray_hook_slots::lookup_typed_array_kind(handle).is_some() {
            let ta = handle as *const crate::typedarray_hook_slots::TypedArrayHeader;
            return unsafe { crate::buffer::store::raw_length(ta as usize) as f64 };
        }
    }

    // Everything else — undefined, null, booleans, int32, plain
    // doubles, BigInt pointers — has no `.length`.
    0.0
}

/// Read `.length` with ordinary JavaScript property semantics.
///
/// [`js_value_length_f64`] is deliberately numeric: array internals feed its
/// result through `ToLength`, so a missing property historically collapses to
/// zero there.  A source-level `receiver.length` read cannot use that sentinel
/// when its inline layout guard misses.  TypeScript annotations are erased, so
/// a receiver declared as an array may hold a number (whose `length` is
/// `undefined`), an object with a non-numeric `length`, or a nullish value
/// (which must throw).
///
/// Keep the SSO case here because the general dynamic property getter has no
/// heap pointer to dispatch through.  Everything else delegates to that getter
/// so Proxy traps, native handles, accessors, inherited properties, functions,
/// buffers, typed arrays, and ordinary objects all share the normal property
/// lookup rather than a second `.length` implementation.
#[no_mangle]
pub extern "C" fn js_value_length_property_f64(value: f64) -> f64 {
    value_length_property(value)
}

/// Pooled-key ABI for source-level length reads. The key global is rewritten
/// by moving GC; root it before any array-subclass priming or user getter.
#[no_mangle]
pub extern "C" fn js_value_length_property_key_f64(
    value: f64,
    key: *const crate::StringHeader,
) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        crate::error::js_throw_type_error_property_access(
            jsval.is_null() as u32,
            b"length".as_ptr(),
            6,
        );
    }
    if jsval.is_short_string() {
        return jsval.short_string_utf16_len() as f64;
    }
    if let Some(length) = unsafe { crate::typedarray_named::try_get(jsval, b"length") } {
        return length;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let key = scope.root_raw_const_ptr(key);
    if let Some(length) = crate::array::array_subclass_fast_length(value) {
        return length;
    }
    let bits = receiver.get_nanbox_f64().to_bits();
    crate::object::js_object_get_field_by_name_f64(
        bits as *const crate::object::ObjectHeader,
        key.get_raw_const_ptr(),
    )
}

fn value_length_property(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        crate::error::js_throw_type_error_property_access(
            jsval.is_null() as u32,
            b"length".as_ptr(),
            6,
        );
    }

    if jsval.is_string() {
        return crate::string::js_string_length(jsval.as_string_ptr()) as f64;
    }
    if jsval.is_number() || jsval.is_bool() {
        // The live prototype shapes prove absence. A user-installed length
        // or accessor still takes ordinary Get with the primitive as `this`.
        let prototype = crate::array::primitive_wrapper_prototype_addr(value);
        if prototype != 0 {
            if let Some(value) = unsafe {
                crate::object::native_get::try_data_lookup_bytes(
                    JSValue::pointer(prototype as *mut u8),
                    b"length",
                )
            } {
                return f64::from_bits(value.unwrap_or_else(JSValue::undefined).bits());
            }
        }
        // The dynamic-object dispatcher does not implement primitive
        // prototype Get. Declined proofs must use the named getter, which
        // preserves accessors and their original primitive `this` value.
        let key = crate::string::intern_ascii_literal(b"length");
        return crate::object::js_object_get_field_by_name_f64(
            value.to_bits() as usize as *const crate::object::ObjectHeader,
            key,
        );
    }

    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(value) {
        if matches!(
            crate::builtins::boxed_primitive_to_string_tag(value),
            Some("String")
        ) {
            return value_length_property(payload);
        }
    }

    if jsval.is_short_string() {
        return jsval.short_string_utf16_len() as f64;
    }

    if let Some(length) = crate::array::array_subclass_fast_length(value) {
        return length;
    }

    // Preserve primitive prototypes (including BigInt/Symbol) and the same
    // receiver ABI the property's ordinary IC miss uses. Interning may collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let key = crate::string::intern_ascii_literal(b"length");
    crate::object::js_object_get_field_by_name_f64(
        receiver.get_nanbox_u64() as usize as *const crate::object::ObjectHeader,
        key,
    )
}

/// Unified object property access that handles both JS handle objects and native objects.
/// Also handles strings for property access like `.length`.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_object_get_property(
    obj_value: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
) -> f64 {
    if !property_name_ptr.is_null() && property_name_len != 0 {
        let name = std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len);
        if let Some(value) =
            crate::typedarray_named::try_get(JSValue::from_bits(obj_value.to_bits()), name)
        {
            return value;
        }
        if let Some(value) = crate::object::native_get::try_data_get_bytes(
            JSValue::from_bits(obj_value.to_bits()),
            name,
        ) {
            return f64::from_bits(value.bits());
        }
    }
    // A revocable Proxy value reaching this generic dynamic getter must go
    // through the Proxy's `get` trap. Proxy ids live at the TOP of the handle
    // band ([PROXY_ID_BAND_START, HANDLE_BAND_MAX)), so without this branch the
    // `is_handle_band` dispatch further down routes the read to the stdlib
    // handle dispatcher and the trap never fires. The Promise resolution
    // procedure's `Get(resolution, "then")` runs through here, so
    // `Promise.resolve(proxy)` / `await proxy` on a thenable Proxy (React's RSC
    // client-module reference, whose `.then` trap lazily marks the module as an
    // async ESM export) silently skipped the trap — #5989: the Flight stream
    // dropped the `,1` async flag on client references. Mirror the proxy branch
    // in `js_object_get_field_by_name`. `js_proxy_is_proxy` confirms a
    // *registered* proxy so a real heap object with a small address isn't
    // misrouted.
    if !property_name_ptr.is_null() {
        let addr = js_nanbox_get_pointer(obj_value);
        if addr != 0 && crate::value::addr_class::is_proxy_cell(addr as usize) {
            let boxed = f64::from_bits(crate::value::POINTER_TAG | (addr as u64 & POINTER_MASK));
            if crate::proxy_hooks::proxy_is_proxy(boxed) != 0 {
                let name_slice = if property_name_len > 0 {
                    std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
                } else {
                    std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char)
                        .to_bytes()
                };
                let key = crate::string::intern_ascii_literal(name_slice);
                let key_f64 = crate::value::js_nanbox_string(key as i64);
                return crate::proxy_hooks::proxy_get(boxed, key_f64);
            }
        }
    }

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
    // Inline SSO string (#11519): it carries no header, so the heap arm
    // below cannot see it, and the pointer path further down unboxes it to 0.
    let sso = crate::value::JSValue::from_bits(bits);
    if sso.is_short_string() {
        if property_name_ptr.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let name_slice = if property_name_len > 0 {
            std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
        } else {
            std::ffi::CStr::from_ptr(property_name_ptr as *const std::ffi::c_char).to_bytes()
        };
        if name_slice == b"length" {
            return sso.short_string_utf16_len() as f64;
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
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
        let key = crate::string::intern_ascii_literal(name_slice);
        let class_ref_obj = obj_value.to_bits() as *const crate::object::ObjectHeader;
        return crate::object::js_object_get_field_by_name_f64(class_ref_obj, key);
    }

    // Not a JS handle - it's a native object pointer
    let ptr = js_nanbox_get_pointer(obj_value);

    if ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    if JSValue::from_bits(obj_value.to_bits()).is_handle() {
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

    if crate::typedarray_named::is_metadata_name(name_slice) {
        if let Some(header) = crate::value::addr_class::try_read_gc_header(ptr as usize) {
            if crate::gc::is_byte_family_type(header.obj_type) {
                let scope = crate::gc::RuntimeHandleScope::new();
                let receiver = scope.root_nanbox_f64(obj_value);
                let key = crate::string::intern_ascii_literal(name_slice);
                let bits = receiver.get_nanbox_f64().to_bits();
                return crate::object::js_object_get_field_by_name_f64(
                    bits as *const crate::object::ObjectHeader,
                    key,
                );
            }
        }
    }

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(TAG_UNDEFINED),
    };

    // #7930: TypedArrayHeader starts with `length: u32`, at the same payload
    // offset where ObjectHeader used to store its object-type word. Classify
    // the receiver through the authoritative side table before any
    // header-shaped dispatch below: a two-element typed array otherwise read as
    // `OBJECT_TYPE_ERROR == 2`, so `.length` / `.byteLength` entered the Error
    // branch and returned `undefined` even though construction was correct.
    // (#8113 replaced that raw read with a `GcHeader` kind test, which no
    // longer confuses the two — but the side-table classification below is
    // still what gives the typed array its property semantics.)
    //
    // Delegate to the normal by-name typed-array path rather than duplicating
    // its property semantics here. It gives an own expando/accessor precedence
    // over the inherited `length` accessor and handles prototype mutations,
    // indexed keys, `buffer`, and the remaining typed-array builtins. Creating
    // the StringHeader may collect, so keep the old-generation typed-array
    // receiver live and re-read its address after that allocation.
    if crate::typedarray_hook_slots::lookup_typed_array_kind(ptr as usize).is_some()
        || crate::buffer::is_uint8array_buffer(ptr as usize)
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver =
            scope.root_raw_const_ptr(ptr as *const crate::typedarray_hook_slots::TypedArrayHeader);
        let (key, typed) =
            receiver.across_const(|| crate::string::intern_ascii_literal(name_slice));
        return crate::object::js_object_get_field_by_name_f64(
            typed as *const crate::object::ObjectHeader,
            key,
        );
    }

    // Check if this is a ClosureHeader (CLOSURE_MAGIC at offset 12).
    // ClosureHeader layout: func_ptr (8B), capture_count u32 (4B), type_tag u32 (4B), captures at 16+
    // ObjectHeader layout (#8047): class_id u32 (4B), parent_class_id u32 (4B), meta (8B).
    // Without this check, the closure header would be interpreted as an object descriptor token.
    if crate::closure::is_closure_ptr(ptr as usize) {
        // A generator function declares `prototype`: an unmaterialized slot
        // is born with the function's bag (allocates).
        if property_name == "prototype"
            && crate::closure::props::closure_has_generator_body(
                ptr as *const crate::closure::ClosureHeader,
            )
            && crate::closure::props::bag_of(ptr as usize).is_null()
        {
            if let Some(prototype) = crate::object::generator_function_prototype_of(ptr as usize) {
                return prototype;
            }
        }
        return crate::closure::closure_get_dynamic_prop(ptr as usize, property_name);
    }

    // Handle Buffer/Uint8Array properties (buffer, byteOffset, byteLength, length)
    // BufferHeader has same layout as ArrayHeader (length u32, capacity u32, data...)
    // and doesn't have ObjectHeader fields, so we must check before treating as ObjectHeader.
    if crate::buffer::is_registered_buffer(ptr as usize) {
        let buf = ptr as *const crate::buffer::BufferHeader;
        match property_name {
            // #8149: `length` is a `%TypedArray%` slot. An `ArrayBuffer` /
            // `SharedArrayBuffer` / `DataView` exposes only `byteLength`, so
            // node answers `undefined` for `dv.length` and `ab.length`. Asked
            // ABOVE the shared arm, which answers the byte count for both
            // spellings. (`js_value_length_f64`, the deliberately-numeric
            // sibling above, is left alone: its callers feed the result through
            // `ToLength`, where `undefined` and `0` are the same answer.)
            "length" if crate::buffer::is_non_indexed_buffer_view(ptr as usize) => {
                return f64::from_bits(TAG_UNDEFINED);
            }
            "length" | "byteLength" => {
                return crate::buffer::store::raw_length(buf as usize) as f64;
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
    if !crate::typedarray_hook_slots::is_offheap_sidetable_alloc(ptr as usize) {
        let gc_header = (ptr as usize - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        // Typed arrays are `std::alloc`-backed with no GcHeader, so the byte at
        // `ptr - 8` can read as `GC_TYPE_PROMISE` (5) by coincidence; exclude
        // them before acting. (Buffers/maps/sets were already handled above.)
        if (*gc_header).obj_type == crate::gc::GC_TYPE_PROMISE
            && crate::typedarray_hook_slots::lookup_typed_array_kind(ptr as usize).is_none()
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

    // #8113: `GcHeader.obj_type == GC_TYPE_ERROR`. This used to read the punned
    // `object_type` word at offset 0; offset 0 is `class_id` now, so the raw
    // read would classify every object whose class id happens to be
    // `OBJECT_TYPE_ERROR` (= 2) as an Error and hand its field slots to
    // `ErrorHeader`'s accessors.
    let is_native_error = crate::error::ptr_is_native_error(ptr as usize);

    // Handle native module namespace objects (e.g., `const fn = fs.lstatSync`)
    // Create a bound method closure so the method reference can be called
    // later. Routed through the armed ops table (see `nm_namespace_hooks`) so
    // binaries with no module imports don't link the bind/dispatch machinery;
    // unarmed + matching class_id is unreachable (only the arming bootstrap
    // assigns NATIVE_MODULE_CLASS_ID).
    let obj_header = ptr as *const crate::object::ObjectHeader;
    if (*obj_header).class_id == crate::object::NATIVE_MODULE_CLASS_ID {
        return match crate::object::nm_namespace_ops() {
            Some(ops) => (ops.bind_method)(obj_value, property_name.as_ptr(), property_name.len()),
            None => f64::from_bits(TAG_UNDEFINED),
        };
    }

    // Handle Error objects specially
    if is_native_error {
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

    // #8220: root the receiver across the key-string allocation. `ptr` was
    // extracted at the top of this function from the NaN-boxed `obj_value` and
    // is a raw `*const` to a nursery-eligible heap object. An intern-cache miss
    // allocates a StringHeader and can trigger a copying
    // minor that evacuates the receiver. Without rooting, `ptr` becomes a stale
    // from-space pointer and the `js_object_get_field_by_name_f64` call below
    // dereferences retired memory — the #8220 class (raw pointer in a Rust
    // frame local, invisible to the precise root map).
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_const_ptr(ptr as *const crate::object::ObjectHeader);
    let (key_ptr, obj_ptr) =
        receiver.across_const(|| crate::string::intern_ascii_literal(property_name.as_bytes()));

    // Call native object property access with the post-collection pointer.

    crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
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

    // #8113: `GcHeader.obj_type == GC_TYPE_ERROR` — see `js_dynamic_get_property`.
    // Handle Error objects specially - they have fixed keys
    if crate::error::ptr_is_native_error(ptr as usize) {
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

    #[test]
    fn primitive_length_absence_uses_live_shapes_and_sees_mutations() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::object::js_get_global_this();
        let scope = crate::gc::RuntimeHandleScope::new();
        let prototype = scope
            .root_raw_mut_ptr(crate::array::primitive_wrapper_prototype_addr(42.0)
                as *mut crate::object::ObjectHeader);
        let key = scope.root_string_ptr(crate::string::intern_ascii_literal(b"length"));
        prototype.with_mut_ptr(|p: *mut crate::object::ObjectHeader| unsafe {
            assert!(
                matches!(
                    crate::object::native_get::try_data_lookup_bytes(
                        JSValue::pointer(p.cast()),
                        b"length",
                    ),
                    Some(None)
                ),
                "the absent shape path must actually be live"
            );
        });
        assert_eq!(js_value_length_property_f64(42.0).to_bits(), TAG_UNDEFINED);
        prototype.with_mut_ptr(|p| {
            key.with_const_ptr(|k| {
                crate::object::js_object_set_field_by_name(p, k, 73.0);
            })
        });
        assert_eq!(js_value_length_property_f64(42.0), 73.0);
        prototype.with_mut_ptr(|p| {
            key.with_const_ptr(|k| {
                assert_eq!(crate::object::js_object_delete_field(p, k), 1);
            })
        });
        assert_eq!(js_value_length_property_f64(42.0).to_bits(), TAG_UNDEFINED);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn low_macos_allocation_reaches_both_length_dispatch_paths() {
        let _lock = crate::gc::global_side_table_test_lock();
        crate::gc::initialize_runtime_fixture();
        let scope = crate::gc::RuntimeHandleScope::new();
        let array = scope.root_raw_mut_ptr(crate::typedarray_hook_slots::typed_array_alloc(
            crate::typedarray_hook_slots::KIND_UINT8,
            37,
        ));
        let addr =
            array.get_raw_mut_ptr::<crate::typedarray_hook_slots::TypedArrayHeader>() as usize;
        assert!(
            (addr_class::HANDLE_BAND_MAX..0x200_0000_0000).contains(&addr),
            "fixture must exercise the macOS address range that the old 2 TiB floor rejected; got {addr:#x}"
        );
        // Admission requires allocator ownership as well as a typed-array
        // header. A private mmap with a fabricated header proves neither;
        // use the real byte-store birth while retaining the low-address gate.
        assert_eq!(
            js_value_length_f64(crate::value::js_nanbox_pointer(addr as i64)),
            37.0,
            "POINTER_TAG length dispatch must accept a live low macOS allocation"
        );
        assert_eq!(
            js_value_length_f64(f64::from_bits(addr as u64)),
            37.0,
            "raw-bitcast length dispatch must accept the same allocation"
        );
    }

    /// A POINTER_TAG-boxed *handle-band* value (Web Fetch
    /// Headers/Request/Response/Blob ids, net/http handles, zlib/proxy ids —
    /// all `< HANDLE_BAND_MAX`) is a registry id, not a heap pointer. Reaching the
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
            addr_class::HANDLE_BAND_MAX - 1,         // 0x1FFFFF — last handle-band id
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

    #[test]
    fn named_native_callable_length_paths_use_declared_arity() {
        let console_ctor = unsafe {
            crate::object::js_native_module_property_by_name(
                b"console".as_ptr(),
                "console".len(),
                b"Console".as_ptr(),
                "Console".len(),
            )
        };
        let console_ptr = crate::value::js_nanbox_get_pointer(console_ctor) as usize;
        let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        assert_eq!(js_value_length_f64(console_ctor), 1.0);
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(console_ptr, "length"),
            1.0
        );
        assert_eq!(
            crate::object::field_get_set::get_field_miss(
                console_ptr as *const crate::ObjectHeader,
                length_key
            ),
            1.0
        );
    }

    #[test]
    fn property_length_preserves_missing_and_non_numeric_values() {
        assert_eq!(
            js_value_length_property_f64(42.0).to_bits(),
            crate::value::TAG_UNDEFINED,
            "a number has no length property"
        );

        let obj = crate::object::js_object_alloc(0, 1);
        let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let seven = crate::string::js_string_from_bytes(b"seven".as_ptr(), 5);
        let seven_value = crate::value::js_nanbox_string(seven as i64);
        crate::object::js_object_set_field_by_name(obj, length_key, seven_value);
        let boxed_obj = crate::value::js_nanbox_pointer(obj as i64);

        assert_eq!(
            js_value_length_property_f64(boxed_obj).to_bits(),
            seven_value.to_bits(),
            "a source-level property read must not coerce its value"
        );
    }

    /// #7930: `TypedArrayHeader::length` shares payload offset zero with an
    /// `ObjectHeader`'s type word. A two-element typed array must be classified
    /// by its registry entry before that word can masquerade as
    /// `OBJECT_TYPE_ERROR == 2` in the generic property getter.
    #[test]
    fn length_two_typed_array_never_enters_error_object_dispatch() {
        let typed = crate::typedarray_hook_slots::js_typed_array_new(
            crate::typedarray_hook_slots::KIND_INT32 as i32,
            2.5,
        );
        assert_eq!(
            crate::typedarray_hook_slots::js_typed_array_length(typed),
            2,
            "test premise: ToIndex truncates the fractional constructor length"
        );

        let boxed = crate::value::js_nanbox_pointer(typed as i64);
        assert_eq!(js_value_length_property_f64(boxed), 2.0);
        assert_eq!(
            unsafe {
                js_dynamic_object_get_property(boxed, b"byteLength".as_ptr() as *const i8, 10)
            },
            8.0
        );
    }

    /// A Uint8Array uses BufferHeader storage rather than TypedArrayHeader
    /// registry storage, but its ordinary own properties still live in the
    /// shared typed-array property table. The property-semantic length helper
    /// must consult that table before the intrinsic BufferHeader length.
    #[test]
    fn uint8array_own_length_shadows_intrinsic_length() {
        let typed = crate::buffer::js_uint8array_alloc(3);
        assert!(crate::buffer::is_uint8array_buffer(typed as usize));
        assert!(unsafe {
            crate::typedarray_props::typed_array_set_property_by_name(typed as usize, "length", 1.0)
        });

        let boxed = crate::value::js_nanbox_pointer(typed as i64);
        assert_eq!(js_value_length_property_f64(boxed), 1.0);
    }
}
