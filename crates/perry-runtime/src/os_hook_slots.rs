//! Installer-selected process streams, listeners and signal polling.
use crate::array::ArrayHeader;
use crate::feature_hooks::Hook;
static ADD_INTERNAL_PROCESS_LISTENER: Hook<fn(&str, *const crate::closure::ClosureHeader) -> ()> =
    Hook::empty();
pub(crate) fn add_internal_process_listener(
    event: &str,
    callback: *const crate::closure::ClosureHeader,
) -> () {
    ADD_INTERNAL_PROCESS_LISTENER
        .get()
        .map_or_else(|| (), |run| run(event, callback))
}
static EMIT_PROCESS_EVENT: Hook<fn(&str, &[f64]) -> bool> = Hook::empty();
pub(crate) fn emit_process_event(event: &str, args: &[f64]) -> bool {
    EMIT_PROCESS_EVENT
        .get()
        .map_or_else(|| false, |run| run(event, args))
}
static EMIT_PROCESS_UNCAUGHT_EXCEPTION: Hook<fn(f64) -> ()> = Hook::empty();
pub(crate) fn emit_process_uncaught_exception(error: f64) -> () {
    EMIT_PROCESS_UNCAUGHT_EXCEPTION.get().map_or_else(
        || crate::exception::exit_on_uncaught(error),
        |run| run(error),
    )
}
static ENABLE_PROCESS_STDIN_KEYPRESS_EVENTS: Hook<fn(i64) -> ()> = Hook::empty();
pub(crate) fn enable_process_stdin_keypress_events(callback: i64) -> () {
    ENABLE_PROCESS_STDIN_KEYPRESS_EVENTS
        .get()
        .map_or_else(|| (), |run| run(callback))
}
static IS_PROCESS_STDIN_HANDLE: Hook<fn(i64) -> bool> = Hook::empty();
pub(crate) fn is_process_stdin_handle(handle: i64) -> bool {
    IS_PROCESS_STDIN_HANDLE
        .get()
        .map_or_else(|| false, |run| run(handle))
}
static JS_PROCESS_ADD_LISTENER: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_add_listener(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_ADD_LISTENER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_EMIT: Hook<extern "C" fn(i64, *const ArrayHeader) -> f64> = Hook::empty();
pub(crate) fn js_process_emit(event_bits: i64, args: *const ArrayHeader) -> f64 {
    JS_PROCESS_EMIT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, args),
    )
}
static JS_PROCESS_EMIT_EXIT: Hook<extern "C" fn(f64) -> ()> = Hook::empty();
pub(crate) fn js_process_emit_exit(code: f64) -> () {
    JS_PROCESS_EMIT_EXIT
        .get()
        .map_or_else(|| (), |run| run(code))
}
static JS_PROCESS_EVENT_NAMES: Hook<extern "C" fn() -> *mut ArrayHeader> = Hook::empty();
pub(crate) fn js_process_event_names() -> *mut ArrayHeader {
    JS_PROCESS_EVENT_NAMES
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run())
}
static JS_PROCESS_GET_MAX_LISTENERS: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_process_get_max_listeners() -> f64 {
    JS_PROCESS_GET_MAX_LISTENERS
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PROCESS_KILL: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_process_kill(pid: f64, signal: f64) -> f64 {
    JS_PROCESS_KILL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(pid, signal),
    )
}
static JS_PROCESS_LISTENER_COUNT: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_listener_count(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_LISTENER_COUNT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_LISTENERS: Hook<extern "C" fn(i64) -> *mut ArrayHeader> = Hook::empty();
pub(crate) fn js_process_listeners(event_bits: i64) -> *mut ArrayHeader {
    JS_PROCESS_LISTENERS
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(event_bits))
}
static JS_PROCESS_OFF: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_off(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_OFF.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_ON: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_on(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_ON.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_ONCE: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_once(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_ONCE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_PREPEND_LISTENER: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_prepend_listener(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_PREPEND_LISTENER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_PREPEND_ONCE_LISTENER: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_prepend_once_listener(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_PREPEND_ONCE_LISTENER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_RAW_LISTENERS: Hook<extern "C" fn(i64) -> *mut ArrayHeader> = Hook::empty();
pub(crate) fn js_process_raw_listeners(event_bits: i64) -> *mut ArrayHeader {
    JS_PROCESS_RAW_LISTENERS
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(event_bits))
}
static JS_PROCESS_REMOVE_ALL_LISTENERS: Hook<extern "C" fn(i64) -> f64> = Hook::empty();
pub(crate) fn js_process_remove_all_listeners(event_bits: i64) -> f64 {
    JS_PROCESS_REMOVE_ALL_LISTENERS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits),
    )
}
static JS_PROCESS_REMOVE_LISTENER: Hook<extern "C" fn(i64, i64) -> f64> = Hook::empty();
pub(crate) fn js_process_remove_listener(event_bits: i64, listener_bits: i64) -> f64 {
    JS_PROCESS_REMOVE_LISTENER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(event_bits, listener_bits),
    )
}
static JS_PROCESS_SET_MAX_LISTENERS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_process_set_max_listeners(value: f64) -> f64 {
    JS_PROCESS_SET_MAX_LISTENERS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(value),
    )
}
static JS_PROCESS_SIGNAL_DRAIN: Hook<extern "C" fn() -> i32> = Hook::empty();
pub(crate) fn js_process_signal_drain() -> i32 {
    JS_PROCESS_SIGNAL_DRAIN.get().map_or_else(|| 0, |run| run())
}
static JS_PROCESS_SIGNAL_HAS_ACTIVE: Hook<extern "C" fn() -> i32> = Hook::empty();
pub(crate) fn js_process_signal_has_active() -> i32 {
    JS_PROCESS_SIGNAL_HAS_ACTIVE
        .get()
        .map_or_else(|| 0, |run| run())
}
static JS_PROCESS_STDERR: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_process_stderr() -> f64 {
    JS_PROCESS_STDERR
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PROCESS_STDIN: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_process_stdin() -> f64 {
    JS_PROCESS_STDIN
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PROCESS_STDOUT: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_process_stdout() -> f64 {
    JS_PROCESS_STDOUT
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_UTIL_CONVERT_PROCESS_SIGNAL_TO_EXIT_CODE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_util_convert_process_signal_to_exit_code(signal_code: f64) -> f64 {
    JS_UTIL_CONVERT_PROCESS_SIGNAL_TO_EXIT_CODE
        .get()
        .map_or_else(
            || f64::from_bits(crate::value::TAG_UNDEFINED),
            |run| run(signal_code),
        )
}
static ON_SIGNAL_COMPLETION: Hook<fn(i32, crate::turnloop_proc::StreamEvent) -> ()> = Hook::empty();
pub(crate) fn on_signal_completion(signum: i32, event: crate::turnloop_proc::StreamEvent) -> () {
    ON_SIGNAL_COMPLETION
        .get()
        .map_or_else(|| (), |run| run(signum, event))
}
static PROCESS_STDIN_NEEDS_PUMP: Hook<fn() -> bool> = Hook::empty();
pub(crate) fn process_stdin_needs_pump() -> bool {
    PROCESS_STDIN_NEEDS_PUMP
        .get()
        .map_or_else(|| false, |run| run())
}
static PUMP_PROCESS_STDIN: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn pump_process_stdin() -> () {
    PUMP_PROCESS_STDIN.get().map_or_else(|| (), |run| run())
}
static REFRESH_TTY_STREAM_SIZES: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn refresh_tty_stream_sizes() -> () {
    REFRESH_TTY_STREAM_SIZES
        .get()
        .map_or_else(|| (), |run| run())
}
static REMOVE_INTERNAL_PROCESS_LISTENER: Hook<
    fn(&str, *const crate::closure::ClosureHeader) -> (),
> = Hook::empty();
pub(crate) fn remove_internal_process_listener(
    event: &str,
    callback: *const crate::closure::ClosureHeader,
) -> () {
    REMOVE_INTERNAL_PROCESS_LISTENER
        .get()
        .map_or_else(|| (), |run| run(event, callback))
}
static SIGNALS_ON_TURNLOOP: Hook<fn() -> usize> = Hook::empty();
pub(crate) fn signals_on_turnloop() -> usize {
    SIGNALS_ON_TURNLOOP.get().map_or_else(|| 0, |run| run())
}
static STDIN_LISTENERS_KEEP_LOOP_ALIVE: Hook<fn() -> bool> = Hook::empty();
pub(crate) fn stdin_listeners_keep_loop_alive() -> bool {
    STDIN_LISTENERS_KEEP_LOOP_ALIVE
        .get()
        .map_or_else(|| false, |run| run())
}
static STDIN_PUSH_BYTES: Hook<fn(&[u8]) -> ()> = Hook::empty();
pub(crate) fn stdin_push_bytes(bytes: &[u8]) -> () {
    STDIN_PUSH_BYTES.get().map_or_else(|| (), |run| run(bytes))
}
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register();
    }
}
fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "process_emitter",
        crate::os::process_emitter_root_scanner,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::os::process_emitter_root_scanner",
        crate::os::process_emitter_root_scanner,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::os::scan_process_event_listener_roots_mut",
        crate::os::scan_process_event_listener_roots_mut,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::os::scan_process_stream_singleton_roots_mut",
        crate::os::scan_process_stream_singleton_roots_mut,
    );
}

