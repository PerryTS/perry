import { Readable } from "node:stream";
// on(event, non-function) — should throw TypeError per EE spec.
const r = new Readable({ read() {} });
let caught: string | null = null;
try {
  r.on("data", "not-a-function" as any);
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
