import { constants } from "node:zlib";

console.log("ZLIB_VERNUM typeof:", typeof constants.ZLIB_VERNUM);
console.log(
  "ZLIB_VERNUM positive integer:",
  Number.isInteger(constants.ZLIB_VERNUM) && constants.ZLIB_VERNUM > 0,
);
console.log("Z_TREES:", constants.Z_TREES);
