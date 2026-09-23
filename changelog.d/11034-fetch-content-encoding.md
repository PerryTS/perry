Fixed global `fetch()` Content-Encoding handling to match Node (undici) (#10475).

- Requests now carry undici's default `Accept-Encoding` — `gzip, deflate` over
  `http:`, `br, gzip, deflate, zstd` over `https:`, recomputed per redirect hop —
  unless the caller set one; a `Range` request appends `identity`. Previously
  Perry sent none, so servers that negotiate compression never compressed for
  it, which hid the decoding gaps below.
- Response decoding follows undici exactly: the header is lower-cased, split on
  `,` and decoded in reverse, so stacked codings (`deflate, gzip`) now decode
  instead of being delivered still compressed; more than five codings rejects;
  a coding undici does not know (including `identity`) delivers the body as
  received. `Content-Encoding` / `Content-Length` stay observable.
- A gzip header or trailer (or deflate's 2-byte zlib sniff) split across two
  body chunks no longer corrupts the body. `StreamingDecoder::process` leaves
  such input unconsumed for the caller to retain; the engine dropped it.

Ported from #11034, which patched the reqwest transport that #11101 removed.
On current `main` the turnloop client engine (`turnloop_client/exchange.rs`)
already decoded a single gzip/deflate/br/zstd coding, so the port is the
missing default header, the multi-coding chain and the carried tail
(`turnloop_client/content_decoding.rs`). The codecs are `turnloop-http`'s own —
flate2, brotli and zstd are unconditional dependencies of that crate — so
decoding `br` needs no perry-stdlib feature and is unaffected by Brotli being
feature-gated for the `zlib` / WHATWG-streams codecs. The gap test
`test_gap_10475_fetch_content_encoding` now runs its server in-process on an
ephemeral port (the original read `process.env.PORT` and expected an external
server, which is why it failed parity in CI).
