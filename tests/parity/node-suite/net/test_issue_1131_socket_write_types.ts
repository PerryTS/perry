// Issue #1131 — `net.Socket.write(chunk)` must put the correct bytes
// on the wire regardless of whether `chunk` is a JS string, a
// `Buffer`, or a `Uint8Array`.
//
// Pre-#1131 `js_net_socket_write` reinterpreted EVERY chunk argument
// as a `*const BufferHeader`. A JS string is a `*StringHeader` (a
// 20-byte header: utf16_len, byte_len, capacity, refcount, flags),
// while a Buffer is an 8-byte `BufferHeader` (length, capacity). So
// `sock.write("ping")` read the string's `utf16_len` (4) as the
// buffer length but pulled "data" from `ptr + sizeof(BufferHeader)`
// = `ptr + 8` — the middle of the StringHeader struct — emitting
// garbage instead of `p i n g`. (Empirically: pre-fix the wire
// carried a 326-byte blob, not the 4 intended bytes.) This is the
// outbound mirror of #1124 (inbound `res.write(Buffer)` zeroed
// because a Buffer was read through the StringHeader layout).
//
// The fix passes the full NaN-boxed value to the runtime, which
// probes `BUFFER_REGISTRY` (Buffer / Uint8Array) vs STRING_TAG (JS
// string) and reads through the correct layout.
//
// This test sends all three chunk shapes from the client over one
// connection and asserts the server received the exact bytes for
// each. The server asserts via `chunk.toString("hex")` rather than
// `chunk[N]`: there is a separate pre-existing bug where integer
// indexing of a Buffer delivered through an `any`-typed
// `.on('data', cb)` param returns 0 (length / toString / hex are
// all correct — only `chunk[N]` is wrong; reproduces with zero net
// code). `toString("hex")` is layout-correct and preserves the
// high byte (0xFF) that a UTF-8 round-trip would mojibake, so it's
// a true byte-integrity assertion.
//
// Writes are staggered with small timers so the three server-side
// 'data' events arrive in send order on loopback. Expected stdout:
//
//   LISTENING 18996
//   GOT hex=70696e67
//   GOT hex=010203ff
//   GOT hex=0a141e
//   CLOSED

import { createServer, connect } from "node:net";

const PORT = 18996;

const server = createServer((sock: any) => {
    sock.on("data", (chunk: any) => {
        console.log("GOT hex=" + chunk.toString("hex"));
    });
});

server.listen(PORT, () => {
    console.log("LISTENING " + PORT);
    const client = connect(PORT, "127.0.0.1");
    client.on("connect", () => {
        // 1. bare JS string → UTF-8 bytes 0x70 0x69 0x6e 0x67.
        client.write("ping");
        // 2. Buffer.from([...]) with a high byte (0xFF) that a UTF-8
        //    round-trip would mojibake — proves binary integrity.
        setTimeout(() => {
            client.write(Buffer.from([1, 2, 3, 255]));
        }, 80);
        // 3. Uint8Array — same BUFFER_REGISTRY membership as Buffer,
        //    must read through the BufferHeader layout too.
        setTimeout(() => {
            client.write(new Uint8Array([10, 20, 30]));
        }, 160);
        // Close after the last frame has had time to land.
        setTimeout(() => {
            client.end();
        }, 280);
    });
    client.on("close", () => {
        console.log("CLOSED");
        server.close();
    });
});

// Self-terminating safety net (same shape as the #1123 listen test).
setTimeout(() => {}, 2000);
