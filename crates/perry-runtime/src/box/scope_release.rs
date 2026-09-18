//! #10464: release the box cells an ordinary frame minted once that frame can
//! no longer name them.
//!
//! A variable box is minted per execution of the declaration of a captured and
//! reassigned binding, and every registered cell is a strong GC root
//! (`scan_box_roots_mut`). Before #10464 only a lowered plain-async activation
//! ever released its cells (#7933/#8208), so a synchronous function, method,
//! arrow, generator, or an `async` function without `await` kept one registered
//! root per boxed binding per call alive for the life of the thread, together
//! with everything that binding last pointed at.
//!
//! Codegen now names each frame-owned cell before every `ret` of the frame and
//! before a declaration inside a loop mints the next iteration's cell. The
//! frame is the holder that disappears at those points; every other holder is
//! a compiler-declared closure capture edge (`js_closure_set_box_capture_ptr`),
//! which is exactly the edge set #8303 counts. So:
//!
//! - a cell with no capture edge publishes immediately: de-registered,
//!   positive-cache-evicted, cleared, and pushed on its kind's free list;
//! - a captured cell stays registered and readable, and its capture-edge record
//!   is marked frame-released. It publishes when authoritative GC death
//!   pruning removes its last capture edge. Like a drained async terminal cell,
//!   a full trace reaches its payload only through live capturing closures (the
//!   ephemeron half), so a payload that references its own closure does not
//!   keep either alive.
//!
//! A running closure keeps its own edges: every capturing closure body spills
//! `%this_closure` into a frame-long root slot (#7055), so a cell a closure body
//! cached at entry cannot be published while that body is still executing.
//!
//! These entries are deliberately separate from `js_*box_release`, which parks
//! into the AMBIENT async activation: a synchronous callee running inside an
//! async step must not append its own cells to the caller activation's
//! contiguous terminal release range.

use super::*;

