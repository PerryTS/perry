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
    object_alloc_with_parent_impl::<false, false>(class_id, parent_class_id, field_count)
}

/// A class-less ORDINARY object (`JSON.parse` records, `Object.create`),
/// born marked plain-ordinary before its birth stamp so the birth shape is
/// minted `Ordinary` directly (charter step 3; `mark_object_plain_ordinary`).
pub(crate) fn object_alloc_plain(field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<true, false>(0, 0, field_count)
}

/// A null-parent object must publish that edge in its birth shape, before
/// any reader can observe the object. Setting only a post-birth header bit
/// leaves the descriptor claiming the default prototype.
pub(crate) fn object_alloc_null_proto(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<false, true>(class_id, 0, field_count)
}

fn object_alloc_with_parent_impl<const PREMARK_PLAIN: bool, const BORN_NULL: bool>(
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
        if PREMARK_PLAIN {
            crate::object::shapes::store_kind::premark_plain_ordinary(ptr);
        }
        if BORN_NULL {
            // The allocator just returned this live cell; no safepoint has
            // intervened, so its trusted header is still ours to initialize.
            let gc = crate::gc::header_from_trusted_user_ptr(ptr.cast()).cast_mut();
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
        }
        // A class-less newborn's birth shape is a function of its prototype
        // edge, slot count and kind: replay the one this site last published while its
        // record still names those facts (ShapeIds are never reused).
        let memo = class_id == 0 && parent_class_id == 0;
        if memo {
            let id = KEYLESS_BIRTH.with(std::cell::Cell::get);
            if id != 0
                && crate::object::shapes::shape_is_keyless_birth_of(
                    id,
                    crate::object::shapes::object_proto_id(ptr),
                    field_count,
                    crate::object::shapes::store_kind::receiver_ordinary_kind(ptr),
                )
            {
                if crate::arena::pointer_in_nursery(ptr as usize) {
                    // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
                    (*ptr).parent_class_id = id;
                } else {
                    crate::object::shapes::stamp_object_shape_id_with_carrier_note(ptr, id);
                }
                return ptr;
            }
        }
        // #8113: the birth live-slot bound is published here and nowhere else.
        let id = crate::object::shapes::birth_publish_object_shape(ptr, field_count);
        if memo {
            KEYLESS_BIRTH.with(|birth| birth.set(id));
        }

        ptr
    }
}

thread_local! {
    /// One memo at this allocation site, validated against all birth facts
    /// on every use. A polymorphic allocation misses and replaces the memo;
    /// there is no count/kind-indexed table.
    static KEYLESS_BIRTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// An object born on a known birth shape: `class_id`, `field_count` live
/// inline slots (all `undefined`), stamped `shape_id` — a keyless ShapeId
/// minted for exactly that class and bound (#10507's prototype birth
/// record), so no descriptor is derived from the object.
pub(crate) fn object_alloc_born(
    class_id: u32,
    field_count: u32,
    shape_id: u32,
) -> *mut ObjectHeader {
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let total_size =
        std::mem::size_of::<ObjectHeader>() + alloc_field_count * std::mem::size_of::<JSValue>();
    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        let fields_ptr = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        if crate::arena::pointer_in_nursery(ptr as usize) {
            // A nursery newborn: no proof to retire, nobody inherits from it
            // and no old-generation carrier to note — the stamp is the store.
            // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
            (*ptr).parent_class_id = shape_id;
        } else {
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(ptr, shape_id);
        }
    }
    ptr
}

/// A fresh class-less object born on `shape_id`, a construction's recorded
/// FINAL shape (`shapes::shape_is_filled_birth`): `count` own keys at inline
/// slots `0..count`, linked to the prototype `proto_bits` names. One
/// allocation and one stamp stand in for the key-add and prototype
/// transitions the construction's first run took. `None`, leaving nothing
/// reachable, when the record no longer names those facts for this object
/// (the caller then takes its full sequence).
///
/// Every slot reads `undefined` until the caller fills it: the caller stores
/// each of the `count` slots through [`store_object_field_slot`] before
/// anything else can allocate.
pub(crate) fn object_alloc_filled_birth(
    shape_id: u32,
    proto_bits: u64,
    count: u32,
) -> Option<*mut ObjectHeader> {
    // Resolve the prototype identity BEFORE the allocating call. The caller
    // roots the prototype, but its copied word here would not be rewritten
    // if the allocation moved it. Only scalar identities cross the call.
    // SAFETY: the caller holds the live prototype through its root.
    let proto_id = unsafe { crate::object::shapes::stable_linked_proto_id(0, proto_bits) }?;
    let obj = object_alloc_unpublished(0, count);
    unsafe {
        // This allocator creates an unmarked class-less object. Recheck the
        // record AFTER allocation: the collector may have pruned the memo.
        let kind = crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked;
        debug_assert_eq!(
            crate::object::shapes::store_kind::receiver_ordinary_kind(obj),
            kind
        );
        if !crate::object::shapes::shape_is_filled_birth(shape_id, proto_id, count, kind) {
            return None;
        }
        // The construction fills a pointer in its first slot. Refuse a
        // recorded typed lane before using the newborn store shortcut.
        if crate::object::field_rep::slot_rep(crate::object::shapes::shape_rep_by_id(shape_id), 0)
            != crate::object::field_rep::REP_ANY
        {
            return None;
        }
        if crate::arena::pointer_in_nursery(obj as usize) {
            // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
            (*obj).parent_class_id = shape_id;
        } else {
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, shape_id);
        }
    }
    Some(obj)
}

/// The storage `js_object_alloc(class_id, field_count)` allocates (header,
/// `undefined` slots, pointer-free layout) with NO shape published: its
/// stamp word is 0. The caller stamps the object's shape before anything
/// else can allocate: a per-evaluation class object, prototype or instance
/// born directly in its template's shape (`class_object_template`).
#[inline]
pub(crate) fn object_alloc_unpublished(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let total_size = header_size + alloc_field_count * std::mem::size_of::<JSValue>();
    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }
    ptr
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
