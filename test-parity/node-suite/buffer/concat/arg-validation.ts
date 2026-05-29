import { Buffer } from "node:buffer";

// #2013: Buffer.concat validates the list, its elements, and totalLength.
for (const value of ["x", 5, null, {}] as any[]) {
  try {
    Buffer.concat(value);
    console.log("concat", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("concat", String(value), "=>", err.name, err.code);
  }
}

// Non-Buffer element.
try {
  Buffer.concat([1] as any);
  console.log("concat([1]) => NO THROW");
} catch (err: any) {
  console.log("concat([1]) =>", err.name, err.code);
}

// totalLength validation.
for (const len of ["x", -1, 2.5, NaN] as any[]) {
  try {
    Buffer.concat([Buffer.from("ab")], len);
    console.log("concat len", String(len), "=> NO THROW");
  } catch (err: any) {
    console.log("concat len", String(len), "=>", err.name, err.code);
  }
}

// Valid concat (with and without totalLength) still works.
console.log("concat:", Buffer.concat([Buffer.from("ab"), Buffer.from("c")]).toString());
console.log("concat+len:", Buffer.concat([Buffer.from("ab"), Buffer.from("cd")], 3).toString());
console.log("concat([]):", Buffer.concat([]).length);
