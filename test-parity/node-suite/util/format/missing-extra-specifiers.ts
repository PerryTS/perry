import { format } from "node:util";

console.log(format("missing:%s:%d:%j"));
console.log(format("extra", "a", 1, { b: 2 }));
console.log(format("unknown:%q", "x"));
