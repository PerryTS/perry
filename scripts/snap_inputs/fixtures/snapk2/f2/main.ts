import { box } from "./eager.ts";
(globalThis as any).k = 41;
const j0 = (globalThis as any).j;
// (c): an entry birth stored into an imaged object.
box.x = { tag: "entry" };
await import("./a.ts");
await import("./b.ts");
console.log("K2A_F2", j0, (globalThis as any).j, (globalThis as any).fromA);
