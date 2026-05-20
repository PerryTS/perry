import { StringDecoder } from "string_decoder";

const dec = new StringDecoder("utf8");
console.log("prefixless write:", dec.write(Buffer.from([0xe2, 0x82, 0xac])));
