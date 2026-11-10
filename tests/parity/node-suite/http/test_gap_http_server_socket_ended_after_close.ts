// A server connection that answers `Connection: close` is closed by the HTTP
// layer itself (node: `socket.destroySoon()`), which ends the socket's write
// side. The socket must report that (`writableEnded` / not `writable`), and an
// application's own `socket.end()` afterwards (a server ending the connection
// from its 'finish' handler) is a no-op instead of a second shutdown that could
// fail and destroy the socket with an unhandled 'error'.
// A raw client keeps the connection open, so the server socket can be observed
// as soon as the client sees the server's FIN.
import http from "node:http";
import net from "node:net";

let serverSocket: any;
const serverErrors: string[] = [];
const server = http.createServer((req, res) => {
  serverSocket = req.socket;
  serverSocket.on("error", (e: any) => serverErrors.push(String(e && e.code)));
  res.end("ok");
});

server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const client = net.connect(port, "127.0.0.1");
  let reply = "";
  client.setEncoding("utf8");
  client.on("data", (chunk) => (reply += chunk));
  client.on("end", () => {
    console.log("reply ends with body:", reply.endsWith("ok"));
    console.log("server socket writableEnded:", serverSocket.writableEnded);
    console.log("server socket writable:", serverSocket.writable);
    serverSocket.end();
    console.log("second end() returned");
  });
  client.on("close", () => {
    console.log("server socket errors:", serverErrors.length);
    server.close();
    process.exit(0);
  });
  client.write("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
});
