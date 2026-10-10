//! Compatibility with 447's operation boundaries, including its chain debt.
use super::*;
use crate::object::class_registry as registry;
use crate::object::shapes::{identity_prototype_word, PROTO_ID_CLASS};

extern "C" fn method(_this: f64) -> f64 {
    1.0
}
fn register(cid: u32) {
    unsafe {
        crate::object::js_register_class_name(cid, b"Selection".as_ptr(), 9);
        registry::js_register_class_method_with_entry(
            cid as i64,
            b"m".as_ptr(),
            1,
            method as *const () as usize as i64,
            0,
            0,
            0,
            0,
        );
    }
}
fn raw(cid: u32) -> u64 {
    identity_prototype_word(PROTO_ID_CLASS | u64::from(cid))
}
fn ptr(bits: u64) -> *const ObjectHeader {
    let value = crate::JSValue::from_bits(bits);
    assert!(value.is_pointer());
    value.as_pointer::<ObjectHeader>()
}
fn prototype(cid: u32) -> *const ObjectHeader {
    ptr(registry::class_decl_prototype_value(cid).to_bits())
}
fn chain(a: u32, b: u32, c: u32) {
    crate::object::js_register_class_generic_origin(a, b);
    crate::object::js_register_class_generic_origin(b, c);
}

#[test]
fn s7b_selection_prebuilt_chain_preserves_each_consumer_answer() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (a, b, c) = (0x7100, 0x7101, 0x7102);
    for cid in [a, b, c] {
        register(cid);
    }
    let bp = prototype(b);
    let cp = prototype(c);
    chain(a, b, c);
    let recv = crate::object::js_object_alloc(a, 0);
    assert_eq!(registry::class_decl_prototype_object(a).cast_const(), bp);
    // The public materializer retains its INNER projection of selected B.
    assert_eq!(prototype(a), cp);
    assert_eq!(crate::object::class_holder_word(a), raw(b));
    assert_eq!(raw(b), crate::value::js_nanbox_pointer(bp as i64).to_bits());
    assert_eq!(registry::class_method_slot_owner(a, "m"), Some(b));
    assert_eq!(
        registry::class_method_slot_value(a, "m"),
        crate::object::class_object_own_field_bytes(bp, b"m").map(f64::to_bits)
    );
    assert!(crate::object::class_has_own_method(a, "m"));
    unsafe {
        assert_eq!(shape_named_class(recv), Some(b));
        assert_eq!(class_instance_prototype(recv), cp);
        assert_eq!(class_instance_prototype_built(recv), cp);
        assert_eq!(guarded_class_instance_prototype(recv), bp);
    }
}

#[test]
fn s7b_selection_lazy_chain_preserves_inner_materializer_projection() {
    let _no_move = crate::gc::GcSuppressScope::new();
    for consumer in 0..4 {
        let a = 0x7110 + consumer * 3;
        let b = a + 1;
        let c = a + 2;
        for cid in [a, b, c] {
            register(cid);
        }
        chain(a, b, c);
        assert!(!crate::JSValue::from_bits(raw(b)).is_pointer());
        assert!(!crate::JSValue::from_bits(raw(c)).is_pointer());
        match consumer {
            0 => assert_eq!(registry::class_method_slot_owner(a, "m"), Some(c)),
            1 => {
                let value = registry::class_method_slot_value(a, "m").unwrap();
                assert_eq!(
                    value,
                    crate::object::class_object_own_field_bytes(ptr(raw(c)), b"m")
                        .unwrap()
                        .to_bits()
                );
            }
            2 => assert!(crate::object::class_has_own_method(a, "m")),
            3 => {
                let recv = crate::object::js_object_alloc(a, 0);
                unsafe {
                    assert!(guarded_class_instance_prototype(recv).is_null());
                    assert_eq!(class_instance_prototype_built(recv), ptr(raw(c)));
                }
            }
            _ => unreachable!(),
        }
        assert_eq!(unsafe { (*ptr(raw(c))).class_id }, c);
        assert!(
            !crate::JSValue::from_bits(raw(b)).is_pointer(),
            "447's one-hop lazy debt is preserved"
        );
    }
}

