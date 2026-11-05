import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("ascii");
const out = dec.write(Buffer.from([0x41, 0xc1, 0xff])) + dec.end();
console.log("ascii codes:", out.charCodeAt(0), out.charCodeAt(1), out.charCodeAt(2));
