//! Site memos hold their holders as plain, unrooted words (I1, `gc::never_move`;
//! the argument is "Weak entries" in `object/method_site/read_holder.rs`). Each
//! test here checks one premise against a real moving collector, and runs the
//! same steps with that premise sabotaged to show the check can fail.
use super::super::*;
use super::support::*;
use crate::gc::never_move;
use crate::object::method_site::read_holder;
use crate::object::{ObjectHeader, PicCache, PicCacheSlot};

fn register_object_model_scanners() {
    for scanner in [
        crate::object::scan_object_cache_roots_mut,
        crate::object::scan_shape_cache_roots_mut,
        crate::object::shapes::scan_shape_table_rekey_mut,
    ] {
        gc_register_mutable_root_scanner(scanner);
    }
}

fn holder_key() -> *const crate::StringHeader {
    crate::string::canonical_key(b"weak_holder_memo_key") as *const crate::StringHeader
}

unsafe fn shadow_object(slot: u32) -> *mut ObjectHeader {
    (js_shadow_slot_get(slot) & POINTER_MASK) as *mut ObjectHeader
}

/// The site's answer for the receiver in shadow slot 0, from the entry alone.
unsafe fn entry_answer_for(slot: PicCacheSlot) -> Option<u64> {
    let cache = crate::object::field_get_set::pic_slot_peek::<PicCache>(
        &slot as *const PicCacheSlot as *mut PicCacheSlot,
    );
    if cache.is_null() {
        return None;
    }
    let recv = shadow_object(0);
    let token = (u64::from(crate::object::shapes::object_shape_stamp(recv))
        | crate::object::shapes::PIC_ID_TOKEN_BIT) as i64;
    read_holder::entry_answer(&*cache, token)
}

/// Premise 1, young leg: a prime that would name a young holder is refused,
/// the holder is promoted by the next minor, and the entry published after it
/// reads the live holder. Returns whether the entry answered the live value
/// after the holder's value changed.
fn young_holder_scenario(admit_young: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_object_model_scanners();
    let _sabotage = never_move::sabotage::Guard::new(admit_young, false, false);
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(proto, holder_key(), 17.0);
        let recv = crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
        js_shadow_slot_set(0, recv.to_bits());
        js_shadow_slot_set(1, ptr_bits(proto as usize));
        assert!(crate::arena::pointer_in_nursery(proto as usize));
        let mut slot: PicCacheSlot = std::ptr::null_mut();
        let prime = |slot: &mut PicCacheSlot| {
            read_holder::prime_read_holder(shadow_object(0), holder_key(), slot).map(|v| v.bits())
        };
        assert_eq!(prime(&mut slot), Some(17.0f64.to_bits()));
        if !admit_young {
            assert_eq!(
                entry_answer_for(slot),
                None,
                "a young holder must not be published"
            );
        }
        let minor = collect_minor_trace(GcTriggerKind::Direct);
        assert!(minor.copying_nursery.promoted_objects >= 1);
        let proto = shadow_object(1);
        assert!(
            never_move::is_never_move(header_from_user_ptr(proto as *const u8)),
            "the refused prime asked for the holder"
        );
        if !admit_young {
            assert_eq!(
                crate::arena::classify_heap_generation(proto as usize),
                crate::arena::HeapGeneration::Old,
                "a never-move object is promoted at its first survival"
            );
            assert_eq!(prime(&mut slot), Some(17.0f64.to_bits()));
            assert_eq!(entry_answer_for(slot), Some(17.0f64.to_bits()));
        }
        crate::object::js_object_set_field_by_name(proto, holder_key(), 18.0);
        entry_answer_for(slot) == Some(18.0f64.to_bits())
    }
}

#[test]
fn a_young_holder_is_refused_then_published_once_old() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "a_young_holder_is_refused_then_published_once_old",
    ) {
        return;
    }
    assert!(young_holder_scenario(false));
    // Negative control: publishing the young holder leaves the entry naming
    // its from-space copy after the minor.
    assert!(!young_holder_scenario(true));
}

