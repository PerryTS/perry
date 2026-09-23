//! Regression tests for the #10969 review of canonical shape identity
//! (step 2.5). They began as the review's witnesses (findings 1, 2 and 3);
//! each now states the INVARIANT the runtime must keep, and asserts its
//! premises separately, so a run whose scenario did not happen fails on the
//! premise instead of passing.
//!
//! - Finding 1, generalized: after a minor, no published canonical list names
//!   a key at an address that key has moved away from — whichever producer
//!   built the list.
//! - Finding 2: a keys array handed to a class allocator is a raw copy of a
//!   root the allocator does not own. When the instance allocation itself
//!   collects, the instance must be born with the array's LIVE address.
//! - Finding 3: the class keys memo belongs to the agent that built it.

use super::super::*;
use super::runtime_roots::force_next_general_arena_alloc_slow;
use super::support::*;
use crate::array::ArrayHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::canonical_keys::{self, SharedLayout};
use crate::object::ObjectHeader;
use crate::value::POINTER_MASK;

fn nursery_key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set(handle: RuntimeHandle<'_>, key: *mut crate::StringHeader, value: f64) {
    handle.with_mut_ptr(|ptr| crate::object::js_object_set_field_by_name(ptr, key, value));
}

/// The address a handle roots right now, as an integer to compare. Never
/// dereferenced: a caller that needs the object reads it through the handle.
fn addr_of(handle: &RuntimeHandle<'_>) -> usize {
    handle.with_const_ptr(|ptr: *const u8| ptr as usize)
}

unsafe fn keys_of(handle: RuntimeHandle<'_>) -> *mut ArrayHeader {
    handle.with_const_ptr(|ptr: *const ObjectHeader| crate::object::object_keys(ptr).arr())
}

/// The production scanners for every table these scenarios touch. The
/// copying-nursery guard takes the thread's registry away, and a missing
/// rewrite would turn a hit into a miss (or a live array into a stale one)
/// for a reason that has nothing to do with the code under test.
fn register_object_model_scanners() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
}

/// Pin the conservative native-stack scan off so an allocation-point minor
/// may MOVE (see `AllocPointRelocationGuard` in
/// `runtime_roots/generator_attach_prototype.rs` for why both this and
/// `force_alloc_point_minor_pacing` are needed).
struct NoConservativeScan(Option<crate::gc::roots::ConservativeStackScanMode>);

impl NoConservativeScan {
    fn new() -> Self {
        Self(crate::gc::roots::set_conservative_stack_scan_override(
            Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
        ))
    }
}

impl Drop for NoConservativeScan {
    fn drop(&mut self) {
        crate::gc::roots::set_conservative_stack_scan_override(self.0);
    }
}

/// Make the next general-arena allocation collect.
fn arm_collection_on_next_block(trigger: &GcTriggerThresholdTestGuard) {
    force_next_general_arena_alloc_slow();
    trigger.make_arena_trigger_due();
}

// ---------------------------------------------------------------- finding 1

/// A heap-string slot must name a live, unforwarded string with one of the
/// expected names, at an address none of the tracked keys has moved away from.
unsafe fn assert_slot_names_a_live_key(
    list: *const ArrayHeader,
    index: usize,
    moved_from: &[(usize, usize)],
    label: &str,
) {
    let bits = crate::array::js_array_get(list, index as u32).bits();
    let v = crate::JSValue::from_bits(bits);
    if !v.is_string() {
        // SSO keys and tombstones carry no address to go stale.
        return;
    }
    let addr = (bits & POINTER_MASK) as usize;
    for &(before, after) in moved_from {
        assert!(
            addr != before || before == after,
            "INVARIANT ({label}): canonical list {list:p} slot {index} names {addr:#x}, the \
             address its key moved AWAY from (live copy at {after:#x})"
        );
    }
    let header = crate::value::addr_class::try_read_tracked_gc_header(addr).unwrap_or_else(|| {
        panic!("INVARIANT ({label}): list {list:p} slot {index} = {addr:#x} is not a tracked cell")
    });
    assert_eq!(
        (*header.as_ptr()).gc_flags & GC_FLAG_FORWARDED,
        0,
        "INVARIANT ({label}): list {list:p} slot {index} names a forwarded cell at {addr:#x}"
    );
    let s = addr as *const crate::StringHeader;
    let bytes = std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
    assert!(
        [b"gk_a".as_slice(), b"gk_b", b"gk_c"].contains(&bytes),
        "INVARIANT ({label}): list {list:p} slot {index} names {:?}",
        String::from_utf8_lossy(bytes)
    );
}

