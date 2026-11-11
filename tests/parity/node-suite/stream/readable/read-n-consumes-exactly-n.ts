import { Readable } from "node:stream";
// read(N) consumes exactly N bytes (when available) from the front of
// the buffer; subsequent reads consume the next N bytes.
const r = new Readable({ read() {} });
r.push("hello");
const first = r.read(2);
const second = r.read(2);
const third = r.read();
console.log("first:", first && first.toString("utf8"));
console.log("second:", second && second.toString("utf8"));
console.log("third:", third && third.toString("utf8"));
