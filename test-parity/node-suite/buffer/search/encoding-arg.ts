import { Buffer } from "node:buffer";

const b = Buffer.from("48656c6c6f20576f726c64", "hex");
console.log("index hex:", b.indexOf("576f", 0, "hex"));
console.log("includes hex:", b.includes("576f", 0, "hex"));
console.log("last hex:", b.lastIndexOf("6c", b.length, "hex"));
console.log("index utf8:", b.indexOf("World", 0, "utf8"));
