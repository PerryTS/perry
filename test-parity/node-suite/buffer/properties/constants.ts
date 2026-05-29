import { constants, kMaxLength, kStringMaxLength } from "node:buffer";

console.log("constants types:", typeof constants.MAX_LENGTH, typeof constants.MAX_STRING_LENGTH);
console.log("top-level types:", typeof kMaxLength, typeof kStringMaxLength);
console.log("max length:", constants.MAX_LENGTH);
console.log("top max length:", kMaxLength);
console.log("same max:", constants.MAX_LENGTH === kMaxLength);
console.log("string max:", constants.MAX_STRING_LENGTH);
console.log("top string max:", kStringMaxLength);
console.log("same string max:", constants.MAX_STRING_LENGTH === kStringMaxLength);
console.log("keys:", Object.keys(constants).sort().join(","));
