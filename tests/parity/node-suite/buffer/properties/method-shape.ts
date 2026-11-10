import { Buffer } from "node:buffer";

const b = Buffer.from([1, 2]);
console.log("typeof readUInt8:", typeof b.readUInt8);
console.log("typeof writeUInt8:", typeof b.writeUInt8);
console.log("typeof includes:", typeof b.includes);
console.log("typeof hasOwnProperty:", typeof b.hasOwnProperty);
