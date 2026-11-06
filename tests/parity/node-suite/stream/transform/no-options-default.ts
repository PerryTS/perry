import { Transform } from "node:stream";
// new Transform() with no options uses the default _transform that pushes
// each chunk through unchanged (PassThrough-like).
const t = new Transform();
const out: string[] = [];
t.on("data", (c) => out.push(String(c)));
t.on("end", () => console.log("joined:", out.join("")));
t.write("a");
t.write("b");
t.end();
