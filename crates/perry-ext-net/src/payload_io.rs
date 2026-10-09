//! Link-token completion sink. JS runs after the driver's turn has returned.

use super::{payload_closed as closed, payload_events as events, payload_server as server};
use super::{payload_provider as provider, payload_socket as socket, payload_transport as p};
use perry_ffi::native_payload::{self as np, OwnerLink};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};

pub(crate) const ROUTE: u8 = crate::TURNLOOP_SUBSYSTEM;
pub(crate) const TLS_SUBSYSTEM: u8 = 4;

pub(crate) fn register() {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        assert!(
            tl::register_link_sink(ROUTE, sink),
            "net link sink registration refused"
        );
        assert!(
            tl::register_link_sink(TLS_SUBSYSTEM, sink),
            "TLS wrapper link sink registration refused"
        );
    });
}

pub(crate) fn enabled() -> bool {
    register();
    tl::available(ROUTE)
}

pub(crate) fn snapshot(socket: OwnerLink) -> Option<tl::HandleSnapshot> {
    unsafe {
        p::socket_core(socket)
            .ok()
            .and_then(|core| tl::link_snapshot_handle(&mut *core, socket))
    }
}
pub(crate) fn matches(socket: OwnerLink, snapshot: &tl::HandleSnapshot) -> bool {
    unsafe {
        p::socket_core(socket)
            .ok()
            .is_some_and(|core| tl::link_handle_matches(&mut *core, socket, snapshot))
    }
}

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    let event = unsafe { &*completion };
    let Some(link) = event.link() else {
        return;
    };
    let Some(owner) = (unsafe { np::link_event_owner(link) }) else {
        return;
    };
    let scope = TransientRootScope::enter();
    let original = scope.root_nanbox(owner);
    let owner = scope.root_nanbox(
        if p::socket_link(original.get()).is_ok()
            && (event.kind == tl::NET_CLOSED || super::payload_tls::installed(link))
        {
            let state = scope.root_nanbox(unsafe { p::socket_state(link, false) });
            let pending = scope.root_nanbox(
                unsafe { event.closed_handle_parts() }
                    .map(|parts| closed::peek(state.get(), parts))
                    .unwrap_or_else(p::undefined),
            );
            let saved = scope.root_nanbox(p::own_get(pending.get(), "tlsWrapper"));
            let wrapper = scope.root_nanbox(if p::socket_link(saved.get()).is_ok() {
                saved.get()
            } else if event.kind != tl::NET_CLOSED
                || (!JsValue::from_bits(pending.get().to_bits()).is_pointer()
                    && event.flags & tl::NET_FLAG_STALE == 0)
            {
                let current = scope.root_nanbox(p::own_get(state.get(), "tlsWrapper"));
                if p::socket_link(current.get())
                    .is_ok_and(|own| unsafe { p::socket_ptr(own) }.is_ok())
                {
                    current.get()
                } else {
                    original.get()
                }
            } else {
                original.get()
            });
            if p::socket_link(wrapper.get()).is_ok() {
                wrapper.get()
            } else {
                original.get()
            }
        } else {
            original.get()
        },
    );
    if p::server_link(owner.get()).is_ok() {
        match event.kind {
            tl::NET_ACCEPT => server::accepted(owner.get(), completion),
            tl::NET_CLOSED => server::listener_closed(owner.get(), event),
            tl::NET_ERROR => {
                let snapshot = unsafe {
                    p::server_core(link)
                        .ok()
                        .and_then(|core| tl::link_snapshot_handle(&mut *core, link))
                };
                let error = socket::error(
                    unsafe { event.code() }.unwrap_or("EIO"),
                    &error_message(event),
                );
                events::emit(owner.get(), "error", &[error]);
                if snapshot.as_ref().is_some_and(|snapshot| unsafe {
                    p::server_core(link)
                        .ok()
                        .is_some_and(|core| tl::link_handle_matches(&mut *core, link, snapshot))
                }) {
                    server::close(owner.get(), p::undefined());
                }
            }
            _ => {}
        }
        return;
    }
    if p::socket_link(owner.get()).is_err() {
        return;
    }
    let _account = p::AccountSocket(link);
    match event.kind {
        tl::NET_CONNECT => connected(owner.get()),
        tl::NET_DATA => {
            let bytes = unsafe { event.bytes() };
            if event.flags & tl::NET_FLAG_PLAINTEXT == 0 && super::payload_tls::installed(link) {
                super::payload_tls::receive(owner.get(), bytes);
                return;
            }
            unsafe {
                if let Ok(payload) = p::socket_ptr(link) {
                    (*payload).ext.bytes_read =
                        (*payload).ext.bytes_read.saturating_add(bytes.len() as u64);
                }
            }
            socket::refresh_timeout(link);
            socket::data(owner.get(), bytes);
        }
        tl::NET_EOF => eof(owner.get()),
        tl::NET_WROTE => {
            if super::payload_tls::installed(link) {
                super::payload_tls::wrote(owner.get(), event.len);
            } else {
                wrote(owner.get(), event.user, event.len, event.queued);
            }
        }
        tl::NET_SHUTDOWN => shutdown(owner.get(), event.user),
        tl::NET_CLOSED => {
            if owner.get().to_bits() != original.get().to_bits() {
                // A wrapper may reopen on its own cell before this parent
                // handle's Closed arrives. It remains an owner event only.
                let mut closed = *event;
                if socket::link(owner.get()) != link {
                    closed.flags |= tl::NET_FLAG_STALE;
                }
                // Dispose both resources before either close listener runs.
                // A listener may reopen either object during the first emit.
                let wrapper_record = scope.root_nanbox(prepare_socket_closed(owner.get(), &closed));
                let parent_record = scope.root_nanbox(prepare_socket_closed(original.get(), event));
                emit_socket_closed(owner.get(), wrapper_record.get());
                emit_socket_closed(original.get(), parent_record.get());
            } else {
                socket_closed(owner.get(), event);
            }
        }
        tl::NET_ERROR => {
            let error = socket::error(
                unsafe { event.code() }.unwrap_or("EIO"),
                &error_message(event),
            );
            socket::destroy(owner.get(), error);
        }
        tl::NET_TIMER => {
            events::emit(owner.get(), "timeout", &[]);
        }
        _ => {}
    }
}

