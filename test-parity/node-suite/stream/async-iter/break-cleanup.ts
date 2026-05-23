import { Readable } from "node:stream";
// Breaking out of `for await (chunk of readable)` destroys the stream by
// default (destroyOnReturn: true).
const r = Readable.from(["a", "b", "c"]);
for await (const chunk of r) {
  console.log("chunk:", chunk);
  break;
}
console.log("destroyed:", r.destroyed);
