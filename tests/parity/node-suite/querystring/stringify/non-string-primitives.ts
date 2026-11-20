import { stringify } from "node:querystring";

console.log("number bool:", stringify({ n: 42, ok: true, no: false }));
console.log("nan inf:", stringify({ nan: NaN, inf: Infinity, negInf: -Infinity }));
console.log("object date:", stringify({ o: { x: 1 }, d: new Date(0) }));
