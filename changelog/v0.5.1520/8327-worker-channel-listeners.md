### Fixed

- **Worker channel listeners now preserve Node registration semantics (#6763).**
  `MessagePort` keeps distinct listeners in order, deduplicates repeated
  registrations, removes only the requested callback, honors `once`, reports
  listener counts, and passes the close event to close listeners.
  `MessagePort` and `BroadcastChannel` EventTarget listeners also honor the
  `{ once: true }` option, with callback snapshots rooted across moving GC.
