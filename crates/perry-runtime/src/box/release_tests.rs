//! Terminal-release and closure-visibility tests for async boxes, split out of
//! `box.rs` to keep it under the 2000-line cap (#8303 took it to 2228).

use super::*;

use super::*;

fn install_test_activation(activation: *mut AsyncBoxActivation) -> crate::promise::InlineTrap {
    crate::promise::INLINE_TRAP.with(|trap| {
        trap.replace(crate::promise::InlineTrap {
            trap_next: std::ptr::null_mut(),
            current_step: 0,
            box_activation: activation,
        })
    })
}

/// `BOX_ALLOC_COUNT` / `BOX_POOL_REUSE_COUNT` / `BOX_RELEASE_COUNT` are
/// process-global atomics, while the registries, quarantines and free
/// lists they describe are THREAD-LOCAL. Any test that asserts on a
/// counter *delta* is therefore not isolated by `test_clear_box_registry`
/// alone — a sibling test allocating on another harness thread lands in
/// the same atomics and moves the delta under it. Observed exactly that:
/// these tests pass under `--test-threads=1` and fail in parallel.
///
/// Serialise the counter-asserting tests against each other. Tests that
/// only assert on addresses and registry membership are thread-local and
/// need no lock.
fn counter_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A panicking test poisons the lock; the data is `()`, so recovering
    // is right — otherwise one failure cascades into spurious ones.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A released cell must be INERT: de-registered (reads `undefined`,
/// writes dropped), evicted from the positive cache, and parked exactly
/// once no matter how many times the terminal arm re-runs (#7933
/// follow-up; the stray-duplicate-resume path re-runs the release list).
#[test]
fn released_cell_is_inert_and_release_is_idempotent() {
    super::test_clear_box_registry();
    let ptr = js_box_alloc_bits(crate::value::TAG_TRUE as i64);
    assert!(is_registered_box_ptr(ptr));
    js_box_release(ptr);
    assert!(
        !is_registered_box_ptr(ptr),
        "released cell must be de-registered (and cache-evicted)"
    );
    assert_eq!(
        js_box_get_bits(ptr) as u64,
        crate::value::TAG_UNDEFINED,
        "released cell must read undefined"
    );
    js_box_set_bits(ptr, crate::value::TAG_TRUE as i64);
    assert_eq!(
        unsafe { (*ptr).value },
        crate::value::TAG_UNDEFINED,
        "write to a released cell must be dropped"
    );
    // Idempotence: a second release must not double-park the address —
    // a double-park would hand the same cell to two future activations.
    js_box_release(ptr);
    js_box_release(ptr);
    let parked =
        BOX_RELEASE_QUARANTINE.with(|q| q.borrow().iter().filter(|&&a| a == ptr as usize).count());
    assert_eq!(parked, 1, "double release must park exactly once");
}

/// Fallback reuse contract: an untracked released cell becomes allocatable
/// only AFTER the outermost-pump quarantine flush, and the reused cell is
/// re-registered with the fresh initial value.
#[test]
fn released_cell_is_reused_only_after_flush() {
    super::test_clear_box_registry();
    let first = js_box_alloc_bits(1.0f64.to_bits() as i64);
    js_box_release(first);
    // Not flushed yet: allocation must NOT reuse the parked cell.
    let second = js_box_alloc_bits(2.0f64.to_bits() as i64);
    assert_ne!(
        first as usize, second as usize,
        "quarantined cell must not be reused before the flush boundary"
    );
    flush_released_boxes();
    let third = js_box_alloc_bits(3.0f64.to_bits() as i64);
    assert_eq!(
        first as usize, third as usize,
        "flushed cell must be reused by the next allocation"
    );
    assert!(is_registered_box_ptr(third), "reused cell re-registers");
    assert_eq!(js_box_get_bits(third), 3.0f64.to_bits() as i64);
}

