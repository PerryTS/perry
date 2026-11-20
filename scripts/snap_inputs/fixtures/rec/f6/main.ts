// SNAPCUT2 F6 stale-holder: b holds the entry object E from cells that dead temporaries of a and c occupied before.
// Every held-pointer row is dated after b started (b holds E; a dead cell of c may hold a free-list link); no word is read twice.
import { a } from "./a.ts";
import { c } from "./c.ts";
(globalThis as any).E = { e: a + c };
await Promise.resolve();
await import("./b.ts");
(globalThis as any).E = 0;                      // the global object no longer holds E: b is its only holder
gc();
console.log("REC_F6");
