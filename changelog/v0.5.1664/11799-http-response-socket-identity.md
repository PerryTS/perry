- HTTP/1 `ServerResponse.socket` and `.connection` now reference the accepted
  connection socket shared with `req.socket` and the server's `connection`
  event. The response keeps that socket identity if `req.socket` is reassigned.
