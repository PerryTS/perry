use super::*;
use crate::codegen_abi::{LayoutWord, PerryStaticLayoutRequestV1, ShapeId};

#[test]
fn prior_dynamic_then_static_preserves_both_and_canonicalizes_static() {
    std::thread::spawn(|| {
        let keys = unsafe { canonical_keys_for_names(&[b"dpa_prior_dynamic"]) };
        let dynamic = shapes::js_object_layout_for_class_keys(keys.arr() as u64, 1, 0, 0);
        let dynamic_id = shapes::shape_record_for_word(dynamic).unwrap().identity().0;
        let mut request = PerryStaticLayoutRequestV1 {
            shape_id: shapes::SHAPE_ID_BASE + 0x7f11,
            layout_word: 0x8000_07d0,
        };
        let static_word =
            js_object_layout_for_class_keys_static(keys.arr() as u64, 1, 1, 0, &mut request, 0);
        assert_eq!(static_word, request.layout_word);
        assert_ne!(dynamic, static_word);
        assert_eq!(
            shapes::handle_by_identity(ShapeId(dynamic_id)),
            Some(LayoutWord(dynamic))
        );
        assert_eq!(
            shapes::handle_by_identity(ShapeId(request.shape_id)),
            Some(LayoutWord(static_word))
        );
        assert_eq!(
            shapes::js_object_layout_for_class_keys(keys.arr() as u64, 1, 0, 0),
            static_word
        );
        assert_eq!(
            js_object_layout_for_class_keys_static(keys.arr() as u64, 1, 1, 0, &mut request, 0),
            static_word
        );
    })
    .join()
    .unwrap();
}

#[test]
fn worker_seed_replays_static_rows_and_allocates_fresh_dynamic_rows() {
    std::thread::spawn(|| {
        crate::object::class_image::enter_current_thread_image();
        let mut request = PerryStaticLayoutRequestV1 {
            shape_id: shapes::SHAPE_ID_BASE + 0x7f12,
            layout_word: 0x8000_07d1,
        };
        // The static keyless variant must survive the worker snapshot.
        let static_word = js_object_layout_for_class_keys_static(0, 0, 3, 0, &mut request, 0);
        let keys = unsafe { canonical_keys_for_names(&[b"dpa_seed_dynamic"]) };
        let dynamic = shapes::js_object_layout_for_class_keys(keys.arr() as u64, 1, 0, 0);
        let dynamic_id = shapes::shape_record_for_word(dynamic).unwrap().identity().0;
        let seed = shapes::worker_shape_seed();
        let image = crate::object::class_image::current_image_handle();
        std::thread::spawn(move || {
            crate::object::class_image::adopt_image(image);
            let _agent = crate::agent::enter_worker_agent();
            // Different allocation history proves that the source suffix word is not replayed.
            for i in 0..9 {
                let name = format!("dpa_worker_filler_{i}");
                let k = unsafe { canonical_keys_for_names(&[name.as_bytes()]) };
                shapes::js_object_layout_for_class_keys(k.arr() as u64, 1, 0, 0);
            }
            shapes::install_worker_shape_seed(&seed);
            assert_eq!(
                shapes::handle_by_identity(ShapeId(request.shape_id)),
                Some(LayoutWord(static_word))
            );
            let local = shapes::handle_by_identity(ShapeId(dynamic_id)).unwrap();
            assert_ne!(local.0, dynamic);
            assert!(local.row().0 >= crate::object::class_image::layout_image().static_rows);
            assert_eq!(
                shapes::shape_descriptor_by_id(dynamic_id)
                    .unwrap()
                    .logical_key_count,
                1
            );
        })
        .join()
        .unwrap();
    })
    .join()
    .unwrap();
}

/// BUG (found by the typed-seam conversion): `declared_class_chain_blocks_store`
/// looked up the class's birth layout record, and compared its prototype
/// holder's stamp, with the declaration's birth IDENTITY (`birth_shape`) as if
/// it were a receiver word. Identities and words are distinct in the dense
/// layout ABI, so the lookup read an unrelated row (or none) and the declared
/// fast path never answered. It must use the declared `birth_layout_word`.
#[test]
fn declared_class_birth_lookup_uses_the_layout_word_not_the_identity() {
    std::thread::spawn(|| {
        let mut request = PerryStaticLayoutRequestV1 {
            shape_id: shapes::SHAPE_ID_BASE + 0x7f31,
            layout_word: 0x8000_07d3,
        };
        let word = js_object_layout_for_class_keys_static(0, 0, 3, 0, &mut request, 0);
        assert_eq!(word, request.layout_word);
        assert_ne!(
            request.shape_id, request.layout_word,
            "identity and word must differ"
        );
        let class_id =
            crate::object::class_registry::prototype_objects::test_alloc_synthetic_class_id();
        crate::object::class_registry::declarations::test_declare_birth_layout(
            class_id,
            request.shape_id,
            request.layout_word,
        );
        assert_eq!(
            unsafe {
                crate::object::descriptor_state::declared_class_chain_blocks_store(
                    class_id,
                    b"dpa_declared_name",
                )
            },
            Some(false),
            "the declared birth layout resolves through its word"
        );
    })
    .join()
    .unwrap();
}