/// Finding 1, generalized. The review found a longlived trie prefix that kept
/// a young key's pre-move address after a minor (82961db74). `3613234a7`
/// removed that mechanism; this test guards the invariant rather than the
/// mechanism: every producer of canonical lists — the grow path, a class
/// declaration that canonicalizes THROUGH a grown prefix, a whole-list
/// canonicalization of a caller's list, an `extend_key` hit, and a receiver
/// that dies before the minor — leaves no published list naming a key at the
/// address it moved away from.
#[test]
fn no_published_canonical_list_names_a_key_at_its_pre_move_address() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let a = scope.root_string_ptr(nursery_key("gk_a"));
        let b = scope.root_string_ptr(nursery_key("gk_b"));
        let c = scope.root_string_ptr(nursery_key("gk_c"));
        let key = |h: &RuntimeHandle<'_>| addr_of(h);
        for (h, name) in [(&a, "a"), (&b, "b"), (&c, "c")] {
            assert!(
                crate::arena::pointer_in_nursery(key(h)),
                "premise: key {name} is young"
            );
        }

        // Grow path: [a], [a,b] with the young keys.
        let o1 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        set(o1, key(&a) as *mut _, 1.0);
        set(o1, key(&b) as *mut _, 2.0);
        // A class whose declared list extends the grown prefix: canonicalizes
        // through [a] and [a,b] and publishes [a,b,c].
        let packed = b"gk_a\0gk_b\0gk_c\0";
        let cls = scope.root_raw_mut_ptr(crate::object::js_build_class_keys_array(
            0x0C1_7731,
            3,
            packed.as_ptr(),
            packed.len() as u32,
        ));
        // Whole-list canonicalization of a caller-built list of young keys.
        let scratch = crate::array::js_array_alloc_with_length(2);
        let scratch = scope.root_raw_mut_ptr(scratch);
        for (i, h) in [&a, &c].into_iter().enumerate() {
            let bits = crate::value::js_nanbox_string(key(h) as i64).to_bits();
            scratch.with_mut_ptr(|arr: *mut ArrayHeader| {
                crate::array::js_array_set(arr, i as u32, crate::JSValue::from_bits(bits))
            });
        }
        // `canonicalize` roots its own operand across its allocation.
        let ac = scratch.with_const_ptr(|list: *const ArrayHeader| {
            canonical_keys::canonicalize(&SharedLayout::shape_cache_entry(), list, 2)
        });
        assert_eq!(ac.len(), 2, "premise: [a,c] canonicalized");
        let ac = scope.root_raw_mut_ptr(ac.as_ptr());
        // An `extend_key` hit: [a] + b is the grown [a,b].
        let prefix_a =
            canonical_keys::canonicalize(&SharedLayout::shape_cache_entry(), keys_of(o1), 1);
        let ab = canonical_keys::extend_key(
            &SharedLayout::shape_cache_entry(),
            prefix_a,
            key(&b) as *const crate::StringHeader,
        );
        assert_eq!(
            ab.as_ptr(),
            keys_of(o1),
            "premise: extend_key hit the grown [a,b]"
        );
        // A receiver that dies before the minor: [a,c,b]. Its handle scope
        // ends here, so nothing roots it at the collection.
        {
            let inner = RuntimeHandleScope::new();
            let dead = inner.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            set(dead, key(&a) as *mut _, 1.0);
            set(dead, key(&c) as *mut _, 2.0);
            set(dead, key(&b) as *mut _, 3.0);
        }
        let published_before = canonical_keys::published_lists_for_test().len();
        assert!(
            published_before >= 4,
            "premise: several lists published ({published_before})"
        );
        let before = [key(&a), key(&b), key(&c)];

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            trace.copying_nursery.copied_objects > 0,
            "premise: the minor copied"
        );
        let after = [key(&a), key(&b), key(&c)];
        let moved: Vec<(usize, usize)> = before.iter().copied().zip(after).collect();
        assert!(
            moved.iter().any(|&(x, y)| x != y),
            "premise: at least one key relocated ({moved:x?})"
        );

        let lists = canonical_keys::published_lists_for_test();
        eprintln!(
            "finding-1 invariant: {} published lists before the minor, {} after",
            published_before,
            lists.len()
        );
        for list in &lists {
            for i in 0..crate::array::js_array_length(*list) as usize {
                assert_slot_names_a_live_key(*list, i, &moved, "trie");
            }
        }
        // The live receivers' own lists, and a fresh receiver that grows
        // through the same keys and adopts whatever the trie hands it.
        for (label, list) in [
            ("o1", keys_of(o1)),
            ("class", addr_of(&cls) as *mut ArrayHeader),
            ("[a,c]", addr_of(&ac) as *mut ArrayHeader),
        ] {
            for i in 0..crate::array::js_array_length(list) as usize {
                assert_slot_names_a_live_key(list, i, &moved, label);
            }
        }
        let o2 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        set(o2, nursery_key("gk_a"), 1.0);
        set(o2, nursery_key("gk_b"), 2.0);
        set(o2, nursery_key("gk_c"), 3.0);
        let list = keys_of(o2);
        assert_eq!(
            crate::array::js_array_length(list),
            3,
            "premise: o2 has three keys"
        );
        for i in 0..3 {
            assert_slot_names_a_live_key(list, i, &moved, "fresh receiver");
        }
    }
}

