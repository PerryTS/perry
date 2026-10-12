//! Shared proof for direct compiled accessor calls. No lookup or invocation:
//! receiver birth ShapeId and complete declaration-holder ShapeIds are authority.
use super::{class_value::class_decl_prototype_link, ObjectHeader};

/// `chain` is immutable compiler data: class id in the low word, complete
/// prototype ShapeId in the high word, from receiver class to accessor owner.
/// Missing lazy holders are pristine; every mutation materializes them first.
///
/// # Safety
/// `receiver` is a JS value; `chain` names `len` compiler-emitted words. A
/// matching birth ShapeId proves an ordinary receiver with no own shadow and
/// its class-implied prototype. This helper never allocates or enters JS.
#[no_mangle]
pub unsafe extern "C" fn js_class_accessor_shape_guard(
    receiver: f64,
    expected: u32,
    chain: *const u64,
    len: usize,
) -> i32 {
    let bits = receiver.to_bits();
    if bits >> 48 != crate::value::POINTER_TAG >> 48 {
        return 0;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr)
        || len == 0
        || !super::shapes::is_shape_id(expected)
    {
        return 0;
    }
    let obj = addr as *const ObjectHeader;
    let Some(header) = crate::value::addr_class::try_read_gc_header_known_plausible(addr) else {
        return 0;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*obj).parent_class_id != expected
    {
        return 0;
    }
    for &word in std::slice::from_raw_parts(chain, len) {
        let proto = class_decl_prototype_link(word as u32);
        if !proto.is_null() && (*proto).parent_class_id != (word >> 32) as u32 {
            return 0;
        }
    }
    1
}
