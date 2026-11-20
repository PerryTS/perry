import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("latin1");
const out = dec.write(Buffer.from([0x41, 0xe9, 0xff])) + dec.end();
console.log("latin1 codes:", out.charCodeAt(0), out.charCodeAt(1), out.charCodeAt(2));
