import { Readable } from "node:stream";
// unshift() during flowing mode — chunk re-enters at the front of the queue.
const r = new Readable({ read() {} });
const out: string[] = [];
let unshifted = false;
r.on("data", (c) => {
  const s = String(c);
  out.push(s);
  if (s === "second" && !unshifted) {
    unshifted = true;
    r.unshift("PUT-BACK");
  }
});
r.on("end", () => console.log("out:", out.join(",")));
r.push("first");
r.push("second");
r.push("third");
r.push(null);
