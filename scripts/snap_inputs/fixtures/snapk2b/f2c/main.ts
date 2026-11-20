import { box } from "./e.ts";
const tag = process.env.SNAPK2B_ENTRY;
// (c): an entry birth stored as a property value of the eager object,
// landing in its spill storage.
box.held = { tag: "entry" };
gc();
console.log("K2B_F2C", box.held.tag, tag ?? "-");
