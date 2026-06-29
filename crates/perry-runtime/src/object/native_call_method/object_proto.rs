use super::super::*;
use super::disposal::*;
use super::proto_dispatch::*;
use super::typed_array::*;
use super::*;

pub(super) unsafe fn object_has_null_proto_flag(object: *const ObjectHeader) -> bool {
    let gc_header =
        (object as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    ((*gc_header)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO) != 0
}

pub(super) unsafe fn call_object_to_string_method(object: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_f64(object);
    let receiver = object_handle.get_nanbox_f64();
    let obj_ptr = object_ptr_from_value(receiver)?;
    let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let key_handle = scope.root_string_ptr(key);
    let key_ptr = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    let method = js_object_get_field_by_name(obj_ptr as *const ObjectHeader, key_ptr);
    if method.is_undefined() {
        if own_key_present(obj_ptr, key_ptr) || object_has_null_proto_flag(obj_ptr) {
            throw_object_to_string_not_function();
        }
        return None;
    }
    if method.is_null() {
        throw_object_to_string_not_function();
    }
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != crate::value::POINTER_TAG {
        throw_object_to_string_not_function();
    }
    let method_ptr = (method_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        throw_object_to_string_not_function();
    }
    let bound = crate::closure::clone_closure_rebind_this(method_bits, receiver);
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);
    Some(result)
}

pub(crate) unsafe fn js_object_default_value_of(receiver: f64) -> f64 {
    let jsval = JSValue::from_bits(receiver.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        throw_object_value_of_nullish_receiver();
    }
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(receiver) {
        return payload;
    }
    // Spec 20.1.3.7: `Object.prototype.valueOf` returns ToObject(this). A
    // primitive receiver (`Object.prototype.valueOf.call(true)`) yields its
    // wrapper object (`typeof` must report "object"), not the primitive.
    // Object receivers (including the fused boxed-wrapper arm above, which
    // serves the `Object(5).valueOf()` Number.prototype.valueOf resolution)
    // pass through unchanged.
    if !jsval.is_pointer() {
        return crate::object::js_object_coerce(receiver);
    }
    receiver
}

