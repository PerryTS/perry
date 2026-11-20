### Added

- **`child_process.fork()` IPC on Windows (#6619).** Unix carried the IPC
  channel on an inherited socketpair fd, which has no Windows equivalent, so
  `fork()` there had no channel at all. Windows now gets a named-pipe transport
  (`child_process/ipc_transport.rs`) and its own spawn path
  (`child_process/windows_fork.rs`), both `#[cfg(windows)]` at the module
  declaration.

  The shared reactor is refactored rather than forked: the concrete
  `Child` / `ChildStdin` types become boxed `CpReader` / `CpWriter` / `CpWaiter`
  trait objects so one event loop serves both platforms. Unix semantics are
  preserved through that change — the `pid` field is still published, and exit
  reporting still extracts `status.signal()` via `ExitStatusExt` on each path
  rather than losing the signal to a generic waiter.
