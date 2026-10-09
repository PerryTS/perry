`net.Socket` and `net.Server` are native payloads with a TransportCore, and
their completions route by link token through the payload cell. perry-ext-net
no longer keeps id-keyed socket or server tables (19 tables and 10 producers
in the native handle ledger, now 0 and 0), so a Socket that closes and is
dropped is collected and its WeakRefs clear, as in Node.

HTTP/1 server and client connections are `net.Socket` payloads. An HTTP
`upgrade` is a single store to the connection's route: the listener gets the
same Socket, with the bytes that arrived with the head as `head` and every
later byte as its own `data`. The upgrade listener's socket parameter is an
ordinary value; the compiler no longer tags it as a `ws` Client, which used to
register its `data` listener in a ws id table that never fired. A server with
an attached `WebSocketServer` still hands the listener a ws client handle,
which dispatches through ws's own handle methods.
