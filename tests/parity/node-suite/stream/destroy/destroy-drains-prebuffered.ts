import { Readable } from "node:stream";

// Chunks accepted before destroy() remain readable until close is emitted.
const r = new Readable({ read() {} });
let dataCount = 0;
r.on("data", () => {
  dataCount++;
  if (dataCount === 1) r.destroy();
});
r.on("close", () => console.log("queued chunks:", dataCount));

for (const chunk of ["a", "b", "c", "d", "e"]) {
  r.push(chunk);
}
r.push(null);
