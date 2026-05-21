//! V8 materialization for perry-stdlib Web Fetch handles (Request /
//! Response / Headers / Blob) plus the master `native_object_to_v8`
//! dispatcher for POINTER_TAG values crossing into V8.

use super::*;

use super::conversion::{native_promise_to_v8, native_to_v8};
use super::native_class::native_closure_to_v8;
use super::sqlite_proxy::{
    is_sqlite_db_handle, is_sqlite_stmt_handle, materialize_sqlite_db_proxy,
    materialize_sqlite_stmt_proxy,
};

/// Materialize a snapshot v8 Object for a perry-stdlib Web Fetch handle
/// (Request / Response). Properties are extracted via the public dispatch
/// helpers in `perry_stdlib::fetch`. Headers/Blob ids return `v8::null`
/// for now — they expose methods, not scalar properties, and adding method
/// bridging requires a Proxy + HANDLE_METHOD_DISPATCH callback (future work).
pub(super) fn materialize_web_fetch_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: usize,
) -> v8::Local<'s, v8::Value> {
    if handle_id == 0 {
        return v8::null(scope).into();
    }

    // sqlite Database / Statement proxies (refs #1022). Drizzle's
    // BetterSQLiteSession reads `this.client.prepare(query.sql)` /
    // `stmt.run(...)` from session.js running in V8 fallback. Without
    // these, the small handle id flows through to the unknown-id
    // fallback and surfaces as `v8::null`, then drizzle throws
    // `Cannot read properties of null (reading 'prepare')`. Detect
    // sqlite handles up front and synthesize a method-bearing proxy
    // before any other materializer runs.
    if is_sqlite_db_handle(handle_id) {
        return materialize_sqlite_db_proxy(scope, handle_id as i64);
    }
    if is_sqlite_stmt_handle(handle_id) {
        return materialize_sqlite_stmt_proxy(scope, handle_id as i64);
    }

    // Try Request first — read a probe property to confirm membership.
    if let Some(url_f64) = perry_stdlib::dispatch_request_property(handle_id, "url") {
        let obj = v8::Object::new(scope);
        let url_v8 = native_to_v8(scope, url_f64);
        if let Some(k) = v8::String::new(scope, "url") {
            obj.set(scope, k.into(), url_v8);
        }
        if let Some(method_f64) = perry_stdlib::dispatch_request_property(handle_id, "method") {
            let m = native_to_v8(scope, method_f64);
            if let Some(k) = v8::String::new(scope, "method") {
                obj.set(scope, k.into(), m);
            }
        }
        if let Some(body_f64) = perry_stdlib::dispatch_request_property(handle_id, "body") {
            let b = native_to_v8(scope, body_f64);
            if let Some(k) = v8::String::new(scope, "body") {
                obj.set(scope, k.into(), b);
            }
        }
        return obj.into();
    }

    // Then Response.
    if let Some(status_f64) = perry_stdlib::dispatch_response_property(handle_id, "status") {
        let obj = v8::Object::new(scope);
        let status_v8 = native_to_v8(scope, status_f64);
        if let Some(k) = v8::String::new(scope, "status") {
            obj.set(scope, k.into(), status_v8);
        }
        if let Some(st_f64) = perry_stdlib::dispatch_response_property(handle_id, "statusText") {
            let v = native_to_v8(scope, st_f64);
            if let Some(k) = v8::String::new(scope, "statusText") {
                obj.set(scope, k.into(), v);
            }
        }
        if let Some(ok_f64) = perry_stdlib::dispatch_response_property(handle_id, "ok") {
            let v = native_to_v8(scope, ok_f64);
            if let Some(k) = v8::String::new(scope, "ok") {
                obj.set(scope, k.into(), v);
            }
        }
        return obj.into();
    }

    // Unknown handle id — return null (safe fallback, no segfault).
    v8::null(scope).into()
}

