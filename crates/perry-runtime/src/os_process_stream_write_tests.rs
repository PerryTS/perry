//! #10903: the bytes `process.stdout.write` / `process.stderr.write` produce,
//! and the fd writer that delivers them.

use super::*;
use crate::buffer::{
    buffer_data_mut, js_array_buffer_new, js_buffer_alloc, js_buffer_slice, js_data_view_new,
    js_uint8array_alloc, BufferHeader,
};

const UNDEFINED: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

fn pointer_value<T>(ptr: *const T) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn heap_string(text: &[u8]) -> f64 {
    let header = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    f64::from_bits(JSValue::string_ptr(header).bits())
}

fn fill(buf: *mut BufferHeader, bytes: &[u8]) {
    // GC_STORE_AUDIT(POINTER_FREE): raw test bytes into a byte payload.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer_data_mut(buf), bytes.len()) };
}

fn written(chunk: f64, encoding: f64) -> Vec<u8> {
    with_write_bytes(chunk, encoding, <[u8]>::to_vec)
}

/// The exact bytes that tripped the Native Messaging host: a little-endian
/// frame length of 1,048,567. Decoded as UTF-8 they are two U+FFFD and `0F 00`.
const FRAME_LENGTH: [u8; 4] = [0xf7, 0xff, 0x0f, 0x00];

#[test]
fn a_uint8array_and_a_buffer_are_written_byte_for_byte() {
    let u8a = js_uint8array_alloc(4);
    fill(u8a, &FRAME_LENGTH);
    assert_eq!(written(pointer_value(u8a), UNDEFINED), FRAME_LENGTH);

    let buf = js_buffer_alloc(4, 0);
    fill(buf, &FRAME_LENGTH);
    assert_eq!(written(pointer_value(buf), UNDEFINED), FRAME_LENGTH);
    // An encoding argument names how to encode a STRING; a binary chunk
    // ignores it.
    assert_eq!(
        written(pointer_value(buf), heap_string(b"hex")),
        FRAME_LENGTH
    );
}

#[test]
fn a_view_writes_only_its_own_window() {
    let source = js_uint8array_alloc(8);
    fill(source, &[0x01, 0x80, 0x81, 0x82, 0x05, 0x06, 0x07, 0x08]);
    let scope = crate::gc::RuntimeHandleScope::new();
    let _source = scope.root_raw_mut_ptr(source);
    let window = js_buffer_slice(source, 1, 4);
    assert_eq!(
        written(pointer_value(window), UNDEFINED),
        [0x80, 0x81, 0x82]
    );

    let backing = js_array_buffer_new(8);
    fill(backing, &[0x10, 0x90, 0x91, 0x13, 0x14, 0x15, 0x16, 0x17]);
    let _backing = scope.root_raw_mut_ptr(backing);
    let data_view = js_data_view_new(pointer_value(backing), 1.0, 2.0);
    assert_eq!(written(data_view, UNDEFINED), [0x90, 0x91]);
}

#[test]
fn a_wider_typed_array_is_written_as_its_raw_element_bytes() {
    let ta = crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_UINT16 as i32, 2);
    crate::typedarray::js_typed_array_set(ta, 0, 0xfffe as f64);
    crate::typedarray::js_typed_array_set(ta, 1, 0x0a80 as f64);
    // Node does not convert elements: it writes the view's bytes as stored.
    let expected: Vec<u8> = [0xfffe_u16, 0x0a80]
        .iter()
        .flat_map(|unit| unit.to_ne_bytes())
        .collect();
    assert_eq!(written(pointer_value(ta), UNDEFINED), expected);
}

#[test]
fn an_empty_view_is_an_empty_chunk_and_an_arraybuffer_is_not_a_chunk() {
    let empty = js_uint8array_alloc(0);
    assert_eq!(
        binary_chunk_span(pointer_value(empty)).map(|s| s.1),
        Some(0)
    );
    assert!(written(pointer_value(empty), UNDEFINED).is_empty());

    // Node rejects an ArrayBuffer chunk; perry keeps writing its display text
    // rather than its bytes, so it must not be classified as binary.
    let array_buffer = js_array_buffer_new(4);
    assert!(binary_chunk_span(pointer_value(array_buffer)).is_none());
    assert!(binary_chunk_span(UNDEFINED).is_none());
    assert!(binary_chunk_span(42.0).is_none());
}

