// Issue #1124 followup — `http.get(url, cb)` client-side `data` event
// must deliver bytes as a Buffer (not a UTF-8 lossy string).
//
// The initial #1124 fix in v0.5.1011 pinned the SERVER-side wire body
// integrity (Buffer vs StringHeader layout in
// `jsvalue_to_body_bytes`) but explicitly left the CLIENT-side
// data-event dispatch as a separate follow-up. Pre-fix code path at
// `crates/perry-ext-http/src/lib.rs:737` routed received bytes
// through `alloc_string(str::from_utf8(&body_clone).unwrap_or(""))`
// — any non-UTF-8 byte (e.g. PNG file-magic's leading 0x89)
// collapsed the entire payload to an empty string before user code
// ever saw a byte. The wire-byte assertion harness
// (run_test_issue_1124.sh) had to do its check via curl-and-xxd
// from outside the process for that reason.
//
// This test exercises both sides in the SAME TS file:
//   1. server binds 18993, responds with `Buffer.from(PNG_MAGIC)`
//      (the server-side fix from v0.5.1011 ensures these bytes
//      survive the wire)
//   2. client `http.get` reads via `'data'` → `Buffer.concat`
//      → prints the first 8 bytes as a comma-joined string
//   3. server.close + exit 0
//
// Expected output proves the client received the exact 8 magic
// bytes — the v0.5.1011 client path would have printed an empty
// "Array.from(b.slice(0,8)).join(',')" because Buffer.concat over
// empty chunks gives an empty Buffer.

import { createServer, get } from "node:http";

const PNG_MAGIC = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const PORT = 18993;

const server = createServer((_req: any, res: any) => {
    res.statusCode = 200;
    res.setHeader("Content-Type", "application/octet-stream");
    res.end(Buffer.from(PNG_MAGIC));
});

server.listen(PORT, () => {
    // Fire the client request once the server is bound. Note:
    // `http.get(url)` returns a ClientRequest; the response
    // callback is the trailing arg.
    //
    // Issue #1132 — the inner response callback param is `res`, the
    // SAME name as the outer `(_req, res)` createServer handler. This
    // used to require renaming to `resp` to sidestep a HIR scope
    // leak: the outer `res`'s `("http", "ServerResponse")`
    // native-instance tag (registered first, never scope-truncated)
    // shadowed the inner `res`, so `res.on('data')` / `res.on('end')`
    // misrouted through ServerResponse dispatch instead of the
    // client-side IncomingMessage path. Fixed in v0.5.1015
    // (last-match-wins `lookup_native_instance` + pre-scan tags
    // scoped to the owning call). This file now uses `res` to also
    // serve as a #1132 regression guard.
    get("http://127.0.0.1:" + PORT + "/", (res: any) => {
        const chunks: any[] = [];
        res.on("data", (c: any) => {
            chunks.push(c);
        });
        res.on("end", () => {
            // `Buffer.concat(chunks)` flattens the per-chunk
            // Buffers into one contiguous block. Slice(0,8)
            // takes the magic-byte prefix; `Array.from(...)`
            // converts to a JS array; `.join(',')` produces
            // the expected stable stdout signature.
            const b = Buffer.concat(chunks);
            const first8 = Array.from(b.slice(0, 8)).join(",");
            console.log(first8);
            server.close();
            console.log("CLOSED");
        });
    });
});

// Self-terminating safety net (same shape as the v0.5.1012 listen
// test). If anything in the lifecycle hangs, the runtime still
// exits within the parity-runner's 30s budget.
setTimeout(() => {}, 1500);
