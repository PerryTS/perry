//! `fs/promises` settles through `promise_value`, which creates a promise
//! (running `promiseHooks` init hooks, which can collect) while it holds the
//! value. The test makes the hook run a moving minor, asserts that the value
//! moved, and poisons the from-space so a stale address cannot pass.
use super::*;
use crate::closure::ClosureHeader;
use crate::gc::RuntimeHandleScope;
use std::cell::Cell;

thread_local! {
    static HOOK_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn boxed<T>(ptr: *const T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

/// `promiseHooks.onInit`: collects on its first call only.
extern "C" fn collecting_init_hook(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _promise: f64,
    _parent: f64,
) -> f64 {
    if HOOK_CALLS.with(|count| count.replace(count.get() + 1)) == 0 {
        crate::gc::gc_collect_minor();
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `fs/promises` resolves with a value through `promise_value`. Creating its
/// promise runs a collecting init hook; the promise must hold the value's
/// live address.
#[test]
fn fs_promise_value_rereads_value_after_collecting_promise_hook() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::descriptor_state::scan_descriptor_roots_mut,
    );
    HOOK_CALLS.with(|count| count.set(0));
    let scope = RuntimeHandleScope::new();
    // The hook table holds the hook raw: promote it so it cannot move.
    let hook = scope.root_nanbox_f64(boxed(crate::closure::js_closure_alloc(
        crate::fn_info!(collecting_init_hook, 2; with_declared(2)),
        0,
    )));
    crate::gc::gc_collect_minor();
    crate::gc::gc_collect_minor();
    let hook_address = hook.get_nanbox_f64().to_bits();
    let value = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 1)));
    let before = value.get_nanbox_f64().to_bits();
    let stop = crate::v8::js_v8_promise_hooks_on_init(hook.get_nanbox_f64());
    let stop = scope.root_nanbox_f64(stop);

    let promise = promise_value(value.get_nanbox_f64());
    let promise = scope.root_nanbox_f64(promise);

    let stop = crate::value::js_nanbox_get_pointer(stop.get_nanbox_f64()) as *const ClosureHeader;
    crate::closure::js_closure_call0(stop, crate::closure::plain_call_receiver());
    assert_eq!(
        hook.get_nanbox_f64().to_bits(),
        hook_address,
        "the hook must not move"
    );
    assert!(HOOK_CALLS.with(Cell::get) >= 1, "the init hook must run");
    assert_ne!(
        value.get_nanbox_f64().to_bits(),
        before,
        "the value must move"
    );
    let promise = crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64())
        as *mut crate::promise::Promise;
    assert_eq!(crate::promise::js_promise_state(promise), 1);
    assert_eq!(
        crate::promise::js_promise_value(promise).to_bits(),
        value.get_nanbox_f64().to_bits(),
        "the promise must hold the value's live address"
    );
}
