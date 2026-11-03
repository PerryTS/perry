use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
static CALLS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn accepted(
    _: *const RawClosureHeader,
    _: perry_ffi::JsThis,
    ws: f64,
    req: f64,
) -> f64 {
    assert_eq!(req, 42.0);
    let id = decode_client_id(ws);
    assert!(WS_CONNECTIONS.lock().unwrap().contains_key(&id));
    assert!(WS_CLIENT_PARENT_SERVER.lock().unwrap().contains_key(&id));
    CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(JsValue::UNDEFINED.bits())
}
#[test]
fn legacy_pre_upgraded_host_still_invokes_callback_once() {
    ensure_runtime_hooks_registered();
    let scope = perry_ffi::TransientRootScope::enter();
    let clients = scope.root_nanbox(f64::from_bits(alloc_set(4).bits()));
    let server = register_handle(WsServerHandle {
        owner_agent: perry_ffi::agent_post::current_agent(),
        counted: false,
        listeners: HashMap::new(),
        port: 0,
        host: "127.0.0.1".into(),
        attached_server: None,
        no_server: true,
        is_listening: false,
        client_ids: Vec::new(),
        clients_bits: clients.get().to_bits(),
        listener_id: None,
    });
    let client = register_turnloop_client(987654321);
    let cb = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(accepted, 2; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    ) as i64);
    CALLS.store(0, Ordering::SeqCst);
    unsafe {
        js_ws_handle_upgrade(
            server,
            42.0,
            f64::from_bits(client_js_value(client).bits()),
            f64::from_bits(JsValue::UNDEFINED.bits()),
            cb.get(),
        )
    };
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(
        get_handle_mut::<WsServerHandle>(server).unwrap().client_ids,
        vec![client]
    );
    WS_CONNECTIONS.lock().unwrap().remove(&client);
    WS_CLIENT_LISTENERS.lock().unwrap().remove(&client);
    WS_CLIENT_PARENT_SERVER.lock().unwrap().remove(&client);
    perry_ffi::drop_handle(client as i64);
    perry_ffi::drop_handle(server);
}
