//! #11471 thread-exit regression tests (ui group).
//!
//! perry/ui, perry/tui, onFrame and geisterhand keep process-global tables of
//! NaN-boxed JS values and raw closure pointers. None of their entry points
//! is restricted to the main thread, so a thread can leave its own heap
//! values there; each test checks that the thread's exit takes them out.
//!
//! Not covered here: `plugin::REGISTRY` (perry-runtime feature `full`, which
//! perry-stdlib's default test build does not enable) and
//! `media_playback::MEDIA_STATE_INBOX` (feature `ohos-napi`, HarmonyOS only).

use perry_runtime::closure::js_closure_alloc;
use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::{ArrayHeader, ClosureHeader, JSValue};

extern "C" fn thunk0(_closure: *const ClosureHeader, _this: perry_runtime::closure::JsThis) -> f64 {
    0.0
}

extern "C" fn thunk2(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _a: f64,
    _b: f64,
) -> f64 {
    0.0
}

const UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;

fn string_value(s: &str) -> f64 {
    let ptr = perry_runtime::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn closure_bits(closure: *const ClosureHeader) -> u64 {
    JSValue::pointer(closure as *const u8).bits()
}

fn array_bits(array: *const ArrayHeader) -> u64 {
    JSValue::pointer(array as *const u8).bits()
}

#[test]
fn thread_exit_releases_the_threads_geisterhand_callbacks_and_queued_actions() {
    use perry_runtime::geisterhand_registry as g;
    const HANDLE: i64 = 0x1147_1001;

    let (queued_arg, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure =
            scope.root_raw_mut_ptr(js_closure_alloc(perry_runtime::fn_info!(thunk0, 0), 0));
        let arg = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let closure_f64 = f64::from_bits(closure_bits(closure.get_raw_mut_ptr::<ClosureHeader>()));
        let arg_f64 = f64::from_bits(array_bits(arg.get_raw_mut_ptr::<ArrayHeader>()));
        g::perry_geisterhand_register(
            HANDLE,
            g::WIDGET_BUTTON,
            g::CB_ON_CLICK,
            closure_f64,
            std::ptr::null(),
        );
        g::perry_geisterhand_queue_action1(closure_f64, arg_f64);
        let alive = g::perry_geisterhand_get_closure(HANDLE, g::CB_ON_CLICK).to_bits()
            == closure_f64.to_bits()
            && g::geisterhand_pending_holds_value_for_test(arg_f64);
        (arg_f64, alive)
    })
    .join()
    .unwrap();

    assert!(alive, "the entries must exist while their thread lives");
    assert_eq!(
        g::perry_geisterhand_get_closure(HANDLE, g::CB_ON_CLICK).to_bits(),
        0.0f64.to_bits(),
        "a dead thread's widget callback outlived its heap"
    );
    assert!(
        !g::geisterhand_pending_holds_value_for_test(queued_arg),
        "a dead thread's queued action outlived its heap"
    );
}

#[test]
fn thread_exit_releases_the_threads_ui_state_values_and_foreach_bindings() {
    use perry_runtime::ui_text_registry as u;
    const STATE_ID: &str = "__perry_11471_ui_state_probe";
    const FOREACH_ID: &str = "__perry_11471_ui_foreach_probe";

    let alive = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure =
            scope.root_raw_mut_ptr(js_closure_alloc(perry_runtime::fn_info!(thunk0, 0), 0));
        let value = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let value_bits = array_bits(value.get_raw_mut_ptr::<ArrayHeader>());
        let closure_f64 = f64::from_bits(closure_bits(closure.get_raw_mut_ptr::<ClosureHeader>()));
        u::js_state_init(string_value(STATE_ID), f64::from_bits(value_bits));
        // No render handler is registered in this process, so this only records.
        u::js_foreach_register(string_value(FOREACH_ID), 0x1147_1002, closure_f64);
        let state_bits = u::js_state_get(string_value(STATE_ID)).to_bits();
        (state_bits >> 48) == 0x7FFD && u::foreach_binding_count_for_test(FOREACH_ID) == 1
    })
    .join()
    .unwrap();

    assert!(alive, "the entries must exist while their thread lives");
    assert_eq!(
        u::js_state_get(string_value(STATE_ID)).to_bits(),
        UNDEFINED_BITS,
        "a dead thread's perry/ui state value outlived its heap"
    );
    assert_eq!(
        u::foreach_binding_count_for_test(FOREACH_ID),
        0,
        "a dead thread's ForEach render closure outlived its heap"
    );
}

