// In-process fastify smoke test mirroring the
// /tmp/perry-compat-sweep/fastify/entry.ts fixture.
//
// Pre-fix `js_fastify_listen` (in both perry-stdlib's bundled adapter
// AND perry-ext-fastify) entered a blocking inner event loop and never
// returned, so `await app.listen(...)` never resumed — the in-process
// `fetch` against the same process never even started and the program
// timed out at gtimeout(30s) with rc=124.
//
// After the fix, `listen()` returns immediately and the per-server
// mpsc receiver is drained from perry-stdlib's main pump on every
// tick. The handler runs on the main TS thread, the response flows
// back through hyper, the in-process fetch resolves, and `app.close()`
// flips the listening flag so `js_stdlib_has_active_handles` lets the
// runtime exit.
import Fastify from "fastify";

const app = Fastify();
app.get("/ping", async () => ({ ok: true }));

async function main() {
  await app.listen({ port: 18933 });
  const r = await fetch("http://127.0.0.1:18933/ping");
  const j = (await r.json()) as { ok: boolean };
  console.log("ok=" + j.ok);
  await app.close();
}

main();
