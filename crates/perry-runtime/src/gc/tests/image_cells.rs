//! WRT step 1: GC-headed objects in the realm image (`perry_realm`,
//! DESIGN §2.3, decision §7.1).
//!
//! A static image in the section holds one function cell and one object cell
//! on the ITERATOR_RESULT layout (`{value, done}`), whose `value` slot names
//! the function cell: the shape of what codegen emits for a realm. The tests
//! drive the real store paths and every collector. Each clause of the
//! contract has a knockout (`gc::image_cells::sabotage`) and a test that shows
//! the failure the clause prevents.
#![cfg(target_pointer_width = "64")]
use super::support::*;
use crate::closure::ClosureHeader;
use crate::codegen_abi::LayoutWord;
use crate::gc::image_cells::sabotage;
use crate::gc::*;
use crate::object::ObjectHeader;

#[repr(C, align(8))]
struct RealmFixture {
    closure_gc: GcHeader,
    closure: ClosureHeader,
    object_gc: GcHeader,
    object: ObjectHeader,
    slots: [u64; 2],
}

const IMAGE_FLAGS: u8 = GC_FLAG_MARKED | GC_FLAG_PINNED;
/// The object cell's kind while no test holds the image: a pointer-free leaf
/// of the same size. Every full trace in this test binary visits every image
/// cell, and a live object cell would keep its layout's keys array alive in
/// tests that never touch the image (the byte-exact pacing tests noticed).
const PARKED: u8 = GC_TYPE_BIGINT;
const ITERATOR_RESULT: u32 = crate::codegen_abi::native_layout_word("ITERATOR_RESULT", 0);
const CLOSURE_CELL: u32 = (GC_HEADER_SIZE + std::mem::size_of::<ClosureHeader>()) as u32;
const OBJECT_CELL: u32 = (GC_HEADER_SIZE + std::mem::size_of::<ObjectHeader>() + 16) as u32;

const fn fixture() -> RealmFixture {
    RealmFixture {
        closure_gc: GcHeader {
            obj_type: GC_TYPE_CLOSURE,
            gc_flags: IMAGE_FLAGS,
            _reserved: 0,
            size: CLOSURE_CELL,
        },
        closure: ClosureHeader {
            capture_count: 0,
            shape_id: LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD),
            info: &crate::value::addr_class::realm::perry_realm_fixture_info,
            props: std::ptr::null_mut(),
        },
        // Parked until a test takes the image (`ImageFixture`): see `PARKED`.
        object_gc: GcHeader {
            obj_type: PARKED,
            gc_flags: IMAGE_FLAGS,
            _reserved: 0,
            size: OBJECT_CELL,
        },
        object: ObjectHeader {
            class_id: 0,
            parent_class_id: ITERATOR_RESULT,
            meta: std::ptr::null_mut(),
        },
        // `value` is linked to the function cell by `reset_image` (a Rust
        // initializer cannot tag an address); codegen emits it at link time.
        slots: [crate::value::TAG_UNDEFINED, crate::value::TAG_FALSE],
    }
}

#[used]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
#[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_realm"
)]
static mut FIXTURE: RealmFixture = fixture();

/// The same bytes in ordinary data: a header bit could not tell them apart.
#[used]
static mut COUNTERFEIT: RealmFixture = fixture();

fn image() -> *mut RealmFixture {
    std::ptr::addr_of_mut!(FIXTURE)
}
fn closure_user() -> usize {
    unsafe { std::ptr::addr_of_mut!((*image()).closure) as usize }
}
fn object_user() -> usize {
    unsafe { std::ptr::addr_of_mut!((*image()).object) as usize }
}
fn object_header() -> *mut GcHeader {
    unsafe { std::ptr::addr_of_mut!((*image()).object_gc) }
}
fn done_slot() -> *mut u64 {
    unsafe { std::ptr::addr_of_mut!((*image()).slots[1]) }
}

