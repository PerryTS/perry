import { Readable } from "node:stream";
// Each registered 'data' listener receives the same chunk for every push.
const r = new Readable({ read() {} });
const a: string[] = [];
const b: string[] = [];
r.on("data", (c) => a.push(String(c)));
r.on("data", (c) => b.push(String(c)));
r.push("x");
r.push("y");
r.push(null);
r.on("end", () => {
  console.log("a:", a.join(","));
  console.log("b:", b.join(","));
  console.log("same:", a.join(",") === b.join(","));
});