// ---------------------------------------------------------------- finding 2

/// Finding 2, both compiled-class entry points. The keys array is young (the
/// canonical copy of the class list), rooted by the test the way the codegen
/// per-class global roots it, and passed RAW, the way generated code passes
/// the global's value. The instance allocation is the collection point.
fn class_inline_keys_birth_follows_the_move(stamped: bool) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = NoConservativeScan::new();
    let trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    canonical_keys::reset_for_test();
    let label = if stamped {
        "js_object_alloc_class_inline_keys_stamped"
    } else {
        "js_object_alloc_class_inline_keys"
    };
    let class_id = if stamped { 0x0C1_7741 } else { 0x0C1_7742 };
    let scope = RuntimeHandleScope::new();
    unsafe {
        let packed = b"ck_x\0ck_y\0";
        let keys = scope.root_raw_mut_ptr(crate::object::js_build_class_keys_array(
            class_id,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        ));
        let before = addr_of(&keys) as *mut ArrayHeader;
        assert!(
            crate::arena::pointer_in_nursery(before as usize),
            "premise ({label}): the class keys array is young"
        );
        let shape_id = crate::object::shapes::shape_id_for_keys_ensure(before, 2);

        arm_collection_on_next_block(&trigger);
        let obj = if stamped {
            crate::object::js_object_alloc_class_inline_keys_stamped(
                class_id, 0, 2, before, shape_id,
            )
        } else {
            crate::object::js_object_alloc_class_inline_keys(class_id, 0, 2, before)
        };
        let after = addr_of(&keys) as *mut ArrayHeader;
        assert_ne!(
            after, before,
            "premise ({label}): subject not live — the birth allocation did not move the \
             keys array, so this run proved nothing. Check the trigger arming."
        );
        let installed = crate::object::object_keys(obj).arr();
        assert_eq!(
            installed, after,
            "INVARIANT ({label}): the instance must be born with the keys array's LIVE \
             address; installed {installed:p}, live {after:p}, pre-move {before:p}"
        );
    }
}