fn reset_image() {
    unsafe {
        let f = image();
        (*f).object.parent_class_id = ITERATOR_RESULT;
        (*f).object.meta = std::ptr::null_mut();
        (*f).slots = [POINTER_TAG | closure_user() as u64, crate::value::TAG_FALSE];
        (*f).closure.props = std::ptr::null_mut();
        (*f).closure.shape_id = LayoutWord(crate::codegen_abi::FUNCTION_LAYOUT_WORD);
    }
}

static IMAGE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One test's hold on the process-wide image: serialized, and restored before
/// and after, because every test binary's verifiers walk these cells.
struct ImageFixture {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl ImageFixture {
    fn take() -> Self {
        let lock = IMAGE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::test_support::bootstrap_test_thread();
        reset_image();
        crate::value::addr_class::realm::install_fixture_layout();
        unsafe { (*object_header()).obj_type = GC_TYPE_OBJECT };
        crate::gc::image_cells::hold_image_for_tests(true);
        crate::gc::image_cells::forget_for_tests();
        Self { _lock: lock }
    }
}

impl Drop for ImageFixture {
    fn drop(&mut self) {
        reset_image();
        unsafe { (*object_header()).obj_type = PARKED };
        crate::gc::image_cells::hold_image_for_tests(false);
        crate::gc::image_cells::forget_for_tests();
    }
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}
fn get(name: &str) -> u64 {
    crate::object::js_object_get_field_by_name(object_user() as *const ObjectHeader, key(name))
        .bits()
}
fn set(name: &str, bits: u64) {
    crate::object::js_object_set_field_by_name(
        object_user() as *mut ObjectHeader,
        key(name),
        f64::from_bits(bits),
    );
}
fn call_value() -> f64 {
    let bits = get("value");
    assert_eq!(
        bits & TAG_MASK,
        POINTER_TAG,
        "value must be a function object"
    );
    crate::closure::js_closure_call0(
        (bits & POINTER_MASK) as *const ClosureHeader,
        crate::closure::plain_call_receiver(),
    )
}

/// A young string with known bytes, reachable only through what the test
/// stores (the conservative scan is off in these tests).
fn young_payload(tag: &str) -> u64 {
    string_bits(crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32) as usize)
}

fn names_live_object(bits: u64) -> bool {
    let addr = (bits & POINTER_MASK) as usize;
    build_valid_pointer_set().contains(&addr)
}

unsafe fn assert_string_slot(bits: u64, expected: &str) {
    assert_eq!(
        bits & TAG_MASK,
        STRING_TAG,
        "slot no longer holds a string: {bits:#x}"
    );
    assert!(
        names_live_object(bits),
        "slot names no live object: {bits:#x}"
    );
    assert_string_bytes(
        (bits & POINTER_MASK) as *const crate::StringHeader,
        expected.as_bytes(),
    );
}

#[derive(Clone, Copy, Debug)]
enum Collector {
    /// The copying nursery: moves every young survivor.
    CopyingMinor,
    /// Forced evacuation with the copying fast path declined: moves young
    /// survivors and repairs references in `rewrite_forwarded_references`.
    EvacuatingMinor,
    /// The idle compaction's minor: old-page defrag armed, copying declined.
    CompactingMinor,
    /// A budgeted (incremental) minor driven to completion step by step.
    BudgetedMinor,
    FullMarkSweep,
    /// A budgeted (incremental) full cycle, the idle reclaimer's path.
    BudgetedFull,
}

const EVERY_COLLECTOR: [Collector; 6] = [
    Collector::CopyingMinor,
    Collector::EvacuatingMinor,
    Collector::CompactingMinor,
    Collector::BudgetedMinor,
    Collector::FullMarkSweep,
    Collector::BudgetedFull,
];

