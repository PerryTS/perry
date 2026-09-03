//! Socket event emission helpers, split from `lib.rs` for the 2000-line
//! file cap (#9607 added the borrowed-fd plumbing above).

use super::*;

fn socket_receiver(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
}

unsafe fn emit_socket_no_arg(handle: i64, event: &str) {
    extern "C" {
        fn js_implicit_this_set(value: f64) -> f64;
    }
    let frame = dispatch_custody::DispatchFrame::park(listeners_for(handle, event));
    let previous_this = js_implicit_this_set(socket_receiver(handle));
    for index in 0..frame.len() {
        let callback = frame.cb(index);
        if callback != 0 {
            let _ = JsClosure::from_raw(callback as *const RawClosureHeader).call0();
        }
    }
    js_implicit_this_set(previous_this);
    drop(frame);
    lifecycle::drain_once_listeners(handle, event);
}

unsafe fn emit_tls_secure_connect(handle: i64) {
    extern "C" {
        fn js_tls_client_check_identity_from_metadata(handle: i64) -> f64;
    }
    let identity_error = js_tls_client_check_identity_from_metadata(handle);
    if !JsValue::from_bits(identity_error.to_bits()).is_undefined() {
        let mut frame = dispatch_custody::DispatchFrame::park(listeners_for(handle, "error"));
        frame.set_payload(identity_error.to_bits());
        for index in 0..frame.len() {
            let callback = frame.cb(index);
            if callback != 0 {
                let _ = JsClosure::from_raw(callback as *const RawClosureHeader)
                    .call1(f64::from_bits(frame.payload_bits()));
            }
        }
        drop(frame);
        lifecycle::drain_once_listeners(handle, "error");
        if let Some(socket) = statics::sockets().lock().unwrap().get(&handle) {
            let _ = socket.cmd_tx.send(SocketCommand::Destroy);
        }
        return;
    }
    emit_socket_no_arg(handle, "secureConnect");
}

