//! Server liveness is maintained when its existing state changes. No handle
//! enumeration and no remembered active bit: the state is the authority.
use super::{deferred_events::server_is_active, HttpServer};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

// Primary-agent pump threads share the primary count. Worker servers retain
// their creator's counter, so destruction on another thread decrements the
// right agent and never keeps that thread's agent alive.
static PRIMARY: LazyLock<Arc<AtomicUsize>> = LazyLock::new(|| Arc::new(AtomicUsize::new(0)));
thread_local! {
    static WORKER: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
}

pub(crate) fn current_count() -> Arc<AtomicUsize> {
    if perry_ffi::agent_post::current_agent() == 0 {
        Arc::clone(&PRIMARY)
    } else {
        WORKER.with(Arc::clone)
    }
}

pub(crate) fn has_active_servers() -> bool {
    if perry_ffi::agent_post::current_agent() == 0 {
        PRIMARY.load(Ordering::Relaxed) != 0
    } else {
        WORKER.with(|count| count.load(Ordering::Relaxed) != 0)
    }
}

impl HttpServer {
    pub(crate) fn finish_activity_change(&self, was_active: bool) {
        match (was_active, server_is_active(self)) {
            (false, true) => {
                self.activity_count.fetch_add(1, Ordering::Relaxed);
            }
            (true, false) => {
                self.activity_count.fetch_sub(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub(crate) fn set_listening(&mut self, listening: bool) {
        let before = server_is_active(self);
        self.listening = listening;
        self.finish_activity_change(before);
    }

    pub(crate) fn set_refed(&mut self, refed: bool) {
        let before = server_is_active(self);
        self.refed = refed;
        self.finish_activity_change(before);
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        if server_is_active(self) {
            self.activity_count.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::deferred_events::*;
    use super::*;

    #[test]
    fn activity_count_covers_ref_deferred_close_and_removal() {
        let count = current_count();
        let baseline = count.load(Ordering::Relaxed);
        let mut server = HttpServer::with_handler(0);
        server.set_listening(true);
        server.set_listening(true);
        assert_eq!(count.load(Ordering::Relaxed), baseline + 1);
        server.set_refed(false);
        assert_eq!(count.load(Ordering::Relaxed), baseline);
        queue_deferred_listening_emit(&mut server, 0);
        assert_eq!(count.load(Ordering::Relaxed), baseline + 1);
        server.set_listening(false);
        queue_deferred_close_emit(&mut server, 0);
        let handle = perry_ffi::register_handle(server);
        assert_eq!(drain_deferred_listen_for::<HttpServer, _>(handle, |s| s), 0);
        assert_eq!(count.load(Ordering::Relaxed), baseline + 1);
        assert_eq!(drain_deferred_close_for::<HttpServer, _>(handle, |s| s), 0);
        assert_eq!(count.load(Ordering::Relaxed), baseline);
        assert!(perry_ffi::drop_handle(handle));
        assert!(!perry_ffi::handle_exists(handle));

        // Removal must also release an active server, including wrappers.
        let mut server = HttpServer::with_handler(0);
        server.set_listening(true);
        let handle = perry_ffi::register_handle(server);
        assert_eq!(count.load(Ordering::Relaxed), baseline + 1);
        assert!(perry_ffi::drop_handle(handle));
        assert_eq!(count.load(Ordering::Relaxed), baseline);
    }

    #[test]
    fn activity_count_does_not_include_another_agents_servers() {
        let primary = current_count();
        let baseline = primary.load(Ordering::Relaxed);
        let server = std::thread::spawn(|| {
            perry_runtime::agent::enter_worker_agent();
            let mut server = HttpServer::with_handler(0);
            server.set_listening(true);
            assert!(has_active_servers());
            server
        })
        .join()
        .unwrap();
        assert_eq!(primary.load(Ordering::Relaxed), baseline);
        let worker_count = Arc::clone(&server.activity_count);
        drop(server);
        assert_eq!(worker_count.load(Ordering::Relaxed), 0);
        // A second primary pump thread observes the primary's same count.
        let mut server = HttpServer::with_handler(0);
        server.set_listening(true);
        assert!(std::thread::spawn(has_active_servers).join().unwrap());
        drop(server);
        assert_eq!(primary.load(Ordering::Relaxed), baseline);
    }

    #[test]
    fn closed_request_handles_are_removed() {
        use super::super::in_flight::finalize_request_handles_deferred;
        use crate::server::response::ServerResponse;
        let request = perry_ffi::register_handle(123_u32);
        let response = perry_ffi::register_handle(ServerResponse::new());
        assert!(perry_ffi::handle_exists(request));
        assert!(perry_ffi::handle_exists(response));
        finalize_request_handles_deferred(request, response, None);
        assert!(!perry_ffi::handle_exists(request));
        assert!(!perry_ffi::handle_exists(response));
    }
}

/// Count client sessions with the same per-agent liveness total as servers.
pub(crate) fn new_session_count(active: bool) -> Arc<AtomicUsize> {
    let count = current_count();
    if active {
        count.fetch_add(1, Ordering::Relaxed);
    }
    count
}

impl crate::server::http2_server::Http2SessionHandle {
    pub(crate) fn mark_closed(&mut self) {
        if self.session_type == 1 && !self.closed && !self.destroyed {
            self.activity_count.fetch_sub(1, Ordering::Relaxed);
        }
        self.closed = true;
        self.destroyed = true;
    }
}

impl Drop for crate::server::http2_server::Http2SessionHandle {
    fn drop(&mut self) {
        self.mark_closed();
    }
}
