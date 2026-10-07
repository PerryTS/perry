//! A piped readable's destination fan-out must reread every destination after
//! a listener collects (#11882). The first destination's listener runs a
//! moving minor; the second destination and the source are young, so they
//! move, and the poisoned from-space makes any stale address visible: a
//! stale destination has no listeners and no state.
use super::*;
use crate::gc::RuntimeHandleScope;
use std::cell::{Cell, RefCell};

thread_local! {
    static COLLECTIONS: Cell<usize> = const { Cell::new(0) };
    static SEEN: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
}

/// Runs one moving minor, on its first call only.
extern "C" fn collecting_listener(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    if COLLECTIONS.with(|count| count.replace(count.get() + 1)) == 0 {
        crate::gc::gc_collect_minor();
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Records the receiver and first argument it was called with.
extern "C" fn recording_listener(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    arg: f64,
) -> f64 {
    SEEN.with(|seen| {
        seen.borrow_mut()
            .push((this.as_f64().to_bits(), arg.to_bits()))
    });
    f64::from_bits(TAG_UNDEFINED)
}

fn listen(stream: f64, event: &'static [u8], closure: *mut ClosureHeader) {
    let _ = js_node_stream_method_on(
        raw_ptr_from_value(stream) as i64,
        literal_string_value(event),
        box_pointer(closure as *const u8),
    );
}

/// Source piped to two writables; `event` on the first collects, on the
/// second records. Runs `run(source)`, asserts that the collection moved the
/// second destination and the source and that the second listener saw the
/// live destination, then hands `check` the live source and second
/// destination and the recorded `(this, arg)` call.
fn fan_out_after_collecting_listener(
    event: &'static [u8],
    run: fn(f64),
    check: impl FnOnce(f64, f64, (u64, u64)),
) {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _poison =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);

    COLLECTIONS.with(|count| count.set(0));
    SEEN.with(|seen| seen.borrow_mut().clear());

    let undef = f64::from_bits(TAG_UNDEFINED);
    let scope = RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(js_node_stream_passthrough_new(undef));
    let first = scope.root_nanbox_f64(js_node_stream_writable_new(undef));
    let second = scope.root_nanbox_f64(js_node_stream_writable_new(undef));
    add_pipe_destination(source.get_nanbox_f64(), first.get_nanbox_f64());
    add_pipe_destination(source.get_nanbox_f64(), second.get_nanbox_f64());
    let collecting = js_closure_alloc(crate::fn_info!(collecting_listener, 1; with_declared(1)), 0);
    listen(first.get_nanbox_f64(), event, collecting);
    let recording = js_closure_alloc(crate::fn_info!(recording_listener, 1; with_declared(1)), 0);
    listen(second.get_nanbox_f64(), event, recording);
    let second_before = second.get_nanbox_f64().to_bits();
    let source_before = source.get_nanbox_f64().to_bits();

    run(source.get_nanbox_f64());

    assert_eq!(
        COLLECTIONS.with(Cell::get),
        1,
        "the first destination's listener must run once and collect"
    );
    assert_ne!(
        second.get_nanbox_f64().to_bits(),
        second_before,
        "the second destination must actually move"
    );
    assert_ne!(
        source.get_nanbox_f64().to_bits(),
        source_before,
        "the source must actually move"
    );
    let seen = SEEN.with(|seen| seen.borrow().clone());
    assert_eq!(
        seen.len(),
        1,
        "the second destination's listener must run exactly once: {seen:?}"
    );
    assert_eq!(
        seen[0].0,
        second.get_nanbox_f64().to_bits(),
        "the listener must receive the current second destination"
    );
    check(source.get_nanbox_f64(), second.get_nanbox_f64(), seen[0]);
}

#[test]
fn unpipe_all_rereads_destinations_and_source_after_collecting_listener() {
    fan_out_after_collecting_listener(b"unpipe", unpipe_all_destinations, |source, _, seen| {
        assert_eq!(
            seen.1,
            source.to_bits(),
            "'unpipe' must receive the current source, not its old address"
        );
    });
}

#[test]
fn end_pipe_destinations_rereads_destinations_after_collecting_listener() {
    fan_out_after_collecting_listener(b"end", end_pipe_destinations, |_, second, _| {
        assert!(
            has_truthy_hidden(second, hidden_end_emitted_key()),
            "the live second destination must be ended"
        );
    });
}