fn collect(collector: Collector) {
    let trigger = || GcTriggerSnapshot::capture(GcTriggerKind::Direct);
    match collector {
        Collector::CopyingMinor => {
            let trace = collect_minor_trace(GcTriggerKind::Direct);
            assert!(trace.copying_nursery.eligible, "the copying minor must run");
        }
        Collector::EvacuatingMinor => {
            let _force = ForcedEvacuationTestGuard::on();
            gc_collect_compacting_minor(trigger());
        }
        Collector::CompactingMinor => {
            gc_collect_compacting_minor(trigger());
        }
        Collector::BudgetedMinor => {
            // Just due (`make_arena_trigger_due`), then suppressed again: the
            // test's `Quiet` guard owns the saved thresholds.
            let just_due = crate::arena::arena_total_bytes().saturating_sub(1);
            GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(just_due));
            let mut result = JsGcStepResult::default();
            assert_eq!(
                js_gc_step_work_units(1, &mut result),
                JS_GC_STEP_STATUS_ACTIVE
            );
            assert_eq!(result.collection_kind, GcCollectionKind::Minor.ffi_code());
            complete_budgeted_gc_cycle();
            GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(usize::MAX));
        }
        Collector::FullMarkSweep => {
            gc_collect_full_mark_sweep_with_trigger(trigger());
        }
        Collector::BudgetedFull => {
            // What the idle reclaimer arms (`budgeted_step_api`'s setup).
            let old_in_use = crate::arena::old_gen_in_use_bytes();
            GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
            GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
            let mut result = JsGcStepResult::default();
            assert_eq!(
                js_gc_step_work_units(1, &mut result),
                JS_GC_STEP_STATUS_ACTIVE
            );
            assert_eq!(result.collection_kind, GcCollectionKind::Full.ffi_code());
            complete_budgeted_gc_cycle();
        }
    }
}

struct Quiet {
    _copying: CopyingNurseryTestGuard,
    _triggers: GcTriggerThresholdTestGuard,
    _scan: ConservativeScanDisabledGuard,
}

