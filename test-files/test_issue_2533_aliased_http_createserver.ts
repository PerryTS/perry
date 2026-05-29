// Refs #2533: a `node:http` `createServer` reached INDIRECTLY through a local
// binding — `@hono/node-server`'s `const createServer = options.createServer ||
// createServerHTTP` — must still bind to native dispatch. Pre-fix the aliased
// reference lowered to a value-read that resolved to a non-function, so calling
// it threw `TypeError: value is not a function` at `serve()` bind time.
//
// The fix lists `("http"/"https"/"http2", "createServer"/"Server"/...)` in
// `is_native_module_callable_export` so the value-read yields a bound-method
// closure, and routes the closure's invocation through a new
// `JS_NATIVE_HTTP_DISPATCH` hook (registered by perry-stdlib under
// `external-http-server-pump`) to the perry-ext-http-server factories.
//
// Scope mirrors #2153 (HttpServer dynamic dispatch): this verifies the bind
// path — `createServer` no longer throws and the returned server supports
// listen / address / close. Full request serving through the aliased path
// additionally needs runtime dynamic dispatch for IncomingMessage /
// ServerResponse methods, tracked as a follow-up.

import { createServer as createServerHTTP } from "node:http";

// A bare value-read of the native import must read as a function.
console.log("typeof createServerHTTP:", typeof createServerHTTP);

// Direct alias: `const cs = createServerHTTP`.
const cs = createServerHTTP;
console.log("typeof cs:", typeof cs);

// Indirect alias through `||` — the exact `@hono/node-server` shape.
const opts: any = {};
const createServer = opts.createServer || createServerHTTP;
console.log("typeof createServer:", typeof createServer);

const server: any = createServer((req: any, res: any) => {
  res.end();
});

console.log("server typeof:", typeof server);
console.log("listen returns server:", server.listen(0) === server);

const addr = server.address();
console.log("address typeof:", typeof addr);
// `address.family` is omitted: it reflects the default bind family (Perry
// binds IPv4 0.0.0.0, Node binds IPv6 ::) which is orthogonal to #2533.
console.log("address.port typeof:", addr && typeof addr.port);

const closeResult = server.close();
console.log("close returns server:", closeResult === server);
