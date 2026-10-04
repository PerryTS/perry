// A Worker whose file is not compiled into the binary throws
// ERR_WORKER_NOT_COMPILED synchronously from the constructor, and the program
// keeps running. Node instead emits an asynchronous `error` event
// (MODULE_NOT_FOUND), so this test compares against stored expected output.
import { Worker } from "node:worker_threads";

const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
const name = ["not", "compiled", "worker"].join("-") + ".ts";
const cases: [string, () => unknown][] = [
    ["lexical, built at run time", () => new Worker(new URL("./" + name, import.meta.url))],
    ["namespace, built at run time", () => new ns.Worker(new URL("./" + name, import.meta.url))],
    ["namespace, absolute string", () => new ns.Worker("/" + name)],
    ["bare relative string", () => new ns.Worker(name)],
    ["not a path", () => new ns.Worker(42)],
];
for (const [label, make] of cases) {
    try {
        make();
        console.log(label, "-> constructed");
    } catch (e: any) {
        console.log(label, "->", e.name, e.code, e instanceof Error);
    }
}
console.log("still running");
