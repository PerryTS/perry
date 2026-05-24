import { Readable } from "node:stream";
// push() returns true while the buffer is below HWM (and end not pushed).
const r = new Readable({ highWaterMark: 100, read() {} });
const a = r.push("aa");
const b = r.push("bb");
console.log("first:", a, "second:", b);
console.log("both true:", a === true && b === true);
