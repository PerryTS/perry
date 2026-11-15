// Positive control: the entry stores one of its own objects in a dependency's module-level variable. That is a hold of an entry value by imaged state (rule c) and must stay reported.
import { hold } from "./d.ts";
hold({ k: Math.random() });
await Promise.resolve();
await import("./b.ts");
gc();
console.log("REC_F8");
