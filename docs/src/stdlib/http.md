# HTTP & Networking

Perry natively implements HTTP servers, clients, and WebSocket support.

## Node.js compatibility — `node:http` / `node:https` / `node:http2`

Perry exposes a faithful subset of Node.js's stdlib HTTP server modules
on top of hyper + rustls + tokio-tungstenite. The whole shape — handler
signature, IncomingMessage / ServerResponse properties + methods,
TLS opts, ALPN-negotiated HTTP/2, WebSocket upgrade dispatch — works
unmodified, so unmodified Node servers (Express / Koa / Polka / hono via
`@hono/node-server` / etc.) compile and run natively (issue #577).

### `http.createServer(handler)`

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:node-http-server}}
```

Supported on `IncomingMessage`: `.method`, `.url`, `.headers`,
`.rawHeaders`, `.httpVersion`, `.complete`, `.aborted`, `.destroyed`,
`.socket.remoteAddress`, `.socket.remotePort`, `.on('data'|'end'|'close'|
'error', cb)`, `.read()`, `.pause()`, `.resume()`, `.destroy()`.

Supported on `ServerResponse`: `.statusCode` (get/set),
`.statusMessage` (set), `.setHeader/.getHeader/.removeHeader/.hasHeader/
.getHeaders/.getHeaderNames`, `.headersSent`, `.writableEnded`,
`.writableFinished`, `.writeHead(status, msg?, headers?)`,
`.write(chunk)`, `.end(chunk?)`, `.flushHeaders()`,
`.on('finish'|'close', cb)`. Auto Content-Length on `.end()` when no
`Transfer-Encoding` was set.

### `https.createServer({ key, cert }, handler)`

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:node-https-server}}
```

Both `key` and `cert` are PEM strings (PKCS#8 / RSA / EC keys + multi-cert
chains all parse). ALPN defaults to `http/1.1` only — programs that want
HTTP/2 should reach for `node:http2`'s `createSecureServer` (which always
advertises `[h2, http/1.1]`).

### `http2.createSecureServer({ key, cert }, handler)`

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:node-http2-server}}
```

Driven through `hyper-util`'s `auto::Builder`, so an HTTP/1.1 client
(curl without `--http2`) and an HTTP/2 client (curl with `--http2`)
hit the same handler over the same port.

### WebSocket upgrade — `Server.on('upgrade', (req, wsId, head) => …)`

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:node-http-ws-upgrade}}
```

