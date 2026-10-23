// An uncaught exception in a worker stops that worker only: the parent gets
// an 'error' event with the thrown value, then 'exit' with code 1, and keeps
// running. An unhandled rejection in a worker is treated the same way (Node's
// default), and so is a throw while the worker's module is loading.
import { Worker } from "node:worker_threads";

const guard = setTimeout(() => {
    console.log("timeout");
    process.exit(2);
}, 20000);

// A worker's 'message' events and its 'error' event travel on different
// channels, so their relative order is not part of Node's contract (Node
// itself delivers the error first in a fair share of runs). What is
// guaranteed is that 'exit' comes last. Collect the events and report them
// when the worker exits: messages in order, then the error, then the exit.
function run(mode: string): Promise<void> {
    return new Promise((done) => {
        const w = new Worker(new URL("./_helpers/worker_errors_worker.ts", import.meta.url), { workerData: mode });
        const lines: string[] = [];
        let error = "";
        w.on("message", (m: string) => {
            lines.push(mode + " message " + m);
            if (mode === "caught" && m.startsWith("caught")) w.terminate();
        });
        w.on("error", (e: any) => {
            if (e instanceof Error) error = mode + " error " + e.name + " " + JSON.stringify(e.message) + " is Error";
            else error = mode + " error value " + JSON.stringify(e);
        });
        w.on("exit", (code: number) => {
            if (error) lines.push(error);
            lines.push(mode + " exit " + code);
            console.log(lines.join("\n"));
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
