//! A pinned object is a root, marked and traced like any other object.
//!
//! A pin means "don't move, don't sweep" — never "already marked". Each birth
//! path below builds a parent whose only child is a string, then drops every
//! root but the pin (pinned) or keeps a shadow root (unpinned control), runs a
//! collection, reallocates same-size strings over any freed cell, and reads the
//! child back through the parent. A child reached only through an untraced
//! pinned parent is freed and its cell handed back: the read-back differs.

use super::super::*;
use super::support::{ptr_bits, CopyingNurseryTestGuard};
use crate::gc::pin::pinned_mark_sabotage;
use crate::object::ObjectHeader;

/// The copying-nursery isolation guard empties the scanner registry; install
/// the families these tests rely on.
///
/// The shape table's scanner is one of them. A store of a new key into an old
/// parent transitions it to a shape whose ordered keys array is born young, and
/// that keys word lives in the shape descriptor, outside the GC heap, so no
/// barrier can record it: `scan_shape_table_rekey_mut` is what keeps it across
/// a minor (see `gc/shape_keys_edge.rs`). Without it an old parent's slot
/// survives, forwarded, while the key that names it is lost, and the lookup
/// reads `undefined`.
fn pinned_guard() -> CopyingNurseryTestGuard {
    let guard = CopyingNurseryTestGuard::new(1);
    gc_register_named_mutable_root_scanner("pinned", crate::gc::pin::scan_pinned_object_roots_mut);
    gc_register_named_mutable_root_scanner("promise", promise_mutable_root_scanner);
    gc_register_named_mutable_root_scanner(
        "shape_table",
        crate::object::shapes::scan_shape_table_rekey_mut,
    );
    guard
}

const CHILD_TEXT: &[u8] = b"pinned-child-payload";
const OTHER_TEXT: &[u8] = b"overwrite-overwrite!";
const KEY: &[u8] = b"s";

#[derive(Clone, Copy, Debug)]
enum Birth {
    Young,
    BornTenured,
    Old,
    Malloc,
}

#[derive(Clone, Copy, Debug)]
enum Collection {
    Full,
    /// A direct minor: the copying nursery when eligible.
    Minor,
    /// An explicit `gc()` under forced evacuation: a moving minor.
    Evacuate,
}

/// An empty object of `INLINE_SLOT_FLOOR` slots from a non-young allocator,
/// initialised the way the born-old allocation paths initialise one.
unsafe fn raw_bag(birth: Birth) -> *mut ObjectHeader {
    let _nc = crate::gc::GcSuppressScope::new();
    let n = crate::object::INLINE_SLOT_FLOOR;
    let total = std::mem::size_of::<ObjectHeader>() + n * 8;
    let ptr = match birth {
        Birth::BornTenured => {
            crate::arena::arena_alloc_gc_old_born_tenured(total, 8, GC_TYPE_OBJECT)
        }
        Birth::Old => crate::arena::arena_alloc_gc_old(total, 8, GC_TYPE_OBJECT),
        Birth::Malloc => crate::gc::gc_malloc(total, GC_TYPE_OBJECT),
        Birth::Young => unreachable!(),
    } as *mut ObjectHeader;
    (*ptr).class_id = 0;
    (*ptr).parent_class_id = 0;
    (*ptr).meta = std::ptr::null_mut();
    let f = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut crate::value::JSValue;
    for i in 0..n {
        std::ptr::write(f.add(i), crate::value::JSValue::undefined());
    }
    crate::gc::layout_init_pointer_free(ptr as *mut u8);
    crate::object::shapes::birth_publish_object_shape(ptr, 0);
    ptr
}

