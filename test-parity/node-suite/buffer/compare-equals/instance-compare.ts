import { Buffer } from "node:buffer";

const a = Buffer.from("abc");
const b = Buffer.from("abd");
console.log("compare:", a.compare(b));
console.log("equals false:", a.equals(b));
console.log("equals true:", a.equals(Buffer.from("abc")));
