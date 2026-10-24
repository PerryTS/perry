//! Basic object allocation and coercion.

use super::*;
use crate::codegen_abi::LayoutWord;

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
    if jsval.is_reference() {
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
    object_alloc_with_parent_impl::<false, false, false>(class_id, parent_class_id, field_count)
}

/// A class-less ORDINARY object (`JSON.parse` records, `Object.create`),
/// born marked plain-ordinary before its birth stamp so the birth shape is
/// minted `Ordinary` directly (charter step 3; `mark_object_plain_ordinary`).
pub(crate) fn object_alloc_plain(field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<true, false, false>(0, 0, field_count)
}

/// Literal builders have ordinary [[Set]] from birth, including before their
/// source-ordered descriptor definitions. Reuse the plain-record birth funnel.
#[no_mangle]
pub extern "C" fn js_object_alloc_plain(field_count: u32) -> *mut ObjectHeader {
    object_alloc_plain(field_count)
}

/// A statically identified module-singleton literal prototype. O3 storage
/// uses the same ordinary birth mint and barriered construction as a young
/// literal; only its allocation generation differs. Its ShapeId remains the
/// authority for reads, writes and weak holder-memo reachability.
#[no_mangle]
pub extern "C" fn js_object_literal_prototype_birth() -> f64 {
    let object = object_alloc_with_parent_impl::<true, false, true>(0, 0, 0);
    crate::value::js_nanbox_pointer(object as i64)
}

/// Keyless ordinary birth: Object.create or an ordinary function's existing
/// scalar birth proof. Publish only the final prototype shape. Only
/// scalar shape/prototype identities cross the allocating call; the prototype
/// is refreshed through its caller's handle afterwards.
pub(crate) fn object_alloc_created(
    proto: &crate::gc::RuntimeHandle<'_>,
    proto_id: u64,
    birth: shapes::KeylessBirth,
    recorded: LayoutWord,
) -> *mut ObjectHeader {
    use crate::object::shapes;
    let width = birth.width();
    let mut shape = shapes::created_birth_shape(proto_id, proto.get_nanbox_u64(), &birth, recorded);
    let alloc_width = (width as usize).max(crate::object::INLINE_SLOT_FLOOR);
    let size = std::mem::size_of::<ObjectHeader>() + alloc_width * 8;
    let fast = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
        as *mut ObjectHeader;
    let no_collection = !fast.is_null();
    let mut obj = if no_collection {
        // The shape was just resolved and no collection can have retired it.
        // This final shape traces exactly `width` slots. A zero-bound birth
        // can leave its physical floor hidden until stores publish slots;
        // tracking/wide births must still initialize all traced slack.
        unsafe { init_unpublished(fast, 0, width as usize, LayoutWord(shape)) };
        fast
    } else {
        object_alloc_unpublished(0, width)
    };
    unsafe {
        shapes::store_kind::premark_plain_ordinary(obj);
        // A full collection during allocation may retire an uncarried record.
        // Remint those same birth facts, with the unpublished newborn rooted.
        if !no_collection
            && !shapes::shape_is_keyless_birth_of(
                LayoutWord(shape),
                proto_id,
                width,
                shapes::ShapeObjectKind::Ordinary,
            )
        {
            let scope = crate::gc::RuntimeHandleScope::new();
            let owner = scope.root_raw_mut_ptr(obj);
            (shape, obj) = owner.across_mut::<ObjectHeader, _>(|| {
                shapes::created_birth_shape(proto_id, proto.get_nanbox_u64(), &birth, LayoutWord(0))
            });
        }
        if !no_collection {
            if crate::arena::pointer_in_nursery(obj as usize) {
                // GC_STORE_AUDIT(POINTER_FREE): birth publication is a ShapeId.
                (*obj).parent_class_id = shape;
            } else {
                shapes::stamp_object_shape_id_with_carrier_note(obj, LayoutWord(shape));
            }
        }
        // The prototype edge lives outside the heap. A black newborn must
        // still shade it during an incremental mark, as the link funnel does.
        crate::gc::runtime_shade_external_edge(proto.get_nanbox_u64());
        shapes::store_kind::check_store_facts(obj);
    }
    obj
}

