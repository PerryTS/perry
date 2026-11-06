import { Buffer } from "node:buffer";

const b = Buffer.from("abcdefabc");
console.log("str:", b.indexOf("bc"));
console.log("str offset:", b.indexOf("bc", 3));
console.log("buf:", b.indexOf(Buffer.from("fa")));
console.log("num:", b.indexOf(0x64));