fn error_message(event: &tl::NetCompletion) -> String {
    format!(
        "{} {}",
        unsafe { event.syscall() }.unwrap_or("read"),
        unsafe { event.code() }.unwrap_or("EIO")
    )
}

pub(crate) fn connected(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let Some(snapshot) = snapshot(link) else {
        return;
    };
    unsafe {
        if let Ok(payload) = p::socket_ptr(link) {
            (*payload).ext.connecting = false;
            (*payload).ext.opened = true;
        }
    }
    socket::update_addresses(link);
    socket::refresh_timeout(link);
    super::payload_tls::begin_connected(owner.get());
    if !matches(link, &snapshot) {
        return;
    }
    let state = scope.root_nanbox(socket::state(owner.get()));
    let connect = scope.root_nanbox(p::own_get(state.get(), provider::CONNECT));
    events::emit_in(connect.get(), owner.get(), "connect", &[]);
    provider::retire(connect.get());
    if !matches(link, &snapshot) {
        return;
    }
    events::emit(owner.get(), "ready", &[]);
    if !matches(link, &snapshot) {
        return;
    }
    socket::flow(owner.get());
}

pub(crate) fn eof(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let Some(snapshot) = snapshot(link) else {
        return;
    };
    let emit = unsafe {
        p::socket_ptr(link).ok().is_some_and(|payload| {
            let fields = &mut (*payload).ext;
            fields.read_ended = true;
            if fields.read_end_emitted || !fields.read_buffer.is_empty() {
                return false;
            }
            fields.read_end_emitted = true;
            true
        })
    };
    if !emit {
        return;
    }
    events::emit(owner.get(), "end", &[]);
    if !matches(link, &snapshot) {
        return;
    }
    let action = unsafe {
        p::socket_ptr(link).ok().map(|payload| {
            let fields = &(*payload).ext;
            (
                fields.allow_half_open,
                fields.write_ended,
                fields.shutdown_done,
            )
        })
    };
    if let Some((false, false, _)) = action {
        socket::end(owner.get(), p::undefined(), p::undefined(), p::undefined());
    } else if let Some((_, _, true)) = action {
        socket::destroy(owner.get(), p::undefined());
    }
}

