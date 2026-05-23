import { Readable } from "node:stream";
// A Readable emits 'end' once after the final chunk is consumed.
let endFires = 0;
const r = Readable.from(["x"]);
r.on("data", () => {});
r.on("end", () => {
  endFires++;
  console.log("end fired:", endFires);
});
