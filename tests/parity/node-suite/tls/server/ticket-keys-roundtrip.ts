import tls from "node:tls";

const server = tls.createServer();
const original = server.getTicketKeys();
const replacement = Buffer.alloc(48, 7);
console.log("original:", Buffer.isBuffer(original), original.length);
console.log("set return:", server.setTicketKeys(replacement) === undefined);
const actual = server.getTicketKeys();
console.log("roundtrip:", actual.equals(replacement), actual !== replacement);
replacement[0] = 9;
console.log("copied:", actual[0] === 7);
