// demonstrates: Fastify route returning a JS object — the exact reproducer
// from issue #125, where curl POSTing against this endpoint returned memory
// garbage before the fix in c5f264e. This example is `run: false` because
// `server.listen()` blocks forever; the doc-tests harness only verifies it
// compiles + links, which catches API/TS drift in the Fastify surface (the
// `post(path, handler)`/object-return contract that #125 broke).
//
// Runtime correctness is now covered by perry-stdlib's own unit tests after
// the async-handler fix. A full integration test (start server, self-fetch)
// needs 2-process orchestration in the harness; tracked as a follow-up.
//
// docs: docs/src/stdlib/http.md
// platforms: macos, linux, windows
// requires: auto-optimize
// run: false

import Fastify from "fastify"

const server = Fastify({ logger: false })

server.post("/gonderi-yap", async () => {
    // Returning a plain object is the exact code path from #125.
    return { status: "Success", message: "Data received" }
})

server.get("/status", async () => {
    // A GET variant with a different object shape keeps the TS-side
    // contract honest against future refactors.
    return { status: "ok", uptime: Date.now(), counters: { a: 1, b: 2 } }
})

await server.listen({ port: 3000, host: "0.0.0.0" })
