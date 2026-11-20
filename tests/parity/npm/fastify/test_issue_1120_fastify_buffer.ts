// Issue #1120 — fastify reply Buffer / Uint8Array integrity + auto-HEAD.
//
// Part 1 (body): returning a `Buffer` / `Uint8Array` from a handler
// used to be JSON-serialized via `Buffer.toJSON()` /
// numeric-key-object stringification, producing wire payloads like
// `{"type":"Buffer","data":[137,80,...]}` or `{"0":1,"1":2,...}` and
// overwriting any `reply.type(...)` the handler had set. A 1.3 MB
// binary asset would balloon ~6× and ship as `application/json`.
//
// Root cause: both perry-ext-fastify and the bundled perry-stdlib
// fastify funnelled non-string handler returns through
// `js_json_stringify`. Fix: probe `js_buffer_is_buffer(ptr)` first
// in `crates/perry-ext-fastify/src/{context,server}.rs` and
// `crates/perry-stdlib/src/fastify/{context,server}.rs`, ship raw
// bytes, default `content-type` to `application/octet-stream` when
// the handler didn't pin one via `reply.type(...)`.
//
// Part 2 (HEAD): Node fastify auto-handles HEAD against any
// registered GET (via `app.head` shadowing). Pre-fix Perry's
// fastify returned a 404 JSON for `HEAD /buf` because the route
// matcher only accepted exact-method matches. Fix: when no exact
// HEAD route matches, fall back to a GET route on the same path,
// rewrite the dispatch method to GET, then drop the body on the
// wire while preserving Content-Length so clients see the size
// they'd get from GET.
//
// The wire-byte assertions happen in `run_test_issue_1120.sh`:
//   - GET /buf  →  body is the 8-byte PNG magic, content-type is
//     `application/octet-stream`. Validates part 1 (Buffer path).
//   - GET /u8   →  body is `01 02 03 04 05`, content-type is
//     `application/octet-stream`. Validates Uint8Array path.
//   - HEAD /buf →  body is empty, Content-Length is 8, status 200.
//     Validates part 2 (auto-HEAD).

import fastify from "fastify";
import { Buffer } from "node:buffer";

const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const PORT = 18996;

const app = fastify();

app.get("/buf", async (_req: any, reply: any) => {
    reply.type("application/octet-stream");
    return Buffer.from(PNG_MAGIC);
});

app.get("/u8", async (_req: any, _reply: any) => {
    // No `reply.type(...)` here — the dispatcher should default to
    // `application/octet-stream` because the returned payload is a
    // `Uint8Array` (binary). Pre-fix this came back as `application/json`
    // with a `{"0":1,"1":2,...}` body.
    return new Uint8Array([1, 2, 3, 4, 5]);
});

app.listen({ port: PORT, host: "127.0.0.1" }, (err: any) => {
    if (err) {
        console.log("ERR " + err);
        return;
    }
    console.log("LISTENING");
    // Self-close so the parity runner sees deterministic stdout and
    // exits within its 30s budget. 1500ms gives the harness room to
    // issue 3 sequential curls.
    setTimeout(() => {
        app.close();
        console.log("CLOSED");
    }, 1500);
});