pub(crate) unsafe fn js_object_default_to_locale_string(receiver: f64) -> f64 {
    let jsval = JSValue::from_bits(receiver.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        throw_object_to_locale_string_nullish_receiver();
    }
    // #2808: numbers use `Number.prototype.toLocaleString` (thousands
    // separators), so a number element / receiver formats as `1,000.5` rather
    // than the bare `toString` form. Locale/option-aware grouping is not yet
    // modeled — the default-locale grouping matches Node's en-US output for
    // the common integer/decimal cases.
    if jsval.is_number() {
        let s = crate::date::js_number_to_locale_string(jsval.as_number());
        return f64::from_bits(JSValue::string_ptr(s).bits());
    }
    // #2808: a Date value uses `Date.prototype.toLocaleString` (date+time
    // rendering) rather than `[object Date]`.
    if crate::date::is_date_value(receiver) {
        let ts = crate::date::date_cell_timestamp(receiver);
        let s = crate::date::js_date_to_locale_string(ts);
        return f64::from_bits(JSValue::string_ptr(s).bits());
    }
    // #5580: a `Temporal.*` value formats via its own `toLocaleString` (a
    // calendar-aware, type-specific rendering plus the spec's calendar-mismatch
    // `RangeError`) rather than the `[object Object]` default.  Calls that
    // carry locale/options args bypass `Expr::DateToLocaleString` and reach
    // the Temporal dispatch via the generic method-call path (which preserves
    // args).  Only the zero-arg form arrives here; dispatch it with an empty
    // slice so the Temporal method applies its type-appropriate defaults.
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(receiver) {
        return crate::temporal::dispatch::call_method(receiver, "toLocaleString", &[]);
    }
    // Symbols are POINTER-tagged, so `!jsval.is_pointer()` would be false for
    // them — check before the pointer guard so the branch is reachable.
    let is_symbol = unsafe { crate::symbol::js_is_symbol(receiver) } != 0;
    if !jsval.is_pointer() || is_symbol {
        // Spec 20.1.3.6 Object.prototype.toLocaleString: step 1 is "Let O be
        // the this value" (NOT ToObject), step 2 is "Return ? Invoke(O,
        // 'toString')". Invoke resolves the method on the primitive's prototype
        // chain and calls it with the original primitive as `this`. A
        // user-patched Boolean/Number/BigInt/String prototype toString must be
        // honoured, and a strict callee must receive the raw primitive (not a
        // boxed wrapper) — call_primitive_closure_value handles both.
        let builtin_name: &[u8] = if jsval.is_bool() {
            b"Boolean"
        } else if jsval.is_bigint() {
            b"BigInt"
        } else if jsval.is_any_string() {
            b"String"
        } else if is_symbol {
            b"Symbol"
        } else {
            b""
        };
        if !builtin_name.is_empty() {
            if let Some(patched) =
                unsafe { super::builtin_proto_user_method(builtin_name, "toString") }
            {
                if let Some(result) =
                    unsafe { call_primitive_closure_value(receiver, patched, std::ptr::null(), 0) }
                {
                    return result;
                }
            }
        }
        return unsafe {
            js_native_call_method(
                receiver,
                b"toString".as_ptr() as *const i8,
                "toString".len(),
                std::ptr::null(),
                0,
            )
        };
    }
    // An own `toLocaleString` closure wins over the default rendering —
    // notably `%TypedArray%.prototype.toLocaleString()` invoked as a method ON
    // the prototype object itself must run the installed brand-check thunk
    // (which throws for the non-TypedArray receiver, test262
    // toLocaleString/invoked-as-method).
    {
        let own = crate::object::js_object_get_own_field_or_undef(
            receiver,
            b"toLocaleString".as_ptr(),
            14,
        );
        let own_value = JSValue::from_bits(own.to_bits());
        if let Some(result) = call_primitive_closure_value(receiver, own_value, std::ptr::null(), 0)
        {
            return result;
        }
    }
    if let Some(result) = call_object_to_string_method(receiver) {
        return result;
    }
    crate::object::js_object_to_string(receiver)
}

/// #4546: codegen entry point for `value.toLocaleString()` when the
/// receiver's static type is unknown (plain object, string, boolean) — the
/// `Expr::DateToLocaleString` LLVM arm used to mis-route every non-number
/// receiver to `js_date_to_locale_string`, yielding a 1970-epoch
/// "Invalid Date" string. Dispatches on the runtime tag (number → grouping,
/// Date → date string, object → custom/`[object Object]`). Returns an
/// already-NaN-boxed value.
#[no_mangle]
pub extern "C" fn js_value_to_locale_string(receiver: f64) -> f64 {
    unsafe { js_object_default_to_locale_string(receiver) }
}

