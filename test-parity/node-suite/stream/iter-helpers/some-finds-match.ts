import { Readable } from "node:stream";
// some(fn) returns true on first match; stops early.
let calls = 0;
const r = Readable.from([1, 2, 3, 4, 5]);
const result = await (r as any).some((x: number) => {
  calls++;
  return x === 3;
});
console.log("result:", result);
console.log("stopped early (calls <= 3):", calls <= 3);
