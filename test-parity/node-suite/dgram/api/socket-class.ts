// Upstream: Node v26.5.0 lib/dgram.js Socket constructor and prototype setup.
// Coverage added: construction and prototype identity.
import * as dgram from "node:dgram";
import { EventEmitter } from "node:events";

async function bindAndClose(socket: dgram.Socket): Promise<void> {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
}

const created = dgram.createSocket("udp4");
const constructed = new dgram.Socket("udp4");

try {
  console.log(
    "created class:",
    created instanceof dgram.Socket,
    created instanceof EventEmitter,
    Object.getPrototypeOf(created) === dgram.Socket.prototype,
  );
  console.log(
    "constructed class:",
    constructed instanceof dgram.Socket,
    constructed.type,
  );
  console.log(
    "constructors:",
    created.constructor === dgram.Socket,
    dgram.Socket.prototype.constructor === dgram.Socket,
  );
} finally {
  await Promise.all([bindAndClose(created), bindAndClose(constructed)]);
}
