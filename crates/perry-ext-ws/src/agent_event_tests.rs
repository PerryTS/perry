//! A different agent may pump concurrently while a WebSocket event waits.
use super::*;

#[test]
fn another_agent_cannot_consume_a_queued_server_close() {
    std::thread::spawn(|| {
        let owner = perry_runtime::agent::enter_worker_agent();
        let server = js_ws_server_new(f64::from_bits(JsValue::UNDEFINED.bits()));
        push_ws_event(PendingWsEvent::ServerClose(server));
        std::thread::spawn(|| {
            let other = perry_runtime::agent::enter_worker_agent();
            js_ws_process_pending();
            perry_runtime::agent::retire_agent(other);
        })
        .join()
        .unwrap();
        assert!(
            perry_ffi::get_handle::<WsServerHandle>(server).is_some(),
            "another agent must leave the owner's event queued"
        );
        js_ws_process_pending();
        assert!(
            perry_ffi::get_handle::<WsServerHandle>(server).is_none(),
            "the owning agent must deliver its queued close"
        );
        perry_runtime::agent::retire_agent(owner);
    })
    .join()
    .unwrap();
}

#[test]
fn retiring_an_agent_discards_only_its_pending_events() {
    std::thread::spawn(|| {
        let owner = perry_runtime::agent::enter_worker_agent();
        ensure_runtime_hooks_registered();
        push_ws_event(PendingWsEvent::Error(usize::MAX, "retired".into()));
        std::thread::spawn(|| {
            let other = perry_runtime::agent::enter_worker_agent();
            ensure_runtime_hooks_registered();
            push_ws_event(PendingWsEvent::Error(usize::MAX - 1, "other".into()));
            perry_runtime::agent::retire_agent(other);
        })
        .join()
        .unwrap();
        assert!(WS_PENDING_EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(agent, _)| *agent == owner));
        perry_runtime::agent::retire_agent(owner);
        assert!(!WS_PENDING_EVENTS
            .lock()
            .unwrap()
            .iter()
            .any(|(agent, _)| *agent == owner));
    })
    .join()
    .unwrap();
}
