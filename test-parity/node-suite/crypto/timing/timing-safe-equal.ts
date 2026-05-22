import * as crypto from "node:crypto";
import { timingSafeEqual } from "node:crypto";
import { Buffer } from "node:buffer";

const a = Buffer.from("0123456789abcdef", "hex");
const b = Buffer.from("0123456789abcdef", "hex");
const c = Buffer.from("0123456789abcdee", "hex");
console.log("equal:", crypto.timingSafeEqual(a, b));
console.log("diff:", crypto.timingSafeEqual(a, c));
console.log("named:", timingSafeEqual(new Uint8Array([1, 2, 3]), new Uint8Array([1, 2, 3])));
