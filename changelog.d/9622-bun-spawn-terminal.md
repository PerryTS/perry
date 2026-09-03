# Bun subprocess and terminal support

The `bun` compatibility layer now implements `Bun.spawn` in array and object
forms, including consumable output streams, process lifecycle controls, stdio
file descriptors and `Bun.file` sinks, and structured spawn errors. On POSIX
targets, `Bun.Terminal` attaches subprocesses to a native PTY with data, exit,
drain, write, resize, raw-mode, ref/unref, close, and async-disposal support.
