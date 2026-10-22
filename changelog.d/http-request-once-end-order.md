A server request (`http.IncomingMessage`) delivers its body to every listener
order Node supports. `req.once(...)` was missing from the request's dynamic
method surface (`typeof req.once` was `"undefined"`, the call a silent no-op),
and an `'end'` listener registered before the `'data'` listener fired at once,
before the body. effect's `NodeStream.toArrayBuffer` registers
`once("error")`, `once("end")`, then `on("data")`, so every opencode request
that reads a JSON body (`POST /api/session`, `POST /api/pty`, ...) never got a
response. `'end'` now waits for the buffered body to reach `'data'`, and
`once` registers a self-removing listener for both events.