#[test]
fn class_inline_keys_birth_installs_the_live_keys_array_when_its_allocation_collects() {
    class_inline_keys_birth_follows_the_move(false);
}

#[test]
fn class_inline_keys_stamped_birth_installs_the_live_keys_array_when_its_allocation_collects() {
    class_inline_keys_birth_follows_the_move(true);
}

/// Finding 2, `js_object_alloc_class_dynamic_parent`. The merged list is
/// already published young by a grow-path receiver, so the MISS path's
/// canonicalization hits it without allocating and the instance allocation is
/// the collection point. The HIT path is the same collection landing on a
/// cached young list.
#[test]
fn dynamic_parent_birth_installs_the_live_merged_keys_when_its_allocation_collects() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = NoConservativeScan::new();
    let trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    for (phase, parent_cid, child_cid, names) in [
        (
            "miss",
            0x0C1_7751u32,
            0x0C1_7752u32,
            ["dm_a", "dm_b", "dm_c"],
        ),
        (
            "hit",
            0x0C1_7753u32,
            0x0C1_7754u32,
            ["dh_a", "dh_b", "dh_c"],
        ),
    ] {
        unsafe {
            let parent_packed = format!("{}\0{}\0", names[0], names[1]);
            let parent = scope.root_raw_mut_ptr(crate::object::js_build_class_keys_array(
                parent_cid,
                2,
                parent_packed.as_ptr(),
                parent_packed.len() as u32,
            ));
            crate::object::register_class(child_cid, parent_cid);
            let own_packed = format!("{}\0", names[2]);
            // The merged list, published young by the grow path.
            let grown = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            for (i, name) in names.iter().enumerate() {
                set(grown, nursery_key(name), i as f64);
            }
            let merged_before = keys_of(grown);
            assert!(
                crate::arena::pointer_in_nursery(merged_before as usize),
                "premise ({phase}): the merged list is young"
            );
            if phase == "hit" {
                // Fill the shape cache without collecting.
                let _ = crate::object::js_object_alloc_class_dynamic_parent(
                    child_cid,
                    1,
                    own_packed.as_ptr(),
                    own_packed.len() as u32,
                );
            }
            arm_collection_on_next_block(&trigger);
            let inst = crate::object::js_object_alloc_class_dynamic_parent(
                child_cid,
                1,
                own_packed.as_ptr(),
                own_packed.len() as u32,
            );
            let merged_after = keys_of(grown);
            assert_ne!(
                merged_after, merged_before,
                "premise ({phase}): subject not live — the birth allocation did not move the \
                 merged list"
            );
            let installed = crate::object::object_keys(inst).arr();
            assert_eq!(
                installed, merged_after,
                "INVARIANT ({phase}): a dynamically-parented instance must be born with the \
                 merged list's LIVE address; installed {installed:p}, live {merged_after:p}, \
                 pre-move {merged_before:p}"
            );
            assert_eq!(
                crate::array::js_array_length(installed),
                3,
                "INVARIANT ({phase}): parent keys first, then own"
            );
            assert_ne!(
                addr_of(&parent),
                0,
                "premise ({phase}): the parent keys are rooted"
            );
        }
    }
}

// ---------------------------------------------------------------- finding 3

