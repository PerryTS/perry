### Fixed

- Enforce `maxOutputLength` for synchronous and callback-based Brotli one-shot
  decompression through named, namespace, and captured calls. Synchronous calls
  now forward their options consistently and decode with a fixed-size scratch
  buffer plus bounded EOF lookahead, accepting output exactly at the cap without
  appending bytes beyond it.
- Guard deferred Brotli output before accumulation and report oversized results
  as a managed `RangeError` with `ERR_BUFFER_TOO_LARGE` and no callback output.
  Preserve the genuine codec receiver, existing deferred completion schedule,
  and release-once cleanup before invoking the callback. Ordinary uncapped
  output, compressed input bytes, compression backend, and quality are unchanged.
