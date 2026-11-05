// #5432: `.forEach()` / `.entries()` on the Headers of a Response returned by a
// member-call `app.fetch(req)` (the Fetch-API / WinterCG server-handler shape
// Hono, itty-router, and Cloudflare Workers all use) SIGSEGV'd in
// js_array_forEach: the unregistered receiver let the static array-method fold
// rewrite `res.headers.forEach(cb)` into `Expr::ArrayForEach` and codegen
// dispatched `js_array_forEach` on the Headers handle id (0x40000 band).
// `app.fetch` is modeled here without hono so the test needs no npm package —
// the crash is the member-`.fetch()` call returning a native Response, not hono
// itself. The assertions filter to the explicit `x-` headers so the test does
// not depend on whether a default `content-type` is derived from the body.
class App {
  fetch(_req: Request): Response {
    return new Response("ok", { headers: { "x-a": "1", "x-b": "2" } });
  }
}

const app = new App();
const res = app.fetch(new Request("http://h/x"));
console.log("status:", res.status);

// forEach must iterate (used to SIGSEGV).
const seen: string[] = [];
res.headers.forEach((value: string, key: string) => {
  if (key.startsWith("x-")) seen.push(key + "=" + value);
});
seen.sort();
console.log("forEach:", JSON.stringify(seen));

// entries() must yield the same pairs (used to silently yield 0).
const ents: string[] = [];
for (const [k, v] of res.headers.entries()) {
  if (k.startsWith("x-")) ents.push(k + "=" + v);
}
ents.sort();
console.log("entries:", JSON.stringify(ents));
