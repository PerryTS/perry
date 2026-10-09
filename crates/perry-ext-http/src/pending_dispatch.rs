use super::*;
use perry_ffi::TransientRootScope;

/// Drain the pending HTTP-event queue and fire user callbacks. Events remain
/// in the shared queue until selected so re-entrant event-loop pumps can make
/// progress on later response chunks (#5783).
#[no_mangle]
pub unsafe extern "C" fn js_http_process_pending() -> i32 {
    let owner = perry_ffi::agent_post::current_agent();
    let mut count = 0i32;
    loop {
        let ev = match HTTP_PENDING_EVENTS.lock() {
            Ok(mut q) => take_event(&mut q, owner),
            Err(_) => return count,
        };
        let Some(mut ev) = ev else { break };
        // Queue ownership ends here. Root a transported Socket before async
        // hooks or listeners can collect and rewrite its ordinary object.
        let scope = TransientRootScope::enter();
        let socket = match &ev {
            PendingHttpEvent::Upgrade { socket_handle, .. } => {
                Some(scope.root_addr(*socket_handle))
            }
            _ => None,
        };
        count += 1;
        let request_handle = pending_request_handle(&ev);
        let terminal = terminal_http_event(&ev);
        let async_id = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |request| {
            request.async_id
        })
        .unwrap_or(0);
        if async_id != 0 {
            js_async_hooks_provider_enter(async_id);
        }
        if let (PendingHttpEvent::Upgrade { socket_handle, .. }, Some(socket)) = (&mut ev, &socket)
        {
            *socket_handle = socket.get();
        }
        match ev {
            PendingHttpEvent::Socket { request_handle } => {
                client_events::fire_request_socket_event(request_handle);
            }
            PendingHttpEvent::SignalAbort { request_handle } => {
                client_abort::handle_request_signal_abort(request_handle);
            }
            PendingHttpEvent::Response {
                request_handle,
                status,
                status_message,
                headers,
                trailers,
                body,
                http_version,
            } => client_events::handle_response_event(
                request_handle,
                status,
                status_message,
                headers,
                trailers,
                body,
                http_version,
            ),
            PendingHttpEvent::ResponseHead {
                request_handle,
                status,
                status_message,
                headers,
                http_version,
            } => client_events::handle_response_head_event(
                request_handle,
                status,
                status_message,
                headers,
                http_version,
            ),
            PendingHttpEvent::Upgrade {
                request_handle,
                status,
                status_message,
                headers,
                socket_handle,
                head,
            } => client_events::handle_upgrade_event(
                request_handle,
                status,
                status_message,
                headers,
                socket_handle,
                head,
            ),
            PendingHttpEvent::ResponseChunk {
                request_handle,
                chunk,
            } => client_events::handle_response_chunk_event(request_handle, chunk),
            PendingHttpEvent::ResponseEnd { request_handle } => {
                client_events::handle_response_end_event(request_handle);
            }
            PendingHttpEvent::Error {
                request_handle,
                error_message,
            } => client_events::handle_error_event(request_handle, &error_message),
            PendingHttpEvent::CodedError {
                request_handle,
                message,
                code,
            } => client_events::handle_coded_error_event(request_handle, &message, &code),
            PendingHttpEvent::TransportError {
                request_handle,
                message,
                code,
                syscall,
                errno,
            } => client_events::handle_transport_error_event(
                request_handle,
                &message,
                &code,
                &syscall,
                errno,
            ),
            PendingHttpEvent::Timeout { request_handle } => {
                client_events::handle_timeout_event(request_handle);
            }
            PendingHttpEvent::Abort { request_handle } => {
                client_events::fire_request_event_listeners(request_handle, "abort");
                client_events::fire_request_close_once(request_handle);
                finish_agent_request(request_handle, false);
            }
            PendingHttpEvent::Flushed { request_handle } => {
                client_events::handle_flushed_event(request_handle);
            }
            PendingHttpEvent::Continue { request_handle } => {
                client_events::fire_request_event_listeners(request_handle, "continue");
            }
            PendingHttpEvent::DeferredArmContinue { request_handle } => {
                continue_client::arm_expect_continue(request_handle);
            }
        }
        if async_id != 0 {
            js_async_hooks_provider_leave(async_id);
            if terminal {
                js_async_hooks_provider_destroy(async_id);
            }
        }
    }
    count
}

/// Select the next event of this heap, preserving each agent's FIFO order.
fn take_event(queue: &mut Vec<(u64, PendingHttpEvent)>, owner: u64) -> Option<PendingHttpEvent> {
    let index = queue.iter().position(|(agent, _)| *agent == owner)?;
    Some(queue.remove(index).1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_events_stay_queued_and_own_events_keep_fifo_order() {
        let mut queue = vec![
            (2, PendingHttpEvent::Abort { request_handle: 20 }),
            (1, PendingHttpEvent::Abort { request_handle: 10 }),
            (2, PendingHttpEvent::Abort { request_handle: 21 }),
        ];
        assert_eq!(
            pending_request_handle(&take_event(&mut queue, 1).unwrap()),
            10
        );
        assert!(take_event(&mut queue, 1).is_none());
        assert_eq!(
            pending_request_handle(&take_event(&mut queue, 2).unwrap()),
            20
        );
        assert_eq!(
            pending_request_handle(&take_event(&mut queue, 2).unwrap()),
            21
        );
        assert!(queue.is_empty());
    }
}
