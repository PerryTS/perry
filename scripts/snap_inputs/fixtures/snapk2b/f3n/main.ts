// dispatch shape (dispatch.ts:8,14): the entry copies argv, an imported
// function compares the copy's first element with two exported sentinels,
// and the entry reaches a.ts only when neither matches.
import { isHost } from "./dispatch.ts";
const argv = process.argv.slice(2);
if (!isHost(argv)) {
  await import("./a.ts");
}
// The census boundary (snap25 fixtures mark it the same way).
gc();
console.log("K2B_F3N", (globalThis as any).seen ?? "none");
