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

/// A template cell as codegen emits it: `words` long, zero but its length.
fn cell(words: usize) -> *mut u64 {
    let mut cell = vec![0u64; words].into_boxed_slice();
    cell[0] = words as u64;
    Box::leak(cell).as_mut_ptr()
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
    let cell = cell(64);
    js_register_class_template_cell(cid as i64, cell as i64);
    let scope = crate::gc::RuntimeHandleScope::new();
    let before = template_hits();
    let classes: Vec<_> = (0..3)
        .map(|_| {
            scope.root_raw_mut_ptr(js_class_evaluation_object(cid, 6, 0, cell) as *mut ObjectHeader)
        })
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
        let addr =
            |h: &crate::gc::RuntimeHandle<'_>| h.with_mut_ptr::<ObjectHeader, _>(|o| o as usize);
        let slot_of = |h: &crate::gc::RuntimeHandle<'_>, key: &[u8]| {
            h.with_mut_ptr::<ObjectHeader, _>(|o| slot(o, key))
        };
        assert!(
            addr(&classes[0]) != addr(&classes[1])
                && addr(&protos[0]) != addr(&protos[1])
                && addr(&protos[1]) != addr(&protos[2]),
            "one object per evaluation"
        );
        assert_eq!(shape(&classes[0]), shape(&classes[1]));
        assert_eq!(shape(&classes[1]), shape(&classes[2]));
        assert_eq!(shape(&protos[0]), shape(&protos[1]));
        assert_eq!(shape(&protos[1]), shape(&protos[2]));
        for i in 0..3 {
            classes[i].with_mut_ptr::<ObjectHeader, _>(|c| {
                protos[i].with_mut_ptr::<ObjectHeader, _>(|p| {
                    let s = slot(c, b"s");
                    assert!(
                        static_method_value_runs(s, info, c),
                        "s of evaluation {i} is at home in it"
                    );
                    let m = slot(p, b"m");
                    assert!(
                        static_method_value_runs(m, info, c),
                        "m of evaluation {i} is at home in it"
                    );
                    assert_eq!(
                        slot(p, b"constructor"),
                        crate::value::js_nanbox_pointer(c as i64).to_bits(),
                        "prototype {i}'s constructor is its class object"
                    );
                    assert_eq!(
                        super::super::class_registry::class_object_own_field_bytes(
                            c,
                            super::class_object_props::CLASS_EVALUATION_PROTOTYPE_KEY,
                        )
                        .map(f64::to_bits),
                        Some(crate::value::js_nanbox_pointer(p as i64).to_bits()),
                        "class object {i} links its own prototype"
                    );
                })
            });
        }
        assert_ne!(
            slot_of(&classes[1], b"s"),
            slot_of(&classes[2], b"s"),
            "statics are per evaluation"
        );
        assert_ne!(
            slot_of(&protos[1], b"m"),
            slot_of(&protos[2], b"m"),
            "methods are per evaluation"
        );
    }
}

/// The template's memo lives in its own cell, and only the thread that
/// recorded it reads it: ShapeIds name one agent's shape records, so another
/// thread must take the ordinary path. A cell too short for a record refuses
/// to record rather than write past its end.
#[test]
fn a_template_cell_answers_only_its_recording_thread() {
    unsafe {
        let words = cell(W_FILLS + 2);
        let c = TemplateCell::from_ptr(words).expect("a cell");
        assert!(
            TemplateCell::from_ptr(cell(W_FILLS - 1)).is_none(),
            "too short for its fixed words"
        );
        assert!(TemplateCell::from_ptr(std::ptr::null()).is_none());
        assert!(c.claim(), "the first thread claims the cell");
        assert!(c.owned() && c.claim(), "and keeps it");
        c.set(W_CLASS_SHAPE, 7);
        c.set(W_CLASS_FACTS, 6 | 1 << 48);
        assert_eq!(c.class_template(6, 0), Some((7, 1, false)));
        assert_eq!(
            c.class_template(5, 0),
            None,
            "another width is another site"
        );
        let addr = words as usize;
        let other = std::thread::spawn(move || {
            let c = TemplateCell::from_ptr(addr as *const u64).unwrap();
            (c.owned(), c.claim(), c.class_template(6, 0).is_some())
        })
        .join()
        .unwrap();
        assert_eq!(
            other,
            (false, false, false),
            "another thread never reads the memo"
        );
    }
}