#[inline]
fn release_scope_cell(addr: usize, tag: usize, registered: bool) {
    // Not registered: a TAG_UNDEFINED slot whose declaration never ran, an
    // already-published cell, or a foreign address. All are no-ops, which is
    // what makes a repeated release unable to push one cell twice.
    if !registered {
        return;
    }
    // An async activation's terminal cell: its activation already decided.
    let async_pending = ASYNC_PENDING_RELEASES.with(|pending| {
        let pending = pending.borrow();
        !pending.is_empty() && pending.contains_key(&addr)
    });
    if async_pending {
        return;
    }
    match crate::closure::note_frame_released_cell(addr, tag) {
        crate::closure::FrameRelease::Uncaptured => {
            BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
            publish_box_cell(addr, tag);
        }
        crate::closure::FrameRelease::Deferred => {
            BOX_RELEASE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        crate::closure::FrameRelease::AlreadyReleased => {}
    }
}

/// The last capture edge of a frame-released cell disappeared (GC death
/// pruning): publish it for reuse.
pub(crate) fn publish_frame_released_cell(addr: usize, tag: usize) {
    publish_box_cell(addr, tag);
}

/// Release a JSValue box cell owned by the exiting (or re-entering) frame.
#[no_mangle]
pub extern "C" fn js_box_scope_release(ptr: *mut Box) {
    release_scope_cell(ptr as usize, ASYNC_RELEASE_JS, is_registered_box_ptr(ptr));
}

/// [`js_box_scope_release`] for the compiler-private i32 control cells of a
/// generator frame.
#[no_mangle]
pub extern "C" fn js_i32_box_scope_release(ptr: *mut I32Box) {
    release_scope_cell(
        ptr as usize,
        ASYNC_RELEASE_I32,
        is_registered_i32_box_ptr(ptr),
    );
}

/// [`js_box_scope_release`] for the compiler-private boolean control cells of
/// a generator frame.
#[no_mangle]
pub extern "C" fn js_bool_box_scope_release(ptr: *mut BoolBox) {
    release_scope_cell(
        ptr as usize,
        ASYNC_RELEASE_BOOL,
        is_registered_bool_box_ptr(ptr),
    );
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOX_SCOPE_RELEASE: extern "C" fn(*mut Box) = js_box_scope_release;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_I32_BOX_SCOPE_RELEASE: extern "C" fn(*mut I32Box) = js_i32_box_scope_release;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_BOOL_BOX_SCOPE_RELEASE: extern "C" fn(*mut BoolBox) = js_bool_box_scope_release;

#[cfg(test)]
mod tests {
    use super::*;

    fn int_bits(value: i32) -> i64 {
        crate::value::JSValue::int32(value).bits() as i64
    }

    fn free_list_contains(addr: usize) -> bool {
        let mut cursor = BOX_FREE_HEAD.with(std::cell::Cell::get);
        let mut steps = 0;
        while cursor != 0 && steps < 1 << 20 {
            if cursor == addr {
                return true;
            }
            cursor = unsafe { (cursor as *const usize).read() };
            steps += 1;
        }
        false
    }

    /// The common case: the frame was the cell's only holder. The cell is
    /// inert at once and the next allocation reuses it; repeating the release
    /// (a second `ret` path, a loop that exits right after re-entering) must
    /// not push it onto the free list a second time.
    #[test]
    fn uncaptured_cell_publishes_at_scope_exit_exactly_once() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(int_bits(7));
        js_box_scope_release(cell);
        assert!(!is_registered_box_ptr(cell), "published cell de-registers");
        assert_eq!(js_box_get_bits(cell) as u64, crate::value::TAG_UNDEFINED);
        js_box_scope_release(cell);
        js_box_scope_release(cell);

        let first = js_box_alloc_bits(int_bits(1));
        let second = js_box_alloc_bits(int_bits(2));
        assert_eq!(first, cell, "the published cell is reused immediately");
        assert_ne!(
            first, second,
            "a repeated release must not alias two live bindings onto one cell"
        );
        assert_eq!(js_box_get_bits(first), int_bits(1));
        assert_eq!(js_box_get_bits(second), int_bits(2));
    }

    /// A slot whose declaration never ran still holds TAG_UNDEFINED; codegen
    /// releases it unconditionally at `ret`. Foreign pointers are rejected
    /// by the same registry gate as every other box entry point.
    #[test]
    fn unminted_slot_values_and_foreign_pointers_are_no_ops() {
        test_clear_box_registry();
        let live = js_box_alloc_bits(int_bits(3));
        js_box_scope_release(crate::value::TAG_UNDEFINED as usize as *mut Box);
        js_box_scope_release(std::ptr::null_mut());
        static RODATA: [u64; 1] = [0xDEAD_BEEF];
        js_box_scope_release((&RODATA[0] as *const u64) as *mut Box);
        assert_eq!(RODATA[0], 0xDEAD_BEEF);
        assert!(is_registered_box_ptr(live));
        assert_eq!(js_box_get_bits(live), int_bits(3));
        assert!(!free_list_contains(live as usize));
    }

    /// `function counter() { let n = 0; return () => ++n; }`: the returned
    /// closure outlives the frame. Its cell stays readable and writable after
    /// scope exit, a closure created later from that closure adds its own
    /// edge, and only the last closure's death publishes the cell.
    #[test]
    fn escaped_closure_keeps_its_cell_until_gc_death() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(int_bits(0));
        let counter = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(counter, 0, cell as i64);
        js_box_scope_release(cell);

        assert!(is_registered_box_ptr(cell), "an escaped closure owns it");
        js_box_set_bits(cell, int_bits(1));
        assert_eq!(js_box_get_bits(cell), int_bits(1));
        assert!(!free_list_contains(cell as usize));
        js_box_scope_release(cell);
        assert!(
            crate::closure::frame_released_js_cell(cell as usize, ASYNC_RELEASE_JS),
            "a repeated scope release leaves the released state unchanged"
        );
        assert_eq!(crate::closure::box_capture_count(cell as usize), 1);

        let child = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(child, 0, cell as i64);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == counter as usize);
        assert!(is_registered_box_ptr(cell), "the child closure is live");
        assert_eq!(js_box_get_bits(cell), int_bits(1));

        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == child as usize);
        assert!(!is_registered_box_ptr(cell));
        assert!(free_list_contains(cell as usize));
        let reused = js_box_alloc_bits(int_bits(9));
        assert_eq!(reused, cell, "last closure death publishes the cell");
    }

    /// A scope-released captured cell must follow the #8303 full-trace rule:
    /// not a global root (else a payload that references its own closure is
    /// immortal), but traced through a closure the mark set proved live.
    #[test]
    fn scope_released_captured_cell_is_an_ephemeron_edge_in_a_full_trace() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
        let closure_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        js_box_set_bits(cell, closure_bits as i64);

        let mut minor_roots = Vec::new();
        scan_box_roots(&mut |value| minor_roots.push(value.to_bits()));
        assert!(
            minor_roots.contains(&closure_bits),
            "sabotage check: before release the registry roots the payload"
        );
        js_box_scope_release(cell);

        crate::gc::begin_full_trace();
        let mut rooted = Vec::new();
        scan_box_roots(&mut |value| rooted.push(value.to_bits()));
        assert!(
            !rooted.contains(&closure_bits),
            "a scope-released cell must not root its own closure in a full trace"
        );
        let header = unsafe {
            (closure as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
        };
        let saved_flags = unsafe { (*header).gc_flags };
        unsafe { (*header).gc_flags |= crate::gc::GC_FLAG_MARKED };
        let slots = crate::gc::test_gc_rewrite_slot_addresses(closure as usize)
            .expect("closure rewrite descriptor");
        unsafe { (*header).gc_flags = saved_flags };
        crate::gc::finish_full_trace();
        assert!(
            slots.contains(&(cell as usize)),
            "a marked closure must trace the scope-released cell's payload"
        );

        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        assert!(!is_registered_box_ptr(cell));
    }

    /// A minor walks ONLY the box young log. A full trace that stops rooting a
    /// released cell must therefore still LOG it while its payload is young,
    /// or the next minor has no root for a payload a live closure still reads.
    /// `PERRY_GC_PROTECT_FROMSPACE` caught exactly this as a stale from-space
    /// closure call; this test is its cheap, deterministic twin.
    #[test]
    fn a_full_trace_keeps_a_released_cell_in_the_minor_remembered_set() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, cell as i64);
        let payload = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        js_box_set_bits(cell, payload as i64);
        assert!(
            crate::gc::young_log::bits_are_minor_relevant(payload),
            "premise: the payload is a young object a minor must rewrite"
        );
        js_box_scope_release(cell);

        crate::gc::begin_full_trace();
        scan_box_roots(&mut |_| {});
        crate::gc::finish_full_trace();

        BOX_YOUNG_ROOTS.with(|log| {
            log.borrow()
                .debug_assert_logged(BOX_YOUNG_LOG_NAME, &relevant_box_roots())
        });
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
    }

    /// A synchronous callee running inside an async step must not park its
    /// cells into the caller's activation range (that is what
    /// `js_box_release` would do): the scope release is independent of the
    /// ambient activation, and the activation's own terminal release still
    /// works afterwards.
    #[test]
    fn scope_release_ignores_the_ambient_async_activation() {
        test_clear_box_registry();
        let activation = new_async_box_activation();
        retain_async_box_activation(activation);
        let previous = crate::promise::INLINE_TRAP.with(|trap| {
            trap.replace(crate::promise::InlineTrap {
                trap_next: std::ptr::null_mut(),
                current_step: 0,
                box_activation: activation,
            })
        });
        let callee_cell = js_box_alloc_bits(int_bits(4));
        js_box_scope_release(callee_cell);
        assert!(!is_registered_box_ptr(callee_cell));
        assert_eq!(
            unsafe { (*activation).release_start.get() },
            NO_RELEASE_RANGE,
            "the callee's cell must not enter the caller's release range"
        );

        let frame_cell = js_box_alloc_bits(int_bits(5));
        js_box_release(frame_cell);
        assert!(is_registered_box_ptr(frame_cell), "a step still owns it");
        release_async_box_activation(activation);
        assert!(!is_registered_box_ptr(frame_cell));
        crate::promise::INLINE_TRAP.with(|trap| trap.set(previous));
    }

    /// Generator frames also own compiler-private i32/bool control cells.
    /// Each kind publishes through its own registry, and a cell of one kind
    /// is never accepted by another kind's release entry.
    #[test]
    fn primitive_control_cells_release_through_their_own_registries() {
        test_clear_box_registry();
        let state = js_i32_box_alloc(3);
        let done = js_bool_box_alloc(1);
        let ordinary = js_box_alloc_bits(int_bits(6));

        js_i32_box_scope_release(ordinary.cast::<I32Box>());
        js_bool_box_scope_release(ordinary.cast::<BoolBox>());
        js_box_scope_release(state.cast::<Box>());
        assert!(is_registered_box_ptr(ordinary));
        assert!(is_registered_i32_box_ptr(state));

        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 1);
        crate::closure::js_closure_set_box_capture_ptr(closure, 0, done as i64);
        js_i32_box_scope_release(state);
        js_bool_box_scope_release(done);
        assert!(!is_registered_i32_box_ptr(state));
        assert!(is_registered_bool_box_ptr(done), "a live closure reads it");
        assert_eq!(js_bool_box_get(done), 1);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == closure as usize);
        assert!(!is_registered_bool_box_ptr(done));
        assert_eq!(js_i32_box_alloc(8), state, "i32 free list reuses it");
        assert_eq!(js_bool_box_alloc(0), done, "bool free list reuses it");
    }
}
