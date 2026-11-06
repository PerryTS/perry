import http from "node:http";

// Regression test for #4004.
//
// A node:http server allocates handles in the low small-handle band
// (< 0x40000); WHATWG fetch `Request` / `Headers` handles were moved into a
// disjoint high band (0x40000+) by #4018 so the two can no longer share an id.
// But that move surfaced a latent crash: the runtime's small-handle type
// probes (`is_date_value`, `is_registered_map`, …) only skipped addresses
// below ~0x1000, so a 0x40000+ fetch handle reached an untyped method/property
// dispatch was dereferenced as a heap pointer (reading a GC header at id-8) and
// segfaulted. This is exactly the access shape a Hono "node:http adapter"
// takes — `request.headers.get(...)` on an `any`-typed request — which is why
// it forced a startup-warmup workaround.
//
// Allocate a live server handle (low band), then build a fetch Request (high
// band) and exercise it through an `any`-typed receiver so the call routes
// through the runtime handle-dispatch tower rather than the statically-typed
// fast path.
const server = http.createServer(() => {});
console.log("server typeof:", typeof server);

const request = new Request("http://localhost/path", {
  method: "POST",
  headers: { "x-test": "perry", "content-type": "application/json" },
}) as any;

console.log("is Request:", request instanceof Request);
console.log("method:", request.method);
console.log("url:", request.url);

const headers = request.headers;
console.log("headers typeof:", typeof headers);
console.log("get is function:", typeof headers.get === "function");
console.log("x-test:", headers.get("x-test"));
console.log("content-type:", headers.get("content-type"));
console.log("missing:", headers.get("nope"));
console.log("has x-test:", headers.has("x-test"));
console.log("has nope:", headers.has("nope"));

// A standalone Headers reached through an `any` receiver hits the same path.
const h = new Headers({ a: "1", b: "2" }) as any;
console.log("untyped Headers get:", h.get("a"));
console.log("untyped Headers has:", h.has("b"));
console.log("untyped Headers missing:", h.get("z"));
