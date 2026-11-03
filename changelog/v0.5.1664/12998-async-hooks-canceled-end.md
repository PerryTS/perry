### Fix canceled socket end callback context

- Run canceled `Socket.end()` callbacks in the queued `TickObject` context, preserving its `AsyncLocalStorage` store.
- Use the actual end-callback dispatch array as the callback receiver.
- Preserve canceled write callbacks' `TCPWRAP` context; native shutdown completion and full write error/order parity remain separate work.
