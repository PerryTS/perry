// demonstrates: per-API HTTP/networking snippets shown in docs/src/stdlib/http.md
// docs: docs/src/stdlib/http.md
// platforms: macos, linux, windows
// run: false

// Each ANCHOR block below is the exact code that the http docs page renders
// inline (via {{#include ... :NAME}}). The whole file is compiled and linked
// by the doc-tests harness, so every snippet is a tested artifact — if any
// snippet drifts from the real Fastify / fetch / axios / ws API, CI fails.
//
// `run: false` because every snippet either binds a port (`server.listen`),
// hits a remote URL (axios / fetch), or opens a WebSocket — none of which
// is hermetic in CI. Compile + link is the contract here, and that catches
// the API-shape regressions we care about (e.g. the issue #125 Fastify
// async-handler regression that the sibling `fastify_json.ts` covers).

// ANCHOR: fastify-server
import fastify from "fastify"

const app = fastify()

app.get("/", async (request: any, reply: any) => {
    return { hello: "world" }
})

app.get("/users/:id", async (request: any, reply: any) => {
    const id = request.params.id
    return { id, name: "User " + id }
})

app.post("/data", async (request: any, reply: any) => {
    const body = request.body
    reply.code(201)
    return { received: body }
})

app.listen({ port: 3000 }, () => {
    console.log("Server running on port 3000")
})
// ANCHOR_END: fastify-server

// ANCHOR: fetch-api
async function fetchExamples(): Promise<void> {
    // GET request
    const response = await fetch("https://jsonplaceholder.typicode.com/posts/1")
    const data = await response.json()

    // POST request
    const result = await fetch("https://jsonplaceholder.typicode.com/posts", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ title: "hello", body: "world", userId: 1 }),
    })

    console.log(`fetch ok: ${data !== null} status=${result.status}`)
}
// ANCHOR_END: fetch-api

// ANCHOR: axios-client
import axios from "axios"

async function axiosExamples(): Promise<void> {
    const getResp = await axios.get("https://jsonplaceholder.typicode.com/users/1")
    const data = getResp.data

    const response = await axios.post("https://jsonplaceholder.typicode.com/users", {
        name: "Perry",
        email: "perry@example.com",
    })

    console.log(`axios ok: ${data !== null} status=${response.status}`)
}
// ANCHOR_END: axios-client

// ANCHOR: websocket-client
import { WebSocket } from "ws"

function wsExample(): void {
    const ws = new WebSocket("ws://localhost:8080")

    ws.on("open", () => {
        ws.send("Hello, server!")
    })

    ws.on("message", (data: any) => {
        console.log(`Received: ${data}`)
    })

    ws.on("close", () => {
        console.log("Connection closed")
    })
}
// ANCHOR_END: websocket-client

// ANCHOR: node-http-server
// node:http server (issue #577). Drop-in for Node.js's `http.createServer`
// — same handler shape `(req, res) => …` and same property/method
// surface (`req.method`, `req.url`, `req.headers`, `res.statusCode`,
// `res.setHeader`, `res.end`, `res.write`, `res.writeHead`). The
// canonical Express body-collection pattern (`req.on('data', ...)`,
// `req.on('end', ...)`) works against a fully-buffered request body.
import { createServer } from "node:http"

const httpServer = createServer((req: any, res: any) => {
    if (req.method === "POST" && req.url === "/echo") {
        let chunks: string[] = []
        req.on("data", (chunk: string) => chunks.push(chunk))
        req.on("end", () => {
            const body = chunks.join("")
            res.statusCode = 200
            res.setHeader("Content-Type", "text/plain")
            res.end("got:" + body)
        })
        return
    }
    res.statusCode = 200
    res.setHeader("Content-Type", "application/json")
    res.end(`{"path":"${req.url}"}`)
})

httpServer.listen(3000, () => {
    console.log("[node:http] listening on http://0.0.0.0:3000")
})
// ANCHOR_END: node-http-server

// ANCHOR: node-https-server
// node:https server (issue #577 Phase 2). Same handler surface as
// `node:http`, plus a `{ key, cert }` opts arg with PEM-encoded TLS
// material. rustls 0.23 underneath; the CryptoProvider is installed
// lazily on first `https.createServer` call. ALPN defaults to
// `http/1.1`; opt into HTTP/2 by passing
// `alpnProtocols: ["h2", "http/1.1"]` (or use `node:http2` directly).
import { createServer as createTlsServer } from "node:https"
import { readFileSync } from "node:fs"

const tlsServer = createTlsServer(
    {
        key: readFileSync("/tmp/perry-https-cert/key.pem", "utf8"),
        cert: readFileSync("/tmp/perry-https-cert/cert.pem", "utf8"),
    },
    (req: any, res: any) => {
        res.statusCode = 200
        res.setHeader("Content-Type", "application/json")
        res.end(`{"tls":"ok","path":"${req.url}"}`)
    }
)

tlsServer.listen(443)
// ANCHOR_END: node-https-server

// ANCHOR: node-http2-server
// node:http2 server (issue #577 Phase 3). `createSecureServer({ key, cert })`
// drives a hyper-util auto::Builder so HTTP/2 and HTTP/1.1 share a
// single port via ALPN auto-negotiation. The handler signature is
// the same as Phase 1 / Phase 2 — IncomingMessage / ServerResponse
// are reused as Http2ServerRequest / Http2ServerResponse since each
// `:path` request becomes a single buffered IncomingMessage.
import { createSecureServer } from "node:http2"

const h2Server = createSecureServer(
    {
        key: readFileSync("/tmp/perry-https-cert/key.pem", "utf8"),
        cert: readFileSync("/tmp/perry-https-cert/cert.pem", "utf8"),
    },
    (req: any, res: any) => {
        res.statusCode = 200
        res.setHeader("Content-Type", "application/json")
        res.end(`{"h2":"ok","path":"${req.url}","httpVersion":"${req.httpVersion}"}`)
    }
)

h2Server.listen(8443)
// ANCHOR_END: node-http2-server

// ANCHOR: node-http-ws-upgrade
// node:http + WebSocket upgrade (issue #577 Phase 4). The `'upgrade'`
// event fires once per WebSocket client; the `wsId` argument is
// already a fully-handshaked, perry-ext-ws-registered connection,
// so the usual `wsId.on('message', ...)` / `wsId.send(...)` /
// `wsId.close()` surface works without further plumbing. The
// IncomingMessage `req` carries the original upgrade request
// (URL, headers — useful for routing or auth).
const wsHttpServer = createServer((req: any, res: any) => {
    res.statusCode = 200
    res.end("perry node:http server with ws upgrade")
})

wsHttpServer.on("upgrade", (req: any, wsId: any, _head: any) => {
    wsId.on("message", (msg: string) => {
        wsId.send("echo:" + msg)
    })
    wsId.send("perry-hello")
})

wsHttpServer.listen(3001)
// ANCHOR_END: node-http-ws-upgrade

// Reference everything so unused-import elimination doesn't strip it.
const _keep = [
    fetchExamples,
    axiosExamples,
    wsExample,
    httpServer,
    tlsServer,
    h2Server,
    wsHttpServer,
]
console.log(`http-snippets: ${_keep.length}`)
