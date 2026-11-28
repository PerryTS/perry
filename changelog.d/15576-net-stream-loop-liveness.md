- **A `net.Socket` keeps the process alive as Node's does.** A stream holds
  the event loop while it reads or listens, which `unref()` gives up, or
  while it owes a connect, write, shutdown or close, whatever its ref. A
  connected socket that never started reading (paused before it connected,
  or a `pauseOnConnect` socket) no longer keeps the process alive, and an
  `unref()`'d socket stays alive until its pending write is flushed.
