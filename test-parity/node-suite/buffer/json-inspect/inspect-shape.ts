import { Buffer } from "node:buffer";

const b = Buffer.from([0, 1, 2, 255]);
console.log("toString default:", b.toString());
console.log("valueOf same:", b.valueOf() === b);
console.log("length:", b.length);
