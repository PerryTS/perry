import { Readable } from "node:stream";
// Many push() calls accumulate; final readableLength = sum.
const r = new Readable({ read() {} });
let totalBytes = 0;
for (let i = 0; i < 10; i++) {
  const s = "x".repeat(i + 1);
  r.push(s);
  totalBytes += s.length;
}
console.log("expected:", totalBytes);
console.log("readableLength:", r.readableLength);
console.log("match:", r.readableLength === totalBytes);
