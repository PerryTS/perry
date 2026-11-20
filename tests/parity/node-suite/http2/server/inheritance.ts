import { createServer } from "node:http2";
import { Server } from "node:net";

const server = createServer();
console.log("constructor:", server.constructor.name);
console.log("net server:", server instanceof Server);
console.log("listen:", typeof server.listen);
console.log("close:", typeof server.close);
