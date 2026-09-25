//! An HTTP exchange over a socket JS produced: `agent.createConnection` /
//! `createSocket` (#2154) and the request-level `createConnection` (#10469).
//!
//! The socket belongs to `perry-ext-net`, which already runs it on the agent's
//! turnloop loop (#11105) and, for `tls.connect`, decrypts above it. This
//! module never touches the handle directly: it speaks to the socket through
//! perry-ffi's raw-net vtable (`attach` / `write` / `poll_read` / `detach` /
//! `close`), so the user's socket keeps its own connect/TLS state and JS
//! identity, exactly as before.
//!
//! What changed is how the bytes are *waited for*. This used to be a tokio task
//! that called `poll_read` in a loop with a 1 ms `tokio::time::sleep` between
//! empty reads. Now `perry-ext-net` calls `perry_ffi::raw_net_notify` when a
//! raw-mode socket gains bytes or goes terminal, and [`raw_socket_ready`] schedules a
//! drain on a 0 ms loop timer, so the socket is read when there is something
//! to read and not otherwise. The drain runs from this module's own completion
//! (never inside `perry-ext-net`'s dispatch, per `RawNetNotify`'s contract).
//!
//! The exchange itself is unchanged: the request goes out in one write with
//! `Connection: close` (`client_connect_override::serialize_http_request`), the
//! response is read to EOF and parsed with `plain_client::parse_http_response`,
//! and a `101` to an upgrade request detaches the socket and hands it to the
//! request's `'upgrade'` listener with the bytes after the head. The deadline
//! keeps its old default of 30 s; it is a loop timer now.

use std::sync::atomic::Ordering;

use perry_ffi::Handle;

use super::{next_id, on_loop, run, with_state, Effect, State, Timer, RAW_COMPLETED};
use crate::plain_client::parse_http_response;
use crate::{push_event, ClientInflightGuard, PendingHttpEvent};

/// The deadline the tokio task applied when the request set none.
const DEFAULT_DEADLINE_MS: u64 = 30_000;

pub(super) struct RawExchange {
    request_handle: Handle,
    wants_upgrade: bool,
    /// Response bytes read so far; parsed once the peer closes.
    raw: Vec<u8>,
    /// Armed drain timer, `0` when none is pending.
    drain_timer: i64,
    deadline_timer: i64,
    _inflight: ClientInflightGuard,
}

/// Start the exchange. Called on the JS thread with the socket already in
/// raw mode (`attach`ed by the caller so no byte can reach a JS `'data'`
/// listener first). Delivers exactly one terminal event for the request.
pub(crate) fn start_raw_exchange(
    request_handle: Handle,
    request: Vec<u8>,
    wants_upgrade: bool,
    timeout_ms: Option<u64>,
    socket_id: i64,
) {
    let Some(vtable) = perry_ffi::raw_net() else {
        push_event(PendingHttpEvent::Error {
            request_handle,
            error_message: "agent.createConnection requires node:net (not linked)".to_string(),
        });
        return;
    };
    perry_ffi::register_raw_net_notify(raw_socket_ready);
    let inflight = ClientInflightGuard::new(request_handle);
    let carried = on_loop(move || {
        (vtable.attach)(socket_id);
        if (vtable.write)(socket_id, request.as_ptr(), request.len()) == 0 {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: "failed to write request to agent socket".to_string(),
            });
            drop(inflight);
            return;
        }
        let effects = with_state(|st| {
            let mut fx = Vec::new();
            let deadline = next_id();
            let deadline_timer = if deadline == perry_ffi::INVALID_HANDLE {
                0
            } else {
                st.timers
                    .insert(deadline, Timer::RawDeadline { socket: socket_id });
                fx.push(Effect::ArmTimer(
                    deadline,
                    timeout_ms.unwrap_or(DEFAULT_DEADLINE_MS),
                ));
                deadline
            };
            st.raw.insert(
                socket_id,
                RawExchange {
                    request_handle,
                    wants_upgrade,
                    raw: Vec::new(),
                    drain_timer: 0,
                    deadline_timer,
                    _inflight: inflight,
                },
            );
            // Bytes may already be buffered (a server that answers before the
            // request finished writing): drain once without waiting for a
            // notification.
            schedule_raw_drain(st, socket_id, &mut fx);
            fx
        });
        run(effects);
    });
    if !carried {
        push_event(PendingHttpEvent::TransportError {
            request_handle,
            message: format!("connect {}", super::NO_LOOP_CODE),
            code: super::NO_LOOP_CODE.to_string(),
            syscall: "connect".to_string(),
            errno: perry_ffi::turnloop_net::errno_for_code(super::NO_LOOP_CODE) as i64,
        });
    }
}

/// `perry_ffi::raw_net_notify`'s target. Runs inside `perry-ext-net`'s
/// completion handling, so it only schedules; see [`drain_raw_socket`].
extern "C" fn raw_socket_ready(socket_id: i64) {
    let effects = with_state(|st| {
        let mut fx = Vec::new();
        schedule_raw_drain(st, socket_id, &mut fx);
        fx
    });
    run(effects);
}

fn schedule_raw_drain(st: &mut State, socket_id: i64, fx: &mut Vec<Effect>) {
    let Some(exchange) = st.raw.get_mut(&socket_id) else {
        return;
    };
    if exchange.drain_timer != 0 {
        return;
    }
    let timer = next_id();
    if timer == perry_ffi::INVALID_HANDLE {
        // No id for a timer: drain from the effect queue instead, which runs
        // after the current dispatch has returned.
        fx.push(Effect::RawDrain(socket_id));
        return;
    }
    exchange.drain_timer = timer;
    st.timers
        .insert(timer, Timer::RawDrain { socket: socket_id });
    fx.push(Effect::ArmTimer(timer, 0));
}

