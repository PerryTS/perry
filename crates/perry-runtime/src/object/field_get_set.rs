//! Indexed and named field get/set: the inline-cache hot path
//! (`js_object_get_field_by_name`, `js_object_get_field_ic_miss`,
//! `js_object_set_field_by_name`), plus keys/values/entries/has_property
//! and the polymorphic index accessors.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes.

use super::*;

/// Get a field from an object by index
#[no_mangle]
pub extern "C" fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JSValue {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return JSValue::undefined();
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return JSValue::undefined();
    }
    unsafe {
        // Bounds check: check inline fields first, then overflow map
        let fc = (*obj).field_count;
        if field_index >= fc {
            // Check overflow map for fields that didn't fit in inline storage
            return match overflow_get(obj as usize, field_index as usize) {
                Some(bits) => JSValue::from_bits(bits),
                None => JSValue::undefined(),
            };
        }
        // Guard: corrupted objects with unreasonably large field_count
        if fc > 10000 {
            return JSValue::undefined();
        }
        let fields_ptr =
            (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
        let val = *fields_ptr.add(field_index as usize);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        if val.bits() == 0x7FFD_0000_0000_0000 {
            eprintln!(
                "[NULL_PTR_FIELD_GET] obj={:p} field_index={} class_id={} field_count={}",
                obj,
                field_index,
                (*obj).class_id,
                (*obj).field_count
            );
            return JSValue::undefined();
        }
        val
    }
}

// Issue #922: Rate-limit and bound the [WARN_NULL_PTR] message stream
// + abort the process when a runaway loop is detected.
//
// Background: when codegen emits an `Expr::New { ... }` whose constructor
// args include a NULL POINTER_TAG (typically the result of a cross-module
// reference to an export that didn't link, or an async-step rejected-
// before-resolved capture), every constructor invocation calls
// `js_object_set_field` once per field. Each call previously emitted one
// `eprintln!` line. The gscmaster-api production loop (#922) printed
// 5.7M+ identical lines on a single Fastify route hit before PM2
// declared the process dead -- actionable signal drowned in noise.
//
// Hard limits + circuit breaker:
//   * The per-call [WARN_NULL_PTR] log line is gated behind PERRY_DEBUG=1
//     (issue #924) and ALSO rate-limited to `WARN_NULL_PTR_LOG_LIMIT`
//     (=64) per thread under PERRY_DEBUG so even debug runs don't drown
//     in noise. After the limit a one-time `...further entries suppressed`
//     notice fires.
//   * `WARN_NULL_PTR_ABORT_LIMIT` (=100_000) -- if the SAME obj+
//     field_index has been written with a null POINTER_TAG this many
//     times consecutively, eprintln a one-line diagnostic and trigger
//     `std::process::abort()`. This is UNCONDITIONAL (not gated by
//     PERRY_DEBUG) because a 100K-iteration same-site loop is real
//     corruption, not happy-path noise. The async-step reentry guard
//     at `crates/perry-runtime/src/promise.rs::ASYNC_STEP_REENTRY_BOUND`
//     bounds the loop at 10K iterations BEFORE this fires in the normal
//     case; this is the catch-all for paths the async-step guard misses
//     (e.g. sync `throw_not_callable` inside a non-async fastify hook).
const WARN_NULL_PTR_LOG_LIMIT: u64 = 64;
const WARN_NULL_PTR_ABORT_LIMIT: u64 = 100_000;

thread_local! {
    static WARN_NULL_PTR_STATE: std::cell::Cell<WarnNullPtrState>
        = const { std::cell::Cell::new(WarnNullPtrState {
            total_count: 0,
            last_obj: 0,
            last_field_index: u32::MAX,
            consecutive_same_site: 0,
        }) };
}

#[derive(Copy, Clone)]
struct WarnNullPtrState {
    total_count: u64,
    last_obj: usize,
    last_field_index: u32,
    consecutive_same_site: u64,
}

#[cold]
#[inline(never)]
fn record_warn_null_ptr(obj: *mut ObjectHeader, field_index: u32, class_id: u32) {
    let (total_count, should_abort) = WARN_NULL_PTR_STATE.with(|cell| {
        let mut s = cell.get();
        s.total_count = s.total_count.saturating_add(1);
        let same_site = s.last_obj == obj as usize && s.last_field_index == field_index;
        s.consecutive_same_site = if same_site {
            s.consecutive_same_site.saturating_add(1)
        } else {
            1
        };
        s.last_obj = obj as usize;
        s.last_field_index = field_index;
        let total = s.total_count;
        let abort = s.consecutive_same_site >= WARN_NULL_PTR_ABORT_LIMIT;
        cell.set(s);
        (total, abort)
    });
    // perry#924: the per-call log is gated behind PERRY_DEBUG=1. Even
    // under PERRY_DEBUG we cap at WARN_NULL_PTR_LOG_LIMIT occurrences
    // per thread (issue #922 -- the production loop produced 5.7M of
    // these and the actionable signal got buried).
    if total_count <= WARN_NULL_PTR_LOG_LIMIT && std::env::var_os("PERRY_DEBUG").is_some() {
        eprintln!(
            "[WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj={:p} field_index={} class_id={} -- replacing with undefined",
            obj, field_index, class_id
        );
        if total_count == WARN_NULL_PTR_LOG_LIMIT {
            eprintln!(
                "[WARN_NULL_PTR] further entries suppressed after {} occurrences -- this usually indicates an unresolved import or an uninitialized cross-module export being constructed into an object field",
                WARN_NULL_PTR_LOG_LIMIT
            );
        }
    }
    if should_abort {
        eprintln!(
            "[PERRY ABORT] js_object_set_field: detected runaway null POINTER_TAG writes at obj={:p} field_index={} class_id={} ({}+ consecutive same-site writes -- issue #922 circuit breaker). Common cause: an async function throws across an await boundary inside try/catch AND the catch arm re-enters the same await, OR an unresolved import was constructed into a field. Convert to a result-tag pattern (see issue #921 workaround) or check perry --print-hir for an uninitialized capture.",
            obj, field_index, class_id, WARN_NULL_PTR_ABORT_LIMIT
        );
        std::process::abort();
    }
}

/// Set a field on an object by index
#[no_mangle]
pub extern "C" fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JSValue) {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return;
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return;
    }
    unsafe {
        // Bounds check: guard against out-of-range field writes that corrupt adjacent
        // arena allocations. js_object_alloc_with_shape uses max(field_count, 8) physical
        // slots, but the stored field_count is the logical count. Class objects from
        // js_object_alloc_class_with_keys use exactly field_count slots.
        // We use a generous limit of max(field_count, 8) to avoid false positives from
        // js_object_alloc_with_shape's extra padding while still catching real overflows.
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate -- replace with undefined.
        // The diagnostic + circuit breaker live in `record_warn_null_ptr` (issue #922).
        // perry#924: the [WARN_NULL_PTR] log line itself is gated behind
        // `PERRY_DEBUG=1` inside `record_warn_null_ptr`; the circuit
        // breaker abort path is unconditional (it's a real corruption
        // signal, not happy-path noise).
        let vbits = value.bits();
        let value = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            record_warn_null_ptr(obj, field_index, (*obj).class_id);
            JSValue::undefined()
        } else {
            value
        };
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        let slot = fields_ptr.add(field_index as usize);
        ptr::write(slot, value);
        super::note_object_field_slot(obj, field_index as usize, value.bits());
        crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, value.bits());
    }
}

