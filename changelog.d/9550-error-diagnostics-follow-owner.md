### Fixed

- Preserve Node-style Error diagnostics (`code`, `syscall`, `errno`, `path`,
  `dest`, and `hostname`) when a minor GC relocates the Error or its message.
  The fields now share one record keyed by the owning ErrorHeader, so the
  Error's existing move and finalize hooks keep the record in sync.
