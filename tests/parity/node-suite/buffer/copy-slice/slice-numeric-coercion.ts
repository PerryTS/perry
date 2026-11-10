import { Buffer } from "node:buffer";

const b = Buffer.from("0123456789");
console.log("infinity start length:", b.slice(Infinity).length);
console.log("fractional range:", b.slice(1.5, 4.8).toString("utf8"));
console.log("negative zero:", b.slice(-0.5).toString("utf8"));
