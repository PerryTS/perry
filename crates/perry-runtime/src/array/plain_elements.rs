//! Elements store of a PLAIN object: the integer-keyed half of an ordinary
//! object, as engines keep it.
//!
//! A plain object (`{}`, class-less, born ordinary) that is given an
//! array-index key while its layout names no index key takes an elements
//! store in `ObjectMeta::elements` (a holey `GC_TYPE_ARRAY`, a traced child
//! edge exactly like an Array subclass's store) with
//! `OBJECT_META_FLAG_PLAIN_ELEMENTS` set. Element `i` that is not the hole is
//! own data property `i`: writable, enumerable, configurable. There is no
//! exotic `length`: the store's own length is its bound, never a property.
//!
//! The invariant every reader relies on: while the store exists, the object's
//! layout names no index key, so an index key is own exactly when its element
//! is present, and every other key is answered by the layout as before. The
//! shape kind records the store (`ShapeObjectKind::OrdinaryIndexed`, F-C of
//! `shapes_store_kind`): no store probe is admitted on that kind, so no
//! layout add can put an index key beside the store, and no layout reader
//! takes a layout miss of an index key as absence.
//!
//! The property funnel of `super::subclass_elements` (`own_elements`) hands
//! out the same view for both stores; [`ElementsView::plain`] tells them
//! apart. The store is left for good (every element becomes a layout
//! property, ascending, then the store is detached: [`leave`]) by every
//! operation the layout models and the store does not: a definition of an
//! index key, freeze / seal / preventExtensions, becoming a prototype (an
//! inherited read walks layouts), and an index past the store's reach.
//!
//! [`ElementsView::plain`]: super::subclass_elements::ElementsView
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;

/// The largest index a store holds, and the largest gap an add may open past
/// the store's end: V8's `kMaxGap`. A key past either keeps (or moves) every
/// index key in the layout.
pub(crate) const PLAIN_ELEMENTS_MAX_GAP: u32 = 1024;
const PLAIN_ELEMENTS_MAX_INDEX: u32 = 1 << 20;

/// `obj`'s plain elements store, or null when it has none.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` user pointer.
#[inline(always)]
pub(crate) unsafe fn store_of(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // A1_UNPROVEN: record-first: layout word of the receiver (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).flags & crate::object::OBJECT_META_FLAG_PLAIN_ELEMENTS == 0 {
        return std::ptr::null_mut();
    }
    (*meta).elements as *mut ArrayHeader
}

/// The own value of index `index` from a live store: `None` for a hole or an
/// index past its bound (not own: the layout names no index key).
///
/// # Safety
/// `store` is a live plain elements store.
#[inline(always)]
pub(crate) unsafe fn get(store: *const ArrayHeader, index: u32) -> Option<u64> {
    if index >= (*store).length {
        return None;
    }
    let bits = *crate::array::array_elements_ptr(store)
        .cast::<u64>()
        .add(index as usize);
    (bits != crate::value::TAG_HOLE).then_some(bits)
}

/// `obj[index]` read for a Number key on a live, unforwarded ordinary object:
/// `Some(value)` when its store holds the element. `None` (a hole, no store,
/// no index) leaves the read to the complete path, which reaches the
/// prototype chain.
///
/// # Safety
/// `obj` is a live, unforwarded `GC_TYPE_OBJECT` user pointer.
#[inline(always)]
pub(crate) unsafe fn get_number_key(obj: *const ObjectHeader, key: f64) -> Option<f64> {
    let store = store_of(obj);
    if store.is_null() {
        return None;
    }
    // An index is an integral Number in [0, 2^32 - 2]; `as u32` saturates,
    // and the bound check excludes everything the store cannot hold.
    let index = key as u32;
    if f64::from(index) != key {
        return None;
    }
    get(store, index).map(f64::from_bits)
}

/// `obj[index] = value` for a Number key when `obj` holds element `index`:
/// the store of an own writable data property. `false` (nothing stored) for
/// every other case; the complete `[[Set]]` decides.
///
/// # Safety
/// `obj` is a live, unforwarded `GC_TYPE_OBJECT` user pointer.
#[inline]
pub(crate) unsafe fn set_existing_number_key(
    obj: *const ObjectHeader,
    key: f64,
    value: f64,
) -> bool {
    let store = store_of(obj);
    if store.is_null() {
        return false;
    }
    let index = key as u32;
    if f64::from(index) != key || get(store, index).is_none() {
        return false;
    }
    crate::array::js_array_set_f64(store, index, value);
    true
}

