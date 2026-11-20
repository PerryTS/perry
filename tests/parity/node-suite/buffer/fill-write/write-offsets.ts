import { Buffer } from "node:buffer";

const b = Buffer.alloc(6);
console.log("write ret 1:", b.write("ab", 2));
console.log("after first:", b.toString("hex"));
console.log("write ret 2:", b.write("cd", 4, "utf8"));
console.log("after second:", b.toString("hex"));