/// Get the class ID of an object.
///
/// Returns 0 unless `obj` is a real GC-arena-allocated class instance.
/// Issue #350 (round 2): the codegen's `idispatch` tower for unknown-receiver
/// method calls (e.g. `set.has(c)` when the static type is `ReadonlySet<T>`,
/// or `a.componentTypeSet.has(c)` where `a` is `Archetype | undefined`) uses
/// this function to compare the receiver's class id against every user
/// class implementing the same method name. Without the GC-type guard we
/// blindly read 4 bytes at offset 4 of the receiver — which for a
/// `SetHeader` (allocated via std::alloc, no GcHeader, layout
/// `{ size: u32, capacity: u32, elements: *mut f64 }`) is its `capacity`
/// field. `js_set_alloc(0)` defaults capacity to 4, which collides with
/// whichever user class lands at id 4, routing the call into the wrong
/// method body and crashing on the bogus `this` pointer.
#[no_mangle]
pub extern "C" fn js_object_get_class_id(obj: *const ObjectHeader) -> u32 {
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    let addr = obj as usize;
    // Built-in headers (Set / Map / Regex) live in their own per-type
    // registries — they're never user class instances. Reject them first
    // so we never try to read a GcHeader at obj-8, which doesn't exist
    // for these std::alloc'd headers.
    if crate::set::is_registered_set(addr)
        || crate::map::is_registered_map(addr)
        || crate::regex::is_regex_pointer(obj as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(obj as *const u8) {
            return 0;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
        (*obj).class_id
    }
}

/// Free an object (for manual memory management / testing)
#[no_mangle]
pub extern "C" fn js_object_free(_obj: *mut ObjectHeader) {
    // No-op: GC handles deallocation of arena-allocated objects
}

/// Convert an object pointer to a JSValue
#[no_mangle]
pub extern "C" fn js_object_to_value(obj: *const ObjectHeader) -> JSValue {
    JSValue::pointer(obj as *const u8)
}

/// Extract an object pointer from a JSValue
#[no_mangle]
pub extern "C" fn js_value_to_object(value: JSValue) -> *mut ObjectHeader {
    value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

/// Get a field as f64 (returns raw JSValue bits as f64)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_f64(obj: *const ObjectHeader, field_index: u32) -> f64 {
    let value = js_object_get_field(obj, field_index);
    f64::from_bits(value.bits())
}

/// Set a field from f64 (interprets raw bits as JSValue)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_set_field_f64(obj: *mut ObjectHeader, field_index: u32, value: f64) {
    // Check frozen flag — frozen objects reject all writes
    if !obj.is_null() && (obj as usize) > 0x10000 {
        unsafe {
            let gc =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return;
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Store a raw f64 into an object field slot for the unboxed numeric-field prototype.
///
/// This is only intended for construction sites whose static type has already
/// proven a raw-number slot. Dynamic writes still go through the normal setters,
/// which deopt the typed descriptor before tracing non-number values.
#[no_mangle]
pub extern "C" fn js_object_set_unboxed_f64_field(
    obj: *mut ObjectHeader,
    field_index: u32,
    value: f64,
) {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return;
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return;
    }
    unsafe {
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_unboxed_f64_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        let bits = value.to_bits();
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        let slot = fields_ptr.add(field_index as usize);
        ptr::write(slot, bits);
        super::note_object_field_slot(obj, field_index as usize, bits);
        crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, bits);
    }
}

/// Read a raw f64 object field slot used by the unboxed numeric-field prototype.
#[no_mangle]
pub extern "C" fn js_object_get_unboxed_f64_field(
    obj: *const ObjectHeader,
    field_index: u32,
) -> f64 {
    f64::from_bits(js_object_get_field(obj, field_index).bits())
}

/// Set a field by index with a raw f64 value (for dynamic object creation)
/// This is a convenience wrapper that takes field_index as u32 and value as f64.
/// Honors `Object.freeze` and per-key `writable: false` descriptors so codegen
/// paths that resolve property writes to a field index still respect the JS
/// invariants set up by `Object.defineProperty`.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_index(
    obj: *mut ObjectHeader,
    key: *const crate::string::StringHeader,
    field_index: u32,
    value: f64,
) {
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return;
    }
    unsafe {
        // Frozen objects reject all writes.
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        // Per-key writable / accessor check when the key string is provided.
        if !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                if ACCESSORS_IN_USE.with(|c| c.get()) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.set != 0 {
                            let closure = (acc.set & crate::value::POINTER_MASK)
                                as *const crate::closure::ClosureHeader;
                            if !closure.is_null() {
                                crate::closure::js_closure_call1(closure, value);
                            }
                        }
                        return;
                    }
                }
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.writable() {
                        return;
                    }
                }
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set the keys array for an object (used for Object.keys() support)
/// The keys_array should be an array of string pointers
#[no_mangle]
pub extern "C" fn js_object_set_keys(obj: *mut ObjectHeader, keys_array: *mut ArrayHeader) {
    unsafe {
        set_object_keys_array(obj, keys_array);
    }
}

/// Get the keys of an object as an array of strings.
/// If any key has a per-property descriptor with `enumerable: false`, that key is filtered out.
/// Otherwise (the common case), this returns the stored keys array directly.
#[no_mangle]
pub extern "C" fn js_object_keys(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893: defensive sibling of `js_object_entries`'s
        // is_valid_obj_ptr filter — `Object.keys(undefined)` /
        // `Object.keys(ansiStyles)` (cross-module import) previously
        // dereferenced a low-48-bit-of-undefined pointer (~0x1) and
        // segfaulted. Return empty array.
        return crate::array::js_array_alloc(0);
    }
    // Issue #323: arrays land here too (the codegen routes every `Object.keys`
    // call through this entry point, regardless of receiver type). Treating an
    // ArrayHeader as an ObjectHeader read garbage from the slot-0 element bits
    // — `obj_type=length`, `keys_array=elements[1]` — which happened to look
    // null when slots were zero-filled. After the issue #323 init-to-HOLE fix,
    // slot[1] reads as TAG_HOLE which is non-null and segfaulted downstream.
    // Detect arrays by GC type byte and emit string indices for non-HOLE slots.
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = stripped as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return crate::array::js_array_alloc(0);
                }
                let elements = (arr as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    // Format `i` as decimal into a stack buffer; SSO covers
                    // 0..=99999 (≤5 bytes), and a length-100k array hits the
                    // sanity-cap above so we never need a heap StringHeader.
                    let s = i.to_string();
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(result, key_box);
                }
                return result;
            }
        }
    }
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return crate::array::js_array_alloc(0);
        }
        // Per JS spec, `Object.keys` must return a fresh array — callers
        // can `.sort()`, `.push()`, etc. without mutating the receiver.
        // Pre-fix this fast path returned the object's own internal
        // `keys_array` pointer, so `Object.keys(o).sort()` reordered
        // `o`'s key→slot mapping and subsequent `o.foo` reads returned
        // the wrong slot's value. The slow path below already builds a
        // fresh array; the fast path now mirrors it, just without the
        // per-key descriptor check.
        let has_descriptors =
            PROPERTY_DESCRIPTORS.with(|m| m.borrow().keys().any(|(ptr, _)| *ptr == obj as usize));
        let len = crate::array::js_array_length(keys) as usize;
        if !has_descriptors {
            let out = crate::array::js_array_alloc(len as u32);
            for i in 0..len {
                let key_val = crate::array::js_array_get(keys, i as u32);
                crate::array::js_array_push_f64(out, f64::from_bits(key_val.bits()));
            }
            return out;
        }
        // Slow path: filter out non-enumerable keys.
        let filtered = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() {
                continue;
            }
            let stored_key = key_val.as_string_ptr();
            if stored_key.is_null() {
                continue;
            }
            let name_ptr =
                (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*stored_key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            let key_str = match std::str::from_utf8(name_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            // If a descriptor explicitly marks this key non-enumerable, skip it.
            if let Some(attrs) = get_property_attrs(obj as usize, key_str) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            crate::array::js_array_push_f64(filtered, f64::from_bits(key_val.bits()));
        }
        filtered
    }
}

