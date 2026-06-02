import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
console.log("negative start:", b.slice(-3).toString("utf8"));
console.log("negative end:", b.slice(1, -1).toString("utf8"));
console.log("beyond end:", b.slice(4, 99).toString("utf8"));
console.log("inverted:", b.slice(5, 2).length);
