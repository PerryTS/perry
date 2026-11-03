use super::*;
use crate::object::{canonical_keys, shapes};

struct Capture {
    state: u8,
    mints: u64,
    hits: u64,
}

impl Capture {
    fn new() -> Self {
        *census().lock().unwrap() = Census::default();
        Self {
            state: STATE.swap(2, Ordering::Relaxed),
            mints: MINTS.swap(0, Ordering::Relaxed),
            hits: MEMO_HITS.swap(0, Ordering::Relaxed),
        }
    }

    fn counts(&self) -> (u64, u64, u64) {
        let c = census().lock().unwrap();
        (
            MINTS.load(Ordering::Relaxed),
            c.duplicate_layouts.len() as u64,
            c.by_cause.get(&MintCause::Identical).copied().unwrap_or(0),
        )
    }

    fn check(&self) {
        let (mints, duplicates, identical) = self.counts();
        assert!(mints > 0, "the mint hooks must have run");
        assert_eq!(duplicates, 0, "canonical keys funnel broken");
        assert_eq!(identical, 0, "complete-facts memo broken");
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        STATE.store(self.state, Ordering::Relaxed);
        MINTS.store(self.mints, Ordering::Relaxed);
        MEMO_HITS.store(self.hits, Ordering::Relaxed);
        *census().lock().unwrap() = Census::default();
    }
}

unsafe fn raw_key() -> *mut crate::array::ArrayHeader {
    crate::object::alloc::build_keys_source_array(std::ptr::null_mut(), 0, &[b"mint_fixture"])
}

#[test]
fn census_gates_empty_classes_attributes_and_complete_facts() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    let capture = Capture::new();
    unsafe {
        let mut empty: *mut crate::array::ArrayHeader = std::ptr::null_mut();
        for class in 910_000..910_008 {
            let keys = crate::object::js_build_class_keys_array(class, 0, std::ptr::null(), 0, 0);
            assert!(!keys.is_null(), "class globals require a backing");
            if empty.is_null() {
                empty = keys;
            }
            assert_eq!(keys, empty, "empty classes share the root backing");
            shapes::class_birth_shape_ensure(keys, 0, 0, class, 0, None).unwrap();
        }
        let raw_empty = crate::array::js_array_alloc_with_length(0);
        let (_, legacy_empty) = crate::object::shape_cache_insert(
            910_008,
            canonical_keys::LiveObject::none(),
            crate::object::ObjectKeys::new(raw_empty, 0),
            0,
        );
        assert_eq!(
            legacy_empty.arr(),
            empty,
            "raw empty producers use the same root"
        );
        let proof = canonical_keys::SharedLayout::shape_cache_entry();
        let keys = canonical_keys::canonicalize(&proof, raw_key(), 1);
        let readonly = canonical_keys::rebuild_with_entries(&proof, keys.view(), 0, |_, _, _| {
            crate::object::key_attrs::ENTRY_NON_WRITABLE
        });
        assert_ne!(
            keys.addr(),
            readonly.addr(),
            "attributes are layout identity"
        );
        assert_eq!(
            key_list_content_hash(keys.addr() as u64, 1),
            key_list_content_hash(readonly.addr() as u64, 1)
        );
        assert_ne!(
            key_layout_hash(keys.addr() as u64, 1),
            key_layout_hash(readonly.addr() as u64, 1)
        );
        for layout in [keys, readonly] {
            for kind in [
                shapes::ShapeObjectKind::Ordinary,
                shapes::ShapeObjectKind::OrdinaryUnmarked,
            ] {
                for proto in [0, 731] {
                    for rep in [0, crate::object::field_rep::REP_F64] {
                        let ensure = || {
                            shapes::shape_descriptor_ensure_with_rep(
                                layout.as_const_ptr(),
                                1,
                                1,
                                0,
                                kind,
                                0,
                                proto,
                                shapes::ReceiverFacts::NONE,
                                rep,
                                None,
                            )
                            .unwrap()
                        };
                        let id = ensure();
                        assert_eq!(ensure(), id, "equal complete facts hit the memo");
                    }
                }
            }
        }
        // Whole-list and append producers must converge on the same leaf.
        let again = canonical_keys::canonicalize(&proof, raw_key(), 1);
        assert_eq!(again.addr(), keys.addr());
        let parsed_key = crate::string::js_string_from_bytes(b"mint_fixture".as_ptr(), 12);
        let (parsed, parsed_id) = crate::json::parse_shape_keys_array_with_id(&[parsed_key]);
        assert_eq!(
            parsed as usize,
            keys.addr(),
            "JSON and class/list producers converge"
        );
        assert_eq!(
            shapes::shape_descriptor_for_word(parsed_id).unwrap().keys as usize,
            keys.addr()
        );

        let before = capture.counts().0;
        for _ in 0..10 {
            shapes::shape_descriptor_ensure(keys.as_const_ptr(), 1, 1).unwrap();
        }
        // The first Ordinary/Any/default-prototype shape was already minted.
        assert_eq!(capture.counts().0, before, "repeated work mints no shapes");
    }
    capture.check();
}

