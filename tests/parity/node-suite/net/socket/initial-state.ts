import { Socket } from "node:net";

const socket = new Socket();

console.log(
  "state:",
  socket.connecting,
  socket.pending,
  socket.destroyed,
  socket.readyState,
);
console.log("stream:", socket.readable, socket.writable, socket.allowHalfOpen);
console.log("bytes:", socket.bytesRead, socket.bytesWritten, socket.bufferSize);
console.log(
  "address:",
  socket.address(),
  socket.localAddress,
  socket.remoteAddress,
);
console.log("refs:", typeof (socket as any).hasRef);

socket.destroy();
