// A client that leaves mid-response closes the response (#15172): 'aborted',
// the response's 'close', the socket's 'close', then the request's 'close',
// and later writes report a destroyed response.
import http from "node:http";
import net from "node:net";

const server = http.createServer((req, res) => {
  console.log("request");
  req.on("aborted", () => console.log("req 'aborted'"));
  req.on("close", () => console.log("req 'close'"));
  req.socket.on("close", () => console.log("socket 'close'"));
  res.on("close", () => console.log("res 'close'"));
  res.writeHead(200, { "content-type": "text/event-stream" });
  let n = 0;
  const timer = setInterval(() => {
    n += 1;
    const ok = res.write(`data: ${n}\n\n`);
    if (n === 5) {
      console.log(`write #${n} returned ${ok}, destroyed=${res.destroyed}, writableEnded=${res.writableEnded}`);
      clearInterval(timer);
      res.end();
      server.close();
    }
  }, 20);
});

server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as { port: number };
  const client = net.connect(port, "127.0.0.1", () => {
    client.write("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
  });
  client.once("data", () => {
    console.log("client destroys its socket");
    client.destroy();
  });
});
