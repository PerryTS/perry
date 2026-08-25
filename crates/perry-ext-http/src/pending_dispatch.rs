use super::*;

/// Drain the pending HTTP-event queue and fire user callbacks. Events remain
/// in the shared queue until selected so re-entrant event-loop pumps can make
/// progress on later response chunks (#5783).
#[no_mangle]
pub unsafe extern "C" fn js_http_process_pending() -> i32 {
    let mut count = 0i32;
    loop {
        let ev = match HTTP_PENDING_EVENTS.lock() {
            Ok(mut q) => {
                if q.is_empty() {
                    None
                } else {
                    Some(q.remove(0))
                }
            }
            Err(_) => return count,
        };
        let Some(ev) = ev else { break };
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
        match ev {
            PendingHttpEvent::Socket { request_handle } => {
                client_events::fire_request_socket_event(request_handle);
            }
            PendingHttpEvent::SignalAbort { request_handle } => {
                client_abort::handle_request_signal_abort(request_handle);
            }
            PendingHttpEvent::AgentIdleExpire {
                agent_handle,
                key,
                socket,
                generation,
            } => agent::expire_free_socket(agent_handle, &key, socket, generation),
            PendingHttpEvent::Response {
                request_handle,
                status,
                status_message,
                headers,
                trailers,
                body,
            } => client_events::handle_response_event(
                request_handle,
                status,
                status_message,
                headers,
                trailers,
                body,
            ),
            PendingHttpEvent::ResponseHead {
                request_handle,
                status,
                status_message,
                headers,
            } => client_events::handle_response_head_event(
                request_handle,
                status,
                status_message,
                headers,
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
