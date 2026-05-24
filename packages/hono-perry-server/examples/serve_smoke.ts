// Smoke test / example for `@hono/perry-server`. Drives the adapter with a
// minimal hand-rolled fetch handler (no `hono` dependency) so it can be
// compiled and run standalone in CI:
//
//   perry compile examples/serve_smoke.ts -o /tmp/serve_smoke
//   /tmp/serve_smoke &        # listens on :18992
//   curl localhost:18992/     # -> {"ok":true,"method":"GET"}
//
// Mirrors what `serve({ fetch: app.fetch, port })` does for a real
// `new Hono()` app, but keeps the fixture self-contained.
import { serve } from '../src/index'

const fetchHandler = async (req: Request): Promise<Response> => {
  return new Response(JSON.stringify({ ok: true, method: req.method }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

serve({ fetch: fetchHandler, port: 18992 }, (info) => {
  console.log(`serve_smoke listening on :${info.port} (${info.family})`)
})