fn quiet() -> Quiet {
    let quiet = Quiet {
        _copying: CopyingNurseryTestGuard::new(1),
        _triggers: GcTriggerThresholdTestGuard::suppress_automatic_triggers(),
        _scan: ConservativeScanDisabledGuard::new(),
    };
    register_runtime_handle_root_scanner_for_tests();
    quiet
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[test]
fn image_cells_are_classified_by_range_not_by_header_bits() {
    let _image = ImageFixture::take();
    let realm = crate::value::addr_class::realm::bounds();
    assert!(realm.0 < realm.1, "the perry_realm section must exist");
    assert!(crate::value::addr_class::realm::is_image_object(
        object_user()
    ));
    assert!(crate::value::addr_class::realm::is_image_object(
        closure_user()
    ));
    let mut walked = Vec::new();
    crate::value::addr_class::realm::for_each_cell(&mut |h| walked.push(h as usize));
    let closure_header = closure_user() - GC_HEADER_SIZE;
    let at = walked
        .iter()
        .position(|&h| h == closure_header)
        .expect("the walk finds the function cell");
    assert_eq!(
        walked.get(at + 1),
        Some(&(object_header() as usize)),
        "and the object cell after it"
    );
    unsafe {
        assert!(
            crate::value::addr_class::try_read_tracked_gc_header_of_type(
                object_user(),
                GC_TYPE_OBJECT
            )
            .is_some()
        );
        assert!(
            crate::value::addr_class::try_read_tracked_gc_header_of_type(
                closure_user(),
                GC_TYPE_CLOSURE
            )
            .is_some()
        );
        // The counterfeit carries the same header bytes outside the section.
        let fake = std::ptr::addr_of_mut!((*std::ptr::addr_of_mut!(COUNTERFEIT)).object) as usize;
        assert!(!crate::value::addr_class::realm::is_image_object(fake));
        assert!(crate::value::addr_class::try_read_tracked_gc_header(fake).is_none());
    }
    // A heap object is not an image object.
    let heap = crate::object::js_object_alloc(0, 1) as usize;
    assert!(!crate::value::addr_class::realm::is_image_object(heap));
}

#[test]
fn the_image_object_reads_through_its_function_cell() {
    let _image = ImageFixture::take();
    assert_eq!(call_value(), 42.0);
    assert_eq!(get("done"), crate::value::TAG_FALSE);
}

// ---------------------------------------------------------------------------
// (a) reads through the image across every collector
// ---------------------------------------------------------------------------

#[test]
fn reads_through_the_image_survive_every_collector() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _verify = VerifyEvacuationTestGuard::on();
    // A heap object that points INTO the image: the common direction.
    let holder = crate::object::js_object_alloc(0, 1);
    let scope = RuntimeHandleScope::new();
    let holder = scope.root_raw_mut_ptr(holder as *mut u8);
    crate::object::js_object_set_field_by_name(
        holder.get_raw_mut_ptr::<ObjectHeader>(),
        key("realm"),
        f64::from_bits(POINTER_TAG | object_user() as u64),
    );
    for collector in EVERY_COLLECTOR {
        collect(collector);
        unsafe {
            assert_eq!(
                (*object_header()).gc_flags,
                IMAGE_FLAGS,
                "{collector:?} touched the image header"
            );
        }
        assert_eq!(call_value(), 42.0, "after {collector:?}");
        let back = crate::object::js_object_get_field_by_name(
            holder.get_raw_mut_ptr::<ObjectHeader>(),
            key("realm"),
        )
        .bits();
        assert_eq!(
            back,
            POINTER_TAG | object_user() as u64,
            "heap→image edge after {collector:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// (b) a heap object stored into an image slot, across moving collections
// ---------------------------------------------------------------------------

fn store_and_collect_everywhere(store: impl Fn(u64)) {
    for collector in EVERY_COLLECTOR {
        let payload = format!("payload-{collector:?}");
        store(young_payload(&payload));
        assert!(
            crate::gc::image_cells::image_cell_remembered(object_header()),
            "the barrier must remember the image cell"
        );
        // Twice: a young child is copied or promoted by the first, then the
        // edge must survive as an image→old edge through the second.
        collect(collector);
        unsafe { assert_string_slot(get("done"), &payload) };
        collect(collector);
        unsafe { assert_string_slot(get("done"), &payload) };
        collect(Collector::FullMarkSweep);
        unsafe { assert_string_slot(get("done"), &payload) };
    }
}

#[test]
fn a_heap_child_stored_through_the_runtime_path_survives_every_collector() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _verify = VerifyEvacuationTestGuard::on();
    store_and_collect_everywhere(|bits| set("done", bits));
}

#[test]
fn a_heap_child_stored_like_generated_code_survives_every_collector() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _verify = VerifyEvacuationTestGuard::on();
    store_and_collect_everywhere(|bits| unsafe {
        // What an emitted store does: the slot write, the #7511 gate on the
        // live header (an image header has no ARENA bit, so it is taken),
        // then `js_write_barrier_slot` with the NaN-boxed parent.
        *done_slot() = bits;
        assert!(parent_flags_may_need_remembering(
            (*object_header()).gc_flags
        ));
        js_write_barrier_slot(POINTER_TAG | object_user() as u64, done_slot() as u64, bits);
    });
}

/// Knockouts: without remembering, root marking, the copying root pass or the
/// post-move rewrite, the image's heap child is lost. Detected by asking the
/// heap for the child, never by dereferencing it.
fn lost_without(clause: u8, collector: Collector) {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    set("done", young_payload("doomed"));
    let _knock = sabotage::Guard::arm(clause);
    collect(collector);
    collect(Collector::FullMarkSweep);
    assert!(
        !names_live_object(unsafe { *done_slot() }),
        "with clause {clause:#x} knocked out, {collector:?} should have lost the image's child"
    );
}

#[test]
fn knockout_image_root_marking_loses_the_child_in_a_full_collection() {
    lost_without(sabotage::ROOTS, Collector::FullMarkSweep);
}

#[test]
fn knockout_image_root_marking_loses_the_child_in_a_budgeted_full() {
    lost_without(sabotage::ROOTS, Collector::BudgetedFull);
}

#[test]
fn knockout_image_root_marking_loses_the_child_in_a_budgeted_minor() {
    lost_without(sabotage::ROOTS, Collector::BudgetedMinor);
}

/// The image object is the only carrier of its layout here: without the full
/// trace's visit, its shape's keys die and every lookup misses.
#[test]
fn knockout_image_root_visits_lose_the_image_shape_keys() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _knock = sabotage::Guard::arm(sabotage::ROOTS);
    collect(Collector::FullMarkSweep);
    assert_ne!(
        get("value") & TAG_MASK,
        POINTER_TAG,
        "the lookup should miss once the keys are gone"
    );
}

