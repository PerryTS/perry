// Connections that come and go one after another: a descriptor number the host
// freed is handed out again, and nothing from the previous connection leaks
// into the next one (the client checks every echo).
import * as net from "node:net";

let closed = 0;
const server = net.createServer((socket) => {
  socket.on("data", (chunk) => socket.write(chunk));
  socket.on("end", () => socket.end());
  socket.on("close", () => {
    console.log("closed", ++closed);
    if (closed === 3) server.close();
  });
});
server.listen(0, () => console.log("listening"));
