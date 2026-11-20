import { Readable } from "node:stream";
// In flowing mode, each push() generally produces its own 'data' event
// (chunks not concatenated unless buffered before listener attaches).
const r = new Readable({ read() {} });
const events: string[] = [];
r.on("data", (c) => events.push(String(c)));
r.on("end", () => {
  console.log("event count:", events.length);
  console.log("each event:", events.join(","));
});
r.push("a");
r.push("b");
r.push("c");
r.push(null);
