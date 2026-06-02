import { format } from "node:util";
// Regression: a previous string-replace-based %j compactor mangled
// strings containing ", ", ": ", "{ ", or " }".
console.log(format("%j", { msg: "a, b: c" }));
console.log(format("%j", { k: "x { y } z" }));
console.log(format("%j", { a: 1, b: "v" }));
console.log(format("%j", [1, "x, y", { n: 2 }]));
