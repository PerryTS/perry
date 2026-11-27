An HTTP/2 server session's `close(cb)` callback removes only its own `'close'` listener, as Node's `once` does: the same function also added with `on('close', cb)` stays registered.
