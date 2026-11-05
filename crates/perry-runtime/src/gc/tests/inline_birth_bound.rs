//! Idle-color inline permission closes and reopens with the collector.
use super::support::*;

#[test]
fn color_transitions_close_the_bound_and_refills_do_not_reopen_it() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let state = crate::arena::js_inline_arena_state();
        assert!((*state).size > (*state).offset);
        let prior = crate::gc::gc_birth_extra_flags();
        crate::gc::set_birth_extra_flags(crate::gc::GC_FLAG_MARKED);
        assert_eq!((*state).size, 0, "no emitted idle-color birth may fit");
        let array = crate::array::js_array_alloc(0);
        assert_ne!(
            (*crate::gc::header_from_user_ptr(array.cast())).gc_flags & crate::gc::GC_FLAG_MARKED,
            0,
            "the refused birth must take the runtime coloring path"
        );
        assert_eq!(
            (*state).size,
            0,
            "runtime allocation must keep the bound closed"
        );
        // A forced physical block refill must preserve the same permission.
        let _ = crate::arena::arena_alloc(crate::arena::BLOCK_SIZE, 8);
        assert_eq!((*state).size, 0);
        crate::gc::set_birth_extra_flags(prior);
        assert!(
            (*state).size >= (*state).offset,
            "idle bound must reopen on the actual block"
        );
        assert_eq!(
            crate::arena::js_inline_arena_state(),
            state,
            "no new allocator state is installed"
        );
    })
    .join()
    .unwrap();
}
