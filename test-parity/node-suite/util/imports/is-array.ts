import * as util from "node:util";
import { isArray } from "node:util";

process.noDeprecation = true;

const keys = Object.keys(util);

console.log("enumerable:", keys.includes("isArray"));
console.log("type:", typeof util.isArray);
console.log("length:", util.isArray.length);
console.log("array:", util.isArray([1, 2]));
console.log("object:", util.isArray({ length: 2 }));
console.log("named type:", typeof isArray);
console.log("named length:", isArray.length);
console.log("named array:", isArray([]));
