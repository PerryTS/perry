import http from "node:http";
import net from "node:net";

// Nothing but the teardown of the unref'd connection holds the loop once the
// response has ended, and `close(callback)` still calls back.
const server = http.createServer((_req, res) => {
  res.socket!.unref();
  console.log("close called");
  server.close(() => console.log("callback"));
  setTimeout(() => {
    console.log("response ends");
    res.end("done");
  }, 100);
});
server.on("close", () => console.log("close event"));
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as net.AddressInfo;
  const client = net.connect(port, "127.0.0.1", () => {
    client.write("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
  });
  client.on("data", () => {});
  client.unref();
});
