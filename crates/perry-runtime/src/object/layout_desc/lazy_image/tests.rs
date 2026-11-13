use super::*;
use crate::codegen_abi::{
    LayoutRow, PerryLayoutImageV1, PerryProgramTableV1, ShapeObjectKind, NATIVE_LAYOUT_ROWS,
};

// Existing unit fixtures own rows below 4096; this image has a separate span.
const FIRST: u32 = 4096;
static mut FUNCTION_FAMILY: PerryProtoFamily = PerryProtoFamily {
    serial: 1,
    image_prototype: std::ptr::null(),
};
const fn row(index: u32, kind: ShapeObjectKind, proto: *const PerryProtoFamily) -> PerryLayoutDesc {
    let (band, proto_kind) = if index == 2 {
        (2, 1)
    } else if index == 1 {
        (0, 2)
    } else {
        (0, 0)
    };
    PerryLayoutDesc {
        family: 0,
        variant: 0,
        kind: kind as u8,
        integrity: 0,
        slot_count: 0,
        layout_id: LayoutWord::new(LayoutRow(FIRST + index), band, proto_kind)
            .unwrap()
            .0,
        rep: 0,
        proto,
        keys: std::ptr::null(),
        key_count: 0,
        index_mask: 0,
        index: std::ptr::null(),
        class_id: 0,
        parent_class_id: 0,
        methods: std::ptr::null(),
        shape_id: 0,
        gc_plan: 0,
        image_keys: std::ptr::null_mut(),
        gc_header_word: crate::object::layout_desc::layout_gc_header_word(0),
        realm_family: match index {
            0 => crate::object::BuiltinConstructorIdentity::Map as u8,
            1 => crate::object::BuiltinConstructorIdentity::Date as u8,
            _ => crate::object::BuiltinConstructorIdentity::Function as u8,
        },
        _tail_pad: [0; 31],
    }
}
static mut FACTS: [PerryLayoutDesc; 3] = [
    row(0, ShapeObjectKind::Ordinary, LAYOUT_PROTO_OBJECT),
    row(1, ShapeObjectKind::Ordinary, LAYOUT_PROTO_NULL),
    row(
        2,
        ShapeObjectKind::Function,
        std::ptr::addr_of!(FUNCTION_FAMILY),
    ),
];

#[test]
fn lazy_row_publishes_once_per_agent_without_a_descriptor_scan() {
    let run = || {
        std::thread::spawn(|| unsafe {
            crate::object::class_image::enter_current_thread_image();
            let facts = std::ptr::addr_of!(FACTS).cast::<PerryLayoutDesc>();
            let program = Box::leak(Box::new(PerryProgramTableV1 {
                lazy_first: FIRST,
                lazy_count: 3,
                lazy_layouts: facts.cast(),
                ..PerryProgramTableV1::EMPTY
            }));
            // The unit fixtures' families follow the native rows; the span
            // follows them.
            crate::object::class_image::bind_layout_image(PerryLayoutImageV1 {
                static_rows: FIRST + 3,
                family_count: FIRST - NATIVE_LAYOUT_ROWS,
                program,
                ..PerryLayoutImageV1::RUNTIME
            });
            crate::test_support::bootstrap_test_thread();
            let mut identities = Vec::new();
            for index in 0..3 {
                let desc = facts.add(index);
                let word = LayoutWord((*desc).layout_id);
                // Held-word reads and the startup registrar cannot mint image rows.
                assert!(crate::object::shapes::shape_record_for_word(word).is_none());
                register_static_layouts();
                assert!(crate::object::shapes::shape_record_for_word(word).is_none());
                assert!(contains(desc));
                assert_eq!(descriptor(word.0), Some(desc));
                // Receiver reads keep main's single-compare path; the row's
                // first touch publishes it through the normal registrar.
                publish_receiver(word.0);
                publish_receiver(word.0);
                let first = crate::object::shapes::shape_descriptor_of_receiver(word.0).unwrap();
                let again = crate::object::shapes::shape_descriptor_of_receiver(word.0).unwrap();
                assert_eq!(first, again);
                assert_eq!(first.object_kind.code(), u64::from((*desc).kind));
                let record = crate::object::shapes::shape_record_for_word(word).unwrap();
                identities.push(record.identity().0);
                assert_eq!(layout_word_for_birth(desc), Some(word.0));
            }
            assert!(descriptor(LayoutWord::new(LayoutRow(FIRST + 3), 0, 0).unwrap().0).is_none());
            identities
        })
        .join()
        .unwrap()
    };
    assert_eq!(
        run(),
        run(),
        "agents publish the same identities in their own rows"
    );
}