pub(crate) fn wrote(owner: f64, user: u64, len: usize, queued: usize) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    if snapshot(link).is_none() {
        return;
    }
    let users = unsafe {
        p::socket_ptr(link).ok().map(|payload| {
            let fields = &mut (*payload).ext;
            fields.bytes_written = fields.bytes_written.saturating_add(len as u64);
            fields.queued = queued.saturating_add(fields.cork_bytes.len());
            if fields
                .coalesced_users
                .front()
                .is_some_and(|group| group.0 == user)
            {
                fields.coalesced_users.pop_front().unwrap().1
            } else {
                vec![user]
            }
        })
    };
    socket::refresh_timeout(link);
    // The acknowledged callbacks no longer belong to the writable queue.
    // Root them and their original provider before a drain listener can
    // destroy/reopen the Socket or enqueue another write.
    let callbacks = users
        .unwrap_or_default()
        .into_iter()
        .map(|user| scope.root_nanbox(socket::take_callback(owner.get(), user)))
        .collect::<Vec<_>>();
    let resource = scope.root_nanbox(provider::resource(owner.get()));
    let drain = unsafe {
        p::socket_ptr(link).ok().is_some_and(|payload| {
            let fields = &mut (*payload).ext;
            if fields.need_drain && fields.queued == 0 {
                fields.need_drain = false;
                true
            } else {
                false
            }
        })
    };
    if drain {
        events::emit(owner.get(), "drain", &[]);
    }
    // Node emits drain before the completed write callbacks. They are already
    // successful completions, including if the drain listener closed its old
    // transport; none of these calls projects the possibly replaced payload.
    for callback in callbacks {
        if socket::is_callback(callback.get()) {
            events::call_in(resource.get(), callback.get(), JsThis::UNDEFINED, &[]);
        }
    }
}

pub(crate) fn shutdown(owner: f64, user: u64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let Some(snapshot) = snapshot(link) else {
        return;
    };
    let state = scope.root_nanbox(socket::state(owner.get()));
    let resource = scope.root_nanbox(p::own_get(state.get(), provider::SHUTDOWN));
    let users = unsafe {
        p::socket_ptr(link).ok().map(|payload| {
            let fields = &mut (*payload).ext;
            fields.shutdown_done = true;
            let mut users = vec![user];
            users.append(&mut fields.extra_end_users);
            users
        })
    };
    for user in users.unwrap_or_default() {
        if !matches(link, &snapshot) {
            return;
        }
        let callback = scope.root_nanbox(socket::take_callback(owner.get(), user));
        if socket::is_callback(callback.get()) {
            events::call_in(resource.get(), callback.get(), JsThis::UNDEFINED, &[]);
        }
    }
    if !matches(link, &snapshot) {
        return;
    }
    events::emit_in(resource.get(), owner.get(), "finish", &[]);
    provider::retire(resource.get());
    if !matches(link, &snapshot) {
        return;
    }
    let read_ended = unsafe {
        p::socket_ptr(link)
            .ok()
            .is_some_and(|payload| (*payload).ext.read_end_emitted)
    };
    if read_ended {
        socket::destroy(owner.get(), p::undefined());
    }
}

pub(crate) fn socket_closed(owner: f64, event: &tl::NetCompletion) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let record = scope.root_nanbox(prepare_socket_closed(owner.get(), event));
    emit_socket_closed(owner.get(), record.get());
}

fn prepare_socket_closed(owner: f64, event: &tl::NetCompletion) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    let own = p::socket_link(owner.get()).unwrap();
    let parent = scope.root_nanbox(unsafe { np::link_event_owner(link) }.unwrap_or(owner.get()));
    let state = scope.root_nanbox(socket::state(owner.get()));
    let mut record = scope.root_nanbox(
        unsafe { event.closed_handle_parts() }
            .map(|parts| closed::take(state.get(), parts))
            .unwrap_or_else(p::undefined),
    );
    if event.flags & tl::NET_FLAG_STALE == 0 && unsafe { p::socket_ptr(link) }.is_ok() {
        if !JsValue::from_bits(record.get().to_bits()).is_pointer() {
            record = scope.root_nanbox(closed::record(
                unsafe { event.closed_handle_parts() }.unwrap_or([0; 4]),
            ));
            let group = scope.root_nanbox(p::own_get(state.get(), "serverGroup"));
            p::own_set(record.get(), "serverGroup", group.get());
        }
        socket::cache_before_release(owner.get(), link);
        if own != link {
            socket::cache_before_release(parent.get(), link);
            let parent_state = scope.root_nanbox(socket::state(parent.get()));
            let parent_record = scope.root_nanbox(closed::record(
                unsafe { event.closed_handle_parts() }.unwrap_or([0; 4]),
            ));
            let group = scope.root_nanbox(p::own_get(parent_state.get(), "serverGroup"));
            p::own_set(parent_record.get(), "serverGroup", group.get());
            provider::capture_socket(parent_state.get(), parent_record.get());
            closed::append(parent_state.get(), parent_record.get());
        }
        let error = scope.root_nanbox(socket::error("ERR_STREAM_DESTROYED", "Socket is closed"));
        socket::cancel_callbacks(owner.get(), state.get(), error.get());
        provider::capture_socket(state.get(), record.get());
        super::native_transport::release_codec(owner.get());
        // The driver already retired this handle. No close is submitted here.
        unsafe {
            np::close_link(link, &p::SOCKET);
        }
    }
    if own != link && event.flags & tl::NET_FLAG_STALE == 0 && unsafe { p::socket_ptr(own) }.is_ok()
    {
        if !JsValue::from_bits(record.get().to_bits()).is_pointer() {
            record = scope.root_nanbox(closed::record(
                unsafe { event.closed_handle_parts() }.unwrap_or([0; 4]),
            ));
            socket::cache_before_release(owner.get(), link);
            let error =
                scope.root_nanbox(socket::error("ERR_STREAM_DESTROYED", "Socket is closed"));
            socket::cancel_callbacks(owner.get(), state.get(), error.get());
            provider::capture_socket(state.get(), record.get());
        }
        // The driver resource belongs to the parent. Dispose the wrapper's
        // idle payload as well, while its ordinary event object remains.
        unsafe {
            np::close_link(own, &p::SOCKET);
        }
    }
    record.get()
}

