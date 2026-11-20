// Gap test for #5433: `new Response()` / `new Request()` must satisfy
// `instanceof Response` / `instanceof Request`. The native fetch handles are
// pointer-tagged small-integer ids (not heap objects), so `instanceof` relies
// on a registered kind-probe. A feature-gate mismatch (#5174 split
// `http-client = ["web-fetch"]`) left the probe unregistered in the
// auto-optimize `web-fetch` build, so the guard below was always false.
// Compared byte-for-byte against `node --experimental-strip-types`.

const r = new Response("hi", { status: 200 });
console.log("typeof:", typeof r);
console.log("r instanceof Response:", r instanceof Response);
console.log("r instanceof Request:", r instanceof Request);
console.log("r.status:", r.status);

const q = new Request("http://example.com/");
console.log("q instanceof Request:", q instanceof Request);
console.log("q instanceof Response:", q instanceof Response);

// The Hono route-fallback idiom: discriminate a `T | Response` union.
function gate(ok: boolean): string | Response {
  return ok ? "value" : new Response("denied", { status: 401 });
}
const a = gate(false);
console.log("guard catches Response:", a instanceof Response);
const b = gate(true);
console.log("guard passes value:", b instanceof Response);
