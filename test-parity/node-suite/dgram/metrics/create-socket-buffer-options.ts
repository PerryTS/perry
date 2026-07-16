import * as dgram from "node:dgram";

const socket = dgram.createSocket({
  type: "udp4",
  recvBufferSize: 10_000,
  sendBufferSize: 15_000,
});
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));

const recvSize = socket.getRecvBufferSize();
const sendSize = socket.getSendBufferSize();
console.log("receive option applied:", recvSize === 10_000 || recvSize === 20_000);
console.log("send option applied:", sendSize === 15_000 || sendSize === 30_000);

await new Promise<void>((resolve) => socket.close(resolve));
