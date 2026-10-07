//! All descriptor queries resolve to the holder's ordinary property bag.
use super::*;
#[derive(Clone, Copy)]
pub(super) enum DescriptorRoute {
    Keys(*const ObjectHeader),
}
#[inline]
pub(super) unsafe fn descriptor_route(owner: usize) -> DescriptorRoute {
    let bag = if crate::closure::is_closure_ptr(owner) {
        crate::closure::props::bag_of(owner)
    } else if crate::buffer::header::is_owned_byte_cell(owner) {
        crate::buffer::store::bag(owner)
    } else {
        match crate::value::addr_class::try_read_gc_header(owner).map(|h| h.obj_type) {
            Some(crate::gc::GC_TYPE_OBJECT) if super::key_attrs::attrs_live_in_keys(owner) => {
                owner as *mut ObjectHeader
            }
            Some(crate::gc::GC_TYPE_ARRAY) => {
                crate::array::array_property_bag(owner as *const ArrayHeader)
            }
            Some(_)
                if crate::value::addr_class::try_read_tracked_gc_header(owner).is_some()
                    && super::cell_meta_slot(owner).is_some() =>
            {
                super::cell_expando_get(owner).unwrap_or(std::ptr::null_mut())
            }
            Some(crate::gc::GC_TYPE_TEMPORAL) => super::exotic_expando::property_bag(owner),
            _ => super::handle_expando::handle_property_bag(owner as i64),
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
