import { StringDecoder } from "node:string_decoder";

const d = new StringDecoder("utf8");
console.log("w1:", JSON.stringify(d.write(Buffer.from([0xe2]))));
console.log("w2:", JSON.stringify(d.write(Buffer.from([0x82]))));
console.log("w3:", JSON.stringify(d.write(Buffer.from([0xac]))));
console.log("end:", JSON.stringify(d.end()));
