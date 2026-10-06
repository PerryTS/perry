import { Worker } from "node:worker_threads";
import { makeLiteral } from "./_helpers/regexp_literal_site.ts";
const a = makeLiteral(), b = makeLiteral();
console.log("literal_fresh", a === b, b.lastIndex);
a.test("aa");
console.log("literal_state", a.lastIndex, b.lastIndex, makeLiteral().lastIndex);
a.compile("b", "g");
console.log("literal_compile", a.source, b.source, makeLiteral().source);
const worker = new Worker(new URL("./_helpers/regexp_literal_worker.ts", import.meta.url));
await new Promise<void>((resolve) => {
    worker.on("message", (value: any) => {
        console.log("literal_worker", JSON.stringify(value));
        resolve();
    });
});
