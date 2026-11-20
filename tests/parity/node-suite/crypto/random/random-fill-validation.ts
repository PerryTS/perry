import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", value);
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code");
  }
}

probe("offset negative", () => crypto.randomFillSync(Buffer.alloc(4), -1).length);
probe("offset too large", () => crypto.randomFillSync(Buffer.alloc(4), 5).length);
probe("size beyond end", () => crypto.randomFillSync(Buffer.alloc(4), 1, 10).length);
probe("invalid buffer", () => crypto.randomFillSync(123 as any));

const fractional = Buffer.alloc(4);
const fractionalRet = crypto.randomFillSync(fractional, 1.5, 2.5);
console.log("fractional valid:", fractionalRet === fractional, fractionalRet.length);

const ab = new ArrayBuffer(4);
const abRet = crypto.randomFillSync(ab);
console.log("arraybuffer valid:", abRet === ab, abRet.byteLength);

const u16 = new Uint16Array(4);
const u16Ret = crypto.randomFillSync(u16, 1, 2);
console.log("typedarray valid:", u16Ret === u16, u16Ret.byteLength);

const captured = crypto.randomFillSync;
probe("captured offset too large", () => captured(Buffer.alloc(4), 5).length);
