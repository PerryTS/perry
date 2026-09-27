Fix HTTP response framing after `writeHead()`. Buffered `end(body)` calls now
retain committed framing instead of synthesizing `Content-Length`: HTTP/1.1
uses chunked encoding, including the terminating chunk for an empty body;
HTTP/1.0 closes the connection when no length was declared. Explicit lengths
and transfer encodings remain authoritative. Standalone `ServerResponse`
objects use the same framing rules as live server responses.

Regression coverage checks raw response bytes against Node, including implicit
headers, explicit framing, empty bodies, HEAD, 204/304, streaming, HTTP/1.0
keep-alive requests, and standalone responses.
