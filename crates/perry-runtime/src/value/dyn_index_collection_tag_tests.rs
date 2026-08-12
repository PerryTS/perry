//! Receiver-tag gating for dynamic-index Map/Set registry probes (#7865).

use super::{js_dyn_index_get, js_dyn_index_set};

fn probes() -> (u64, u64) {
    super::test_collection_registry_probe_count()
}

fn header_reads() -> u64 {
    super::test_receiver_gc_header_read_count()
}

fn arm_both_registries() -> (*mut crate::map::MapHeader, *mut crate::set::SetHeader) {
    let map = crate::map::js_map_alloc(4);
    crate::map::js_map_set(map, 1.0, 10.0);
    let set = crate::set::js_set_alloc(4);
    crate::set::js_set_add(set, 20.0);
    assert!(crate::map::is_registered_map(map as usize));
    assert!(crate::set::is_registered_set(set as usize));
    (map, set)
}

#[test]
fn plain_array_dynamic_indexing_never_probes_collection_registries() {
    let (_map, _set) = arm_both_registries();
    let mut array = crate::array::js_array_alloc(2);
    array = crate::array::js_array_push_f64(array, 10.0);
    let receiver = crate::value::js_nanbox_pointer(array as i64);
    let before = probes();

    assert_eq!(js_dyn_index_get(receiver, 0.0), 10.0);
    assert_eq!(js_dyn_index_set(receiver, 0.0, 30.0), 30.0);
    assert_eq!(js_dyn_index_get(receiver, 0.0), 30.0);

    assert_eq!(
        probes(),
        before,
        "GC_TYPE_ARRAY must bypass both Map/Set registry probes"
    );
}

#[test]
fn collection_receivers_still_use_their_authoritative_registries() {
    let (map, set) = arm_both_registries();
    let map_receiver = crate::value::js_nanbox_pointer(map as i64);
    let set_receiver = crate::value::js_nanbox_pointer(set as i64);

    let before = probes();
    assert_eq!(js_dyn_index_get(map_receiver, 0.0), 1.0);
    assert_eq!(probes(), (before.0 + 1, before.1));

    let before = probes();
    assert_eq!(js_dyn_index_get(set_receiver, 0.0), 20.0);
    assert_eq!(probes(), (before.0, before.1 + 1));

    let before = probes();
    assert_eq!(js_dyn_index_set(map_receiver, 0.0, 99.0), 99.0);
    assert_eq!(probes(), (before.0 + 1, before.1));

    let before = probes();
    assert_eq!(js_dyn_index_set(set_receiver, 0.0, 88.0), 88.0);
    assert_eq!(probes(), (before.0, before.1 + 1));
}

#[test]
fn unmapped_legacy_raw_i64_is_rejected_before_the_gc_header_read() {
    // An aligned, finite raw-I64 candidate inside the platform's permissive
    // heap magnitude range, but not inside any arena or tracked malloc object.
    // The pre-review ordering passed it to `receiver_gc_tag` and attempted to
    // read the unmapped header at 4 GiB - 8.
    let raw_bits = 0x0000_0001_0000_0000_u64;
    let raw_receiver = f64::from_bits(raw_bits);
    assert!(!raw_receiver.is_nan());
    assert!(crate::value::addr_class::is_plausible_heap_addr(
        raw_bits as usize
    ));
    assert!(matches!(
        crate::arena::classify_heap_space(raw_bits as usize),
        crate::arena::HeapSpace::Unknown
    ));

    let probes_before = probes();
    assert_eq!(header_reads(), 0);

    assert_eq!(
        js_dyn_index_get(raw_receiver, 0.0).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(probes(), probes_before);
    assert_eq!(header_reads(), 0);

    assert_eq!(js_dyn_index_set(raw_receiver, 0.0, 42.0), 42.0);
    assert_eq!(probes(), probes_before);
    assert_eq!(header_reads(), 0);
}
