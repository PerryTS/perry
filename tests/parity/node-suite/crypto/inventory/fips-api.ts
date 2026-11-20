import * as crypto from "node:crypto";
import { getFips, setFips } from "node:crypto";

console.log("getFips initial:", crypto.getFips());
console.log("getFips named:", getFips());
const ret = crypto.setFips(false);
console.log("setFips false return:", ret);
console.log("getFips after false:", crypto.getFips());
const ret2 = setFips(0 as any);
console.log("setFips named zero return:", ret2);
console.log("getFips after zero:", getFips());
console.log("getFips name:", crypto.getFips.name);
console.log("setFips name:", crypto.setFips.name);
