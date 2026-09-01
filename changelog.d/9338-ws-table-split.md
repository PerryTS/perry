### Internal

- **Splits the WebSocket dispatch rows out of `net_events.rs`.** That file was
  at 1989 lines and #9335's `WebSocketServer` rows took it past the 2000-line
  gate. The `ws` block moves to `ws_events.rs`, assembled into
  `NATIVE_MODULE_TABLE` immediately before `NET_EVENTS_ROWS` so dispatch order
  is byte-for-byte what it was — the same shape as the earlier `tls_events.rs`
  split (#3196-#3200).
