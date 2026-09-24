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

function describe(label: string, socket: any): string {
  const proto = Object.getPrototypeOf(socket);
  return [
    label,
    "null:", proto === null,
    "is Socket.prototype:", proto === net.Socket.prototype,
    "ctor is Socket:", proto !== null && proto.constructor === net.Socket,
    "instanceof:", socket instanceof net.Socket,
  ].join(" ");
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

// The accept callback and the client's connect callback race, so the
// server-side result is held and printed once both sides are done.
let serverSideLine = "server-side socket: never accepted";
const server = net.createServer((serverSide: any) => {
  serverSideLine = describe("server-side socket:", serverSide);
  serverSide.on("error", () => {});
  serverSide.on("close", () => {
    server.close(() => {
      console.log(serverSideLine);
      console.log("server closed");
    });
  });
});

server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const client: any = net.connect(port, "127.0.0.1", () => {
    console.log(describe("client socket:", client));
    console.log("cjs Socket.prototype:", Object.getPrototypeOf(client) === netCjs.Socket.prototype);
    client.on("error", () => {});
    client.on("close", () => console.log("client closed, destroyed:", client.destroyed));
    try {
      destroyLikeUndici(client, new Error("boom"));
      console.log("destroyLikeUndici returned");
    } catch (e: any) {
      console.log("destroyLikeUndici threw:", e.message);
    }
  });
});

// An unconnected socket, asked BEFORE anything has read
// `net.Socket.prototype`: the prototype object is created on first access,
// so this is the order that exercises that path.
console.log(describe("new net.Socket():", new net.Socket()));
