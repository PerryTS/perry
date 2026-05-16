// Regression for the #835/#846 follow-up — the auto-optimize stdlib
// build dropped the `bundled-streams` feature whenever the user TS did
// not contain `import "streams"`. Compiled-package code (Effect's
// `Stream` lowering, Express compile, …) emits FFI calls to
// `js_readable_stream_*` / `js_node_http_create_server` directly, but
// those symbols were `#[cfg]`-gated out of perry-stdlib (and
// perry-ext-http never made it onto the link line), so the link
// failed with "Undefined symbols: _js_readable_stream_new" etc.
//
// The fix: codegen's ext_registry (`crates/perry-codegen/src/ext_registry.rs`)
// now records every Stdlib-resident FFI call with its required
// perry-stdlib Cargo feature. The compile driver drains those into
// `ctx.extra_stdlib_features`, and `build_optimized_libs` unions them
// into the feature set so the auto-optimize stdlib build actually
// includes the providing module.
//
// What this test exercises (without importing `"streams"` or `"node:http"`):
//   - `new ReadableStream({...})` → js_readable_stream_new +
//     js_readable_stream_controller_enqueue +
//     js_readable_stream_controller_close
//   - `createServer(handler)` from `node:http` → js_node_http_create_server
//
// We DO have `import { createServer } from "node:http"` here because
// the user-facing API requires it — the regression that would slip
// past is the ReadableStream half: if `bundled-streams` is dropped
// from the auto-optimize rebuild, the link fails on the
// `_js_readable_stream_*` symbols.
//
// If perry-ext-http is missing from disk in the build environment,
// the link will still fail on `_js_node_http_create_server` — that's a
// separate environmental concern, not a regression of this fix.

import { createServer } from "node:http";

async function main(): Promise<void> {
  // ── ReadableStream half — exercises the streams FFI surface ─────
  const stream = new ReadableStream<string>({
    start(controller: any): void {
      controller.enqueue("hello");
      controller.close();
    },
  });

  const reader = stream.getReader();
  const r1 = await reader.read();
  console.log("stream r1 done: " + r1.done);
  console.log("stream r1 value: " + r1.value);

  // ── http.createServer half — exercises the node:http FFI surface ─
  // We don't actually .listen() — just construct the server so the
  // js_node_http_create_server symbol is reachable at link time.
  // (A listening server would block the process unless we ran on a
  // worker; this test only validates that the FFI symbol resolves.)
  const server = createServer((_req: any, res: any) => {
    res.end("ok");
  });
  console.log("server created: " + (typeof server === "object"));
}

main();