#[test]
fn knockout_the_copying_root_pass_loses_the_child_in_a_copying_minor() {
    lost_without(sabotage::COPY, Collector::CopyingMinor);
}

#[test]
fn knockout_the_post_move_rewrite_loses_the_child_in_an_evacuating_minor() {
    lost_without(sabotage::REWRITE, Collector::EvacuatingMinor);
}

#[test]
fn knockout_the_barrier_loses_the_child_in_a_copying_minor() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _knock = sabotage::Guard::arm(sabotage::BARRIER);
    set("done", young_payload("unremembered"));
    assert!(!crate::gc::image_cells::image_cell_remembered(
        object_header()
    ));
    collect(Collector::CopyingMinor);
    assert!(!names_live_object(unsafe { *done_slot() }));
}

// ---------------------------------------------------------------------------
// Lazy arming (#7187): stores before the first collection
// ---------------------------------------------------------------------------

fn store_before_arming_then_minor(knock: u8) -> bool {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    reset_barrier_arming_for_tests();
    let _knock = sabotage::Guard::arm(knock);
    set("done", young_payload("before-arming"));
    assert!(
        !crate::gc::image_cells::image_cell_remembered(object_header()),
        "an unarmed barrier records nothing"
    );
    collect(Collector::CopyingMinor);
    collect(Collector::FullMarkSweep);
    names_live_object(unsafe { *done_slot() })
}

#[test]
fn the_arming_reconstruct_recovers_image_stores_made_before_arming() {
    assert!(store_before_arming_then_minor(0));
}

#[test]
fn knockout_the_arming_reconstruct_loses_a_store_made_before_arming() {
    assert!(!store_before_arming_then_minor(sabotage::RECONSTRUCT));
}

// ---------------------------------------------------------------------------
// (c) adding a key: shape transition and overflow storage on the heap
// ---------------------------------------------------------------------------

#[test]
fn adding_keys_to_the_image_object_survives_every_collector() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _verify = VerifyEvacuationTestGuard::on();
    set("extra", young_payload("extra-0"));
    for i in 1..24 {
        let name = format!("k{i}");
        set(&name, young_payload(&format!("value-{i}")));
    }
    unsafe {
        assert_ne!(
            (*image()).object.parent_class_id,
            ITERATOR_RESULT,
            "the shape transitioned"
        );
    }
    for collector in EVERY_COLLECTOR {
        collect(collector);
        unsafe {
            assert_string_slot(get("extra"), "extra-0");
            for i in 1..24 {
                assert_string_slot(get(&format!("k{i}")), &format!("value-{i}"));
            }
        }
        assert_eq!(call_value(), 42.0, "value after {collector:?}");
    }
}