/// Drain ext-net's own pending-event queue.
///
/// This carries a DISTINCT `#[no_mangle]` symbol (`js_ext_net_drain_pending`),
/// deliberately NOT the `js_net_process_pending` name that the bundled stdlib
/// net ALSO exports. In a workspace/auto-optimize build both crates are
/// linked, so `js_net_process_pending` is a duplicate symbol; the link binds
/// every reference to whichever twin wins (stdlib's). The aux pump
/// (`process_pending_aux`) and the extern wrapper above therefore call THIS
/// uniquely-named entry point instead — a symbol with no twin and nothing to
/// fold against — so the adopted raw-`'upgrade'` socket's `Close` event in
/// ext-net's own queue is actually drained rather than left to pin the event
/// loop forever. Without this the loop hung, and the behavior flipped with
/// unrelated code-size changes (link-order roulette). (#5010)
///
/// # Safety
/// Fires user JS closures (listeners); callers must hold a valid runtime.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_drain_pending() -> i32 {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<PendingNetEvent>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut events = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    events.clear();
    {
        let mut g = statics::pending_events().lock().unwrap();
        events.append(&mut *g);
    }
    let count = events.len() as i32;

    for ev in events.drain(..) {
        prepare_event_provider(&ev);
        let provider_id = event_provider_id(&ev);
        let destroy_ids: Vec<u64> = match &ev {
            PendingNetEvent::Connect(id, _) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| sockets.get(id).map(|socket| vec![socket.connect_async_id]))
                .unwrap_or_default(),
            PendingNetEvent::ShutdownComplete(id, _, _) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| sockets.get(id).map(|socket| vec![socket.shutdown_async_id]))
                .unwrap_or_default(),
            PendingNetEvent::Close(id) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| {
                    sockets.get(id).map(|socket| {
                        vec![
                            socket.connect_async_id,
                            socket.shutdown_async_id,
                            socket.tcp_async_id,
                        ]
                    })
                })
                .unwrap_or_default(),
            PendingNetEvent::ServerClose(id) => statics::servers()
                .lock()
                .ok()
                .and_then(|servers| servers.get(id).map(|server| vec![server.async_id]))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let provider_scope = ProviderScope::enter(provider_id);
        match ev {
            PendingNetEvent::Connect(id, local_server) => {
                server_state::finish_local_connect(local_server);
                // #8259: park the snapshot so callback N stays rooted (and is
                // rewritten on evacuation) while callback N-1 runs user JS.
                emit_socket_no_arg(id, "connect");
                // TLS sockets additionally fire 'secureConnect' once the
                // handshake completes — the direct-TLS connect path only
                // signals Connect after the handshake, so this is the right
                // tick. Plain sockets simply have no listeners here. #4971.
                extern "C" {
                    fn js_tls_client_is_connected(handle: i64) -> i32;
                }
                if js_tls_client_is_connected(id) != 0 {
                    emit_tls_secure_connect(id);
                }
            }
            PendingNetEvent::SecureConnect(id) => emit_tls_secure_connect(id),
            PendingNetEvent::Data(id, bytes) => {
                let cbs = listeners_for(id, "data");
                if cbs.is_empty() {
                    server_state::buffer_pending_server_data(id, bytes);
                    continue;
                }
                // #8259: park BEFORE the payload allocation below — it can
                // collect, and the evacuating arms then move the closures a
                // bare snapshot would still point at. The payload is parked
                // too: callback 1's JS can move it before callback 2 runs.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                // #4973: `socket.setEncoding(enc)` switches 'data' delivery
                // from Buffers to decoded strings (Node readable-stream
                // semantics). 'hex'/'base64' render their text forms; the
                // remaining text encodings decode as UTF-8 (lossy).
                let encoding = statics::encodings().lock().unwrap().get(&id).cloned();
                let payload_f64 = if let Some(enc) = encoding {
                    let s = match enc.as_str() {
                        "hex" => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        "base64" => adopt::base64_encode(&bytes),
                        _ => String::from_utf8_lossy(&bytes).into_owned(),
                    };
                    let hdr = alloc_string(&s);
                    f64::from_bits(
                        0x7FFF_0000_0000_0000 | (hdr.as_raw() as u64 & 0x0000_FFFF_FFFF_FFFF),
                    )
                } else {
                    let buf = alloc_buffer(&bytes);
                    if buf.is_null() {
                        continue;
                    }
                    // POINTER_TAG over the buffer pointer.
                    f64::from_bits(0x7FFD_0000_0000_0000 | (buf as u64 & 0x0000_FFFF_FFFF_FFFF))
                };
                frame.set_payload(payload_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "data");
            }
            PendingNetEvent::Error(id, msg) => {
                let cbs = listeners_for(id, "error");
                if cbs.is_empty() {
                    continue;
                }
                // #8259: park before the allocating build_error_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                // Issue #770 — emit an Error-shaped object `{message: msg}`
                // so user code can read `err.message`. Pre-fix this was a
                // raw NaN-boxed string and `err.message` was `undefined`.
                let err_f64 = build_error_object(&msg);
                frame.set_payload(err_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "error");
            }
            PendingNetEvent::AbortError(id) => {
                let cbs = listeners_for(id, "error");
                if cbs.is_empty() {
                    continue;
                }
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                extern "C" {
                    fn js_abort_error_value() -> f64;
                }
                frame.set_payload(js_abort_error_value().to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "error");
            }
            PendingNetEvent::End(id) => {
                // Issue #1852 — readable side ended (peer FIN). Fire the
                // `'end'` listeners; the trailing `Close` event (pushed
                // right after `End` in `run_socket_task`) does the actual
                // listener-map / socket-map teardown, so don't remove
                // anything here.
                let frame = dispatch_custody::DispatchFrame::park(listeners_for(id, "end"));
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "end");
            }
            PendingNetEvent::WriteComplete(_, completion, error)
            | PendingNetEvent::ShutdownComplete(_, completion, error) => {
                lifecycle::dispatch_socket_completion(completion, error);
            }
            PendingNetEvent::Close(id) => {
                lifecycle::drop_socket_completions(id);
                extern "C" {
                    fn js_tls_client_record_closed(handle: i64);
                }
                js_tls_client_record_closed(id);
                let had_error = f64::from_bits(JsValue::from_bool(false).bits());
                let frame = dispatch_custody::DispatchFrame::park(listeners_for(id, "close"));
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(had_error);
                    }
                }
                drop(frame);
                statics::listeners().lock().unwrap().remove(&id);
                statics::sockets().lock().unwrap().remove(&id);
                statics::once_flags().lock().unwrap().remove(&id);
                statics::encodings().lock().unwrap().remove(&id);
                statics::http_agent_phases().lock().unwrap().remove(&id);
                statics::max_listeners().lock().unwrap().remove(&id);
                server_state::discard_pending_server_data(id);
            }
            // Issue #1123 followup — server-side events. The
            // accept loop pushes `ServerConnection`/`ServerListening`/
            // `ServerError`/`ServerClose`; the main-thread pump
            // converts them into the appropriate JS dispatch.
            PendingNetEvent::ServerConnection(server_id, socket_id, released) => {
                if !released && server_state::defer_server_connection(server_id, socket_id) {
                    continue;
                }
                server_state::activate_connection(server_id, socket_id);
                let cbs = listeners_for(server_id, "connection");
                if cbs.is_empty() {
                    // Drain any `server.once('connection', cb)` flagged
                    // here too — listeners_for returned empty but the
                    // once-set may still be holding stale entries.
                    lifecycle::drain_once_listeners(server_id, "connection");
                    server_state::release_pending_server_data(socket_id);
                    server_state::release_connection_callback(socket_id);
                    continue;
                }
                // Sockets returned by the codegen's `net.connect`
                // path (`js_net_socket_connect` → NR_PTR ret kind in
                // lower_call.rs) are NaN-boxed with POINTER_TAG over
                // the raw socket id. Match that here so user code
                // sees the same value shape regardless of which side
                // produced the socket: `sock.on(...)` then dispatches
                // through the `("net", true, "on", Some("Socket"))`
                // NATIVE_MODULE_TABLE row (which `unbox_to_i64`s the
                // receiver back to the raw id). Bare-number sockets
                // skipped the dispatch and hit the generic property
                // path → `(number).on is not a function`.
                let sock_f64 = f64::from_bits(
                    0x7FFD_0000_0000_0000 | (socket_id as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                // #8259: sock_f64 is a handle id (not a heap address), so
                // only the callbacks need custody.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(sock_f64);
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "connection");
                server_state::release_pending_server_data(socket_id);
                server_state::release_connection_callback(socket_id);
            }
            PendingNetEvent::ServerListening(server_id) => {
                // Take + drain the 'listening' listeners so the
                // optional `listen(port, cb)` callback fires exactly
                // once (Node's semantics). Subsequent
                // `.on('listening', ...)` registrations would have
                // to wait for another `.listen(...)` cycle — fine,
                // re-binding without close() in between would error
                // on bind anyway.
                let cbs = {
                    let mut listeners = statics::listeners().lock().unwrap();
                    if let Some(per_server) = listeners.get_mut(&server_id) {
                        per_server.remove("listening").unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                // #8259: these were REMOVED from the table above (one-shot),
                // so this frame is their ONLY root during dispatch — without
                // it a collection here can free, not just move, them.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
            }
            PendingNetEvent::ServerClose(server_id) => {
                // Drain close listeners (one-shot, like Node).
                let cbs = {
                    let mut listeners = statics::listeners().lock().unwrap();
                    if let Some(per_server) = listeners.get_mut(&server_id) {
                        per_server.remove("close").unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                // #8259: removed from the table above — custody is the only
                // root; see the ServerListening arm.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
                // Tear down the server entry so the keepalive gate
                // (`js_ext_net_has_active_handles`) lets the runtime
                // exit cleanly after the user's close() resolves.
                server_state::remove_server(server_id);
                statics::servers().lock().unwrap().remove(&server_id);
                statics::listeners().lock().unwrap().remove(&server_id);
                statics::once_flags().lock().unwrap().remove(&server_id);
            }
            PendingNetEvent::ServerError(server_id, msg) => {
                let cbs = listeners_for(server_id, "error");
                if cbs.is_empty() {
                    // Node prints to stderr if there's no handler and
                    // crashes the process; we just log and continue —
                    // less hostile to test harnesses that haven't
                    // wired an error listener yet.
                    eprintln!("[perry-ext-net] server {} error: {}", server_id, msg);
                    continue;
                }
                // #8259: park before the allocating build_error_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                let err_f64 = build_error_object(&msg);
                frame.set_payload(err_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "error");
            }
            PendingNetEvent::ServerDrop(server_id, info) => {
                let cbs = listeners_for(server_id, "drop");
                if cbs.is_empty() {
                    lifecycle::drain_once_listeners(server_id, "drop");
                    continue;
                }
                // #8259: park before the allocating build_drop_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                let info = server_state::build_drop_object(&info);
                frame.set_payload(info.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "drop");
            }
        }
        drop(provider_scope);
        for async_id in destroy_ids {
            if async_id != 0 {
                js_async_hooks_provider_destroy(async_id);
            }
        }
    }

    // Restore the (capacity-retaining) buffer to the thread-local so the
    // next tick reuses it. A re-entrant pump call during dispatch may
    // have left a grown buffer in the slot — keep whichever is larger.
    SCRATCH.with(|s| {
        let mut slot = s.borrow_mut();
        if events.capacity() >= slot.capacity() {
            *slot = events;
        }
    });

    count
}
