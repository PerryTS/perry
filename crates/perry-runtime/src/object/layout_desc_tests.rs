use super::layout_desc::*;
use super::*;

use super::runtime_layouts::{atom_hash, layout_id};
use crate::codegen_abi::LayoutWord;

#[test]
fn static_layout_real_abi_and_dynamic_atom_identity() {
    crate::gc::initialize_runtime_fixture();
    let descriptor = std::ptr::addr_of!(runtime_layouts::ITERATOR_RESULT);
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(unsafe { perry_alloc_layout(descriptor) });
    let atom = unsafe { (*(*descriptor).keys).atom };
    let key = unsafe { (*atom).string };
    let supplied = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let interned = crate::string::js_string_intern(supplied, atom_hash(b"value"));
    assert_eq!(
        interned, key,
        "dynamic names must share the static atom address"
    );
    assert_eq!(unsafe { perry_layout_index_probe(descriptor, atom) }, 0);
    obj.with_const_ptr(|object: *const ObjectHeader| unsafe {
        let gc = crate::gc::header_from_trusted_user_ptr(object.cast());
        assert_eq!(
            (*gc).size as usize,
            crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ObjectHeader>() + 16
        );
        assert_eq!((*object).parent_class_id, layout_id(descriptor));
        let fields = object
            .cast::<u8>()
            .add(std::mem::size_of::<ObjectHeader>())
            .cast::<u64>();
        assert_eq!(*fields, crate::value::TAG_HOLE);
        assert_eq!(*fields.add(1), crate::value::TAG_HOLE);
    });
    obj.with_mut_ptr(|object| js_object_set_field(object, 0, JSValue::number(37.0)));
    crate::gc::js_gc_collect();
    obj.with_const_ptr(|object| assert_eq!(js_object_get_field_by_name_f64(object, key), 37.0));
}

#[test]
fn static_layout_zero_slots_expand_without_an_inline_floor() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_raw_mut_ptr(js_object_alloc_plain(1));
    let inherited = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"inherited".as_ptr(),
        9,
    ));
    prototype.with_mut_ptr::<ObjectHeader, _>(|object| {
        js_object_set_field_by_name(object, inherited.get_raw_mut_ptr(), 41.0);
    });
    prototype.with_mut_ptr::<ObjectHeader, _>(|object| unsafe {
        builtin_proto::seal_runtime_prototype(
            object,
            crate::closure::shape::INTRINSIC_SERIAL_TIMEOUT,
        );
    });
    let obj = scope.root_raw_mut_ptr(unsafe {
        perry_alloc_layout(std::ptr::addr_of!(runtime_layouts::TIMEOUT))
    });
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"extra".as_ptr(), 5));
    obj.with_mut_ptr::<ObjectHeader, _>(|object| unsafe {
        assert_eq!(object_inline_capacity(object), 0);
        (*object).class_id = crate::native_class_ids::TIMEOUT;
        assert_eq!(
            shape_store::try_store(
                crate::value::js_nanbox_pointer(object as i64),
                crate::value::js_nanbox_string(
                    key.get_raw_const_ptr::<crate::StringHeader>() as i64
                ),
                23.0,
            ),
            Some(23.0),
        );
    });
    obj.with_const_ptr::<ObjectHeader, _>(|object| unsafe {
        assert_eq!(
            object_live_slot_count(object),
            0,
            "expando must not expose nonexistent inline storage"
        );
        assert_eq!(
            js_object_get_field_by_name_f64(object, key.get_raw_mut_ptr()),
            23.0
        );
        assert_eq!(
            js_object_get_field_by_name_f64(object, inherited.get_raw_mut_ptr()),
            41.0,
            "key addition must retain the declared prototype identity"
        );
    });
    crate::gc::js_gc_collect();
    obj.with_const_ptr::<ObjectHeader, _>(|object| {
        assert_eq!(
            js_object_get_field_by_name_f64(object, key.get_raw_mut_ptr()),
            23.0
        );
        assert_eq!(
            js_object_get_field_by_name_f64(object, inherited.get_raw_mut_ptr()),
            41.0
        );
    });
}

#[test]
fn static_layout_state_views_have_no_fixed_field_spill() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let readable = scope.root_nanbox_f64(crate::node_stream::js_node_stream_readable_new(
        f64::from_bits(crate::value::TAG_UNDEFINED),
    ));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"_readableState".as_ptr(),
        14,
    ));
    let stream = crate::js_nanbox_get_pointer(readable.get_nanbox_f64()) as *mut ObjectHeader;
    let view = scope.root_nanbox_f64(js_object_get_field_by_name_f64(
        stream,
        key.get_raw_mut_ptr(),
    ));
    let object = crate::js_nanbox_get_pointer(view.get_nanbox_f64()) as *const ObjectHeader;
    unsafe {
        assert_eq!(
            (*object).parent_class_id,
            layout_id(std::ptr::addr_of!(runtime_layouts::READABLE_STATE))
        );
        assert_eq!(object_live_slot_count(object), 6);
        assert_eq!(
            (*(*object).meta).spill,
            0,
            "fixed state fields belong to the declared slots"
        );
    }
}

