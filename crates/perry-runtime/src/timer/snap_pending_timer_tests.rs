//! SNAPK4: the census names each scheduled timer and immediate by its class,
//! the init that queued it, its callback's code address, its delay and whether
//! it repeats (`snap_pending_timers`), read from the store's own entries.
use super::*;
use std::time::Duration;

extern "C" fn timer_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    1.0
}

extern "C" fn timer_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    2.0
}

fn entry(id: i64, class: Class, delay_ms: u64, callback: i64, owner: u32) -> Entry {
    let mut e = Entry::callback(
        id,
        class,
        Instant::now() + Duration::from_millis(delay_ms),
        delay_ms,
        callback,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        Vec::new(),
        crate::async_context::AsyncContextSnapshot::default(),
        0,
        0,
        None,
    );
    e.context.snap_init = owner;
    e
}

#[test]
fn pending_timers_name_kind_owner_callback_delay_and_repeat() {
    reset_for_test();
    let info_a = crate::fn_info!(timer_a, 1);
    let info_b = crate::fn_info!(timer_b, 1);
    let code_a = unsafe { (*info_a).code as usize };
    let code_b = unsafe { (*info_b).code as usize };
    assert_ne!(code_a, code_b);
    let cb_a = crate::closure::js_closure_alloc(info_a, 0) as i64;
    let cb_b = crate::closure::js_closure_alloc(info_b, 0) as i64;
    with_current(|timers| {
        timers.insert_timer(entry(1, Class::Timeout, 25, cb_a, 5));
        timers.insert_check(entry(2, Class::Immediate, 0, cb_b, 6));
        timers.insert_timer(entry(3, Class::Interval, 40, cb_b, 8));
    });
    let pending = snap_pending_timers().expect("store not locked");
    reset_for_test();
    assert_eq!(
        pending,
        vec![
            SnapPendingTimer {
                kind: "timeout",
                owner: 5,
                code: code_a,
                delay_ms: 25,
                repeat: false
            },
            SnapPendingTimer {
                kind: "immediate",
                owner: 6,
                code: code_b,
                delay_ms: 0,
                repeat: false
            },
            SnapPendingTimer {
                kind: "interval",
                owner: 8,
                code: code_b,
                delay_ms: 40,
                repeat: true
            },
        ]
    );
}

#[test]
fn no_pending_timer_no_entry() {
    reset_for_test();
    with_current(|timers| {
        timers.insert_timer(entry(1, Class::Timeout, 25, 0, 5));
        assert!(
            timers.test_find_by_id(1).is_some(),
            "the subject must hold the entry first"
        );
        assert!(timers.remove_by_id(1, |_| true).is_some());
    });
    assert_eq!(snap_pending_timers(), Some(Vec::new()));
}
