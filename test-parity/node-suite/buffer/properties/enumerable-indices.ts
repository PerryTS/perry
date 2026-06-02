import { Buffer } from "node:buffer";

const b = Buffer.from([5, 6]);
console.log("has 0:", b.hasOwnProperty("0"));
console.log("has 2:", b.hasOwnProperty("2"));
console.log("enum 1:", b.propertyIsEnumerable("1"));
