//! What `process.stdout.write(chunk[, encoding])` / `process.stderr.write(...)`
//! put on the fd, and how they put it there.
//!
//! #10903. Both stubs used to start from `js_jsvalue_to_string(chunk)`, i.e.
//! the chunk's *display text*, and ignored `encoding`. Three things followed:
//!
//! * a `Buffer` / `Uint8Array` chunk was UTF-8 **decoded** (`Buffer#toString`)
//!   and the text written, so every byte that is not valid UTF-8 reached the
//!   fd as `EF BF BD` — a 4-byte frame length of 200 (`C8 00 00 00`) was
//!   already enough to corrupt a binary protocol;
//! * any other `TypedArray` was written as its `join(",")` text
//!   (`new Uint16Array([0x6968])` printed `26984`) and a `DataView` as
//!   `[object DataView]`;
//! * `write("6869", "hex")` wrote the four characters, not the two bytes.
//!
//! Node's contract (`writable.write(chunk[, encoding][, callback])`): a binary
//! chunk — `Buffer`, any `TypedArray`, `DataView` — is written byte for byte,
//! exactly the window the view covers; a string chunk is encoded with
//! `encoding` (default `utf8`).
//!
//! The write itself also changed. `std::io::Stdout::write_all` gives up on the
//! first `EAGAIN`, after an unknown prefix has already gone out, and the stubs
//! discarded that error — so on a non-blocking fd 1 (a pipe or tty whose open
//! file description someone put in `O_NONBLOCK`; the flag is shared by every
//! process holding the description) a large chunk was silently truncated.
//! [`write_all_fd`] owns the partial-write loop instead and waits for
//! `POLLOUT`.

use crate::value::JSValue;

/// The window a binary write chunk covers, or `None` when `chunk` is not one.
///
/// `ArrayBuffer` / `SharedArrayBuffer` share the `BufferHeader` shape but are
/// NOT chunks — Node rejects them — so they stay on the display-text path.
///
/// The pointer borrows the live allocation: it is valid until the next
/// collection. The callers below neither allocate on the JS heap nor reach a
/// safepoint between taking it and finishing the write.
fn binary_chunk_span(chunk: f64) -> Option<(*const u8, usize)> {
    if !JSValue::from_bits(chunk.to_bits()).is_pointer() {
        return None;
    }
    // Everything below keys registries by address; nothing dereferences
    // `addr` unless a registry has vouched for it.
    let addr = (chunk.to_bits() & crate::value::POINTER_MASK) as usize;
    if crate::buffer::is_any_array_buffer(addr) {
        return None;
    }
    let mut len = 0_u32;
    // SAFETY: `len` is a valid out-pointer. This is the shared native-span
    // accessor: it resolves a registered view (subarray / `new T(ab, off, n)` /
    // DataView) to its backing window rather than to the view header.
    let data = unsafe { crate::buffer::js_value_buffer_or_typedarray_data(chunk, &mut len) };
    if !data.is_null() && len != 0 {
        return Some((data, len as usize));
    }
    // `(null, 0)` is both "not a binary chunk" and "an empty one" (a
    // zero-length or detached view). Only the second is a chunk.
    let is_view = crate::buffer::is_registered_buffer(addr)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some();
    if !is_view {
        return None;
    }
    // Node re-wraps every view that is not already a `Buffer`
    // (`new FastBuffer(chunk.buffer, chunk.byteOffset, chunk.byteLength)`),
    // and that construction throws once the ArrayBuffer has been transferred
    // away. A detached `Buffer` is written as it is: empty. Only reachable
    // for an empty view, so a live chunk never pays for the lookup.
    if !crate::buffer::is_node_buffer(addr) && view_backing_is_detached(addr) {
        crate::typedarray::throw_type_error(b"Cannot perform Construct on a detached ArrayBuffer");
    }
    Some((std::ptr::NonNull::<u8>::dangling().as_ptr() as *const u8, 0))
}

/// Whether the `ArrayBuffer` a view aliases has been detached (`transfer()`).
fn view_backing_is_detached(addr: usize) -> bool {
    let backing = match crate::typedarray_view::view_meta_of(addr) {
        Some(meta) => meta.backing,
        None => crate::buffer::view::backing_of(addr),
    };
    crate::buffer::is_detached_buffer(backing)
}

/// The `Buffer.from(string, encoding)` tag for a `write` encoding argument.
/// Anything that is not a string — `undefined`, or the completion callback in
/// `write(chunk, cb)` — is the default, `utf8` (tag 0).
fn string_encoding_tag(encoding: f64) -> i32 {
    if JSValue::from_bits(encoding.to_bits()).is_any_string() {
        crate::buffer::js_encoding_tag_from_value(encoding)
    } else {
        0
    }
}

