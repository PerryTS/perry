use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use perry_runtime::string::js_string_from_bytes;
use perry_runtime::thread::{deserialize_nanbox_on_current_thread, SerializedValue};
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
    transfer_list: f64,
    timeout: f64,
) -> f64 {
    let Some(target_thread_id) = thread_id_from_value(thread_id) else {
        return rejected_worker_messaging_promise(WorkerMessagingError::Failed);
    };
    let source_thread_id = CURRENT_WORKER_ID.with(|id| id.get());
    if target_thread_id == source_thread_id {
        return rejected_worker_messaging_promise(WorkerMessagingError::SameThread);
    }

    let (sender, message) =
        match prepare_direct_message(source_thread_id, target_thread_id, value, transfer_list) {
            Ok(prepared) => prepared,
            Err(error) => return rejected_worker_messaging_promise(error),
        };
    let (ack_tx, ack_rx) = mpsc::channel::<DirectMessageResult>();

    let promise = unsafe { super::async_shim::js_promise_new_for_native_resolution() };
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

fn prepare_direct_message(
    source_thread_id: u64,
    target_thread_id: u64,
    value: f64,
    transfer: f64,
) -> Result<(super::WorkerSender, SerializedValue), WorkerMessagingError> {
    let lookup = || {
        WORKERS
            .lock()
            .unwrap()
            .get(&target_thread_id)
            .and_then(|worker| worker.alive.then(|| worker.sender.clone()))
    };
    let early_sender = if source_thread_id == 0 {
        Some(lookup().ok_or(WorkerMessagingError::Failed)?)
    } else {
        None
    };
    // A JS throw abandons Rust frames, so keep the sender outside the trap.
    let outcome =
        perry_runtime::exception::catch_js_throw(|| super::clone_message(value, transfer));
    let message = match outcome {
        Ok(message) => message,
        Err(error) => {
            drop(early_sender);
            perry_runtime::exception::js_throw(error);
        }
    };
    // Worker-origin forwarding clones before destination admission in Node.
    let sender = match early_sender {
        Some(sender) => sender,
        None => lookup().ok_or(WorkerMessagingError::Failed)?,
    };
    Ok((sender, message))
}

#[used(compiler)]
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
            super::async_shim::queue_promise_resolution(promise_ptr, true, js_undefined_bits());
        }
        Ok(DirectMessageResult::Failed) => {
            queue_worker_messaging_rejection(promise_ptr, WorkerMessagingError::Failed);
        }
        Err(error) => queue_worker_messaging_rejection(promise_ptr, error),
    }
}

fn queue_worker_messaging_rejection(promise_ptr: usize, error: WorkerMessagingError) {
    super::async_shim::queue_deferred_resolution(promise_ptr, false, move || {
        worker_messaging_error_value(error).to_bits()
    });
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use perry_runtime::gc::RuntimeHandleScope;

    struct TestWorker(u64);
    impl TestWorker {
        fn for_direct_message_admission() -> Self {
            let undefined = JSValue::undefined().bits();
            Self(super::super::thread_exit_probe::insert_worker_for_test(
                undefined, undefined,
            ))
        }
    }
    impl Drop for TestWorker {
        fn drop(&mut self) {
            if let Some(mut record) = WORKERS.lock().unwrap().remove(&self.0) {
                record.set_liveness(false, false);
            }
        }
    }

    extern "C" fn reject_direct_message_clone(
        _closure: *const perry_runtime::closure::ClosureHeader,
        _this: perry_runtime::closure::JsThis,
    ) -> f64 {
        perry_runtime::exception::js_throw(123.0)
    }

    fn throwing_clone_input(scope: &RuntimeHandleScope, property: &[u8]) -> f64 {
        let object = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(
            perry_runtime::object::js_object_alloc(0, 0) as i64,
        ));
        let getter = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(reject_direct_message_clone, 0; with_declared(0)),
            0,
        );
        let getter = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(getter as i64));
        let key = js_string_from_bytes(property.as_ptr(), property.len() as u32);
        perry_runtime::object::js_object_define_getter(
            object.get_nanbox_f64(),
            perry_runtime::value::js_nanbox_string(key as i64),
            getter.get_nanbox_f64(),
        );
        object.get_nanbox_f64()
    }

    fn transfer_input(scope: &RuntimeHandleScope) -> (f64, f64) {
        let buffer = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(
            perry_runtime::buffer::js_array_buffer_new(4) as i64,
        ));
        let list = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(
            perry_runtime::array::js_array_alloc(1) as i64,
        ));
        perry_runtime::array::js_array_push_f64(
            perry_runtime::value::js_nanbox_get_pointer(list.get_nanbox_f64()) as *mut _,
            buffer.get_nanbox_f64(),
        );
        (buffer.get_nanbox_f64(), list.get_nanbox_f64())
    }

    #[test]
    fn direct_admission_main_skips_clone_for_missing_and_dead_destinations() {
        let scope = RuntimeHandleScope::new();
        let dead = TestWorker::for_direct_message_admission();
        WORKERS
            .lock()
            .unwrap()
            .get_mut(&dead.0)
            .unwrap()
            .set_liveness(false, false);
        for target in [2147483647, dead.0] {
            let (buffer, list) = transfer_input(&scope);
            assert!(matches!(
                prepare_direct_message(0, target, buffer, list),
                Err(WorkerMessagingError::Failed)
            ));
            assert_eq!(
                perry_runtime::buffer::js_buffer_length(
                    perry_runtime::value::js_nanbox_get_pointer(buffer) as *const _
                ),
                4
            );
            for property in [b"payload".as_slice(), b"transfer".as_slice()] {
                let input = throwing_clone_input(&scope, property);
                let undefined = f64::from_bits(JSValue::undefined().bits());
                let (value, transfer) = if property == b"payload" {
                    (input, undefined)
                } else {
                    (undefined, input)
                };
                let outcome = perry_runtime::exception::catch_js_throw(|| {
                    prepare_direct_message(0, target, value, transfer)
                });
                assert!(matches!(outcome, Ok(Err(WorkerMessagingError::Failed))));
            }
        }
    }

    #[test]
    fn direct_admission_worker_clones_before_missing_destination_rejection() {
        let scope = RuntimeHandleScope::new();
        let (buffer, list) = transfer_input(&scope);
        assert!(matches!(
            prepare_direct_message(1, 2147483647, buffer, list),
            Err(WorkerMessagingError::Failed)
        ));
        assert_eq!(
            perry_runtime::buffer::js_buffer_length(perry_runtime::value::js_nanbox_get_pointer(
                buffer
            ) as *const _),
            0
        );
    }

    #[test]
    fn direct_admission_releases_early_sender_when_clone_throws() {
        let scope = RuntimeHandleScope::new();
        let live = TestWorker::for_direct_message_admission();
        let agent = WORKERS
            .lock()
            .unwrap()
            .get(&live.0)
            .unwrap()
            .sender
            .agent
            .clone();
        let before = std::sync::Arc::strong_count(&agent);
        let value = throwing_clone_input(&scope, b"payload");
        let outcome = perry_runtime::exception::catch_js_throw(|| {
            prepare_direct_message(
                0,
                live.0,
                value,
                f64::from_bits(JSValue::undefined().bits()),
            )
        });
        assert_eq!(outcome.err(), Some(123.0));
        assert_eq!(std::sync::Arc::strong_count(&agent), before);
    }
}
