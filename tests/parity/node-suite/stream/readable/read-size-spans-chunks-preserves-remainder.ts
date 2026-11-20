import { Readable } from "node:stream";

// read(N) may span queued chunks, but the unread suffix must remain queued
// as the next chunk instead of being merged with later pushes/unshifts.
const r = new Readable({ read() {} });
r.push("aa");
r.push("bb");

const first = r.read(3);
const second = r.read();

console.log("first:", first && first.toString("utf8"));
console.log("second:", second && second.toString("utf8"));
