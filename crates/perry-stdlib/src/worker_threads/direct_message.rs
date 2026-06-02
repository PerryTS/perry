use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use perry_runtime::string::js_string_from_bytes;
use perry_runtime::thread::{
    deserialize_nanbox_on_current_thread, serialize_nanbox_for_thread, SerializedValue,
};
use perry_runtime::value::JSValue;

use super::{is_undefined, js_bool, WorkerCommand, CURRENT_WORKER_ID, WORKERS};

pub(super) enum DirectMessageResult {
    Delivered,
    Failed,
}

#[derive(Clone, Copy)]
enum WorkerMessagingError {
    Failed,
    SameThread,
    Timeout,
}

impl WorkerMessagingError {
    fn code(self) -> &'static str {
        match self {
            WorkerMessagingError::Failed => "ERR_WORKER_MESSAGING_FAILED",
            WorkerMessagingError::SameThread => "ERR_WORKER_MESSAGING_SAME_THREAD",
            WorkerMessagingError::Timeout => "ERR_WORKER_MESSAGING_TIMEOUT",
        }
    }

    fn message(self) -> &'static str {
        match self {
            WorkerMessagingError::Failed => "Cannot find the destination thread or listener",
            WorkerMessagingError::SameThread => "Cannot sent a message to the same thread",
            WorkerMessagingError::Timeout => "Sending a message to another thread timed out",
        }
    }
}

/// worker_threads.postMessageToThread(threadId, value[, transferList][, timeout])
#[no_mangle]
pub extern "C" fn js_worker_threads_post_message_to_thread(
    thread_id: f64,
    value: f64,
    _transfer_list: f64,
    timeout: f64,
) -> f64 {
    let Some(target_thread_id) = thread_id_from_value(thread_id) else {
        return rejected_worker_messaging_promise(WorkerMessagingError::Failed);
    };
    let source_thread_id = CURRENT_WORKER_ID.with(|id| id.get());
    if target_thread_id == source_thread_id {
        return rejected_worker_messaging_promise(WorkerMessagingError::SameThread);
    }

    let message = unsafe { serialize_nanbox_for_thread(value.to_bits()) };
    let (ack_tx, ack_rx) = mpsc::channel::<DirectMessageResult>();
    let sender = WORKERS
        .lock()
        .unwrap()
        .get(&target_thread_id)
        .and_then(|worker| worker.alive.then(|| worker.sender.clone()));
    let Some(sender) = sender else {
        return rejected_worker_messaging_promise(WorkerMessagingError::Failed);
    };

    let promise = unsafe { crate::common::async_bridge::js_promise_new_for_native_resolution() };
    let promise_ptr = promise as usize;
    let timeout = timeout_duration(timeout);
    if sender
        .send(WorkerCommand::DirectMessage {
            message,
            source_thread_id,
            ack: ack_tx,
        })
        .is_err()
    {
        queue_worker_messaging_rejection(promise_ptr, WorkerMessagingError::Failed);
    } else {
        std::thread::spawn(move || wait_for_direct_message_ack(promise_ptr, ack_rx, timeout));
    }

    perry_runtime::value::js_nanbox_pointer(promise as i64)
}

#[used]
static KEEP_WT_POST_MESSAGE_TO_THREAD: extern "C" fn(f64, f64, f64, f64) -> f64 =
    js_worker_threads_post_message_to_thread;

pub(super) fn deliver_worker_message(
    message: &SerializedValue,
    source_thread_id: u64,
) -> DirectMessageResult {
    let bits = unsafe { deserialize_nanbox_on_current_thread(message) };
    let event = b"workerMessage";
    let event_ptr = js_string_from_bytes(event.as_ptr(), event.len() as u32);
    let event_bits = JSValue::string_ptr(event_ptr).bits() as i64;
    let mut args = perry_runtime::js_array_alloc(0);
    args = perry_runtime::js_array_push(args, JSValue::from_bits(bits));
    args = perry_runtime::js_array_push(args, JSValue::number(source_thread_id as f64));
    let delivered = perry_runtime::os::js_process_emit(event_bits, args);
    if delivered.to_bits() == js_bool(true).to_bits() {
        DirectMessageResult::Delivered
    } else {
        DirectMessageResult::Failed
    }
}

fn js_undefined_bits() -> u64 {
    JSValue::undefined().bits()
}

fn worker_messaging_error_value(error: WorkerMessagingError) -> f64 {
    let message = error.message();
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg_ptr, error.code());
    let err = perry_runtime::error::js_error_new_with_message(msg_ptr);
    perry_runtime::value::js_nanbox_pointer(err as i64)
}

fn rejected_worker_messaging_promise(error: WorkerMessagingError) -> f64 {
    let err = worker_messaging_error_value(error);
    let promise = perry_runtime::js_promise_rejected(err);
    perry_runtime::value::js_nanbox_pointer(promise as i64)
}

fn thread_id_from_value(value: f64) -> Option<u64> {
    if value.is_finite() && value >= 0.0 {
        Some(value as u64)
    } else {
        None
    }
}

fn timeout_duration(timeout: f64) -> Option<Duration> {
    if is_undefined(timeout) || !timeout.is_finite() || timeout < 0.0 {
        return None;
    }
    Some(Duration::from_millis(timeout as u64))
}

fn wait_for_direct_message_ack(
    promise_ptr: usize,
    ack_rx: Receiver<DirectMessageResult>,
    timeout: Option<Duration>,
) {
    let result = match timeout {
        Some(timeout) => match ack_rx.recv_timeout(timeout) {
            Ok(result) => Ok(result),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(WorkerMessagingError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(WorkerMessagingError::Failed),
        },
        None => ack_rx.recv().map_err(|_| WorkerMessagingError::Failed),
    };

    match result {
        Ok(DirectMessageResult::Delivered) => {
            crate::common::async_bridge::queue_promise_resolution(
                promise_ptr,
                true,
                js_undefined_bits(),
            );
        }
        Ok(DirectMessageResult::Failed) => {
            queue_worker_messaging_rejection(promise_ptr, WorkerMessagingError::Failed);
        }
        Err(error) => queue_worker_messaging_rejection(promise_ptr, error),
    }
}

fn queue_worker_messaging_rejection(promise_ptr: usize, error: WorkerMessagingError) {
    crate::common::async_bridge::queue_deferred_resolution(promise_ptr, false, move || {
        worker_messaging_error_value(error).to_bits()
    });
}
