/// Construct a native built-in base with a declared Perry class as newTarget.
/// The native constructor supplies its internal slots; the distinct newTarget
/// supplies the subclass prototype used by `instanceof` and inherited methods.
#[no_mangle]
pub unsafe extern "C" fn js_builtin_subclass_construct(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if class_id == 0 || name_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let target = super::super::js_get_global_this_builtin_value(name_ptr, name_len);
    let new_target = super::super::class_constructor_ref_value(class_id);
    js_new_function_construct_with_new_target(target, args_ptr, args_len, new_target)
}

#[used]
static KEEP_JS_BUILTIN_SUBCLASS_CONSTRUCT: unsafe extern "C" fn(
    u32,
    *const u8,
    usize,
    *const f64,
    usize,
) -> f64 = js_builtin_subclass_construct;

fn constructor_return_overrides_this(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    // Typed arrays and buffer-backed exotic objects may be represented by a
    // raw registered owner pointer rather than a NaN-boxed heap object. They
    // are still ECMAScript Objects and a base constructor returning one must
    // replace the derived constructor's provisional `this` binding.
    if crate::typedarray_props::typed_array_addr_from_value(value).is_some() {
        return true;
    }
    let bits = value.to_bits();
    let raw_addr = if jv.is_pointer() {
        (bits & crate::value::POINTER_MASK) as usize
    } else if (bits >> 48) == 0 {
        bits as usize
    } else {
        0
    };
    if raw_addr != 0 && crate::buffer::is_registered_buffer(raw_addr) {
        return true;
    }
    if !jv.is_pointer() {
        return false;
    }
    if is_callable_function_value(value) {
        return true;
    }
    // A Proxy is represented by a registered handle rather than a directly
    // dereferenceable heap pointer. It is nevertheless an Object and therefore
    // overrides the default receiver when returned from a constructor. Detect
    // it before the raw object/array probes below inspect the pointer payload.
    if crate::proxy::js_proxy_is_proxy(value) != 0 {
        return true;
    }
    let raw = jv.as_pointer::<u8>();
    if raw.is_null() {
        return false;
    }
    if super::super::is_arguments_object(raw as *const ObjectHeader) {
        return true;
    }
    unsafe {
        let arr = crate::array::clean_arr_ptr(raw as *const crate::array::ArrayHeader);
        if !arr.is_null() {
            return true;
        }
        let Some(gc_header) = crate::value::addr_class::try_read_tracked_gc_header(raw as usize)
        else {
            return false;
        };
        matches!(
            (*gc_header.as_ptr()).obj_type,
            // Per spec, a constructor returning ANY Object overrides the
            // implicit `this`. Promises are objects — a user constructor like
            // `function P(exec){ return new Promise(...) }` (the
            // `NewPromiseCapability` shape exercised by the Promise-combinator
            // test262 cases) must yield that Promise, not the empty default.
            // GC_TYPE_TEMPORAL: `new Temporal.Duration(...)` (and every other
            // Temporal constructor) is dispatched through this generic path —
            // the constructor thunk allocates a Temporal cell and returns it, so
            // that cell must override the empty default `this` (#4687).
            crate::gc::GC_TYPE_OBJECT
                | crate::gc::GC_TYPE_ERROR
                | crate::gc::GC_TYPE_PROMISE
                | crate::gc::GC_TYPE_TEMPORAL
                | crate::gc::GC_TYPE_MAP
                | crate::gc::GC_TYPE_SET
                | crate::gc::GC_TYPE_DATE_CELL
                | crate::gc::GC_TYPE_REGEXP
                | crate::gc::GC_TYPE_LAZY_ARRAY
        )
    }
}

/// Apply ECMAScript constructor return-override semantics for an inlined
/// constructor body's explicit `return <value>`. Given the implicit `this`
/// and the returned value:
///   - returned value is an Object  → it becomes the construction result;
///   - returned value is `undefined` → result is `this`;
///   - returned value is any other primitive → for a derived constructor
///     (`class X extends Y`) this is a TypeError; for a base constructor the
///     primitive is ignored and the result is `this`.
/// `is_derived` is 1 for a class with an `extends` clause, 0 otherwise.
/// Refs class/subclass/derived-class-return-override-*.
#[no_mangle]
pub extern "C" fn js_ctor_return_override(this_val: f64, return_val: f64, is_derived: i32) -> f64 {
    use crate::value::JSValue;
    if constructor_return_overrides_this(return_val) {
        return return_val;
    }
    let jv = JSValue::from_bits(return_val.to_bits());
    if jv.is_undefined() {
        return this_val;
    }
    if is_derived != 0 {
        crate::collection_iter::throw_type_error(
            "Derived constructors may only return object or undefined",
        );
    }
    // Base constructor: a returned primitive is ignored.
    this_val
}
