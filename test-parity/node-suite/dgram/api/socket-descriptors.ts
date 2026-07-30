// Upstream: Node v26.5.0 lib/dgram.js Socket prototype and type setup.
// Coverage added: public method and socket type descriptors.
import * as dgram from "node:dgram";

function descriptor(value: object, key: PropertyKey): string {
  const item = Object.getOwnPropertyDescriptor(value, key);
  return item
    ? `${item.writable}:${item.enumerable}:${item.configurable}`
    : "missing";
}

const socket = dgram.createSocket("udp4");

try {
  console.log("type descriptor:", socket.type, descriptor(socket, "type"));
  console.log(
    "bind descriptor:",
    descriptor(dgram.Socket.prototype, "bind"),
    Object.hasOwn(socket, "bind"),
  );
} finally {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
}
