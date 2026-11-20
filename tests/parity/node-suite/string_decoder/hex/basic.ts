import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("hex");
console.log("hex:", dec.write(Buffer.from([0x00, 0x0f, 0xa0, 0xff])) + dec.end());
