//! Checked family birth for a class whose heritage evaluates at runtime.

use super::layout_desc::{family_of_class, perry_alloc_layout, PerryLayoutDesc};
use super::ObjectHeader;

/// Allocate on the compiler's proposed family only while the class declaration
/// and its runtime parent agree. A miss uses ordinary class allocation.
///
/// # Safety
/// `desc` addresses this program's static class-family descriptor.
#[no_mangle]
pub unsafe extern "C" fn perry_alloc_class_family(
    desc: *const PerryLayoutDesc,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    let class_id = (*desc).class_id;
    if family_of_class(class_id).is_some_and(|(_, actual)| actual == desc) {
        return perry_alloc_layout(desc);
    }
    super::js_object_alloc_with_parent(class_id, parent_class_id, field_count)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_ALLOC_CLASS_FAMILY: unsafe extern "C" fn(
    *const PerryLayoutDesc,
    u32,
    u32,
) -> *mut ObjectHeader = perry_alloc_class_family;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::class_registry::declarations::{
        js_register_class_declaration, ClassDeclaration,
    };

    const CHILD: u32 = 60_271;
    const PARENT: u32 = 60_272;

    static EMPTY_KEYS: [crate::object::layout_desc::PerryLayoutKey; 0] = [];
    static EMPTY_INDEX: [u32; 1] = [u32::MAX];

    #[used(linker)]
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
    #[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
    #[cfg_attr(
        not(any(target_vendor = "apple", target_os = "windows")),
        link_section = "perry_layouts"
    )]
    static mut LAYOUT: PerryLayoutDesc = PerryLayoutDesc {
        family: 0x41574201,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 0,
        layout_id: crate::object::layout_desc::LAYOUT_ID_CARRIED,
        rep: 0,
        proto: std::ptr::null(),
        keys: EMPTY_KEYS.as_ptr(),
        key_count: 0,
        index_mask: 0,
        index: EMPTY_INDEX.as_ptr(),
        class_id: CHILD,
        parent_class_id: PARENT,
        methods: std::ptr::null(),
    };

    static DECLARATION: ClassDeclaration = ClassDeclaration {
        instance_layout: std::ptr::addr_of!(LAYOUT),
        ..ClassDeclaration::native(&[])
    };

    #[test]
    fn checked_birth_requires_the_declaration_and_runtime_parent() {
        crate::gc::initialize_runtime_fixture();
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let desc = std::ptr::addr_of!(LAYOUT);
        let fallback = scope.root_raw_mut_ptr(unsafe { perry_alloc_class_family(desc, 0, 3) });
        fallback.with_const_ptr::<ObjectHeader, _>(|obj| unsafe {
            assert_eq!((*obj).class_id, CHILD);
            assert!(crate::object::object_inline_capacity(obj) >= 3);
        });
        unsafe { js_register_class_declaration(CHILD, &DECLARATION) };
        crate::object::class_registry::register_class(CHILD, PARENT);
        let family = scope.root_raw_mut_ptr(unsafe { perry_alloc_class_family(desc, 0, 3) });
        family.with_const_ptr::<ObjectHeader, _>(|obj| unsafe {
            assert_eq!((*obj).parent_class_id, (*desc).layout_id);
            assert_eq!(crate::object::object_inline_capacity(obj), 0);
        });
        crate::object::class_registry::register_class(CHILD, PARENT + 1);
        let fallback = scope.root_raw_mut_ptr(unsafe { perry_alloc_class_family(desc, 0, 3) });
        fallback.with_const_ptr::<ObjectHeader, _>(|obj| unsafe {
            assert_eq!((*obj).class_id, CHILD);
            assert_ne!((*obj).parent_class_id, (*desc).layout_id);
            assert!(crate::object::object_inline_capacity(obj) >= 3);
        });
    }
}
