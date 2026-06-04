use crate::bigint::{clean_bigint_ptr, js_bigint_from_f64, js_bigint_from_i64, js_bigint_from_u64};
use crate::value::{js_nanbox_bigint, js_nanbox_pointer, JSValue};

#[cold]
fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

#[inline]
unsafe fn gc_payload_type(addr: usize) -> Option<u8> {
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header_addr = addr - crate::gc::GC_HEADER_SIZE;
    if matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    ) {
        return None;
    }
    let header = header_addr as *const crate::gc::GcHeader;
    let obj_type = (*header).obj_type;
    crate::gc::gc_type_info(obj_type).map(|_| obj_type)
}

pub(crate) fn bigint64_element_bits(value: f64) -> u64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_number() || jsval.is_int32() {
        throw_type_error(b"Cannot convert value to a BigInt");
    }
    let ptr = if jsval.is_bigint() {
        jsval.as_bigint_ptr()
    } else if jsval.is_pointer() {
        let ptr = jsval.as_pointer::<crate::bigint::BigIntHeader>();
        let addr = ptr as usize;
        unsafe {
            if gc_payload_type(addr) != Some(crate::gc::GC_TYPE_BIGINT) {
                throw_type_error(b"Cannot convert object to a BigInt");
            }
        }
        ptr
    } else {
        js_bigint_from_f64(value)
    };
    let ptr = clean_bigint_ptr(ptr);
    if ptr.is_null() {
        throw_type_error(b"Cannot convert value to a BigInt");
    }
    unsafe { (*ptr).limbs[0] }
}

#[inline]
pub(crate) fn box_bigint64(value: i64) -> f64 {
    js_nanbox_bigint(js_bigint_from_i64(value) as i64)
}

#[inline]
pub(crate) fn box_biguint64(value: u64) -> f64 {
    js_nanbox_bigint(js_bigint_from_u64(value) as i64)
}
