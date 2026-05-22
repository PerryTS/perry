import { StringDecoder } from "node:string_decoder";

const d = new StringDecoder("utf8");
console.log("partial:", JSON.stringify(d.write(Buffer.from([0xe2, 0x82]))));
console.log("end1:", JSON.stringify(d.end()));
console.log("end2:", JSON.stringify(d.end()));
