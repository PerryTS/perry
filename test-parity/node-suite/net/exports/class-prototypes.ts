import * as net from "node:net";
import { Duplex } from "node:stream";
import { EventEmitter } from "node:events";

const socket = new net.Socket();
const server = new net.Server();

console.log(
  "socket class:",
  socket instanceof net.Socket,
  socket instanceof Duplex,
);
console.log(
  "server class:",
  server instanceof net.Server,
  server instanceof EventEmitter,
);
console.log("socket constructor:", socket.constructor === net.Socket);
console.log("server constructor:", server.constructor === net.Server);
console.log("socket call:", net.Socket() instanceof net.Socket);
console.log("server call:", net.Server() instanceof net.Server);

socket.destroy();
server.close();