fn string(text: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

fn slot_ptr<T>() -> *mut T {
    (js_shadow_slot_get(0) & POINTER_MASK) as *mut T
}

/// Hand any freed child-sized cell back to a different payload.
fn reuse_freed_cells() {
    for _ in 0..4096 {
        let _ = string(OTHER_TEXT);
    }
}

fn header_of(user: *mut u8) -> *mut GcHeader {
    unsafe { user.sub(GC_HEADER_SIZE) as *mut GcHeader }
}

/// A `birth` parent in shadow slot 0 holding a fresh young string under `KEY`
/// (stored through the runtime's own store path, write barrier included).
/// With `pin`, the parent is pinned and slot 0 cleared, so the pin is its only
/// root. Returns the parent.
fn build_parent(birth: Birth, pin: bool) -> *mut ObjectHeader {
    let parent = match birth {
        Birth::Young => crate::object::js_object_alloc(0, 2),
        _ => unsafe { raw_bag(birth) },
    };
    js_shadow_slot_set(0, ptr_bits(parent as usize));
    let child = string(CHILD_TEXT);
    let key = string(KEY);
    crate::object::js_object_set_field_by_name(
        slot_ptr(),
        key,
        crate::value::js_nanbox_string(child as i64),
    );
    let parent: *mut ObjectHeader = slot_ptr();
    if pin {
        unsafe { crate::gc::pin::js_gc_pin_user_ptr(parent as *mut u8) };
        js_shadow_slot_set(0, 0);
    }
    parent
}

/// Build, collect, read back. Returns whether the child survived intact.
///
/// The read-back sees a freed child only when its cell is handed back, and a
/// full collection force-marks every object of a recent block holding
/// anything live (`BLOCK_PERSIST_WINDOW`), so this is the behavioural check;
/// [`full_mark_reaches_child`] is the exact one for the full trace.
fn child_survives(birth: Birth, pin: bool, collection: Collection) -> bool {
    let _guard = pinned_guard();
    let parent = build_parent(birth, pin);
    match collection {
        Collection::Full => crate::gc::js_gc_collect(),
        Collection::Minor => {
            let _ = crate::gc::gc_collect_minor();
        }
        Collection::Evacuate => {
            let _force = super::support::ForcedEvacuationTestGuard::on();
            crate::gc::js_gc_collect();
        }
    }
    if pin {
        // A pin means "don't move": the parent is still where it was.
        let flags = unsafe { (*header_of(parent as *mut u8)).gc_flags };
        assert!(
            flags & GC_FLAG_FORWARDED == 0 && flags & GC_FLAG_PINNED != 0,
            "{birth:?} pinned parent was moved by a {collection:?} collection (flags={flags:#x})"
        );
    }
    reuse_freed_cells();
    let parent: *mut ObjectHeader = if pin { parent } else { slot_ptr() };
    let v = crate::object::js_object_get_field_by_name(parent, string(KEY));
    let got = (v.bits() & POINTER_MASK) as *const crate::StringHeader;
    let intact = !got.is_null() && crate::string::js_string_equals(got, string(CHILD_TEXT)) == 1;
    if pin {
        unsafe { crate::gc::unpin_object(header_of(parent as *mut u8)) };
    }
    intact
}

/// The full trace's own marking, stopped before block persistence and the
/// sweep: the registered root scanners (the pin scanner among them), then the
/// mark worklist. Returns
/// whether it marked the child a pinned `birth` parent holds.
fn full_mark_reaches_child(birth: Birth) -> bool {
    let _guard = pinned_guard();
    let parent = build_parent(birth, true);
    let v = crate::object::js_object_get_field_by_name(parent, string(KEY));
    let child = (v.bits() & POINTER_MASK) as usize;
    assert_ne!(child, 0, "premise: the child was stored");
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid_ptrs);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);
    let parent_marked = unsafe { (*header_of(parent as *mut u8)).gc_flags } & GC_FLAG_MARKED != 0;
    let child_marked = unsafe { (*header_of(child as *mut u8)).gc_flags } & GC_FLAG_MARKED != 0;
    clear_marks();
    clear_mark_seeds();
    unsafe { crate::gc::unpin_object(header_of(parent as *mut u8)) };
    parent_marked && child_marked
}

#[test]
fn full_mark_traces_through_every_pinned_birth() {
    for birth in [Birth::Young, Birth::BornTenured, Birth::Old, Birth::Malloc] {
        assert!(
            full_mark_reaches_child(birth),
            "the full trace did not mark and trace through a pinned {birth:?} parent"
        );
    }
}

fn assert_child_survives(birth: Birth, collection: Collection) {
    for pin in [false, true] {
        assert!(
            child_survives(birth, pin, collection),
            "{birth:?} parent (pinned={pin}) lost its child across a {collection:?} collection"
        );
    }
}

macro_rules! birth_matrix {
    ($($name:ident: $birth:expr, $collection:expr;)*) => {$(
        #[test]
        fn $name() {
            assert_child_survives($birth, $collection);
        }
    )*};
}

birth_matrix! {
    young_parent_full: Birth::Young, Collection::Full;
    young_parent_minor: Birth::Young, Collection::Minor;
    young_parent_evacuate: Birth::Young, Collection::Evacuate;
    born_tenured_parent_full: Birth::BornTenured, Collection::Full;
    born_tenured_parent_minor: Birth::BornTenured, Collection::Minor;
    born_tenured_parent_evacuate: Birth::BornTenured, Collection::Evacuate;
    old_parent_full: Birth::Old, Collection::Full;
    old_parent_minor: Birth::Old, Collection::Minor;
    old_parent_evacuate: Birth::Old, Collection::Evacuate;
    malloc_parent_full: Birth::Malloc, Collection::Full;
    malloc_parent_minor: Birth::Malloc, Collection::Minor;
    malloc_parent_evacuate: Birth::Malloc, Collection::Evacuate;
}

