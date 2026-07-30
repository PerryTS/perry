//! The shadow-stack layout contract between `perry-codegen` and
//! `perry-runtime` (#7086).
//!
//! Generated code writes GC roots *inline*: it computes the address of a
//! `ShadowEntry` from the `ShadowStackState` pointer `js_shadow_frame_enter`
//! returned, using hardcoded field offsets, and stores the two words itself.
//! `perry-codegen` deliberately does not depend on `perry-runtime`, so those
//! offsets exist twice.
//!
//! Nothing in either crate's own build fails if one copy moves. The result
//! would not be a compile error or a crash: the emitted code would write a
//! live root into the wrong word of the wrong structure, and the collector
//! would read a stale or bogus one — a silent wrong-answer bug of exactly the
//! kind this campaign keeps finding.
//!
//! `perry` depends on both crates, so this is where the two copies can be
//! compared. It runs in `cargo-test`, i.e. per PR, unlike the integration
//! suites under `crates/*/tests/`.

#[cfg(test)]
mod tests {
    use perry_codegen::expr_shadow_layout as cg;
    use perry_runtime::gc as rt;

    /// Every offset codegen bakes into the emitted store must equal the
    /// runtime's.
    ///
    /// Sabotage check: change any one constant in either crate and this fails.
    #[test]
    fn shadow_layout_contract_matches_the_runtime() {
        assert_eq!(
            cg::SHADOW_STATE_PTR_OFFSET as usize,
            rt::SHADOW_STATE_PTR_OFFSET,
            "ShadowStackState::ptr offset drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_STATE_LEN_OFFSET as usize,
            rt::SHADOW_STATE_LEN_OFFSET,
            "ShadowStackState::len offset drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_STATE_FRAME_TOP_OFFSET as usize,
            rt::SHADOW_STATE_FRAME_TOP_OFFSET,
            "ShadowStackState::frame_top offset drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_ENTRY_SIZE as usize,
            rt::SHADOW_ENTRY_SIZE,
            "ShadowEntry size drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_ENTRY_META_OFFSET as usize,
            rt::SHADOW_ENTRY_META_OFFSET,
            "ShadowEntry::meta offset drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_SLOT_ACTIVE_BIT as usize,
            rt::SHADOW_SLOT_ACTIVE_BIT,
            "slot liveness bit drifted between codegen and runtime"
        );
        assert_eq!(
            cg::SHADOW_STACK_HEADER_SLOTS as usize,
            rt::SHADOW_STACK_HEADER_SLOTS,
            "frame header size drifted; codegen recovers the frame handle as \
             frame_top - HEADER_SLOTS, so a mismatch unbalances every pop"
        );
    }

    /// Codegen indexes the entry buffer with a shift, so the entry size must
    /// stay a power of two and the shift must match it.
    #[test]
    fn shadow_entry_shift_matches_the_entry_size() {
        assert_eq!(
            cg::SHADOW_ENTRY_SIZE,
            1u64 << cg::SHADOW_ENTRY_SHIFT,
            "emitted `shl` amount does not match the entry size"
        );
    }

    /// The state pointer codegen caches per activation must be the address the
    /// runtime's own accessors use — that is the whole basis for the inline
    /// store touching the same memory as `js_shadow_slot_set`.
    ///
    /// Sabotage check: have `js_shadow_frame_enter` return anything other than
    /// the thread-local's address and this fails.
    #[test]
    fn frame_enter_returns_the_runtime_thread_local_address() {
        let state = rt::js_shadow_frame_enter(1);
        assert!(!state.is_null());
        assert_eq!(
            state as usize,
            rt::js_shadow_state_addr() as usize,
            "cached state pointer must name the runtime's own thread-local"
        );

        // And the handle codegen recovers from it must pop that frame.
        let frame_top = unsafe {
            *(state
                .cast::<u8>()
                .add(rt::SHADOW_STATE_FRAME_TOP_OFFSET)
                .cast::<usize>())
        };
        let depth_before = rt::shadow_stack_depth();
        rt::js_shadow_frame_pop((frame_top - rt::SHADOW_STACK_HEADER_SLOTS) as u64);
        assert_eq!(
            rt::shadow_stack_depth(),
            depth_before - 1,
            "handle recovered the way codegen recovers it must balance the frame"
        );
    }
}
