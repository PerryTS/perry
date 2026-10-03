//! The template shapes of a per-evaluation class: every evaluation after the
//! first is born in them, and what it is born with is its own.

use super::*;

fn register(cid: u32) {
    let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
    guard
        .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
        .insert(cid);
}

extern "C" fn body(_: *const crate::closure::ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    0.0
}

extern "C" fn method(_this: f64) -> f64 {
    0.0
}

unsafe fn slot(obj: *const ObjectHeader, name: &[u8]) -> u64 {
    super::super::class_registry::class_object_own_field_bytes(obj, name)
        .expect("own data property")
        .to_bits()
}

/// A template with a static method `s` and a method `m`, both with their
/// closure-convention entries, evaluated three times: the second and third
/// class objects and prototypes are built from the template's shapes (the
/// counters say so), carry exactly the first one's ShapeIds, and hold their
/// own function objects, each at home in its own class object.
#[test]
fn later_evaluations_are_born_in_the_template_shapes() {
    let cid = 0x6E01;
    register(cid);
    let info = crate::fn_info!(body, 0) as *const crate::closure::JsFunctionInfo as usize;
    unsafe {
        crate::object::js_register_class_name(cid, b"Tpl".as_ptr(), 3);
        crate::object::js_register_class_length(cid, 0);
        super::super::class_registry::js_register_class_static_method(
            cid as i64,
            b"s".as_ptr(),
            1,
            method as *const () as usize as i64,
            0,
            0,
        );
        super::super::class_registry::parent_static::js_register_class_static_method_entry(
            cid as i64,
            b"s".as_ptr(),
            1,
            info as i64,
        );
        super::super::class_registry::js_register_class_method(
            cid as i64,
            b"m".as_ptr(),
            1,
            method as *const () as usize as i64,
            0,
            0,
            0,
        );
        super::super::class_registry::js_register_class_method_entry(
            cid as i64,
            b"m".as_ptr(),
            1,
            info as i64,
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let before = template_hits();
    let classes: Vec<_> = (0..3)
        .map(|_| scope.root_raw_mut_ptr(js_class_evaluation_object(cid, 6, 0) as *mut ObjectHeader))
        .collect();
    let protos: Vec<_> = classes
        .iter()
        .map(|c| {
            let p = c.with_mut_ptr::<ObjectHeader, _>(|c| unsafe {
                super::class_object_props::class_object_prototype_value(c)
            });
            assert!(p.is_pointer(), "a prototype object");
            scope.root_raw_mut_ptr(p.as_pointer::<ObjectHeader>() as *mut ObjectHeader)
        })
        .collect();
    let after = template_hits();
    assert_eq!(
        after.0 - before.0,
        2,
        "class objects 2 and 3 come from the template"
    );
    assert_eq!(
        after.1 - before.1,
        2,
        "prototypes 2 and 3 come from the template"
    );
    unsafe {
        let shape = |h: &crate::gc::RuntimeHandle<'_>| {
            h.with_mut_ptr::<ObjectHeader, _>(|o| crate::object::shapes::object_shape_id(o))
        };
        let c = classes
            .iter()
            .map(|h| h.get_raw_mut_ptr::<ObjectHeader>())
            .collect::<Vec<_>>();
        let p = protos
            .iter()
            .map(|h| h.get_raw_mut_ptr::<ObjectHeader>())
            .collect::<Vec<_>>();
        assert!(
            c[0] != c[1] && p[0] != p[1] && p[1] != p[2],
            "one object per evaluation"
        );
        assert_eq!(shape(&classes[0]), shape(&classes[1]));
        assert_eq!(shape(&classes[1]), shape(&classes[2]));
        assert_eq!(shape(&protos[0]), shape(&protos[1]));
        assert_eq!(shape(&protos[1]), shape(&protos[2]));
        for i in 0..3 {
            let s = slot(c[i], b"s");
            assert!(
                static_method_value_runs(s, info, c[i]),
                "s of evaluation {i} is at home in it"
            );
            let m = slot(p[i], b"m");
            assert!(
                static_method_value_runs(m, info, c[i]),
                "m of evaluation {i} is at home in it"
            );
            assert_eq!(
                slot(p[i], b"constructor"),
                crate::value::js_nanbox_pointer(c[i] as i64).to_bits(),
                "prototype {i}'s constructor is its class object"
            );
            assert_eq!(
                super::super::class_registry::class_object_own_field_bytes(
                    c[i],
                    super::class_object_props::CLASS_EVALUATION_PROTOTYPE_KEY,
                )
                .map(f64::to_bits),
                Some(crate::value::js_nanbox_pointer(p[i] as i64).to_bits()),
                "class object {i} links its own prototype"
            );
        }
        assert_ne!(
            slot(c[1], b"s"),
            slot(c[2], b"s"),
            "statics are per evaluation"
        );
        assert_ne!(
            slot(p[1], b"m"),
            slot(p[2], b"m"),
            "methods are per evaluation"
        );
    }
}
