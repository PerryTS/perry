// Upstream: Node v26.5.0 lib/dgram.js module.exports and ESM named-export bridge.
// Coverage added: exact enumerable exports beyond callable members.
import * as dgram from "node:dgram";

console.log("exports:", Object.keys(dgram).sort().join(","));
console.log("named types:", typeof dgram.Socket, typeof dgram.createSocket);
