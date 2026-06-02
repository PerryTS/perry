// Regression test for issue #997 — V8-fallback `http.createServer().listen()`
// must keep the outer event loop alive while the bind future is in flight on
// a tokio worker, even when no other source (timers, stdlib pumps, http
// active count) has been registered yet.
//
// Pre-fix, `app.listen(port, callback)` returned to the codegen-emitted
// outer event loop while `op_perry_http_listen`'s `TcpListener::bind`
// future was still suspended on the multi-thread runtime. The header
// `js_jsruntime_has_active_handles` only inspected resolved-state
// counters (ACTIVE_SERVERS / pending tick / module eval), saw zero,
// and exited the program before bind completed. User-visible symptom:
// "listening" never prints and the server isn't reachable.
//
// Fix: `poll_v8_event_loop_once` now records `last_poll_was_pending`,
// and `jsruntime_has_active_handles` returns 1 whenever the last
// poll left deno_core with refed ops in flight. This keeps the
// outer loop ticking until the bind resolves and ACTIVE_SERVERS
// increments. See crates/perry-jsruntime/src/{lib,interop}.rs.
//
// Smoke shape mirrors test_http_createserver_v8.ts but with a
// synchronous listen callback and an immediate server.close(), so
// the test exits deterministically once the callback fires. The
// success criterion is that "listening" prints at all — pre-fix this
// hung indefinitely (rc=124 under `timeout`).

import http from "node:http";

const server = http.createServer((_req, res) => {
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end("ok");
});

const port = 18997;
server.listen(port, () => {
    console.log("listening");
    server.close();
});
