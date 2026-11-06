import { Readable } from "node:stream";
// destroy() in the middle of flowing rejects future pushes but still lets
// chunks already queued before the close event drain.
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
r.push("b"); // queued before close, so it still emits as data
r.push(null);