/// An intrinsic's empty object, born with its internal brand and class
/// prototype in one shape. Only scalar facts cross allocation; no literal
/// keys cache, intermediate prototype shape or brand transition is needed.
pub(crate) fn object_alloc_branded(class_id: u32) -> *mut ObjectHeader {
    let shape = shapes::branded_birth_layout(class_id);
    let width = super::INLINE_SLOT_FLOOR;
    let size = std::mem::size_of::<ObjectHeader>() + width * 8;
    let fast = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
        as *mut ObjectHeader;
    if !fast.is_null() {
        unsafe {
            init_unpublished(fast, class_id, width, LayoutWord(shape));
            shapes::store_kind::check_store_facts(fast);
        }
        return fast;
    }
    let object = object_alloc_unpublished(class_id, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(object);
    let object = owner.get_raw_mut_ptr::<ObjectHeader>();
    unsafe {
        if crate::arena::pointer_in_nursery(object as usize) {
            // GC_STORE_AUDIT(POINTER_FREE): the sole birth publication is a ShapeId.
            (*object).parent_class_id = shape;
        } else {
            shapes::stamp_object_shape_id_with_carrier_note(object, LayoutWord(shape));
        }
        shapes::store_kind::check_store_facts(object);
    }
    object
}

/// A null-parent object must publish that edge in its birth shape, before
/// any reader can observe the object. Setting only a post-birth header bit
/// leaves the descriptor claiming the default prototype.
pub(crate) fn object_alloc_null_proto(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<false, true, false>(class_id, 0, field_count)
}

/// `Object.create(null)`: an ORDINARY object with no [[Prototype]], born
/// marked plain-ordinary before its first stamp, so its birth shape is the
/// `Ordinary` kind with `PROTO_ID_NULL`. That shape proves a store or add
/// meets no inherited setter or non-writable property (there is no chain),
/// the same proof an `Object.setPrototypeOf(o, null)` relink of a literal
/// already carries. Runtime-born records that are not plain dictionaries
/// (namespaces, scopes, resources, internal state) keep
/// [`object_alloc_null_proto`]: their births do not establish ordinariness.
pub(crate) fn object_alloc_created_null_proto() -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<true, true, false>(0, 0, 0)
}

/// A runtime-born plain dictionary with no [[Prototype]] (`querystring.parse`,
/// `Object.groupBy`, a sqlite row, `util.parseEnv`): the same ordinary birth
/// as `Object.create(null)`, with room for `field_count` inline slots. Its
/// keys are own data properties only, so the birth shape is `Ordinary` with
/// `PROTO_ID_NULL`.
pub(crate) fn object_alloc_ordinary_null_proto(field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<true, true, false>(0, 0, field_count)
}

/// A prototype the runtime builds itself (a declared class's `prototype`,
/// born in its final shape): allocated old and never-move from birth
/// (owner decision O3; `gc::never_move`), so the site memos that answer
/// from it are admitted at once instead of after the next minor. Its stores
/// go through the ordinary barriered slot stores.
pub(crate) fn object_alloc_prototype_old(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<false, false, true>(class_id, 0, field_count)
}

/// [`object_alloc_prototype_old`] born with a null [[Prototype]]
/// (`%Object.prototype%`).
pub(crate) fn object_alloc_prototype_old_null_proto(
    class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    object_alloc_with_parent_impl::<false, true, true>(class_id, 0, field_count)
}

fn object_alloc_with_parent_impl<
    const PREMARK_PLAIN: bool,
    const BORN_NULL: bool,
    const BORN_OLD_NEVER_MOVE: bool,
>(
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

    let ptr = if BORN_OLD_NEVER_MOVE {
        crate::arena::arena_alloc_gc_old_born_tenured(total_size, 8, crate::gc::GC_TYPE_OBJECT)
            as *mut ObjectHeader
    } else {
        arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader
    };

    unsafe {
        if BORN_OLD_NEVER_MOVE {
            crate::gc::never_move::request_never_move(ptr as usize);
        }
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
        // GC_TYPE_OBJECT layout is carried by its shape, not header layout bits.
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
                    LayoutWord(id),
                    crate::object::shapes::object_proto_id(ptr),
                    field_count,
                    crate::object::shapes::store_kind::receiver_ordinary_kind(ptr),
                )
            {
                if crate::arena::pointer_in_nursery(ptr as usize) {
                    // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
                    (*ptr).parent_class_id = id;
                } else {
                    crate::object::shapes::stamp_object_shape_id_with_carrier_note(
                        ptr,
                        LayoutWord(id),
                    );
                }
                return ptr;
            }
        }
        // #8113: the birth live-slot bound is published here and nowhere else.
        let id = crate::object::shapes::birth_publish_object_shape(ptr, field_count);
        if memo {
            KEYLESS_BIRTH.with(|birth| birth.set(id.0));
            crate::object::shape_carriers::note_shape_id(id);
        }

        ptr
    }
}

