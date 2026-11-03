**fix(stdlib): pause process.stdin when closing a readline interface (#9594)**

Closing a stdin-backed readline interface now pauses the shared
`process.stdin` stream, matching Node. Previously `rl.close()` fired the
interface's close callback but left Perry's background stdin reader flowing,
so bytes written afterwards still reached `process.stdin` `data` listeners and
could keep a CLI alive unexpectedly.

Interface close is now tracked separately from physical stdin EOF. An explicit
`process.stdin.resume()` can therefore restore delivery after close, a later
real EOF still reaches stdin's `end` / `close` listeners, and constructing a
new readline interface resumes stdin as Node's constructor does.
