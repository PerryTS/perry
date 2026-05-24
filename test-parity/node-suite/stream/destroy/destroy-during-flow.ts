import { Readable } from "node:stream";
// destroy() in the middle of flowing — outstanding data is dropped,
// 'data' stops, 'close' fires.
const r = new Readable({ read() {} });
let dataCount = 0;
let closed = false;
r.on("data", () => {
  dataCount++;
  if (dataCount === 1) r.destroy(); // destroy after first chunk
});
r.on("close", () => {
  closed = true;
  console.log("got chunks:", dataCount);
  console.log("closed:", closed);
});
r.push("a");
r.push("b"); // shouldn't be emitted as data
r.push(null);
