import { StringDecoder } from "node:string_decoder";

const ctorProto = StringDecoder.prototype;
const dec = new StringDecoder("utf8");
const proto = Object.getPrototypeOf(dec);

console.log("ctor:", typeof StringDecoder, StringDecoder.name);
console.log("ctor proto typeof:", typeof ctorProto);
console.log("ctor proto names:", Object.getOwnPropertyNames(ctorProto).join(","));
console.log("instance ctor:", dec?.constructor?.name);
console.log("own keys:", Object.keys(dec).join(","));
console.log("own names:", Object.getOwnPropertyNames(dec).join(","));
console.log("proto keys:", Object.getOwnPropertyNames(proto).join(","));
console.log("proto same:", proto === StringDecoder.prototype);
console.log("ctor proto write:", typeof StringDecoder.prototype.write);
console.log("typeof write:", typeof dec.write);
console.log("typeof end:", typeof dec.end);
console.log("write result:", dec.write(Buffer.from([0xe2, 0x82])));
console.log("end result:", dec.end(Buffer.from([0xac])));
