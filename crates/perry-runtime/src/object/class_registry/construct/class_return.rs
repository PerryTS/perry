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
    let current_target = crate::object::js_new_target_get();
    let new_target = if is_callable_function_value(current_target) || is_class_object_value(current_target) {
        current_target
    } else {
        super::super::class_constructor_ref_value(class_id)
    };
    js_new_function_construct_with_new_target(target, args_ptr, args_len, new_target)
}

#[used(compiler)]
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
    // A POINTER_TAG word names a GC-headed object (honest pointers): its
    // header kind answers `Type(value) is Object` (Proxy cells, closures,
    // arrays, collections and exotic cells are objects; symbols are not).
    if jv.is_pointer() {
        return crate::builtins::arithmetic::js_value_is_object(value);
    }
    // Native ids (Web Fetch, streams) are objects.
    if jv.is_handle() {
        return true;
    }
    // Typed arrays and buffer-backed exotic objects may be represented by a
    // raw registered owner pointer rather than a NaN-boxed heap object. They
    // are still ECMAScript Objects and a base constructor returning one must
    // replace the derived constructor's provisional `this` binding.
    if crate::typedarray_props::typed_array_addr_from_value(value).is_some() {
        return true;
    }
    let bits = value.to_bits();
    // A raw word may be arbitrary bits (a top-16-clear number), so the
    // allocator must vouch for it before its header is read.
    (bits >> 48) == 0 && bits != 0 && crate::buffer::buffer_family_type_owned(bits as usize).is_some()
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
