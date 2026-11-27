use super::*;
use crate::promise::{Promise, PromiseState};

#[test]
fn first_pull_after_public_readable_ended_completes() {
    let scope = crate::gc::RuntimeHandleScope::new();
    // A net.Socket exposes EOF only through `readableEnded`, not node:stream's
    // hidden end slots; no `end` event will reach a listener attached now.
    let stream = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 1) as *const u8
    ));
    set_hidden_value(
        stream.get_nanbox_f64(),
        crate::runtime_state_key!(b"readableEnded"),
        f64::from_bits(TAG_TRUE),
    );
    let iterator =
        scope.root_nanbox_f64(build_readable_async_iterator(stream.get_nanbox_f64(), true));
    let next = get_hidden_value(
        iterator.get_nanbox_f64(),
        crate::runtime_state_key!(b"next"),
    )
    .expect("iterator.next");
    let promise = crate::closure::js_closure_call0(
        raw_ptr_from_value(next) as *const ClosureHeader,
        crate::closure::plain_call_receiver(),
    );
    assert_ne!(crate::promise::js_value_is_promise(promise), 0);
    let promise = raw_ptr_from_value(promise) as *const Promise;
    let result = unsafe {
        assert_eq!(
            (*promise).state,
            PromiseState::Fulfilled,
            "pull must settle"
        );
        (*promise).value
    };
    let done = get_hidden_value(result, hidden_key(b"done")).expect("done");
    assert_eq!(done.to_bits(), TAG_TRUE);
}
