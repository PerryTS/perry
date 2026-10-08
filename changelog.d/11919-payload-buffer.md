- **runtime/ffi: `PayloadBuffer`, working buffers for native payloads; zlib adopts it (#11919).**
  A native payload's codec working memory (windows, hash tables, output
  scratch) now has one allocator: `PayloadBuffer::alloc(owner, len) -> (ptr,
  len)`, `grow`, `shrink`, `release`
  (`crates/perry-runtime/src/native_payload_buffer.rs`; perry-ffi
  `native_payload::buffer` for binding crates). Blocks are raw, zeroed and
  16-byte aligned, and they are never traced. The payload's
  `PayloadBufferOwner` counts the exact bytes of its live buffers, and the
  family adds them into the external bytes it already states (`alloc`,
  `set_external_bytes`, `StepOut::external_bytes`). `close` / `destroy()`
  drops the payload at once, so the bytes go back before any collection. The
  sweep's drop is the backstop for a payload that is never closed. A C-ABI
  hook pair (`alloc(opaque, size)` / `free(opaque, ptr)`, the shape of
  brotli's `CAllocator`) serves codecs that take a custom allocator. Every
  byte comes from one internal function, `backing`. It uses the global
  allocator (mimalloc) today, and the region page-run allocator will replace
  that one function.

  `perry-ext-zlib` now keeps these in payload buffers: brotli's decoder and
  encoder state plus every brotli allocation (through the hook), the inflate
  state with its 32 KiB window (inflate, inflateRaw, gunzip, unzip), and the
  output scratch of all eleven codecs. The brotli counting allocator and the
  fixed inflate estimate are gone, because the owner's exact count replaces
  both. Not routed: the deflate compressor's dictionary, hash chains and
  Huffman tables, which miniz_oxide boxes internally with no allocator hook
  (their stated size is unchanged), and zstd, which allocates in C. zstd's
  `ZSTD_customMem` needs zstd-sys's experimental API, and zstd keeps
  reporting `sizeof`.

  If the backing runs out of memory, codec construction reports
  `ErrorKind::OutOfMemory`. The brotli decoder fails the stream with its
  `BROTLI_DECODER_ERROR_ALLOC_*` code, and the encoder aborts, as it does on
  its stock allocator. The inflate state is built in place in its zeroed
  buffer, with no 43 KB stack copy. Brotli's zero-fill shortcut is a
  type-level `HookFill` marker, and every brotli block holds its owner.

  Tests: a runtime witness checks that close returns buffer bytes and
  external bytes with no collection in between. A child-process sabotage
  (`release_on_drop_only`) turns that witness red. There is also a sweep
  backstop test and a 16k-cycle churn test against an empty-buffer control.
  In zlib, every buffered codec is destroyed mid-stream and returns its
  buffers before any collection (red under `release_in_finalizer_only`), and
  a mid-stream destroy churn keeps buffer bytes and RSS flat.

  Measured on qb6 with the default auto build, n=5 interleaved vs main
  (instructions:u medians): buffer_heavy +0.001%, fastify +0.05% (inside
  its ±0.1% run spread, and fastify uses no zlib), tsc +0.006%, Zod 5k
  +0.011%, and a zlib/brotli stream micro −0.011%. RSS and full-collection
  counts are unchanged (buffer_heavy has 36 full collections in both arms).
  Output matches node on every program. In the zlib gap subset (34 tests),
  head and main produce byte-identical output with 0 regressions.
