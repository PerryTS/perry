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
}

/// A runtime layout's descriptor, emitted by the compiler into each program
/// whose realm reaches it (`perry_runtime_layout_<NAME>`). The reference is
/// weak: a wrapper linked into a program whose realm lacks the layout links,
/// and a birth there aborts naming the layout.
macro_rules! runtime_layout_refs {
    ($($name:ident),* $(,)?) => {$(
        #[doc = concat!("The runtime layout `", stringify!($name), "`.")]
        #[allow(non_snake_case)]
        pub mod $name {
            unsafe extern "C" {
                #[linkage = "extern_weak"]
                #[link_name = concat!("perry_runtime_layout_", stringify!($name))]
                static DEFINITION: *const super::RuntimeLayout;
            }
            // ELF: the reference is program-internal. A hidden undefined weak
            // symbol resolves at link time, to the program's descriptor or to
            // null: no dynamic symbol, no load-time lookup in ld.so, and no
            // shared object can interpose a descriptor. (Default visibility
            // made each unresolved reference an R_X86_64_GLOB_DAT relocation
            // that ld.so looked up by name at every startup.)
            #[cfg(all(unix, not(target_vendor = "apple")))]
            core::arch::global_asm!(concat!(
                ".weak perry_runtime_layout_", stringify!($name),
                "\n.hidden perry_runtime_layout_", stringify!($name)
            ));
            /// The descriptor of this layout for a birth.
            #[inline(always)]
            pub fn desc() -> *const super::RuntimeLayout {
                // SAFETY: an extern_weak static reads as the symbol's
                // address, or null when the program does not define it.
                let desc = unsafe { DEFINITION };
                if desc.is_null() {
                    super::undefined_runtime_layout(stringify!($name));
                }
                desc
            }
        }
    )*};
}

#[cold]
#[inline(never)]
fn undefined_runtime_layout(name: &'static str) -> ! {
    eprintln!("Perry internal error: runtime layout {name} is born, but the program's realm does not reach it");
    std::process::abort()
}

runtime_layout_refs!(
    RECORD_V026,
    RECORD_F0_0,
    RECORD_F3_0,
    RECORD_F6_0,
    RECORD_F7_0,
    RECORD_F13_0,
    RECORD_F17_0,
    RECORD_F20_0,
    RECORD_F21_0,
    RECORD_F28_0,
    RECORD_F29_0,
    RECORD_F30_0,
    RECORD_F31_0,
    RECORD_F37_0,
    RECORD_F327_0,
    RECORD_F328_0,
    RECORD_F411_0,
    RECORD_F412_0,
    RECORD_F413_0,
    RECORD_F414_0,
    RECORD_F418_0,
    RECORD_F426_0,
    RECORD_F427_0,
    RECORD_F429_0,
    RECORD_F430_0,
    RECORD_F434_0,
    RECORD_F470_0,
    RECORD_F471_0,
    RECORD_F473_0,
    RECORD_F483_0,
    RECORD_F487_0,
    RECORD_F568_0,
    RECORD_PCHECKED031,
    RECORD_U610,
    RECORD_U613,
    RECORD_Q435_0,
    RECORD_Q436_0,
    RECORD_Q439_0,
    RECORD_Q440_0,
    RECORD_Q441_0,
    RECORD_Q443_0,
    RECORD_Q448_0,
    RECORD_Q449_0,
    RECORD_Q450_0,
    RECORD_Q451_0,
    RECORD_Q452_0,
    RECORD_Q453_0,
    RECORD_Q454_0,
    RECORD_Q455_0,
    RECORD_Q456_0,
    RECORD_Q457_0,
    RECORD_Q458_0,
    RECORD_Q459_0,
    RECORD_Q460_0,
    RECORD_Q461_0,
    RECORD_Q462_0,
    RECORD_Q463_0,
    RECORD_Q464_0,
    RECORD_Q465_0,
    RECORD_Q466_0,
    RECORD_Q467_0,
    RECORD_Q468_0,
    RECORD_Q468_1,
    RECORD_Q468_2,
    RECORD_Q468_3,
);

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
        let object = roots.root_addr(unsafe { alloc_runtime_record(RECORD_F21_0::desc()) } as i64);
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
