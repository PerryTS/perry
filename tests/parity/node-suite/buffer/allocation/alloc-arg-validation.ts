import { Buffer } from "node:buffer";

// #2013: Buffer.alloc / allocUnsafe / allocUnsafeSlow reject bad `size`
// arguments with Node's ERR_INVALID_ARG_TYPE / ERR_OUT_OF_RANGE.
const bad: any[] = ["x", true, null, {}, -1, NaN, Infinity, -Infinity];

for (const value of bad) {
  try {
    Buffer.alloc(value);
    console.log("alloc", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("alloc", String(value), "=>", err.name, err.code);
  }
}

for (const value of ["x", true, -1] as any[]) {
  try {
    Buffer.allocUnsafe(value);
    console.log("allocUnsafe", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("allocUnsafe", String(value), "=>", err.name, err.code);
  }
}

for (const value of ["x", -1] as any[]) {
  try {
    Buffer.allocUnsafeSlow(value);
    console.log("allocUnsafeSlow", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("allocUnsafeSlow", String(value), "=>", err.name, err.code);
  }
}

// Valid sizes still work (non-integers truncate toward zero).
console.log("alloc(3).length", Buffer.alloc(3).length);
console.log("alloc(2.9).length", Buffer.alloc(2.9).length);
console.log("allocUnsafe(4).length", Buffer.allocUnsafe(4).length);
