//! Main-thread pump for worker events plus the stdin IPC bridge.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim;
//! `deliver_parent_port_message` / `start_stdin_reader` are widened to
//! `pub(super)` and the two `#[no_mangle]` entry points are re-exported from
//! the parent module so `crate::worker_threads::js_worker_threads_*_pending`
//! keeps resolving.

use super::*;

/// Put the transport payload through the common MessagePort dispatch. The
/// port consumes once listeners before calling JS and folds that into ref state.
pub(super) fn deliver_parent_port_message(message: SerializedValue) {
    parent_port::worker_parent_port_object();
    MESSAGE_PORTS.with(|ports| {
        let mut ports = ports.borrow_mut();
        let state = ports.get_mut(&(PARENT_PORT_HANDLE as u64)).unwrap();
        if !state.closed {
            state.inbox.push_back(message);
        }
    });
    js_worker_threads_channels_process_pending();
}

/// Start the background stdin reader thread
pub(super) fn start_stdin_reader() {
    let already_started = STDIN_READER_STARTED.with(|s| {
        let was = *s.borrow();
        *s.borrow_mut() = true;
        was
    });
    if already_started {
        return;
    }

    // Threads never run on wasi: no reader, so no stdin messages.
    if cfg!(target_os = "wasi") {
        return;
    }
    // Spawn a thread to read lines from stdin
    // We use a regular thread (not tokio) because stdin reading is blocking
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if line.is_empty() {
                        continue;
                    }
                    // Queue the message for main thread processing
                    PENDING_MESSAGES.with(|q| {
                        q.borrow_mut().push(line);
                    });
                }
                Err(_) => break,
            }
        }
        // stdin EOF
        STDIN_EOF.with(|eof| {
            *eof.borrow_mut() = true;
        });
    });
}

/// Process pending messages - called from main thread event loop
/// Returns number of messages processed
#[no_mangle]
pub extern "C" fn js_worker_threads_process_pending() -> i32 {
    let mut processed = 0;

    // Only the events addressed to this agent (#11433); a Worker's own pump
    // must leave its parent's events where the parent will find them.
    let agent = perry_runtime::agent::current_agent();
    // O(1) when nothing is addressed to this agent: every agent's pump runs
    // this, and the queue is shared by all of them.
    let events: Vec<WorkerEvent> = if with_own_counts(|c| c.events.load(Ordering::Acquire)) == 0 {
        Vec::new()
    } else {
        let mut q = PARENT_EVENTS.lock().unwrap();
        let mut mine = Vec::new();
        q.retain_mut(|(parent, event)| {
            if *parent != agent {
                return true;
            }
            mine.push(std::mem::replace(event, WorkerEvent::Online(0)));
            false
        });
        with_own_counts(|c| c.events.fetch_sub(mine.len(), Ordering::AcqRel));
        mine
    };
    for event in events {
        match event {
            WorkerEvent::Online(worker_id) => {
                dispatch_worker_event(worker_id, "online", None);
                processed += 1;
            }
            WorkerEvent::Message(worker_id, message) => {
                let bits = unsafe { deserialize_nanbox_on_current_thread(&message) };
                dispatch_worker_event(worker_id, "message", Some(f64::from_bits(bits)));
                processed += 1;
            }
            WorkerEvent::Error(worker_id, error) => {
                let error = error.deserialize();
                // Like an EventEmitter 'error' with no listener: the worker's
                // error becomes an uncaught exception of the parent.
                if !dispatch_worker_event(worker_id, "error", Some(error)) {
                    perry_runtime::exception::js_throw(error);
                }
                processed += 1;
            }
            WorkerEvent::Exit(worker_id, code) => {
                let (terminate_promise, async_resources) =
                    if let Some(worker) = WORKERS.lock().unwrap().get_mut(&worker_id) {
                        let refed = worker.refed;
                        worker.set_liveness(false, refed);
                        (
                            worker.terminate_promise.take(),
                            Some(worker.async_resources),
                        )
                    } else {
                        (None, None)
                    };
                dispatch_worker_event(worker_id, "exit", Some(code as f64));
                if let Some(async_resources) = async_resources {
                    for resource in async_resources {
                        perry_runtime::async_hook_slots::destroy(resource.async_id);
                    }
                }
                if let Some(promise) = terminate_promise {
                    super::async_shim::queue_promise_resolution(
                        promise,
                        true,
                        (code as f64).to_bits(),
                    );
                }
                processed += 1;
            }
        }
        // Each queued worker event is one host callback. Finish its nextTicks
        // and promise jobs before the next event, as timer callbacks already do.
        processed += perry_runtime::promise::microtasks::js_promise_run_microtasks_checkpoint();
    }

    // Collect messages to process
    let messages: Vec<String> = PENDING_MESSAGES.with(|q| {
        let mut q = q.borrow_mut();
        q.drain(..).collect()
    });

    for msg in messages {
        let str_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let bits = unsafe { js_json_parse(str_ptr) };
        deliver_parent_port_message(unsafe {
            perry_runtime::thread::serialize_nanbox_for_thread(bits)
        });
        processed += 1;
    }

    if STDIN_EOF.with(|eof| *eof.borrow()) && !parent_port::is_closed() {
        message_port::close_port(PARENT_PORT_HANDLE as u64);
    }
    processed
}

/// `perry_runtime::agent::retire_agent` hook: events addressed to an agent that
/// has exited can never be delivered (a nested Worker's children outliving it).
fn purge_parent_events(agent: perry_runtime::agent::AgentId) {
    let dropped: VecDeque<_> = {
        let mut q = PARENT_EVENTS.lock().unwrap();
        let (dead, live): (VecDeque<_>, VecDeque<_>) = std::mem::take(&mut *q)
            .into_iter()
            .partition(|(parent, _)| *parent == agent);
        *q = live;
        // Under the queue lock, so no push can count an event for this agent
        // after its counts are gone.
        forget_agent_counts(agent);
        dead
    };
    drop(dropped);
}