// A pointer-free function cell remains valid under every unrelated GC test.
#[repr(C, align(8))]
struct PrototypeCell {
    gc: crate::gc::GcHeader,
    closure: crate::closure::ClosureHeader,
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_realm"
)]
static mut PROTOTYPE: PrototypeCell = PrototypeCell {
    gc: crate::gc::GcHeader {
        obj_type: crate::gc::GC_TYPE_CLOSURE,
        gc_flags: crate::value::addr_class::realm::IMAGE_CELL_FLAGS,
        _reserved: crate::gc::GC_LAYOUT_POINTER_FREE,
        size: std::mem::size_of::<PrototypeCell>() as u32,
    },
    closure: crate::closure::ClosureHeader {
        capture_count: 0,
        shape_id: LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD),
        info: &crate::value::addr_class::realm::perry_realm_fixture_info,
        props: std::ptr::null_mut(),
    },
};

#[test]
fn image_prototype_binds_the_agent_word_and_preserves_a_worker_binding() {
    std::thread::spawn(|| unsafe {
        crate::test_support::bootstrap_test_thread();
        let pointer = std::ptr::addr_of!(PROTOTYPE.closure).cast();
        assert!(crate::value::addr_class::realm::header(pointer as usize).is_some());
        let family = PerryProtoFamily {
            serial: 58,
            image_prototype: pointer,
        };
        let desc = row(0, ShapeObjectKind::Ordinary, &family);
        let bits = crate::value::JSValue::pointer(pointer).bits();
        assert_eq!(crate::object::shapes::identity_prototype_word(58), 0);
        // Registration reads the serial only; the row's publication binds.
        assert_eq!(proto_family_serial(&family), 58);
        assert_eq!(crate::object::shapes::identity_prototype_word(58), 0);
        crate::object::global_this::family_link::link_named_prototype_first(
            (*desc.proto).serial,
            (*desc.proto).image_prototype as usize,
        );
        assert_eq!(crate::object::shapes::identity_prototype_word(58), bits);
        // Model a worker's already-bound realm pointer with another live
        // function; the template must not overwrite the agent's own object.
        let worker = crate::closure::js_closure_alloc(
            &crate::value::addr_class::realm::perry_realm_fixture_info,
            0,
        );
        let worker_bits = crate::value::JSValue::pointer(worker.cast()).bits();
        crate::object::shapes::write_identity_word(58, worker_bits);
        crate::object::global_this::family_link::link_named_prototype_first(
            (*desc.proto).serial,
            (*desc.proto).image_prototype as usize,
        );
        assert_eq!(
            crate::object::shapes::identity_prototype_word(58),
            worker_bits
        );
    })
    .join()
    .unwrap();
}

#[repr(C, align(8))]
struct PendingCell {
    gc: crate::gc::GcHeader,
    obj: crate::object::ObjectHeader,
}
const fn pending_fixture() -> PendingCell {
    PendingCell {
        gc: crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_OBJECT,
            gc_flags: crate::value::addr_class::realm::IMAGE_CELL_FLAGS,
            _reserved: crate::gc::GC_LAYOUT_POINTER_FREE | crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
            size: std::mem::size_of::<PendingCell>() as u32,
        },
        obj: crate::object::ObjectHeader {
            class_id: 0,
            parent_class_id: 0,
            meta: std::ptr::null_mut(),
        },
    }
}
#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_realm"
)]
static mut PENDING_CELLS: [PendingCell; 2] = [pending_fixture(), pending_fixture()];

