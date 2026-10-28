//! Shared ordinary property operations. Proxy dispatch uses installer slots.
use super::*;

pub(crate) fn is_callable(value: f64) -> bool {
    // Treat any pointer-tagged value as potentially callable. Inside the
    // closure call the runtime will no-op if the pointer isn't a closure.
    let bits = value.to_bits();
    let tag = bits & !POINTER_MASK;
    tag == POINTER_TAG && (bits & POINTER_MASK) != 0
}

pub(crate) fn closure_from(value: f64) -> *const crate::ClosureHeader {
    let bits = value.to_bits();
    ((bits & POINTER_MASK) as usize) as *const crate::ClosureHeader
}

/// Coerce a trap return value to a NaN-boxed boolean (`ToBoolean`), as the
/// `Reflect.{set,deleteProperty,defineProperty,preventExtensions}` paths must
/// return the trap's boolean result rather than discarding it.
pub(crate) fn nanbox_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

pub(crate) fn coerce_trap_bool(value: f64) -> f64 {
    nanbox_bool(crate::value::js_is_truthy(value) != 0)
}

/// Throw `TypeError: Reflect.<op> called on non-object`. Does not return.
pub(crate) fn reflect_non_object_typeerror(op: &str) -> f64 {
    let msg = format!("Reflect.{op} called on non-object");
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed);
}

/// Throw a `TypeError` with an arbitrary message. Does not return.
pub(crate) fn throw_type_error(msg: &str) -> f64 {
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed)
}

/// `String(value)` rendering of a JS value, for diagnostic messages that
/// embed the offending value (e.g. Node's `"1 is not a constructor"` and
/// the proxy construct-trap `"… non-object ('1')"`). Returns an empty
/// string on a null/unrenderable value. (#2768)
pub(crate) fn value_display_string(value: f64) -> String {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let str_ptr = crate::value::js_jsvalue_to_string(value);
    if str_ptr.is_null() {
        return String::new();
    }
    let nb = f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(nb, &mut scratch) {
        if !ptr.is_null() && len > 0 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            return String::from_utf8_lossy(bytes).into_owned();
        }
    }
    String::new()
}

/// #6828/#10482: Annex B §B.3.1 `set __proto__` semantics — shared by the
/// real accessor descriptor installed on `Object.prototype`
/// (`object/global_this/proto_methods.rs`'s setter closure, reached via
/// `own_set_descriptor` + `call_setter_with_receiver` above) and this
/// function's own caller (the walk's defensive fallback for the same key).
/// Both must behave identically, so both call this one implementation
/// instead of keeping the logic written out twice.
///
/// Per spec: a non-object/non-null `value` or a non-object `receiver` is
/// silently ignored (no throw, unlike `Object.setPrototypeOf`); a genuine
/// `[[SetPrototypeOf]]` failure (cyclic / non-extensible) still throws via
/// `js_object_set_prototype_of` itself, matching `Object.setPrototypeOf`'s
/// failure behavior for that case.
pub(crate) fn legacy_dunder_proto_set(receiver: f64, value: f64) {
    let value_bits = value.to_bits();
    let valid_proto = value_bits == TAG_NULL
        || lookup(value).is_some()
        || crate::object::class_ref_id(value).is_some()
        || unsafe { crate::object::value_is_object_like(value) };
    if valid_proto && reflect_value_is_object(receiver) {
        crate::object::js_object_set_prototype_of(receiver, value);
    }
}

/// Is `value` a Reflect-acceptable object? Heap objects, class refs (callable
/// constructors), and proxies all count. Primitives / null / undefined do not.
pub(crate) fn reflect_value_is_object(value: f64) -> bool {
    // One shared Type(value) is Object answer: a POINTER_TAG word names a
    // GC-headed object (honest pointers), so its header byte decides, and
    // native ids are objects. Raw untagged words keep the allocator proof;
    // class refs (INT32-tagged constructors) are callable objects.
    if crate::builtins::arithmetic::js_value_is_object(value) {
        return true;
    }
    let top16 = value.to_bits() >> 48;
    if top16 == 0 {
        return crate::object::js_value_is_heap_object(value);
    }
    top16 == 0x7FFE
}

