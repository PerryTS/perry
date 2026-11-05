// One poll with everything live: the listener, an accepted connection, the
// signal stream, a timer and stdout. USR2 is handled, TERM ends the program.
import * as net from "node:net";

let ticks = 0;
const server = net.createServer((socket) => {
  socket.on("data", (chunk) => socket.write(chunk));
  socket.on("end", () => socket.end());
  socket.on("close", () => console.log("connection closed"));
});
const timer = setInterval(() => ticks++, 25);
process.on("SIGUSR2", () => console.log("usr2"));
process.on("SIGTERM", () => {
  console.log("term");
  console.log("ticks", ticks >= 20 ? "ok" : `too few (${ticks})`);
  clearInterval(timer);
  server.close();
});
server.listen(0, () => console.log("listening"));
