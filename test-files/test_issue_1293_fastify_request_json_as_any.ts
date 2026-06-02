// Issue #1293 — fastify `(request as any).json()` / `(request as any).body`
// returned NaN / undefined (silent 400) under the well-known-flipped
// perry-ext-fastify backend.
//
// #1240 fixed `request.json()` on the *typed* receiver — that lowers to a
// `NativeMethodCall { module: "fastify" }` and dispatches through the static
// NATIVE_MODULE_TABLE → `js_fastify_ctx_json` (with its zero-arg detection).
//
// But the standard Fastify body-parse pattern in the wild casts the receiver:
// `const body = (request as any).json(); if (!body) return reply.status(400)…`.
// The `as any` cast erases the static type, so codegen emits a *generic*
// dynamic dispatch (`Call { callee: PropertyGet }` / `PropertyGet`) instead of
// a `NativeMethodCall`. At runtime that lands in perry-stdlib's
// `js_handle_method_dispatch` / `js_handle_property_dispatch` — which probed
// only perry-stdlib's *own* handle registry. Under the well-known flip the
// FastifyContext lives in perry-ffi's registry instead, so the probe missed,
// the call fell through, and `json()` came back as a bare NaN (`typeof
// "number"`) while `.body` came back `undefined`.
//
// Fix: perry-ext-fastify exports `js_ext_fastify_is_context_handle`, and
// perry-stdlib's dispatch grows `external-fastify-pump` arms (mirroring the
// `external-net-pump` arms) that consult it and forward to the linked
// `js_fastify_*` exports.
//
// Wire-level assertion: every shape — typed json(), `(as any).json()`,
// `(as any).body`, and json() consumed only by `if(!body)` — returns the
// parsed object intact.

import Fastify from "fastify";

const PORT = 18993;
const app = Fastify({ logger: false });

// Typed receiver — the #1240 path (NativeMethodCall). Must keep working.
app.post("/typed", async (request, reply) => {
    const body = request.json() as { hello?: string };
    if (!body) return reply.status(400).send({ where: "typed", body: "falsy" });
    return { where: "typed", typeofBody: typeof body, hello: body.hello ?? null };
});

// `(request as any).json()` consumed only by `if (!body)` — the exact #1293
// repro. Pre-fix this 400'd because json() returned NaN.
app.post("/any-json", async (request, reply) => {
    const body = (request as any).json() as any;
    if (!body) return reply.status(400).send({ where: "any-json", body: "falsy" });
    return { where: "any-json", typeofBody: typeof body, hello: body.hello ?? null };
});

// `(request as any).body` — the property-access shape (the workaround the
// issue author shipped). Goes through `js_handle_property_dispatch`.
app.post("/any-body", async (request, reply) => {
    const body = (request as any).body as any;
    if (!body) return reply.status(400).send({ where: "any-body", body: "falsy" });
    return { where: "any-body", typeofBody: typeof body, hello: body.hello ?? null };
});

await app.listen({ port: PORT, host: "127.0.0.1" });
console.log(`listening on :${PORT}`);
