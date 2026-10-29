//! URL instance layout proof shared with ordinary object operations.
use crate::object::ObjectHeader;

pub(crate) unsafe fn is_gc_object_header(obj: *mut ObjectHeader) -> bool {
    if obj.is_null() || !crate::object::is_valid_obj_ptr(obj as *const u8) {
        return false;
    }
    crate::value::addr_class::try_read_gc_header(obj as usize)
        .is_some_and(|header| crate::gc::gc_type_is_object(header.obj_type))
}

pub(crate) fn is_url_object_shape(url: *mut ObjectHeader) -> bool {
    if url.is_null() {
        return false;
    }
    unsafe {
        is_gc_object_header(url) && (*url).class_id == crate::object::runtime_layouts::URL_CLASS_ID
    }
}
