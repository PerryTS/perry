import { cachedDataVersionTag } from "node:v8";

const first = cachedDataVersionTag();
const second = cachedDataVersionTag();
console.log("type:", typeof first);
console.log(
  "uint32:",
  Number.isInteger(first),
  first >= 0,
  first <= 0xffffffff,
);
console.log("stable:", first === second);