The HTTP/1.1 server detects `Upgrade: websocket` in the request,
performs the handshake server-side (Sec-WebSocket-Accept derived via
tungstenite's `derive_accept_key`), then registers the upgraded stream
in perry-ext-ws's connection map. The TS-side `wsId` argument is
already a fully-connected client — drive it via the standard
`wsId.on('message', cb)` / `wsId.send(msg)` / `wsId.close()` surface
that standalone `WebSocketServer({ port })` clients use.

## Hono

[Hono](https://hono.dev/) is a runtime-agnostic web framework whose only
required interface is `app.fetch(req: Request) → Promise<Response>`. Add
it to `perry.compilePackages` and the entire `app.fetch` surface
including middleware (`hono/logger`, `hono/cors`, `hono/jwt`), route
groups, and JSON responses works unchanged (issues #421, #486, #487
closed). `app.fetch` is enough for testing, edge-runtime deployments
(Cloudflare Workers / Vercel Edge / AWS Lambda / Deno Deploy — those
runtimes call `app.fetch` themselves), and any scenario where some
outer host hands you a `Request`.

```typescript,no-test
import { Hono } from "hono"
import { logger } from "hono/logger"

const app = new Hono()
app.use("*", logger())
app.get("/", (c) => c.json({ message: "hello", ok: true }))

// app.fetch() works end-to-end — feed it a Request, get a Response.
const res = await app.fetch(new Request("http://localhost/"))
console.log(res.status, await res.text())

export default app  // for CF Workers / similar runtimes
```

`package.json`:

```json
{
  "perry": {
    "compilePackages": ["hono"]
  }
}
```

### Long-lived HTTP server (port-listening) — currently blocked

The canonical "deploy a hono app as a native binary on a Linux VM"
pattern — `serve({ fetch: app.fetch, port: 3000 })` via
`@hono/node-server`, or a hand-rolled `node:http` adapter that drives
`app.fetch` — currently fails to link because the Web Fetch FFIs
(`Headers` / `Response` constructors) aren't pulled in alongside
perry-ext-http-server. Tracked at [issue #589](https://github.com/PerryTS/perry/issues/589).

Workaround until #589 lands: deploy as an edge-runtime worker (CF
Workers / Vercel Edge), or use [perry's Fastify binding](#fastify-server)
with a single catch-all route delegating to `app.fetch`.

## Fastify Server

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:fastify-server}}
```

Perry's Fastify implementation is API-compatible with the npm package. Routes, request/reply objects, params, query strings, and JSON body parsing all work.

## Fetch API

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:fetch-api}}
```

## Axios

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:axios-client}}
```

## WebSocket

```typescript,no-test
{{#include ../../examples/stdlib/http/snippets.ts:websocket-client}}
```

## AWS S3 / S3-Compatible Object Storage

[`@bradenmacdonald/s3-lite-client`](https://github.com/bradenmacdonald/s3-lite-client) is a zero-dependency, MIT-licensed S3 client (~1.9k LoC, derived from the official MinIO JS client without the lodash/async/xml2js baggage). It compiles natively under `perry.compilePackages` with no patches required — verified against a SigV4 presigned-URL byte-for-byte match with `bun` (issue #551).

```json
{
  "perry": {
    "compilePackages": ["@bradenmacdonald/s3-lite-client"]
  }
}
```

```typescript,no-test
import { S3Client } from "@bradenmacdonald/s3-lite-client"

const s3 = new S3Client({
    endPoint: "https://s3.us-east-1.amazonaws.com",
    region: "us-east-1",
    bucket: "my-bucket",
    accessKey: process.env.AWS_ACCESS_KEY_ID,
    secretKey: process.env.AWS_SECRET_ACCESS_KEY,
})

// Presigned GET URL (no network I/O — pure SigV4 signing)
const url = await s3.presignedGetObject("path/to/object.png", { expirySeconds: 3600 })
console.log(url)

// Upload bytes
await s3.putObject("path/to/object.txt", "hello world", {
    metadata: { "x-amz-acl": "public-read" },
})

// Stream a download — returns a standard fetch Response
const res = await s3.getObject("path/to/object.txt")
console.log(await res.text())

// Head / Delete / List
const meta = await s3.statObject("path/to/object.txt")
console.log(meta.size, meta.lastModified)

for await (const obj of s3.listObjects({ prefix: "path/to/" })) {
    console.log(obj.key, obj.size)
}

await s3.deleteObject("path/to/object.txt")
```

Same code works against any S3-compatible service — only `endPoint` changes:

| Service | `endPoint` |
|---------|-----------|
| AWS S3 | `https://s3.<region>.amazonaws.com` |
| Cloudflare R2 | `https://<account>.r2.cloudflarestorage.com` |
| MinIO | `http://localhost:9000` |
| Backblaze B2 | `https://s3.<region>.backblazeb2.com` |
| DigitalOcean Spaces | `https://<region>.digitaloceanspaces.com` |
| Supabase Storage | `https://<project>.supabase.co/storage/v1/s3` |
| LocalStack (testing) | `http://localhost:4566` |

The full SigV4 signing chain (Web Crypto HMAC-SHA-256 + SHA-256, TextEncoder, URLSearchParams, Headers iteration, typed-array byte marshalling) is exercised end-to-end. Read paths (`getObject`, `statObject`, `deleteObject`, `listObjects`, `presignedGetObject`, `presignedPostObject`) are verified byte-identical to `bun` against pinned test vectors and will authenticate against real S3.

Multipart uploads (`putObject` with a `ReadableStream` source large enough to chunk) exercise additional surface — `WritableStream` / `TransformStream` subclassing per #562 — that path compiles but isn't independently verified against pinned vectors here.

For the AWS SDK v3 (`@aws-sdk/client-s3`): Perry currently can't compile it. Its dependency tree pulls in `@smithy/*` and runtime middleware registration that uses `Proxy` and dynamic property assignment, neither of which is in Perry's [TypeScript subset](../language/limitations.md). `@bradenmacdonald/s3-lite-client` covers the same surface (Put/Get/Head/Delete/List/presign + multipart) for almost every real-world need.

## Next Steps

- [Databases](database.md)
- [Overview](overview.md) — All stdlib modules
