### Fixed

- A number is always a Number. Web Streams handles (ReadableStream,
  WritableStream, TransformStream, readers and writers) used to travel as
  plain numbers in `[0x100000, 0x200000)`, so while stream id 0x100000 was
  live the number 1048576 answered `typeof` "object", passed
  `instanceof ReadableStream`, failed `typeof x === "number"`, and could be
  dispatched as a stream. OpenCode's catalog Schema rejected a model record
  with `limit.context: 1048576` ("Expected number, got {"[ownKeys threw]"}").
  Stream handles are now `POINTER_TAG` registry handles like every other
  native handle: the stream id band is the top of the handle band
  (`HANDLE_BAND_MAX` and the emitted receiver floor `RECEIVER_HANDLE_FLOOR`
  move from 1 MiB to 2 MiB), the stdlib encodes and decodes stream handles in
  one place (`streams::stream_value` / `streams::stream_id`), and every
  runtime classifier (typeof, the inline `typeof x === "number"` test,
  instanceof, `Object.prototype.toString`, property get/set, `in`, method
  calls, WeakMap keys, iteration) recognises a stream by its tag, never by a
  number's value.
