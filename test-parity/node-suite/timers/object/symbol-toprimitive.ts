import { setTimeout, clearTimeout } from "node:timers";
// Coercion via Number()/String()/default hints. Node returns a numeric
// id-like value; Perry uses its own handle id. We assert only on type
// shapes and finite-ness so the test is portable.
const timeout = setTimeout(() => {}, 50);
const asNum = Number(timeout);
console.log("Number type:", typeof asNum);
console.log("Number finite:", Number.isFinite(asNum));
const asStr = String(timeout);
console.log("String type:", typeof asStr);
console.log("String length > 0:", asStr.length > 0);
clearTimeout(timeout);
