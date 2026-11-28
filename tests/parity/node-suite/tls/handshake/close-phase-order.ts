// TLS sockets destroyed in one turn close newest first in the close-callbacks
// phase, with each 'close' listener's ticks before the next 'close'.
import * as tls from "node:tls";

const log: string[] = [];
const sockets = ["A", "B", "C"].map((label) => {
  const socket = tls.connect(1, "localhost");
  socket.on("error", () => {});
  socket.on("close", () => {
    log.push(`${label} close`);
    process.nextTick(() => log.push(`${label} tick`));
  });
  return socket;
});
sockets.forEach((socket) => socket.destroy());
setTimeout(() => console.log(log.join(", ")), 50);
