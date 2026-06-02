import { StringDecoder } from "node:string_decoder";

const bytes = new Uint8Array([0xe2, 0x82, 0xac]);
const dec1 = new StringDecoder("utf8");
console.log("uint8array:", dec1.write(bytes) + dec1.end());

const view = new DataView(bytes.buffer);
const dec2 = new StringDecoder("utf8");
console.log("dataview:", dec2.write(view) + dec2.end());
