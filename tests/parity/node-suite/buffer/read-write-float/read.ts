import { Buffer } from "node:buffer";

const b = Buffer.alloc(16);
b.writeFloatBE(1.5, 0);
b.writeFloatLE(1.5, 4);
b.writeDoubleBE(1.5, 8);
console.log("floatBE:", b.readFloatBE(0));
console.log("floatLE:", b.readFloatLE(4));
console.log("doubleBE:", b.readDoubleBE(8));