/// Get the values of an object as an array
/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893: defensive sibling of `js_object_entries` —
        // see that function's comment for the rationale.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        // Iterate up to keys_len (logical property count), not
        // field_count — same fix as Object.entries above. Without
        // this, objects with overflow fields silently returned only
        // their first 8 values.
        let keys = (*obj).keys_array;
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        for i in 0..count {
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893 lineage: chalk's `Object.entries(ansiStyles)` passed a
        // value whose unboxed low-48 bits weren't a real heap pointer
        // (cross-module import where the default-export wrapper hasn't
        // finished initializing). Pre-fix the `(*obj).keys_array` deref
        // SIGSEGV'd at 0x14; now we return an empty array so the user's
        // `for (const [k, v] of Object.entries(undefined)) {}` no-ops the
        // way the spec's "abstract conversion to object" path would for
        // an unrecognized receiver. Real JS throws TypeError here; we
        // prefer the empty-array fallback because Perry doesn't have a
        // clean "throw at codegen-call boundaries" path for these
        // pointer-typed entry points and a segfault is strictly worse
        // for the caller.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        // Iterate up to keys_len (the logical property count), not
        // field_count. Parser-built and dict-built objects with ≥9
        // fields cap field_count at the inline alloc_limit (8) and
        // store overflow values in OVERFLOW_FIELDS — for those,
        // field_count under-counts the actual property count by N-8.
        // Without this fix, `Object.entries(obj)` on a 50-key dict
        // returned only the first 8 entries (silent data loss).
        // Mirrors the same fix in `js_object_keys` and the
        // `actual_fields = keys_len` line in `json.rs::stringify_object`.
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        for i in 0..count {
            // Create a pair array [key, value]
            let pair = crate::array::js_array_alloc(2);

            // Get the key (from keys array — already validated non-null
            // when count came from there).
            if !keys.is_null() && (i as u32) < crate::array::js_array_length(keys) {
                let key = crate::array::js_array_get_f64(keys, i as u32);
                crate::array::js_array_push_f64(pair, key);
            } else {
                crate::array::js_array_push_f64(pair, 0.0);
            }

            // Read the value. `js_object_get_field` handles the
            // inline-vs-overflow split internally (inline if
            // i < field_count, overflow_get otherwise).
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}

/// Check if a property exists in an object by its string key name
/// Returns NaN-boxed true if the property exists, NaN-boxed false otherwise
/// This implements the JavaScript 'in' operator: "key" in obj
#[no_mangle]
pub extern "C" fn js_object_has_property(obj: f64, key: f64) -> f64 {
    let nanbox_false = f64::from_bits(0x7FFC_0000_0000_0003u64); // TAG_FALSE
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64); // TAG_TRUE

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    // Refs #420 / #618: `Symbol in ClassRef` — drizzle's `entityKind in cls`.
    // Class refs are INT32-tagged. Check CLASS_STATIC_SYMBOLS for symbol
    // keys and CLASS_DYNAMIC_PROPS for string keys.
    {
        let bits = obj.to_bits();
        if (bits >> 48) == 0x7FFE {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            // Symbol key path.
            if let Some(_) = crate::symbol::class_static_symbol_lookup(class_id, key) {
                return nanbox_true;
            }
            // String key path: check CLASS_DYNAMIC_PROPS via the get-by-name fn.
            if !key_val.is_pointer() && key_val.is_string() {
                // is_string covers heap StringHeader. Route through the
                // CLASS_DYNAMIC_PROPS-aware get fn.
            }
            // Fallback: emit false for class refs that aren't in either table.
            return nanbox_false;
        }
    }

    if !obj_val.is_pointer() {
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    // Issue #323: array fast path. `n in arr` with a numeric key was always
    // returning false because the receiver was treated as ObjectHeader and
    // the key-is-string guard below rejected the numeric key. Detect an
    // ArrayHeader by GC type byte; for numeric keys check `index < length`
    // and slot != TAG_HOLE (distinguishes a hole from an explicit
    // `arr[i] = undefined` write, the latter overwrites HOLE with UNDEFINED).
    if (obj_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = obj_ptr as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return nanbox_false;
                }
                // Numeric key: extract the index. Accept both NaN-boxed i32
                // and plain f64 (e.g. literal `1`) provided it's a
                // non-negative integer in range.
                let idx: Option<u32> = if key_val.is_int32() {
                    let i = key_val.as_int32();
                    if i >= 0 {
                        Some(i as u32)
                    } else {
                        None
                    }
                } else if key_val.is_number() {
                    let f = f64::from_bits(key_val.bits());
                    if f >= 0.0 && f.fract() == 0.0 && f < u32::MAX as f64 {
                        Some(f as u32)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(idx) = idx {
                    if idx >= length {
                        return nanbox_false;
                    }
                    let elements = (arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const u64;
                    if std::ptr::read(elements.add(idx as usize)) == crate::value::TAG_HOLE {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
                // Non-numeric key on an array: only `length` and inherited
                // prototype methods would return true. Conservatively return
                // false for now — out of scope for #323.
                return nanbox_false;
            }
        }
    }

    if !key_val.is_string() {
        return nanbox_false;
    }

    let key_str = key_val.as_string_ptr();

    unsafe {
        let keys = (*obj_ptr).keys_array;
        if keys.is_null() {
            return nanbox_false;
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let stored_key_val = crate::array::js_array_get(keys, i as u32);
            if stored_key_val.is_string() {
                let stored_key = stored_key_val.as_string_ptr();
                if crate::string::js_string_equals(key_str, stored_key) != 0 {
                    // Check if the field was deleted (set to undefined by delete operator)
                    let field_val = js_object_get_field(obj_ptr, i as u32);
                    if field_val.is_undefined() {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
            }
        }

        nanbox_false
    }
}

/// Get a field by its string key name
/// Returns the field value or undefined if the key is not found
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    // Issue #818 (Effect class-instance pattern): a V8 handle (JS_HANDLE_TAG
    // = 0x7FFB) reaches here when codegen routes a generic `PropertyGet`
    // through this slow path — e.g. `Effect.succeed(42).value` where the
    // call return was a JS handle but the HIR `js_transform` pass didn't
    // rewrite the consumer-side `.value` into `JsGetProperty` (because the
    // call lowered as a `StaticMethodCall`, not as a `JsCallMethod`). The
    // method-call counterpart in `js_call_method` already routes
    // JS_HANDLE_TAG values to V8 via JS_HANDLE_CALL_METHOD; do the same
    // here via JS_HANDLE_OBJECT_GET_PROPERTY so subsequent property reads
    // on a returned class instance reach the live V8 object instead of
    // falling to the small-handle dispatch (which only knows about
    // Fastify/axios/sqlite, not generic V8 handles).
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFB && !key.is_null() {
            let func_ptr = crate::value::JS_HANDLE_OBJECT_GET_PROPERTY
                .load(std::sync::atomic::Ordering::SeqCst);
            if !func_ptr.is_null() {
                let func: unsafe extern "C" fn(f64, *const i8, usize) -> f64 =
                    unsafe { std::mem::transmute(func_ptr) };
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let result = func(f64::from_bits(bits), key_ptr as *const i8, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            return JSValue::undefined();
        }
    }
    // Issue #618-followup: read INT32-tagged class ref's dynamic property
    // from the side-table (mirror of the set-side intercept). For drizzle's
    // `SQL.Aliased` lookup pattern.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("");
                // v0.5.752: class_ref.constructor synthesizes back to the
                // same class ref so drizzle's
                // `Object.getPrototypeOf(value).constructor === Class` chain
                // collapses correctly (with v0.5.751's getPrototypeOf
                // returning the class ref for instance receivers). Refs
                // #420 / #618 followup.
                if name == "constructor" && class_id != 0 && is_class_id_registered(class_id) {
                    return JSValue::from_bits(bits);
                }
                if name == "prototype" && class_id != 0 && is_class_id_registered(class_id) {
                    return JSValue::from_bits(bits);
                }
                if class_id != 0 && class_has_own_method(class_id, name) {
                    let value = class_prototype_method_value_for_name(class_id, name);
                    return JSValue::from_bits(value.to_bits());
                }
                if !name.is_empty() {
                    let result = CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow()
                            .get(&class_id)
                            .and_then(|props| props.get(name).copied())
                    });
                    if let Some(v) = result {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // SSO property access (v0.5.213 Step 1 gate). The codegen inline
    // `.length` path routes SHORT_STRING_TAG receivers here because
    // it doesn't yet know about the SSO tag. Handle `.length` by
    // reading the length byte directly from the NaN-box payload.
    // Other property accesses on an SSO string (e.g. `.charAt` via
    // `[0]`, `.slice`) aren't yet routed here — handled by the
    // string method dispatch in a future migration step; today they
    // fall through to "undefined" which matches the behavior for
    // string-valued property access on untyped locals in general.
    {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            if !key.is_null() {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if key_bytes == b"length" {
                        let len = (obj_bits & crate::value::SHORT_STRING_LEN_MASK)
                            >> crate::value::SHORT_STRING_LEN_SHIFT;
                        return JSValue::number(len as f64);
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                // undefined/null tag or null pointer — return undefined
                return JSValue::undefined();
            }
            // Issue #340: small-handle receivers (raw < 0x100000) come
            // from native modules (axios, fastify, ioredis, ...) that
            // store objects in registries and expose integer ids. The
            // handle property dispatcher (registered by stdlib via
            // `js_register_handle_property_dispatch`) routes the
            // property name to the per-module accessor (e.g. axios
            // status/data, fastify req query/params/...). Without
            // this, every property access on those handles silently
            // returned undefined.
            if (raw as usize) > 0 && (raw as usize) < 0x100000 {
                if !key.is_null() {
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if is_timer_handle_method_key(key_bytes)
                            && crate::timer::is_known_timer_id(raw as i64)
                        {
                            let this_f64 = f64::from_bits(
                                crate::value::js_nanbox_pointer(raw as i64).to_bits(),
                            );
                            let result = super::js_class_method_bind(this_f64, key_ptr, key_len);
                            return JSValue::from_bits(result.to_bits());
                        }
                    }
                    // Drizzle-sqlite blocker: synth `data.constructor` for
                    // small-handle native instances so drizzle's
                    // `isConfig(data)` duck-type via
                    // `data.constructor.name !== "Object"` doesn't crash on
                    // `(undefined).name` under #648's strict catch-all.
                    // Returning the existing NULL_OBJECT_BYTES stub (a real
                    // ObjectHeader-shape with no fields) makes `(stub).name`
                    // return undefined safely, and `undefined !== "Object"`
                    // makes isConfig return false at the first gate. Refs
                    // #645 deeper followup.
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if key_bytes == b"constructor" {
                            let null_obj_ptr =
                                &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                            return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                        }
                    }
                    let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
                    if let Some(dispatch) = dispatch {
                        unsafe {
                            let key_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let key_len = (*key).byte_len as usize;
                            let bits = dispatch(raw as i64, key_ptr, key_len);
                            return JSValue::from_bits(bits.to_bits());
                        }
                    }
                }
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() {
        return JSValue::undefined();
    }
    // Same handle-receiver path for already-stripped pointers — happens
    // when the codegen passes a raw i64 handle through the slow path.
    if (obj as usize) < 0x100000 {
        if !key.is_null() {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if is_timer_handle_method_key(key_bytes)
                    && crate::timer::is_known_timer_id(obj as i64)
                {
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = super::js_class_method_bind(this_f64, key_ptr, key_len);
                    return JSValue::from_bits(result.to_bits());
                }
            }
            let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
            if let Some(dispatch) = dispatch {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    return JSValue::from_bits(bits.to_bits());
                }
            }
        }
        return JSValue::undefined();
    }
    if (obj as usize) < 0x1000000 {
        return JSValue::undefined();
    }
    unsafe {
        // Buffers: BufferHeader is allocated via raw `alloc()` (no GcHeader)
        // and tracked in BUFFER_REGISTRY. Detect first so the GC header check
        // below doesn't read garbage one word before the BufferHeader.
        // Route `.length` to `js_buffer_length` (matches the codegen path that
        // routes through PropertyGet for chained `Buffer.from(...).length`
        // expressions where the static type isn't recognized as Buffer).
        if crate::buffer::is_registered_buffer(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" || key_bytes == b"byteLength" {
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
                if key_bytes == b"buffer" {
                    return JSValue::from_bits(
                        crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                    );
                }
                // Issue #639 followup: method-as-value reads on a Buffer
                // (e.g. duck-type tests like `typeof v.readUInt8 === "function"`
                // in @perryts/mysql's `isBufferLike`) need to return a
                // bound-method closure so `typeof` reports `"function"` and
                // a subsequent call routes through `js_native_call_method`'s
                // existing `dispatch_buffer_method` arm. Pre-fix every
                // non-length read returned undefined, so duck tests failed
                // and the encoder fell through to its `String(buf)` fallback —
                // BLOB params got encoded as VAR_STRING and the INSERT
                // silently corrupted the binary column.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if is_buffer_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        // Buffers are stored as raw f64-bitcast pointers
                        // (NOT NaN-boxed) per CLAUDE.md "Module-level
                        // variables" — but `js_native_call_method`'s
                        // buffer arm at line ~5031 strips both raw and
                        // NaN-boxed payloads via `(bits >> 48) >= 0x7FF8`,
                        // so wrapping in POINTER_TAG here is equally
                        // valid and matches `js_class_method_bind`.
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
        // Sets: SetHeader is allocated via raw `alloc()` (no GcHeader),
        // so we can't safely read the byte preceding the pointer to
        // determine its type. Detect via the SET_REGISTRY first and
        // route `.size` to `js_set_size`. Other property accesses on a
        // Set return undefined (matching Node behavior — Sets only have
        // a `size` getter property).
        if crate::set::is_registered_set(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let s = obj as *const crate::set::SetHeader;
                    return JSValue::number(crate::set::js_set_size(s) as f64);
                }
            }
            return JSValue::undefined();
        }
        // Symbols: registered in SYMBOL_POINTERS by symbol.rs. Symbols
        // allocated via Symbol.for(...) are Box-leaked (no GcHeader), so
        // reading the byte before would be UB. Detect via the side table.
        if crate::symbol::is_registered_symbol(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let sym_f64 =
                    f64::from_bits(0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF));
                if key_bytes == b"description" {
                    return JSValue::from_bits(
                        crate::symbol::js_symbol_description(sym_f64).to_bits(),
                    );
                }
            }
            return JSValue::undefined();
        }
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return JSValue::undefined();
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if !is_valid_obj_ptr(obj as *const u8) {
            return JSValue::undefined();
        }
        let gc_type = (*gc_header).obj_type;
        // Issue #618: closures have their own GC type (GC_TYPE_CLOSURE=4)
        // distinct from GC_TYPE_OBJECT, but support dynamic-property storage
        // via the `CLOSURE_DYNAMIC_PROPS` side-table. `js_object_set_field_by_name`
        // routes writes there for the IIFE-namespace pattern
        // (`((sql2) => { sql2.identifier = ...; })(sql)`); mirror the read
        // path here so the companion get fires. Pre-fix the
        // `gc_type != GC_TYPE_OBJECT` arm below would early-return undefined
        // for any closure receiver, masking the dynamic-prop side-table.
        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                // `fn.length` — return the registered declared-param
                // count for the underlying function. Ramda's
                // `converge` / `useWith` / `addIndex` chain feeds
                // `pluck('length', fns)` through
                // `reduce(max, 0, …)` → `curryN(N, …)` → `_arity(N, …)`;
                // without a real number here that pipeline produces
                // `NaN`, and `_arity` throws
                // `First argument to _arity must be a non-negative
                // integer no greater than ten` at module init.
                if name_bytes == b"length" {
                    let arity =
                        crate::closure::closure_arity(obj as *const crate::closure::ClosureHeader);
                    return JSValue::number(arity.unwrap_or(0) as f64);
                }
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                    return JSValue::from_bits(val.to_bits());
                }
            }
            return JSValue::undefined();
        }
        // Error objects: route the common instance properties (message,
        // name, stack, cause) through the dedicated error accessors.
        // `js_object_get_field_by_name_f64` is the codegen's default
        // property dispatch for caught exceptions, so this is the only
        // sensible place to wire Error access.
        if gc_type == crate::gc::GC_TYPE_ERROR {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let err_ptr = obj as *mut crate::error::ErrorHeader;
                match key_bytes {
                    b"message" => {
                        let s = crate::error::js_error_get_message(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"name" => {
                        let s = crate::error::js_error_get_name(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"stack" => {
                        let s = crate::error::js_error_get_stack(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"cause" => {
                        let v = crate::error::js_error_get_cause(err_ptr);
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"errors" => {
                        // AggregateError.errors — return the errors array
                        // NaN-boxed with POINTER_TAG so callers can index
                        // into it. (The LLVM backend also has a direct
                        // `js_error_get_errors` fast path in expr.rs but
                        // this covers dynamic dispatch on caught errors.)
                        let errs = crate::error::js_error_get_errors(err_ptr);
                        if errs.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(crate::js_nanbox_pointer(errs as i64).to_bits());
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        // Arrays: handle `.length` so dynamic property access on a
        // typed-Any local returned from `JSON.parse("[1,2,3]")` picks
        // up the real length instead of falling through to object
        // field lookup and returning undefined. The array-length
        // inline fast path in codegen fires only when the type is
        // statically known, so this branch catches the dynamic case.
        if gc_type == crate::gc::GC_TYPE_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                // date-fns / drizzle / lodash duck-typing path:
                // `arr.constructor === Array`, `new arr.constructor(...)`,
                // etc. expect a non-undefined function-typed value that
                // refers back to the global `Array` constructor. Resolve
                // through the singleton so this returns the same closure
                // pointer as the bare `Array` identifier.
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
            }
            return JSValue::undefined();
        }
        // Issue #179 Phase 2: lazy array dispatch. `.length` returns
        // cached_length without materializing; any other property
        // access force-materializes (via the call into the generic
        // array path, which goes through `clean_arr_ptr` and hits
        // the lazy branch there).
        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
                if key_bytes == b"constructor" {
                    let v = js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
                    return JSValue::from_bits(v.to_bits());
                }
            }
            // Any other property access force-materializes, then
            // re-enters via the materialized ArrayHeader pointer.
            let materialized = crate::json_tape::force_materialize_lazy(
                obj as *mut crate::json_tape::LazyArrayHeader,
            );
            return js_object_get_field_by_name(materialized as *const ObjectHeader, key);
        }
        // Strings: handle `.length` so `(x as string).length` on an
        // unknown-typed local (TypeScript `as` casts are erased in
        // HIR) produces the real codepoint length.
        if gc_type == crate::gc::GC_TYPE_STRING {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let s = obj as *const crate::StringHeader;
                    return JSValue::number((*s).byte_len as f64);
                }
            }
            return JSValue::undefined();
        }
        // Maps: handle `.size` for `obj.m.size` style access where m is
        // a Map field stored in a plain object literal. Without this
        // the dynamic property dispatch returns undefined.
        if gc_type == crate::gc::GC_TYPE_MAP {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let m = obj as *const crate::map::MapHeader;
                    return JSValue::number(crate::map::js_map_size(m) as f64);
                }
            }
            return JSValue::undefined();
        }
        // RegExp: RegExpHeader is allocated via GC_TYPE_OBJECT but tracked
        // in REGEX_POINTERS. Detect and route `.source`, `.flags`,
        // `.lastIndex`, `.global`, `.ignoreCase`, `.multiline`, `.sticky`,
        // `.unicode`, `.dotAll` to the regex header fields. Must run
        // before the generic object-field path so the keys_array lookup
        // doesn't try to read the regex header bytes as ObjectHeader.
        if gc_type == crate::gc::GC_TYPE_OBJECT && crate::regex::is_regex_pointer(obj as *const u8)
        {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let re = obj as *const crate::regex::RegExpHeader;
                match key_bytes {
                    b"source" => {
                        let s = crate::regex::js_regexp_get_source(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"flags" => {
                        let s = crate::regex::js_regexp_get_flags(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"lastIndex" => {
                        return JSValue::number((*re).last_index as f64);
                    }
                    b"global" => {
                        return JSValue::bool((*re).global);
                    }
                    b"ignoreCase" => {
                        return JSValue::bool((*re).case_insensitive);
                    }
                    b"multiline" => {
                        return JSValue::bool((*re).multiline);
                    }
                    b"sticky" | b"unicode" | b"dotAll" | b"hasIndices" => {
                        return JSValue::bool(false);
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return JSValue::undefined();
            }
        }

        // Issue #649: native-module sub-namespace property access.
        // `fs.constants.F_OK` lowers to `PropertyGet { PropertyGet { fs,
        // "constants" }, "F_OK" }` — the inner expression's runtime value
        // is a NATIVE_MODULE_CLASS_ID-tagged ObjectHeader produced by
        // `js_create_native_module_namespace`; the outer PropertyGet then
        // arrives here with the sub-namespace as receiver. Pre-fix the
        // lookup fell through to the field-bag scan (which only stores
        // `__module__`) and returned undefined. Now we route through
        // `get_native_module_constant` directly.
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let nb_ptr = crate::value::js_nanbox_pointer(obj as i64);
            let module_name = get_module_name_from_namespace(nb_ptr);
            if !module_name.is_empty() {
                let property_name =
                    std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
                if let Some(val) = get_native_module_constant(module_name, property_name, nb_ptr) {
                    return JSValue::from_bits(val.to_bits());
                }
                // Issue #894: parity with the direct-NativeModuleRef
                // fast path (`js_native_module_property_by_name`). For
                // (module, prop) pairs whose property-read should
                // produce a callable handle — e.g.
                // `("events", "EventEmitter")` — synthesize the same
                // BOUND_METHOD_FUNC_PTR closure so the require-then-
                // member-access shape (`const { EventEmitter } =
                // require("node:events")`) matches the direct
                // namespace-import shape (`import { EventEmitter } from
                // "node:events"`). Pre-fix the slow path returned
                // undefined here, and the downstream
                // `EventEmitter.prototype` read tripped the spec
                // "Cannot read properties of undefined" throw.
                if is_native_module_callable_export(module_name, property_name) {
                    let prop_bytes = property_name.as_bytes();
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(prop_bytes.len().max(1), 1)
                                .unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(prop_bytes.as_ptr(), ptr, prop_bytes.len());
                        ptr
                    };
                    let closure =
                        crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
                    crate::closure::js_closure_set_capture_f64(closure, 0, nb_ptr);
                    crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
                    crate::closure::js_closure_set_capture_ptr(closure, 2, prop_bytes.len() as i64);
                    super::set_bound_native_closure_name(closure, property_name);
                    return JSValue::from_bits(
                        crate::value::js_nanbox_pointer(closure as i64).to_bits(),
                    );
                }
                return JSValue::undefined();
            }
        }

        // Refs #420 / #618 followup: `instance.constructor` returns the
        // class ref. Pre-fix this fell through to the keys_array lookup
        // which never finds "constructor" (the class itself isn't stored
        // as a field on the instance), and the chain returned undefined.
        // Drizzle's `is(value, type)` walks `value.constructor[entityKind]`
        // which depends on this. Spec: every instance's `__proto__.constructor`
        // points back to the class function. We materialize that lookup
        // by reading the ObjectHeader's class_id and returning the
        // INT32-tagged class ref if registered. Unregistered class_id
        // (e.g. `class C {}` with no methods) still returns undefined
        // here; pure object literals have class_id=0 and also return
        // undefined (matches Node behavior — bare object literals don't
        // get a custom constructor; their .constructor would be Object).
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                let class_id = (*obj).class_id;
                // Object-literal instances (`{ x: 1 }`) carry a synthetic
                // `__AnonShape_*` class id. Spec says their `.constructor`
                // is the global `Object`, not the synthetic class — so
                // resolve through the globalThis singleton so the value
                // matches the bare `Object` identifier (`x.constructor
                // === Object`, date-fns `constructFrom`, drizzle's
                // `isPlainObject` duck check).
                if class_id != 0 && is_anon_shape_class_id(class_id) {
                    let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
                    return JSValue::from_bits(v.to_bits());
                }
                if class_id != 0 && is_class_id_registered(class_id) {
                    let bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64);
                    return JSValue::from_bits(bits);
                }
                // class_id == 0 fallback: plain ObjectHeader allocated
                // without an HIR shape (Object.create(null) hybrids, raw
                // empty `{}` produced by JSON.parse, etc.). Report
                // `Object` so duck-type tests don't trip undefined.
                if class_id == 0 {
                    let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
                    return JSValue::from_bits(v.to_bits());
                }
            }
        }

        let keys = (*obj).keys_array;

        if keys.is_null() {
            // #809: an object with no own keys (e.g. an `Object.create(proto)`
            // result, or a `Function.prototype = obj` instance) still has to
            // resolve inherited props/methods. Pre-fix this returned undefined
            // here — BEFORE the `class_id` prototype-walk below — so
            // `Object.create(P).m()` threw `TypeError: m is not a function`.
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some(v) = resolve_proto_chain_field(class_id, key) {
                    return v;
                }
                // Issue #838 followup (b): same keyless-receiver gap for
                // JS-classic prototype methods. An instance allocated via
                // `js_new_function_construct` (no constructor-body write
                // yet, or a constructor that runs the closures' own
                // capture writes but never `this.<own field> = …`)
                // starts with `keys_array == null`. Without this arm
                // dayjs's `(new _(cfg)).format` returned undefined
                // because the keyless branch skipped the regular
                // `CLASS_PROTOTYPE_METHODS` walk reached further down
                // — see the matching arm at line ~4083.
                let key_bytes = std::slice::from_raw_parts(
                    (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                    (*key).byte_len as usize,
                );
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(v) = lookup_prototype_method(class_id, name) {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }

        // Validate keys_array is a real heap pointer (upper 16 bits must be 0 for ARM64/x86-64 user space).
        // If the object is actually a non-Object type (closure, array, map, etc.), keys_array at offset
        // 16 may contain garbage. An invalid upper 16-bit value catches this case defensively.
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return JSValue::undefined();
        }

        // Issue #62 phase B: the previous "ASCII-like pointer value" heuristic
        // assumed macOS mmap always returns arena pointers with `top_byte < 0x20`.
        // That stopped holding once strings started arena-allocating (more blocks,
        // mimalloc mapping into higher ranges): valid 0x000_04355_a033_* pointers
        // triggered false positives, the heuristic returned `undefined`, and tests
        // like `Object.defineProperty` flapped. The GcHeader `obj_type ==
        // GC_TYPE_ARRAY` check immediately below is a real content-level validation
        // (can't be faked by an address in any range) and fully supersedes this
        // address-sniffing heuristic.

        // Cross-platform safety: validate keys_array has a valid GcHeader.
        // If the keys_array pointer is corrupt (e.g., due to a stale reference after GC,
        // or a func_addr relocation issue on x86_64), the GcHeader check catches it
        // before we dereference the array contents.
        {
            let keys_gc =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_gc_type = (*keys_gc).obj_type;
            // keys_array must be GC_TYPE_ARRAY (arena-allocated array)
            if keys_gc_type != crate::gc::GC_TYPE_ARRAY {
                return JSValue::undefined();
            }
        }

        // Fast path: check field index cache (keys_array_ptr + key_hash → field_index)
        // Objects with the same shape share the same keys_array, so we cache per-shape lookups.
        let key_bytes = std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        );
        let key_hash = {
            let mut h: u32 = 0x811c9dc5;
            for &b in key_bytes {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h
        };
        let keys_id = keys as usize;

        // Thread-local inline cache: fixed-size direct-mapped cache (no allocation, no HashMap)
        // Each entry stores (keys_ptr, key_hash, field_index) for collision-safe validation
        const FIELD_CACHE_SIZE: usize = 1024;
        thread_local! {
            static FIELD_CACHE: std::cell::UnsafeCell<[(usize, u32, u32); FIELD_CACHE_SIZE]> =
                const { std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]) };
        }
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % FIELD_CACHE_SIZE;
        let cached = FIELD_CACHE.with(|c| {
            let cache = &*c.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash {
                Some(entry.2)
            } else {
                None
            }
        });
        if let Some(field_idx) = cached {
            // Accessor short-circuit: if this (obj, key) has a getter installed,
            // invoke it instead of reading the slot. The `ACCESSORS_IN_USE`
            // thread-local gate keeps this off the hot path in the common case.
            if ACCESSORS_IN_USE.with(|c| c.get()) {
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.get != 0 {
                            let closure = (acc.get & crate::value::POINTER_MASK)
                                as *const crate::closure::ClosureHeader;
                            if !closure.is_null() {
                                let result_f64 = crate::closure::js_closure_call0(closure);
                                return JSValue::from_bits(result_f64.to_bits());
                            }
                        }
                        // Has accessor but no getter → undefined.
                        return JSValue::undefined();
                    }
                }
            }
            return js_object_get_field(obj, field_idx);
        }

        // Slow path: linear scan through keys array
        let key_count = crate::array::js_array_length(keys) as usize;
        let _field_count = (*obj).field_count as usize;

        if key_count > 65536 {
            return JSValue::undefined();
        }

        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Cache this lookup for next time
                    FIELD_CACHE.with(|c| {
                        let cache = &mut *c.get();
                        cache[cache_idx] = (keys_id, key_hash, i as u32);
                    });
                    // Accessor short-circuit (see fast path above).
                    if ACCESSORS_IN_USE.with(|c| c.get()) {
                        if let Ok(name) = std::str::from_utf8(key_bytes) {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                                if acc.get != 0 {
                                    let closure = (acc.get & crate::value::POINTER_MASK)
                                        as *const crate::closure::ClosureHeader;
                                    if !closure.is_null() {
                                        let result_f64 = crate::closure::js_closure_call0(closure);
                                        return JSValue::from_bits(result_f64.to_bits());
                                    }
                                }
                                return JSValue::undefined();
                            }
                        }
                    }
                    if i < alloc_limit {
                        return js_object_get_field(obj, i as u32);
                    } else {
                        return match overflow_get(obj as usize, i) {
                            Some(bits) => JSValue::from_bits(bits),
                            None => JSValue::undefined(),
                        };
                    }
                }
            }
        }

        // Key not found in the keys_array — fall back to the class
        // vtable's getter map. Refs #486 (hono): cross-module class
        // getters (e.g. hono Context's `get req()` defined in
        // `hono/dist/context.js` and read from a user `c.req.url`
        // expression in main.ts) reach this point because the field
        // dispatcher only looks for stored fields, not getter accessors.
        // The getter is registered in `CLASS_VTABLE_REGISTRY` via
        // `js_register_class_getter` at module init by codegen — invoke
        // it with the same NaN-boxed `this` the codegen passes for
        // method dispatch.
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    // Walk the class -> parent chain so a getter declared
                    // on a base class is also found when the receiver is
                    // a subclass instance. `get_parent_class_id` reads
                    // CLASS_REGISTRY (populated by `js_register_class_parent`).
                    let mut cid = class_id;
                    let mut depth = 0usize;
                    while depth < 32 {
                        if let Some(vtable) = reg.get(&cid) {
                            if let Ok(name) = std::str::from_utf8(key_bytes) {
                                if let Some(&getter_ptr) = vtable.getters.get(name) {
                                    // Getters take `this` as f64 (NaN-boxed
                                    // POINTER_TAG), matching the codegen
                                    // calling convention for class methods.
                                    let this_f64: f64 = f64::from_bits(
                                        crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                    );
                                    let f: extern "C" fn(f64) -> f64 =
                                        std::mem::transmute(getter_ptr);
                                    return JSValue::from_bits(f(this_f64).to_bits());
                                }
                            }
                        }
                        match get_parent_class_id(cid) {
                            Some(p) if p != 0 && p != cid => {
                                cid = p;
                                depth += 1;
                            }
                            _ => break,
                        }
                    }
                }
            }

            // Issue #711 part 2: walk the class chain for a registered
            // prototype object (from `Function.prototype = X`). When
            // found, the method is an own-property of the proto
            // object — return its value directly. `pipe`, `[Equal.symbol]`,
            // etc. on Effect's EffectPrototype reach here.
            {
                if let Some(v) = resolve_proto_chain_field(class_id, key) {
                    return v;
                }
            }

            // Issue #838: JS-classic `Class.prototype.method = fn`
            // assignment registered via `js_register_prototype_method`.
            // Read returns the stored closure value directly, mirroring
            // Node's `Object.getPrototypeOf(inst).method` lookup. The
            // bound-method-closure fallback below handles vtable methods;
            // this arm covers methods that only exist as prototype
            // assignments (never declared inside the `class` block).
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if let Some(v) = lookup_prototype_method(class_id, name) {
                    return JSValue::from_bits(v.to_bits());
                }
            }

            // v0.5.756: method-as-value fallback. If `obj.method` reads via
            // the runtime path (Any-typed receiver, so the codegen #446
            // arm at expr.rs:3596 didn't fire), look up the method in the
            // class vtable chain and return a bound-method closure
            // (BOUND_METHOD_FUNC_PTR sentinel + (this, name_ptr, name_len)
            // captures). This makes both `typeof obj.method === "function"`
            // and `obj.method(args)` work for class methods on Any-typed
            // receivers — the closure-call dispatch routes through
            // `js_native_call_method` which walks the same vtable chain.
            // Refs #446 / drizzle's `(ins as any)._prepare()` chain.
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if lookup_class_method_in_chain(class_id, name).is_some() {
                    // Allocate a fresh i8 buffer for the method name owned
                    // by the closure. The keys_array's StringHeader bytes
                    // could in theory be GC'd if the keys_array is not
                    // pinned for the closure's lifetime.
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
                }
            }
        }

        // Key not found
        JSValue::undefined()
    }
}

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    // date-fns `constructFrom`: the parameter is statically Any so the
    // codegen Date intercept doesn't fire here. Detect Date instances
    // by their registered f64 bit pattern and route `.constructor` to
    // the global Date constructor closure — same value the bare `Date`
    // identifier produces, so `date instanceof Date` followed by `new
    // date.constructor(value)` lands on the right factory inside
    // `js_new_function_construct`.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if crate::date::is_registered_date_bits(obj_bits) {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"constructor" {
                    return js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

