// SNAPREC2 F3: sub imports t (top-level await) then u: t settles ASYNC, the cut is before t, u is not imaged.
await 0;
await import("./sub.ts");
gc();
console.log("REC_F3");
