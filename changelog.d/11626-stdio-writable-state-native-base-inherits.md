### Fixed

- **`process.stdout` / `process.stderr` Writable buffer state (#11418).** Both
  stdio shapes (TTY write stream and pipe/file) now carry `writableLength` (`0`),
  `writableHighWaterMark` (`65536`) and `writableNeedDrain` (`false`). The
  `write` stubs are synchronous, so nothing is ever queued. A "flush stdio,
  then `process.exit()`" helper that waits for `'drain'` unless
  `writableLength === 0` used to wait out its whole cap (2 s per CLI command)
  because all three read `undefined`. The dead `process_stream_set_encoding_stub`
  (only reachable from an `if is_stdin` inside an `if is_stdin`) is removed to
  keep `os_process_streams.rs` under the file cap.
- **`http.ServerResponse` / `stream.Readable` subclasses built without ES
  `class` syntax (#10454)** — light-my-request's (fastify `inject()`) shape:
  - `http.ServerResponse.prototype` now carries the response methods
    (`setHeader`, `getHeader`, `writeHead`, `write`, `end`, `assignSocket`, …),
    installed by a new `nm_attach_http` hook. Each forwards `this` to its
    native handle: either the receiver itself, or the handle it is aliased to.
    On a receiver with neither it throws a `TypeError`, as Node does.
  - `ServerResponse.call(this, req)` / `.apply(this, [req])` and `super(req)`
    (every heritage shape, via `js_fetch_or_value_super`) now construct the
    handle through the http dispatcher and alias `this` to it. Before this, the
    plain call built a handle and dropped it. `ServerResponse` aliases dispatch
    through the composite handle dispatcher, because perry-ext-http's extension
    owns those handles. The `http.Server` aliases stay primary-only (#4973).
  - `Readable/Writable/Duplex/Transform/PassThrough/Stream.call(this, opts)`
    runs the same subclass-init shim `super()` uses on the explicit `this`
    (shared `run_node_stream_subclass_init`). Before this, the plain call built
    a fresh stream and discarded it.
  - A class instance whose chain ends at a bound native-module export
    (`class Sub extends http.ServerResponse`) now continues named reads on that
    export's `.prototype` (`bound_native_parent_prototype_field`). Before this,
    `typeof sub.setHeader` was `undefined`, even for methods a user added to
    `ServerResponse.prototype`, while `Object.getPrototypeOf` reached them.
  - Known limit: the alias table is the #4973 linear, strongly-rooted `Vec`, so
    each aliased `ServerResponse` stays alive for the life of the thread.
- Regression coverage for #10449 (PassThrough/Transform ended early across
  ticks) and #10544 (chained `fs.createReadStream` stopped after 512 streams).
  Neither reproduces on current `main`. The new gap tests pin the issue shapes
  plus write-stream chains and the mongodb framer.

Tests: `test_gap_process_stdio_writable_state`,
`test_gap_10454_inherits_native_base`,
`test_gap_10449_transform_write_across_ticks`,
`test_gap_10544_chained_read_streams`.
