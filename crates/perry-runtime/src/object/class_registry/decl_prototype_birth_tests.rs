//! A declared class's prototype is born in its final shape, linked from the
//! class's function object, and found again from either end in O(1).

use super::*;
use crate::codegen_abi::{LayoutWord, ShapeId};

extern "C" fn entry_a(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

extern "C" fn entry_b(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    2.0
}

extern "C" fn method_body(_this: f64) -> f64 {
    0.0
}

fn register(cid: u32, name: &[u8]) {
    {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }
    unsafe {
        crate::object::js_register_class_name(cid, name.as_ptr(), name.len() as u32);
        crate::object::js_register_class_length(cid, 0);
    }
}

unsafe fn register_method(cid: u32, name: &[u8], entry: Option<usize>) {
    super::js_register_class_method_with_entry(
        cid as i64,
        name.as_ptr(),
        name.len() as i64,
        method_body as *const () as usize as i64,
        0,
        0,
        0,
        entry.unwrap_or(0) as i64,
    );
}

fn infos() -> (usize, usize) {
    (
        crate::fn_info!(entry_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
            as *const crate::closure::JsFunctionInfo as usize,
        crate::fn_info!(entry_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
            as *const crate::closure::JsFunctionInfo as usize,
    )
}

fn born_final_builds() -> u32 {
    super::decl_prototype_birth::BORN_FINAL_BUILDS.with(std::cell::Cell::get)
}

fn object_of(value: f64) -> *mut ObjectHeader {
    let v = crate::value::JSValue::from_bits(value.to_bits());
    assert!(v.is_pointer(), "a prototype object");
    v.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

unsafe fn slot(obj: *const ObjectHeader, i: usize) -> u64 {
    std::ptr::read(
        ((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64).add(i),
    )
}

#[test]
fn a_declared_prototype_is_born_in_its_final_linked_shape() {
    let cid = 0x6E41;
    register(cid, b"Born");
    let (a, b) = infos();
    unsafe {
        register_method(cid, b"a", Some(a));
        register_method(cid, b"b", Some(b));
    }
    let before = born_final_builds();
    let proto = object_of(class_decl_prototype_value(cid));
    assert_eq!(
        born_final_builds(),
        before + 1,
        "the born-final path built it"
    );

    // Linked from the class, found from the object, built once.
    assert_eq!(class_decl_prototype_object(cid), proto);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
    assert_eq!(object_of(class_decl_prototype_value(cid)), proto);
    assert_eq!(
        born_final_builds(),
        before + 1,
        "a second demand builds nothing"
    );

    unsafe {
        let d = crate::object::shapes::object_shape_descriptor(proto).expect("shaped");
        assert_eq!(d.logical_key_count, 3);
        assert_eq!(d.live_inline_slot_count, 3);
        assert_eq!(d.hole_count, 0);
        let keys = d.keys_view();
        for (pos, name) in [b"constructor".as_slice(), b"a", b"b"].iter().enumerate() {
            assert!(
                crate::string::js_string_key_matches_bytes(keys.get(pos as u32), name),
                "key {pos} in ClassBody order"
            );
            assert_eq!(
                crate::object::key_attrs::keys_entry(
                    d.keys as usize as *const crate::ArrayHeader,
                    pos as u32
                ),
                crate::object::key_attrs::ENTRY_NON_ENUMERABLE,
                "key {pos} is {{ writable, !enumerable, configurable }}"
            );
        }
        // Slot 0 is the class itself; each method slot names its own body.
        assert_eq!(
            slot(proto, 0),
            crate::object::class_constructor_ref_value(cid).to_bits()
        );
        let record = crate::object::shapes::shape_record_for_word(
            crate::object::shapes::object_shape_stamp(proto),
        )
        .expect("record");
        assert_eq!(record.constfn_info(0), None);
        assert_eq!(record.constfn_info(1), Some(a as u64));
        assert_eq!(record.constfn_info(2), Some(b as u64));
        assert_eq!(
            slot(proto, 1),
            crate::object::class_prototype_method_value_for_name(cid, "a").to_bits()
        );
        // Linked to Object.prototype through the shape's identity.
        assert_eq!(
            crate::object::shapes::object_prototype_word(proto),
            crate::object::object_prototype_intrinsic_bits().expect("Object.prototype")
        );
        assert_eq!(
            crate::object::shapes::object_proto_id(proto),
            d.proto_id,
            "the shape names the link the object records"
        );
    }
}

#[test]
fn a_subclass_prototype_links_its_parents() {
    let (pa, pb) = (0x6E42, 0x6E43);
    register(pa, b"Base");
    register(pb, b"Derived");
    let (a, b) = infos();
    unsafe {
        register_method(pa, b"a", Some(a));
        register_method(pb, b"b", Some(b));
        crate::object::js_register_class_parent(pb, pa);
    }
    let before = born_final_builds();
    let derived = object_of(class_decl_prototype_value(pb));
    assert_eq!(
        born_final_builds(),
        before + 2,
        "both born final, parent first"
    );
    let base = class_decl_prototype_object(pa);
    assert!(!base.is_null());
    unsafe {
        assert_eq!(
            crate::object::shapes::object_prototype_word(derived),
            crate::value::js_nanbox_pointer(base as i64).to_bits()
        );
    }
}

#[test]
fn a_prototype_the_registry_does_not_fully_describe_takes_the_general_path() {
    let cid = 0x6E44;
    register(cid, b"Mixed");
    let (a, _) = infos();
    unsafe {
        register_method(cid, b"a", Some(a));
        // No entry: the name trampoline, which no shape lane can name.
        register_method(cid, b"old", None);
    }
    let before = born_final_builds();
    let proto = object_of(class_decl_prototype_value(cid));
    assert_eq!(born_final_builds(), before, "the general path built it");
    assert_eq!(class_decl_prototype_object(cid), proto);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
}

#[test]
fn a_general_prototype_birth_keeps_guards_until_ordinary_mutation() {
    let cid = 0x6E47;
    register(cid, b"GeneralGuard");
    let (a, _) = infos();
    unsafe {
        register_method(cid, b"a", Some(a));
        register_method(cid, b"old", None);
    }
    let before = born_final_builds();
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(object_of(class_decl_prototype_value(cid)));
    assert_eq!(born_final_builds(), before, "the general path built it");
    let pristine =
        proto.with_const_ptr(|p| unsafe { crate::object::shapes::object_shape_stamp(p) });
    let key = crate::string::js_string_from_bytes(b"a".as_ptr(), 1);
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::js_object_set_field_by_name(p, key, 3.0)
    });
    assert_ne!(
        pristine,
        proto.with_const_ptr(|p| unsafe { crate::object::shapes::object_shape_stamp(p) }),
        "the holder shape must retire on an ordinary overwrite"
    );
}

#[test]
fn the_reverse_lookup_answers_only_the_linked_object() {
    let cid = 0x6E45;
    register(cid, b"Rev");
    let (a, _) = infos();
    unsafe { register_method(cid, b"a", Some(a)) };
    let proto = object_of(class_decl_prototype_value(cid));
    // An instance carries the same class id but is not the linked object.
    let instance = crate::object::js_object_alloc(cid, 1);
    assert_eq!(class_id_for_decl_prototype_object(instance as usize), None);
    assert_eq!(class_id_for_decl_prototype_object(0), None);
    assert_eq!(class_id_for_decl_prototype_object(0x10), None);
    // A class function object is not an ordinary object.
    let class = crate::object::class_value::class_value_ptr(cid) as usize;
    assert_eq!(class_id_for_decl_prototype_object(class), None);
    assert_eq!(
        class_id_for_decl_prototype_object(proto as usize),
        Some(cid)
    );
}

#[test]
fn static_declaration_holders_state_their_real_parent_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let (base, child) = (0x6E4B, 0x6E4C);
    register(base, b"StaticParentBase");
    register(child, b"StaticParentChild");
    crate::object::js_register_class_parent(child, base);
    // Complete holder content, including its private declaration identity.
    super::registration::js_register_class_prototype_shape(
        base,
        crate::object::shapes::SHAPE_ID_BASE + 991,
    );
    super::registration::js_register_class_prototype_shape(
        child,
        crate::object::shapes::SHAPE_ID_BASE + 992,
    );
    let p = object_of(class_decl_prototype_value(child));
    let b = class_decl_prototype_object(base);
    for obj in [p, b] {
        unsafe {
            let descriptor = crate::object::shapes::object_shape_descriptor(obj).unwrap();
            assert_eq!(
                crate::object::shapes::object_proto_id(obj),
                descriptor.proto_id
            );
        }
    }
    let other = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_prototype_of(
        crate::value::js_nanbox_pointer(p as i64),
        crate::value::js_nanbox_pointer(other as i64),
    );
    unsafe {
        let descriptor = crate::object::shapes::object_shape_descriptor(p).unwrap();
        assert_ne!(
            descriptor.proto_id,
            crate::object::shapes::PROTO_ID_CLASS | u64::from(base)
        );
        assert_eq!(
            crate::object::shapes::object_proto_id(p),
            descriptor.proto_id
        );
    }
}

// The linker descriptor is the compiler's immutable candidate, not the
// runtime parent edge. A changed edge and another evaluation of a template
// must each independently revoke the requested prototype shape.
const CHECKED_CHILD: u32 = 60_281;
const CHECKED_PARENT: u32 = 60_282;
static EMPTY_KEYS: [crate::object::layout_desc::PerryLayoutKey; 0] = [];
static EMPTY_INDEX: [u32; 1] = [u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
static mut CHECKED_LAYOUT: crate::object::layout_desc::PerryLayoutDesc =
    crate::object::layout_desc::PerryLayoutDesc {
        image_keys: std::ptr::null_mut(),
        shape_id: 0,
        gc_plan: 0,
        family: 0x41574202,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 0,
        layout_id: 0x80000086,
        rep: 0,
        proto: std::ptr::null(),
        keys: EMPTY_KEYS.as_ptr(),
        key_count: 0,
        index_mask: 0,
        index: EMPTY_INDEX.as_ptr(),
        class_id: CHECKED_CHILD,
        parent_class_id: CHECKED_PARENT,
        methods: std::ptr::null(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        slots_first: crate::object::layout_desc::layout_slots_first(),
        _tail_pad: [0; 30],
    };
static CHECKED_DECLARATION: super::declarations::ClassDeclaration =
    super::declarations::ClassDeclaration {
        instance_layout: std::ptr::addr_of!(CHECKED_LAYOUT),
        ..super::declarations::ClassDeclaration::native(&[])
    };

#[test]
fn static_prototype_requires_the_candidate_parent_and_its_canonical_holder() {
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    register(CHECKED_PARENT, b"CheckedParent");
    register(CHECKED_CHILD, b"CheckedChild");
    unsafe {
        super::declarations::js_register_class_declaration(CHECKED_CHILD, &CHECKED_DECLARATION)
    };
    crate::object::js_register_class_parent(CHECKED_CHILD, CHECKED_PARENT);
    let canonical = scope.root_nanbox_f64(class_decl_prototype_value(CHECKED_PARENT));
    // No guard needed a seed: the family still supplies the exact candidate.
    let requested = ShapeId(crate::object::shapes::SHAPE_ID_BASE + 1991);
    assert!(crate::object::shapes::shape_record_by_id(requested).is_none());
    let matches =
        |bits| super::decl_prototype_birth::static_parent_matches(CHECKED_CHILD, requested, bits);
    assert!(
        matches(canonical.get_nanbox_f64().to_bits()),
        "the admitted case is live"
    );
    let other =
        scope.root_raw_mut_ptr(crate::object::object_alloc_prototype_old(CHECKED_PARENT, 0));
    let other_bits = other
        .with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64).to_bits());
    assert!(
        !matches(other_bits),
        "same template id, different parent prototype"
    );
    assert!(
        !matches(crate::value::TAG_NULL),
        "extends null is a different edge"
    );
    crate::object::class_registry::register_class(CHECKED_CHILD, CHECKED_PARENT + 1);
    assert!(
        !matches(canonical.get_nanbox_f64().to_bits()),
        "changed actual parent, original canonical holder"
    );
    crate::object::class_registry::register_class(CHECKED_CHILD, CHECKED_PARENT);
    assert!(
        matches(canonical.get_nanbox_f64().to_bits()),
        "restoring the edge restores the proof"
    );
    use crate::object::shapes;
    let seed = |id, parent| {
        shapes::shape_descriptor_ensure_with_holes(
            std::ptr::null(),
            0,
            3,
            (1u64 << 63) | u64::from(CHECKED_CHILD),
            shapes::ShapeObjectKind::Ordinary,
            0,
            shapes::PROTO_ID_CLASS | u64::from(parent),
            shapes::ReceiverFacts::NONE,
            Some(id),
        )
        .unwrap()
    };
    assert_eq!(
        shapes::shape_record_for_word(seed(requested, CHECKED_PARENT)).map(|r| r.identity()),
        Some(requested)
    );
    assert!(
        matches(canonical.get_nanbox_f64().to_bits()),
        "a seeded candidate also admits"
    );
    let wrong = ShapeId(requested.0 + 1);
    assert_eq!(
        shapes::shape_record_for_word(seed(wrong, CHECKED_PARENT + 1)).map(|r| r.identity()),
        Some(wrong)
    );
    assert!(
        !super::decl_prototype_birth::static_parent_matches(
            CHECKED_CHILD,
            wrong,
            canonical.get_nanbox_f64().to_bits()
        ),
        "a seed must agree with the family's candidate parent"
    );
}

#[test]
fn static_root_parent_uses_the_realm_intrinsic_identity() {
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    use crate::object::shapes;
    let cid = 0x6e90;
    register(cid, b"CheckedRoot");
    let requested = ShapeId(shapes::SHAPE_ID_BASE + 1995);
    shapes::shape_descriptor_ensure_with_holes(
        std::ptr::null(),
        0,
        3,
        (1u64 << 63) | u64::from(cid),
        shapes::ShapeObjectKind::Ordinary,
        0,
        shapes::PROTO_ID_DEFAULT,
        shapes::ReceiverFacts::NONE,
        Some(requested),
    )
    .unwrap();
    let canonical = super::global_object_prototype_bits().unwrap();
    assert!(super::decl_prototype_birth::static_parent_matches(
        cid, requested, canonical
    ));
    assert!(!super::decl_prototype_birth::static_parent_matches(
        cid,
        requested,
        crate::value::TAG_NULL,
    ));
    let other = crate::object::object_alloc_prototype_old(cid, 0);
    assert!(!super::decl_prototype_birth::static_parent_matches(
        cid,
        requested,
        crate::value::js_nanbox_pointer(other as i64).to_bits(),
    ));
}

#[test]
fn a_refused_static_request_keeps_the_general_born_final_shape() {
    crate::gc::initialize_runtime_fixture();
    let _lock = crate::gc::global_side_table_test_lock();
    let cid = 0x6e91;
    register(cid, b"RefusedStaticRequest");
    let requested = ShapeId(crate::object::shapes::SHAPE_ID_BASE + 1996);
    assert!(crate::object::shapes::shape_record_by_id(requested).is_none());
    let declaration = Box::leak(Box::new(super::declarations::ClassDeclaration {
        birth_shape: requested,
        birth_layout_word: LayoutWord(crate::object::static_shapes::fixtures::fixture_word(
            requested.0,
        )),
        ..super::declarations::ClassDeclaration::native(&[])
    }));
    unsafe {
        super::declarations::js_register_class_declaration(cid, declaration);
    }
    assert!(!super::decl_prototype_birth::static_parent_matches(
        cid,
        requested,
        crate::value::TAG_NULL,
    ));
    // The builder's caller has already minted the class function object.
    let _ = class_constructor_ref_value(cid);
    let value = super::decl_prototype_birth::decl_prototype_born_final(cid, crate::value::TAG_NULL)
        .expect("the actual-parent shape needs no compiled identity");
    let obj = object_of(value);
    unsafe {
        assert_ne!((*obj).parent_class_id, requested.0);
        assert_eq!(
            crate::object::shapes::object_proto_id(obj),
            crate::object::shapes::PROTO_ID_NULL
        );
        assert_eq!(slot(obj, 0), class_constructor_ref_value(cid).to_bits());
    }
}
