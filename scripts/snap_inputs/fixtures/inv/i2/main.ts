import { err, bytes, re } from "./a.ts";
import { later } from "./b.ts";
gc();
console.log(typeof err, bytes.length, re.lastIndex, later);