thread_local! {
    /// One memo at this allocation site, validated against all birth facts
    /// on every use. A polymorphic allocation misses and replaces the memo;
    /// there is no count/kind-indexed table. The memo word is dereferenced by
    /// that validation, so it is a cache carrier (note_keyless_birth_carrier).
    static KEYLESS_BIRTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Claim the layout word the keyless-birth memo names (full-trace recompute).
pub(crate) fn note_keyless_birth_carrier() {
    let id = KEYLESS_BIRTH.with(std::cell::Cell::get);
    if id != 0 {
        crate::object::shape_carriers::note_shape_id(LayoutWord(id));
    }
}

/// An object born on a known birth shape: `class_id`, `field_count` live
/// inline slots (all `undefined`), stamped `shape_id` — a keyless ShapeId
/// minted for exactly that class and bound (#10507's prototype birth
/// record), so no descriptor is derived from the object.
pub(crate) fn object_alloc_born(
    class_id: u32,
    field_count: u32,
    shape_id: LayoutWord,
) -> *mut ObjectHeader {
    object_alloc_born_impl(class_id, field_count, shape_id, false)
}

/// A plain ordinary receiver on a carried shape, or its keyless birth shape
/// when the memo is absent or collection pruned it. Returns the receiver
/// and, when it was born on `shape_id`, that shape's representation word
/// (read from the same record that validated it).
///
/// The common birth is `object_alloc_created`'s: the open nursery block
/// serves it with no collection point (`arena_alloc_gc_no_collect`), so the
/// shape validated just before is still live at the stamp and the newborn is
/// known young. Otherwise validate AFTER allocating: a scalar ShapeId memo
/// does not retain the descriptor or its keys.
#[inline(always)]
pub(crate) fn object_alloc_plain_born(
    field_count: u32,
    shape_id: LayoutWord,
) -> (*mut ObjectHeader, Option<u64>) {
    // Presence, ordinary kind and width are read from the same record.
    if let Some(rep) = shapes::plain_birth_rep_for_word(shape_id, field_count) {
        let alloc_width = (field_count as usize).max(crate::object::INLINE_SLOT_FLOOR);
        let size = std::mem::size_of::<ObjectHeader>() + alloc_width * 8;
        let fast = crate::arena::arena_alloc_gc_no_collect(size, 8, crate::gc::GC_TYPE_OBJECT)
            as *mut ObjectHeader;
        if !fast.is_null() {
            unsafe {
                // GC_STORE_AUDIT(INIT): fresh nursery receiver; its stamp is a scalar ShapeId.
                init_unpublished(fast, 0, alloc_width, shape_id);
                crate::object::shapes::store_kind::premark_plain_ordinary(fast);
            }
            return (fast, Some(rep));
        }
    }
    object_alloc_plain_born_slow(field_count, shape_id)
}

/// [`object_alloc_plain_born`] through the collecting allocator.
#[cold]
#[inline(never)]
fn object_alloc_plain_born_slow(
    field_count: u32,
    shape_id: LayoutWord,
) -> (*mut ObjectHeader, Option<u64>) {
    let object = object_alloc_unpublished(0, field_count);
    unsafe {
        crate::object::shapes::store_kind::premark_plain_ordinary(object);
        let rep = shapes::plain_birth_rep_for_word(shape_id, field_count);
        if rep.is_some() {
            if crate::arena::pointer_in_nursery(object as usize) {
                // GC_STORE_AUDIT(POINTER_FREE): fresh nursery receiver's scalar ShapeId.
                (*object).parent_class_id = shape_id.0;
            } else {
                crate::object::shapes::stamp_object_shape_id_with_carrier_note(object, shape_id);
            }
        } else {
            crate::object::shapes::birth_publish_object_shape(object, field_count);
        }
        (object, rep)
    }
}

fn object_alloc_born_impl(
    class_id: u32,
    field_count: u32,
    shape_id: LayoutWord,
    premark_plain: bool,
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
        // GC_TYPE_OBJECT layout is carried by its shape, not header layout bits.
        if premark_plain {
            crate::object::shapes::store_kind::premark_plain_ordinary(ptr);
        }
        if crate::arena::pointer_in_nursery(ptr as usize) {
            // A nursery newborn: no proof to retire, nobody inherits from it
            // and no old-generation carrier to note — the stamp is the store.
            // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
            (*ptr).parent_class_id = shape_id.0;
        } else {
            crate::object::shapes::stamp_object_shape_id_with_carrier_note(ptr, shape_id);
        }
    }
    ptr
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
    unsafe { init_unpublished(ptr, class_id, alloc_field_count, LayoutWord(0)) };
    ptr
}

/// Initialize storage before a birth shape is published. No collection point.
#[inline(always)]
pub(super) unsafe fn init_unpublished(
    ptr: *mut ObjectHeader,
    class_id: u32,
    initialized_slots: usize,
    shape: LayoutWord,
) {
    let header_size = std::mem::size_of::<ObjectHeader>();
    unsafe {
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = shape.0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..initialized_slots {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        // Objects carry layout in the birth shape; layout_init_pointer_free
        // would classify this known object only to return without a write.
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
        // GC_TYPE_OBJECT layout is carried by its shape, not header layout bits.
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
        // GC_TYPE_OBJECT layout is carried by its shape, not header layout bits.
        // #8113: the birth live-slot bound is published here and nowhere else.
        crate::object::shapes::birth_publish_object_shape(ptr, field_count);
    }

    ptr
}

#[cfg(test)]
mod filled_birth_tests {
    use super::*;

    #[test]
    fn literal_prototype_birth_is_immediately_memo_admitted() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let cycles = crate::gc::copying_minor_cycles();
        let value = js_object_literal_prototype_birth();
        let object = crate::JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>();
        unsafe {
            assert!(crate::gc::never_move::lives_old(object as usize));
            let shape = shapes::shape_record_of_receiver((*object).shape_word()).unwrap();
            assert_eq!(shape.object_kind(), shapes::ShapeObjectKind::Ordinary);
            let header = crate::gc::header_from_trusted_user_ptr(object.cast());
            assert_eq!((*header).gc_flags & crate::gc::GC_FLAG_PINNED, 0);
        }
        assert_eq!(cycles, crate::gc::copying_minor_cycles());
    }
}