#[test]
fn s7b_selection_guarded_fallback_requires_identical_raw_cid() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (a, b, c) = (0x7120, 0x7121, 0x7122);
    for cid in [a, b, c] {
        register(cid);
    }
    let before = crate::object::js_object_alloc(a, 0);
    let cp = prototype(c);
    chain(a, b, c);
    let after = crate::object::js_object_alloc(a, 0);
    unsafe {
        assert_eq!(shape_named_class(before), Some(a));
        assert_eq!(shape_named_class(after), Some(b));
        assert!(!crate::JSValue::from_bits(raw(b)).is_pointer());
        assert!(guarded_class_instance_prototype(before).is_null());
        assert_eq!(guarded_class_instance_prototype(after), cp);
        assert_eq!(class_instance_prototype_built(before), cp);
    }
}

#[test]
fn s7b_selection_independent_calls_reproject_late_alias_and_replacement() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (a, b, c, d) = (0x7130, 0x7131, 0x7132, 0x7133);
    for cid in [a, b, c, d] {
        register(cid);
    }
    let old = prototype(a);
    let bp = prototype(b);
    let cp = prototype(c);
    let recv = crate::object::js_object_alloc(a, 0);
    let stamp = unsafe { crate::object::shapes::object_shape_stamp(recv) };
    crate::object::js_register_class_generic_origin(a, b);
    crate::object::js_register_class_generic_origin(d, b);
    assert_ne!(old, bp);
    assert_eq!(registry::class_method_slot_owner(a, "m"), Some(b));
    crate::object::js_register_class_generic_origin(a, c);
    assert_eq!(registry::class_method_slot_owner(a, "m"), Some(c));
    assert_eq!(unsafe { class_instance_prototype_built(recv) }, cp);
    registry::class_decl_prototype_object_root_store(b, cp.cast_mut());
    assert_eq!(registry::class_method_slot_owner(d, "m"), Some(c));
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_stamp(recv) },
        stamp
    );
    assert_eq!(
        raw(a),
        crate::value::js_nanbox_pointer(old as i64).to_bits(),
        "no cold alias publication"
    );
}

#[test]
fn s7b_selection_generic_aliases_share_birth_and_noop_prototype_identity() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let (a, d, b) = (0x7140, 0x7141, 0x7142);
    for cid in [a, d, b] {
        register(cid);
    }
    let bp = prototype(b);
    crate::object::js_register_class_generic_origin(a, b);
    crate::object::js_register_class_generic_origin(d, b);
    let x = crate::object::js_object_alloc(a, 0);
    let y = crate::object::js_object_alloc(d, 0);
    unsafe {
        assert_eq!(
            crate::object::shapes::object_shape_stamp(x),
            crate::object::shapes::object_shape_stamp(y)
        );
        assert_eq!(class_instance_prototype(x), bp);
        assert_eq!(class_instance_prototype(y), bp);
        let stamp = crate::object::shapes::object_shape_stamp(x);
        crate::object::js_object_set_prototype_of(
            crate::value::js_nanbox_pointer(x as i64),
            crate::value::js_nanbox_pointer(bp as i64),
        );
        assert_eq!(crate::object::shapes::object_shape_stamp(x), stamp);
    }
}

#[test]
fn s7b_selection_recorded_words_and_zero_class_keep_existing_precedence() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let b = 0x7150;
    register(b);
    let bp = prototype(b);
    crate::object::js_register_class_generic_origin(0, b);
    assert_eq!(
        crate::object::class_generic_origin(0),
        None,
        "registration rejects zero edges"
    );
    let zero = crate::object::js_object_alloc(0, 0);
    assert_eq!(unsafe { shape_named_class(zero) }, None);
    assert!(unsafe { class_instance_prototype_built(zero) }.is_null());
    let default = identity_prototype_word(crate::object::shapes::PROTO_ID_DEFAULT);
    assert_eq!(crate::object::class_holder_word(0), default);
    let expected = if crate::JSValue::from_bits(default).is_pointer() {
        crate::object::class_object_own_field_bytes(ptr(default), b"m").map(f64::to_bits)
    } else {
        None
    };
    assert_eq!(registry::class_method_slot_value(0, "m"), expected);
    assert_eq!(registry::class_method_slot_owner(0, "m"), None);
    assert!(!crate::object::class_has_own_method(0, "m"));
    let recv = crate::object::js_object_alloc(b, 0);
    crate::object::prototype_chain::object_set_user_prototype(
        recv as usize,
        crate::value::TAG_NULL,
    );
    assert!(unsafe { class_instance_prototype_built(recv) }.is_null());
    crate::object::prototype_chain::object_set_user_prototype(
        recv as usize,
        crate::value::js_nanbox_pointer(bp as i64).to_bits(),
    );
    assert_eq!(unsafe { class_instance_prototype_built(recv) }, bp);
}