/// Shared implementation for `Object.prototype.isPrototypeOf`.
pub(crate) unsafe fn js_object_is_prototype_of_value(receiver: f64, target: f64) -> bool {
    // The receiver (and every link in the target's `[[Prototype]]` chain) is
    // compared by raw heap address. Exotic-typed prototype objects —
    // `Array.prototype` is itself a GC_TYPE_ARRAY, `Uint8Array.prototype` a
    // typed-array proto — are NOT `GC_TYPE_OBJECT`, so resolving them with
    // `object_ptr_from_value` (which only accepts GC_TYPE_OBJECT) returned
    // `None` and the walk bailed. #4549: use the raw GC pointer instead.
    let heap_addr = |v: f64| -> Option<usize> {
        gc_pointer_and_type_from_value(v).map(|(ptr, _)| ptr as usize)
    };
    let receiver_addr = match heap_addr(receiver) {
        Some(addr) => addr,
        None => return false,
    };

    if crate::date::is_date_value(target) {
        let ctor = crate::object::js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
        let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
        if ctor_ptr == 0 {
            return false;
        }
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        if let Some(proto_addr) = heap_addr(proto) {
            return proto_addr == receiver_addr;
        }
        return false;
    }

    // A RegExp's `[[Prototype]]` chain is `RegExp.prototype → Object.prototype`.
    // The RegExpHeader isn't a plain GC_TYPE_OBJECT with a registered class
    // prototype, so the generic class-id walk below misses it (which is why
    // `RegExp.prototype.isPrototypeOf(re)` returned false). Handle it directly.
    {
        let tv = JSValue::from_bits(target.to_bits());
        if tv.is_pointer() && crate::regex::is_regex_pointer(tv.as_pointer::<u8>()) {
            for name in ["RegExp", "Object"] {
                let proto = crate::object::builtin_prototype_value(name);
                if let Some(proto_addr) = heap_addr(proto) {
                    if proto_addr == receiver_addr {
                        return true;
                    }
                }
            }
            return false;
        }
    }

    let target_jsval = JSValue::from_bits(target.to_bits());
    if !target_jsval.is_pointer() && gc_pointer_and_type_from_value(target).is_none() {
        return false;
    }

    if let Some(target_ptr) = object_ptr_from_value(target) {
        let has_instance_prototype =
            crate::object::prototype_chain::object_static_prototype(target_ptr as usize).is_some();
        if target_ptr as usize == receiver_addr {
            return false;
        }
        // A `new Func()` instance snapshots the function's current
        // `.prototype` via the object prototype side table. Honor that
        // per-instance chain before consulting the synthetic class map,
        // because later `Func.prototype = other` must not rewrite older
        // instances.
        if !has_instance_prototype {
            let mut cid = crate::object::js_object_get_class_id(target_ptr as *const ObjectHeader);
            let mut depth = 0usize;
            let mut visited: [u32; 32] = [0; 32];
            while cid != 0 && depth < visited.len() {
                if visited[..depth].contains(&cid) {
                    break;
                }
                visited[depth] = cid;

                let proto_obj = crate::object::class_registry::class_prototype_object(cid);
                let mut next_cid = 0;
                if !proto_obj.is_null() {
                    if proto_obj as usize == receiver_addr {
                        return true;
                    }
                    next_cid =
                        crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
                }

                if next_cid != 0 && next_cid != cid {
                    cid = next_cid;
                    depth += 1;
                    continue;
                }

                match crate::object::class_registry::get_parent_class_id(cid) {
                    Some(parent_id) if parent_id != 0 && parent_id != cid => {
                        cid = parent_id;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    } else {
        let (_, target_gc_type) = match gc_pointer_and_type_from_value(target) {
            Some(info) => info,
            None => return false,
        };
        // #4549: arrays and typed arrays are objects whose `[[Prototype]]`
        // chain is modeled (`Array.prototype` → `Object.prototype`,
        // `Uint8Array.prototype` → `%TypedArray%.prototype` →
        // `Object.prototype`), so they must reach the generic walk below.
        // Previously only closures/errors were allowed, so
        // `Array.prototype.isPrototypeOf([1, 2])` and
        // `Object.prototype.isPrototypeOf([])` wrongly returned `false`.
        // #4554: ArrayBuffer / SharedArrayBuffer use BufferHeader storage
        // without a GcHeader for small buffers, but they still have a modeled
        // prototype chain via `js_object_get_prototype_of`.
        if target_gc_type != crate::gc::GC_TYPE_CLOSURE
            && target_gc_type != crate::gc::GC_TYPE_ERROR
            && target_gc_type != crate::gc::GC_TYPE_ARRAY
            && target_gc_type != crate::gc::GC_TYPE_TYPED_ARRAY
            && target_gc_type != crate::gc::GC_TYPE_BUFFER
        {
            return false;
        }
    }

    let mut current = target;
    for _ in 0..32 {
        let current_addr = heap_addr(current);
        let proto = crate::object::js_object_get_prototype_of(current);
        let proto_jsval = JSValue::from_bits(proto.to_bits());
        if proto_jsval.is_null() || proto_jsval.is_undefined() {
            break;
        }
        let proto_addr = match heap_addr(proto) {
            Some(addr) => addr,
            None => break,
        };
        if current_addr == Some(proto_addr) {
            break;
        }
        if proto_addr == receiver_addr {
            return true;
        }
        current = proto;
    }

    false
}
