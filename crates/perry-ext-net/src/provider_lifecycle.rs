use super::*;

pub(super) unsafe fn init_provider(name: &'static [u8]) -> u64 {
    js_async_hooks_provider_init(name.as_ptr(), name.len())
}

pub(super) unsafe fn init_provider_with_trigger(name: &'static [u8], trigger_async_id: u64) -> u64 {
    js_async_hooks_provider_init_with_trigger(name.as_ptr(), name.len(), trigger_async_id)
}

pub(super) struct ProviderScope(u64);

impl ProviderScope {
    pub(super) unsafe fn enter(async_id: u64) -> Self {
        if async_id != 0 {
            js_async_hooks_provider_enter(async_id);
        }
        Self(async_id)
    }
}

impl Drop for ProviderScope {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { js_async_hooks_provider_leave(self.0) };
        }
    }
}

pub(super) unsafe fn prepare_event_provider(ev: &PendingNetEvent) {
    match ev {
        PendingNetEvent::ServerConnection(server_id, socket_id, _) => {
            let server_async_id = statics::servers()
                .lock()
                .ok()
                .and_then(|servers| servers.get(server_id).map(|server| server.async_id))
                .unwrap_or(0);
            let needs_init = statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| {
                    sockets
                        .get(socket_id)
                        .map(|socket| socket.tcp_async_id == 0)
                })
                .unwrap_or(false);
            if needs_init {
                let async_id = init_provider_with_trigger(b"TCPWRAP", server_async_id);
                if let Some(socket) = statics::sockets().lock().unwrap().get_mut(socket_id) {
                    socket.tcp_async_id = async_id;
                }
            }
        }
        PendingNetEvent::ShutdownComplete(id) => {
            let trigger = statics::sockets().lock().ok().and_then(|sockets| {
                sockets.get(id).and_then(|socket| {
                    (socket.shutdown_async_id == 0).then_some(socket.tcp_async_id)
                })
            });
            if let Some(trigger) = trigger {
                let async_id = init_provider_with_trigger(b"SHUTDOWNWRAP", trigger);
                if let Some(socket) = statics::sockets().lock().unwrap().get_mut(id) {
                    socket.shutdown_async_id = async_id;
                }
            }
        }
        _ => {}
    }
}

pub(super) fn event_provider_id(ev: &PendingNetEvent) -> u64 {
    match ev {
        PendingNetEvent::Connect(id, _) => statics::sockets()
            .lock()
            .ok()
            .and_then(|sockets| sockets.get(id).map(|socket| socket.connect_async_id))
            .unwrap_or(0),
        PendingNetEvent::Data(id, _)
        | PendingNetEvent::End(id)
        | PendingNetEvent::Error(id, _)
        | PendingNetEvent::AbortError(id)
        | PendingNetEvent::Close(id) => statics::sockets()
            .lock()
            .ok()
            .and_then(|sockets| sockets.get(id).map(|socket| socket.tcp_async_id))
            .unwrap_or(0),
        PendingNetEvent::ShutdownComplete(id) => statics::sockets()
            .lock()
            .ok()
            .and_then(|sockets| sockets.get(id).map(|socket| socket.shutdown_async_id))
            .unwrap_or(0),
        PendingNetEvent::ServerConnection(_, socket_id, _) => statics::sockets()
            .lock()
            .ok()
            .and_then(|sockets| sockets.get(socket_id).map(|socket| socket.tcp_async_id))
            .unwrap_or(0),
        PendingNetEvent::ServerListening(id)
        | PendingNetEvent::ServerClose(id)
        | PendingNetEvent::ServerError(id, _)
        | PendingNetEvent::ServerDrop(id, _) => statics::servers()
            .lock()
            .ok()
            .and_then(|servers| servers.get(id).map(|server| server.async_id))
            .unwrap_or(0),
    }
}
