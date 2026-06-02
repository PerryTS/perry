import { Buffer } from "node:buffer";

const all = Buffer.from("ffffffffffffffff", "hex");
const high = Buffer.from("8000000000000000", "hex");
const le = Buffer.from("0000000000000080", "hex");
console.log("all be:", all.readBigUInt64BE(0).toString());
console.log("high be:", high.readBigUInt64BE(0).toString());
console.log("high le:", le.readBigUInt64LE(0).toString());
console.log("signed be:", all.readBigInt64BE(0).toString());
