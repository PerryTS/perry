import * as dgram from "node:dgram";

function codeOf(fn) {
  try {
    fn();
    return "none";
  } catch (error) {
    return error.code;
  }
}

function describe(value) {
  if (value === undefined) return "undefined";
  if (value === null) return "null";
  return `${typeof value}:${value}`;
}

function positiveNumber(value) {
  return typeof value === "number" && value > 0;
}

const unbound = dgram.createSocket("udp4");
console.log("unbound recv buffer:", codeOf(() => unbound.getRecvBufferSize()));
console.log("unbound send buffer:", codeOf(() => unbound.getSendBufferSize()));
console.log("unbound set broadcast:", codeOf(() => unbound.setBroadcast(true)));
console.log("unbound set ttl:", codeOf(() => unbound.setTTL(64)));
console.log("unbound set recv buffer:", codeOf(() => unbound.setRecvBufferSize(65536)));
console.log("unbound queue:", unbound.getSendQueueSize(), unbound.getSendQueueCount());
console.log("ref identity:", unbound.ref() === unbound);
console.log("unref identity:", unbound.unref() === unbound);
unbound.close();

const socket = dgram.createSocket({ type: "udp4", reuseAddr: true });
await new Promise((resolve) => {
  socket.bind(0, "0.0.0.0", () => resolve());
});

console.log("setBroadcast:", describe(socket.setBroadcast(true)));
console.log("setTTL:", describe(socket.setTTL(64)));
console.log("setMulticastTTL:", describe(socket.setMulticastTTL(32)));
console.log("setMulticastLoopback:", describe(socket.setMulticastLoopback(false)));
console.log("setMulticastInterface:", describe(socket.setMulticastInterface("0.0.0.0")));
console.log("setRecvBufferSize:", describe(socket.setRecvBufferSize(65536)));
console.log("setSendBufferSize:", describe(socket.setSendBufferSize(65536)));
console.log(
  "bound buffer sizes:",
  positiveNumber(socket.getRecvBufferSize()),
  positiveNumber(socket.getSendBufferSize()),
);
console.log("bad ttl type:", codeOf(() => socket.setTTL("64")));
console.log("bad ttl range:", codeOf(() => socket.setTTL(0)));
console.log("bad buffer:", codeOf(() => socket.setRecvBufferSize(-1)));
console.log("bad interface type:", codeOf(() => socket.setMulticastInterface(1)));
console.log("bad interface value:", codeOf(() => socket.setMulticastInterface("")));
socket.close();
