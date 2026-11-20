### Fixed

- `Readable.toWeb()` now streams event-backed sources such as `fs.ReadStream`
  instead of closing with an empty body. File read errors propagate to the web
  stream, backpressure stays bounded, and cancellation destroys the source.
