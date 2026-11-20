// Issue #4728 — a `node:http` request handler that finishes the response
// on a *later* event-loop tick (an async handler: `setTimeout`, an
// outbound `fetch()`, any `await` chain that calls `res.end()` from a
// microtask/timer/tokio resolution) must still flush the real response.
//
// Pre-fix, `process_pending` synthesized a premature empty 200 and freed
// the per-request handles the moment the handler returned, so the real
// `res.end(...)` later fired on a dropped handle (no-op) and the client
// saw an empty/closed reply. Now such requests are parked until the
// response is actually flushed.
import http from 'node:http';

// Backend: responds 150ms after the request arrives (async res.end via
// setTimeout). Exercises the "handler returns before res.end()" path.
const backend = http.createServer((req, res) => {
  setTimeout(() => {
    res.statusCode = 200;
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ hello: 'backend' }));
  }, 150);
});

// Front: fetch()es the backend *inside* the request handler, then writes
// the result. Exercises the "outbound fetch inside handler" path that the
// issue's Hono/SSR adapter hit.
const front = http.createServer((req, res) => {
  void (async () => {
    const r = await fetch(`http://127.0.0.1:18841/`);
    const body = await r.text();
    res.statusCode = 200;
    res.end('fetched:' + body);
  })();
});

await new Promise<void>(resolve => backend.listen(18841, '127.0.0.1', () => resolve()));
await new Promise<void>(resolve => front.listen(18840, '127.0.0.1', () => resolve()));

// 1) Async res.end (setTimeout) on the backend, hit directly.
const direct = await fetch('http://127.0.0.1:18841/');
console.log('direct:', direct.status, (await direct.text()));

// 2) fetch()-inside-handler on the front (which itself depends on the
//    backend's async res.end resolving).
const chained = await fetch('http://127.0.0.1:18840/');
console.log('chained:', chained.status, (await chained.text()));

backend.close();
front.close();
