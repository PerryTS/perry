Fix stale addresses in native stream code when user code collects mid-call:
the `node:stream/promises` pipeline (direct and stream-list paths, chunks,
receivers, method getters, the source's `_read`, `promiseHooks` init hooks),
a piped readable's `unpipe()` and end fan-out, and `Promise.reject`-style
rejections created while an init hook collects.
