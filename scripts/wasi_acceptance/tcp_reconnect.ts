import * as net from "node:net";
import { collect } from "perry/gc";

// Reuse one socket/owner cell across transport incarnations. Each deadline
// fires once, then close retires its resource before the next connect. GC
// runs both while the deadline owns the cell and during its callback.
let timeouts = 0;
const server = net.createServer(peer => peer.on("end", () => peer.end()));
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const socket = new net.Socket();
  socket.on("connect", () => { socket.setTimeout(10); collect(); });
  socket.on("timeout", () => {
    collect();
    console.log("timeout", ++timeouts);
    socket.destroy();
  });
  socket.on("close", () => {
    if (timeouts < 2) socket.connect(port, "127.0.0.1");
    else server.close();
  });
  socket.connect(port, "127.0.0.1");
});
