import { Buffer, isAscii, isUtf8 } from "node:buffer";

console.log("ascii ok:", isAscii(Buffer.from("hello")));
console.log("ascii high:", isAscii(Buffer.from([0x80])));
console.log("utf8 ok:", isUtf8(Buffer.from("hé")));
console.log("utf8 bad:", isUtf8(Buffer.from([0xff])));
console.log("uint8 ascii:", isAscii(new Uint8Array([0x41, 0x7f])));
console.log("arraybuffer utf8:", isUtf8(new Uint8Array([0x68, 0x69]).buffer));
