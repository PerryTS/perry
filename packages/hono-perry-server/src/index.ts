/**
 * `@hono/perry-server` — Hono's standard `serve({ fetch, port })` adapter
 * contract on Perry-native, wrapping Perry's bundled Fastify.
 *
 * Hono is runtime-agnostic: the same `(request: Request) => Response` handler
 * runs on Workers / Bun / Deno / Node — and now Perry. This adapter is the
 * Perry equivalent of `@hono/node-server` / `@hono/bun`: one well-tested
 * place that absorbs the Perry-specific quirks instead of every app vendoring
 * its own shim.
 *
 * Usage:
 * ```ts
 * import { Hono } from 'hono'
 * import { serve } from '@hono/perry-server'
 *
 * const app = new Hono()
 * app.get('/', (c) => c.json({ ok: true }))
 *
 * serve({ fetch: app.fetch, port: 3000 }, (info) => {
 *   console.log(`listening on :${info.port}`)
 * })
 * ```
 *
 * Requires Perry ≥ 0.5.1027 (#1649 — `Request.headers` is a real `Headers`
 * object; the adapter's hot path reads `c.req.headers.get(...)` inside Hono).
 */
import fastify from 'fastify'

/** A Hono-style fetch handler: `app.fetch`. */
export type FetchHandler = (request: Request) => Response | Promise<Response>

export interface ServeOptions {
  /** The app's fetch handler, e.g. `app.fetch` from `new Hono()`. */
  fetch: FetchHandler
  /** TCP port to listen on. */
  port: number
  /** Bind address. Defaults to `0.0.0.0`. */
  hostname?: string
}

export interface ServeInfo {
  port: number
  address: string
  family: 'IPv4' | 'IPv6'
}

/**
 * Start a Fastify server that delegates every request to `opts.fetch`.
 *
 * A single catch-all route translates the Fastify request into a Web
 * `Request`, awaits the app's `fetch`, and copies the resulting `Response`'s
 * status / headers / body onto the Fastify reply.
 */
export function serve(opts: ServeOptions, listener?: (info: ServeInfo) => void): void {
  const app = fastify({ logger: false })

  app.all('/*', async (req: any, reply: any) => {
    const host = req.headers.host ?? `localhost:${opts.port}`
    const url = `http://${host}${req.url}`

    const headers = new Headers()
    for (const [key, value] of Object.entries(req.headers)) {
      if (Array.isArray(value)) {
        for (const v of value) headers.append(key, String(v))
      } else if (value !== undefined && value !== null) {
        headers.set(key, String(value))
      }
    }

    const method = req.method ?? 'GET'
    const hasBody = method !== 'GET' && method !== 'HEAD'
    const body = hasBody
      ? typeof req.body === 'string'
        ? req.body
        : req.body != null
          ? JSON.stringify(req.body)
          : undefined
      : undefined

    const fetchReq = new Request(url, { method, headers, body })
    const res = await opts.fetch(fetchReq)

    res.headers.forEach((value: string, key: string) => {
      // Fastify sets content-length from the body it sends; forwarding the
      // upstream value double-counts and can truncate the response.
      if (key !== 'content-length') reply.header(key, value)
    })
    reply.code(res.status)
    // #1650 will let us stream `res.body` straight through; until the streaming
    // path is wired everywhere we buffer with `res.text()`, which is correct
    // for the JSON / HTML responses Hono apps return today.
    return reply.send(await res.text())
  })

  const host = opts.hostname ?? '0.0.0.0'
  app.listen({ port: opts.port, host }, (err: any, address: string) => {
    if (err) throw err
    listener?.({
      port: opts.port,
      address,
      // Classify off the bound host, not `address` — the latter is a URL like
      // `http://0.0.0.0:18992` whose port colon would always read as IPv6.
      family: host.includes(':') ? 'IPv6' : 'IPv4',
    })
  })
}

export default serve
