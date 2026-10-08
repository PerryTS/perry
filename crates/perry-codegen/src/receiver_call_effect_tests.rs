//! Receiver proofs use the same memory effects as LLVM declarations.
use crate::block::{LlBlock, RegCounter};
use crate::types::{DOUBLE, I32};
use std::rc::Rc;

#[test]
fn readonly_calls_preserve_receiver_proofs_and_other_calls_invalidate_them() {
    for callee in [
        "js_is_truthy",
        "js_typed_i32_arg_guard",
        "js_typed_i32_arg_to_raw",
    ] {
        assert_eq!(
            crate::gc_call_effects::classify_direct_callee(callee),
            crate::gc_call_effects::GcCallEffect::CannotCollect,
            "proof preservation also needs a noncollecting call"
        );
        let counter = Rc::new(RegCounter::new());
        counter.push_byte_access_dirty_slot("%byte_state".into());
        counter.push_stable_packed_revalidation_slot("%packed_state".into());
        let mut block = LlBlock::new("entry", counter);
        block.call(I32, callee, &[(DOUBLE, "%value")]);
        let text = block.to_ir();
        assert!(
            !text.contains("store"),
            "{callee} has an audited no-write contract: {text}"
        );
        block.call_void("js_gc_loop_safepoint", &[]);
        let text = block.to_ir();
        assert!(text.contains("store i8 0, ptr %byte_state"));
        assert!(text.contains("store i1 1, ptr %packed_state"));
    }
}

#[test]
fn noncollecting_writers_and_unknown_calls_still_invalidate_receiver_proofs() {
    for callee in [
        "js_typed_feedback_numeric_array_index_get_guard",
        "js_dyn_index_get",
        "js_array_length",
        "unknown_callee",
    ] {
        let counter = Rc::new(RegCounter::new());
        counter.push_byte_access_dirty_slot("%byte_state".into());
        let mut block = LlBlock::new("entry", counter);
        block.call(DOUBLE, callee, &[]);
        assert!(block.to_ir().contains("store i8 0, ptr %byte_state"));
    }
    let counter = Rc::new(RegCounter::new());
    counter.push_byte_access_dirty_slot("%byte_state".into());
    let mut block = LlBlock::new("entry", counter);
    block.call_indirect(DOUBLE, "%callee", &[]);
    assert!(block.to_ir().contains("store i8 0, ptr %byte_state"));
}