#[cfg(test)]
pub(super) fn purge_parent_events_for_test(agent: perry_runtime::agent::AgentId) {
    purge_parent_events(agent);
}

pub(super) fn ensure_parent_event_retire_hook() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| perry_runtime::agent::register_retire_hook(purge_parent_events));
}

/// Check if worker_threads has pending work (stdin reader active)
#[no_mangle]
pub extern "C" fn js_worker_threads_has_pending() -> i32 {
    let started = STDIN_READER_STARTED.with(|s| *s.borrow());
    let eof = STDIN_EOF.with(|eof| *eof.borrow());
    let has_messages = PENDING_MESSAGES.with(|q| !q.borrow().is_empty());
    // Per-agent counters (#12448). The record's owner is authoritative on
    // every agent: counting the parent's record on the child keeps the child
    // itself alive forever; counting nested children on the primary defeats
    // Worker.unref().
    let (has_worker_events, has_live_refed_worker) = with_own_counts(|c| {
        (
            c.events.load(Ordering::Acquire) != 0,
            c.live_refed.load(Ordering::Acquire) != 0,
        )
    });

    if has_messages
        || has_worker_events
        || has_live_refed_worker
        || (started && !eof && message_port::port_is_refed(PARENT_PORT_HANDLE as u64))
    {
        1
    } else {
        0
    }
}

/// Run the listeners of one worker event. Returns whether anything listened
/// (a listener or an `on<event>` property).
fn dispatch_worker_event(worker_id: u64, event: &str, arg: Option<f64>) -> bool {
    // Collect (callback, web_event) pairs, then invoke OUTSIDE the WORKERS lock —
    // a listener may re-enter postMessage / terminate, which needs the lock again.
    let (object_bits, callbacks, async_resources): (
        u64,
        Vec<(u64, bool)>,
        [perry_runtime::async_hook_slots::AsyncResourceIds; 3],
    ) = {
        let mut workers = WORKERS.lock().unwrap();
        let Some(worker) = workers.get_mut(&worker_id) else {
            return false;
        };
        let callbacks = worker
            .listeners
            .get_mut(event)
            .map(|listeners| {
                let callbacks = listeners
                    .iter()
                    .map(|listener| (listener.callback_bits, listener.web_event))
                    .collect::<Vec<_>>();
                listeners.retain(|listener| !listener.once);
                callbacks
            })
            .unwrap_or_default();
        (worker.object_bits, callbacks, worker.async_resources)
    };

    // Web-style `addEventListener` listeners receive a `MessageEvent` wrapper
    // (with `.data`) for "message" events; Node-style `on` listeners receive the
    // raw payload. Lazily build the event object only if a web listener exists.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let object_h = scope.root_nanbox_f64(f64::from_bits(object_bits));
    // Root the callbacks BEFORE allocating the `MessageEvent` (or any value the
    // listeners are called with): the allocation can trigger a moving GC, which
    // rewrites the canonical `WorkerListener.callback_bits` via the worker root
    // scanner but would leave this snapshot's raw bits stale.
    let callbacks = callbacks
        .into_iter()
        .map(|(callback_bits, web_event)| {
            (
                scope.root_nanbox_f64(f64::from_bits(callback_bits)),
                web_event,
            )
        })
        .collect::<Vec<_>>();
    let arg_handle = arg.map(|a| scope.root_nanbox_f64(a));
    let resource = match event {
        "online" => async_resources[1],
        "message" | "messageerror" => async_resources[2],
        _ => async_resources[0],
    };
    let mut listened = !callbacks.is_empty();
    perry_runtime::async_hook_slots::run_resource_scope_catching(resource, || {
        let property_name = match event {
            "message" => Some("onmessage"),
            "error" => Some("onerror"),
            "messageerror" => Some("onmessageerror"),
            _ => None,
        };
        let property_handler = property_name
            .and_then(|name| object_event_handler(object_h.get_nanbox_f64().to_bits(), name))
            .map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
        listened |= property_handler.is_some();
        let needs_event = property_handler.is_some() || callbacks.iter().any(|(_, web)| *web);
        let event_handle = if needs_event {
            let data = (event == "message")
                .then(|| arg_handle.as_ref().map(|h| h.get_nanbox_f64()))
                .flatten();
            let ev = event_object(event, object_h.get_nanbox_f64().to_bits(), data);
            Some(scope.root_nanbox_f64(ev))
        } else {
            None
        };

        if let (Some(callback_h), Some(event_h)) = (property_handler, event_handle.as_ref()) {
            call_callback1(
                callback_h.get_nanbox_f64().to_bits(),
                object_h.get_nanbox_f64().to_bits(),
                event_h.get_nanbox_f64(),
            );
        }

        for (callback_h, web_event) in callbacks {
            let closure_ptr =
                perry_runtime::value::js_nanbox_get_pointer(callback_h.get_nanbox_f64());
            if closure_ptr == 0 {
                continue;
            }
            let closure = closure_ptr as *const ClosureHeader;
            let call_arg = if web_event {
                event_handle.as_ref().map(|h| h.get_nanbox_f64())
            } else {
                arg_handle.as_ref().map(|h| h.get_nanbox_f64())
            };
            if let Some(arg) = call_arg {
                perry_runtime::closure::js_closure_call1(
                    closure,
                    perry_runtime::closure::plain_call_receiver(),
                    arg,
                );
            } else {
                perry_runtime::closure::js_closure_call0(
                    closure,
                    perry_runtime::closure::plain_call_receiver(),
                );
            }
        }
        js_undefined()
    });
    listened
}
