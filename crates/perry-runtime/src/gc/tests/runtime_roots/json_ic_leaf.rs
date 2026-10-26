use super::*;
use crate::codegen_abi::LayoutWord;
use crate::object::ObjectHeader;
use crate::value::{TAG_HOLE, TAG_UNDEFINED};

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
    let atom = scope.root_raw_mut_ptr(unsafe {
        crate::string::property_atom(key.get_raw_mut_ptr::<crate::StringHeader>())
            as *mut crate::StringHeader
    });
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
        unsafe {
            assert_eq!(
                crate::object::shapes::object_shape_stamp(raw.cast()),
                LayoutWord(0)
            );
            // The emitted read's probe is a GC leaf: it answers or declines
            // without materializing the lazy array.
            let atom = atom.get_raw_mut_ptr::<crate::StringHeader>();
            let probed = crate::object::layout_probe::perry_probe_get(
                crate::object::shapes::ordinary_dir_addr(),
                (raw as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
                string_bits(atom as usize),
                crate::string::property_atom_hash(atom),
            );
            assert!(matches!(probed.to_bits(), TAG_HOLE | TAG_UNDEFINED));
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