/// May `obj` take a store for its first index key `index`? A plain record
/// (no class surface; ordinary data slots: no arguments object, no typed-array
/// prototype) of the `Ordinary` kind whose layout names no index key, with
/// no descriptor, no integrity level, no overflow table and no prototype
/// role, and an index within the store's reach.
///
/// # Safety
/// `obj` is a live, unforwarded `GC_TYPE_OBJECT` user pointer.
unsafe fn admits_store(obj: *mut ObjectHeader, index: u32) -> bool {
    if index >= PLAIN_ELEMENTS_MAX_GAP {
        return false;
    }
    let reserved = (*crate::gc::header_from_trusted_user_ptr(obj as *const u8))._reserved;
    if reserved
        & (crate::gc::OBJ_FLAG_FROZEN
            | crate::gc::OBJ_FLAG_SEALED
            | crate::gc::OBJ_FLAG_NO_EXTEND
            | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS)
        != 0
        // F-A (plain data slots: no arguments object, no typed-array
        // prototype, no unmarked class-less record) and no class surface.
        || !crate::object::shapes::store_kind::receiver_admits_plain_store(obj)
        || !crate::object::shapes::object_is_plain_record(obj)
    {
        return false;
    }
    if (*obj).parent_class_id & crate::object::overflow::SHADOW_BIT != 0 {
        return false;
    }
    // A1_UNPROVEN: record-first: layout word of the receiver (kind-A/B question for S0f).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
    if !meta.is_null()
        && ((*meta).elements != 0
            || (*meta).overflow != 0
            || (*meta).boxed_primitive != 0
            || (*meta).flags
                & (crate::object::OBJECT_META_FLAG_IS_PROTOTYPE
                    | crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                    | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                != 0)
    {
        return false;
    }
    let Some(record) = crate::object::shapes::object_shape_record(obj) else {
        return false;
    };
    record.kind_is(crate::object::shapes::ShapeObjectKind::Ordinary)
        && record.enum_bits() & crate::object::key_attrs::ENUM_INDEX_KEY == 0
}

/// The own-data store of the index key named by `key` on `obj` at the point
/// the layout would hold it (`js_object_set_field_by_name`): an element of
/// `obj`'s store, or `false` when the layout keeps the key (no store, or a
/// key past the store's reach, which first moves every index key to the
/// layout). Never installs a store: this funnel also stores the value of a
/// definition whose attributes the layout then records.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` user pointer or a POINTER-tagged box of
/// one; `key` a live string header (or null).
pub(crate) unsafe fn set_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    set_by_name_with(obj, key, value, false)
}

/// [`set_by_name`] for an ordinary `[[Set]]` whose interception checks have
/// passed (`property_ops::set::target_set`): a plain object's first index key
/// also takes a new store.
///
/// # Safety
/// As [`set_by_name`].
pub(crate) unsafe fn set_by_name_admitting(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    set_by_name_with(obj, key, value, true)
}

unsafe fn set_by_name_with(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
    admit: bool,
) -> bool {
    let Some(super::subclass_elements::ElementsKey::Index(index)) =
        super::subclass_elements::key_of_header(key)
    else {
        return false;
    };
    let raw = if (obj as u64) >> 48 == 0x7FFD {
        (obj as u64 & crate::value::POINTER_MASK) as *mut ObjectHeader
    } else {
        obj
    };
    let Some(header) = crate::value::addr_class::try_read_gc_header(raw as usize) else {
        return false;
    };
    if crate::gc_type_is_not_object!(header.obj_type)
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return false;
    }
    set_index(raw, index, value, admit)
}