// ---------------------------------------------------------------------------
// Promise reactions: the real-code shape of the bug.
// ---------------------------------------------------------------------------

std::thread_local! {
    static SETTLED_WITH: std::cell::Cell<f64> = const { std::cell::Cell::new(f64::NAN) };
}

extern "C" fn record_cb(_c: *const crate::closure::ClosureHeader, v: f64) -> f64 {
    SETTLED_WITH.with(|s| s.set(v));
    v
}

extern "C" fn overwrite_cb(_c: *const crate::closure::ClosureHeader, v: f64) -> f64 {
    SETTLED_WITH.with(|s| s.set(-1.0));
    v
}

fn churn_garbage(bytes: usize) {
    let chunk = [b'g'; 200];
    let mut done = 0;
    while done < bytes {
        let _ = crate::string::js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
        done += 224;
    }
}

/// A promise whose `then` reaction is reachable only through the promise, and
/// the promise only through its pin. Ages the reaction out of the full trace's
/// recent-block window, runs `fulls` full collections, then settles it.
/// Returns whether the reaction ran with the settled value.
fn pinned_promise_reaction_runs(cross_thread: bool, fulls: usize) -> bool {
    let _guard = pinned_guard();
    SETTLED_WITH.with(|s| s.set(f64::NAN));
    let p = if cross_thread {
        crate::promise::js_promise_new_cross_thread()
    } else {
        crate::promise::js_promise_new()
    };
    js_shadow_slot_set(0, ptr_bits(p as usize));
    let cb = crate::closure::js_closure_alloc(record_cb as *const u8, 0);
    let _derived = crate::promise::js_promise_then(slot_ptr(), cb, std::ptr::null());
    let p: *mut crate::promise::Promise = slot_ptr();
    if !cross_thread {
        // The native-resolution pin of perry-stdlib's async_bridge.
        unsafe { crate::gc::pin_object(header_of(p as *mut u8)) };
    }
    js_shadow_slot_set(0, 0);
    for _ in 0..fulls {
        churn_garbage(16 << 20);
        crate::gc::js_gc_collect();
    }
    // A freed reaction cell is handed back to a closure that records -1.
    for _ in 0..4096 {
        let _ = crate::closure::js_closure_alloc(overwrite_cb as *const u8, 0);
    }
    if !cross_thread {
        unsafe { crate::gc::unpin_object(header_of(p as *mut u8)) };
    }
    crate::promise::js_promise_resolve(p, 42.0);
    crate::promise::js_promise_run_microtasks();
    SETTLED_WITH.with(|s| s.get()) == 42.0
}

#[test]
fn cross_thread_promise_reaction_survives_full_collections() {
    assert!(
        pinned_promise_reaction_runs(true, 4),
        "the cross-thread promise's reaction closure was freed while the promise was pinned"
    );
}

/// The async_bridge promise is pinned in Eden. It used to survive only because
/// the full trace force-marked every object of a recent general block holding
/// a pinned header; aged out of that window, the pin alone must keep its
/// reaction.
#[test]
fn aged_bridge_promise_reaction_survives_full_collections() {
    assert!(
        pinned_promise_reaction_runs(false, 4),
        "the async_bridge promise's reaction closure was freed while the promise was pinned"
    );
}

// ---------------------------------------------------------------------------
// Sabotage: each arm of the fix, removed alone, must turn the tests red.
// ---------------------------------------------------------------------------

/// (A) the mark entries treat a pinned header as already marked again.
#[test]
fn sabotage_pinned_counts_as_marked_frees_the_child() {
    let _sabotage = pinned_mark_sabotage::Guard::new(true, false);
    for birth in [Birth::Young, Birth::BornTenured, Birth::Old, Birth::Malloc] {
        assert!(!full_mark_reaches_child(birth), "{birth:?}");
    }
    // The copying minor's own mark entry for a malloc or longlived object.
    assert!(!child_survives(Birth::Malloc, true, Collection::Minor));
    assert!(!pinned_promise_reaction_runs(true, 4));
}

/// (B) the pinned-root scan is dropped.
#[test]
fn sabotage_no_pinned_roots_frees_the_child() {
    let _sabotage = pinned_mark_sabotage::Guard::new(false, true);
    for birth in [Birth::Young, Birth::BornTenured, Birth::Old, Birth::Malloc] {
        assert!(!full_mark_reaches_child(birth), "{birth:?}");
    }
    assert!(!child_survives(Birth::Malloc, true, Collection::Minor));
    assert!(!pinned_promise_reaction_runs(true, 4));
}
