// #3905: node:http2 server/client/handshake/sensitiveHeaders export surface.
import http2Default from "node:http2";
import * as http2 from "node:http2";
import {
  connect,
  createServer,
  performServerHandshake,
  sensitiveHeaders,
} from "node:http2";

console.log("connect:", typeof connect, connect.length);
console.log("createServer:", typeof createServer, createServer.length);
console.log("performServerHandshake:", typeof performServerHandshake);
console.log("sensitiveHeaders:", typeof sensitiveHeaders);
console.log("namespace connect:", typeof http2.connect);
console.log("default:", typeof http2Default);
console.log("default.connect:", typeof http2Default.connect);
console.log("default.createServer:", typeof http2Default.createServer);