#[test]
fn static_layout_generator_results_share_iterator_descriptor() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_string_ptr(crate::string::js_string_from_bytes(b"yielded".as_ptr(), 7));
    let result = scope.root_nanbox_f64(crate::object::global_this::js_generator_result(
        f64::from_bits(JSValue::string_ptr(value.get_raw_mut_ptr()).bits()),
        0,
    ));
    let desc = std::ptr::addr_of!(runtime_layouts::ITERATOR_RESULT);
    let object =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
    unsafe {
        assert_eq!((*object).class_id, 0);
        assert_eq!((*object).parent_class_id, layout_id(desc));
        assert_eq!(object_live_slot_count(object), 2);
        assert_eq!(
            object_field_at_with_live(object, 1, 2).bits(),
            crate::value::TAG_FALSE
        );
    }
    crate::gc::js_gc_collect();
    let object =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const ObjectHeader;
    unsafe {
        assert_eq!(
            object_field_at_with_live(object, 0, 2).bits(),
            JSValue::string_ptr(value.get_raw_mut_ptr()).bits()
        );
    }
    let done = crate::object::global_this::js_generator_result(37.0, 1);
    let object = crate::value::js_nanbox_get_pointer(done) as *const ObjectHeader;
    unsafe {
        assert_eq!((*object).parent_class_id, layout_id(desc));
        assert_eq!(object_field_at_with_live(object, 0, 2).as_number(), 37.0);
        assert_eq!(
            object_field_at_with_live(object, 1, 2).bits(),
            crate::value::TAG_TRUE
        );
    }
}

/// A descriptor names a sealed runtime prototype by its intrinsic serial: an
/// object born on the layout has that prototype with no link, shares the
/// identity an `Object.create(P)` receiver gets, and a write to the sealed
/// prototype sets its own shadow bit.
#[test]
fn static_layout_names_a_sealed_runtime_prototype() {
    crate::gc::initialize_runtime_fixture();
    let serial = crate::closure::shape::INTRINSIC_SERIAL_TIMEOUT;
    let descriptor = std::ptr::addr_of!(runtime_layouts::TIMEOUT);
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_raw_mut_ptr(js_object_alloc_plain(1));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"tick".as_ptr(), 4));
    prototype.with_mut_ptr::<ObjectHeader, _>(|object| unsafe {
        js_object_set_field_by_name(object, key.get_raw_mut_ptr(), 5.0);
        builtin_proto::seal_runtime_prototype(object, serial);
        assert!(
            !word_is_shadowed((*object).parent_class_id),
            "sealing writes nothing"
        );
    });
    let proto_bits = || {
        prototype.with_const_ptr::<ObjectHeader, _>(|object| {
            crate::js_nanbox_pointer(object as i64).to_bits()
        })
    };
    let obj = scope.root_raw_mut_ptr(unsafe { perry_alloc_layout(descriptor) });
    let created = scope.root_nanbox_f64(js_object_create(f64::from_bits(proto_bits())));
    obj.with_const_ptr::<ObjectHeader, _>(|object| unsafe {
        let id = (*object).parent_class_id;
        assert_eq!(id, layout_id(descriptor));
        assert_eq!(
            shapes::shape_proto_id(LayoutWord(id)),
            Some(serial),
            "the serial is the identity"
        );
        assert_eq!(
            js_object_get_prototype_of(crate::js_nanbox_pointer(object as i64)).to_bits(),
            proto_bits(),
            "born on the layout, linked by its shape"
        );
        let other = crate::js_nanbox_get_pointer(created.get_nanbox_f64()) as *const ObjectHeader;
        assert_eq!(
            shapes::shape_proto_id(shapes::object_shape_stamp(other)),
            Some(serial),
            "Object.create(P) names the same identity"
        );
        assert_eq!(
            js_object_get_field_by_name_f64(object, key.get_raw_mut_ptr()),
            5.0
        );
    });
    // The named-store miss's own-slot overwrite (`shape_store::try_own_store`),
    // the path a compiled `proto.tick = 6` takes.
    prototype.with_mut_ptr::<ObjectHeader, _>(|object| unsafe {
        assert!(shape_store::try_own_store(
            object,
            key.get_raw_mut_ptr(),
            6.0
        ));
        assert!(
            word_is_shadowed((*object).parent_class_id),
            "a write marks the prototype"
        );
    });
    obj.with_const_ptr::<ObjectHeader, _>(|object| {
        assert_eq!(
            js_object_get_field_by_name_f64(object, key.get_raw_mut_ptr()),
            6.0
        );
    });
}