fn is_timer_handle_method_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"ref"
            | b"unref"
            | b"hasRef"
            | b"refresh"
            | b"close"
            | b"__perry_dispose__"
            | b"@@__perry_wk_toPrimitive"
    )
}

/// Monomorphic inline cache miss handler (issue #51).
///
/// Called when the codegen-emitted shape check (`obj->keys_array == cache[0]`)
/// fails. Performs the full field lookup via `js_object_get_field_by_name`,
/// then populates the per-site cache so subsequent calls with the same shape
/// hit the inline fast path (no function call, direct field load).
///
/// `cache` layout: `[keys_array_ptr: i64, field_slot_index: i64]`
///
/// Only caches when:
/// - obj is a valid ObjectHeader (not null, not handle, not string/array/etc.)
/// - field exists and its slot index < 8 (inline allocation limit)
///
/// Overflow fields (slot >= alloc_limit) are NOT cached and fall through to
/// the slow path — the fast path loads from `obj_ptr + 24 + slot*8` which
/// would read past the inline allocation.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache: *mut [i64; 2],
) -> f64 {
    // SSO receiver — never cacheable. Route through the SSO-aware
    // `js_object_get_field_by_name` which handles `.length` inline
    // and returns undefined for other keys.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            let v = js_object_get_field_by_name(obj, key);
            return f64::from_bits(v.bits());
        }
    }
    if obj.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Issue #340: small-handle receivers (axios, fastify, ioredis,
    // ...) are passed here from the codegen IC miss path with the
    // lower-48 of the NaN-box stripped — `obj as usize` is the
    // raw handle id (1, 2, 3, ...). Route to HANDLE_PROPERTY_DISPATCH
    // (registered by stdlib via js_register_handle_property_dispatch)
    // so `r.status` / `r.data` and similar handle-property accesses
    // dispatch to the per-module accessor instead of silently
    // returning undefined.
    if (obj as usize) > 0 && (obj as usize) < 0x100000 {
        // Drizzle-sqlite blocker: synth `data.constructor` for small-handle
        // receivers — IC-miss path mirror of the constructor intercept in
        // `js_object_get_field_by_name`. Refs #645 deeper followup.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }
        let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
        if let Some(dispatch) = dispatch {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                return dispatch(obj as i64, key_ptr, key_len);
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if (obj as usize) < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // When accessors are active anywhere in the program, skip the cache
    // entirely: the PIC fast path does a direct field load that bypasses
    // getter dispatch, so any object that uses defineProperty / get / set
    // would silently return the raw slot value instead of calling the
    // getter. The slow path through js_object_get_field_by_name handles
    // accessors correctly.
    let can_cache = !ACCESSORS_IN_USE.with(|c| c.get());
    unsafe {
        // Issue #72: validate this really is a GC_TYPE_OBJECT before reading
        // (*obj).keys_array — otherwise an Array/String/Buffer/etc. receiver
        // (whose `object_type` byte at offset 0 happens to be 1, matching
        // OBJECT_TYPE_REGULAR for a length-1 array) would be treated as
        // cacheable and seed the per-site PIC with garbage from element[1].
        // The codegen guard funnels non-OBJECT receivers here too, so this
        // belt-and-braces check keeps the cache from being primed with
        // values that would survive into the inline hot path.
        let is_object = (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 && {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
        };
        let keys = (*obj).keys_array;
        let is_regular = is_object && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
        if can_cache && is_regular && !keys.is_null() && (keys as usize) > 0x10000 {
            let key_count = *(keys as *const u32) as usize;
            let keys_data = (keys as *const u8).add(8) as *const f64;
            let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
            for i in 0..key_count {
                let k_bits = (*keys_data.add(i)).to_bits();
                let k_ptr = (k_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
                if !k_ptr.is_null() && crate::string::js_string_equals(k_ptr, key) != 0 {
                    if i >= alloc_limit {
                        // Field is in the overflow map — fall through to the
                        // slow path which handles overflow correctly.
                        break;
                    }
                    // The codegen IC fast path computes `obj + 24 + slot*8`
                    // and does a direct load. Any inline slot (`i <
                    // alloc_limit`) is reachable via that path, so cache
                    // every inline slot — including the ones at index >= 8
                    // for classes whose `field_count` exceeds the
                    // MIN_FIELD_SLOTS=8 baseline (e.g. World.commandBuffer
                    // sits at slot 12). Pre-fix this branch capped the cache
                    // at `i < 8` which left every >8-slot field permanently
                    // missing the cache: every access fell through to a
                    // fresh keys_array walk + js_string_equals chain. On
                    // perf-comprehensive's hot loops that path was hit
                    // ~900k times per run (40% inclusive samples per
                    // perfcomp.profile).
                    (*cache)[0] = keys as i64;
                    (*cache)[1] = i as i64;
                    let field_ptr = (obj as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + i * 8)
                        as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// Polymorphic numeric-key get: companion of `js_object_set_index_polymorphic`.
/// Reads `obj[idx]` where `idx` is a number and the receiver type isn't
/// statically narrowed. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` (and forwarded / lazy variants) → `js_array_get_f64`,
///   which routes through `clean_arr_ptr` for forwarding-chain follow.
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`            → stringify `idx` and
///   delegate to `js_object_get_field_by_name_f64`. JS treats `obj[0]` as
///   `obj["0"]`, so the stringification matches spec semantics.
///
/// Closes #471 (read side): paired with the IndexSet polymorphic fix so
/// `Record<number, T>` stores and reads through the same path. Without
/// this, `constMap[i] = v; constMap[i]` would set via the object setter
/// but read from `obj+8+i*8` (stale ObjectHeader fields), returning
/// garbage f64 values.
#[no_mangle]
pub extern "C" fn js_object_get_index_polymorphic(obj_handle: i64, idx: f64) -> f64 {
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative numeric keys → string keys on the object path.
        let s = idx_i32.to_string();
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
        return f64::from_bits(v.bits());
    }

    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        return crate::array::js_array_get_f64(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
        );
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        let s = if idx == (idx_i32 as f64) {
            idx_i32.to_string()
        } else {
            format!("{}", idx)
        };
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
        return f64::from_bits(v.bits());
    }
    // Buffer / Map / Set / typed-array / unknown — try the array getter
    // (which handles registered buffers + typed arrays via per-kind reads).
    crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, idx_i32 as u32)
}

/// Polymorphic numeric-key set: `obj[idx] = value` where `idx` is a number
/// and the receiver type isn't statically known. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` / buffer / typed-array → `js_array_set_f64_extend`,
///   which preserves the array fast-path (forwarding chain follow + grow).
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`   → stringify `idx` and delegate
///   to `js_object_set_field_by_name`. JS treats `obj[0] = v` as `obj["0"] = v`,
///   so the stringification matches spec semantics.
///
/// Closes #471: codegen's previous IndexSet numeric-key fallback emitted
/// an inline `obj+8+idx*8` store. That layout assumes an `ArrayHeader`
/// (8-byte header) but `ObjectHeader` is 24 bytes followed by `max(field_count, 8)`
/// inline slots, so any `idMap[i] = v` on an object with i ≥ 7 wrote past
/// the object's allocation, corrupting whatever heap memory followed.
/// In the @perryts/mongodb repro, that memory happened to be doc[0]'s
/// `keys_array` pointer — Object.keys returned a stale string pointer
/// the BSON encoder read as an empty array, emitting empty BSON docs
/// over the wire.
///
/// Receiver layout other than array/object (e.g. raw pointer below the heap
/// or a small handle) silently no-ops, matching the existing tolerant-on-
/// bad-args contract of `js_array_set_f64` / `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn js_object_set_index_polymorphic(obj_handle: i64, idx: f64, value: f64) {
    // Strip NaN-box tags defensively. Codegen calls this with the lower-48
    // bits already extracted via `unbox_to_i64`, but match the convention
    // of every other entry-point so a stray un-stripped caller (or a JIT
    // that forgets the mask) still works.
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return;
    }
    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative indices on objects coerce to e.g. "-1" string keys; on
        // arrays, JS spec gates them to no-ops. Stringify and delegate so
        // the object case (rare but possible) still routes correctly.
        let s = idx_i32.to_string();
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
        return;
    }

    // Read GC type byte (offset 0 of GcHeader, which lives at obj-8).
    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return;
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_ARRAY {
        // Includes lazy/forwarded — js_array_set_f64_extend's clean_arr_ptr_mut
        // walks the forwarding chain and routes buffers/typed-arrays through
        // their per-kind setter.
        crate::array::js_array_set_f64_extend(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
            value,
        );
        return;
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        // Stringify the index and route through the object field setter,
        // which handles shape transitions, frozen/sealed/extensible checks,
        // overflow into out-of-line storage, and accessor descriptors.
        let s = if idx == (idx_i32 as f64) {
            // Common integer case — avoid the Display path's allocator hit
            // and just format an i32 directly.
            idx_i32.to_string()
        } else {
            format!("{}", idx)
        };
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
        return;
    }
    // Buffer / Map / Set / other GC types — fall through to the array
    // setter, which has its own per-kind dispatch (registered buffer →
    // byte write, registered typed-array → typed setter). Anything not
    // recognized is a no-op via clean_arr_ptr_mut returning null.
    crate::array::js_array_set_f64_extend(
        raw as *mut crate::array::ArrayHeader,
        idx_i32 as u32,
        value,
    );
}

/// Issue #615 helper — read a `*const StringHeader` as a Rust `String`
/// for inclusion in TypeError diagnostic messages. Returns `"<unknown>"`
/// for null / non-UTF-8 / corrupt headers so the throw still fires
/// rather than panicking on the slow-path edge case.
unsafe fn key_to_str_for_diag(key: *const crate::StringHeader) -> String {
    if key.is_null() {
        return "<unknown>".to_string();
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if name_len == 0 {
        return String::new();
    }
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string())
}

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[allow(unused_assignments)]
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    // Issue #618-followup: detect INT32-tagged class ref (top16 == 0x7FFE).
    // Drizzle's `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)` pattern sets
    // a static property on an imported class — Perry stores classes as
    // INT32-tagged class ids, so the receiver here is e.g. 0x7FFE_0000_0000_002A
    // not a real ObjectHeader. Route to the CLASS_DYNAMIC_PROPS side-table
    // so a later `SQL.Aliased` read can find it.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow_mut()
                            .entry(class_id)
                            .or_insert_with(std::collections::HashMap::new)
                            .insert(name, value);
                    });
                }
            }
            return;
        }
    }
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                return;
            }
            if (raw as usize) < 0x10000 {
                // Small handle — dispatch to handle property set if registered
                unsafe {
                    if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                        if !key.is_null() {
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            dispatch(raw as i64, name_ptr, name_len, value);
                        }
                    }
                }
                return;
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        // Small non-null value — could be a stripped handle (after ensure_i64 stripped NaN-box tag)
        if !obj.is_null() && (obj as usize) > 0 {
            unsafe {
                if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                    if !key.is_null() {
                        let name_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).byte_len as usize;
                        dispatch(obj as i64, name_ptr, name_len, value);
                    }
                }
            }
        }
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    let mut key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    let mut value = value_handle.get_nanbox_f64();
    // Safety: obj is a valid heap pointer (> 0x10000) at this point
    unsafe {
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT && gc_type != crate::gc::GC_TYPE_CLOSURE {
            if !is_valid_obj_ptr(obj as *const u8) {
                return;
            }
            // Not a heap object/closure — only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return;
            }
        }

        // Check if this is a ClosureHeader — closures support dynamic props via separate storage.
        // ClosureHeader has CLOSURE_MAGIC (0x434C4F53) at offset 12.
        // Without this check, (*obj).keys_array reads capture[0] → corruption/crash.
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    crate::closure::closure_set_dynamic_prop(obj as usize, name_str, value);
                }
            }
            return;
        }

        // Refs #486 (hono): class setter dispatch. JS spec: a `set X(...)`
        // accessor on the prototype intercepts `obj.X = value` writes
        // before they hit the instance's data slots. Hono's `set res(_res)
        // { …; this.#res = _res; this.finalized = true; }` is the canonical
        // example — without setter dispatch, `c.res = response` from inside
        // compose stored the response into a regular field slot but never
        // ran the body, so `this.finalized = true` never executed and
        // hono-base's `if (!context.finalized) throw` fired on every
        // request. Walk the class -> parent chain mirroring the getter
        // dispatch in `js_object_get_field_by_name`.
        if !key.is_null() && (key as usize) > 0x10000 {
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        let key_bytes = {
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            std::slice::from_raw_parts(name_ptr, name_len)
                        };
                        let mut cid = class_id;
                        let mut depth = 0usize;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cid) {
                                if let Ok(name) = std::str::from_utf8(key_bytes) {
                                    if let Some(&setter_ptr) = vtable.setters.get(name) {
                                        // Setters take `(this_f64, value_f64)`
                                        // matching the codegen calling
                                        // convention for class methods (this
                                        // = NaN-boxed POINTER_TAG of the
                                        // receiver).
                                        let this_f64: f64 = f64::from_bits(
                                            crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                        );
                                        let f: extern "C" fn(f64, f64) -> f64 =
                                            std::mem::transmute(setter_ptr);
                                        let _ = f(this_f64, value);
                                        return;
                                    }
                                }
                            }
                            match get_parent_class_id(cid) {
                                Some(p) if p != 0 && p != cid => {
                                    cid = p;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }

        // Check Object.freeze/seal/preventExtensions flags
        let obj_flags = (*gc_header)._reserved;
        let is_frozen = obj_flags & crate::gc::OBJ_FLAG_FROZEN != 0;
        let is_sealed_or_no_extend =
            obj_flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;

        let keys = (*obj).keys_array;

        // Validate keys_array is a real heap pointer or null.
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                return;
            }
        }

        let mut prev_keys_usize = keys as usize;

        // Resolve to interned pointer for transition cache (pointer identity).
        // If the key is already interned (GC_FLAG_INTERNED set — e.g. from
        // js_string_concat intern hit), skip the FNV-1a hash entirely.
        let mut interned_key = if !key.is_null() && (key as usize) > 0x10000 {
            let gc_hdr =
                (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_hdr).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
                key // already interned
            } else {
                let kh = key_content_hash(key);
                crate::string::js_string_intern(key, kh)
            }
        } else {
            key
        };
        let interned_key_handle = scope.root_string_ptr(interned_key);
        interned_key = interned_key_handle.get_raw_const_ptr::<crate::StringHeader>();
        macro_rules! refresh_roots_after_alloc {
            () => {{
                obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
                key = key_handle.get_raw_const_ptr::<crate::StringHeader>();
                value = value_handle.get_nanbox_f64();
                interned_key = interned_key_handle.get_raw_const_ptr::<crate::StringHeader>();
            }};
        }

        // FAST PATH: shape-transition cache with interned string pointer identity.
        if !key.is_null()
            && !is_frozen
            && !is_sealed_or_no_extend
            && !GLOBAL_DESCRIPTORS_IN_USE.load(Ordering::Relaxed)
        {
            if let Some((next_keys, slot_idx)) =
                transition_cache_lookup(prev_keys_usize, interned_key)
            {
                // Defensive: strip a raw-null POINTER_TAG value the same
                // way the slow overflow path below does, so a bogus
                // 0x7FFD_0000_0000_0000 store doesn't leak into an
                // overflow map.
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                set_object_keys_array(obj, next_keys as *mut ArrayHeader);
                super::mark_object_dynamic_shape_unknown(obj);
                let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
                if (slot_idx as usize) < alloc_limit {
                    // Inline the field write — `obj` has already been
                    // validated (GC header read, type check, closure
                    // check) by the prelude above, and `vbits` has had
                    // the null-POINTER-TAG replacement applied. No
                    // point re-doing it in `js_object_set_field`.
                    let fields_ptr =
                        (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                    let slot = fields_ptr.add(slot_idx as usize);
                    ptr::write(slot, JSValue::from_bits(vbits));
                    super::note_object_field_slot(obj, slot_idx as usize, vbits);
                    crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, vbits);
                    // Bump field_count only for inline slots — leaving
                    // it at the physical capacity is what steers
                    // `js_object_get_field_by_name`'s reads to the
                    // overflow map for slots ≥ alloc_limit. Bumping it
                    // past capacity would make reads dereference past
                    // the object's inline field array into adjacent
                    // arena data.
                    if slot_idx >= (*obj).field_count {
                        (*obj).field_count = slot_idx + 1;
                    }
                } else {
                    // Cached slot is past the object's inline capacity —
                    // store in the overflow map (same as the slow path's
                    // `new_index >= alloc_limit` branch).
                    overflow_set(obj as usize, slot_idx as usize, vbits);
                    // Deliberately do NOT bump field_count here — see
                    // above.
                }
                return;
            }
        }

        // If no keys array exists, create one (adding new key)
        if keys.is_null() {
            // Frozen or sealed/non-extensible objects reject new keys.
            // Issue #615 — strict-mode throw instead of silent return.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Create a new keys array with the key
            let new_keys = crate::array::js_array_alloc(4);
            refresh_roots_after_alloc!();
            let new_keys =
                crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);

            // Reallocate fields to hold at least one value
            // Note: We assume the object has enough field slots pre-allocated
            js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
            // Bump field_count so Object.keys()/values()/entries() see the new property.
            if (*obj).field_count == 0 {
                (*obj).field_count = 1;
            }
            // Record the null→single-key transition so the next object
            // that starts with `{}` and sets the same first key hits the
            // fast path above instead of allocating a fresh 4-elem
            // keys_array here.
            transition_cache_insert(0, interned_key, new_keys as usize, 0);
            return;
        }

        // Defer the Rust-String allocation for the incoming key: we only
        // need it if an accessor descriptor or per-property writable
        // attribute has been installed on this object. Both paths are
        // guarded by process-wide flags (`ACCESSORS_IN_USE` and
        // `PROPERTY_ATTRS_IN_USE`) so the common case — plain data
        // properties on a normal object — avoids the `.to_string()`
        // entirely. A 20-property row object written at 10k rows saw
        // 200k of those allocations per query; with this guard the
        // count drops to zero unless userland actually defined a
        // descriptor.
        let needs_descriptor_key =
            ACCESSORS_IN_USE.with(|c| c.get()) || PROPERTY_ATTRS_IN_USE.with(|c| c.get());
        let incoming_key_str: Option<String> = if needs_descriptor_key && !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        } else {
            None
        };

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        // Sidecar O(1) lookup when keys_array has grown past the
        // linear-scan break-even. Without this, the build-then-fill
        // pattern (`for i in 0..N { obj["k_"+i] = i; }`) is O(N²)
        // because every insert does a linear scan that grows by one
        // each iteration. With the sidecar, the per-insert cost is
        // O(1) amortized (rebuild after a `js_array_push` realloc is
        // bounded by the doubling growth pattern).
        if !key.is_null() && (key as usize) > 0x10000 && key_count >= KEYS_INDEX_THRESHOLD as usize
        {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            let key_hash = key_bytes_hash(name_ptr, name_len);
            if let Some(i) = keys_index_lookup(obj, keys, name_bytes, key_hash) {
                let i = i as usize;
                if is_frozen {
                    let key_str = key_to_str_for_diag(key);
                    crate::error::throw_immutable_write(0, &key_str);
                }
                if i < alloc_limit {
                    js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                } else {
                    let vbits = value.to_bits();
                    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                        crate::value::TAG_UNDEFINED
                    } else {
                        vbits
                    };
                    overflow_set(obj as usize, i, vbits);
                }
                return;
            }
            // Miss path: the linear scan below will confirm and then
            // append. We skip the scan entirely and just append the
            // key (the sidecar would have found it if it existed).
            // Same effect as scanning all N entries with no match.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Skip the linear-scan loop by jumping past it via a
            // labeled-block break. The append code that follows the
            // scan is shared.
            // We achieve this by setting a marker, then the linear
            // scan checks it and skips.
            let keys_gc_header =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
                && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
            {
                (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
            } else {
                true
            };
            let owned_keys = if keys_shared {
                let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
                refresh_roots_after_alloc!();
                let keys = (*obj).keys_array;
                prev_keys_usize = keys as usize;
                let src_data = (keys as *const u8).add(8) as *const f64;
                let dst_data = (cloned as *mut u8).add(8) as *mut f64;
                for i in 0..key_count {
                    *dst_data.add(i) = *src_data.add(i);
                }
                (*cloned).length = key_count as u32;
                super::rebuild_array_layout_from_slots(cloned);
                set_object_keys_array(obj, cloned);
                cloned
            } else {
                keys
            };
            let new_index = key_count;
            if new_index >= alloc_limit {
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
                let new_keys =
                    crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
                prev_keys_usize = if keys_shared {
                    prev_keys_usize
                } else {
                    owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
                };
                refresh_roots_after_alloc!();
                set_object_keys_array(obj, new_keys);
                super::mark_object_dynamic_shape_unknown(obj);
                overflow_set(obj as usize, new_index, vbits);
                transition_cache_insert(
                    prev_keys_usize,
                    interned_key,
                    new_keys as usize,
                    new_index as u32,
                );
                keys_index_insert(
                    obj as usize,
                    (new_index + 1) as u32,
                    key_hash,
                    new_index as u32,
                );
                return;
            }
            let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            prev_keys_usize = if keys_shared {
                prev_keys_usize
            } else {
                owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
            };
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);
            js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
            if new_index as u32 >= (*obj).field_count {
                (*obj).field_count = new_index as u32 + 1;
            }
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
            keys_index_insert(
                new_keys as usize,
                (new_index + 1) as u32,
                key_hash,
                new_index as u32,
            );
            return;
        }

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - update the field. Frozen objects must
                    // throw a TypeError on writes to existing keys
                    // (issue #615 — strict-mode behavior, default for TS).
                    if is_frozen {
                        let key_str = key_to_str_for_diag(key);
                        crate::error::throw_immutable_write(0, &key_str);
                    }
                    // Accessor short-circuit: if a setter is registered, invoke
                    // it instead of writing the slot. A property with `get` but
                    // no `set` silently ignores the write (non-strict mode).
                    if ACCESSORS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, k) {
                                if acc.set != 0 {
                                    let closure = (acc.set & crate::value::POINTER_MASK)
                                        as *const crate::closure::ClosureHeader;
                                    if !closure.is_null() {
                                        crate::closure::js_closure_call1(closure, value);
                                    }
                                }
                                return;
                            }
                        }
                    }
                    // Per-property writable check (set by Object.defineProperty / freeze).
                    // Issue #615 — strict-mode throw on read-only assign.
                    if PROPERTY_ATTRS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(attrs) = get_property_attrs(obj as usize, k) {
                                if !attrs.writable() {
                                    crate::error::throw_immutable_write(0, k);
                                }
                            }
                        }
                    }
                    if i < alloc_limit {
                        js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                    } else {
                        // This key was previously stored in the overflow map — update it there
                        let vbits = value.to_bits();
                        let vbits =
                            if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                                crate::value::TAG_UNDEFINED
                            } else {
                                vbits
                            };
                        overflow_set(obj as usize, i, vbits);
                    }
                    return;
                }
            }
        }

        // Key not found - add it to the object.
        // Frozen/sealed/non-extensible objects reject new keys.
        // Issue #615 — strict-mode throw.
        if is_frozen || is_sealed_or_no_extend {
            let key_str = key_to_str_for_diag(key);
            crate::error::throw_immutable_write(1, &key_str);
        }
        // CRITICAL: The keys_array may be SHARED via SHAPE_CACHE (multiple objects with
        // the same shape hash share the same keys array). We must clone it before mutating
        // to avoid corrupting other objects' keys.
        //
        // We detect sharing via the `GC_FLAG_SHAPE_SHARED` bit that
        // `shape_cache_insert` stamps onto the array's GC header —
        // arrays allocated in the `keys.is_null()` branch above are
        // exclusively owned and don't have the flag, so we skip the
        // clone entirely. This saves ~19 clones of growing size per
        // 20-property plain-object literal.
        //
        // Validate the GC header before reading it. `keys_array` has
        // already been range-checked for user address space but may
        // still point at something other than a GC-allocated array
        // in rare cases (static data, buffers re-interpreted as keys
        // arrays). If the header doesn't identify as GC_TYPE_ARRAY,
        // assume shared and clone (the previous, always-safe behaviour).
        let keys_gc_header =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
            && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        {
            (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
        } else {
            // Unknown provenance — take the safe side.
            true
        };
        let owned_keys = if keys_shared {
            let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
            refresh_roots_after_alloc!();
            let keys = (*obj).keys_array;
            prev_keys_usize = keys as usize;
            let src_data = (keys as *const u8).add(8) as *const f64;
            let dst_data = (cloned as *mut u8).add(8) as *mut f64;
            for i in 0..key_count {
                *dst_data.add(i) = *src_data.add(i);
            }
            (*cloned).length = key_count as u32;
            super::rebuild_array_layout_from_slots(cloned);
            set_object_keys_array(obj, cloned);
            cloned
        } else {
            keys
        };

        // Check if we have a spare physical slot (js_object_alloc_with_shape allocates max(N,8) slots).
        // Class objects (js_object_alloc_class_with_keys) have only exactly field_count slots;
        // attempting to write to new_index = key_count would overflow into the next heap allocation.
        let new_index = key_count;
        if new_index >= alloc_limit {
            // No inline room — store in the overflow HashMap so the value is not lost.
            // Also add the key to keys_array so Object.keys() sees it.
            let vbits = value.to_bits();
            let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                eprintln!("[WARN_NULL_PTR] overflow new store: null POINTER_TAG at obj={:p} new_index={} — replacing with undefined", obj, new_index);
                crate::value::TAG_UNDEFINED
            } else {
                vbits
            };
            let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            prev_keys_usize = if keys_shared {
                prev_keys_usize
            } else {
                owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
            };
            refresh_roots_after_alloc!();
            set_object_keys_array(obj, new_keys);
            super::mark_object_dynamic_shape_unknown(obj);
            overflow_set(obj as usize, new_index, vbits);
            // Record the shape transition so the next object sharing
            // `prev_keys` that adds the same key hits the fast path.
            // The cached target is stamped `GC_FLAG_SHAPE_SHARED` by
            // `transition_cache_insert`, which triggers clone-on-extend
            // on either object if someone later appends past this key.
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
            return;
        }
        // First, add the key to the keys array (may reallocate)
        let owned_keys_handle = scope.root_raw_mut_ptr(owned_keys);
        let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
        prev_keys_usize = if keys_shared {
            prev_keys_usize
        } else {
            owned_keys_handle.get_raw_mut_ptr::<ArrayHeader>() as usize
        };
        refresh_roots_after_alloc!();
        // Update the object's keys_array pointer in case js_array_push reallocated
        set_object_keys_array(obj, new_keys);
        super::mark_object_dynamic_shape_unknown(obj);

        // Set the field at the new index and update logical field_count
        js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
        // Bump field_count to reflect the newly added property
        if new_index as u32 >= (*obj).field_count {
            (*obj).field_count = new_index as u32 + 1;
        }
        // Record the shape transition — see above for semantics.
        transition_cache_insert(
            prev_keys_usize,
            interned_key,
            new_keys as usize,
            new_index as u32,
        );
    }
}
