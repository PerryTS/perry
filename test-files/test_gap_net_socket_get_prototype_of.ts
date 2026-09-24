// `Object.getPrototypeOf(socket)` for a live `net.Socket` answered `null`,
// although `socket instanceof net.Socket` was already `true`. A socket is a
// registry handle, and the prototype resolution for handles only knew about
// fetch values, StringDecoder and X509Certificate.
//
// undici 8.9.0 hits this on every socket error path. `util.destroy(socket,
// err)` in `lib/core/util.js` runs
//
//   if (Object.getPrototypeOf(stream).constructor === IncomingMessage) { ... }
//
// which threw "Cannot read properties of null (reading 'constructor')" and
// replaced the real error (#11046).

import * as net from "node:net";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const netCjs = require("net");

function describe(label: string, socket: any) {
  const proto = Object.getPrototypeOf(socket);
  console.log(
    label,
    "null:", proto === null,
    "is Socket.prototype:", proto === net.Socket.prototype,
    "ctor is Socket:", proto !== null && proto.constructor === net.Socket,
    "instanceof:", socket instanceof net.Socket,
  );
}

// undici's `util.destroy` shape, verbatim apart from the constructor compared.
class NotASocket {}
function destroyLikeUndici(stream: any, err: Error) {
  if (typeof stream.destroy === "function") {
    if (Object.getPrototypeOf(stream).constructor === NotASocket) {
      stream.socket = null;
    }
    stream.destroy(err);
  }
}

const server = net.createServer((serverSide: any) => {
  describe("server-side socket:", serverSide);
  serverSide.on("error", () => {});
  serverSide.on("close", () => {
    server.close(() => console.log("server closed"));
  });
});

server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const client: any = net.connect(port, "127.0.0.1", () => {
    describe("client socket:", client);
    console.log("cjs Socket.prototype:", Object.getPrototypeOf(client) === netCjs.Socket.prototype);
    client.on("error", (e: Error) => console.log("client error:", e.message));
    client.on("close", () => console.log("client closed, destroyed:", client.destroyed));
    try {
      destroyLikeUndici(client, new Error("boom"));
      console.log("destroyLikeUndici returned");
    } catch (e: any) {
      console.log("destroyLikeUndici threw:", e.message);
    }
  });
});

// Control: an unconnected socket built with `new net.Socket()`.
describe("new net.Socket():", new net.Socket());
