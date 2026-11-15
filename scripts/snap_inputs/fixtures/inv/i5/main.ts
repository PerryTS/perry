import { held } from "./a.ts";
import { later } from "./b.ts";
gc();
console.log(held.length, held[7].name, later);
