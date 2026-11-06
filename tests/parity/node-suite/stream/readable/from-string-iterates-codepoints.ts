import { Readable } from "node:stream";
// Documented Node behavior: Readable.from(string) treats the string as a single
// chunk (string-as-iterable shortcut). Validates whole-chunk emission.
const r = Readable.from("abc");
const chunks: string[] = [];
r.on("data", (c) => chunks.push(String(c)));
r.on("end", () => {
  console.log("chunk count:", chunks.length);
  console.log("first:", chunks[0]);
});
