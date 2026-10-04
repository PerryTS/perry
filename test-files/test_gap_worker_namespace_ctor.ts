// `new ns.Worker(...)` where `ns` is the worker_threads namespace reached as a
// value (`process.getBuiltinModule`) constructs a real Worker. The compiler
// cannot see that call site; the runtime finds the worker file in the table of
// worker entries compiled into the binary (here, from the static site below).
import { Worker } from "node:worker_threads";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 10000);

function run(label: string, make: () => Worker): Promise<void> {
    return new Promise((done) => {
        const w = make();
        console.log(label, "has on/postMessage/terminate:", typeof w.on, typeof w.postMessage, typeof w.terminate);
        console.log(label, "threadId > 0:", w.threadId > 0);
        w.once("message", (hello: unknown) => {
            console.log(label, "hello", JSON.stringify(hello));
            w.postMessage("ping");
            w.once("message", (reply: unknown) => {
                console.log(label, "reply", reply);
                w.terminate().then((code) => {
                    console.log(label, "terminate resolved", code);
                    done();
                });
            });
        });
    });
}

const ns = (globalThis as any).process.getBuiltinModule("node:worker_threads");
console.log("main isMainThread:", ns.isMainThread, "threadId:", ns.threadId);
const entry = () => new URL("./_helpers/worker_namespace_ctor_worker.ts", import.meta.url);

await run("static", () => new Worker(new URL("./_helpers/worker_namespace_ctor_worker.ts", import.meta.url), { workerData: "static" }));
await run("builtin", () => new ns.Worker(entry(), { workerData: { via: "getBuiltinModule" } }));
const Ctor = ns.Worker;
await run("stored", () => new Ctor(entry(), { workerData: [1, 2] }));
clearTimeout(guard);
