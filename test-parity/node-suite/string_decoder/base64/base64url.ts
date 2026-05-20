import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("base64url");
console.log("one:", dec.write(Buffer.from([0x61])) + dec.end());
console.log("two:", dec.write(Buffer.from([0x61, 0x61])) + dec.end());
console.log("three:", dec.write(Buffer.from([0x61, 0x61, 0x61])) + dec.end());
