//! Legacy materialized iterator arrays own their brand in their private bag.
//! The public array header has no free flag bits. The bag already travels with
//! array growth and is a traced child, so brands need no address registry.
use super::*;

pub(crate) unsafe fn mark_materialized_iterator(
    array: *mut ArrayHeader,
    brand: u64,
) -> *mut ArrayHeader {
    if array.is_null() {
        return array;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let bag = array_property_bag_ensure(array);
    let meta = crate::object::object_meta_ensure(
        crate::object::MetaObj::runtime_born(bag),
        crate::object::MetaReason::ArrayIteratorBrand,
    );
    // GC_STORE_AUDIT(NONPOINTER): scalar Map/Set brand in an internal bag.
    (*meta).native_state = brand;
    clean_arr_ptr_mut(array)
}

pub(crate) fn has_materialized_iterator_brand(addr: usize, brand: u64) -> bool {
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_gc_header(addr) else {
            return false;
        };
        if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY
            || (*header.as_ptr()).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        {
            return false;
        }
        let bag = array_property_bag(addr as *const ArrayHeader);
        !bag.is_null() && !(*bag).meta.is_null() && (*(*bag).meta).native_state == brand
    }
}
