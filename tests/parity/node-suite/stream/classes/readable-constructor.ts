import { Readable } from "node:stream";
// new Readable({ read }) constructs a readable stream instance.
const r = new Readable({ read() {} });
console.log("instance:", r instanceof Readable);
console.log("readable flag:", r.readable);
console.log("push fn:", typeof r.push === "function");
console.log("read fn:", typeof r.read === "function");
console.log("pipe fn:", typeof r.pipe === "function");
