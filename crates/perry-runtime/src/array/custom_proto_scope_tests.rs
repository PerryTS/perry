//! #10593: `Object.setPrototypeOf` on ONE ordinary array must not stand the
//! index fast path down for every OTHER array in the process.
//!
//! It used to flip the process-wide `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`
//! byte, which every generated element guard loads. From then on every element
//! store in the program went out of line and took the full old-to-young write
//! barrier (1.51 G -> 50.0 G retired instructions on the issue's fixture). The
//! fact is now carried on the retargeted array's own header
//! (`GC_ARRAY_CUSTOM_PROTO`), which the guards test beside the byte.
//!
//! These assert on the byte, the bit and the guard predicate rather than on a
//! behavioural result: the results were correct before the fix too — only the
//! cost was wrong — so a behavioural assertion cannot see a regression here.

use super::*;
use std::sync::atomic::Ordering;

/// Hold the typed-feedback lock (every test touching the latch/byte does) and
/// put both process-wide facts back the way this test found them.
struct LatchRestore {
    _lock: std::sync::MutexGuard<'static, ()>,
    latch: bool,
    invalidated: u8,
}

impl LatchRestore {
    fn clear() -> Self {
        let _lock = crate::typed_feedback::typed_feedback_test_lock();
        Self {
            _lock,
            latch: crate::object::prototype_chain::test_swap_array_static_proto_recorded(false),
            invalidated: test_swap_array_index_fast_path_invalidated(0),
        }
    }
}

impl Drop for LatchRestore {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.latch);
        test_swap_array_index_fast_path_invalidated(self.invalidated);
    }
}

fn reserved_of(arr: *const ArrayHeader) -> u16 {
    unsafe { (*crate::gc::header_from_trusted_user_ptr(arr.cast()))._reserved }
}

fn filled_array(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for &value in values {
        arr = js_array_push_f64(arr, value);
    }
    arr
}

fn set_prototype(owner: usize, proto: usize) {
    crate::object::js_object_set_prototype_of(
        crate::value::js_nanbox_pointer(owner as i64),
        crate::value::js_nanbox_pointer(proto as i64),
    );
}

fn forget_owners(owners: &[usize]) {
    crate::object::prototype_chain::prune_dead_object_prototype_owners(&|owner| {
        owners.contains(&owner)
    });
}

#[test]
fn retargeting_one_array_keeps_every_other_array_on_the_fast_path() {
    let _serialized = test_serialize();
    let _restore = LatchRestore::clear();

    let retargeted = filled_array(&[1.0, 2.0, 3.0]);
    let bystander = filled_array(&[4.0, 5.0, 6.0]);
    assert!(
        !array_index_fast_path_invalid_for(reserved_of(retargeted))
            && !array_index_fast_path_invalid_for(reserved_of(bystander)),
        "premise: two fresh plain arrays start on the fast path"
    );

    let proto = crate::object::js_object_alloc(0, 1) as usize;
    set_prototype(retargeted as usize, proto);
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(retargeted as usize),
        Some(crate::value::js_nanbox_pointer(proto as i64).to_bits()),
        "premise: the custom prototype was actually recorded"
    );

    assert_eq!(
        PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed),
        0,
        "one retargeted array must not flip the process-wide byte every \
         generated element guard loads (#10593)"
    );
    assert_ne!(
        reserved_of(retargeted) & crate::gc::GC_ARRAY_CUSTOM_PROTO,
        0,
        "the retargeted array must carry its own custom-prototype bit"
    );
    assert!(
        array_index_fast_path_invalid_for(reserved_of(retargeted)),
        "the retargeted array itself must leave the fast path: its holes and \
         out-of-bounds reads now walk the custom chain"
    );
    assert!(
        !array_index_fast_path_invalid_for(reserved_of(bystander)),
        "an unrelated array must stay on the fast path"
    );
    assert!(
        crate::object::prototype_chain::array_static_proto_recorded(),
        "the slow paths' side-table latch is still armed"
    );

    // The bit rides the growth replacement allocation, like the registry
    // entry does (`js_array_grow` copies `_reserved`).
    let old_owner = retargeted as usize;
    let mut grown = retargeted;
    for i in 0..64 {
        grown = js_array_push_f64(grown, i as f64);
    }
    assert_ne!(grown as usize, old_owner, "premise: the array really grew");
    assert!(
        array_index_fast_path_invalid_for(reserved_of(grown)),
        "growth must not drop the retargeted array's custom-prototype bit"
    );

    forget_owners(&[old_owner, grown as usize]);
}

#[test]
fn retargeting_a_lazy_json_array_still_invalidates_globally() {
    let _serialized = test_serialize();
    let _restore = LatchRestore::clear();

    let input = b"[1,2,3]";
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(
            tape,
            0,
            crate::json_tape::count_array_length(tape, 0),
            text,
        )
    })
    .expect("valid JSON should build a tape") as usize;
    assert_eq!(
        unsafe { (*crate::gc::header_from_trusted_user_ptr(lazy as *const u8)).obj_type },
        crate::gc::GC_TYPE_LAZY_ARRAY,
        "premise: the subject is a lazy JSON array"
    );

    let proto = crate::object::js_object_alloc(0, 1) as usize;
    set_prototype(lazy, proto);
    assert_eq!(
        PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED.load(Ordering::Relaxed),
        1,
        "a lazy array's materialized storage is a separate allocation without \
         the owner's bit, so its retarget must keep the conservative global byte"
    );

    forget_owners(&[lazy]);
}