/// [`set_by_name_with`] for an index in hand.
///
/// # Safety
/// `obj` is a live, unforwarded `GC_TYPE_OBJECT` user pointer.
unsafe fn set_index(obj: *mut ObjectHeader, index: u32, value: f64, admit: bool) -> bool {
    let store = store_of(obj);
    if store.is_null() {
        if !admit || !admits_store(obj, index) {
            return false;
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_h = scope.root_raw_mut_ptr(obj);
        let value_h = scope.root_nanbox_f64(value);
        let (_, obj) = obj_h.across_mut::<ObjectHeader, _>(|| install(obj));
        return put(obj, store_of(obj), index, value_h.get_nanbox_f64());
    }
    if put(obj, store, index, value) {
        return true;
    }
    // Past the store's reach: every index key moves to the layout, which
    // then holds this one as well.
    leave(obj);
    false
}

/// Install an empty store on `obj` and move it to the `OrdinaryIndexed` kind.
///
/// # Safety
/// As [`set_index`]; allocates.
unsafe fn install(obj: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let (_, _) = obj_h.across_mut::<ObjectHeader, _>(|| {
        crate::object::object_meta_ensure(
            crate::meta_obj_runtime_born!(obj),
            crate::object::MetaReason::PlainElements,
        )
    });
    let (store, obj) =
        obj_h.across_mut::<ObjectHeader, _>(|| crate::array::js_array_alloc_with_length_exact(0));
    set_head(obj, store);
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
    // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit.
    (*meta).flags |= crate::object::OBJECT_META_FLAG_PLAIN_ELEMENTS;
    let (_, _) = obj_h.across_mut::<ObjectHeader, _>(|| {
        crate::object::shapes::store_kind::restamp_object_store_kind(obj)
    });
}

/// Store `store` as `obj`'s elements head (a barriered meta-record slot).
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` with a meta record.
unsafe fn set_head(obj: *mut ObjectHeader, store: *mut ArrayHeader) {
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
    // GC_STORE_AUDIT(BARRIERED): meta-record child edge, as `set_elements_head`.
    (*meta).elements = store as u64;
    crate::gc::runtime_write_barrier_slot(
        meta as usize,
        &(*meta).elements as *const _ as usize,
        store as u64,
    );
}

/// Element `index` = `value` in `obj`'s live store: in place, a hole fill, or
/// an extension by at most [`PLAIN_ELEMENTS_MAX_GAP`]. `false` (nothing
/// stored) for an index past that reach.
///
/// # Safety
/// `store` is `obj`'s live store.
unsafe fn put(obj: *mut ObjectHeader, store: *mut ArrayHeader, index: u32, value: f64) -> bool {
    let length = (*store).length;
    if index < length {
        crate::array::js_array_set_f64(store, index, value);
        return true;
    }
    if index >= PLAIN_ELEMENTS_MAX_INDEX || index - length >= PLAIN_ELEMENTS_MAX_GAP {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let value_h = scope.root_nanbox_f64(value);
    let (head, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
        if index == length {
            crate::array::js_array_push_f64(store, value_h.get_nanbox_f64())
        } else {
            crate::array::js_array_set_f64_extend(store, index, value_h.get_nanbox_f64())
        }
    });
    if !head.is_null() && head != store_of(obj) {
        set_head(obj, head);
    }
    true
}

/// Leave the store for good: every present element becomes an index property
/// of the layout (ascending), the store is detached and the shape returns to
/// the `Ordinary` kind. A no-op for an object without a store.
///
/// # Safety
/// `obj` is a live, unforwarded `GC_TYPE_OBJECT` user pointer; allocates.
pub(crate) unsafe fn leave(obj: *mut ObjectHeader) {
    let store = store_of(obj);
    if store.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_raw_mut_ptr(obj);
    let store_h = scope.root_raw_mut_ptr(store);
    // Detach first: the layout stores below must not be routed back here.
    // The shape keeps the `OrdinaryIndexed` kind until the first of them, so
    // no store is admitted again meanwhile; the first index key in the
    // layout then refuses one for good.
    set_head(obj, std::ptr::null_mut());
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj));
    // GC_STORE_AUDIT(POINTER_FREE): scalar classification bit.
    (*meta).flags &= !crate::object::OBJECT_META_FLAG_PLAIN_ELEMENTS;
    let length = (*store).length;
    for index in 0..length {
        let present = store_h.with_mut_ptr(|s: *mut ArrayHeader| {
            *crate::array::array_elements_ptr(s)
                .cast::<u64>()
                .add(index as usize)
                != crate::value::TAG_HOLE
        });
        if !present {
            continue;
        }
        let name = index.to_string();
        let (key, obj) = obj_h.across_mut::<ObjectHeader, _>(|| {
            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
        });
        // Read after the key's allocation, which can move the value.
        let bits = store_h.with_mut_ptr(|s: *mut ArrayHeader| {
            *crate::array::array_elements_ptr(s)
                .cast::<u64>()
                .add(index as usize)
        });
        crate::object::js_object_set_field_by_name(obj, key, f64::from_bits(bits));
    }
    let obj = obj_h.with_mut_ptr::<ObjectHeader, _>(|obj_h_ptr| obj_h_ptr);
    crate::object::shapes::store_kind::restamp_object_store_kind(obj);
}

/// A complete default data definition (`{ value, writable, enumerable,
/// configurable }` all true) of index `index` on the object `value`: the
/// element is exactly such a property, so on an object with an elements store
/// that reaches `index`, or a plain object that takes one for it
/// ([`admits_store`]: extensible, no index key in its layout), the definition
/// is the element's store (`true`). `false`, having changed nothing, for
/// every other object or index.
pub(crate) fn define_default_value(value: f64, index: u32, element: f64) -> bool {
    let js = crate::JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
            return false;
        };
        if crate::gc_type_is_not_object!(header.obj_type)
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        set_index(raw as *mut ObjectHeader, index, element, true)
    }
}

/// [`leave`] for a NaN-boxed value that may be anything.
pub(crate) fn leave_value(value: f64) {
    let js = crate::JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return;
    }
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_gc_header(raw) else {
            return;
        };
        if crate::gc_type_is_not_object!(header.obj_type)
            || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return;
        }
        leave(raw as *mut ObjectHeader);
    }
}
