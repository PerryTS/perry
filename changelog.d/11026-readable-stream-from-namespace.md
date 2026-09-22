### Fixed

- `ReadableStream.from()` now works through a `node:stream/web` namespace
  import, including TypeScript-cast forms such as
  `(streamWeb.ReadableStream as any).from(items)`. The returned stream and
  reader retain their native types, so `read()` yields `{ done, value }`
  objects and iterable drain loops terminate.
