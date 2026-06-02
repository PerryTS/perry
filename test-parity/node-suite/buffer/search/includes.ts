import { Buffer } from "node:buffer";

const b = Buffer.from("abcdef");
console.log("includes str:", b.includes("cd"));
console.log("includes offset:", b.includes("cd", 4));
console.log("includes buf:", b.includes(Buffer.from("ef")));
console.log("includes num:", b.includes(0x61));