thread_local! {
    static REDIRECT: std::cell::Cell<(u32,u32,u32,u32,u32)> = const { std::cell::Cell::new((0,0,0,0,0)) };
}
extern "C" fn parent_entry(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
extern "C" fn redirecting_prototype_getter(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    REDIRECT.with(|state| {
        let (a, b, c, d, calls) = state.get();
        state.set((a, b, c, d, calls + 1));
        crate::object::js_register_class_generic_origin(a, d);
        crate::object::js_register_class_generic_origin(b, c);
    });
    f64::from_bits(crate::value::TAG_NULL)
}

#[test]
fn s7b_selection_parent_getter_preserves_outer_selection_and_inner_reprojection() {
    let scope = crate::gc::RuntimeHandleScope::new();
    for consumer in 0..4 {
        let a = 0x7160 + consumer * 4;
        let b = a + 1;
        let c = a + 2;
        let d = a + 3;
        for cid in [a, b, c, d] {
            register(cid);
        }
        let cp = scope.root_raw_const_ptr(prototype(c));
        let dp = scope.root_raw_const_ptr(prototype(d));
        let parent = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(parent_entry, 0),
            0,
        ));
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(redirecting_prototype_getter, 0),
            0,
        ));
        parent.with_mut_ptr::<crate::closure::ClosureHeader, _>(|parent| {
            crate::object::descriptor_state::set_accessor_descriptor(
                parent as usize,
                "prototype".to_string(),
                crate::object::descriptor_state::AccessorDescriptor {
                    get: getter.with_mut_ptr::<crate::closure::ClosureHeader, _>(|getter| {
                        crate::value::js_nanbox_pointer(getter as i64).to_bits()
                    }),
                    set: 0,
                },
            );
        });
        let parent_value = parent.with_mut_ptr::<crate::closure::ClosureHeader, _>(|parent| {
            crate::value::js_nanbox_pointer(parent as i64)
        });
        registry::parent_static::register_class_parent_dynamic(b, parent_value, false);
        crate::object::js_register_class_generic_origin(a, b);
        REDIRECT.with(|state| state.set((a, b, c, d, 0)));
        // The built-instance consumer names unredirected raw B at birth.
        let recv = scope.root_raw_mut_ptr(crate::object::js_object_alloc(b, 0));
        match consumer {
            0 => assert_eq!(registry::class_method_slot_owner(a, "m"), Some(c)),
            1 => assert_eq!(
                registry::class_method_slot_value(a, "m"),
                cp.with_const_ptr(|cp| crate::object::class_object_own_field_bytes(cp, b"m"))
                    .map(f64::to_bits)
            ),
            2 => assert!(crate::object::class_has_own_method(a, "m")),
            3 => {
                let built =
                    recv.with_mut_ptr(|recv| unsafe { class_instance_prototype_built(recv) });
                cp.with_const_ptr(|cp| assert_eq!(built, cp));
            }
            _ => unreachable!(),
        }
        assert!(
            REDIRECT.with(|state| state.get().4) > 0,
            "the parent getter actually redirected both aliases"
        );
        let selected = registry::class_decl_prototype_object(a).cast_const();
        dp.with_const_ptr(|dp| assert_eq!(selected, dp, "a later call selects the new outer edge"));
        let selected = registry::class_decl_prototype_object(b).cast_const();
        cp.with_const_ptr(|cp| {
            assert_eq!(
                selected, cp,
                "the inner projection observes the getter's mutation"
            )
        });
    }
}
