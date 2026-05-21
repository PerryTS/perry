// Issue #1240 — fastify `request.json()` returns `undefined` (silent 400).
//
// Before the fix, the native dispatch table routed the `json` method to
// `js_fastify_ctx_json` (the Hono-style `c.json(data, status?)` reply
// helper). When user code wrote `request.json()` with no args, the codegen
// padded the missing `data` slot with NaN-boxed `undefined` and the runtime
// shim happily serialized `"undefined"` as the response body AND set
// `ctx.sent = true`, silently 400-ing every POST endpoint.
//
// Fix (perry-stdlib + perry-ext-fastify): `js_fastify_ctx_json` now detects
// the zero-arg shape (`data == NaN-boxed undefined`) and routes to
// `js_fastify_req_json`, matching the Fetch API `Request.json()` semantics
// the user's codebase relies on.
//
// Wire-level assertion: POST a JSON payload, return the parsed object, and
// verify the round-trip arrived intact. Pre-fix the response was
// `{"viaJsonUndefined":true}`; post-fix it has the parsed key.

import Fastify from "fastify";

const PORT = 18997;
const app = Fastify({ logger: false });

app.post("/json-test", async (request, _reply) => {
    const parsed = request.json();
    return {
        typeofParsed: typeof parsed,
        parsedIsUndefined: parsed === undefined,
        hasHelloKey:
            parsed !== null &&
            parsed !== undefined &&
            typeof parsed === "object" &&
            "hello" in (parsed as Record<string, unknown>),
        hello: (parsed as { hello?: string })?.hello ?? null,
    };
});

await app.listen({ port: PORT, host: "127.0.0.1" });
console.log(`listening on :${PORT}`);
