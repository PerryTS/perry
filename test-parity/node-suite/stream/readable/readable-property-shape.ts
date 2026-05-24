import { Readable } from "node:stream";
// Stream instance should expose .pipe, .destroy, .read methods.
const r = new Readable({ read() {} });
const props = ["pipe", "destroy", "read", "pause", "resume", "unpipe", "wrap"];
for (const p of props) {
  console.log(`${p}:`, typeof (r as any)[p]);
}
console.log("all function:", props.every(p => typeof (r as any)[p] === "function"));
