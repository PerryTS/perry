import { Buffer } from "node:buffer";

const bytes = Buffer.from([0x41, 0x80, 0xff]);
console.log("latin1:", bytes.toString("latin1").split("").map(c => c.charCodeAt(0)).join(","));
console.log("ascii:", bytes.toString("ascii").split("").map(c => c.charCodeAt(0)).join(","));
console.log("latin1 from:", Buffer.from("A\u0080\u00ff", "latin1").toString("hex"));
