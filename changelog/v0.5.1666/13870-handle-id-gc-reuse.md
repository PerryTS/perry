A native handle id (an http request or response, a WebSocket, a database
statement, any perry-ffi payload) is reissued only after a full collection
proves that no JavaScript value and no WeakMap entry still names it. Before,
an id was recycled at the next event-loop tick, so on a keep-alive connection
a new request could be `===` an old request the program still held, and a
WeakMap keyed by the old request answered for the new one. The collection that
releases an id also deletes every WeakMap entry keyed by it, and a boxed
common-registry handle used as a WeakMap key is no longer dropped while its
handle is still alive. A WebSocket connect promise resolves with a handle
instead of a plain number id.
