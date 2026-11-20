import { Buffer } from "node:buffer";

const b = Buffer.alloc(0);
console.log("zero len:", b.length);
console.log("zero hex:", b.toString("hex"));
console.log("zero isBuffer:", Buffer.isBuffer(b));
