// A Worker whose file is not compiled into the binary constructs, then fails
// as its script would fail to load: an asynchronous `error` event
// (ERR_WORKER_NOT_COMPILED) and an exit, as Node reports a module it cannot
// find (MODULE_NOT_FOUND) and a Web Worker a script it cannot fetch. A
// malformed filename still throws from the constructor. The codes differ from
// Node's, so this test compares against stored expected output.
import { Worker } from "node:worker_threads";

const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
const name = ["not", "compiled", "worker"].join("-") + ".ts";
const cases: [string, () => any][] = [
    ["lexical, built at run time", () => new Worker(new URL("./" + name, import.meta.url))],
    ["namespace, built at run time", () => new ns.Worker(new URL("./" + name, import.meta.url))],
    ["namespace, absolute string", () => new ns.Worker("/" + name)],
    ["eval source", () => new ns.Worker("1 + 1", { eval: true })],
    ["bare relative string", () => new ns.Worker(name)],
    ["not a path", () => new ns.Worker(42)],
];
const results: string[] = [];
let pending = 0;
for (const [label, make] of cases) {
    try {
        const worker = make();
        pending++;
        let error = "none";
        worker.on("error", (e: any) => { error = `${e.name} ${e.code}`; });
        worker.on("exit", (code: number) => {
            results.push(`${label} -> constructed, error ${error}, exit ${code}`);
            if (--pending === 0) finish();
        });
    } catch (e: any) {
        results.push(`${label} -> threw ${e.name} ${e.code} ${e instanceof Error}`);
    }
}
console.log("constructors returned");
function finish() {
    for (const line of results.sort()) console.log(line);
    console.log("still running");
}
