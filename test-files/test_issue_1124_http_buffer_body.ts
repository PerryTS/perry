// Issue #1124 — `http.createServer` accepted `Buffer` chunks in
// `res.write(buf)` / `res.end(buf)` and emitted the correct `Content-Length`,
// but the wire body was zeroed.
//
// Root cause: `crates/perry-ext-http-server/src/types.rs::jsvalue_to_body_bytes`
// (lines 121–160) cast every POINTER_TAG pointer straight to
// `*mut StringHeader` and read `byte_len` + `data_after_StringHeader` from it.
// But `BufferHeader` is `{ length: u32, capacity: u32 }` (8 bytes, data
// immediately after) and `StringHeader` is
// `{ utf16_len, byte_len, capacity, refcount, flags }` (20 bytes, data after
// that). Reading a Buffer pointer through the string-shaped header surfaced
// the buffer's `capacity` slot at offset 4 as the "byte_len" — coincidentally
// equal to the requested size for exact-fit allocations — and then indexed
// the data from `ptr + sizeof(StringHeader)`, past the buffer's actual
// bytes. The result: length preserved, contents = zeros.
//
// Fix: probe the runtime's BUFFER_REGISTRY via the existing
// `js_buffer_is_buffer(ptr) -> i32` extern (declared in
// `crates/perry-runtime/src/buffer.rs:601`) before falling back to the
// StringHeader layout. Buffers read through BufferHeader's
// `length` + 8-byte-header layout; the StringHeader path stays for the
// non-buffer POINTER_TAG case.
//
// This is the server side of the regression test — the validation that
// the bytes survive the wire happens in
// `test-files/run_test_issue_1124.sh`, which boots this server in the
// background, curls it, and asserts that the response body is the
// PNG file-magic byte sequence (8 bytes, first byte 0x89, NOT all zeros).
// We can't do the assertion inside the same TS file because perry-ext-http's
// client-side `http.get` `data` listener dispatch (lib.rs:737) routes
// the body through `alloc_string(str::from_utf8(&body).unwrap_or(""))` —
// invalid UTF-8 (0x89 byte) collapses to an empty string before the
// listener sees it. Fixing client-side binary-body delivery is tracked
// separately; this test pins the SERVER-side fix #1124 alone.

import { createServer } from "node:http";

const PNG_MAGIC = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const PORT = 18993;

const server = createServer((_req: any, res: any) => {
    res.statusCode = 200;
    res.setHeader("Content-Type", "application/octet-stream");
    const buf = Buffer.from(PNG_MAGIC);
    res.end(buf);
});

server.listen(PORT, () => {
    console.log("LISTENING");
    // Self-close so the test fixture exits cleanly under the parity runner
    // (which compares stdout against `test-parity/expected/<name>.txt` and
    // times out at 30s if the process doesn't terminate). The wire-byte
    // assertion happens in `run_test_issue_1124.sh` — that script runs
    // this same fixture in the background, curls it BEFORE the timer
    // fires, then lets the timer take the server down. 750ms is generous
    // enough for the curl + xxd pipe in the harness to land first.
    setTimeout(() => {
        server.close();
        console.log("CLOSED");
    }, 750);
});