/// Finding 3. Two agents register the same class id, each with an array in
/// its own heap. Neither the other agent's registration nor its prune may
/// change what this agent's memo names — on the process-global table the
/// worker's registration replaced the main thread's entry, and a prune that
/// could not attribute the foreign address then dropped it.
#[test]
fn class_keys_memo_belongs_to_the_agent_that_built_it() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    const AGENT_MEMO_CLASS_ID: u32 = 0x0C1_7761;
    let packed = b"pa_x\0pa_y\0";
    let scope = RuntimeHandleScope::new();
    let mine = scope.root_raw_mut_ptr(crate::object::js_build_class_keys_array(
        AGENT_MEMO_CLASS_ID,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    ));
    let current = || addr_of(&mine);
    let registered = || {
        crate::object::registered_class_keys_array(AGENT_MEMO_CLASS_ID)
            .map(|(a, _)| a.arr() as usize)
    };
    assert_eq!(
        registered(),
        Some(current()),
        "premise: this agent registered its array"
    );

    let (tx, rx) = std::sync::mpsc::channel::<(usize, Option<usize>, Option<usize>)>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let packed = b"pa_x\0pa_y\0";
        let theirs = crate::object::js_build_class_keys_array(
            AGENT_MEMO_CLASS_ID,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        ) as usize;
        let seen = crate::object::registered_class_keys_array(AGENT_MEMO_CLASS_ID)
            .map(|(a, _)| a.arr() as usize);
        crate::object::alloc::prune_dead_class_keys_entries(&|_| false);
        let seen_after_prune = crate::object::registered_class_keys_array(AGENT_MEMO_CLASS_ID)
            .map(|(a, _)| a.arr() as usize);
        tx.send((theirs, seen, seen_after_prune))
            .expect("spawning thread is waiting");
        // Keep this thread's heap mapped until the spawning thread is done.
        let _ = done_rx.recv();
    });
    let (theirs, worker_saw, worker_saw_after_prune) = rx.recv().expect("worker reported");
    let after_worker = registered();
    crate::object::alloc::prune_dead_class_keys_entries(&|_| false);
    let after_own_prune = registered();
    let _ = done_tx.send(());
    worker.join().expect("worker thread panicked");

    assert_ne!(theirs, current(), "premise: the worker built its own array");
    assert_eq!(
        worker_saw,
        Some(theirs),
        "INVARIANT: the worker's memo names the worker's array"
    );
    assert_eq!(
        worker_saw_after_prune,
        Some(theirs),
        "INVARIANT: the worker's prune keeps the worker's live entry"
    );
    assert_eq!(
        after_worker,
        Some(current()),
        "INVARIANT: another agent registering class {AGENT_MEMO_CLASS_ID:#x} must not change this agent's \
         memo (it names {after_worker:x?}; this agent's array is {:#x}, the worker's {theirs:#x})",
        current()
    );
    assert_eq!(
        after_own_prune,
        Some(current()),
        "INVARIANT: this agent's prune keeps this agent's live entry"
    );
}

/// One backing serves a whole growth chain, and the collector moves it as
/// one array: after a minor every list on it resolves at the new address
/// with its own count, and the tip keeps growing in place there.
#[test]
fn a_moved_backing_keeps_every_list_on_it() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let o = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (i, name) in ["mv_0", "mv_1", "mv_2"].into_iter().enumerate() {
            set(o, nursery_key(name), i as f64);
        }
        let short = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        set(short, nursery_key("mv_0"), 0.0);
        let before = keys_of(o);
        assert_eq!(
            keys_of(short),
            before,
            "premise: [mv_0] and [mv_0..2] share a backing"
        );
        assert!(
            crate::arena::pointer_in_nursery(before as usize),
            "premise: the backing is young"
        );
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            trace.copying_nursery.copied_objects > 0,
            "premise: the minor copied"
        );
        let after = keys_of(o);
        assert_ne!(after, before, "premise: the minor moved the backing");
        assert_eq!(
            keys_of(short),
            after,
            "both lists must follow the one backing"
        );
        for (obj, count) in [(short, 1u32), (o, 3)] {
            let view = obj.with_const_ptr(|p: *const ObjectHeader| crate::object::object_keys(p));
            assert_eq!(view.count(), count);
            for i in 0..count {
                let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
                let expected = format!("mv_{i}");
                assert_eq!(
                    crate::string::js_string_key_bytes(view.get(i), &mut sso),
                    Some(expected.as_bytes())
                );
            }
        }
        // The trie followed the move: re-growing [mv_0] hits [mv_0, mv_1] at
        // the new address, and the tip grows in place there.
        let proof = SharedLayout::shape_cache_entry();
        let one = canonical_keys::canonicalize(&proof, after, 1);
        assert_eq!(
            (one.as_ptr(), one.len()),
            (after, 1),
            "the moved [mv_0] is known"
        );
        let two = canonical_keys::extend_key(&proof, one, nursery_key("mv_1"));
        assert_eq!((two.as_ptr(), two.len()), (after, 2));
        set(o, nursery_key("mv_3"), 3.0);
        assert_eq!(keys_of(o), after, "the moved tip grows in place");
    }
}

