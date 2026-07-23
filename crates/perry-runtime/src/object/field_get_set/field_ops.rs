//! Field set/get FFI entry points + WARN_NULL_PTR circuit breaker.
//! Pure relocation out of field_get_set.rs (issue #1103 split).

use super::*;

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
    if obj.is_null() || (obj as usize) < 0x10000 {
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
        let alloc_limit =
            std::cmp::max(stored_field_count, crate::object::INLINE_SLOT_FLOOR as u32);
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
        crate::gc::runtime_store_jsvalue_slot(
            obj as usize,
            slot as usize,
            field_index as usize,
            value.bits(),
        );
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
    if crate::value::addr_class::is_handle_band(obj as usize) {
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
    if obj.is_null() || (obj as usize) < 0x10000 {
        return;
    }
    unsafe {
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        let stored_field_count = (*obj).field_count;
        let alloc_limit =
            std::cmp::max(stored_field_count, crate::object::INLINE_SLOT_FLOOR as u32);
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
        crate::gc::runtime_store_jsvalue_slot(
            obj as usize,
            slot as usize,
            field_index as usize,
            bits,
        );
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
    if obj.is_null() || (obj as usize) < 0x10000 {
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
                // Gate on the per-object descriptor flag: `ACCESSOR_DESCRIPTORS`
                // is keyed by raw address, so a fresh object reusing a freed
                // address must not pick up the previous tenant's stale accessor
                // (it would silently drop `obj.k = v` for a getter-only stale
                // entry). A fresh allocation has the flag clear.
                if crate::state::state().descriptors.accessors_in_use.get()
                    && super::super::object_has_descriptors(obj as usize)
                {
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
