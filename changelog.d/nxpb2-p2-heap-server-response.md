http server: a `ServerResponse` is now an ordinary object born on node's
ServerResponse layout, with its native state in the payload slot, and the
server's pending exchanges are owned by the server instead of process
globals. The response carries node's head fields itself: `statusCode` set as
a property is the status the implicit head writes (a program that replaced
`writeHead`, as `compression`'s on-headers does, now sees 201 rather than
200), and `_header` is stored when the head is encoded, so `end(body)`
without an explicit head still sends `Content-Length`. `finish` and `close`
are emitted through the response's own `emit`. HTTP/2 responses keep the
registry until the next step.