#[test]
fn pending_first_touch_publishes_its_row_and_leaves_sibling_pending() {
    std::thread::spawn(|| unsafe {
        use crate::codegen_abi::LAYOUT_ROW_PENDING as PENDING;
        crate::gc::image_cells::hold_image_for_tests(true);
        crate::object::class_image::enter_current_thread_image();
        let facts = std::ptr::addr_of!(FACTS).cast::<PerryLayoutDesc>();
        let program = Box::leak(Box::new(PerryProgramTableV1 {
            lazy_first: FIRST,
            lazy_count: 3,
            lazy_layouts: facts.cast(),
            touch_pending: Some(super::js_realm_touch_pending),
            ..PerryProgramTableV1::EMPTY
        }));
        crate::object::class_image::bind_layout_image(PerryLayoutImageV1 {
            static_rows: FIRST + 3,
            family_count: FIRST - NATIVE_LAYOUT_ROWS,
            program,
            ..PerryLayoutImageV1::RUNTIME
        });
        crate::test_support::bootstrap_test_thread();
        crate::gc::ensure_gc_initialized();
        let word = (*facts).layout_id;
        let sibling_word = (*facts.add(1)).layout_id;
        let first = std::ptr::addr_of_mut!(PENDING_CELLS[0].obj);
        let sibling = std::ptr::addr_of_mut!(PENDING_CELLS[1].obj);
        (*first).parent_class_id = word | PENDING;
        (*sibling).parent_class_id = sibling_word | PENDING;
        assert!(crate::object::shapes::shape_record_for_word(LayoutWord(word)).is_none());
        assert!(crate::object::shapes::object_shape_record_in_gc(first).is_none());
        assert!(crate::object::shapes::shape_record_of_receiver(word | PENDING).is_none());
        assert_eq!((*first).parent_class_id, word | PENDING);
        // The real collector must leave both cells pending and publish no row.
        crate::gc::js_gc_collect();
        assert!(crate::object::shapes::shape_record_for_word(LayoutWord(word)).is_none());
        assert_eq!((*first).parent_class_id, word | PENDING);
        let published =
            crate::object::shapes::object_shape_record(first).expect("first touch publishes");
        assert_eq!((*first).parent_class_id, word);
        assert_eq!((*sibling).parent_class_id, sibling_word | PENDING);
        assert_eq!(
            published.identity(),
            crate::object::shapes::shape_record_for_word(LayoutWord(word))
                .unwrap()
                .identity()
        );
        assert!(
            crate::object::shapes::object_shape_record_in_gc(sibling).is_none(),
            "publishing one family cannot publish the other family's row"
        );
        assert_eq!((*sibling).parent_class_id, sibling_word | PENDING);
        assert!(crate::object::shapes::object_shape_record(sibling).is_some());
        assert_eq!((*sibling).parent_class_id, sibling_word);
        let child_bits =
            crate::object::js_object_create(crate::value::js_nanbox_pointer(first as i64));
        let child =
            (child_bits.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
        let key = crate::string::js_string_from_bytes(b"r1_pending".as_ptr(), 10);
        (*first).parent_class_id = word | PENDING;
        let _ = crate::object::js_object_get_field_by_name(child, key);
        assert_eq!(
            (*first).parent_class_id,
            word,
            "a keyed inherited read materializes its image holder"
        );
        (*first).parent_class_id = 0;
        (*sibling).parent_class_id = 0;
        crate::gc::image_cells::hold_image_for_tests(false);
    })
    .join()
    .unwrap();
}

#[test]
fn pending_words_never_fill_site_words_or_tokens() {
    use crate::codegen_abi::LAYOUT_ROW_PENDING as PENDING;
    let word = unsafe { FACTS[0].layout_id };
    assert!(crate::object::shapes::is_site_matchable_layout_word(
        word | PENDING
    ));
    assert!(!crate::object::shapes::is_site_fillable_word(
        word | PENDING
    ));
    assert!(!crate::object::shapes::is_site_fillable_token(
        crate::object::shapes::PIC_ID_TOKEN_BIT | u64::from(word | PENDING)
    ));
    assert!(crate::object::shapes::is_site_fillable_word(word));
    assert!(crate::object::shapes::is_site_fillable_token(
        crate::object::shapes::PIC_ID_TOKEN_BIT | u64::from(word)
    ));
}

#[test]
#[should_panic(expected = "pending cell on an agent without an image")]
fn an_agent_without_an_image_rejects_pending_words() {
    crate::test_support::bootstrap_test_thread();
    crate::gc::image_cells::hold_image_for_tests(false);
    unsafe {
        crate::object::shapes::shape_record_of_object(
            std::ptr::null(),
            crate::codegen_abi::LAYOUT_TAG | crate::codegen_abi::LAYOUT_ROW_PENDING | FIRST,
        );
    }
}
