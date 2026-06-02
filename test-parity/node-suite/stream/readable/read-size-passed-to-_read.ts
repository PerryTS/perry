import { Readable } from "node:stream";
// User _read(size) implementations receive the size hint from the caller.
// Test that it's always passed as a positive integer.
let sizesSeen: number[] = [];
const r = new Readable({
  read(size: number) {
    sizesSeen.push(size);
    this.push("x");
    this.push(null);
  },
});
r.on("data", () => {});
r.on("end", () => {
  console.log("count:", sizesSeen.length);
  console.log("first size is number:", typeof sizesSeen[0] === "number");
  console.log("first size > 0:", sizesSeen[0] > 0);
});
