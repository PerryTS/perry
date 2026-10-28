// The host does not announce its listener, which sits on descriptor 12 behind
// a gap: the scan for stream sockets must look past the unused numbers.
import * as net from "node:net";

const server = net.createServer((socket) => {
  socket.on("data", (chunk) => socket.write(chunk));
  socket.on("end", () => {
    socket.end();
    server.close();
  });
});
server.listen(0, () => console.log("listening"));
