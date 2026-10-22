use crate::io::sync::wasi::io::{
    poll::HostPollable,
    streams::{HostInputStream, HostOutputStream},
};
use crate::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi_io::streams::{DynInputStream, DynOutputStream, StreamError};

#[test]
fn streams_share_cursor_and_subscriptions_preserve_parent_lifetime() {
    let mut table = ResourceTable::new();
    let input = MemoryInputPipe::new("abcdefgh");
    let first = table
        .push(Box::new(input.clone()) as DynInputStream)
        .unwrap();
    let second = table.push(Box::new(input) as DynInputStream).unwrap();
    let pollable =
        HostInputStream::subscribe(&mut table, Resource::new_borrow(first.rep())).unwrap();
    assert!(HostPollable::ready(&mut table, Resource::new_borrow(pollable.rep())).unwrap());
    assert!(HostInputStream::drop(&mut table, Resource::new_own(first.rep())).is_err());
    assert_eq!(
        HostInputStream::read(&mut table, Resource::new_borrow(first.rep()), 2).unwrap(),
        b"ab"
    );
    assert_eq!(
        HostInputStream::skip(&mut table, Resource::new_borrow(second.rep()), 2).unwrap(),
        2
    );
    assert_eq!(
        HostInputStream::blocking_read(&mut table, Resource::new_borrow(first.rep()), 8).unwrap(),
        b"efgh"
    );
    assert!(matches!(
        HostInputStream::read(&mut table, Resource::new_borrow(second.rep()), 1),
        Err(StreamError::Closed)
    ));
    HostPollable::drop(&mut table, pollable).unwrap();
    HostInputStream::drop(&mut table, first).unwrap();
    HostInputStream::drop(&mut table, second).unwrap();
}

#[test]
fn splice_and_zero_writes_obey_output_capacity() {
    let mut table = ResourceTable::new();
    let input = table
        .push(Box::new(MemoryInputPipe::new("abcd")) as DynInputStream)
        .unwrap();
    let output = MemoryOutputPipe::new(8);
    let writer = table
        .push(Box::new(output.clone()) as DynOutputStream)
        .unwrap();
    assert_eq!(
        HostOutputStream::check_write(&mut table, Resource::new_borrow(writer.rep())).unwrap(),
        8
    );
    assert_eq!(
        HostOutputStream::blocking_splice(
            &mut table,
            Resource::new_borrow(writer.rep()),
            Resource::new_borrow(input.rep()),
            3
        )
        .unwrap(),
        3
    );
    HostOutputStream::blocking_write_zeroes_and_flush(
        &mut table,
        Resource::new_borrow(writer.rep()),
        2,
    )
    .unwrap();
    assert_eq!(&output.contents()[..], b"abc\0\0");
    assert!(
        HostOutputStream::write(&mut table, Resource::new_borrow(writer.rep()), vec![0; 4])
            .is_err()
    );
    assert_eq!(&output.contents()[..], b"abc\0\0");
    HostOutputStream::blocking_write_and_flush(
        &mut table,
        Resource::new_borrow(writer.rep()),
        b"xyz".to_vec(),
    )
    .unwrap();
    assert!(matches!(
        HostOutputStream::check_write(&mut table, Resource::new_borrow(writer.rep())),
        Err(StreamError::Closed)
    ));
    HostOutputStream::drop(&mut table, writer).unwrap();
    HostInputStream::drop(&mut table, input).unwrap();
}

#[test]
fn typed_stream_errors_are_distinct_from_traps() {
    use crate::io::bindings::wasi::io::{
        error::HostError,
        streams::{Host, StreamError as GuestError},
    };
    let mut table = ResourceTable::new();
    let typed = Host::convert_stream_error(
        &mut table,
        StreamError::LastOperationFailed(wasmtime::format_err!("native read failed")),
    )
    .unwrap();
    let GuestError::LastOperationFailed(error) = typed else {
        panic!("lost typed I/O error")
    };
    assert!(
        HostError::to_debug_string(&mut table, Resource::new_borrow(error.rep()))
            .unwrap()
            .contains("native read failed")
    );
    HostError::drop(&mut table, error).unwrap();
    assert!(matches!(
        Host::convert_stream_error(&mut table, StreamError::Closed).unwrap(),
        GuestError::Closed
    ));
    assert!(Host::convert_stream_error(&mut table, StreamError::trap("host invariant")).is_err());
}
