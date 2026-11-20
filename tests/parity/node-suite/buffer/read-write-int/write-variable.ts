import { Buffer } from "node:buffer";

const b = Buffer.alloc(8);
console.log("writeUIntBE ret:", b.writeUIntBE(0x123456, 0, 3));
console.log("writeUIntLE ret:", b.writeUIntLE(0xabcdef, 3, 3));
b.writeIntBE(-2, 6, 2);
console.log("variable hex:", b.toString("hex"));
