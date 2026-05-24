import { Readable } from "node:stream";
// Iterator that immediately returns {done:true} — empty stream, end fires.
const emptyIter = {
  [Symbol.iterator]() {
    return {
      next() {
        return { value: undefined, done: true };
      },
    };
  },
};
const r = Readable.from(emptyIter as any);
let dataCount = 0;
let endFired = false;
r.on("data", () => dataCount++);
r.on("end", () => {
  endFired = true;
  console.log("data count:", dataCount);
  console.log("end fired:", endFired);
});
