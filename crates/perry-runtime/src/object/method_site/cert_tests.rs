use super::*;

extern "C" fn body_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

extern "C" fn body_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    2.0
}

const IMAGE: u32 = crate::codegen_abi::FN_PERMANENT_IMAGE | crate::codegen_abi::FN_COMPILED_BODY;

unsafe fn method(info: *const crate::closure::JsFunctionInfo) -> f64 {
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64)
}

/// The certificate is exactly the shared Shape fact: it holds for the
/// certified word at the recorded epoch, and an in-place method replacement
/// on the (marked) holder revokes it. Re-resolving rewrites the same way.
#[test]
fn a_certificate_is_the_shape_fact_and_a_replacement_revokes_it() {
    if !run_with_fresh_worker_gate("a_certificate_is_the_shape_fact_and_a_replacement_revokes_it") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    unsafe {
        let name = b"cert_method";
        let key = crate::string::canonical_key(name);
        let a = crate::fn_info!(body_a, 0; with_flags(IMAGE));
        let b = crate::fn_info!(body_b, 0; with_flags(IMAGE));
        let holder = crate::object::js_object_alloc(0, 1);
        crate::object::js_object_set_field_by_name(holder, key, method(a));
        let recv = crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
        let obj = crate::value::js_nanbox_get_pointer(recv) as *const ObjectHeader;
        assert!(
            crate::object::proto_validity::object_is_marked_prototype(holder as usize),
            "the [[Prototype]] funnel marks the holder"
        );
        let id = crate::value::JSValue::string_ptr(key as *mut _).bits() as i64;
        let mut slot: MethodCertSlot = std::ptr::null_mut();
        assert_eq!(js_method_site_certify(&mut slot, recv, id), a as u64);
        assert!(!slot.is_null());
        let fact = |slot: MethodCertSlot| PristineReceiverFact::Shape {
            receiver_word: receiver_word(obj),
            proof_word: (*slot).ways[0].word,
            proof_epoch: (*slot).epoch,
        };
        assert_eq!((*slot).ways[0].info, a as u64);
        assert!(fact(slot).holds());

        // In place: no shape change anywhere, only the epoch moves.
        let word_before = receiver_word(obj);
        crate::object::js_object_set_field_by_name(holder, key, method(b));
        assert_eq!(receiver_word(obj), word_before);
        assert!(
            !fact(slot).holds(),
            "an in-place replacement must revoke the certificate"
        );
        assert_eq!(js_method_site_certify(&mut slot, recv, id), b as u64);
        assert_eq!((*slot).ways[0].info, b as u64);
        assert!(fact(slot).holds());
        assert_eq!(
            (*slot).ways[1].word,
            METHOD_SITE_EMPTY,
            "rewritten, not added"
        );

        // An own property certifies no body: the word (a new ShapeId) is
        // recorded with body 0, which routes its hits to the generic arm.
        crate::object::js_object_set_field_by_name(obj as *mut ObjectHeader, key, method(a));
        assert_eq!(js_method_site_certify(&mut slot, recv, id), 0);
        let own = receiver_word(obj);
        assert_ne!(own, word_before);
        assert!((*slot).ways.iter().any(|w| w.word == own && w.info == 0));
    }
}

/// Only a resolution to a permanent-image body is certified; an absent
/// method is not.
#[test]
fn only_permanent_bodies_are_certified() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    unsafe {
        let name = b"cert_unmarked";
        let key = crate::string::canonical_key(name);
        // A plain object with the default prototype: nothing on its chain
        // has the name.
        let plain = crate::object::js_object_alloc(0, 1);
        assert!(prove(plain, name).is_none());
        // The body without a permanent image is refused too.
        let transient = crate::fn_info!(body_b, 0);
        let other = crate::object::js_object_alloc(0, 1);
        crate::object::js_object_set_field_by_name(other, key, method(transient));
        let recv = crate::object::js_object_create(crate::value::js_nanbox_pointer(other as i64));
        assert!(prove(
            crate::value::js_nanbox_get_pointer(recv) as *const ObjectHeader,
            name
        )
        .is_none());
    }
}

/// A polymorphic site keeps one certificate per receiver word, all proven
/// at the record's one epoch.
#[test]
fn receiver_words_share_the_record_epoch() {
    if !run_with_fresh_worker_gate("receiver_words_share_the_record_epoch") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    unsafe {
        let key = crate::string::canonical_key(b"cert_poly");
        let a = crate::fn_info!(body_a, 0; with_flags(IMAGE));
        let holder = crate::object::js_object_alloc(0, 1);
        crate::object::js_object_set_field_by_name(holder, key, method(a));
        let id = crate::value::JSValue::string_ptr(key as *mut _).bits() as i64;
        let mut slot: MethodCertSlot = std::ptr::null_mut();
        let mut words = Vec::new();
        for (i, own) in [b"p0".as_slice(), b"p1", b"p2", b"p3"].iter().enumerate() {
            let recv =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
            let obj = crate::value::js_nanbox_get_pointer(recv) as *mut ObjectHeader;
            crate::object::js_object_set_field_by_name(
                obj,
                crate::string::canonical_key(own),
                i as f64,
            );
            words.push(receiver_word(obj));
            assert_eq!(js_method_site_certify(&mut slot, recv, id), a as u64);
        }
        for w in &words {
            assert!((*slot)
                .ways
                .iter()
                .any(|way| way.word == *w && way.info == a as u64));
            let fact = PristineReceiverFact::Shape {
                receiver_word: *w,
                proof_word: *w,
                proof_epoch: (*slot).epoch,
            };
            assert!(fact.holds());
        }
    }
}
