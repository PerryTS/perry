import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("utf8");
console.log("first:", JSON.stringify(dec.write(Buffer.from([0xe2]))));
console.log("end:", JSON.stringify(dec.end()));
console.log("after:", JSON.stringify(dec.write(Buffer.from("a")) + dec.end()));
