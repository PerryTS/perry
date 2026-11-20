`process.stdout.write(chunk[, encoding])` / `process.stderr.write(...)` now put
the chunk's **bytes** on the fd (#10903).

Both stubs started from `js_jsvalue_to_string(chunk)` — the chunk's display text
— and never read `encoding`. A `Buffer` / `Uint8Array` was UTF-8 *decoded* and
the text written, so every byte that is not valid UTF-8 reached the fd as
`EF BF BD`: a 4-byte frame length of 200 (`C8 00 00 00`) was enough to corrupt
a binary protocol (found with a Native Messaging host). Any other `TypedArray`
went out as its `join(",")` text (`new Uint16Array([0x6968, 0x0a21])` printed
`26984,2593`), a `DataView` as `[object DataView]`, and
`write("6865780a", "hex")` wrote eight characters instead of four bytes. The
write itself used `Stdout::write_all`, which stops at the first `EAGAIN` after
an unknown prefix, and the error was discarded — on a non-blocking fd 1 an
8 MiB chunk delivered 131,072 bytes, exit code 0.

Now, as in Node: a `Buffer` / any `TypedArray` / `DataView` is written byte for
byte, exactly the window the view covers (`subarray`, `new T(ab, off, n)`,
`DataView(ab, off, n)`); a string is encoded with `encoding` (`latin1`,
`binary`, `ascii`, `hex`, `base64`, `base64url`, `ucs2`, `utf16le`; default
`utf8`), using `Buffer.from`'s encoder; a binary chunk ignores `encoding`; a
view whose ArrayBuffer was `transfer()`red away throws a `TypeError`. The new
`write_all_fd` completes partial writes, retries `EINTR`, waits out `EAGAIN`
with `POLLOUT`, returns `EPIPE` instead of spinning, and caps a request at
1 GiB; Rust's stdout handle is flushed first and its lock held across the
write, so `console.log` and `write` stay in program order.

The common path got cheaper: a utf8 string chunk is written from its own
payload instead of a `to_vec()` copy — `instructions:u` per call −182 (−5.2%)
for a string write, −23,924 (−86.7%) for a 49-byte `Uint8Array` write, and
`console.log` (untouched, the control) +7 (+0.2%).

Unchanged on purpose: a value that is neither a string nor a binary chunk
(`42`, `null`, an `ArrayBuffer`) and an unknown encoding name keep perry's
leniency; Node throws `ERR_INVALID_ARG_TYPE` / `ERR_STREAM_NULL_VALUES` /
`ERR_UNKNOWN_ENCODING` there.

Tests: `crates/perry/tests/issue_10903_stdout_write_bytes.rs` compares both fds
byte for byte with what Node 26.5.1 writes for
`test-files/test_gap_10903_stdout_write_bytes.ts` — chunk kinds and windows,
encodings, callbacks, `console.log` interleaving, Native Messaging framing,
5 MiB / 16 / 32 / 64 MiB single writes, 20,000 small writes, detached views,
and the large cases again over a non-blocking socket with a slow reader. The
same fixture, undriven, is a text parity case. Nine unit tests in
`os_process_stream_write_tests.rs` cover the conversion and `write_all_fd`.
