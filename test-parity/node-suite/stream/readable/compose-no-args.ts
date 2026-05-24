import { compose } from "node:stream";
// compose() with no args — should throw TypeError per docs.
let caught: string | null = null;
try {
  (compose as any)();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
