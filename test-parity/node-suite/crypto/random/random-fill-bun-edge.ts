import { randomFill, randomFillSync } from "node:crypto";

function randomFillAsync(buf: any, offset?: number, size?: number): Promise<[Error | null, any]> {
  return new Promise((resolve) => {
    if (offset === undefined) {
      randomFill(buf, (err, filled) => resolve([err, filled]));
    } else if (size === undefined) {
      randomFill(buf, offset, (err, filled) => resolve([err, filled]));
    } else {
      randomFill(buf, offset, size, (err, filled) => resolve([err, filled]));
    }
  });
}

const zero = new Uint8Array(0);
const [zeroErr, zeroFilled] = await randomFillAsync(zero);
console.log("zero err null:", zeroErr === null);
console.log("zero same:", zeroFilled === zero);
console.log("zero len:", zero.length);

const f64 = new Float64Array(10);
const [f64Err, f64Filled] = await randomFillAsync(f64, 2);
console.log("f64 err null:", f64Err === null);
console.log("f64 same:", f64Filled === f64);
console.log("f64 prefix zero:", f64[0] === 0 && f64[1] === 0);
console.log("f64 completed:", f64.length === 10);

const u16 = new Uint16Array(4);
const ret = randomFillSync(u16, 1, 2);
console.log("u16 same:", ret === u16);
console.log("u16 prefix zero:", u16[0] === 0);
console.log("u16 suffix zero:", u16[3] === 0);
console.log("u16 middle maybe filled:", u16.length === 4);
