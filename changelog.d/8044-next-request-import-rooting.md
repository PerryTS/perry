### Fixed

- Preserve request objects and their nested URL, search-parameter, header, and
  body state when generated route modules re-export imported handlers. Imported
  handler closures and complete expired-timer batches now remain rooted across
  allocation, promise/timer continuations, microtask checkpoints, and moving GC.
