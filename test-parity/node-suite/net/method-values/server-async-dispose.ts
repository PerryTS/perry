import * as net from "node:net";

const server = new net.Server();

console.log("asyncDispose typeof:", typeof server[Symbol.asyncDispose]);
console.log("listening before:", (server as any)["listening"]);

const result = server[Symbol.asyncDispose]();
console.log("asyncDispose result then:", typeof result?.then);

await result;
console.log("asyncDispose resolved:", (server as any)["listening"], (server as any).closed);