/// `CreateListFromArrayLike(value)` — collect indexed `0..length` properties of
/// an array-like object into an owned `Vec<f64>`. Throws `TypeError` for a
/// non-object `value`, matching Node's `CreateListFromArrayLike called on
/// non-object`. Plain Arrays use the fast array accessors; any other object
/// reads `.length` then `[0]..[length-1]` via the field getter.
pub(crate) fn create_list_from_array_like(value: f64) -> Vec<f64> {
    // Fast path: a real Array.
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let is_pointer = top16 == 0x7FFD || (top16 == 0 && bits > 0x10000);
    if is_pointer {
        let ptr = (bits & POINTER_MASK) as usize;
        // `arguments` is an ordinary ObjectHeader backed by the arguments
        // registry, not a GC_TYPE_ARRAY. Reading it through the generic object
        // field path below loses its indexed bindings, so
        // `Reflect.apply(target, thisArg, arguments)` constructed an empty
        // argument list. Next 16's ProxyTracer forwards startActiveSpan with
        // exactly that shape; its callback was consequently never invoked and
        // the production App Route request remained pending (#8036).
        if let Some(values) = unsafe {
            crate::object::arguments_object_to_vec(ptr as *const crate::object::ObjectHeader)
        } {
            return values;
        }
        // #7531: `value` is `argumentsList` from `Reflect.apply(target,
        // thisArg, argumentsList)` / `Reflect.construct` -- caller-supplied,
        // so it can be a fetch/zlib/proxy/common-registry handle id under
        // the same POINTER_TAG as a real Array. The old floor
        // (`GC_HEADER_SIZE + 0x1000`) sits below every handle band and had
        // no other guard before the deref below -- a handle reached
        // `addr - GC_HEADER_SIZE` unconditionally.
        if crate::value::addr_class::is_plausible_heap_addr(ptr) {
            unsafe {
                let gc =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let len = crate::array::js_array_length(arr) as usize;
                    let mut out = Vec::with_capacity(len);
                    for i in 0..len {
                        let v = crate::array::js_array_get(arr, i as u32);
                        out.push(f64::from_bits(v.bits()));
                    }
                    return out;
                }
            }
        }
    }
    if !reflect_value_is_object(value) {
        throw_type_error("CreateListFromArrayLike called on non-object");
    }
    // General array-like object: read `.length`, then `[0]..[length-1]`.
    let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let len_val = {
        let obj_ptr = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
        if obj_ptr.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            crate::object::js_object_get_field_by_name_f64(obj_ptr, len_key)
        }
    };
    let len_f = f64::from_bits(len_val.to_bits());
    let len = if len_f.is_finite() && len_f > 0.0 {
        len_f as usize
    } else {
        0
    };
    // Every index key allocates, and a property read can run a getter: the
    // source object and the elements read so far must survive both, so the
    // list handed back holds their post-collection addresses (#10532 review).
    // Only values a collection can relocate take a handle — an all-primitive
    // argument list pays one scope and a tag test per element.
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(value);
    let mut out = Vec::with_capacity(len);
    let mut moved = Vec::new();
    for i in 0..len {
        let idx_str = i.to_string();
        crate::gc::collection_point("reflect.list_from_array_like.index_key");
        let key = crate::string::js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let obj_ptr =
            extract_pointer(source.get_nanbox_f64().to_bits()) as *const crate::ObjectHeader;
        let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key);
        let word = crate::value::JSValue::from_bits(v.to_bits());
        if word.is_pointer() || word.is_string() || word.is_bigint() {
            moved.push((i, scope.root_nanbox_f64(v)));
        }
        out.push(v);
    }
    for (index, handle) in moved {
        out[index] = handle.get_nanbox_f64();
    }
    out
}

/// Invoke a callable `f64` value with the supplied positional args and an
/// explicit `thisArg` binding, throwing `TypeError` if `f` is not callable.
/// Used by `Reflect.apply`. `thisArg` is passed as the `this` parameter so free
/// functions reading `this` observe it.
pub(crate) fn call_with_this_and_args(f: f64, this_arg: f64, args: &[f64]) -> f64 {
    // A concise/object-literal method reads `this` from a baked capture slot,
    // not only the `this` parameter; the shared explicit-`this` forwarder
    // rebinds it to the `Reflect.apply` receiver (no-op for arrows / plain fns
    // / bound fns) and holds the callee, receiver and arguments in handles
    // when that rebind clones (#10532 review). The receiver is passed as given.
    unsafe {
        crate::closure::forward_with_explicit_this(
            f,
            this_arg,
            args,
            crate::closure::ReceiverBinding::AsGiven,
            |call| dispatch_with_explicit_this(call.target, call.this, call.args),
        )
    }
}

/// Invoke an already-rebound callable with an explicit `this`. Nothing here
/// allocates before the callee runs, so the arguments need no protection.
pub(crate) fn dispatch_with_explicit_this(f: f64, this_arg: f64, args: &[f64]) -> f64 {
    let closure = closure_from(f);
    if closure.is_null() {
        return throw_type_error("Reflect.apply target is not a function");
    }
    let this = crate::closure::JsThis::from_f64(this_arg);
    let a = |i: usize| -> f64 {
        args.get(i)
            .copied()
            .unwrap_or(f64::from_bits(TAG_UNDEFINED))
    };
    match args.len() {
        0 => crate::closure::js_closure_call0(closure, this),
        1 => crate::closure::js_closure_call1(closure, this, a(0)),
        2 => crate::closure::js_closure_call2(closure, this, a(0), a(1)),
        3 => crate::closure::js_closure_call3(closure, this, a(0), a(1), a(2)),
        4 => crate::closure::js_closure_call4(closure, this, a(0), a(1), a(2), a(3)),
        // #10425: this arm was `_ => js_closure_call4(…)`, so every argument
        // after the fourth was dropped. The variadic entry point owns
        // arbitrary-arity dispatch, rest bundling included. (`call_trap`
        // above keeps its catch-all: a proxy trap receives at most four.)
        n => unsafe {
            crate::closure::js_closure_call_array(closure as i64, this, args.as_ptr(), n as i64)
        },
    }
}