#[test]
fn a_key_added_with_a_primitive_keeps_the_image_shape_through_full_collections() {
    // No heap child at all: the cell is not remembered, only its shape is new.
    let _image = ImageFixture::take();
    let _quiet = quiet();
    set("n", 7.0f64.to_bits());
    for collector in [
        Collector::FullMarkSweep,
        Collector::BudgetedFull,
        Collector::FullMarkSweep,
    ] {
        collect(collector);
        assert_eq!(get("n"), 7.0f64.to_bits(), "after {collector:?}");
        assert_eq!(call_value(), 42.0);
    }
}

/// WRT step 2 (a): an image object on a static shape whose keys are image
/// data is visited by no full trace, and its keys resolve afterwards.
#[test]
fn an_image_object_on_a_seeded_shape_needs_no_visit_to_survive_full_collections() {
    // The seed first, as in a program (`js_shape_run_static_seed` precedes
    // every birth): the fixture layout's `{value, done}` then reaches the
    // seed's image cell as its canonical list instead of publishing a heap
    // list the seed would have to share.
    crate::test_support::bootstrap_test_thread();
    let word = super::image_seeds::install_value_done_seed(0x305);
    let _image = ImageFixture::take();
    let _quiet = quiet();
    unsafe { (*(object_user() as *mut ObjectHeader)).parent_class_id = word.0 };
    assert!(
        !crate::gc::image_cells::full_trace_roots().contains(&object_header()),
        "a cell on a static image-keyed shape is not a full-trace root"
    );
    for collector in [
        Collector::FullMarkSweep,
        Collector::BudgetedFull,
        Collector::FullMarkSweep,
    ] {
        collect(collector);
        assert_eq!(call_value(), 42.0, "after {collector:?}");
        assert_eq!(get("done"), crate::value::TAG_FALSE, "after {collector:?}");
    }
}

unsafe extern "C" {
    fn js_object_set_prototype_of(obj: f64, proto: f64) -> f64;
}

/// A transition that stores no heap child into the cell: a prototype change
/// keeps the keys and the inline slots and gives the object a new, dynamic
/// shape (the prototype identity lives on the shape record), so only the
/// full trace's shape selection keeps that record alive.
fn reprototype_image_object() {
    let proto = crate::object::js_object_alloc(0, 1) as u64;
    unsafe {
        js_object_set_prototype_of(
            f64::from_bits(POINTER_TAG | object_user() as u64),
            f64::from_bits(POINTER_TAG | proto),
        )
    };
    assert!(
        !crate::gc::image_cells::image_cell_remembered(object_header()),
        "a prototype change stores no heap child into the cell"
    );
}

/// WRT step 2 (b): an image object transitioned to a dynamic shape is a
/// full-trace root for its shape's sake; without that selection its shape
/// dies.
#[test]
fn an_image_object_on_a_dynamic_shape_is_a_full_trace_root() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    reprototype_image_object();
    assert!(
        crate::gc::image_cells::full_trace_roots().contains(&object_header()),
        "a cell on a dynamic shape is visited by every full trace"
    );
    for collector in [
        Collector::FullMarkSweep,
        Collector::BudgetedFull,
        Collector::FullMarkSweep,
    ] {
        collect(collector);
        assert_eq!(call_value(), 42.0, "after {collector:?}");
    }
}

#[test]
fn knockout_dynamic_shape_selection_loses_the_image_shape_keys() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    reprototype_image_object();
    let _knock = sabotage::Guard::arm(sabotage::DYNAMIC);
    collect(Collector::FullMarkSweep);
    assert_ne!(
        get("value") & TAG_MASK,
        POINTER_TAG,
        "with the shape selection knocked out, the dynamic shape should have died"
    );
}

// ---------------------------------------------------------------------------
// (d) the sabotage: a store that bypasses the barrier is caught by the gate
// ---------------------------------------------------------------------------

fn gate_missing_for_image() -> (usize, Option<usize>) {
    let stats = verify_old_to_young_edges_collect();
    (stats.missing_edges, stats.first_missing.map(|m| m.parent))
}

