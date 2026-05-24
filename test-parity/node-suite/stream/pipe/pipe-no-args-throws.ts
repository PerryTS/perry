import { Readable } from "node:stream";
// pipe() with no args throws TypeError (destination required).
const r = Readable.from(["x"]);
let caught: string | null = null;
try {
  (r as any).pipe();
} catch (e: any) {
  caught = e && e.name;
}
console.log("threw:", caught !== null);
console.log("name:", caught);