pub(crate) fn install() {
    crate::perry_install_once!({
        crate::os::os_process_emitter::install_exit_tls_resolver();
        crate::os::os_process_emitter::install_process_signal_sync();
        ADD_INTERNAL_PROCESS_LISTENER
            .set(crate::os::os_process_emitter::add_internal_process_listener);
        EMIT_PROCESS_EVENT.set(crate::os::os_process_emitter::emit_process_event);
        EMIT_PROCESS_UNCAUGHT_EXCEPTION
            .set(crate::os::os_process_emitter::emit_process_uncaught_exception);
        ENABLE_PROCESS_STDIN_KEYPRESS_EVENTS.set(crate::os::enable_process_stdin_keypress_events);
        IS_PROCESS_STDIN_HANDLE.set(crate::os::is_process_stdin_handle);
        JS_PROCESS_ADD_LISTENER.set(crate::os::os_process_emitter::js_process_add_listener);
        JS_PROCESS_EMIT.set(crate::os::os_process_emitter::js_process_emit);
        JS_PROCESS_EMIT_EXIT.set(crate::os::os_process_emitter::js_process_emit_exit);
        JS_PROCESS_EVENT_NAMES.set(crate::os::os_process_emitter::js_process_event_names);
        JS_PROCESS_GET_MAX_LISTENERS
            .set(crate::os::os_process_emitter::js_process_get_max_listeners);
        JS_PROCESS_KILL.set(crate::os::signal::js_process_kill);
        JS_PROCESS_LISTENER_COUNT.set(crate::os::os_process_emitter::js_process_listener_count);
        JS_PROCESS_LISTENERS.set(crate::os::os_process_emitter::js_process_listeners);
        JS_PROCESS_OFF.set(crate::os::os_process_emitter::js_process_off);
        JS_PROCESS_ON.set(crate::os::os_process_emitter::js_process_on);
        JS_PROCESS_ONCE.set(crate::os::os_process_emitter::js_process_once);
        JS_PROCESS_PREPEND_LISTENER.set(crate::os::os_process_emitter::js_process_prepend_listener);
        JS_PROCESS_PREPEND_ONCE_LISTENER
            .set(crate::os::os_process_emitter::js_process_prepend_once_listener);
        JS_PROCESS_RAW_LISTENERS.set(crate::os::os_process_emitter::js_process_raw_listeners);
        JS_PROCESS_REMOVE_ALL_LISTENERS
            .set(crate::os::os_process_emitter::js_process_remove_all_listeners);
        JS_PROCESS_REMOVE_LISTENER.set(crate::os::os_process_emitter::js_process_remove_listener);
        JS_PROCESS_SET_MAX_LISTENERS
            .set(crate::os::os_process_emitter::js_process_set_max_listeners);
        JS_PROCESS_SIGNAL_DRAIN.set(crate::os::os_process_emitter::js_process_signal_drain);
        JS_PROCESS_SIGNAL_HAS_ACTIVE
            .set(crate::os::os_process_emitter::js_process_signal_has_active);
        JS_PROCESS_STDERR.set(crate::os::js_process_stderr);
        JS_PROCESS_STDIN.set(crate::os::js_process_stdin);
        JS_PROCESS_STDOUT.set(crate::os::js_process_stdout);
        JS_UTIL_CONVERT_PROCESS_SIGNAL_TO_EXIT_CODE
            .set(crate::os::signal::js_util_convert_process_signal_to_exit_code);
        ON_SIGNAL_COMPLETION.set(crate::os::signal::on_signal_completion);
        PROCESS_STDIN_NEEDS_PUMP.set(crate::os::process_stdin_needs_pump);
        PUMP_PROCESS_STDIN.set(crate::os::pump_process_stdin);
        REFRESH_TTY_STREAM_SIZES.set(crate::os::refresh_tty_stream_sizes);
        REMOVE_INTERNAL_PROCESS_LISTENER
            .set(crate::os::os_process_emitter::remove_internal_process_listener);
        SIGNALS_ON_TURNLOOP.set(crate::os::signal::signals_on_turnloop);
        STDIN_LISTENERS_KEEP_LOOP_ALIVE.set(crate::os::stdin_listeners_keep_loop_alive);
        STDIN_PUSH_BYTES.set(crate::os::stdin_push_bytes);
        REGISTER_ROOTS.set(register_roots);
    });
    register_installed_roots();
}

/// `js_process_emit_before_exit` with the pending `process.exitCode` as the
/// argument — the form the generated epilogue calls.
///
/// Node passes the code the process is *about* to exit with, not a constant:
/// `process.exitCode = 5` with a `beforeExit` listener prints `5` there. The
/// epilogue used to pass a literal `0`, so every `beforeExit` listener in a
/// program that had set an exit code saw the wrong number (#9403).
#[no_mangle]
pub extern "C" fn js_process_emit_before_exit_pending() {
    let code = crate::process::js_process_pending_exit_code();
    let _ = crate::os_hook_slots::emit_process_event("beforeExit", &[code as f64]);
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static EXIT_TLS_RESOLVER: Hook<TlsResolver> = Hook::empty();
pub(crate) fn resolve_process_exit_guard(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    EXIT_TLS_RESOLVER
        .get()
        .expect("process installer must precede exit listener storage")(idx)
}
pub(crate) fn install_exit_resolver(resolve: TlsResolver) {
    EXIT_TLS_RESOLVER.set(resolve);
}
