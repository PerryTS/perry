import { randomFillSync, getRandomValues } from "node:crypto";

const u32 = new Uint32Array(4);
const ret = randomFillSync(u32, 1, 2);
console.log("same u32:", ret === u32);
console.log("u32 len:", u32.length);
console.log("u32 offset call completed:", u32.length === 4);

const u8 = new Uint8Array(4);
getRandomValues(u8);
console.log("getRandomValues same length:", u8.length);
