//! Basic object allocation and coercion.

use super::*;

/// Allocate a new object with the given class ID and field count
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    js_object_alloc_with_parent(class_id, 0, field_count)
}

/// `Object(value)` plain-call coercion (#3149, ECMAScript §20.1.1.1 / ToObject).
///
/// Takes and returns a NaN-boxed JSValue (`f64`):
/// - `undefined` / `null` / no-arg → a fresh ordinary `{}`.
/// - an existing object/array/function (any pointer value) → returned unchanged.
/// - primitive values → boxed primitive wrapper objects so
///   `Object(true).valueOf()`, `Object(0).valueOf()`,
///   `Object("x").valueOf()`, and util.types boxed checks match Node.
///
/// The `new Object(value)` form is handled separately by
/// `js_new_function_construct`'s `"Object"` arm; this is only the bare-call
/// path that previously fell through to the generic dispatcher and returned
/// `undefined`.
#[no_mangle]
pub extern "C" fn js_object_coerce(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        let obj = js_object_alloc(0, 0);
        return crate::value::js_nanbox_pointer(obj as i64);
    }
    if jsval.is_bigint() {
        return crate::builtins::js_boxed_bigint_new(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return crate::builtins::js_boxed_symbol_new(value);
    }
    if jsval.is_pointer() {
        // Already an object/array/function — pass through unchanged.
        return value;
    }
    if jsval.is_bool() {
        return crate::builtins::js_boxed_boolean_new(value);
    }
    if jsval.is_any_string() {
        return crate::builtins::js_boxed_string_new(value, 1);
    }
    if crate::object::class_ref_id(value).is_some() {
        // A constructor ClassRef shares the INT32 encoding but is already a
        // Function object, so ToObject returns it unchanged (#10461).
        return value;
    }
    crate::builtins::js_boxed_number_new(value)
}

/// Allocate a new object with class ID, parent class ID, and field count
/// The parent_class_id is used for instanceof inheritance checks
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    // Register this class's parent for inheritance lookups
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate at least INLINE_SLOT_FLOOR field slots to match
    // js_object_set_field_by_name's alloc_limit assumption
    // (max(field_count, INLINE_SLOT_FLOOR)). Without this, empty objects ({})
    // with field_count=0 would have 0 field slots but
    // js_object_set_field_by_name writes up to the floor inline, causing a heap
    // buffer overflow into adjacent arena objects.
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();

        // Initialize ALL allocated field slots to undefined (not just field_count)
        // We allocate max(field_count, 8) slots but must zero all of them to prevent
        // stale data from previously freed GC objects from bleeding through.
        let fields_ptr = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // #8113: the birth live-slot bound is published here and nowhere else.
        crate::object::shapes::birth_publish_object_shape(ptr, field_count);

        ptr
    }
}

/// Fast object allocation using bump allocator - NO field initialization
/// This is significantly faster for hot paths where constructor immediately sets all fields
/// Returns a pointer to the object header with UNINITIALIZED fields
#[no_mangle]
pub extern "C" fn js_object_alloc_fast(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header only - fields left uninitialized for constructor to fill
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // #8113: the birth live-slot bound is published here and nowhere else.
        crate::object::shapes::birth_publish_object_shape(ptr, field_count);
    }

    ptr
}

/// Fast object allocation with parent class ID - NO field initialization
#[no_mangle]
pub extern "C" fn js_object_alloc_fast_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    // Only register class if it has a parent (one-time operation per class)
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // #8113: the birth live-slot bound is published here and nowhere else.
        crate::object::shapes::birth_publish_object_shape(ptr, field_count);
    }

    ptr
}
