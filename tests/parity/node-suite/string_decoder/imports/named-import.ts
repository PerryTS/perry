import { StringDecoder } from "node:string_decoder";

const dec = new StringDecoder("utf8");
console.log("instance typeof:", typeof dec);
console.log("write typeof:", typeof dec.write);
console.log("end typeof:", typeof dec.end);
console.log("write:", dec.write(Buffer.from("ok")));
