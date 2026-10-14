//! Static runtime-record allocation through the shared descriptor ABI.
use crate::{JsValue, ObjectHeader, TransientRootedAddr};

/// Opaque address of a runtime-owned layout declaration.
#[repr(C)]
pub struct RuntimeLayout {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn perry_runtime_image_result(handle: i64, width: f64, height: f64) -> i64;
    fn perry_alloc_runtime_record(desc: *const RuntimeLayout) -> *mut ObjectHeader;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_V026"]
    pub static RECORD_V026: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F0_0"]
    pub static RECORD_F0_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F3_0"]
    pub static RECORD_F3_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F6_0"]
    pub static RECORD_F6_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F7_0"]
    pub static RECORD_F7_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F13_0"]
    pub static RECORD_F13_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F17_0"]
    pub static RECORD_F17_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F20_0"]
    pub static RECORD_F20_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F21_0"]
    pub static RECORD_F21_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F28_0"]
    pub static RECORD_F28_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F29_0"]
    pub static RECORD_F29_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F30_0"]
    pub static RECORD_F30_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F31_0"]
    pub static RECORD_F31_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F37_0"]
    pub static RECORD_F37_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F327_0"]
    pub static RECORD_F327_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F328_0"]
    pub static RECORD_F328_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F411_0"]
    pub static RECORD_F411_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F412_0"]
    pub static RECORD_F412_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F413_0"]
    pub static RECORD_F413_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F414_0"]
    pub static RECORD_F414_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F418_0"]
    pub static RECORD_F418_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F426_0"]
    pub static RECORD_F426_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F427_0"]
    pub static RECORD_F427_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F429_0"]
    pub static RECORD_F429_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F430_0"]
    pub static RECORD_F430_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F434_0"]
    pub static RECORD_F434_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F470_0"]
    pub static RECORD_F470_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F471_0"]
    pub static RECORD_F471_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F473_0"]
    pub static RECORD_F473_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F483_0"]
    pub static RECORD_F483_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F487_0"]
    pub static RECORD_F487_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_F568_0"]
    pub static RECORD_F568_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_PCHECKED031"]
    pub static RECORD_PCHECKED031: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_U610"]
    pub static RECORD_U610: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_U613"]
    pub static RECORD_U613: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q435_0"]
    pub static RECORD_Q435_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q436_0"]
    pub static RECORD_Q436_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q439_0"]
    pub static RECORD_Q439_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q440_0"]
    pub static RECORD_Q440_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q441_0"]
    pub static RECORD_Q441_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q443_0"]
    pub static RECORD_Q443_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q448_0"]
    pub static RECORD_Q448_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q449_0"]
    pub static RECORD_Q449_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q450_0"]
    pub static RECORD_Q450_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q451_0"]
    pub static RECORD_Q451_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q452_0"]
    pub static RECORD_Q452_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q453_0"]
    pub static RECORD_Q453_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q454_0"]
    pub static RECORD_Q454_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q455_0"]
    pub static RECORD_Q455_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q456_0"]
    pub static RECORD_Q456_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q457_0"]
    pub static RECORD_Q457_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q458_0"]
    pub static RECORD_Q458_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q459_0"]
    pub static RECORD_Q459_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q460_0"]
    pub static RECORD_Q460_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q461_0"]
    pub static RECORD_Q461_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q462_0"]
    pub static RECORD_Q462_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q463_0"]
    pub static RECORD_Q463_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q464_0"]
    pub static RECORD_Q464_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q465_0"]
    pub static RECORD_Q465_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q466_0"]
    pub static RECORD_Q466_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q467_0"]
    pub static RECORD_Q467_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q468_0"]
    pub static RECORD_Q468_0: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q468_1"]
    pub static RECORD_Q468_1: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q468_2"]
    pub static RECORD_Q468_2: RuntimeLayout;
    /// Declared layout of a checked fixed record.
    #[link_name = "perry_runtime_layout_RECORD_Q468_3"]
    pub static RECORD_Q468_3: RuntimeLayout;
}

/// Allocate the declared record with every property present.
///
/// # Safety
/// `desc` names one of this binary's registered runtime declarations.
pub unsafe fn alloc_runtime_record(desc: *const RuntimeLayout) -> *mut ObjectHeader {
    perry_alloc_runtime_record(desc)
}

/// Initialize a declared record slot after a value expression's allocations.
///
/// # Safety
/// `object` is a raw-address root for a fresh fixed record with this slot.
pub unsafe fn init_runtime_field(object: &TransientRootedAddr, slot: u32, value: JsValue) {
    crate::js_object_set_field(object.get() as *mut ObjectHeader, slot, value);
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn static_layout_ffi_record_imports_have_present_undefined_slots() {
        crate::_perry_runtime_link::gc::initialize_runtime_fixture();
        let roots = crate::TransientRootScope::enter();
        let object = roots
            .root_addr(unsafe { alloc_runtime_record(std::ptr::addr_of!(RECORD_F21_0)) } as i64);
        for slot in 0..5 {
            assert_eq!(
                unsafe { crate::js_object_get_field(object.get() as *const ObjectHeader, slot) }
                    .bits(),
                crate::JsValue::UNDEFINED.bits()
            );
        }
        unsafe {
            init_runtime_field(&object, 0, crate::JsValue::from_number(17.0));
        }
        assert_eq!(
            unsafe { crate::js_object_get_field(object.get() as *const ObjectHeader, 0) }.bits(),
            17.0f64.to_bits()
        );
    }
}

/// The shared native UI image-result record and resolved promise.
pub fn image_result(handle: i64, width: f64, height: f64) -> i64 {
    unsafe { perry_runtime_image_result(handle, width, height) }
}