#[test]
fn a_string_is_utf8_unless_an_encoding_is_named() {
    let text = "h\u{e9}llo";
    let heap = heap_string(text.as_bytes());
    assert_eq!(written(heap, UNDEFINED), text.as_bytes());
    assert_eq!(written(heap, heap_string(b"utf8")), text.as_bytes());
    assert_eq!(written(heap, heap_string(b"UTF-8")), text.as_bytes());
    // `write(chunk, callback)`: a non-string second argument is not an encoding.
    assert_eq!(written(heap, 1.0), text.as_bytes());

    let latin1 = [b'h', 0xe9, b'l', b'l', b'o'];
    assert_eq!(written(heap, heap_string(b"latin1")), latin1);
    assert_eq!(written(heap, heap_string(b"binary")), latin1);
    assert_eq!(written(heap, heap_string(b"ascii")), latin1);
    assert_eq!(
        written(heap_string(b"hi"), heap_string(b"ucs2")),
        [b'h', 0, b'i', 0]
    );
    assert_eq!(
        written(heap_string(b"f7ff0f00"), heap_string(b"hex")),
        FRAME_LENGTH
    );
    assert_eq!(
        written(heap_string(b"9/8PAA=="), heap_string(b"base64")),
        FRAME_LENGTH
    );
    assert_eq!(
        written(heap_string(b"9_8PAA"), heap_string(b"base64url")),
        FRAME_LENGTH
    );
}

#[test]
fn an_inline_short_string_takes_the_same_path() {
    let short = JSValue::try_short_string(b"f7ff").expect("fits inline");
    let short = f64::from_bits(short.bits());
    assert_eq!(written(short, UNDEFINED), b"f7ff");
    assert_eq!(written(short, heap_string(b"hex")), [0xf7, 0xff]);
}

#[test]
fn a_non_chunk_value_keeps_its_display_text() {
    assert_eq!(written(42.0, UNDEFINED), b"42");
    assert_eq!(written(UNDEFINED, UNDEFINED), b"undefined");
    // The encoding applies to string chunks only.
    assert_eq!(written(42.0, heap_string(b"hex")), b"42");
}

#[cfg(unix)]
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(31) ^ (i >> 8)) as u8)
        .collect()
}

/// A non-blocking pipe holds 16–64 KiB. A 3 MiB chunk therefore hits `EAGAIN`
/// dozens of times while the reader dawdles; `Stdout::write_all` would have
/// returned that error after an unknown prefix and the stub dropped it.
#[cfg(unix)]
#[test]
fn write_all_fd_completes_a_large_chunk_on_a_non_blocking_pipe() {
    use std::io::Read;
    use std::os::fd::FromRawFd;

    let mut fds = [0_i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let (read_fd, write_fd) = (fds[0], fds[1]);
    unsafe {
        let flags = libc::fcntl(write_fd, libc::F_GETFL);
        assert_eq!(
            libc::fcntl(write_fd, libc::F_SETFL, flags | libc::O_NONBLOCK),
            0
        );
    }
    // Precondition the test depends on: a bare write really does fill the pipe
    // and report EAGAIN, i.e. the loop below has something to survive.
    let probe = vec![0_u8; 1 << 20];
    let first = unsafe { libc::write(write_fd, probe.as_ptr().cast(), probe.len()) };
    assert!(first > 0 && (first as usize) < probe.len());
    let again = unsafe { libc::write(write_fd, probe.as_ptr().cast(), probe.len()) };
    assert_eq!(again, -1);
    assert_eq!(
        std::io::Error::last_os_error().kind(),
        std::io::ErrorKind::WouldBlock
    );

    let prefix = first as usize;
    let reader = std::thread::spawn(move || {
        let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let mut received = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            std::thread::sleep(std::time::Duration::from_micros(200));
            match file.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => received.extend_from_slice(&chunk[..n]),
                Err(err) => panic!("reader: {err}"),
            }
        }
        received
    });

    let payload = pattern(3 << 20);
    write_all_fd(write_fd, &payload).expect("write_all_fd");
    unsafe { libc::close(write_fd) };
    let received = reader.join().expect("reader thread");
    assert_eq!(received.len(), prefix + payload.len());
    assert!(received[..prefix].iter().all(|b| *b == 0));
    assert!(
        received[prefix..] == payload[..],
        "payload reordered or lost"
    );
}

#[cfg(unix)]
#[test]
fn write_all_fd_reports_a_closed_reader_instead_of_spinning() {
    let mut fds = [0_i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    unsafe { libc::close(fds[0]) };
    // SIGPIPE is ignored by the runtime at startup; make the test independent
    // of who ran first.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    let err = write_all_fd(fds[1], b"x").expect_err("EPIPE");
    assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    unsafe { libc::close(fds[1]) };
}
