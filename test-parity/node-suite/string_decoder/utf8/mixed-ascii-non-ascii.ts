import { StringDecoder } from "node:string_decoder";

const input = Buffer.from([0xcb, 0xa4, 0x64, 0xe1, 0x8b, 0xa4, 0x30, 0xe3, 0x81, 0x85]);
const dec = new StringDecoder("utf8");
console.log("mixed:", JSON.stringify(dec.write(input) + dec.end()));
