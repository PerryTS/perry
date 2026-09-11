use super::*;
use crate::promise::{Promise, PromiseState};

fn readable() -> f64 {
    register_arities();
    crate::child_process::cp_register_arities();
    crate::child_process::cp_build_readable()
}

fn pull(iterator: f64) -> f64 {
    let next = get_hidden_value(iterator, hidden_key(b"next")).expect("iterator.next");
    crate::closure::js_closure_call0(raw_ptr_from_value(next) as *const ClosureHeader)
}

fn result_field(promise: f64, name: &[u8]) -> f64 {
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
    get_hidden_value(result, hidden_key(name)).expect("iterator result field")
}

#[test]
fn late_child_iterator_observes_retained_eof() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(readable());
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), hidden_key(b"readableEnded"))
            .unwrap()
            .to_bits(),
        TAG_FALSE
    );
    // Same EOF helper called by the child-process reactor, without any
    // iterator/end listener in existence when the pipe closes.
    crate::child_process::cp_readable_end(stream.get_nanbox_f64());
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), hidden_key(b"readable"))
            .unwrap()
            .to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), hidden_key(b"readableEnded"))
            .unwrap()
            .to_bits(),
        TAG_TRUE
    );
    for _ in 0..2 {
        let iterator =
            scope.root_nanbox_f64(build_readable_async_iterator(stream.get_nanbox_f64(), true));
        let promise = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
        assert_eq!(
            result_field(promise.get_nanbox_f64(), b"done").to_bits(),
            TAG_TRUE
        );
    }
}

#[test]
fn child_eof_between_iterator_creation_and_first_pull_is_retained() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(readable());
    let iterator =
        scope.root_nanbox_f64(build_readable_async_iterator(stream.get_nanbox_f64(), true));
    crate::child_process::cp_readable_end(stream.get_nanbox_f64());
    let promise = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
    assert_eq!(
        result_field(promise.get_nanbox_f64(), b"done").to_bits(),
        TAG_TRUE
    );
}

#[test]
fn child_eof_settles_an_already_pending_empty_pull() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(readable());
    let iterator =
        scope.root_nanbox_f64(build_readable_async_iterator(stream.get_nanbox_f64(), true));
    let pending = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
    let promise = raw_ptr_from_value(pending.get_nanbox_f64()) as *const Promise;
    unsafe {
        assert_eq!((*promise).state, PromiseState::Pending);
    }
    crate::child_process::cp_readable_end(stream.get_nanbox_f64());
    assert_eq!(
        result_field(pending.get_nanbox_f64(), b"done").to_bits(),
        TAG_TRUE
    );
}

#[test]
fn child_eof_preserves_buffered_chunks_and_settles_live_pulls() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(readable());
    let iterator =
        scope.root_nanbox_f64(build_readable_async_iterator(stream.get_nanbox_f64(), true));
    let first = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
    let promise = raw_ptr_from_value(first.get_nanbox_f64()) as *const Promise;
    unsafe {
        assert_eq!((*promise).state, PromiseState::Pending);
    }
    crate::child_process::cp_emit(stream.get_nanbox_f64(), "data", &[11.0]);
    crate::child_process::cp_emit(stream.get_nanbox_f64(), "data", &[22.0]);
    crate::child_process::cp_readable_end(stream.get_nanbox_f64());
    assert_eq!(result_field(first.get_nanbox_f64(), b"value"), 11.0);
    assert_eq!(
        result_field(first.get_nanbox_f64(), b"done").to_bits(),
        TAG_FALSE
    );
    let second = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
    assert_eq!(result_field(second.get_nanbox_f64(), b"value"), 22.0);
    assert_eq!(
        result_field(second.get_nanbox_f64(), b"done").to_bits(),
        TAG_FALSE
    );
    let end = scope.root_nanbox_f64(pull(iterator.get_nanbox_f64()));
    assert_eq!(
        result_field(end.get_nanbox_f64(), b"done").to_bits(),
        TAG_TRUE
    );
}
