import { StringDecoder } from "node:string_decoder";

const d = new StringDecoder("hex");
console.log("one:", d.write(Buffer.from([0x0a])));
console.log("two:", d.write(Buffer.from([0xbc, 0xde])));
console.log("end:", d.end());
