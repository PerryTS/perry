import { Socket } from "node:net";

const socket = new Socket();

console.log(
  "no delay:",
  socket.setNoDelay() === socket,
  socket.setNoDelay(false) === socket,
);
console.log(
  "keep alive:",
  socket.setKeepAlive(true, 1000) === socket,
  socket.setKeepAlive(false) === socket,
);
console.log("timeout:", socket.setTimeout(0) === socket);
console.log("flow:", socket.pause() === socket, socket.resume() === socket);
console.log("refs:", socket.unref() === socket, socket.ref() === socket);
console.log("cork:", socket.cork(), socket.uncork());

socket.destroy();
