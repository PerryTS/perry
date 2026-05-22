import { StringDecoder } from "node:string_decoder";

const d = new StringDecoder("base64");
console.log("invalid bytes:", d.write(Buffer.from([255, 254, 253])) + d.end());
const d2 = new StringDecoder("base64");
console.log("split one byte:", d2.write(Buffer.from([1])) + "|" + d2.end());