/// Run `f` over exactly the bytes `write(chunk, encoding)` must put on the fd.
///
/// The common case — a string chunk with no encoding — borrows the string's
/// own payload: no allocation and no copy (the old conversion `to_vec()`'d
/// every chunk). A binary chunk is borrowed the same way. Only a non-UTF-8
/// string encoding, which has to transcode, builds a buffer.
///
/// A value that is neither a string nor a binary chunk keeps perry's existing
/// leniency and is written as its display text; Node throws
/// `ERR_INVALID_ARG_TYPE` there. That is deliberately out of scope here.
pub(super) fn with_write_bytes<R>(chunk: f64, encoding: f64, f: impl FnOnce(&[u8]) -> R) -> R {
    let value = JSValue::from_bits(chunk.to_bits());
    if value.is_any_string() {
        let mut scratch = [0_u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(chunk, &mut scratch) {
            let text: &[u8] = if ptr.is_null() || len == 0 {
                &[]
            } else {
                // SAFETY: the accessor reports `len` readable bytes at `ptr`
                // (the heap payload, or `scratch` for an inline short string).
                unsafe { std::slice::from_raw_parts(ptr, len as usize) }
            };
            return match string_encoding_tag(encoding) {
                0 => f(text),
                tag => f(&crate::buffer::buffer_string_bytes_for_encoding(text, tag)),
            };
        }
    } else if let Some((data, len)) = binary_chunk_span(chunk) {
        // SAFETY: see `binary_chunk_span` — `len` readable bytes, stable for
        // the duration of `f`, which does not touch the JS heap.
        return f(unsafe { std::slice::from_raw_parts(data, len) });
    }
    let s_ptr = crate::value::js_jsvalue_to_string(chunk);
    if s_ptr.is_null() {
        return f(&[]);
    }
    // SAFETY: a non-null `StringHeader` is followed by `byte_len` payload bytes.
    let text = unsafe {
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        std::slice::from_raw_parts(data, (*s_ptr).byte_len as usize)
    };
    f(text)
}

/// Largest single `write(2)` request. macOS rejects a count above `INT_MAX`
/// with `EINVAL`, and Linux transfers at most `0x7FFF_F000` per call anyway.
#[cfg(unix)]
const MAX_WRITE_REQUEST: usize = 0x4000_0000;

/// Block until `fd` accepts more bytes. Errors are not inspected: whatever
/// woke the poll, the caller's next `write` reports the real condition.
#[cfg(unix)]
fn wait_writable(fd: i32) {
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: one valid `pollfd`, count 1, no timeout.
    unsafe { libc::poll(&mut poll_fd, 1, -1) };
}

/// Write all of `bytes` to `fd`, completing partial writes and waiting out
/// `EAGAIN` on a non-blocking descriptor instead of dropping the remainder.
#[cfg(unix)]
pub(super) fn write_all_fd(fd: i32, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let request = bytes.len().min(MAX_WRITE_REQUEST);
        // SAFETY: `bytes` is a live slice of at least `request` bytes.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), request) };
        if written > 0 {
            bytes = &bytes[written as usize..];
            continue;
        }
        if written == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        let err = std::io::Error::last_os_error();
        match err.kind() {
            std::io::ErrorKind::Interrupted => {}
            std::io::ErrorKind::WouldBlock => wait_writable(fd),
            _ => return Err(err),
        }
    }
    Ok(())
}

/// Drain Rust's own buffer for the stream (`console.log` prints through it),
/// so bytes written straight to the fd afterwards cannot overtake it.
#[cfg(unix)]
fn flush_completely(handle: &mut impl std::io::Write, fd: i32) -> bool {
    loop {
        match handle.flush() {
            Ok(()) => return true,
            // `BufWriter` keeps the unwritten tail on error, so retrying the
            // flush resumes exactly where the fd stopped accepting bytes.
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => wait_writable(fd),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// Put `bytes` on fd 1. Holds Rust's stdout lock for the whole write so a
/// chunk is never interleaved with a concurrent `console.log`, and drains
/// that handle's buffer first so the two funnels stay in program order.
pub(super) fn write_stdout(bytes: &[u8]) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    #[cfg(unix)]
    {
        if flush_completely(&mut handle, libc::STDOUT_FILENO) {
            let _ = write_all_fd(libc::STDOUT_FILENO, bytes);
        }
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let _ = handle.write_all(bytes);
        let _ = handle.flush();
    }
}

/// Put `bytes` on fd 2. Same contract as [`write_stdout`].
pub(super) fn write_stderr(bytes: &[u8]) {
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    #[cfg(unix)]
    {
        if flush_completely(&mut handle, libc::STDERR_FILENO) {
            let _ = write_all_fd(libc::STDERR_FILENO, bytes);
        }
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let _ = handle.write_all(bytes);
        let _ = handle.flush();
    }
}

#[cfg(test)]
#[path = "os_process_stream_write_tests.rs"]
mod tests;
