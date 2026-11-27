- **A paused `net.Socket` reads to its high-water mark.** An accepted socket
  starts reading before its `connection` listeners run, as Node's does, so
  pausing it there keeps the read going until the buffer reaches
  `readableHighWaterMark`: it sees the peer's EOF and emits `'end'` instead
  of hanging the process. A `pauseOnConnect` socket and a client paused
  before it connects read nothing until resumed. Once the buffer is full the
  read stops, and a stopped socket that owes no write no longer keeps the
  process alive; `resume()`, `read()`, a `'data'` or a `'readable'` listener
  restarts it.