/// The #8208 floor-closing contract: terminal release alone does not make
/// a cell reusable while another queued resume still owns the activation.
/// The last queued/running-reference decrement publishes it immediately,
/// without waiting for the whole thread's task queue to drain.
#[test]
fn activation_cells_publish_at_its_reachability_zero() {
    test_clear_box_registry();
    let activation = new_async_box_activation(); // lifecycle owner
    retain_async_box_activation(activation); // currently running step
    retain_async_box_activation(activation); // duplicate queued step
    let previous = install_test_activation(activation);

    let released = js_box_alloc_bits(1.0f64.to_bits() as i64);
    js_box_release(released); // also drops the lifecycle owner

    let before_zero = js_box_alloc_bits(2.0f64.to_bits() as i64);
    assert_ne!(
        released, before_zero,
        "a queued resume still reaches the frame"
    );
    release_async_box_activation(activation); // running step exits
    let still_reachable = js_box_alloc_bits(3.0f64.to_bits() as i64);
    assert_ne!(
        released, still_reachable,
        "duplicate task still owns the frame"
    );

    release_async_box_activation(activation); // duplicate dispatch exits
    let after_zero = js_box_alloc_bits(4.0f64.to_bits() as i64);
    assert_eq!(
        released, after_zero,
        "the final decrement must publish immediately"
    );
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// #8213: a closure may outlive the async function that created it. Its
/// captured box remains readable after terminal release, and a child
/// closure created later increments that same cell's capture count. Only
/// death of the last such closure publishes the cell.
#[test]
fn escaped_closures_defer_activation_cell_publication_until_gc_death() {
    test_clear_box_registry();
    let activation = new_async_box_activation();
    retain_async_box_activation(activation); // running step
    let previous = install_test_activation(activation);

    let cell = js_box_alloc_bits(crate::value::JSValue::int32(41).bits() as i64);
    let uncaptured = js_box_alloc_bits(crate::value::JSValue::int32(40).bits() as i64);
    let outer = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(outer, 0, cell as i64);
    crate::closure::js_closure_set_box_capture_ptr(outer, 0, cell as i64);
    assert_eq!(
        crate::closure::box_capture_count(cell as usize),
        1,
        "a singleton cache hit may redeclare the same slot idempotently"
    );

    js_box_release(cell); // terminal lifecycle owner drops
    js_box_release(uncaptured);
    release_async_box_activation(activation); // running step exits
    assert!(is_registered_box_ptr(cell));
    assert!(
        !is_registered_box_ptr(uncaptured),
        "one captured cell must not retain the rest of the async frame"
    );
    assert_eq!(
        js_box_get_bits(cell) as u64,
        crate::value::JSValue::int32(41).bits(),
        "an escaped closure must still observe its captured value"
    );

    // Model invoking `outer` after the async activation returned: no
    // ambient async token exists, but the registered pending cell still
    // lets a nested closure acquire its own capture count.
    crate::promise::INLINE_TRAP.with(|trap| trap.set(crate::promise::InlineTrap::empty()));
    let child = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(child, 0, cell as i64);
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == outer as usize);
    assert!(
        is_registered_box_ptr(cell),
        "the child closure remains live"
    );

    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == child as usize);
    assert!(!is_registered_box_ptr(cell));
    let reused = js_box_alloc_bits(crate::value::JSValue::int32(42).bits() as i64);
    assert_eq!(cell, reused, "the last closure death publishes the cell");
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// The two halves of the full-trace ownership rule must move together:
/// the drained box stops being a global root, while a closure already
/// proven live exposes that box's JSValue payload as its external child.
/// Otherwise a self-cycle is immortal (first half missing) or an escaped
/// closure observes a collected payload (second half missing).
#[test]
fn full_trace_treats_drained_closure_boxes_as_ephemeron_edges() {
    test_clear_box_registry();
    let activation = new_async_box_activation();
    retain_async_box_activation(activation);
    let previous = install_test_activation(activation);
    let cell = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
    let closure_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
    js_box_set_bits(cell, closure_bits as i64);
    js_box_release(cell);
    release_async_box_activation(activation);
    assert!(is_registered_box_ptr(cell));

    crate::gc::begin_full_trace();
    let mut rooted = Vec::new();
    scan_box_roots(&mut |value| rooted.push(value.to_bits()));
    assert!(
        !rooted.contains(&closure_bits),
        "the drained box must not root its own closure during a full trace"
    );

    let header =
        unsafe { (closure as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader };
    let saved_flags = unsafe { (*header).gc_flags };
    unsafe { (*header).gc_flags |= crate::gc::GC_FLAG_MARKED };
    let slots = crate::gc::test_gc_rewrite_slot_addresses(closure as usize)
        .expect("closure rewrite descriptor");
    assert!(
        slots.contains(&(cell as usize)),
        "a marked closure must trace the drained box payload"
    );
    unsafe { (*header).gc_flags = saved_flags };
    crate::gc::finish_full_trace();

    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
    assert!(!is_registered_box_ptr(cell));
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// Pointer-shaped generic captures are common in runtime-created
/// closures. Even if their bits happen to equal a live box address, only
/// the compiler-declared setter may create a box lifetime edge.
#[test]
fn generic_pointer_capture_does_not_retain_an_async_box() {
    test_clear_box_registry();
    let activation = new_async_box_activation();
    retain_async_box_activation(activation);
    let previous = install_test_activation(activation);
    let cell = js_box_alloc_bits(crate::value::JSValue::int32(5).bits() as i64);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);

    crate::closure::js_closure_set_capture_ptr(closure, 0, cell as i64);
    assert_eq!(crate::closure::box_capture_count(cell as usize), 0);
    js_box_release(cell);
    release_async_box_activation(activation);
    assert!(!is_registered_box_ptr(cell));

    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// A closure created during an async step can also capture a mutable box
/// owned by an outer scope. The raw capture count must not make that box a
/// terminal-release candidate for the ambient activation.
#[test]
fn outer_scope_box_capture_is_not_bound_to_the_ambient_activation() {
    test_clear_box_registry();
    let outer_cell = js_box_alloc_bits(crate::value::JSValue::int32(7).bits() as i64);
    let activation = new_async_box_activation();
    retain_async_box_activation(activation);
    let previous = install_test_activation(activation);

    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(closure, 0, outer_cell as i64);
    assert_eq!(crate::closure::box_capture_count(outer_cell as usize), 1);

    let frame_cell = js_box_alloc_bits(crate::value::JSValue::int32(8).bits() as i64);
    js_box_release(frame_cell);
    release_async_box_activation(activation);
    assert!(!is_registered_box_ptr(frame_cell));
    assert!(is_registered_box_ptr(outer_cell));
    assert_eq!(
        js_box_get_bits(outer_cell) as u64,
        crate::value::JSValue::int32(7).bits()
    );
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

#[test]
fn closure_move_rekeys_the_capture_count_before_death_pruning() {
    test_clear_box_registry();
    let activation = new_async_box_activation();
    retain_async_box_activation(activation);
    let previous = install_test_activation(activation);
    let cell = js_box_alloc_bits(crate::value::JSValue::int32(9).bits() as i64);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
    js_box_release(cell);
    release_async_box_activation(activation);

    let moved = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::closure_box_captures_owner_moved(closure as usize, moved as usize);
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
    assert!(
        is_registered_box_ptr(cell),
        "the old address no longer owns the retain"
    );
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == moved as usize);
    assert!(!is_registered_box_ptr(cell));
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

#[test]
fn runtime_closure_clone_copies_exact_box_capture_edges() {
    test_clear_box_registry();
    let activation = new_async_box_activation();
    retain_async_box_activation(activation);
    let previous = install_test_activation(activation);
    let cell = js_box_alloc_bits(crate::value::JSValue::int32(10).bits() as i64);
    let source = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::js_closure_set_box_capture_ptr(source, 0, cell as i64);
    let cloned = crate::closure::js_closure_alloc(std::ptr::null(), 1);
    crate::closure::clone_closure_box_captures(source, cloned);
    assert_eq!(crate::closure::box_capture_count(cell as usize), 2);

    js_box_release(cell);
    release_async_box_activation(activation);
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == source as usize);
    assert!(
        is_registered_box_ptr(cell),
        "the clone retains its exact edge"
    );
    crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == cloned as usize);
    assert!(!is_registered_box_ptr(cell));
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// Pending-await thunks carry a raw malloc-token pointer so the moving GC
/// cannot invalidate it. Reusing that token must not make an old thunk
/// name a new activation; the captured generation is the discriminator.
#[test]
fn recycled_activation_token_rejects_a_stale_generation() {
    test_clear_box_registry();
    let first = new_async_box_activation();
    let first_id = async_box_activation_id(first);
    assert_eq!(find_async_box_activation(first, first_id), first);
    finish_async_box_activation(first);
    assert!(find_async_box_activation(first, first_id).is_null());

    let second = new_async_box_activation();
    let second_id = async_box_activation_id(second);
    assert_eq!(second, first, "the test must exercise token recycling");
    assert_ne!(second_id, first_id);
    assert!(find_async_box_activation(second, first_id).is_null());
    assert_eq!(find_async_box_activation(second, second_id), second);
    finish_async_box_activation(second);
}

/// Reachability is per activation, not a renamed global queue-empty gate:
/// B's completed frame is reusable while A still has a stale queued task.
#[test]
fn one_activation_does_not_quarantine_an_unrelated_completed_frame() {
    test_clear_box_registry();

    let activation_a = new_async_box_activation();
    retain_async_box_activation(activation_a); // running
    retain_async_box_activation(activation_a); // delayed duplicate
    let previous = install_test_activation(activation_a);
    let a = js_box_alloc_bits(10.0f64.to_bits() as i64);
    js_box_release(a);
    release_async_box_activation(activation_a); // leave duplicate alive

    let activation_b = new_async_box_activation();
    retain_async_box_activation(activation_b); // running
    install_test_activation(activation_b);
    let b = js_box_alloc_bits(20.0f64.to_bits() as i64);
    js_box_release(b);
    release_async_box_activation(activation_b); // B reaches zero

    let reused_b = js_box_alloc_bits(30.0f64.to_bits() as i64);
    assert_eq!(
        b, reused_b,
        "B must publish independently of A's queued task"
    );
    assert_ne!(a, reused_b, "A must remain parked");

    release_async_box_activation(activation_a);
    let reused_a = js_box_alloc_bits(40.0f64.to_bits() as i64);
    assert_eq!(a, reused_a, "A publishes when its own duplicate exits");
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// The pump's setjmp recovery must release the inner task reference that
/// `longjmp` skipped, while leaving a re-entrant caller's activation below
/// the saved depth untouched.
#[test]
fn exception_unwind_releases_only_this_pumps_activation_refs() {
    test_clear_box_registry();
    let base_depth = crate::promise::async_box_execution_ref_depth();

    let outer = new_async_box_activation();
    retain_async_box_activation(outer); // running owner
    crate::promise::push_async_box_execution_ref(outer);
    let previous = install_test_activation(outer);
    let outer_cell = js_box_alloc_bits(1.0f64.to_bits() as i64);
    js_box_release(outer_cell); // drop outer lifecycle owner

    let nested_depth = crate::promise::async_box_execution_ref_depth();
    let inner = new_async_box_activation();
    retain_async_box_activation(inner); // running owner skipped by longjmp
    crate::promise::push_async_box_execution_ref(inner);
    install_test_activation(inner);
    let inner_cell = js_box_alloc_bits(2.0f64.to_bits() as i64);
    js_box_release(inner_cell); // drop inner lifecycle owner

    crate::promise::unwind_async_box_execution_refs(nested_depth);
    let reused_inner = js_box_alloc_bits(3.0f64.to_bits() as i64);
    assert_eq!(
        inner_cell, reused_inner,
        "inner unwind must release its owner"
    );
    assert_ne!(
        outer_cell, reused_inner,
        "outer owner is below nested depth"
    );

    crate::promise::pop_async_box_execution_ref(outer);
    release_async_box_activation(outer);
    assert_eq!(
        crate::promise::async_box_execution_ref_depth(),
        base_depth,
        "test must restore the execution-ref stack"
    );
    let reused_outer = js_box_alloc_bits(4.0f64.to_bits() as i64);
    assert_eq!(outer_cell, reused_outer, "outer publishes at its own tail");
    crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
}

/// Generated async-step code reads the compiler-private control cells
/// with RAW loads (`load_async_i32_control_cell` /
/// `load_async_i1_control_cell`), never through the registry-checked
/// getters — so the PARKED VALUES are load-bearing: a stray duplicate
/// resume must observe `__gen_done == true` (the terminal short-circuit)
/// and, were it ever to read state, `-1` (no dispatch case matches).
#[test]
fn typed_control_cells_park_terminal_values() {
    super::test_clear_box_registry();
    let state = js_i32_box_alloc(7);
    let done = js_bool_box_alloc(0);
    js_i32_box_release(state);
    js_bool_box_release(done);
    assert_eq!(
        unsafe { (*state).value },
        -1,
        "parked i32 control cell must raw-read as -1 (no state)"
    );
    assert!(
        unsafe { (*done).value },
        "parked i1 control cell must raw-read as true (done)"
    );
    // And the checked getters treat them as not-a-box.
    assert_eq!(js_i32_box_get(state), 0);
    assert_eq!(js_bool_box_get(done), 0);
}

/// The intrusive free list must round-trip a WHOLE cohort, not just one
/// cell. Each free cell's own 8 bytes hold the link to the next, so a
/// mis-written link would either lose most of the pool (silently
/// reverting to `std::alloc` and re-growing the residue) or splice a cell
/// in twice and hand one address to two live activations.
///
/// Asserts all three: every cell comes back, each exactly once, and each
/// carries its own fresh value rather than a leftover link.
#[test]
fn the_intrusive_free_list_round_trips_a_whole_cohort() {
    let _guard = counter_guard();
    super::test_clear_box_registry();
    const N: usize = 512;
    let first: Vec<*mut Box> = (0..N)
        .map(|i| js_box_alloc_bits((i as f64).to_bits() as i64))
        .collect();
    let minted: std::collections::HashSet<usize> = first.iter().map(|p| *p as usize).collect();
    assert_eq!(minted.len(), N, "the fixture must mint N distinct cells");

    for p in &first {
        js_box_release(*p);
    }
    flush_released_boxes();

    let (a0, r0, _) = box_release_stats();
    let second: Vec<*mut Box> = (0..N)
        .map(|i| js_box_alloc_bits((1000.0 + i as f64).to_bits() as i64))
        .collect();
    let (a1, r1, _) = box_release_stats();
    assert_eq!(a1 - a0, N as u64, "second cohort allocates N cells");
    assert_eq!(
        r1 - r0,
        N as u64,
        "ALL N must come from the free list; {} fell through to std::alloc",
        N as u64 - (r1 - r0)
    );

    let reused: std::collections::HashSet<usize> = second.iter().map(|p| *p as usize).collect();
    assert_eq!(reused.len(), N, "an address was handed out twice");
    assert_eq!(
        reused, minted,
        "reused cells must be exactly the minted set"
    );

    for (i, p) in second.iter().enumerate() {
        assert_eq!(
            js_box_get_bits(*p),
            (1000.0 + i as f64).to_bits() as i64,
            "cell {i} kept a stale free-list link instead of its value"
        );
    }
    // Drained: the next allocation has to mint.
    let before = box_release_stats().1;
    let _fresh = js_box_alloc_bits(0);
    assert_eq!(
        box_release_stats().1,
        before,
        "the list was drained, so this must be a fresh std::alloc"
    );
}

/// perry#4898 discipline extends to release: a structurally-plausible
/// pointer that was never minted as a box must be a TOTAL no-op — no
/// deref, no park.
#[test]
fn foreign_pointer_release_is_a_total_noop() {
    super::test_clear_box_registry();
    static RODATA: [u64; 2] = [0xDEAD_BEEF, 0xFEED_FACE];
    let fake = (&RODATA[0] as *const u64) as *mut Box;
    js_box_release(fake);
    assert_eq!(RODATA[0], 0xDEAD_BEEF, "rodata must be untouched");
    let parked = BOX_RELEASE_QUARANTINE.with(|q| q.borrow().len());
    assert_eq!(parked, 0, "foreign pointer must not be parked");
}

/// THE #7933-follow-up regression gate, as a counter assertion (the leak
/// is behaviorally invisible — a test that merely runs to completion
/// cannot fail on it). Simulate N async-activation lifecycles (alloc a
/// frame of cells, release it at terminal, hit the drain boundary every
/// "turn"): the malloc-side residue — cells that cost a real
/// `std::alloc` allocation, `allocs - pool_reuses` — must stay bounded
/// by one turn's working set instead of growing linearly with N. Before
/// the release/reuse machinery existed, residue == every cell ever
/// allocated (~500 B/activation of cells + registry, 119 MB on
/// asyncpipe_big).
#[test]
fn completed_activation_residue_is_bounded_not_linear() {
    let _guard = counter_guard();
    super::test_clear_box_registry();
    const TURNS: usize = 100;
    const ACTIVATIONS_PER_TURN: usize = 20;
    // handle()-shaped frame: 3 JSValue cells + 1 i32 + 2 bool controls.
    const CELLS_PER_ACTIVATION: usize = 6;
    let (a0, r0, _) = box_release_stats();
    let mut distinct = std::collections::HashSet::new();
    for _ in 0..TURNS {
        for _ in 0..ACTIVATIONS_PER_TURN {
            let b1 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
            let b2 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
            let b3 = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
            let state = js_i32_box_alloc(0);
            let done = js_bool_box_alloc(0);
            let exec = js_bool_box_alloc(0);
            for b in [b1, b2, b3] {
                distinct.insert(b as usize);
            }
            distinct.insert(state as usize);
            distinct.insert(done as usize);
            distinct.insert(exec as usize);
            // Terminal state: release the whole frame.
            for b in [b1, b2, b3] {
                js_box_release(b);
            }
            js_i32_box_release(state);
            js_bool_box_release(done);
            js_bool_box_release(exec);
        }
        // Outermost microtask-pump boundary, task queue empty.
        flush_released_boxes();
    }
    let (a1, r1, _) = box_release_stats();
    // The counters are process-global; sibling tests on other threads
    // also allocate boxes, so assert lower bounds and give the residue
    // bound slack instead of demanding exact equality.
    let total_allocs = (a1 - a0) as usize;
    let residue = total_allocs.saturating_sub((r1 - r0) as usize);
    let own_allocs = TURNS * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
    assert!(
        total_allocs >= own_allocs,
        "every lifecycle allocates its frame ({total_allocs} < {own_allocs})"
    );
    // One turn's working set (the first turn mints real cells; every
    // later turn reuses them), plus generous slack for whatever the
    // parallel sibling tests allocate (they use a handful of cells
    // each). The pre-fix residue is TURNS * the per-turn bound, two
    // orders of magnitude past this.
    let bound = 4 * ACTIVATIONS_PER_TURN * CELLS_PER_ACTIVATION;
    assert!(
        residue <= bound,
        "malloc residue must be bounded by one turn's working set: \
         residue={residue} bound={bound} (linear would be {total_allocs})"
    );
    assert!(
        distinct.len() <= bound,
        "distinct cell addresses must be bounded (got {})",
        distinct.len()
    );
    // The registries hold only the (small) final turn's live set — the
    // linear-growth signature is gone from the scan population too.
    let reg_total = BOX_REGISTRY.with(|r| r.borrow().len())
        + I32_BOX_REGISTRY.with(|r| r.borrow().len())
        + BOOL_BOX_REGISTRY.with(|r| r.borrow().len());
    assert!(
        reg_total <= bound,
        "registry population must not scale with completed activations \
         (got {reg_total})"
    );
}