// ------------------------------------------------- weak-table death ordering

/// The step-2.5 weak tables (the class keys memo and the canonical trie)
/// REWRITE their keys on a move and never MARK them. A dead key therefore has
/// to leave both tables in the collection that frees its storage, before that
/// storage can be handed out again: a survivor would be walked by the next
/// cycle's rewrite pass as a forwarding header (#8040/#8174), and a recycled
/// ARRAY at that address would be served as a class's keys. This is the
/// evidence behind both `dead_owner:` verdicts in
/// `scripts/gc_rekeyed_key_tables.json`.
#[test]
fn a_dead_weak_keys_entry_is_dropped_before_its_storage_is_reused() {
    const DEAD_MEMO_CLASS_ID: u32 = 0x0C1_7762;
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_object_model_scanners();
    // The rewrite passes under test: each runs over the dead key before the
    // prune does, exactly as in production.
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::object::alloc::scan_class_keys_roots_mut);
    canonical_keys::reset_for_test();
    crate::arena::arena_reset_all_blocks_to_zero();

    let memo = || {
        crate::object::registered_class_keys_array(DEAD_MEMO_CLASS_ID)
            .map(|(a, _)| a.arr() as usize)
    };
    let trie_names = |addr: usize| {
        canonical_keys::published_lists_for_test()
            .into_iter()
            .any(|list| list as usize == addr)
    };

    // A young canonical list nothing roots, also remembered as a class's keys
    // (the memo stores canonical lists; `remember_class_keys` is its writer).
    let raw = crate::array::js_array_alloc(1);
    let raw = crate::array::js_array_push(raw, crate::JSValue::from_bits(crate::value::TAG_HOLE));
    let list = unsafe { canonical_keys::canonicalize(&SharedLayout::shape_cache_entry(), raw, 1) };
    let addr = list.addr();
    let capacity = unsafe { (*list.as_ptr()).capacity };
    crate::state::state()
        .object_hot
        .class_keys_by_id
        .borrow_mut()
        .insert(DEAD_MEMO_CLASS_ID, (addr, 1, 1));
    assert!(
        crate::arena::pointer_in_nursery(addr),
        "premise: the list is young"
    );
    assert!(trie_names(addr), "premise: the trie published the list");
    assert_eq!(memo(), Some(addr), "premise: the memo names the list");

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_eq!(
        trace.copying_nursery.copied_objects, 0,
        "premise: nothing reached the list, so the minor did not copy it"
    );
    assert_eq!(
        memo(),
        None,
        "INVARIANT: the class keys memo still names {addr:#x} after the minor that freed it"
    );
    assert!(
        !trie_names(addr),
        "INVARIANT: the canonical trie still names {addr:#x} after the minor that freed it"
    );

    // The storage really is reused -- otherwise the ordering above was never
    // at stake -- and the reused array is not served under the old entries.
    let mut reused = false;
    for _ in 0..64 {
        let next = crate::array::js_array_alloc_with_length_exact(capacity);
        reused |= next as usize == addr;
    }
    assert!(
        reused,
        "premise: the dead list's storage was handed out again"
    );
    assert_eq!(
        memo(),
        None,
        "INVARIANT: a recycled array served as a class's keys"
    );
    assert!(
        !trie_names(addr),
        "INVARIANT: a recycled array served as a canonical list"
    );

    crate::state::state()
        .object_hot
        .class_keys_by_id
        .borrow_mut()
        .remove(&DEAD_MEMO_CLASS_ID);
    canonical_keys::reset_for_test();
}
