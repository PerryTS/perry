// CJL step 1(a): an ESM `export let` read by an importer is a live binding.
// The counter is mutated after the import; the importer must see each write.
import { count, bump } from "./cjl_live/counter.ts";

console.log("before:", count);
bump();
console.log("after one:", count);
bump();
bump();
console.log("after three:", count);
