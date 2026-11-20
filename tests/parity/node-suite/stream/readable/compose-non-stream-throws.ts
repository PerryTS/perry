import { compose } from "node:stream";
// compose(string) accepts strings as iterable readable sources.
let caught: string | null = null;
try {
  (compose as any)("just a string");
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
