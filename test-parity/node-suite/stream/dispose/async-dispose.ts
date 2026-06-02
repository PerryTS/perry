import { Readable } from "node:stream";
// Stream instances expose Symbol.asyncDispose (Node 21+) so they can be
// used with `await using`.
const r = new Readable({ read() {} });
console.log("has asyncDispose:", typeof (r as any)[Symbol.asyncDispose] === "function");