/// A drain timer fired: forget it, then read (outside the lock).
pub(super) fn on_raw_drain_timer(st: &mut State, socket_id: i64, timer: i64) -> Vec<Effect> {
    if let Some(exchange) = st.raw.get_mut(&socket_id) {
        if exchange.drain_timer == timer {
            exchange.drain_timer = 0;
        }
    }
    vec![Effect::RawDrain(socket_id)]
}

/// The deadline fired with the exchange still open.
pub(super) fn on_raw_deadline(st: &mut State, socket_id: i64) -> Vec<Effect> {
    let Some(exchange) = st.raw.remove(&socket_id) else {
        return Vec::new();
    };
    let mut fx = Vec::new();
    if exchange.drain_timer != 0 && st.timers.remove(&exchange.drain_timer).is_some() {
        fx.push(Effect::CancelTimer(exchange.drain_timer));
    }
    fx.push(Effect::RawClose(socket_id));
    fx.push(Effect::Push(PendingHttpEvent::Timeout {
        request_handle: exchange.request_handle,
    }));
    fx
}

/// Settle an exchange: take it out of the table and cancel its timers.
fn settle_raw_exchange(
    st: &mut State,
    socket_id: i64,
    fx: &mut Vec<Effect>,
) -> Option<RawExchange> {
    let exchange = st.raw.remove(&socket_id)?;
    for timer in [exchange.drain_timer, exchange.deadline_timer] {
        if timer != 0 && st.timers.remove(&timer).is_some() {
            fx.push(Effect::CancelTimer(timer));
        }
    }
    Some(exchange)
}

/// Read everything `perry-ext-net` has buffered for the socket, and finish the
/// exchange if the response is complete. Runs from the effect queue with no
/// lock held: the vtable calls re-enter `perry-ext-net`, whose teardown can
/// call [`raw_socket_ready`] synchronously.
pub(super) fn drain_raw_socket(socket_id: i64) {
    let Some(vtable) = perry_ffi::raw_net() else {
        return;
    };
    let mut chunk = [0u8; 16 * 1024];
    loop {
        if !with_state(|st| st.raw.contains_key(&socket_id)) {
            return;
        }
        let n = (vtable.poll_read)(socket_id, chunk.as_mut_ptr(), chunk.len());
        if n < 0 {
            // Would block: the next notification brings more.
            return;
        }
        if n == 0 {
            // Clean EOF: the response is whatever arrived.
            let (effects, exchange) = with_state(|st| {
                let mut fx = Vec::new();
                let exchange = settle_raw_exchange(st, socket_id, &mut fx);
                (fx, exchange)
            });
            run(effects);
            (vtable.close)(socket_id);
            if let Some(exchange) = exchange {
                deliver_raw_response(exchange);
            }
            return;
        }
        let bytes = &chunk[..n as usize];
        let upgraded = with_state(|st| {
            let exchange = st.raw.get_mut(&socket_id)?;
            exchange.raw.extend_from_slice(bytes);
            if !exchange.wants_upgrade {
                return None;
            }
            let end = exchange.raw.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
            let status = std::str::from_utf8(&exchange.raw[..end])
                .ok()
                .and_then(|head| head.lines().next())
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|code| code.parse::<u16>().ok());
            (status == Some(101)).then_some(end)
        });
        if let Some(end) = upgraded {
            let (effects, exchange) = with_state(|st| {
                let mut fx = Vec::new();
                let exchange = settle_raw_exchange(st, socket_id, &mut fx);
                (fx, exchange)
            });
            run(effects);
            let Some(exchange) = exchange else {
                return;
            };
            hand_off_raw_upgrade(exchange, socket_id, end);
            return;
        }
    }
}

fn hand_off_raw_upgrade(exchange: RawExchange, socket_id: i64, head_end: usize) {
    let Some(vtable) = perry_ffi::raw_net() else {
        return;
    };
    RAW_COMPLETED.fetch_add(1, Ordering::Relaxed);
    match parse_http_response(&exchange.raw[..head_end]) {
        Ok(parsed) => {
            (vtable.detach)(socket_id);
            push_event(PendingHttpEvent::Upgrade {
                request_handle: exchange.request_handle,
                status: parsed.status,
                status_message: parsed.status_message,
                headers: parsed.headers,
                socket_handle: socket_id,
                head: exchange.raw[head_end..].to_vec(),
            });
        }
        Err(error_message) => {
            (vtable.close)(socket_id);
            push_event(PendingHttpEvent::Error {
                request_handle: exchange.request_handle,
                error_message,
            });
        }
    }
}

fn deliver_raw_response(exchange: RawExchange) {
    match parse_http_response(&exchange.raw) {
        Ok(parsed) => {
            RAW_COMPLETED.fetch_add(1, Ordering::Relaxed);
            push_event(PendingHttpEvent::Response {
                request_handle: exchange.request_handle,
                status: parsed.status,
                status_message: parsed.status_message,
                headers: parsed.headers,
                trailers: parsed.trailers,
                body: parsed.body,
                http_version: parsed.http_version,
            });
        }
        Err(error_message) => push_event(PendingHttpEvent::Error {
            request_handle: exchange.request_handle,
            error_message,
        }),
    }
}

/// Close the socket for a deadline (from the effect queue, no lock held).
pub(super) fn close_raw_socket(socket_id: i64) {
    if let Some(vtable) = perry_ffi::raw_net() {
        (vtable.close)(socket_id);
    }
}
