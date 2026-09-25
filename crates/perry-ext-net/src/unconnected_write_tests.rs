use super::*;
use crate::{PendingNetEvent, SocketCommand, SocketState};

pub(super) fn take_events(handle: i64) -> Vec<PendingNetEvent> {
    let mut pending = statics::pending_events().lock().unwrap();
    let mut selected = Vec::new();
    let mut i = 0;
    while i < pending.len() {
        if matches!(&pending[i], PendingNetEvent::WriteComplete(id, ..)
            | PendingNetEvent::Error(id, _) | PendingNetEvent::Close(id) if *id == handle)
        {
            selected.push(pending.remove(i));
        } else {
            i += 1;
        }
    }
    selected
}

#[test]
fn unopened_command_refuses_bytes_instead_of_counting_and_dropping_them() {
    let mut socket = SocketState::for_test(true);
    assert_eq!(
        socket.command(-111_560, SocketCommand::Write(vec![1, 2], 0)),
        Err("Socket is closed".to_string())
    );
    assert_eq!(socket.bytes_queued, 0);
    assert_eq!(socket.bytes_written, 0);
}

#[test]
fn unopened_writes_queue_callbacks_before_one_error_and_close() {
    let handle = -111_561;
    take_events(handle);
    statics::sockets()
        .lock()
        .unwrap()
        .insert(handle, SocketState::for_test(true));
    let first = enqueue_socket_write(handle, vec![1], 501);
    let second = enqueue_socket_write(handle, vec![2], 502);
    let events = take_events(handle);
    let socket = statics::sockets().lock().unwrap().remove(&handle).unwrap();
    assert!(!first && !second);
    assert!(!socket.destroyed, "failure delivery must be asynchronous");
    assert_eq!(socket.bytes_queued, 0);
    assert!(!socket.need_drain);
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(
        matches!(&events[0], PendingNetEvent::WriteComplete(id, 501, Some(message)) if *id == handle && message == "Socket is closed")
    );
    assert!(
        matches!(&events[1], PendingNetEvent::WriteComplete(id, 502, Some(message)) if *id == handle && message == "Socket is closed")
    );
    assert!(
        matches!(&events[2], PendingNetEvent::Error(id, message) if *id == handle && message == "Socket is closed")
    );
    assert!(matches!(&events[3], PendingNetEvent::Close(id) if *id == handle));
}

#[test]
fn closed_socket_errors_keep_the_node_code_and_message() {
    unsafe {
        let roots = perry_ffi::TransientRootScope::enter();
        let error = roots.root_nanbox(crate::build_error_object("Socket is closed"));
        assert_eq!(
            crate::get_object_string_field(error.get(), "code").as_deref(),
            Some("ERR_SOCKET_CLOSED")
        );
        assert_eq!(
            crate::get_object_string_field(error.get(), "message").as_deref(),
            Some("Socket is closed")
        );
    }
}
