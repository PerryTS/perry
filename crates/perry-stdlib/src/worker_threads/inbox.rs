//! A worker's inbox: the commands its parent sends. Messages are also
//! delivered by a per-agent pump, so they reach a worker that is blocked in
//! a nested wait of its own event loop (a stream drained synchronously, …)
//! and is waiting for exactly the message that would let it continue. The
//! worker loop (`js_worker_threads_worker_new`) receives everything else.

use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use super::*;

thread_local! {
    /// The command channel of the worker running on this thread.
    static INBOX: RefCell<Option<Receiver<WorkerCommand>>> = const { RefCell::new(None) };
    /// Commands the pump took that only the worker loop may run (terminate,
    /// reload), in order. Nothing after one is delivered before it.
    static HELD: RefCell<VecDeque<WorkerCommand>> = const { RefCell::new(VecDeque::new()) };
}

extern "C" {
    fn js_register_agent_aux_pump(f: extern "C" fn() -> i32);
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
}

/// Make `rx` this thread's inbox. Called once by a worker thread at start.
pub(super) fn install(rx: Receiver<WorkerCommand>) {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_agent_aux_pump(deliver_waiting_messages);
        js_register_aux_has_active(expects_messages);
    });
    INBOX.with(|inbox| *inbox.borrow_mut() = Some(rx));
}

/// Whether the worker registered a Node-style, EventTarget-style or
/// property-style message consumer.
pub(super) fn has_message_consumer() -> bool {
    MESSAGE_CALLBACK.with(|cb| cb.borrow().is_some())
        || MESSAGE_EVENT_CALLBACKS.with(|cbs| !cbs.borrow().is_empty())
        || worker_surface::web_worker_global_handler("onmessage").is_some()
}

fn held() -> Option<WorkerCommand> {
    HELD.with(|held| held.borrow_mut().pop_front())
}

pub(super) fn try_recv() -> Result<WorkerCommand, TryRecvError> {
    if let Some(command) = held() {
        return Ok(command);
    }
    INBOX.with(|inbox| match inbox.borrow().as_ref() {
        Some(rx) => rx.try_recv(),
        None => Err(TryRecvError::Disconnected),
    })
}

pub(super) fn recv_timeout(budget: Duration) -> Result<WorkerCommand, RecvTimeoutError> {
    if let Some(command) = held() {
        return Ok(command);
    }
    INBOX.with(|inbox| match inbox.borrow().as_ref() {
        Some(rx) => rx.recv_timeout(budget),
        None => Err(RecvTimeoutError::Disconnected),
    })
}

pub(super) fn recv() -> Result<WorkerCommand, RecvError> {
    if let Some(command) = held() {
        return Ok(command);
    }
    INBOX.with(|inbox| match inbox.borrow().as_ref() {
        Some(rx) => rx.recv(),
        None => Err(RecvError),
    })
}

/// Per-agent pump: deliver the messages already waiting in this worker's
/// inbox. A no-op on any thread that is not a worker.
extern "C" fn deliver_waiting_messages() -> i32 {
    let mut delivered = 0;
    while HELD.with(|held| held.borrow().is_empty()) {
        // The borrow ends before any JS runs: a handler may nest a wait that
        // runs this pump again.
        let command = INBOX.with(|inbox| inbox.borrow().as_ref().map(Receiver::try_recv));
        match command {
            Some(Ok(WorkerCommand::Message(message))) => {
                worker_pump::deliver_parent_port_message(&message);
            }
            Some(Ok(WorkerCommand::DirectMessage {
                message,
                source_thread_id,
                ack,
            })) => {
                let result = direct_message::deliver_worker_message(&message, source_thread_id);
                let _ = ack.send(result);
            }
            Some(Ok(other)) => {
                HELD.with(|held| held.borrow_mut().push_back(other));
                break;
            }
            _ => break,
        }
        delivered += 1;
    }
    delivered
}

/// Keeps a worker's nested waits alive while its parent may still send it
/// a message it listens for: a send wakes this agent's loop.
extern "C" fn expects_messages() -> i32 {
    let worker = INBOX.with(|inbox| inbox.borrow().is_some());
    i32::from(worker && (has_message_consumer() || HELD.with(|held| !held.borrow().is_empty())))
}
