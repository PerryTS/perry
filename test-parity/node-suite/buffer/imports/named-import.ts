import { Buffer } from "node:buffer";

const b = Buffer.from("buffer");
console.log("named usable:", Buffer.isBuffer(b));
console.log("named hex:", b.toString("hex"));
console.log("named length:", b.length);
