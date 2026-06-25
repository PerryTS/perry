//! Regression tests for `TCP_NODELAY` parity with Node (the per-connection
//! Nagle default + a working `socket.setNoDelay`).
//!
//! Before this fix, perry never called `set_nodelay`, so every connection ran
//! with Nagle ON (a Node-parity bug + tail-latency cost), and the
//! `socket.setNoDelay()` dispatch arm was a no-op that silently dropped the
//! call. These tests drive the real `run_socket_task` command loop over a
//! loopback connection and observe `TCP_NODELAY` on the stream the task owns,
//! so they exercise the actual wiring rather than a stand-in.

use crate::{run_socket_task, SocketCommand, Transport};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

/// Connect a loopback pair and hand the SERVER-side accepted stream to a
/// freshly-spawned `run_socket_task`, returning the command channel plus the
/// client end (kept alive so the connection stays open for the test). The
/// caller drives the socket purely through `cmd_tx`, exactly as the FFI layer
/// does. `default_nodelay` reproduces what the production accept/connect sites
/// do before spawning the task (Node's default ON).
async fn spawn_task_over_loopback(
    default_nodelay: bool,
) -> (mpsc::UnboundedSender<SocketCommand>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();

    // Mirror the production accept/connect path: the Node default is applied
    // to the raw stream before it is handed to the task.
    server.set_nodelay(default_nodelay).unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel::<SocketCommand>();
    tokio::spawn(async move {
        run_socket_task(-12_345, Transport::Plain(server), &mut rx).await;
    });
    (tx, client)
}

/// Round-trip the test-only `QueryNoDelay` command to read the live socket's
/// `TCP_NODELAY` state off the stream the task owns.
async fn live_nodelay(tx: &mpsc::UnboundedSender<SocketCommand>) -> bool {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(SocketCommand::QueryNoDelay(reply_tx)).unwrap();
    reply_rx.await.unwrap()
}

/// The accept/connect default matches Node: a freshly-handed socket has
/// `TCP_NODELAY` ON (Nagle disabled).
#[tokio::test]
async fn accepted_socket_defaults_to_nodelay_on() {
    let (tx, _client) = spawn_task_over_loopback(true).await;
    assert!(
        live_nodelay(&tx).await,
        "a freshly accepted/connected socket must default to TCP_NODELAY on, matching Node"
    );
}

/// `socket.setNoDelay(false)` is no longer a no-op — it re-enables Nagle on the
/// live socket. (Pre-fix the dispatch arm dropped the call and this stayed ON.)
#[tokio::test]
async fn set_no_delay_false_disables_nodelay() {
    let (tx, _client) = spawn_task_over_loopback(true).await;
    assert!(live_nodelay(&tx).await, "precondition: starts ON");

    tx.send(SocketCommand::SetNoDelay(false)).unwrap();
    assert!(
        !live_nodelay(&tx).await,
        "setNoDelay(false) must turn TCP_NODELAY off (re-enable Nagle)"
    );
}

/// `setNoDelay(true)` re-enables nodelay after it was turned off.
#[tokio::test]
async fn set_no_delay_true_reenables_nodelay() {
    let (tx, _client) = spawn_task_over_loopback(true).await;
    tx.send(SocketCommand::SetNoDelay(false)).unwrap();
    assert!(!live_nodelay(&tx).await, "precondition: turned OFF");

    tx.send(SocketCommand::SetNoDelay(true)).unwrap();
    assert!(
        live_nodelay(&tx).await,
        "setNoDelay(true) must turn TCP_NODELAY back on"
    );
}

/// The FFI entry point coerces its argument the way Node does
/// (`enable === undefined || !!enable`): a bare `setNoDelay()` enables, an
/// explicit `false` disables. This covers the `dispatch.rs` arm that was a
/// no-op before the fix. Driven through the public `js_net_socket_*` surface
/// against a real socket so the whole chain — coercion, command, socket
/// option — is exercised.
#[tokio::test]
async fn ffi_set_no_delay_coercion_and_effect() {
    use perry_ffi::JsValue;

    let (tx, _client) = spawn_task_over_loopback(true).await;

    // Register the handle so the FFI function can find its command channel.
    let handle = -54_321_i64;
    crate::statics::sockets()
        .lock()
        .unwrap()
        .insert(handle, crate::SocketState::for_test(tx.clone()));

    // setNoDelay(false) → off. The FFI takes the NaN-boxed bits as i64.
    let false_bits = JsValue::from_bool(false).bits() as i64;
    unsafe { crate::js_net_socket_set_no_delay(handle, false_bits) };
    assert!(
        !live_nodelay(&tx).await,
        "js_net_socket_set_no_delay(false) must disable nodelay"
    );

    // Bare setNoDelay() (undefined arg) → Node treats as enable → on.
    let undef_bits = JsValue::UNDEFINED.bits() as i64;
    unsafe { crate::js_net_socket_set_no_delay(handle, undef_bits) };
    assert!(
        live_nodelay(&tx).await,
        "js_net_socket_set_no_delay(undefined) must enable nodelay (Node default)"
    );

    crate::statics::sockets().lock().unwrap().remove(&handle);
}

/// Drive `socket.setNoDelay(...)` through the EXACT dispatch entry the JS
/// runtime hits — `js_ext_net_handle_method_dispatch` → the `setNoDelay`
/// arm in `dispatch.rs` — which is the literal locus of the original no-op
/// bug. Proves the arm is wired end to end: the no-arg form enables (Node's
/// `enable === undefined` default), an explicit `false` disables.
#[tokio::test]
async fn dispatch_arm_set_no_delay_end_to_end() {
    use perry_ffi::JsValue;

    let (tx, _client) = spawn_task_over_loopback(true).await;

    // `is_net_socket_handle` (the dispatch arm's gate) checks the sockets map.
    let handle = -55_555_i64;
    crate::statics::sockets()
        .lock()
        .unwrap()
        .insert(handle, crate::SocketState::for_test(tx.clone()));

    // Invoke the method exactly as the runtime does, through the public
    // dispatch extension entry point.
    let call = |method: &str, args: &[f64]| -> bool {
        let mut out = 0.0_f64;
        let rc = unsafe {
            crate::dispatch::js_ext_net_handle_method_dispatch(
                handle,
                method.as_ptr(),
                method.len(),
                args.as_ptr(),
                args.len(),
                &mut out,
            )
        };
        rc == 1
    };

    // setNoDelay(false) → off. Args are NaN-boxed values passed as f64 (bits),
    // exactly as the codegen NA_F64 slot delivers them.
    assert!(
        call(
            "setNoDelay",
            &[f64::from_bits(JsValue::from_bool(false).bits())]
        ),
        "dispatch must claim the setNoDelay method"
    );
    assert!(
        !live_nodelay(&tx).await,
        "dispatch setNoDelay(false) must disable nodelay (was a no-op before the fix)"
    );

    // Bare setNoDelay() — the no-arg form the dispatch arm pads with undefined.
    assert!(
        call("setNoDelay", &[]),
        "dispatch must claim no-arg setNoDelay"
    );
    assert!(
        live_nodelay(&tx).await,
        "dispatch setNoDelay() must enable nodelay (Node's undefined→true default)"
    );

    crate::statics::sockets().lock().unwrap().remove(&handle);
}
