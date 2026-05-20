import { constants, kMaxLength, kStringMaxLength } from "node:buffer";

console.log("constants types:", typeof constants.MAX_LENGTH, typeof constants.MAX_STRING_LENGTH);
console.log("top-level types:", typeof kMaxLength, typeof kStringMaxLength);
console.log("same max:", constants.MAX_LENGTH === kMaxLength);
console.log("same string max:", constants.MAX_STRING_LENGTH === kStringMaxLength);
