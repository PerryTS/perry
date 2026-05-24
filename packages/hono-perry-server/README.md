# @hono/perry-server

Hono's standard `serve({ fetch, port })` adapter contract for **Perry-native**
compile, wrapping Perry's bundled [Fastify](https://www.fastify.io/).

Hono is runtime-agnostic — the same `(request: Request) => Response` handler
runs on Workers, Bun, Deno, Node, and now Perry. This package is the Perry
counterpart to `@hono/node-server` / `@hono/bun`: one versioned, well-tested
place that absorbs the Perry-specific quirks instead of every app vendoring
its own shim.

## Usage

```ts
import { Hono } from 'hono'
import { serve } from '@hono/perry-server'

const app = new Hono()
app.get('/', (c) => c.json({ ok: true }))

serve({ fetch: app.fetch, port: 3000 }, (info) => {
  console.log(`listening on :${info.port}`)
})
```

Compile and run as a native binary:

```sh
perry compile src/server.ts -o server && ./server
```

## API

```ts
function serve(opts: ServeOptions, listener?: (info: ServeInfo) => void): void

interface ServeOptions {
  fetch: (request: Request) => Response | Promise<Response> // e.g. app.fetch
  port: number
  hostname?: string // default '0.0.0.0'
}

interface ServeInfo {
  port: number
  address: string
  family: 'IPv4' | 'IPv6'
}
```

## Requirements

- Perry ≥ 0.5.1027 — relies on `Request.headers` being a real `Headers`
  object ([#1649](https://github.com/PerryTS/perry/issues/1649)); inside Hono,
  `c.req.headers.get(...)` runs on the adapter's hot path.

## How it works

A single Fastify catch-all route (`app.all('/*', …)`) translates each Fastify
request into a Web `Request`, awaits `opts.fetch`, then copies the resulting
`Response`'s status / headers / body onto the Fastify reply. Bodies are
buffered via `res.text()`; once response-body streaming is wired end-to-end
([#1650](https://github.com/PerryTS/perry/issues/1650)) the adapter can stream
`res.body` straight through.

Tracked at [#1654](https://github.com/PerryTS/perry/issues/1654).
