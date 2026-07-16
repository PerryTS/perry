import { run } from "node:test";

console.log("run:before");
let events = 0;
for await (const _event of run({ files: [] })) {
  events++;
}
console.log("run:after", events);
