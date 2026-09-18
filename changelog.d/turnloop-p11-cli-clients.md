**turnloop P11 — the `perry` CLI, `axios` and `node-fetch` leave tokio.**
`python3 scripts/tokio_inventory.py` goes from **46 manifest edges across 16
workspace crates to 39 across 13**, the first lane to move P8's count.

- **New `perry-http-client`**: a blocking HTTP/1.1 and WebSocket client on a
  `turnloop::Loop` it owns and turns to completion. It is for callers with no JS
  event loop to cooperate with — the CLI, and the `perry-ext-*` bindings that
  already run on a blocking-pool thread — which is why it is a fraction of the
  size of P5/P6/P7's completion-driven engines. It carries the
  `multipart/form-data` builder `turnloop-http`'s client does not have (P8 named
  that as the blocker for the CLI's uploads); the builder verifies its boundary
  is absent from every part rather than trusting entropy, because three of the
  four callers upload base64.
- **New `perry-tls-session`**: P6's outbound TLS client state machine extracted
  out of perry-stdlib so there is one copy rather than the third one this lane
  would have made. perry-stdlib keeps the `perry_ffi`-shaped `client_config()`.
- **The `perry` CLI**: 13 `reqwest::Client` constructions, 7
  `tokio::runtime::Runtime::new` sites and 2 `tokio-tungstenite` clients are
  gone, and every `async fn` that existed to be driven by one of them is a plain
  `fn`. Peak OS threads during one `perry verify` drop from **66 to 2**, and the
  binary loses 1.79 MB. `login`, `audit`, `verify`, `publish` (multipart upload,
  WebSocket progress and artifact download) and `update --check-only` produce
  byte-identical output on both arms.
- **`perry-ext-fetch` is deleted** and `node-fetch` routes to perry-stdlib's
  WHATWG fetch. The wrapper defined the same 74 `js_fetch_*` / `js_headers_*` /
  `js_response_*` / `js_request_*` / `js_blob_*` / `js_form_data_*` symbols
  perry-stdlib owns, as a strict subset, and the two disagreed on handle
  encoding — perry-stdlib NaN-boxes out of the fetch band, the wrapper returned
  a bare double counting from 1. A program importing `node-fetch` and calling
  the global `fetch()` linked both archives and **SIGSEGV'd**; `nm` on the base
  binary shows the two implementations nine megabytes apart, handing each other
  handles neither can read. `import fetch from 'node-fetch'` now answers
  `status = 200` where it crashed 3/3. node-fetch also gains `AbortSignal`
  (#10325), `Content-Encoding` decoding, pooling and P6's turnloop transport.
- **`perry-ext-axios`** keeps all eleven symbols and its handle encoding and
  swaps reqwest for `perry-http-client`, closing **#10326**: it built a fresh
  ~250 KB `reqwest::Client`, with cold DNS and TLS caches, on every call.
  Eleven assertions across seven methods are byte-identical on both arms.

The full gap suite ran on both arms from source in their own trees: **818 tests,
809 pass, the same nine known parity failures, 0 compile_fail, 0 crashes, and
zero status changes compared per test**.

Full report, including the `nm` evidence, the gap-suite comparison and the
turnloop gaps: `docs/turnloop/p11-report.md`.
