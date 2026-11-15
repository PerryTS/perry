// SNAPREC2 F1: a program with top-level await; two static dependencies settle EVALUATED, no suspension.
import "./a.ts";
import "./b.ts";
await 0;
gc();
console.log("REC_F1");
