//! Checked family birth for a class whose heritage evaluates at runtime.

#[cfg(test)]
use super::layout_desc::{family_desc_of, is_family_object};
use super::layout_desc::{family_of_class, perry_alloc_layout, PerryLayoutDesc};
use super::ObjectHeader;
#[cfg(test)]
use crate::codegen_abi::LayoutWord;

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
        not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
        link_section = "perry_layouts"
    )]
    static mut LAYOUT: PerryLayoutDesc = PerryLayoutDesc {
        image_keys: std::ptr::null_mut(),
        shape_id: 0,
        gc_plan: 0,
        family: 0x41574201,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 0,
        layout_id: 0x80000085,
        rep: 0,
        proto: std::ptr::null(),
        keys: EMPTY_KEYS.as_ptr(),
        key_count: 0,
        index_mask: 0,
        index: EMPTY_INDEX.as_ptr(),
        class_id: CHILD,
        parent_class_id: PARENT,
        methods: std::ptr::null(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        _tail_pad: [0; 32],
    };
    wasm_layout!(LAYOUT);

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

#[cfg(test)]
mod evaluation_layout_tests {
    use super::*;
    use crate::object::class_registry::declarations::{
        js_register_class_declaration, ClassDeclaration,
    };
    use crate::object::shapes;

    const CLASS: u32 = 60_281;

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
        image_keys: std::ptr::null_mut(),
        shape_id: 0,
        gc_plan: 0,
        family: 0x41574202,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 0,
        layout_id: 0x80000084,
        rep: 0,
        proto: std::ptr::null(),
        keys: EMPTY_KEYS.as_ptr(),
        key_count: 0,
        index_mask: 0,
        index: EMPTY_INDEX.as_ptr(),
        class_id: CLASS,
        parent_class_id: 0,
        methods: std::ptr::null(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        _tail_pad: [0; 32],
    };

    static DECLARATION: ClassDeclaration = ClassDeclaration {
        instance_layout: std::ptr::addr_of!(LAYOUT),
        ..ClassDeclaration::native(&[])
    };

    /// An instance linked to a class evaluation's prototype moves to that
    /// evaluation's copy of its family layout: a shape naming the link
    /// (`MIXED | class | serial`), one per evaluation, that is still the
    /// family's layout for every geometry reader.
    #[test]
    fn an_evaluation_link_moves_a_family_instance_to_the_evaluation_layout() {
        crate::gc::initialize_runtime_fixture();
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe { js_register_class_declaration(CLASS, &DECLARATION) };
        let desc = std::ptr::addr_of!(LAYOUT);
        let link = |proto: *mut ObjectHeader| unsafe {
            let obj = perry_alloc_class_family(desc, 0, 0);
            assert_eq!((*obj).parent_class_id, (*desc).layout_id);
            let bits = crate::value::js_nanbox_pointer(proto as i64).to_bits();
            crate::object::prototype_chain::object_link_class_evaluation_prototype(
                obj as usize,
                bits,
            );
            (obj, bits)
        };
        let first = crate::object::js_object_alloc(0, 0);
        let second = crate::object::js_object_alloc(0, 0);
        let (a, a_bits) = link(first);
        let (b, _) = link(first);
        let (c, c_bits) = link(second);
        unsafe {
            for (obj, bits) in [(a, a_bits), (b, a_bits), (c, c_bits)] {
                let word = (*obj).parent_class_id;
                assert_ne!(
                    word,
                    (*desc).layout_id,
                    "the link leaves the class identity"
                );
                assert!(shapes::shape_word_may_be_linked(word));
                assert!(shapes::proto_id_links_class_instance(
                    shapes::object_proto_identity(obj),
                    shapes::class_proto_id(CLASS),
                ));
                assert!(
                    crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)).is_null(),
                    "the link is a shape fact, not a meta one"
                );
                assert_eq!(shapes::object_prototype_word(obj), bits);
                assert!(shapes::shape_is_evaluation_layout_of(
                    LayoutWord(word),
                    LayoutWord((*desc).layout_id)
                ));
                assert!(is_family_object(obj));
                assert_eq!(
                    family_desc_of(obj).map(|d| d as *const PerryLayoutDesc),
                    Some(desc)
                );
            }
            // One layout per evaluation: the same prototype reaches the same
            // record, another prototype its own.
            assert_eq!((*a).parent_class_id, (*b).parent_class_id);
            assert_ne!((*a).parent_class_id, (*c).parent_class_id);
            // A receiver whose geometry is not the family's is no family
            // object, whatever its identity.
            let wide = crate::object::js_object_alloc(CLASS, 3);
            crate::object::prototype_chain::object_link_class_evaluation_prototype(
                wide as usize,
                a_bits,
            );
            assert!(!shape_is_evaluation_layout_of_word(
                wide,
                LayoutWord((*desc).layout_id)
            ));
            assert!(!is_family_object(wide));
        }
    }

    /// The evaluation brand of an instance on an evaluation copy is a shape
    /// fact (no meta record), the branded shape is still the copy, and a
    /// birth straight onto it is the same object the link and stamp make.
    #[test]
    fn an_evaluation_copy_carries_its_brand_and_takes_births_directly() {
        crate::gc::initialize_runtime_fixture();
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe { js_register_class_declaration(CLASS, &DECLARATION) };
        let desc = std::ptr::addr_of!(LAYOUT);
        let proto = crate::object::js_object_alloc(0, 0);
        let proto_bits = crate::value::js_nanbox_pointer(proto as i64).to_bits();
        let class = crate::object::js_object_alloc(CLASS + 1, 0);
        crate::object::js_object_mark_class(class as i64);
        let class_bits = crate::value::js_nanbox_pointer(class as i64).to_bits();
        unsafe {
            let obj = perry_alloc_class_family(desc, 0, 0);
            crate::object::prototype_chain::object_link_class_evaluation_prototype(
                obj as usize,
                proto_bits,
            );
            shapes::stamp_evaluation_holder(obj, class_bits);
            let copy = shapes::object_shape_stamp(obj);
            assert!(
                crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)).is_null(),
                "the brand is a shape fact"
            );
            assert_eq!(shapes::object_evaluation_brand(obj), class_bits);
            assert_eq!(shapes::object_prototype_word(obj), proto_bits);
            assert_eq!(
                crate::object::layout_desc::evaluation_layout_desc(
                    shapes::evaluation_family_desc_index_plus_one(copy)
                )
                .map(|d| d.layout_id),
                Some((*desc).layout_id)
            );
            assert!(is_family_object(obj));
            assert_eq!(
                crate::object::layout_desc::family_own_key(obj, b"not_in_family"),
                Some(None),
                "the evaluation receiver consumes its checked family proof"
            );
            assert_eq!(
                shapes::evaluation_copy_birth_edges(copy, (*desc).layout_id),
                Some((proto_bits, class_bits))
            );
            assert_eq!(
                shapes::evaluation_copy_birth_edges(copy, (*desc).layout_id + 1),
                None
            );
            let born = crate::object::layout_desc::alloc_family_layout_copy(desc, copy);
            assert_eq!((*born).parent_class_id, (*obj).parent_class_id);
            assert!(crate::meta_read!(crate::meta_obj_runtime_born!(born as *mut _)).is_null());
            assert_eq!(shapes::object_evaluation_brand(born), class_bits);
            assert_eq!(shapes::object_prototype_word(born), proto_bits);
            assert_eq!(
                family_desc_of(born).map(|d| d as *const PerryLayoutDesc),
                Some(desc)
            );
            // With heritage, the class object pins its parent; the heritage
            // pin of an instance on the copy is its shape brand, so pinning
            // it allocates no metadata record.
            use crate::object::class_registry::evaluation_heritage as heritage;
            use crate::object::class_registry::parent_static::CLASS_OBJECT_PARENT_KEY;
            heritage::CLASS_OBJECT_HERITAGE_PIN_LATCH.arm();
            let key = crate::string::js_string_from_bytes(
                CLASS_OBJECT_PARENT_KEY.as_ptr(),
                CLASS_OBJECT_PARENT_KEY.len() as u32,
            );
            crate::object::define_builtin_data_property(
                class,
                key,
                f64::from_bits(proto_bits),
                CLASS_OBJECT_PARENT_KEY.to_string(),
                crate::object::PropertyAttrs::new(true, true, true),
            );
            assert!(crate::object::class_registry::class_object_pinned_parent(class).is_some());
            assert_eq!(
                heritage::instance_pinned_constructing_class(born).map(f64::to_bits),
                Some(class_bits)
            );
            heritage::pin_instance_constructing_class(born, f64::from_bits(class_bits));
            assert!(
                crate::meta_read!(crate::meta_obj_runtime_born!(born as *mut _)).is_null(),
                "the pin is the shape brand"
            );
            assert_eq!(
                heritage::instance_pinned_constructing_class(born).map(f64::to_bits),
                Some(class_bits)
            );
        }
    }

    unsafe fn shape_is_evaluation_layout_of_word(
        obj: *const ObjectHeader,
        family: LayoutWord,
    ) -> bool {
        shapes::shape_is_evaluation_layout_of(LayoutWord((*obj).parent_class_id), family)
    }
}
