import * as net from "node:net";

const server = net.createServer();

console.log("server.address() before listen:", server.address());
console.log("server.address typeof:", typeof server.address);
console.log("server.close typeof:", typeof server.close);
console.log("server.getConnections typeof:", typeof server.getConnections);
console.log("server.listen typeof:", typeof server.listen);
console.log("server.ref typeof:", typeof server.ref);
console.log("server.unref typeof:", typeof server.unref);

const serverListen = server.listen;
console.log("server.listen alias typeof:", typeof serverListen);

const socket = new net.Socket();

console.log("socket.destroyed property:", socket.destroyed);
console.log("socket.address typeof:", typeof socket.address);
console.log("socket.connect typeof:", typeof socket.connect);
console.log("socket.destroy typeof:", typeof socket.destroy);
console.log("socket.destroySoon typeof:", typeof socket.destroySoon);
console.log("socket.end typeof:", typeof socket.end);
console.log("socket.pause typeof:", typeof socket.pause);
console.log("socket.ref typeof:", typeof socket.ref);
console.log("socket.resetAndDestroy typeof:", typeof socket.resetAndDestroy);
console.log("socket.resume typeof:", typeof socket.resume);
console.log("socket.setEncoding typeof:", typeof socket.setEncoding);
console.log("socket.setKeepAlive typeof:", typeof socket.setKeepAlive);
console.log("socket.setNoDelay typeof:", typeof socket.setNoDelay);
console.log("socket.setTimeout typeof:", typeof socket.setTimeout);
console.log("socket.unref typeof:", typeof socket.unref);
console.log("socket.write typeof:", typeof socket.write);

const socketWrite = socket.write;
console.log("socket.write alias typeof:", typeof socketWrite);
