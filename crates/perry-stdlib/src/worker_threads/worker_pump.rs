//! Main-thread pump for worker events plus the stdin IPC bridge.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim;
//! `deliver_parent_port_message` / `start_stdin_reader` are widened to
//! `pub(super)` and the two `#[no_mangle]` entry points are re-exported from
//! the parent module so `crate::worker_threads::js_worker_threads_*_pending`
//! keeps resolving.

use super::*;

/// Deliver one main→worker message to the in-worker `parentPort` listeners.
/// Fires the Node-style `MESSAGE_CALLBACK` with the raw payload AND any
/// Web-style `addEventListener("message", fn)` listeners with a `MessageEvent`.
/// Runs on the worker's own thread (its arena), so the value is deserialized
/// here and any event wrapper is allocated in this thread's arena.
pub(super) fn deliver_parent_port_message(message: &SerializedValue) {
    let bits = unsafe { deserialize_nanbox_on_current_thread(message) };
    let value = f64::from_bits(bits);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);

    if let Some(callback_ptr) = MESSAGE_CALLBACK.with(|cb| *cb.borrow()) {
        let closure = callback_ptr as *const ClosureHeader;
        perry_runtime::closure::js_closure_call1(closure, value_h.get_nanbox_f64());
    }

    // Root the listener closures BEFORE allocating the `MessageEvent`: that
    // allocation can trigger a moving GC, which rewrites the canonical
    // `MESSAGE_EVENT_CALLBACKS` storage via the registered root scanner but
    // would leave a plain `clone()` of the raw pointers stale.
    let event_cbs = MESSAGE_EVENT_CALLBACKS.with(|cbs| {
        cbs.borrow()
            .iter()
            .map(|&callback_ptr| {
                scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(callback_ptr))
            })
            .collect::<Vec<_>>()
    });
    if !event_cbs.is_empty() {
        let event = event_object("message", 0, Some(value_h.get_nanbox_f64()));
        let event_h = scope.root_nanbox_f64(event);
        for callback_h in event_cbs {
            let callback_ptr =
                perry_runtime::value::js_nanbox_get_pointer(callback_h.get_nanbox_f64());
            let closure = callback_ptr as *const ClosureHeader;
            perry_runtime::closure::js_closure_call1(closure, event_h.get_nanbox_f64());
        }
    }
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

    let events: Vec<WorkerEvent> = {
        let mut q = PARENT_EVENTS.lock().unwrap();
        q.drain(..).collect()
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
            WorkerEvent::Error(worker_id) => {
                dispatch_worker_event(worker_id, "error", None);
                processed += 1;
            }
            WorkerEvent::Exit(worker_id, code) => {
                let terminate_promise =
                    if let Some(worker) = WORKERS.lock().unwrap().get_mut(&worker_id) {
                        worker.alive = false;
                        worker.terminate_promise.take()
                    } else {
                        None
                    };
                dispatch_worker_event(worker_id, "exit", Some(code as f64));
                if let Some(promise) = terminate_promise {
                    crate::common::async_bridge::queue_promise_resolution(
                        promise,
                        true,
                        (code as f64).to_bits(),
                    );
                }
                processed += 1;
            }
        }
    }

    // Collect messages to process
    let messages: Vec<String> = PENDING_MESSAGES.with(|q| {
        let mut q = q.borrow_mut();
        q.drain(..).collect()
    });

    let callback = MESSAGE_CALLBACK.with(|cb| *cb.borrow());

    if let Some(callback_ptr) = callback {
        for msg in messages {
            // JSON-parse the message string
            let str_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let bits = unsafe { js_json_parse(str_ptr) };
            let parsed = f64::from_bits(bits);

            // Call the message callback with the parsed value
            let closure = callback_ptr as *const ClosureHeader;
            perry_runtime::closure::js_closure_call1(closure, parsed);
            processed += 1;
        }
    }

    // Check for EOF and fire close callback
    let is_eof = STDIN_EOF.with(|eof| *eof.borrow());
    if is_eof {
        let close_cb = CLOSE_CALLBACK.with(|cb| cb.borrow_mut().take());
        if let Some(callback_ptr) = close_cb {
            let closure = callback_ptr as *const ClosureHeader;
            perry_runtime::closure::js_closure_call0(closure);
        }
    }

    processed
}

/// Check if worker_threads has pending work (stdin reader active)
#[no_mangle]
pub extern "C" fn js_worker_threads_has_pending() -> i32 {
    let started = STDIN_READER_STARTED.with(|s| *s.borrow());
    let eof = STDIN_EOF.with(|eof| *eof.borrow());
    let has_messages = PENDING_MESSAGES.with(|q| !q.borrow().is_empty());
    let has_worker_events = !PARENT_EVENTS.lock().unwrap().is_empty();
    let has_live_refed_worker = WORKERS
        .lock()
        .unwrap()
        .values()
        .any(|worker| worker.alive && worker.refed);

    if has_messages || has_worker_events || has_live_refed_worker || (started && !eof) {
        1
    } else {
        0
    }
}

fn dispatch_worker_event(worker_id: u64, event: &str, arg: Option<f64>) {
    // Collect (callback, web_event) pairs, then invoke OUTSIDE the WORKERS lock —
    // a listener may re-enter postMessage / terminate, which needs the lock again.
    let callbacks: Vec<(u64, bool)> = {
        let mut workers = WORKERS.lock().unwrap();
        let Some(worker) = workers.get_mut(&worker_id) else {
            return;
        };
        let Some(listeners) = worker.listeners.get_mut(event) else {
            return;
        };
        let callbacks = listeners
            .iter()
            .map(|listener| (listener.callback_bits, listener.web_event))
            .collect::<Vec<_>>();
        listeners.retain(|listener| !listener.once);
        callbacks
    };

    // Web-style `addEventListener` listeners receive a `MessageEvent` wrapper
    // (with `.data`) for "message" events; Node-style `on` listeners receive the
    // raw payload. Lazily build the event object only if a web listener exists.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
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
    let needs_event = event == "message" && callbacks.iter().any(|(_, web)| *web);
    let event_handle = if needs_event {
        let data = arg_handle.as_ref().map(|h| h.get_nanbox_f64());
        let ev = event_object("message", 0, data);
        Some(scope.root_nanbox_f64(ev))
    } else {
        None
    };

    for (callback_h, web_event) in callbacks {
        let closure_ptr = perry_runtime::value::js_nanbox_get_pointer(callback_h.get_nanbox_f64());
        if closure_ptr == 0 {
            continue;
        }
        let closure = closure_ptr as *const ClosureHeader;
        let call_arg = if web_event && event == "message" {
            event_handle.as_ref().map(|h| h.get_nanbox_f64())
        } else {
            arg_handle.as_ref().map(|h| h.get_nanbox_f64())
        };
        if let Some(arg) = call_arg {
            perry_runtime::closure::js_closure_call1(closure, arg);
        } else {
            perry_runtime::closure::js_closure_call0(closure);
        }
    }
}
