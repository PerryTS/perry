import { Buffer } from "node:buffer";

for (const value of [NaN, Infinity, -Infinity, -0, 1.5]) {
  const b = Buffer.alloc(8);
  b.writeDoubleLE(value, 0);
  const read = b.readDoubleLE(0);
  console.log("double:", String(value), Number.isNaN(read) ? "NaN" : Object.is(read, -0) ? "-0" : String(read));
}
