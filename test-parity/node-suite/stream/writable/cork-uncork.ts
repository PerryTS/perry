import { Writable } from "node:stream";
// cork()/uncork() buffer writes so the underlying _write sees them in one
// batch (cork holds; uncork flushes). Here we just verify the API + final
// joined output is unchanged.
const seen: string[] = [];
const w = new Writable({
  write(chunk, _e, cb) { seen.push(String(chunk)); cb(); },
});
w.on("finish", () => console.log("joined:", seen.join("|")));
w.cork();
w.write("a");
w.write("b");
w.uncork();
w.end("c");
