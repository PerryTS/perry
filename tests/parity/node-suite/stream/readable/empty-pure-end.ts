import { Readable } from "node:stream";
// A Readable that pushes null immediately fires 'end' without any 'data'.
let dataCount = 0;
let endFired = false;
const r = new Readable({
  read() {
    this.push(null);
  },
});
r.on("data", () => dataCount++);
r.on("end", () => {
  endFired = true;
  console.log("data count:", dataCount);
  console.log("end fired:", endFired);
});
