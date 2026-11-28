- **`'lookup'` events from the system resolver.** A socket connecting to a
  hostname now emits `'lookup'` with each resolved address (or the first of a
  pinned family) before it connects, and with the error when resolution
  fails, as in Node. HTTP and WebSocket client sockets get the same events.