/// Convert a native object pointer to a V8 object
pub(super) fn native_object_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Value> {
    if ptr.is_null() {
        return v8::null(scope).into();
    }

    // perry-stdlib's Web Fetch handles (Request / Response / Headers / Blob)
    // arrive here NaN-boxed as POINTER_TAG values whose lower 48 bits hold a
    // small registry id (1, 2, 3, ...) instead of a real heap pointer (see
    // `perry_stdlib::fetch::handle_to_f64`). Mirror the perry-runtime side
    // small-handle threshold (`object.rs:4665`, `< 0x100000`): below that,
    // the value is a handle id, not a dereferenceable pointer. Without this
    // guard the `gc_header_ptr = ptr - 8` arithmetic below wraps to a huge
    // unsigned value, passes the `> 0x1000` bounds check, and segfaults when
    // we deref `gc_header` (the hono `app.fetch(req)` crash where `req` came
    // back from `new Request(...)` as `0x7FFD_0000_0000_0001`).
    //
    // For Request and Response we materialize a real v8 Object so V8-side code
    // (hono, sveltekit, etc.) can read `request.url` / `response.status` etc.
    // The synthesized object is a snapshot — methods like `req.text()` and
    // streaming semantics aren't bridged here yet (would require a Proxy that
    // calls back through HANDLE_METHOD_DISPATCH). For unknown small ids fall
    // through to `v8::null` rather than crashing.
    let ptr_usize = ptr as usize;
    if ptr_usize < 0x10_0000 {
        return materialize_web_fetch_handle(scope, ptr_usize);
    }

    // Issue (jose JWT blocker): Uint8Array / TypedArray pointers crossing
    // into V8 used to fall through to the generic `v8::Array` branch,
    // which turned a perry Uint8Array into a v8 Array. Libraries running
    // in the V8 fallback (jose, jsonwebtoken) check `instanceof Uint8Array`
    // on signing inputs/outputs and fail with "Received an instance of
    // Array". Detect typed-array pointers via the runtime's registry and
    // materialize a real v8 `Uint8Array` (or matching TypedArray) with a
    // copy of the underlying bytes so V8 owns the backing store.
    //
    // Two perry representations cross the boundary here:
    //   - `TypedArrayHeader` — `new Uint8Array([..])` and TypedArray ops.
    //   - `BufferHeader` marked via `mark_as_uint8array` — what
    //     `TextEncoder().encode(...)` and `Buffer.from(...)` return.
    //     Layout is identical (`length: u32, capacity: u32`) but the
    //     "kind" is implicit (always uint8) and tracked in a separate
    //     registry. Handle both before the generic-object branch.
    {
        let buf_addr = ptr as usize;
        // BufferHeader path: registered Uint8Array buffer with the
        // packed-u8 layout. Must materialize as v8 Uint8Array so jose's
        // `instanceof Uint8Array` checks pass.
        let is_buf = perry_runtime::buffer::is_registered_buffer(buf_addr);
        let is_marked_u8 = perry_runtime::buffer::is_uint8array_buffer(buf_addr);
        if is_buf || is_marked_u8 {
            let buf = ptr as *const perry_runtime::buffer::BufferHeader;
            let length = unsafe { (*buf).length } as usize;
            let data_ptr = unsafe {
                (ptr as *const u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>())
            };
            let ab = v8::ArrayBuffer::new(scope, length);
            if length > 0 {
                let bs = ab.get_backing_store();
                let dst = bs.data().map(|nn| nn.as_ptr() as *mut u8);
                if let Some(dst) = dst {
                    unsafe { std::ptr::copy_nonoverlapping(data_ptr, dst, length) };
                }
            }
            if let Some(ta) = v8::Uint8Array::new(scope, ab, 0, length) {
                return ta.into();
            }
        }
        if let Some(kind) = perry_runtime::typedarray::lookup_typed_array_kind(buf_addr) {
            let ta = ptr as *const perry_runtime::typedarray::TypedArrayHeader;
            let length = unsafe { (*ta).length } as usize;
            let elem_size = perry_runtime::typedarray::elem_size_for_kind(kind);
            let byte_len = length.saturating_mul(elem_size);
            let data_ptr = unsafe {
                (ptr as *const u8).add(std::mem::size_of::<
                    perry_runtime::typedarray::TypedArrayHeader,
                >())
            };
            // Build an ArrayBuffer owned by V8 and copy the perry bytes into it.
            // Using a copy (not a backing-store wrapper) keeps lifetimes simple:
            // perry's GC can reclaim the source without confusing V8.
            let ab = v8::ArrayBuffer::new(scope, byte_len);
            if byte_len > 0 {
                let bs = ab.get_backing_store();
                let dst = bs.data().map(|nn| nn.as_ptr() as *mut u8);
                if let Some(dst) = dst {
                    unsafe { std::ptr::copy_nonoverlapping(data_ptr, dst, byte_len) };
                }
            }
            // Element kind → V8 TypedArray constructor.
            use perry_runtime::typedarray as ta_mod;
            let ta_value: v8::Local<v8::Value> = match kind {
                ta_mod::KIND_INT8 => v8::Int8Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT8 | ta_mod::KIND_UINT8_CLAMPED => {
                    // V8 has Uint8ClampedArray as a separate type, but jose
                    // / jsonwebtoken only branch on `Uint8Array`. Use the
                    // plain Uint8Array unless we explicitly need clamped.
                    v8::Uint8Array::new(scope, ab, 0, length)
                        .map(|v| v.into())
                        .unwrap_or_else(|| v8::Array::new(scope, 0).into())
                }
                ta_mod::KIND_INT16 => v8::Int16Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT16 => v8::Uint16Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_INT32 => v8::Int32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_UINT32 => v8::Uint32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_FLOAT32 => v8::Float32Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                ta_mod::KIND_FLOAT64 => v8::Float64Array::new(scope, ab, 0, length)
                    .map(|v| v.into())
                    .unwrap_or_else(|| v8::Array::new(scope, 0).into()),
                _ => v8::Array::new(scope, 0).into(),
            };
            return ta_value;
        }
    }

    // Use GcHeader (8 bytes before user pointer) to reliably determine type.
    // All Perry arrays and objects are arena-allocated with GcHeader via arena_alloc_gc.
    let gc_header_ptr = (ptr as usize).wrapping_sub(perry_runtime::gc::GC_HEADER_SIZE);
    if gc_header_ptr > 0x1000 {
        let gc_header = unsafe { &*(gc_header_ptr as *const perry_runtime::gc::GcHeader) };
        let is_arena = (gc_header.gc_flags & perry_runtime::gc::GC_FLAG_ARENA) != 0;

        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_PROMISE {
            return native_promise_to_v8(scope, ptr as *mut perry_runtime::promise::Promise);
        }

        // Issue: Effect.pipe(map) chain — a Perry closure passed to V8 as
        // an arg (e.g. `Effect.map(fn)` where `fn` is a local arrow) lands
        // here with POINTER_TAG. Confirm the `CLOSURE_MAGIC` tag before
        // wrapping so we don't misidentify a generic native object as a
        // closure. The HIR-level `JsCreateCallback` rewrite handles inline
        // `Closure` literals; this is the LocalGet / FuncRef fallback
        // path.
        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_CLOSURE {
            const CLOSURE_TYPE_TAG_OFFSET: usize = 12;
            let type_tag = unsafe { *(ptr.add(CLOSURE_TYPE_TAG_OFFSET) as *const u32) };
            if type_tag == perry_runtime::closure::CLOSURE_MAGIC {
                if let Some(func_value) = native_closure_to_v8(scope, ptr) {
                    return func_value;
                }
            }
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_ARRAY {
            // GC-tracked array: ArrayHeader { length: u32, capacity: u32 } + f64 elements
            let header = ptr as *const perry_runtime::array::ArrayHeader;
            let length = unsafe { (*header).length };
            let elements_ptr = unsafe {
                ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
            };
            let v8_array = v8::Array::new(scope, length as i32);
            for i in 0..length {
                let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
                let v8_elem = native_to_v8(scope, elem_f64);
                v8_array.set_index(scope, i, v8_elem);
            }
            return v8_array.into();
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_OBJECT {
            // GC-tracked object: ObjectHeader (24 bytes) + field values
            let obj_header = ptr as *const perry_runtime::object::ObjectHeader;
            let field_count = unsafe { (*obj_header).field_count };
            let keys_array = unsafe { (*obj_header).keys_array };

            let v8_obj = v8::Object::new(scope);

            if !keys_array.is_null() && field_count > 0 {
                // Object has named keys - iterate and set each field
                let keys_length = unsafe { (*keys_array).length };
                let keys_elements_ptr = unsafe {
                    (keys_array as *const u8)
                        .add(std::mem::size_of::<perry_runtime::array::ArrayHeader>())
                        as *const f64
                };
                // Fields are stored as f64 (NaN-boxed JSValues) right after ObjectHeader
                let fields_ptr = unsafe {
                    ptr.add(std::mem::size_of::<perry_runtime::object::ObjectHeader>())
                        as *const f64
                };

                let count = std::cmp::min(field_count, keys_length);
                for i in 0..count {
                    // Get key string from keys_array. Keys may be heap strings or
                    // inline short strings, so route through the general V8 bridge.
                    let key_f64 = unsafe { *keys_elements_ptr.add(i as usize) };
                    let key_val = native_to_v8(scope, key_f64);
                    let v8_key = match key_val.to_string(scope) {
                        Some(k) => k,
                        None => continue,
                    };

                    // Get field value (NaN-boxed f64)
                    let field_f64 = unsafe { *fields_ptr.add(i as usize) };
                    let v8_val = native_to_v8(scope, field_f64);

                    v8_obj.set(scope, v8_key.into(), v8_val);
                }
            }

            return v8_obj.into();
        }
    }

    // Safety check: If the pointer looks like a StringHeader (length + capacity match,
    // and data after header is valid UTF-8), convert it as a string instead of an array.
    // This handles the case where a string pointer accidentally gets POINTER_TAG instead of STRING_TAG.
    {
        let str_header = ptr as *const perry_runtime::string::StringHeader;
        let str_len = unsafe { (*str_header).byte_len } as usize;
        let str_cap = unsafe { (*str_header).capacity } as usize;
        if str_len > 0 && str_len <= 100_000 && str_cap >= str_len && str_cap <= str_len + 64 {
            // Capacity is close to length — looks like a string, not an array
            // (Arrays typically have capacity much larger than needed due to growth)
            let data =
                unsafe { ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>()) };
            let bytes = unsafe { std::slice::from_raw_parts(data, str_len) };
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(v8_str) = v8::String::new(scope, s) {
                    return v8_str.into();
                }
            }
        }
    }

    // Fallback: heuristic array detection for non-arena allocations (Maps, etc.)
    let header = ptr as *const perry_runtime::array::ArrayHeader;
    let length = unsafe { (*header).length };
    let capacity = unsafe { (*header).capacity };
    if length <= 100_000 && capacity >= length && capacity <= 200_000 {
        let elements_ptr = unsafe {
            ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
        };
        let v8_array = v8::Array::new(scope, length as i32);
        for i in 0..length {
            let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
            let v8_elem = native_to_v8(scope, elem_f64);
            v8_array.set_index(scope, i, v8_elem);
        }
        return v8_array.into();
    }

    // Unknown type - wrap native pointer for opaque access
    let obj = v8::Object::new(scope);
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    obj.set(scope, key.into(), external.into());

    obj.into()
}
