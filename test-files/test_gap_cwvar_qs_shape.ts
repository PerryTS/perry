// Closed World: a CommonJS module-level `var` table assigned once at module
// init (qs utils' hexTable) is proven assigned before any reader runs.
import lib from "./cwvar/qs_shape.cjs";

console.log(lib.encode("a b&c=d[0]"));
console.log(lib.encode("x~y_z.-!*'()"));
console.log(lib.encode("café € ß"));
console.log(lib.at(0), lib.at(32), lib.at(255));
console.log(String(lib.at(256)), String(lib.at(-1)), String(lib.at(1.5)));
console.log(lib.size(), lib.limit, lib.kind());
let n = 0;
for (let i = 0; i < 2000; i++) n += lib.encode("k" + (i % 7) + "=v w").length;
console.log("total", n);
