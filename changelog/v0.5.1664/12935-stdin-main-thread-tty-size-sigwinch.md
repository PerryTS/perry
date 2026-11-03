Terminal UIs that run a Worker (opencode) no longer lose input, and they see the
real terminal size. A worker's event loop drained the process-global stdin
buffer and called the main thread's `data` listeners on the worker thread; fd 0
now belongs to the main thread, as in Node. A TTY `process.stdout`/`stderr`
carries `columns` and `rows` as own data properties, so reads through an alias or
parameter no longer return `undefined` (opentui drew 80x24 everywhere), and they
refresh on SIGWINCH. `process.on("SIGWINCH")` listeners now fire.
