import { Readable } from "node:stream";
// Instance method extracted and called — `this` no longer set; should throw.
const r = Readable.from(["a"]);
const detachedPush = r.push;
let caught: string | null = null;
try {
  detachedPush("x");
} catch (e: any) {
  caught = e && e.name;
}
console.log("detached push threw:", caught !== null);
