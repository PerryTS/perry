//! Node writable corking: hold owned bytes until the outermost uncork or end.
use crate::{statics, SocketCommand, SocketState};

#[derive(Default)]
pub(crate) struct CorkBuffer {
    pub(crate) depth: u32,
    pub(crate) bytes: Vec<u8>,
    completions: Vec<u64>,
    has_writes: bool,
}

impl CorkBuffer {
    pub(crate) fn push(&mut self, bytes: Vec<u8>, completion: u64) {
        self.bytes.extend_from_slice(&bytes);
        self.has_writes = true;
        if completion != 0 {
            self.completions.push(completion);
        }
    }

    /// Release buffered bytes; callback custody is retired by Close.
    pub(crate) fn discard(&mut self) {
        self.bytes.clear();
        self.completions.clear();
        self.has_writes = false;
    }

    fn take_write(&mut self) -> Option<SocketCommand> {
        if !std::mem::take(&mut self.has_writes) {
            return None;
        }
        let bytes = std::mem::take(&mut self.bytes);
        let tokens = std::mem::take(&mut self.completions);
        // Callbacks stay in the existing GC-scanned custody table. Merge them
        // under the first token so one transport completion fires them in order.
        let token = tokens.first().copied().unwrap_or(0);
        if token != 0 {
            let mut table = crate::lifecycle::socket_completions().lock().unwrap();
            let mut callbacks = Vec::new();
            let mut owner = 0;
            for t in tokens {
                if let Some((id, mut cbs)) = table.remove(&t) {
                    owner = id;
                    callbacks.append(&mut cbs);
                }
            }
            table.insert(token, (owner, callbacks));
        }
        Some(SocketCommand::Write(bytes, token))
    }
}

impl SocketState {
    pub(crate) fn flush_cork(&mut self, id: i64) -> Result<(), String> {
        let Some(cmd) = self.cork.take_write() else {
            return Ok(());
        };
        let completion = match &cmd {
            SocketCommand::Write(_, token) => *token,
            _ => 0,
        };
        if let SocketCommand::Write(ref bytes, _) = cmd {
            self.bytes_queued = self.bytes_queued.saturating_sub(bytes.len() as u64);
        }
        let result = self.command_transport(id, cmd);
        if let Err(ref message) = result {
            // Queue the error; never invoke JS while holding the socket lock.
            crate::push_event(crate::PendingNetEvent::WriteComplete(
                id,
                completion,
                Some(message.clone()),
            ));
        }
        result
    }
}

#[no_mangle]
pub extern "C" fn js_net_socket_cork(handle: i64) -> i64 {
    if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.cork.depth = socket.cork.depth.saturating_add(1);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_net_socket_uncork(handle: i64) -> i64 {
    let failure = {
        let mut sockets = statics::sockets().lock().unwrap();
        sockets.get_mut(&handle).and_then(|socket| {
            socket.cork.depth = socket.cork.depth.saturating_sub(1);
            if socket.cork.depth == 0 {
                socket.flush_cork(handle).err()
            } else {
                None
            }
        })
    };
    if let Some(message) = failure {
        crate::turnloop_io::submission_failed(handle, 0, message);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_net_socket_get_writable_corked(handle: i64) -> f64 {
    statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|s| s.cork.depth as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corked_writes_form_one_ordered_transport_command() {
        let mut socket = SocketState::for_test(false);
        socket.turnloop = true;
        socket.cork.depth = 2;
        for part in ["P", "arse-message-", "B", "ind-portal-", "E"] {
            socket
                .command(123, SocketCommand::Write(part.as_bytes().to_vec(), 0))
                .unwrap();
        }
        assert_eq!(
            socket.bytes_queued,
            b"Parse-message-Bind-portal-E".len() as u64
        );
        assert_eq!(socket.cork.depth, 2);
        match socket.cork.take_write().unwrap() {
            SocketCommand::Write(bytes, 0) => assert_eq!(bytes, b"Parse-message-Bind-portal-E"),
            _ => panic!("expected one write"),
        }
        assert!(socket.cork.take_write().is_none());
    }
    #[test]
    fn batch_completion_retains_every_callback_in_write_order() {
        let _lock = crate::tests::GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut buffer = CorkBuffer::default();
        {
            let mut table = crate::lifecycle::socket_completions().lock().unwrap();
            table.insert(u64::MAX - 1, (-10526, vec![101]));
            table.insert(u64::MAX, (-10526, vec![102]));
        }
        buffer.push(vec![1], u64::MAX - 1);
        buffer.push(vec![2], 0);
        buffer.push(vec![3], u64::MAX);
        assert!(
            matches!(buffer.take_write(), Some(SocketCommand::Write(bytes, token))
            if bytes == [1,2,3] && token == u64::MAX - 1)
        );
        let mut table = crate::lifecycle::socket_completions().lock().unwrap();
        assert_eq!(
            table.remove(&(u64::MAX - 1)),
            Some((-10526, vec![101, 102]))
        );
        assert!(!table.contains_key(&u64::MAX));
    }
    #[test]
    fn cork_before_connect_holds_bytes_without_a_transport() {
        let mut socket = SocketState::for_test(true);
        socket.cork.depth = 1;
        socket
            .command(-10526, SocketCommand::Write(vec![1, 2], 0))
            .unwrap();
        assert_eq!(socket.bytes_queued, 2);
        assert_eq!(socket.cork.bytes, [1, 2]);
        assert!(!socket.unconnected_write_failed);
    }
    #[test]
    fn destroying_unopened_corked_socket_orders_and_retires_completions_once() {
        let _lock = crate::tests::GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let id = unsafe { crate::js_net_socket_alloc() };
        {
            let mut sockets = statics::sockets().lock().unwrap();
            let socket = sockets.get_mut(&id).unwrap();
            socket.cork.depth = 1;
            // Insert out of order so this verifies token ordering rather than
            // HashMap insertion order. Empty callback vectors need no JS heap.
            for token in [u64::MAX - 2, u64::MAX - 4, u64::MAX - 3] {
                crate::lifecycle::socket_completions()
                    .lock()
                    .unwrap()
                    .insert(token, (id, Vec::new()));
                socket
                    .command(id, SocketCommand::Write(vec![1], token))
                    .unwrap();
            }
        }
        crate::lifecycle::js_ext_net_destroy_socket(id);
        crate::lifecycle::js_ext_net_destroy_socket(id);
        assert_eq!(
            crate::lifecycle::pending_socket_completions(id),
            [u64::MAX - 4, u64::MAX - 3, u64::MAX - 2]
        );
        assert_eq!(statics::sockets().lock().unwrap()[&id].bytes_queued, 0);
        assert_eq!(
            statics::pending_events()
                .lock()
                .unwrap()
                .iter()
                .filter(|ev| matches!(ev, crate::PendingNetEvent::Close(h) if *h == id))
                .count(),
            1
        );
        unsafe {
            crate::js_net_process_pending();
        }
        assert!(crate::lifecycle::pending_socket_completions(id).is_empty());
        assert!(!statics::sockets().lock().unwrap().contains_key(&id));
    }
}
