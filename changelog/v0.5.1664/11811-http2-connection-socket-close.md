Fix HTTP/2 server connection sockets to emit one `close` event when their underlying transport shuts down, including graceful and destroyed connections.
