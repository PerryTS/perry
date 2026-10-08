//! All descriptor queries resolve to the holder's ordinary property bag.
use super::*;
#[derive(Clone, Copy)]
pub(super) enum DescriptorRoute {
    Keys(*const ObjectHeader),
}

/// Resolve an ordinary object's forwarding chain before reading its payload.
/// No allocation or collection may occur while following the chain.
pub(super) unsafe fn resolve_object_holder(mut owner: usize) -> usize {
    loop {
        let header = crate::gc::header_from_trusted_user_ptr(owner as *const u8);
        debug_assert_eq!((*header).obj_type, crate::gc::GC_TYPE_OBJECT);
        if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
            return owner;
        }
        owner = crate::gc::forwarding_address(header) as usize;
    }
}
#[inline]
pub(super) unsafe fn descriptor_route(owner: usize) -> DescriptorRoute {
    // The ordinary holder is already its descriptor storage. Keep its shape
    // reads out of closure, byte-owner and exotic admission paths.
    if super::key_attrs::attrs_live_in_keys(owner) {
        return DescriptorRoute::Keys(owner as *const ObjectHeader);
    }
    let bag = if crate::closure::is_closure_ptr(owner) {
        crate::closure::props::bag_of(owner)
    } else if crate::buffer::header::is_owned_byte_cell(owner) {
        crate::buffer::store::bag(owner)
    } else if crate::value::addr_class::try_read_gc_header(owner)
        .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_ARRAY)
    {
        // Array queries already arrive at a GC array holder. Preserve their
        // header-only route; allocator admission belongs to the otherwise
        // unclassified native-owner/cell boundary below.
        crate::array::array_property_bag(owner as *const ArrayHeader)
    } else {
        match crate::value::addr_class::try_read_tracked_gc_header(owner)
            .map(|h| (*h.as_ptr()).obj_type)
        {
            Some(crate::gc::GC_TYPE_OBJECT) => resolve_object_holder(owner) as *mut ObjectHeader,
            Some(crate::gc::GC_TYPE_ARRAY) => {
                crate::array::array_property_bag(owner as *const ArrayHeader)
            }
            Some(crate::gc::GC_TYPE_LAZY_ARRAY) => {
                let array = (*(owner as *const crate::json_tape::LazyArrayHeader)).materialized;
                if array.is_null() {
                    std::ptr::null_mut()
                } else {
                    crate::array::array_property_bag(array)
                }
            }
            Some(
                crate::gc::GC_TYPE_ERROR
                | crate::gc::GC_TYPE_MAP
                | crate::gc::GC_TYPE_SET
                | crate::gc::GC_TYPE_PROMISE
                | crate::gc::GC_TYPE_DATE_CELL,
            ) => super::cell_expando_get(owner).unwrap_or(std::ptr::null_mut()),
            Some(crate::gc::GC_TYPE_TEMPORAL) => super::exotic_expando::property_bag(owner),
            // Native registry ids and native Box backings are stable owners,
            // not GC cells. Their explicit route never admits a managed cell.
            None => super::handle_expando::handle_property_bag(owner as i64),
            Some(kind) => {
                debug_assert!(false, "GC cell type {kind} is not a descriptor holder");
                std::ptr::null_mut()
            }
        }
    };
    DescriptorRoute::Keys(bag)
}
#[inline]
pub(crate) fn may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    unsafe {
        let DescriptorRoute::Keys(bag) = descriptor_route(owner);
        !bag.is_null()
            && if accessor {
                super::key_attrs::object_key_is_accessor(bag, key.as_bytes())
            } else {
                super::key_attrs::object_key_entry(bag, key.as_bytes()) != 0
            }
    }
}
#[cfg(test)]
pub(crate) fn test_may_have_descriptor_entry(owner: usize, key: &str, accessor: bool) -> bool {
    may_have_descriptor_entry(owner, key, accessor)
}
