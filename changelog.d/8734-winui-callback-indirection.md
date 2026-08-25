### Fixed

- Fixed WinUI widget, application-exit, and timer callbacks becoming stale after an evacuating garbage collection. Windows Reactor closures now retain stable widget/slot keys and resolve callbacks from GC-scanned storage immediately before invocation; timer callbacks likewise remain in a scanned key-indexed table instead of being copied into opaque `DispatcherTimer` closures.
