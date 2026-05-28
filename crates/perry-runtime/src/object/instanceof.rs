//! `instanceof` evaluation: `js_instanceof` and the dynamic
//! (runtime-class-ref) form `js_instanceof_dynamic`.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation.

use super::*;

/// v0.5.749: dynamic instanceof — `value instanceof type` where the
/// type is a runtime value (function arg holding a class ref). Extracts
/// the class_id from the INT32 NaN-tag (top16=0x7FFE) and dispatches to
/// `js_instanceof`. Returns FALSE for non-class-ref type values (matches
/// JS spec: `1 instanceof 2` throws, but Perry returns false defensively).
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_instanceof_dynamic(value: f64, type_ref: f64) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bits = type_ref.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 {
            return js_instanceof(value, class_id);
        }
    }
    // #1789: `x instanceof C` where C is a heap class object (the value a
    // class EXPRESSION evaluates to, e.g. `const C = make(x); c instanceof
    // C`). Read its class_id (the compile-time template) and walk the
    // candidate's class chain against it.
    if is_class_object_value(type_ref) {
        let obj = crate::JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
        let class_id = js_object_get_class_id(obj);
        if class_id != 0 {
            return js_instanceof(value, class_id);
        }
    }
    if let Some((module, method)) = unsafe { bound_native_callable_module_and_method(type_ref) } {
        if module == "events"
            && method == "EventEmitter"
            && (crate::node_stream::is_classic_stream_instance_value(value)
                || is_stream_event_emitter_prototype_value(value))
        {
            return f64::from_bits(crate::value::TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Check if a value is an instance of a class with the given class_id
/// Walks the inheritance chain to check parent classes
/// Returns NaN-boxed TAG_TRUE / TAG_FALSE so the result identifies as a boolean.
#[no_mangle]
pub extern "C" fn js_instanceof(value: f64, class_id: u32) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let true_val = f64::from_bits(TAG_TRUE);
    let false_val = f64::from_bits(TAG_FALSE);

    // User-defined `Symbol.hasInstance` takes precedence over the built-in
    // prototype-chain walk. The HIR lifts `static [Symbol.hasInstance](v)`
    // to a top-level function `__perry_wk_hasinstance_<class>` and the
    // LLVM backend registers a pointer to it against the class's id at
    // module init. If a hook is present, call it with the candidate value
    // and return the boolean-shaped result directly.
    if let Some(func_ptr) = lookup_has_instance_hook(class_id) {
        let hook: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(func_ptr as *const u8) };
        let result = hook(value);
        // Normalize: any truthy NaN-boxed bool stays as the TAG_TRUE/FALSE
        // sentinel. User-written `return typeof v === "number" && ...`
        // already returns a NaN-boxed bool, so this is usually a no-op.
        let rbits = result.to_bits();
        if rbits == TAG_TRUE || rbits == TAG_FALSE {
            return result;
        }
        // Fallback: treat as truthy → TRUE, zero/undefined → FALSE.
        if result.is_nan() && rbits & 0xFFFF_0000_0000_0000 == 0x7FFC_0000_0000_0000 {
            return false_val;
        }
        if result == 0.0 || result.is_nan() {
            return false_val;
        }
        return true_val;
    }

    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    // Special handling for Uint8Array/Buffer (class_id 0xFFFF0004)
    // Perry buffers are raw BufferHeader pointers bitcast to f64 (not NaN-boxed),
    // so the normal POINTER_TAG check doesn't work for them.
    // We use a thread-local buffer registry to identify buffer pointers.
    if class_id == crate::buffer::BUFFER_TYPE_ID {
        // Check if NaN-boxed pointer
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::buffer::is_registered_buffer(addr) {
                return true_val;
            }
        }
        // Check if raw pointer (buffer values are bitcast, not NaN-boxed)
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 && crate::buffer::is_registered_buffer(bits as usize) {
            return true_val;
        }
        return false_val;
    }

    // ArrayBuffer — Perry models ArrayBuffer storage with BufferHeader values
    // marked in a side registry. They can arrive either NaN-boxed or as raw
    // buffer pointers, matching the Buffer/Uint8Array path above.
    const CLASS_ID_ARRAY_BUFFER: u32 = 0xFFFF0025;
    if class_id == CLASS_ID_ARRAY_BUFFER {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0
            && crate::buffer::is_registered_buffer(addr)
            && crate::buffer::is_array_buffer(addr)
        {
            return true_val;
        }
        return false_val;
    }

    // #1545: Web Streams `instanceof ReadableStream` / `instanceof
    // WritableStream`. Stream handles are numeric `id as f64`, so consult the
    // stdlib kind-probe (1 = readable, 2 = writable) rather than the class
    // chain. Covers `ts.readable instanceof ReadableStream`,
    // `rs.pipeThrough(ts) instanceof ReadableStream`, etc.
    const CLASS_ID_READABLE_STREAM: u32 = 0xFFFF0060;
    const CLASS_ID_WRITABLE_STREAM: u32 = 0xFFFF0061;
    if class_id == CLASS_ID_READABLE_STREAM || class_id == CLASS_ID_WRITABLE_STREAM {
        if value.is_finite() && value > 0.0 && value.fract() == 0.0 {
            if let Some(probe) = crate::object::stream_handle_kind_probe() {
                let kind = unsafe { probe(value as usize) };
                let want = if class_id == CLASS_ID_READABLE_STREAM {
                    1
                } else {
                    2
                };
                if kind == want {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Built-in JS types Map / Set / RegExp / Date — Perry doesn't define
    // user classes for these, so we use reserved class IDs and detect via
    // the per-type registries (MAP_REGISTRY / SET_REGISTRY / REGEX_POINTERS)
    // or, for Date, by checking that the value is a finite f64 timestamp.
    const CLASS_ID_DATE: u32 = 0xFFFF0020;
    const CLASS_ID_REGEXP: u32 = 0xFFFF0021;
    const CLASS_ID_MAP: u32 = 0xFFFF0022;
    const CLASS_ID_SET: u32 = 0xFFFF0023;
    if class_id == CLASS_ID_DATE {
        // A Perry Date is a raw f64 timestamp (no NaN-box tag, real f64).
        // Distinguishing it from a regular number requires a side-channel:
        // `js_date_new(...)` registers the f64 bits in DATE_REGISTRY, and
        // here we consult that registry. Without the registry, every finite
        // number would match (the prior "approximate" rule), which made
        // `100 instanceof Date` true and broke the BSON encoder's typed
        // dispatch (`if (value instanceof Date) … else if (typeof v === 'number') …`).
        //
        // The Invalid-Date sentinel is itself a NaN, so it must be matched
        // *before* the `!is_nan()` guard — `new Date(NaN) instanceof Date`
        // is `true` per ECMA-262 even though its time value is NaN.
        if value.to_bits() == crate::date::DATE_NAN_BITS
            || (!value.is_nan()
                && value.is_finite()
                && crate::date::is_registered_date_bits(value.to_bits()))
        {
            return true_val;
        }
        return false_val;
    }
    if class_id == CLASS_ID_MAP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::map::is_registered_map(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_SET {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::set::is_registered_set(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_REGEXP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::regex::is_regex_pointer(addr as *const u8) {
                return true_val;
            }
        }
        return false_val;
    }

    // `Object` — ECMAScript spec: `x instanceof Object` is true for any
    // non-primitive (every object/array/function/Map/Set/Buffer/RegExp/
    // Date/typed-array/Promise/etc.). The codegen maps `Object` to this
    // reserved id (#585 follow-up: pre-#585 fix this case worked by
    // accident because the codegen produced `class_id = 0` and the
    // runtime returned true via `0 == 0` on the obj_class_id check).
    const CLASS_ID_OBJECT: u32 = 0xFFFF0050;
    if class_id == CLASS_ID_OBJECT {
        if jsval.is_pointer() {
            return true_val;
        }
        // Invalid Date is still an Object (NaN time value, but a Date).
        if value.to_bits() == crate::date::DATE_NAN_BITS
            || (!value.is_nan()
                && value.is_finite()
                && crate::date::is_registered_date_bits(value.to_bits()))
        {
            return true_val;
        }
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            let addr = bits as usize;
            if crate::buffer::is_registered_buffer(addr)
                || crate::set::is_registered_set(addr)
                || crate::map::is_registered_map(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            {
                return true_val;
            }
        }
        return false_val;
    }

    // Array — Perry arrays are heap allocations with `GC_TYPE_ARRAY` in
    // their gc_header (one byte at obj-8). Pointer can arrive NaN-boxed
    // (POINTER_TAG) or as a raw bitcast f64; handle both. Lazy arrays
    // (Phase 5 JSON.parse result) are also arrays from the user's
    // perspective — must return true without force-materializing.
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == CLASS_ID_ARRAY {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 && addr >= crate::gc::GC_HEADER_SIZE {
            let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            unsafe {
                let obj_type = (*gc_header).obj_type;
                if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Typed arrays — Int8Array..Float64Array reserved IDs (0xFFFF0030..37).
    // The pointer can arrive as either a NaN-boxed POINTER_TAG value or a
    // raw bitcast f64, so handle both forms.
    if (0xFFFF0030..=0xFFFF0037).contains(&class_id) {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 {
            if let Some(actual_kind) = crate::typedarray::lookup_typed_array_kind(addr) {
                let want_id = crate::typedarray::class_id_for_kind(actual_kind);
                if want_id == class_id {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Only objects (pointers) can be instances of classes
    if !jsval.is_pointer() {
        return false_val;
    }

    // Get the object pointer
    let obj_ptr = jsval.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_val;
    }

    // Refs #421: NaN-boxed POINTER_TAG values whose unboxed payload is a
    // small registry id (Web Fetch handles, sockets, DB connections, etc.)
    // are NOT real ObjectHeader pointers — reading the GC header at
    // `obj_ptr - 8` would SIGSEGV on unmapped memory. They aren't instances
    // of any user-defined class either, so return false unconditionally.
    if (obj_ptr as usize) < 0x100000 {
        return false_val;
    }

    unsafe {
        // Special handling for built-in Error and its subclasses (TypeError, RangeError, etc.).
        // ErrorHeader uses GC_TYPE_ERROR; we match by error_kind against the requested CLASS_ID_*.
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let err_ptr = obj_ptr as *const crate::error::ErrorHeader;
            let kind = (*err_ptr).error_kind;
            return match class_id {
                crate::error::CLASS_ID_ERROR => true_val,
                crate::error::CLASS_ID_TYPE_ERROR => {
                    if kind == crate::error::ERROR_KIND_TYPE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_RANGE_ERROR => {
                    if kind == crate::error::ERROR_KIND_RANGE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_REFERENCE_ERROR => {
                    if kind == crate::error::ERROR_KIND_REFERENCE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_SYNTAX_ERROR => {
                    if kind == crate::error::ERROR_KIND_SYNTAX_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_AGGREGATE_ERROR => {
                    if kind == crate::error::ERROR_KIND_AGGREGATE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                _ => false_val,
            };
        }

        // For user-defined classes that extend Error: `myErr instanceof Error` should be true.
        if class_id == crate::error::CLASS_ID_ERROR {
            let obj_class_id = (*obj_ptr).class_id;
            if extends_builtin_error(obj_class_id) {
                return true_val;
            }
        }

        // Check if the object's class_id matches directly
        let obj_class_id = (*obj_ptr).class_id;
        if obj_class_id == class_id {
            return true_val;
        }

        // Walk up the inheritance chain using the class registry
        let mut current_class = obj_class_id;
        while let Some(parent_id) = get_parent_class_id(current_class) {
            if parent_id == 0 {
                break;
            }
            if parent_id == class_id {
                return true_val;
            }
            current_class = parent_id;
        }

        false_val
    }
}