#[test]
fn census_rejects_a_producer_bypassing_the_funnel() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    let capture = Capture::new();
    unsafe {
        let mut last = 0;
        for _ in 0..2 {
            let raw = raw_key();
            shapes::shape_descriptor_ensure(raw, 1, 1).unwrap();
            last = raw as usize;
        }
        assert_eq!(
            capture.counts().1,
            1,
            "the negative control must be detected"
        );
        shapes::test_drop_shape_descriptors(last);
    }
    assert_eq!(
        capture.counts().1,
        1,
        "retirement must not erase an overlap"
    );
    assert!(std::panic::catch_unwind(|| capture.check()).is_err());
}

#[test]
fn census_rejects_a_missing_facts_memo_entry() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    let capture = Capture::new();
    let id = shapes::shape_descriptor_ensure(std::ptr::null(), 0, 73).unwrap();
    shapes::test_remove_facts_index(id);
    let duplicate = shapes::shape_descriptor_ensure(std::ptr::null(), 0, 73).unwrap();
    assert_ne!(id, duplicate);
    assert_eq!(
        capture.counts().2,
        1,
        "the negative control must be detected"
    );
    assert!(std::panic::catch_unwind(|| capture.check()).is_err());
}

#[test]
fn all_omitted_identity_facts_change_the_mint_cause() {
    let base = MintFacts {
        logical_key_count: 1,
        live_inline_slot_count: 1,
        semantic_generation: 0,
        kind: shapes::ShapeObjectKind::Ordinary,
        hole_count: 0,
        proto_id: 0,
        summary: 0,
        rep: 0,
        infos: vec![],
        brands: vec![],
        integrity: 0,
    };
    let mut variants = Vec::new();
    let mut f = base.clone();
    f.proto_id = 1;
    variants.push((f, MintCause::Prototype));
    let mut f = base.clone();
    f.summary = 1;
    variants.push((f, MintCause::Attributes));
    let mut f = base.clone();
    f.rep = 1;
    variants.push((f, MintCause::Representation));
    let mut f = base.clone();
    f.infos.push(shapes::ConstFnSlotInfo { slot: 0, info: 1 });
    variants.push((f, MintCause::ConstFn));
    let mut f = base.clone();
    f.brands.push(1);
    variants.push((f, MintCause::Brands));
    let mut f = base.clone();
    f.integrity = 1;
    variants.push((f, MintCause::Integrity));
    let mut f = base.clone();
    f.kind = shapes::ShapeObjectKind::OrdinaryUnmarked;
    variants.push((f, MintCause::Kind));
    for (facts, cause) in variants {
        assert_eq!(
            classify_mint(std::slice::from_ref(&base), &facts, true),
            cause
        );
        assert_eq!(
            classify_mint(std::slice::from_ref(&facts), &facts, true),
            MintCause::Identical
        );
    }
}

#[test]
fn census_gates_real_add_definition_and_spread_producers() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    let capture = Capture::new();
    unsafe {
        let string =
            |name: &[u8]| crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let alpha = string(b"mint_gate_alpha");
        let beta = string(b"mint_gate_beta");
        let mut previous_mints = 0;
        for iteration in 0..4 {
            let object = crate::object::js_object_alloc(0, 0);
            crate::object::js_object_set_field_by_name(object, alpha, 11.0);
            let descriptor = crate::object::js_object_alloc(0, 0);
            for (name, value) in [
                (b"value".as_slice(), 22.0),
                (b"writable", f64::from_bits(crate::value::TAG_TRUE)),
                (b"enumerable", f64::from_bits(crate::value::TAG_TRUE)),
                (b"configurable", f64::from_bits(crate::value::TAG_TRUE)),
            ] {
                crate::object::js_object_set_field_by_name(descriptor, string(name), value);
            }
            let object_value = crate::value::js_nanbox_pointer(object as i64);
            crate::object::js_object_define_property(
                object_value,
                crate::value::js_nanbox_string(beta as i64),
                crate::value::js_nanbox_pointer(descriptor as i64),
            );
            crate::object::spread::js_object_spread_fresh_leading(
                f64::from_bits(crate::value::TAG_UNDEFINED),
                object_value,
            );
            let readonly = crate::object::js_object_alloc(0, 0);
            crate::object::js_object_set_field_by_name(
                readonly,
                string(b"writable"),
                f64::from_bits(crate::value::TAG_FALSE),
            );
            crate::object::js_object_define_property(
                object_value,
                crate::value::js_nanbox_string(beta as i64),
                crate::value::js_nanbox_pointer(readonly as i64),
            );
            let mints = capture.counts().0;
            if iteration != 0 {
                assert_eq!(mints, previous_mints, "warm producers must reuse facts");
            }
            previous_mints = mints;
        }
    }
    capture.check();
}

#[test]
fn retired_layouts_can_be_republished_without_a_duplicate_verdict() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_gc = crate::gc::GcSuppressScope::new();
    let capture = Capture::new();
    unsafe {
        let first = raw_key();
        let id = shapes::shape_descriptor_ensure(first, 1, 1).unwrap();
        shapes::test_drop_shape_descriptors(first as usize);
        assert!(shapes::shape_is_retired(id));
        assert!(census().lock().unwrap().layout_addrs.is_empty());
        let renewed = raw_key();
        assert_ne!(first, renewed);
        shapes::shape_descriptor_ensure(renewed, 1, 1).unwrap();
    }
    capture.check();
}