#[test]
fn thread_exit_releases_the_threads_frame_callbacks_and_last_fire_entries() {
    use perry_runtime::frame as f;

    let (id, closure_addr, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure =
            scope.root_raw_mut_ptr(js_closure_alloc(perry_runtime::fn_info!(thunk2, 2), 0));
        let addr = || closure.get_raw_mut_ptr::<ClosureHeader>() as i64;
        // Fire once so LAST_FIRE_BY_CLOSURE records this closure, then queue
        // it again so a pending callback is left behind.
        f::js_on_frame_callback(addr());
        f::js_frame_tick(16.0);
        let id = f::js_on_frame_callback(addr());
        let alive =
            f::frame_callback_pending_for_test(id) && f::frame_last_fire_recorded_for_test(addr());
        (id, addr(), alive)
    })
    .join()
    .unwrap();

    assert!(alive, "the entries must exist while their thread lives");
    assert!(
        !f::frame_callback_pending_for_test(id),
        "a dead thread's onFrame callback outlived its heap"
    );
    assert!(
        !f::frame_last_fire_recorded_for_test(closure_addr),
        "a dead thread's last-fire entry outlived its heap"
    );
}

#[test]
fn thread_exit_clears_the_threads_tui_input_handler() {
    use perry_runtime::tui::input as i;

    let (handler, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure =
            scope.root_raw_mut_ptr(js_closure_alloc(perry_runtime::fn_info!(thunk0, 0), 0));
        let handler = closure.get_raw_mut_ptr::<ClosureHeader>() as i64;
        i::js_perry_tui_use_input(handler);
        (handler, i::tui_input_handler_for_test() == handler)
    })
    .join()
    .unwrap();

    assert!(
        alive,
        "the handler must be published while its thread lives"
    );
    assert_ne!(
        i::tui_input_handler_for_test(),
        handler,
        "a dead thread's useInput handler outlived its heap"
    );
}

#[test]
fn thread_exit_resets_the_threads_tui_state_slots() {
    use perry_runtime::tui::state as s;

    let (bits, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let value = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let handle = s::js_perry_tui_state_alloc(f64::from_bits(array_bits(
            value.get_raw_mut_ptr::<ArrayHeader>(),
        )));
        // Read back through the handle: the handle allocation may have moved
        // the value, and the slot follows it.
        let bits = s::js_perry_tui_state_get(handle).to_bits();
        let alive = (bits >> 48) == 0x7FFD && s::tui_state_slots_hold_bits_for_test(bits);
        (bits, alive)
    })
    .join()
    .unwrap();

    assert!(alive, "the slot must hold the value while its thread lives");
    assert!(
        !s::tui_state_slots_hold_bits_for_test(bits),
        "a dead thread's perry/tui state value outlived its heap"
    );
}

#[test]
fn thread_exit_resets_the_threads_tui_hook_slots() {
    use perry_runtime::tui::hooks as h;

    let (bits, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let value = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let bits = h::js_perry_tui_use_state(f64::from_bits(array_bits(
            value.get_raw_mut_ptr::<ArrayHeader>(),
        )))
        .to_bits();
        let alive = (bits >> 48) == 0x7FFD && h::tui_hook_slots_hold_bits_for_test(bits);
        (bits, alive)
    })
    .join()
    .unwrap();

    assert!(
        alive,
        "the hook slot must hold the value while its thread lives"
    );
    assert!(
        !h::tui_hook_slots_hold_bits_for_test(bits),
        "a dead thread's useState value outlived its heap"
    );
}
