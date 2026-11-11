import { Buffer } from "node:buffer";

const b = Buffer.from([0, 1, 2, 255]);
// Print bytes via hex to avoid coupling the test to UTF-8 replacement-char
// policy for the lone 0xFF (WTF-8 lone-surrogate handling is a known gap).
console.log("toString hex:", b.toString("hex"));
console.log("valueOf same:", b.valueOf() === b);
console.log("length:", b.length);
