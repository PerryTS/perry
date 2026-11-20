Fixed Web Fetch bodies built from async iterables. `new Request(url, { body:
asyncGenerator(), duplex: "half" })` and `new Response(asyncGenerator())`
stringified the body to `"[object AsyncGenerator]"`, and `fetch(url, { body:
asyncGenerator() })` sent an empty body. Astro's `@astrojs/node` adapter passes
every POST body this way, so `request.json()` threw `SyntaxError`.

The BodyInit classifier (`js_response_body_init_ptr`) now stashes an async
iterable in a GC-scanned pending slot, and `take_pending_fetch_body_stream_id`
converts it with `js_readable_stream_from_iterable`, so `Request`, `Request`
copies, `Response` and `fetch()` all pick it up. The conversion runs the
iterator's JS, so those consumers copy or root their raw arguments first, and
`ReadableStream.from` now keeps the iterator and collected chunks in runtime
handles. A `Request` body whose iterable throws rejects `text()`/`json()`/…
with the original error. String chunks are UTF-8 encoded in the Fetch body
drain.

Tests: `test-parity/node-suite/fetch/request/async-iterable-body.ts`,
`async-iterable-body-send.ts`, `mutated-init-local.ts`.
