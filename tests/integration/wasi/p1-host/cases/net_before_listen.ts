// An outbound connection opened before the first listen(). The host does not
// announce its listener (it sits on descriptor 12, past the outbound socket's
// number), so the scan for stream sockets must not take the outbound socket for
// a listener.
import * as net from "node:net";

const out = net.connect(Number(process.env.PEER_PORT), "127.0.0.1");
out.on("connect", () => {
  console.log("connected");
  const server = net.createServer((socket) => {
    socket.on("data", (chunk) => socket.write(chunk));
    socket.on("end", () => {
      socket.end();
      server.close();
      out.write("ping");
    });
  });
  server.listen(0, () => console.log("listening"));
});
out.on("data", (chunk) => {
  console.log("got", String(chunk));
  out.end();
});
out.on("error", (error: any) => console.log("error", error.code));
out.on("close", () => console.log("closed"));