/// Premise 1, old leg: old-page evacuation never moves a never-move object.
/// Returns whether the object stayed in place.
fn defrag_scenario(evacuate_never_move: bool) -> bool {
    let _heap_change = crate::gc::heap_generation::HeapChange::begin(
        crate::gc::heap_generation::HeapChangeKind::Compaction,
    );
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let _sabotage = never_move::sabotage::Guard::new(false, evacuate_never_move, false);
    let holder = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (header, total) = old_test_header_and_size(holder);
    unsafe {
        never_move::request_never_move(holder);
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
    let mut pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(header as usize, total) {
        pages.insert(page);
    }
    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let _ = evacuate_selected_old_pages_collecting(&pages, &mut new_headers, &mut original_headers);
    let stayed = unsafe { (*header).gc_flags & GC_FLAG_FORWARDED == 0 };
    let _ = release_evacuated_original_forwarding_stubs(&original_headers);
    clear_marks();
    stayed
}

#[test]
fn old_page_evacuation_never_moves_a_never_move_object() {
    assert!(defrag_scenario(false));
    assert!(!defrag_scenario(true), "negative control: the holder moves");
}

/// A never-move young object is promoted at its first survival. Returns
/// whether it is old after one minor.
fn first_survival_scenario(age_normally: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    // One collection, the one this scenario runs: no seeded schedule (a
    // never-collect override beats PERRY_GC_SCHEDULE_SEED) and no forced
    // evacuation.
    let _schedule = super::super::schedule::ScheduleGuard::set(0, 0);
    let _evacuate = super::super::knob_overrides::ForcedEvacuationTestGuard::off();
    let _sabotage = never_move::sabotage::Guard::new(false, false, age_normally);
    unsafe {
        let (obj, _) = alloc_nursery_test_object(1);
        never_move::request_never_move(obj as usize);
        js_shadow_slot_set(0, ptr_bits(obj as usize));
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        crate::arena::classify_heap_generation(
            js_shadow_slot_get(0) as usize & POINTER_MASK as usize,
        ) == crate::arena::HeapGeneration::Old
    }
}

/// The non-copying (fallback) minor tenures a never-move object at its first
/// survival too, so the next copying minor or evacuation moves it old.
fn first_survival_fallback_scenario(age_normally: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    // One collection, the one this scenario runs: no seeded schedule (a
    // never-collect override beats PERRY_GC_SCHEDULE_SEED) and no forced
    // evacuation.
    let _schedule = super::super::schedule::ScheduleGuard::set(0, 0);
    let _evacuate = super::super::knob_overrides::ForcedEvacuationTestGuard::off();
    let _sabotage = never_move::sabotage::Guard::new(false, false, age_normally);
    unsafe {
        let (obj, _) = alloc_nursery_test_object(1);
        never_move::request_never_move(obj as usize);
        js_shadow_slot_set(0, ptr_bits(obj as usize));
        let trace = {
            let _copy_only = TemporaryCopyOnlyRootScanner::rust_bits(&[]);
            collect_minor_trace(GcTriggerKind::Direct)
        };
        assert!(
            !trace.copying_nursery.eligible,
            "the subject is a non-copying minor"
        );
        // Tenured in place, or already evacuated old by a forced evacuation.
        let obj = (js_shadow_slot_get(0) & POINTER_MASK) as *const u8;
        (*header_from_user_ptr(obj)).gc_flags & GC_FLAG_TENURED != 0
            || crate::arena::classify_heap_generation(obj as usize)
                == crate::arena::HeapGeneration::Old
    }
}

#[test]
fn a_non_copying_minor_tenures_a_never_move_object_at_its_first_survival() {
    assert!(first_survival_fallback_scenario(false));
    assert!(
        !first_survival_fallback_scenario(true),
        "negative control: an aged never-move object is only marked survived"
    );
}

#[test]
fn a_never_move_object_is_promoted_at_its_first_survival() {
    assert!(first_survival_scenario(false));
    assert!(
        !first_survival_scenario(true),
        "negative control: an aged never-move object stays in a survivor space"
    );
}

/// Premise 4: a class relink pins a displaced prototype that a memo named.
/// Returns whether the displaced prototype is pinned.
fn relink_scenario(skip_pin: bool) -> bool {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let _sabotage = skip_pin.then(never_move::sabotage::RetireGuard::new);
    const CID: u32 = 0x4323_2000;
    unsafe {
        crate::object::js_register_class_name(CID, b"WeakMemoRelink".as_ptr(), 14);
        let first = crate::object::object_alloc_prototype_old(CID, 1);
        let second = crate::object::object_alloc_prototype_old(CID, 1);
        crate::object::test_seed_class_decl_prototype_object_root(CID, first as usize);
        assert!(crate::object::method_site::memo_referents_stable([
            first as usize
        ]));
        crate::object::test_seed_class_decl_prototype_object_root(CID, second as usize);
        let header = header_from_user_ptr(first as *const u8);
        let pinned = (*header).gc_flags & GC_FLAG_PINNED != 0;
        if pinned {
            crate::gc::unpin_user_ptr(first as *mut u8);
        }
        pinned
    }
}

#[test]
fn a_relink_pins_the_displaced_prototype_a_memo_named() {
    assert!(relink_scenario(false));
    assert!(!relink_scenario(true), "negative control: left unpinned");
}

/// Premise 5: replacing an accessor pair moves its holder to a new ShapeId.
fn pair_replacement_scenario(skip_transition: bool) -> bool {
    extern "C" fn first(_this: f64) -> f64 {
        1.0
    }
    extern "C" fn second(_this: f64) -> f64 {
        2.0
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let _sabotage = skip_transition
        .then(crate::object::descriptor_state::accessor_replacement_sabotage::Guard::new);
    crate::object::builtin_prototype_value("Object");
    unsafe {
        let holder = crate::object::js_object_alloc(0, 2);
        let install = |getter: extern "C" fn(f64) -> f64| {
            crate::object::set_builtin_accessor_pair(
                holder as usize,
                "weak_memo_pair".to_owned(),
                crate::object::accessor_pair::Accessor {
                    raw_get: getter as *const () as usize,
                    ..Default::default()
                },
                crate::object::PropertyAttrs::new(true, false, true),
            )
        };
        install(first);
        let before = crate::object::shapes::object_shape_stamp(holder);
        install(second);
        crate::object::shapes::object_shape_stamp(holder) != before
    }
}

#[test]
fn replacing_an_accessor_pair_moves_the_holder_shape() {
    assert!(pair_replacement_scenario(false));
    assert!(
        !pair_replacement_scenario(true),
        "negative control: the holder keeps its ShapeId"
    );
}

/// A refused site does not retry its prime until a minor has run: the misses
/// in between take the generic path without walking or publishing. Returns
/// how many publications were refused across six reads before the minor.
fn refused_primes_before_a_minor(ignore_stamp: bool) -> u64 {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    register_object_model_scanners();
    let _sabotage = ignore_stamp.then(crate::object::method_site::young_retry_sabotage::Guard::new);
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        crate::object::js_object_set_field_by_name(proto, holder_key(), 17.0);
        let recv = crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
        js_shadow_slot_set(0, recv.to_bits());
        let mut slot: PicCacheSlot = std::ptr::null_mut();
        let before = crate::object::method_site::memo_unstable_referent_refusals();
        for _ in 0..6 {
            let read = read_holder::prime_read_holder(shadow_object(0), holder_key(), &mut slot)
                .map(|v| v.bits());
            if let Some(bits) = read {
                assert_eq!(bits, 17.0f64.to_bits());
            }
        }
        crate::object::method_site::memo_unstable_referent_refusals() - before
    }
}

#[test]
fn a_refused_site_waits_for_the_next_minor_before_priming_again() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "a_refused_site_waits_for_the_next_minor_before_priming_again",
    ) {
        return;
    }
    assert_eq!(refused_primes_before_a_minor(false), 1);
    assert_eq!(
        refused_primes_before_a_minor(true),
        6,
        "negative control: without the stamp every miss primes again"
    );
}
