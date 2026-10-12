use super::*;
use crate::object::{ObjectHeader, PicCache, PicCacheSlot, PIC_CACHE_WORDS};
use crate::value::TAG_HOLE;
use std::sync::atomic::AtomicU64;

/// The materialization hook forces a copying minor if a read accidentally
/// enters the general array resolver. No generated root mechanism is changed.
#[test]
fn lazy_json_ic_leaf_declines_without_materializing_under_moving_minors() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "lazy_json_ic_leaf_declines_without_materializing_under_moving_minors",
    ) {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let input = br#"[{"text":"live"},[1,2],"tail"]"#;
    let text = scope.root_raw_mut_ptr(crate::js_string_from_bytes(
        input.as_ptr(),
        input.len() as u32,
    ));
    let lazy = crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(
            tape,
            0,
            crate::json_tape::count_array_length(tape, 0),
            text.get_raw_mut_ptr(),
        )
    })
    .unwrap();
    let lazy = scope.root_raw_mut_ptr(lazy);
    let live = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    live.with_mut_ptr(|p| crate::object::js_object_set_field(p, 0, crate::JSValue::number(73.0)));
    let key = scope.root_raw_mut_ptr(crate::string::canonical_key(b"icLeafJson"));
    let mut cache: PicCache = [0; PIC_CACHE_WORDS];
    let mut slot: PicCacheSlot = &mut cache;
    let packed = AtomicU64::new(0xFFFF_FFFF);
    let hook =
        JsonTapeSafepointHookGuard::new(crate::json_tape::JsonTapeSafepoint::ForceLazyHeaderRooted);

    for _ in 0..3 {
        let witness = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
        let original = witness.get_raw_mut_ptr::<ObjectHeader>();
        let trace = collect_minor_trace(GcTriggerKind::ArenaBytes);
        assert!(trace.copying_nursery.eligible);
        assert!(
            trace.copying_nursery.copied_objects + trace.copying_nursery.promoted_objects > 0,
            "the test must actually move live objects"
        );
        assert_ne!(original, witness.get_raw_mut_ptr::<ObjectHeader>());
        let before = gc_collection_count();
        let raw = lazy.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
        let receiver = f64::from_bits(ptr_bits(raw as usize));
        let k = key.get_raw_mut_ptr::<crate::StringHeader>();
        unsafe {
            assert_eq!(crate::object::shapes::object_shape_stamp(raw.cast()), 0);
            // Exercise the shared probe even though unit-test feedback makes
            // the public outlined entries decline before probing the cache.
            assert!(
                crate::object::field_get_set::pic_outlined_mru_hit(raw.cast(), &mut slot,)
                    .is_none()
            );
            assert_eq!(
                crate::object::js_object_get_field_ic_front(
                    crate::object::shapes::ordinary_dir_addr(),
                    (raw as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
                    string_bits(k as usize),
                    &mut slot,
                    &packed,
                )
                .to_bits(),
                TAG_HOLE
            );
            assert_eq!(
                crate::object::js_object_get_field_ic_fast(
                    receiver.to_bits() as i64,
                    k,
                    0,
                    &mut slot,
                )
                .to_bits(),
                TAG_HOLE
            );
            assert!((*raw).materialized.is_null());
        }
        assert_eq!(
            gc_collection_count(),
            before,
            "a leaf read must not collect"
        );
        assert!(!JSON_TAPE_HOOK_FIRED.with(|f| f.get()));
        assert_eq!(
            live.with_const_ptr::<ObjectHeader, _>(|p| crate::object::js_object_get_field(p, 0))
                .as_number(),
            73.0
        );
    }

    // Negative control: the general resolver reaches the armed hook. This
    // proves that moving stress would fire if a leaf read materialized JSON.
    let before = gc_collection_count();
    let witness = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let original = witness.get_raw_mut_ptr::<ObjectHeader>();
    let raw = lazy.get_raw_mut_ptr::<crate::json_tape::LazyArrayHeader>();
    assert!(!crate::array::clean_arr_ptr(raw.cast()).is_null());
    assert_eq!(hook.fired_ptr(), raw as usize);
    assert!(gc_collection_count() > before);
    assert_ne!(original, witness.get_raw_mut_ptr::<ObjectHeader>());
    lazy.with_const_ptr::<crate::json_tape::LazyArrayHeader, _>(|p| unsafe {
        assert!(!(*p).materialized.is_null());
    });
}
