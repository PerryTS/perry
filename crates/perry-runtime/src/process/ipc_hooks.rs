//! IPC bootstrap and event-pump entry points filled by its installer.
use crate::feature_hooks::Hook;
static INITIALIZE_CHANNEL: Hook<fn(Option<String>, &str)> = Hook::empty();
pub(crate) use super::ipc_bootstrap::process_ipc_ensure_initialized as ensure_initialized;
pub(super) fn initialize_channel(fd: Option<String>, mode: &str) {
    if let Some(run) = INITIALIZE_CHANNEL.get() {
        run(fd, mode)
    }
}
static PROCESS_IPC_PROPERTY: Hook<fn(&str) -> Option<f64>> = Hook::empty();
pub(crate) fn process_ipc_property(property: &str) -> Option<f64> {
    if super::worker_restrictions::worker_process_member_hidden("process", property) {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    if super::worker_restrictions::in_node_worker() {
        match property {
            "channel" | "connected" => {
                super::worker_restrictions::ensure_worker_operation_allowed(property)
            }
            "send" | "disconnect" => {
                return Some(crate::object::bound_native_callable_export_value(
                    "process", property,
                ))
            }
            _ => {}
        }
    }
    PROCESS_IPC_PROPERTY
        .get()
        .map_or_else(|| None, |run| run(property))
}
static PROCESS_IPC_NOTE_LISTENER: Hook<fn(&str) -> ()> = Hook::empty();
pub(crate) fn process_ipc_note_listener(event: &str) -> () {
    PROCESS_IPC_NOTE_LISTENER
        .get()
        .map_or_else(|| (), |run| run(event))
}
static JS_PROCESS_IPC_DRAIN: Hook<extern "C" fn() -> i32> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_process_ipc_drain() -> i32 {
    JS_PROCESS_IPC_DRAIN.get().map_or_else(
        || {
            ensure_initialized();
            0
        },
        |run| run(),
    )
}
static JS_PROCESS_IPC_HAS_ACTIVE: Hook<extern "C" fn() -> i32> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_process_ipc_has_active() -> i32 {
    JS_PROCESS_IPC_HAS_ACTIVE.get().map_or_else(
        || {
            if !super::ipc_bootstrap::IPC_PROBED.load(std::sync::atomic::Ordering::Acquire) {
                ensure_initialized();
            }
            0
        },
        |run| run(),
    )
}
static PROCESS_IPC_SEND_CALL: Hook<fn(f64, f64, f64, f64) -> f64> = Hook::empty();
pub(crate) fn process_ipc_send_call(message: f64, a2: f64, a3: f64, a4: f64) -> f64 {
    crate::process::worker_restrictions::ensure_worker_operation_allowed("send");
    PROCESS_IPC_SEND_CALL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(message, a2, a3, a4),
    )
}
static PROCESS_IPC_DISCONNECT_CALL: Hook<fn() -> f64> = Hook::empty();
pub(crate) fn process_ipc_disconnect_call() -> f64 {
    crate::process::worker_restrictions::ensure_worker_operation_allowed("disconnect");
    PROCESS_IPC_DISCONNECT_CALL
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static PROCESS_IPC_SEND_RAW_JSON: Hook<fn(&str) -> bool> = Hook::empty();
pub(crate) fn process_ipc_send_raw_json(json: &str) -> bool {
    PROCESS_IPC_SEND_RAW_JSON
        .get()
        .map_or_else(|| false, |run| run(json))
}
pub(super) fn install() {
    #[cfg(unix)]
    INITIALIZE_CHANNEL.set(super::ipc::initialize_unix_ipc);
    #[cfg(windows)]
    INITIALIZE_CHANNEL.set(super::ipc::initialize_windows_ipc);
    PROCESS_IPC_PROPERTY.set(super::ipc::process_ipc_property_impl);
    PROCESS_IPC_NOTE_LISTENER.set(super::ipc::process_ipc_note_listener_impl);
    JS_PROCESS_IPC_DRAIN.set(super::ipc::js_process_ipc_drain_impl);
    JS_PROCESS_IPC_HAS_ACTIVE.set(super::ipc::js_process_ipc_has_active_impl);
    PROCESS_IPC_SEND_CALL.set(super::ipc::process_ipc_send_call_impl);
    PROCESS_IPC_DISCONNECT_CALL.set(super::ipc::process_ipc_disconnect_call_impl);
    PROCESS_IPC_SEND_RAW_JSON.set(super::ipc::process_ipc_send_raw_json_impl);
}
