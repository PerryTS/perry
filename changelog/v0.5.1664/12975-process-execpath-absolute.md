Fix `process.execPath` and `process.argv[0]` to report the running executable's absolute path, resolving symlinks on Unix. Keep the original invocation spelling in `process.argv0`, and use the same executable identity for child-process and cluster self-launches.

Allow direct `process.send()` and `process.disconnect()` calls to reach the existing IPC methods, including the TypeError when no IPC channel exists.
