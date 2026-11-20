Fixed `net` listeners running with the wrong `this`, and `'close'` being lost
after `destroy()` (#11227).

Node calls every EventEmitter listener with `this` bound to the emitter.
perry-ext-net's event pump did that only for `'connect'`, `'drain'` and
`'readable'`. Everything else ran `function` listeners with
`this === undefined`: socket `'data'`, `'end'`, `'close'` and `'error'`, the
TLS identity `'error'`, server `'listening'`, `'connection'`, `'close'`,
`'error'` and `'drop'`, and user calls to `socket.emit()`. undici's
`onHttpSocketClose` begins with `this[kParser]`, so `Client.close()` and
`Pool.close()` threw. Every dispatch now binds the socket or server handle as
the implicit receiver through a `ListenerThis` guard. The guard restores the
caller's receiver on drop, including when a listener unwinds, and keeps that
receiver in a transient root.

`destroy()` marked the socket destroyed straight away, but the driver reports
the close later. With nothing else pending, the keepalive gate saw no active
handle and the process exited in between, so `'close'` never fired. Node
emits it on a nextTick whatever the socket's ref state. undici's
`Client.close()` awaits that event, so it never settled either. A socket
whose close has been submitted now keeps the loop alive until its `'close'`
is delivered.

Also in this area:

- `net.Server` gains `prependListener` and `prependOnceListener`, on both the
  dynamic and the typed dispatch path. Before, those calls registered
  nothing.
- Sockets now emit `'ready'` right after `'connect'`, as Node does.

Gap tests: `test-files/test_gap_net_listener_this_binding.ts` and
`test-files/test_gap_net_destroy_close_before_exit.ts`.
