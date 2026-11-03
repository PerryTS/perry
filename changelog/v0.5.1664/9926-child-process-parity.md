### Fixed

- Match Node's `child_process` metadata and synchronous buffer-limit behavior by
  preserving the caller's `execFile` spelling and signaling a process when its
  output crosses `maxBuffer` only while it is still running.