fn emit_socket_closed(owner: f64, record: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let record = scope.root_nanbox(record);
    let record_exists = JsValue::from_bits(record.get().to_bits()).is_pointer();
    let error = scope.root_nanbox(if record_exists {
        p::own_get(record.get(), "error")
    } else {
        p::undefined()
    });
    let group = scope.root_nanbox(if record_exists {
        p::own_get(record.get(), "serverGroup")
    } else {
        p::undefined()
    });
    let had_error = !JsValue::from_bits(error.get().to_bits()).is_undefined()
        && !JsValue::from_bits(error.get().to_bits()).is_null();
    let resource = scope.root_nanbox(if record_exists {
        p::own_get(record.get(), provider::TCP)
    } else {
        p::undefined()
    });
    server::child_closed(owner.get(), group.get());
    if had_error {
        events::emit_in(resource.get(), owner.get(), "error", &[error.get()]);
    }
    let had_handle =
        !record_exists || p::own_get(record.get(), "hadHandle").to_bits() != JsValue::FALSE.bits();
    let args = if had_handle {
        vec![f64::from_bits(JsValue::from_bool(had_error).bits())]
    } else {
        Vec::new()
    };
    events::emit_in(resource.get(), owner.get(), "close", &args);
    if record_exists {
        provider::retire_socket(record.get());
    }
}

unsafe extern "C" fn close_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let record = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    emit_socket_closed(owner.get(), record.get());
    p::undefined()
}

unsafe extern "C" fn closed_wrapper_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let record = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let state = scope.root_nanbox(socket::state(owner.get()));
    // Only the wrapper's idle payload is ours. Its parent may already have
    // reopened by this tick; never project or close that replacement core.
    let own = p::socket_link(owner.get()).unwrap();
    if np::link_lifecycle(own, &p::SOCKET) == Ok(np::Lifecycle::Closed) {
        return p::undefined();
    }
    p::own_set(state.get(), "tlsParent", p::undefined());
    p::own_set(
        state.get(),
        "destroyed",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    np::close_link(own, &p::SOCKET);
    emit_socket_closed(owner.get(), record.get());
    p::undefined()
}

pub(crate) fn queue_closed_wrapper(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let record = scope.root_nanbox(closed::record([0; 4]));
    queue_close_job(
        owner.get(),
        record.get(),
        perry_ffi::js_function_info!(closed_wrapper_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
}

pub(crate) fn queue_socket_closed(owner: f64, record: f64) {
    #[cfg(test)]
    if std::env::var("PERRY_NET_BINDING_SABOTAGE").as_deref() == Ok("late_close") {
        emit_socket_closed(owner, record);
        return;
    }
    queue_close_job(
        owner,
        record,
        perry_ffi::js_function_info!(close_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
}

fn queue_close_job(owner: f64, record: f64, info: &'static perry_ffi::JsFunctionInfo) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let record = scope.root_nanbox(record);
    let job = scope.root_addr(perry_ffi::alloc_closure(info, 2) as i64);
    unsafe {
        let ptr = job.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(ptr, 1, record.get());
        extern "C" {
            fn js_queue_next_tick(job: i64);
        }
        js_queue_next_tick(job.get());
    }
}