/// A family birth handed any other prototype is not born on the layout: the
/// prototype's identity is the one compare, and an object that has an identity
/// of its own fails it.
#[test]
fn static_layout_family_birth_declines_another_prototype() {
    crate::gc::initialize_runtime_fixture();
    let descriptor = std::ptr::addr_of!(runtime_layouts::TIMEOUT);
    let scope = crate::gc::RuntimeHandleScope::new();
    let other = scope.root_raw_mut_ptr(js_object_alloc_plain(1));
    let other_bits = || {
        other.with_const_ptr::<ObjectHeader, _>(|object| {
            crate::js_nanbox_pointer(object as i64).to_bits()
        })
    };
    // `Object.create(other)` gives `other` an identity of its own.
    let _child = scope.root_nanbox_f64(js_object_create(f64::from_bits(other_bits())));
    let born = scope
        .root_nanbox_f64(unsafe { perry_create_family(descriptor, f64::from_bits(other_bits())) });
    let obj = crate::js_nanbox_get_pointer(born.get_nanbox_f64()) as *const ObjectHeader;
    unsafe {
        assert_ne!(
            (*obj).parent_class_id,
            layout_id(descriptor),
            "not born on the layout"
        );
    }
    assert_eq!(
        js_object_get_prototype_of(born.get_nanbox_f64()).to_bits(),
        other_bits()
    );
}

#[test]
fn static_layout_runtime_prototype_families_keep_their_reserved_serials() {
    crate::gc::initialize_runtime_fixture();
    use crate::closure::shape::*;
    for (desc, serial) in [
        (
            std::ptr::addr_of!(runtime_layouts::URL_COMPONENTS),
            INTRINSIC_SERIAL_URL,
        ),
        (
            std::ptr::addr_of!(runtime_layouts::READABLE_STATE),
            INTRINSIC_SERIAL_READABLE_STATE,
        ),
        (
            std::ptr::addr_of!(runtime_layouts::WRITABLE_STATE),
            INTRINSIC_SERIAL_WRITABLE_STATE,
        ),
        (
            std::ptr::addr_of!(runtime_layouts::TIMEOUT),
            INTRINSIC_SERIAL_TIMEOUT,
        ),
        (
            std::ptr::addr_of!(runtime_layouts::IMMEDIATE),
            INTRINSIC_SERIAL_IMMEDIATE,
        ),
        (
            std::ptr::addr_of!(runtime_layouts::URL_SEARCH_PARAMS),
            INTRINSIC_SERIAL_URL_SEARCH_PARAMS,
        ),
    ] {
        unsafe {
            assert_eq!((*(*desc).proto).serial, serial);
            // A runtime layout's shape is installed by its first birth.
            assert_eq!(layout_word_for_birth(desc), Some(layout_id(desc)));
            assert_eq!(
                shapes::shape_proto_id(LayoutWord(layout_id(desc))),
                Some(serial)
            );
        }
    }
}

/// Startup draws a runtime layout's id but installs no shape for it: the
/// first birth in the agent does, under that id.
#[test]
fn runtime_layout_is_installed_by_its_first_birth() {
    std::thread::spawn(|| unsafe {
        crate::test_support::bootstrap_test_thread();
        let installs = || super::layout_desc::registration_installs();
        let desc = std::ptr::addr_of!(runtime_layouts::ITERATOR_RESULT);
        let word = LayoutWord(layout_id(desc));
        assert_eq!(installs(), 0, "startup installed a runtime layout");
        assert!(super::shapes::shape_record_for_word(word).is_none());
        assert_ne!((*desc).shape_id, 0, "startup draws the id");
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(perry_alloc_layout(desc));
        assert_eq!(installs(), 1);
        assert!(super::shapes::shape_record_for_word(word).is_some());
        obj.with_const_ptr(|o: *const ObjectHeader| assert_eq!((*o).parent_class_id, word.0));
        let _again = scope.root_raw_mut_ptr(perry_alloc_layout(desc));
        assert_eq!(installs(), 1, "a later birth reuses the installed shape");
    })
    .join()
    .unwrap();
}

/// Every agent draws a runtime layout id in section order before user code,
/// and installs the shape under it at its own first birth: two agents
/// publish one identity under one word. URL_COMPONENTS names a prototype
/// family, so its id comes from the identity allocator, not the section index.
#[test]
fn two_agents_install_a_runtime_layout_under_one_id() {
    let desc = std::ptr::addr_of!(runtime_layouts::URL_COMPONENTS) as usize;
    let agent = move || {
        std::thread::spawn(move || unsafe {
            crate::test_support::bootstrap_test_thread();
            let desc = desc as *const PerryLayoutDesc;
            let drawn = (*desc).shape_id;
            let word = layout_word_for_birth(desc).expect("a runtime layout installs");
            let record = super::shapes::shape_record_for_word(LayoutWord(word)).expect("installed");
            (drawn, word, record.identity().0)
        })
        .join()
        .unwrap()
    };
    let (first, second) = (agent(), agent());
    assert_ne!(first.0, 0, "startup draws the id");
    assert_eq!(first, second);
    assert_eq!(
        first.0, first.2,
        "the shape is installed under the drawn id"
    );
}
