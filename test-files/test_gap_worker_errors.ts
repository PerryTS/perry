// An uncaught exception in a worker stops that worker only: the parent gets
// an 'error' event with the thrown value, then 'exit' with code 1, and keeps
// running. An unhandled rejection in a worker is treated the same way (Node's
// default), and so is a throw while the worker's module is loading.
import { Worker } from "node:worker_threads";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 20000);

function run(mode: string): Promise<void> {
    return new Promise((done) => {
        const w = new Worker(new URL("./_helpers/worker_errors_worker.ts", import.meta.url), { workerData: mode });
        w.on("message", (m: string) => {
            console.log(mode, "message", m);
            if (mode === "caught") w.terminate();
        });
        w.on("error", (e: any) => {
            if (e instanceof Error) console.log(mode, "error", e.name, JSON.stringify(e.message), "is Error");
            else console.log(mode, "error value", JSON.stringify(e));
        });
        w.on("exit", (code: number) => {
            console.log(mode, "exit", code);
            done();
        });
        w.postMessage("go");
    });
}

for (const mode of ["handler-throw", "handler-throw-string", "rejection", "async-handler", "init-throw", "caught"]) {
    await run(mode);
}
console.log("main still running");
clearTimeout(guard);
