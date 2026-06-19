//! `events.errorMonitor` dispatch for the ext-events EventEmitter twin
//! (#4633). Split out of `lib.rs` to keep it under the 2000-line cap.

use super::*;

/// String key under which a listener registered via the `events.errorMonitor`
/// symbol lands: `event_name_from_bits` stringifies symbol event names, and
/// `Symbol.for("events.errorMonitor")` renders as this. Mirrors the stdlib
/// twin's constant so the two implementations stay behaviorally identical.
pub(super) const ERROR_MONITOR_EVENT_NAME: &str = "Symbol(events.errorMonitor)";

/// Node's `events.errorMonitor` semantics (#4633): listeners installed under
/// the monitor symbol observe every `'error'` emit BEFORE the regular
/// `'error'` listeners run, without counting as error handling - an
/// unhandled `'error'` still throws after the monitor fires. Mirrors
/// `dispatch_error_monitor` in perry-stdlib's events twin.
pub(super) unsafe fn dispatch_error_monitor(
    emitter: &mut EventEmitterHandle,
    handle: Handle,
    arg: Option<f64>,
) {
    let snapshot: Vec<Listener> = match emitter.events.get(ERROR_MONITOR_EVENT_NAME) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return,
    };
    if snapshot.iter().any(|l| l.once) {
        if let Some(v) = emitter.events.get_mut(ERROR_MONITOR_EVENT_NAME) {
            v.retain(|l| !l.once);
        }
        emitter.prune_event_if_empty(ERROR_MONITOR_EVENT_NAME);
    }
    for l in snapshot {
        if l.callback != 0 {
            let args: &[f64] = arg.as_slice();
            let _ = call_emitter_listener(handle, l.callback, args);
        }
    }
}
