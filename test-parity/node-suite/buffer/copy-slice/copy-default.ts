import { Buffer } from "node:buffer";

const src = Buffer.from("abcd");
const dst = Buffer.alloc(4);
console.log("copy ret:", src.copy(dst));
console.log("copy default:", dst.toString("utf8"));
