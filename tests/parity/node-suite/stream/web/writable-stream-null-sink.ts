import { WritableStream } from "node:stream/web";
// new WritableStream(null) — typically constructs with default sink, but
// Node may throw or coerce. Test the actual behavior.
let constructed = false;
let threw: string | null = null;
try {
  const ws = new WritableStream(null as any);
  constructed = ws instanceof WritableStream;
} catch (e: any) {
  threw = e && e.name;
}
console.log("constructed:", constructed);
console.log("threw:", threw);
