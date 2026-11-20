// Two concurrent connections to an echo server, over the standard socket calls
// only: each client half-closes and the server answers with its own end.
import * as net from "node:net";

let closed = 0;
const server = net.createServer((socket) => {
  socket.on("data", (chunk) => socket.write(chunk));
  socket.on("end", () => socket.end());
  socket.on("close", () => {
    console.log("closed");
    if (++closed === 2) server.close();
  });
});
server.listen(0, () => console.log("listening"));
