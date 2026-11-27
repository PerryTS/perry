- **A paused `net.Socket` reads to its high-water mark.** A socket paused
  before it starts flowing now reads until its buffer reaches
  `readableHighWaterMark`, as Node's does, so it sees the peer's EOF and
  emits `'end'` instead of hanging the process. Once the buffer is full the
  read stops, and a stopped socket that owes no write no longer keeps the
  process alive; `resume()`, `read()` or a `'data'` listener restarts it.
