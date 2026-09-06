### Fixed

- Callback-form `stream.finished()` now waits for both sides of a duplex stream,
  so ending an unread `PassThrough` does not report completion before its
  readable side emits `end`.
