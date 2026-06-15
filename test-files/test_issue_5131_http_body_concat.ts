// Issue #5131 — a `node:http` server that consumes the request body via
// `req.on("data", c => body += c)` on a POST that actually carries a body
// segfaulted (SIGSEGV / exit 139).
//
// Root cause: the un-typed `data` chunk is a `Buffer`, so `body += c` lowers
// to the fully-dynamic add helper `js_dynamic_string_or_number_add` →
// `to_primitive_default_for_add` (crates/perry-runtime/src/value/dynamic_arith.rs).
// That function lacked a Buffer/TypedArray guard, so it fell through to the
// `js_url_href_if_url` / `try_read_as_search_params` / `OrdinaryToPrimitive`
// probes, all of which bit-cast the operand pointer to an `ObjectHeader` and
// read its fields. A `BufferHeader` carries NO `ObjectHeader`/`GcHeader`, so
// the probe dereferenced a fake header one word before the data → crash.
//
// (The same pattern with a statically-typed `const c = Buffer.from(...)`
// worked because codegen proved `c` non-string and used the safe
// `js_string_concat_value` coerce path, which routes through
// `js_jsvalue_to_string`'s existing buffer guard.)
//
// Fix: detect Buffers/TypedArrays via their registries (by-value lookups, no
// deref) in `to_primitive_default_for_add` before the ObjectHeader probes and
// route them to `js_jsvalue_to_string`, mirroring the guard `js_jsvalue_to_string`
// itself runs. Expected output: `len:13`.

import http from "node:http";

const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    res.writeHead(200);
    res.end("len:" + body.length);
  });
});

server.listen(0, () => {
  const port = (server.address() as { port: number }).port;
  const r = http.request({ port, method: "POST" }, (res) => {
    let d = "";
    res.on("data", (c) => (d += c));
    res.on("end", () => {
      console.log(d);
      server.close();
    });
  });
  r.write("payload-bytes");
  r.end();
});