#[test]
fn a_barriered_store_satisfies_the_verify_gate() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    set("done", young_payload("barriered"));
    let (missing, _) = gate_missing_for_image();
    assert_eq!(missing, 0);
}

#[test]
fn a_store_that_bypasses_the_barrier_is_caught_by_the_verify_gate() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    unsafe { *done_slot() = young_payload("smuggled") };
    let (missing, parent) = gate_missing_for_image();
    assert_eq!(missing, 1, "the gate must see the unremembered image edge");
    assert_eq!(parent, Some(object_header() as usize));
}

#[test]
#[should_panic(expected = "old-young-edge-verifier failed")]
fn a_store_that_bypasses_the_barrier_aborts_the_verified_collection() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _verify = VerifyEvacuationTestGuard::on();
    unsafe { *done_slot() = young_payload("smuggled") };
    collect(Collector::CopyingMinor);
}

// ---------------------------------------------------------------------------
// (e) evacuation: the worker-realm mechanism
// ---------------------------------------------------------------------------

#[test]
fn evacuating_the_image_forwards_every_pointer_into_the_copy() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let evacuation = crate::gc::image_evacuate::evacuate_realm_image();
    assert!(evacuation.cell_count() >= 2);
    let leftovers = crate::gc::image_evacuate::words_pointing_into_image(&evacuation);
    assert!(
        leftovers.is_empty(),
        "words still naming the image: {leftovers:x?}"
    );
    let copy = evacuation
        .forward(object_user())
        .expect("the object cell was copied");
    let closure_copy = evacuation
        .forward(closure_user())
        .expect("the function cell was copied");
    assert!(!crate::value::addr_class::realm::is_image_object(copy));
    assert!(
        build_valid_pointer_set().contains(&copy),
        "the copy is an ordinary heap cell"
    );
    let scope = RuntimeHandleScope::new();
    let rooted = scope.root_raw_mut_ptr(copy as *mut u8);
    let read = |name: &str| {
        crate::object::js_object_get_field_by_name(
            rooted.get_raw_mut_ptr::<ObjectHeader>(),
            key(name),
        )
        .bits()
    };
    assert_eq!(
        read("value"),
        POINTER_TAG | closure_copy as u64,
        "the copy names the copied function"
    );
    // A patch on the copy is invisible to the image, and vice versa.
    crate::object::js_object_set_field_by_name(
        rooted.get_raw_mut_ptr::<ObjectHeader>(),
        key("done"),
        7.0,
    );
    assert_eq!(get("done"), crate::value::TAG_FALSE);
    set("done", crate::value::TAG_TRUE);
    assert_eq!(read("done"), 7.0f64.to_bits());
    // The copy is traced and movable like any heap cell.
    for collector in EVERY_COLLECTOR {
        collect(collector);
        let value = read("value");
        let call = crate::closure::js_closure_call0(
            (value & POINTER_MASK) as *const ClosureHeader,
            crate::closure::plain_call_receiver(),
        );
        assert_eq!(call, 42.0, "the copied function after {collector:?}");
        assert!(!crate::value::addr_class::realm::is_image_object(
            (value & POINTER_MASK) as usize
        ));
    }
}

#[test]
fn knockout_forwarding_leaves_words_naming_the_image() {
    let _image = ImageFixture::take();
    let _quiet = quiet();
    let _knock = sabotage::Guard::arm(sabotage::FORWARD);
    let evacuation = crate::gc::image_evacuate::evacuate_realm_image();
    let leftovers = crate::gc::image_evacuate::words_pointing_into_image(&evacuation);
    let copy = evacuation.forward(object_user()).unwrap();
    assert!(
        leftovers
            .iter()
            .any(|&(at, _)| at > copy && at < copy + OBJECT_CELL as usize),
        "the object copy's `value` slot must still name the image: {leftovers:x?}"
    );
}
