// Issue #1140 — integer indexing into a runtime-allocated
// (registry-backed) Buffer returned 0, while every other accessor on
// the SAME Buffer was correct.
//
// Two Buffer code paths exist:
//   1. JS-constructed `Buffer.from([...])` bound to a local — indexed
//      correctly even pre-fix (a typed-receiver codegen path). Kept
//      below as a regression guard so the fix doesn't regress it.
//   2. A runtime-allocated Buffer — the chunk an `any`-typed
//      `.on('data', cb)` listener receives (allocated via
//      `alloc_buffer`/`js_buffer_alloc`, tracked in BUFFER_REGISTRY).
//      `chunk[i]` came back 0: `obj[i]` on an `any` receiver lowers to
//      `js_dyn_index_get`, which read a GcHeader that registry-backed
//      Buffers DON'T carry, fell through to an 8-byte-f64 inline read
//      at `raw_ptr + 8 + idx*8`, and surfaced the buffer's first 8
//      data bytes reinterpreted as a denormal f64 (prints `0`).
//      `.length` / `.toString()` / `Array.from()` all probe
//      BUFFER_REGISTRY and were correct — that asymmetry IS the bug.
//
// This pins ALL accessors agreeing on BOTH representations. The
// runtime-allocated chunk arrives via the net 'data' path (the same
// shape as test_issue_1123_listen.ts, which documents this exact bug
// in its header comment). The payload is a `Buffer.from([...])` so the
// server receives binary bytes including 0x89 (invalid UTF-8 — proves
// we're reading the byte, not a lossy string).

import { createServer, connect } from "node:net";

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const PORT = 18994;

// ── Regression guard for verified-fact #1: JS-constructed Buffer ──────────
const jsBuf = Buffer.from(PNG_MAGIC);
console.log("JS buf[0]=" + jsBuf[0]);
console.log("JS buf[7]=" + jsBuf[7]);
console.log("JS buf.length=" + jsBuf.length);
console.log("JS buf.hex=" + jsBuf.toString("hex"));
console.log("JS Array.from[0]=" + Array.from(jsBuf)[0]);
console.log("JS buf[99]=" + jsBuf[99]);

const server = createServer((sock: any) => {
    sock.on("data", (chunk: any) => {
        // `chunk` is the registry-backed Buffer — the bug's home.
        // Numeric indexing must equal the magic bytes.
        console.log("RT chunk[0]=" + chunk[0]);
        console.log("RT chunk[last]=" + chunk[chunk.length - 1]);
        console.log("RT chunk.length=" + chunk.length);
        // Working accessors — must agree with the indexing above.
        console.log("RT chunk.hex=" + chunk.toString("hex"));
        console.log("RT Array.from[0]=" + Array.from(chunk)[0]);
        // Out-of-range / negative — Node semantics: undefined, not 0.
        // The index is a `: number`-typed local so the access provably
        // routes through `js_dyn_index_get` (the fixed FFI). NOTE: the
        // more natural `chunk[chunk.length + 5]` form returns 0 here —
        // a SEPARATE pre-existing codegen-routing bug where an index
        // expression that isn't statically numeric (`.length` on an
        // `any` receiver) bypasses `js_dyn_index_get` entirely and hits
        // the generic array/object fall-through. That is independent of
        // this registry-buffer fix and is left untouched.
        const oobIdx: number = chunk.length + 5;
        const negIdx: number = -1;
        console.log("RT chunk[oob]=" + chunk[oobIdx]);
        console.log("RT chunk[neg]=" + chunk[negIdx]);
    });
});

server.listen(PORT, () => {
    console.log("LISTENING " + PORT);
    const client = connect(PORT, "127.0.0.1");
    client.on("connect", () => {
        client.write(Buffer.from(PNG_MAGIC));
        setTimeout(() => {
            client.end();
        }, 100);
    });
    client.on("close", () => {
        console.log("CLOSED");
        server.close();
    });
});

// Self-terminating safety net (same shape as test_issue_1123_listen.ts).
setTimeout(() => {}, 1500);
